//! Borrowed access to checked prototype records and compact initialization inputs.
use crate::reference::WorldFields;
use crate::reference::{
    ReferenceError, SourceId, Transform, WorldInitField, WorldReference, WorldValue,
};
use std::collections::HashMap;
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
    nodes: Vec<u32>,
    owners: Vec<u32>,
    radii: HashMap<SourceId, f64>,
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
        let mut nodes: Vec<_> = (0..reference.initialization.nodes.len() as u32).collect();
        nodes.sort_unstable_by(|a, b| {
            source_order(
                &reference.initialization.nodes[*a as usize].source,
                &reference.initialization.nodes[*b as usize].source,
            )
        });
        let mut owners = nodes.clone();
        owners.sort_unstable_by(|a, b| {
            source_order(
                &reference.initialization.nodes[*a as usize].owner,
                &reference.initialization.nodes[*b as usize].owner,
            )
        });
        Self {
            indices,
            nodes,
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
    fn index(&self, reference: &WorldReference, source: &SourceId) -> Option<(bool, usize)> {
        self.indices
            .binary_search_by(|entry| source_order(Self::indexed_source(reference, *entry), source))
            .ok()
            .map(|i| {
                let (compact, index) = self.indices[i];
                (compact, index as usize)
            })
    }
    fn node_index(&self, reference: &WorldReference, source: &SourceId) -> Option<usize> {
        self.nodes
            .binary_search_by(|i| {
                source_order(&reference.initialization.nodes[*i as usize].source, source)
            })
            .ok()
            .map(|i| self.nodes[i] as usize)
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
    /// Bound per-instance initialization, appearance, temporary traversal, and copied keys.
    pub fn initialization_bound(
        &self,
        reference: &WorldReference,
        root: &SourceId,
        identity: usize,
        depth: usize,
    ) -> Result<usize, ReferenceError> {
        if depth > 256 {
            return Err(ReferenceError(
                "Source initialization hierarchy cycle".into(),
            ));
        }
        let mut bytes = 1024usize + identity.saturating_mul(8);
        let mut component = |source: &SourceId| -> Result<(), ReferenceError> {
            if let Ok(record) = self.get(reference, source) {
                // Selection rows are borrowed. Each selected child gets its own admission.
                // Mesh, collider and raw transform fields never become initialization copies.
                if !matches!(
                    record.class_name,
                    "tosaveitemscript"
                        | "partconditionscript"
                        | "randomTypeSelector"
                        | "electronicsscript"
                        | "flashlightscript"
                        | "partslotscript"
                        | "newAiScript"
                        | "randomFlyer"
                        | "nyulscript"
                        | "paintingScript"
                        | "randomScaleScript"
                        | "tankscript"
                        | "Light"
                ) {
                    return Ok(());
                }
                let fields = match record.fields {
                    Fields::Record(f) => f.values().map(|v| v.input_heap_bytes()).sum::<usize>(),
                    Fields::Compact(f) => {
                        f.iter().map(|f| f.value.input_heap_bytes()).sum::<usize>()
                    }
                };
                bytes = bytes
                    .saturating_add(2048 + identity.saturating_mul(8) + fields.saturating_mul(16));
            }
            Ok(())
        };
        if let Some(i) = self.node_index(reference, root) {
            let node = &reference.initialization.nodes[i];
            for source in &node.components {
                component(source)?;
            }
            for child in &node.children {
                bytes = bytes.saturating_add(self.initialization_bound(
                    reference,
                    child,
                    identity,
                    depth + 1,
                )?);
            }
        } else {
            let node = self.node(reference, root)?;
            for pointer in array(self.get(reference, node.owner)?.fields, "m_Component")? {
                component(link(fields(pointer)?, "component")?)?;
            }
            for child in array(self.get(reference, root)?.fields, "m_Children")? {
                bytes = bytes.saturating_add(self.initialization_bound(
                    reference,
                    value_link(child)?,
                    identity,
                    depth + 1,
                )?);
            }
        }
        Ok(bytes)
    }
    pub fn retained_bytes(&self) -> usize {
        self.indices.capacity() * std::mem::size_of::<(bool, u32)>()
            + (self.nodes.capacity() + self.owners.capacity()) * std::mem::size_of::<u32>()
            + self.radii.capacity() * (std::mem::size_of::<(SourceId, f64)>() + 16)
            + self.radii.keys().map(|s| s.file.capacity()).sum::<usize>()
    }
    pub fn get<'a>(
        &self,
        reference: &'a WorldReference,
        source: &SourceId,
    ) -> Result<SourceRecord<'a>, ReferenceError> {
        let (compact, i) = self.index(reference, source).ok_or_else(|| {
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
            self.index(reference, script)
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
        if let Some(i) = self.node_index(reference, source) {
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
        if let Some(i) = self.node_index(reference, object) {
            return Ok(&reference.initialization.nodes[i].source);
        }
        if let Ok(i) = self.owners.binary_search_by(|i| {
            source_order(&reference.initialization.nodes[*i as usize].owner, object)
        }) {
            return Ok(&reference.initialization.nodes[self.owners[i] as usize].source);
        }
        let r = self.get(reference, object)?;
        if matches!(r.class_name, "Transform" | "RectTransform") {
            return Ok(r.source);
        }
        if let Some((true, i)) = self.index(reference, object) {
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
            if let Ok(r) = self.get(reference, target) {
                if matches!(r.class_name, "Transform" | "RectTransform") {
                    return Ok(r.source);
                }
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
        if let Some(i) = self.node_index(reference, root) {
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
                if let Ok(r) = self.get(reference, target) {
                    if r.class_name == class {
                        return Ok(Some(r));
                    }
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
        if let Some(i) = self.node_index(reference, root) {
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
                if let Ok(r) = self.get(reference, link(fields(p)?, "component")?) {
                    if include(r) {
                        output.push(r.source);
                    }
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::reference::{WorldInitComponent, WorldSourceRecord};

    // This is the original LOD lookup, independent of the sorted locator.
    fn linear_owner<'a>(reference: &'a WorldReference, source: &SourceId) -> Option<&'a SourceId> {
        reference
            .records
            .iter()
            .find(|r| r.source == *source)
            .and_then(|r| match r.fields.get("m_GameObject") {
                Some(WorldValue::Link(owner)) => Some(owner),
                _ => None,
            })
    }

    #[test]
    #[ignore = "requires the installed source export"]
    fn installed_lod_owner_matches_linear_without_retained_growth() {
        #[derive(serde::Deserialize)]
        struct Manifest {
            world: WorldReference,
        }
        let bytes = std::fs::read(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../web/assets/reference/manifest.json"
        ))
        .unwrap();
        let mut reference = serde_json::from_slice::<Manifest>(&bytes).unwrap().world;
        let source = |file: &str, path_id| SourceId {
            file: file.into(),
            path_id,
        };
        let full = source("lod-owner-regression", 1);
        let second = source("lod-owner-regression", 2);
        let other_file = source("lod-owner-other-file", 1);
        let missing_field = source("lod-owner-regression", 3);
        let nonlink = source("lod-owner-regression", 4);
        let compact = source("lod-owner-regression", 5);
        let owner = source("lod-owner-regression", 10);
        let other_owner = source("lod-owner-regression", 11);
        for (id, value) in [
            (&full, Some(WorldValue::Link(owner.clone()))),
            (&second, Some(WorldValue::Link(owner.clone()))),
            (&other_file, Some(WorldValue::Link(other_owner.clone()))),
            (&missing_field, None),
            (&nonlink, Some(WorldValue::Integer(10))),
        ] {
            let fields = value
                .map(|value| WorldInitField {
                    name: "m_GameObject".into(),
                    value,
                })
                .into_iter()
                .collect::<Vec<_>>();
            reference.records.push(WorldSourceRecord {
                source: id.clone(),
                class_name: "Transform".into(),
                fields: serde_json::from_value(serde_json::to_value(fields).unwrap()).unwrap(),
            });
        }
        reference
            .initialization
            .components
            .push(WorldInitComponent {
                source: compact.clone(),
                owner: owner.clone(),
                class_name: "Transform".into(),
                execution_order: 0,
                fields: vec![WorldInitField {
                    name: "m_GameObject".into(),
                    value: WorldValue::Link(owner.clone()),
                }],
            });
        // A compact twin must not mask the full record's owner.
        reference
            .initialization
            .components
            .push(WorldInitComponent {
                source: full.clone(),
                owner: other_owner.clone(),
                class_name: "Transform".into(),
                execution_order: 0,
                fields: vec![WorldInitField {
                    name: "m_GameObject".into(),
                    value: WorldValue::Link(other_owner.clone()),
                }],
            });
        let rules = SourceRules::new(&reference);
        let storage = || {
            (
                (
                    rules.indices.as_ptr(),
                    rules.indices.len(),
                    rules.indices.capacity(),
                ),
                (
                    rules.nodes.as_ptr(),
                    rules.nodes.len(),
                    rules.nodes.capacity(),
                ),
                (
                    rules.owners.as_ptr(),
                    rules.owners.len(),
                    rules.owners.capacity(),
                ),
                (rules.radii.len(), rules.radii.capacity()),
                rules.retained_bytes(),
            )
        };
        let before = storage();
        let mut queries = 0;
        let mut check = |source: &SourceId| {
            let expected = linear_owner(&reference, source);
            let actual = rules.full_record_owner(&reference, source);
            assert_eq!(actual, expected, "owner for {source:?}");
            if let (Some(actual), Some(expected)) = (actual, expected) {
                assert!(std::ptr::eq(actual, expected), "owner must remain borrowed");
            }
            queries += 1;
        };
        for prototype in &reference.prototypes {
            for node in &prototype.nodes {
                check(&node.source);
            }
        }
        let mut groups = 0;
        let mut renderers = 0;
        for record in reference
            .records
            .iter()
            .filter(|r| r.class_name == "LODGroup")
        {
            check(&record.source);
            groups += 1;
            if let Some(WorldValue::Array(levels)) = record.fields.get("m_LODs") {
                for level in levels {
                    let WorldValue::Record(fields) = level else {
                        panic!("LOD level")
                    };
                    let Some(WorldValue::Array(entries)) = fields.get("renderers") else {
                        panic!("LOD renderers")
                    };
                    for entry in entries {
                        let WorldValue::Record(fields) = entry else {
                            panic!("LOD renderer")
                        };
                        if let Some(WorldValue::Link(source)) = fields.get("renderer") {
                            check(source);
                            renderers += 1;
                        }
                    }
                }
            }
        }
        for id in [
            &full,
            &second,
            &other_file,
            &missing_field,
            &nonlink,
            &compact,
            &source("lod-owner-regression", 99),
        ] {
            check(id);
        }
        assert_eq!(rules.full_record_owner(&reference, &full), Some(&owner));
        assert_eq!(
            rules.full_record_owner(&reference, &other_file),
            Some(&other_owner)
        );
        assert_eq!(rules.full_record_owner(&reference, &compact), None);
        let nodes = [&compact, &other_file, &full, &second];
        let expected = nodes
            .iter()
            .position(|s| linear_owner(&reference, s) == Some(&owner));
        let actual = nodes
            .iter()
            .position(|s| rules.full_record_owner(&reference, s) == Some(&owner));
        assert_eq!(expected, Some(2));
        assert_eq!(actual, expected);
        assert!(groups > 0 && renderers > 0 && queries > groups + renderers);
        assert_eq!(storage(), before, "lookup must retain no new storage");
        println!("checked {queries} owners, {groups} LOD groups, {renderers} renderer links");
    }
}
