//! Borrowed access to checked prototype records and compact initialization inputs.
use crate::reference::WorldFields;
use crate::reference::{
    ReferenceError, SourceId, Transform, WorldInitField, WorldReference, WorldValue,
};
use bevy_platform::collections::HashMap;
#[derive(Clone, Copy)]
pub enum Fields<'a> {
    Record(&'a WorldFields),
    Compact(&'a [WorldInitField]),
}
pub trait FieldAccess<'a>: Copy {
    fn get(self, name: &str) -> Option<&'a WorldValue>;
}
impl<'a> FieldAccess<'a> for Fields<'a> {
    fn get(self, name: &str) -> Option<&'a WorldValue> {
        match self {
            Self::Record(f) => f.get(name),
            Self::Compact(f) => f.iter().find(|f| f.name == name).map(|f| &f.value),
        }
    }
}
impl<'a> FieldAccess<'a> for &Fields<'a> {
    fn get(self, name: &str) -> Option<&'a WorldValue> {
        (*self).get(name)
    }
}
impl<'a> FieldAccess<'a> for &'a WorldFields {
    fn get(self, name: &str) -> Option<&'a WorldValue> {
        WorldFields::get(self, name)
    }
}
#[derive(Clone, Copy)]
pub struct SourceRecord<'a> {
    pub location: RecordLocation,
    pub source: &'a SourceId,
    pub class_name: &'a str,
    pub fields: Fields<'a>,
    pub execution_order: i32,
}
/// A location in the immutable checked graph owned by one stream.
#[derive(Clone, Copy)]
pub enum RecordLocation {
    Compact(usize),
    Full(usize, i32),
}
impl RecordLocation {
    pub fn record(self, reference: &WorldReference) -> SourceRecord<'_> {
        match self {
            Self::Compact(index) => {
                let r = &reference.initialization.components[index];
                SourceRecord {
                    location: self,
                    source: &r.source,
                    class_name: &r.class_name,
                    fields: Fields::Compact(&r.fields),
                    execution_order: r.execution_order,
                }
            }
            Self::Full(index, execution_order) => {
                let r = &reference.records[index];
                SourceRecord {
                    location: self,
                    source: &r.source,
                    class_name: &r.class_name,
                    fields: Fields::Record(&r.fields),
                    execution_order,
                }
            }
        }
    }
}
pub struct SourceNode<'a> {
    pub owner: &'a SourceId,
    pub parent: Option<&'a SourceId>,
    pub pose: Transform,
    pub active: bool,
}
pub struct SourceRules {
    indices: Vec<(bool, u32)>,
    /// Interned source file names of the three maps below.
    files: HashMap<String, u32>,
    /// Source to its `indices` entry. Duplicate sources keep the entry that a binary search finds.
    records: HashMap<(u32, i64), (bool, u32)>,
    /// Node source and node owner to the node index.
    nodes: HashMap<(u32, i64), u32>,
    owners: HashMap<(u32, i64), u32>,
    radii: HashMap<SourceId, f64>,
}
/// Sorted positions to a map from source to the value at the position that a binary search finds.
fn source_map<'r>(
    files: &mut HashMap<String, u32>,
    sorted: &[u32],
    source: impl Fn(u32) -> &'r SourceId,
) -> HashMap<(u32, i64), u32> {
    let mut map = HashMap::with_capacity(sorted.len());
    for (i, &entry) in sorted.iter().enumerate() {
        let id = source(entry);
        if i > 0 && source(sorted[i - 1]) == id {
            continue;
        }
        let found = sorted
            .binary_search_by(|e| source_order(source(*e), id))
            .expect("a sorted source finds itself");
        let next = files.len() as u32;
        let file = *files.entry(id.file.clone()).or_insert(next);
        map.insert((file, id.path_id), sorted[found]);
    }
    map
}
impl SourceRules {
    pub fn new(reference: &WorldReference) -> Self {
        let mut indices: Vec<_> = (0..reference.records.len())
            .map(|i| (false, i as u32))
            .chain((0..reference.initialization.components.len()).map(|i| (true, i as u32)))
            .collect();
        indices.sort_unstable_by(|a, b| {
            source_order(
                Self::indexed_source(reference, *a),
                Self::indexed_source(reference, *b),
            )
        });
        let nodes = &reference.initialization.nodes;
        let mut by_source: Vec<_> = (0..nodes.len() as u32).collect();
        by_source.sort_unstable_by(|a, b| {
            source_order(&nodes[*a as usize].source, &nodes[*b as usize].source)
        });
        let mut by_owner = by_source.clone();
        by_owner.sort_unstable_by(|a, b| {
            source_order(&nodes[*a as usize].owner, &nodes[*b as usize].owner)
        });
        let mut files = HashMap::new();
        let positions: Vec<u32> = (0..indices.len() as u32).collect();
        let records = source_map(&mut files, &positions, |i| {
            Self::indexed_source(reference, indices[i as usize])
        })
        .into_iter()
        .map(|(key, i)| (key, indices[i as usize]))
        .collect();
        let node_map = source_map(&mut files, &by_source, |i| &nodes[i as usize].source);
        let owners = source_map(&mut files, &by_owner, |i| &nodes[i as usize].owner);
        Self {
            indices,
            files,
            records,
            nodes: node_map,
            owners,
            radii: super::placement::prototype_radii(reference),
        }
    }
    fn indexed_source(reference: &WorldReference, (compact, index): (bool, u32)) -> &SourceId {
        if compact {
            &reference.initialization.components[index as usize].source
        } else {
            &reference.records[index as usize].source
        }
    }
    fn key(&self, source: &SourceId) -> Option<(u32, i64)> {
        Some((*self.files.get(source.file.as_str())?, source.path_id))
    }
    fn index(&self, source: &SourceId) -> Option<(bool, usize)> {
        let &(compact, index) = self.records.get(&self.key(source)?)?;
        Some((compact, index as usize))
    }
    fn node_index(&self, source: &SourceId) -> Option<usize> {
        Some(*self.nodes.get(&self.key(source)?)? as usize)
    }
    /// Preserve full-record LOD ownership when compact and full sources share an identity.
    pub fn full_record_owner<'a>(
        &self,
        reference: &'a WorldReference,
        source: &SourceId,
    ) -> Option<&'a SourceId> {
        let first = self.indices.partition_point(|entry| {
            source_order(Self::indexed_source(reference, *entry), source).is_lt()
        });
        let (_, index) = self.indices[first..]
            .iter()
            .take_while(|entry| Self::indexed_source(reference, **entry) == source)
            .find(|(compact, _)| !compact)?;
        match reference.records[*index as usize]
            .fields
            .get("m_GameObject")
        {
            Some(WorldValue::Link(owner)) => Some(owner),
            _ => None,
        }
    }
    pub fn radius(&self, source: &SourceId) -> Option<f64> {
        self.radii.get(source).copied()
    }
    pub fn get<'a>(
        &self,
        reference: &'a WorldReference,
        source: &SourceId,
    ) -> Result<SourceRecord<'a>, ReferenceError> {
        let (compact, i) = self.index(source).ok_or_else(|| {
            ReferenceError(format!(
                "Missing source rule {}:{}",
                source.file, source.path_id
            ))
        })?;
        if compact {
            return Ok(RecordLocation::Compact(i).record(reference));
        }
        let r = &reference.records[i];
        let order = if let Some(WorldValue::Link(script)) = r.fields.get("m_Script") {
            self.index(script)
                .filter(|(c, _)| !*c)
                .and_then(|(_, i)| reference.records[i].fields.get("m_ExecutionOrder"))
                .and_then(|v| {
                    if let WorldValue::Integer(n) = v {
                        Some(*n as i32)
                    } else {
                        None
                    }
                })
                .unwrap_or(0)
        } else {
            0
        };
        Ok(RecordLocation::Full(i, order).record(reference))
    }
    pub fn node<'a>(
        &self,
        reference: &'a WorldReference,
        source: &SourceId,
    ) -> Result<SourceNode<'a>, ReferenceError> {
        if let Some(i) = self.node_index(source) {
            let n = &reference.initialization.nodes[i];
            return Ok(SourceNode {
                owner: &n.owner,
                parent: n.parent.as_ref(),
                pose: n.pose,
                active: n.active,
            });
        }
        let r = self.get(reference, source)?;
        let f = r.fields;
        let q = fields(value(f, "m_LocalRotation")?)?;
        let p = vector(f, "m_LocalPosition")?;
        let owner = link(f, "m_GameObject")?;
        let go = self.get(reference, owner)?;
        Ok(SourceNode {
            owner,
            parent: optional(f, "m_Father")?,
            active: flag(go.fields, "m_IsActive")?,
            pose: Transform {
                translation: [p[0], p[1], -p[2]],
                rotation: [
                    -number(q, "x")?,
                    -number(q, "y")?,
                    number(q, "z")?,
                    number(q, "w")?,
                ],
                scale: vector(f, "m_LocalScale")?,
            },
        })
    }
    pub fn root<'a>(
        &'a self,
        reference: &'a WorldReference,
        object: &SourceId,
    ) -> Result<&'a SourceId, ReferenceError> {
        if let Some(i) = self.node_index(object) {
            return Ok(&reference.initialization.nodes[i].source);
        }
        if let Some(i) = self.key(object).and_then(|key| self.owners.get(&key)) {
            return Ok(&reference.initialization.nodes[*i as usize].source);
        }
        let r = self.get(reference, object)?;
        if matches!(r.class_name, "Transform" | "RectTransform") {
            return Ok(r.source);
        }
        if let Some((true, i)) = self.index(object) {
            return self.root(reference, &reference.initialization.components[i].owner);
        }
        let owner = if r.class_name == "GameObject" {
            r.source
        } else {
            link(r.fields, "m_GameObject")?
        };
        let go = self.get(reference, owner)?;
        for pointer in array(go.fields, "m_Component")? {
            let target = link(fields(pointer)?, "component")?;
            if let Ok(r) = self.get(reference, target)
                && matches!(r.class_name, "Transform" | "RectTransform")
            {
                return Ok(r.source);
            }
        }
        Err(ReferenceError("Source object has no transform".into()))
    }
    pub fn component<'a>(
        &'a self,
        reference: &'a WorldReference,
        object: &SourceId,
        class: &str,
    ) -> Result<Option<SourceRecord<'a>>, ReferenceError> {
        let root = self.root(reference, object)?;
        if class == "Transform" {
            return Ok(None);
        }
        if let Some(i) = self.node_index(root) {
            for source in &reference.initialization.nodes[i].components {
                let r = self.get(reference, source)?;
                if r.class_name == class {
                    return Ok(Some(r));
                }
            }
        } else {
            let n = self.node(reference, root)?;
            let go = self.get(reference, n.owner)?;
            for pointer in array(go.fields, "m_Component")? {
                let target = link(fields(pointer)?, "component")?;
                if let Ok(r) = self.get(reference, target)
                    && r.class_name == class
                {
                    return Ok(Some(r));
                }
            }
        }
        Ok(None)
    }
    pub fn start_components<'a>(
        &'a self,
        reference: &'a WorldReference,
        root: &SourceId,
        output: &mut Vec<&'a SourceId>,
    ) -> Result<(), ReferenceError> {
        self.descend(reference, root, "", true, true, output, 0)
    }
    pub fn children<'a>(
        &'a self,
        reference: &'a WorldReference,
        root: &SourceId,
        class: &str,
        inactive: bool,
        output: &mut Vec<&'a SourceId>,
    ) -> Result<(), ReferenceError> {
        self.descend(reference, root, class, inactive, true, output, 0)
    }
    fn descend<'a>(
        &'a self,
        reference: &'a WorldReference,
        root: &SourceId,
        class: &str,
        inactive: bool,
        is_root: bool,
        output: &mut Vec<&'a SourceId>,
        depth: usize,
    ) -> Result<(), ReferenceError> {
        if depth > 256 {
            return Err(ReferenceError("Source hierarchy cycle".into()));
        }
        let n = self.node(reference, root)?;
        if !inactive && !is_root && !n.active {
            return Ok(());
        }
        let include = |r: SourceRecord<'a>| {
            r.class_name == class
                || (class.is_empty()
                    && r.fields
                        .get("m_Enabled")
                        .is_some_and(|v| numeric(v).is_ok_and(|n| n != 0.)))
        };
        if let Some(i) = self.node_index(root) {
            let n = &reference.initialization.nodes[i];
            for source in &n.components {
                let r = self.get(reference, source)?;
                if include(r) {
                    output.push(r.source);
                }
            }
            for child in &n.children {
                self.descend(reference, child, class, inactive, false, output, depth + 1)?;
            }
        } else {
            let go = self.get(reference, n.owner)?;
            for p in array(go.fields, "m_Component")? {
                if let Ok(r) = self.get(reference, link(fields(p)?, "component")?)
                    && include(r)
                {
                    output.push(r.source);
                }
            }
            let t = self.get(reference, root)?;
            for child in array(t.fields, "m_Children")? {
                self.descend(
                    reference,
                    value_link(child)?,
                    class,
                    inactive,
                    false,
                    output,
                    depth + 1,
                )?;
            }
        }
        Ok(())
    }
}
pub fn value<'a>(f: impl FieldAccess<'a>, n: &str) -> Result<&'a WorldValue, ReferenceError> {
    f.get(n)
        .ok_or_else(|| ReferenceError(format!("Missing source field {n}")))
}
pub fn number<'a>(f: impl FieldAccess<'a>, n: &str) -> Result<f32, ReferenceError> {
    numeric(value(f, n)?)
}
pub fn numeric(v: &WorldValue) -> Result<f32, ReferenceError> {
    match v {
        WorldValue::Number(v) => Ok(*v as f32),
        WorldValue::Integer(v) => Ok(*v as f32),
        WorldValue::Bool(v) => Ok(u8::from(*v) as f32),
        _ => Err(ReferenceError("Invalid source number".into())),
    }
}
pub fn flag<'a>(f: impl FieldAccess<'a>, n: &str) -> Result<bool, ReferenceError> {
    Ok(number(f, n)? != 0.)
}
pub fn integer<'a>(f: impl FieldAccess<'a>, n: &str) -> Result<i32, ReferenceError> {
    match value(f, n)? {
        WorldValue::Integer(v) => {
            i32::try_from(*v).map_err(|_| ReferenceError("Source integer overflow".into()))
        }
        _ => Err(ReferenceError(format!("Invalid source integer {n}"))),
    }
}
pub fn array<'a>(f: impl FieldAccess<'a>, n: &str) -> Result<&'a [WorldValue], ReferenceError> {
    match value(f, n)? {
        WorldValue::Array(v) => Ok(v),
        _ => Err(ReferenceError(format!("Invalid source array {n}"))),
    }
}
/// A source string array.
pub fn texts<'a>(f: impl FieldAccess<'a>, n: &str) -> Result<Vec<String>, ReferenceError> {
    array(f, n)?
        .iter()
        .map(|v| match v {
            WorldValue::Text(s) => Ok(s.clone()),
            _ => Err(ReferenceError(format!("Invalid source text in {n}"))),
        })
        .collect()
}
pub fn fields(v: &WorldValue) -> Result<Fields<'_>, ReferenceError> {
    match v {
        WorldValue::Record(f) => Ok(Fields::Record(f)),
        _ => Err(ReferenceError("Invalid source record".into())),
    }
}
pub fn text<'a>(f: impl FieldAccess<'a>, n: &str) -> Result<&'a str, ReferenceError> {
    match value(f, n)? {
        WorldValue::Text(v) => Ok(v),
        _ => Err(ReferenceError(format!("Invalid source text {n}"))),
    }
}
pub fn optional<'a>(
    f: impl FieldAccess<'a>,
    n: &str,
) -> Result<Option<&'a SourceId>, ReferenceError> {
    match value(f, n)? {
        WorldValue::Link(v) => Ok(Some(v)),
        WorldValue::Empty => Ok(None),
        _ => Err(ReferenceError(format!("Invalid source link {n}"))),
    }
}
/// `mainscript.CrossHairStuff`: the GCMMB sprite of a `tuneAble` usablescript.
pub fn tune_icon<'a>(
    usable: impl FieldAccess<'a>,
    ui: &crate::reference::UiReference,
) -> Result<Option<u32>, ReferenceError> {
    if !flag(usable, "tuneAble")? {
        return Ok(None);
    }
    Ok(optional(usable, "img")?.and_then(|img| ui.sprite(img)))
}
pub fn link<'a>(f: impl FieldAccess<'a>, n: &str) -> Result<&'a SourceId, ReferenceError> {
    optional(f, n)?.ok_or_else(|| ReferenceError(format!("Null source link {n}")))
}
pub fn value_link(v: &WorldValue) -> Result<&SourceId, ReferenceError> {
    match v {
        WorldValue::Link(v) => Ok(v),
        _ => Err(ReferenceError("Invalid source pointer".into())),
    }
}
pub fn vector<'a>(f: impl FieldAccess<'a>, n: &str) -> Result<[f32; 3], ReferenceError> {
    let v = fields(value(f, n)?)?;
    Ok([number(v, "x")?, number(v, "y")?, number(v, "z")?])
}

fn source_order(a: &SourceId, b: &SourceId) -> std::cmp::Ordering {
    a.file.cmp(&b.file).then(a.path_id.cmp(&b.path_id))
}

pub fn optional_array<'a>(
    f: impl FieldAccess<'a>,
    name: &str,
) -> Result<&'a [WorldValue], ReferenceError> {
    if matches!(value(f, name)?, WorldValue::Empty) {
        Ok(&[])
    } else {
        array(f, name)
    }
}
