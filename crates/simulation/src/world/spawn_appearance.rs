//! Active source appearance is derived from initialized components.
use super::spawn_contents::SpawnEvaluation;
use super::spawn_rules::*;
use crate::reference::{ReferenceError, SourceId};
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct NodeAppearance {
    pub node: SourceId,
    pub primary: Option<(crate::reference::AssetId, u32)>,
    pub scale: Option<[f32; 3]>,
    /// Local rotation override, port space quaternion xyzw.
    pub rotation: Option<[f32; 4]>,
    pub active: Option<bool>,
    pub material: Option<SourceId>,
    pub color: Option<[f32; 4]>,
    pub color_material: Option<SourceId>,
}
impl NodeAppearance {
    pub fn heap_bytes(&self) -> usize {
        self.node.file.capacity()
            + self.material.as_ref().map_or(0, |s| s.file.capacity())
            + self
                .color_material
                .as_ref()
                .map_or(0, |s| s.file.capacity())
    }
}
pub fn node_active(
    prototype: &crate::reference::WorldPrototypeReference,
    mut node: usize,
    appearance: &[NodeAppearance],
) -> bool {
    for _ in 0..prototype.nodes.len() {
        let n = &prototype.nodes[node];
        if !appearance
            .iter()
            .rev()
            .find(|a| a.node == n.source && a.active.is_some())
            .and_then(|a| a.active)
            .unwrap_or(n.parent.is_none() || n.active)
        {
            return false;
        }
        let Some(parent) = n.parent else {
            return true;
        };
        node = parent as usize;
    }
    false
}
impl SpawnEvaluation<'_> {
    /// Source `buildingscript.FStart` cable. Bake after lamps, so the cable is not lost on refresh.
    pub(super) fn bake_cables(&mut self) {
        for content in &mut self.output {
            content.appearance.extend_from_slice(&content.cable);
        }
    }
    pub(super) fn bake_appearance(&mut self) -> Result<(), ReferenceError> {
        for content in &mut self.output {
            content.inputs = self.initialization.for_parent(&content.placement.id);
            content.appearance.clear();
            for (parent, source, selected) in &self.initialization.paintings {
                if *parent != content.placement.id {
                    continue;
                }
                let f = self.rules.get(self.reference, source)?.fields;
                let main_id = SourceId {
                    file: "level1".into(),
                    path_id: 64919,
                };
                let main = self.rules.get(self.reference, &main_id)?;
                let texture = value_link(
                    array(main.fields, "paintings")?
                        .get(*selected as usize)
                        .ok_or_else(|| {
                            ReferenceError("Source painting selection out of range".into())
                        })?,
                )?;
                let material = link(f, "defmat")?;
                let binding = self
                    .reference
                    .painting_materials
                    .iter()
                    .find(|b| &b.material == material && &b.texture == texture)
                    .ok_or_else(|| ReferenceError("Missing selected painting material".into()))?;
                let renderer = link(f, "r")?;
                content.appearance.push(NodeAppearance {
                    node: self.rules.root(self.reference, renderer)?.clone(),
                    primary: Some((binding.asset, 0)),
                    scale: None,
                    active: None,
                    material: None,
                    color: None,
                    color_material: None,
                    rotation: None,
                });
            }

            for (parent, object, active) in &self.initialization.activations {
                if *parent != content.placement.id {
                    continue;
                }
                content.appearance.push(NodeAppearance {
                    primary: None,
                    node: self.rules.root(self.reference, object)?.clone(),
                    active: Some(*active),
                    material: None,
                    color: None,
                    color_material: None,
                    rotation: None,
                    scale: None,
                });
            }
            for (owner, node, scale) in &self.initialization.scales {
                if owner != &content.placement.id {
                    continue;
                }
                content.appearance.push(NodeAppearance {
                    primary: None,
                    node: node.clone(),
                    scale: Some(*scale),
                    active: None,
                    material: None,
                    color: None,
                    color_material: None,
                    rotation: None,
                });
            }
            for state in &self.initialization.conditions {
                if state.parent != content.placement.id {
                    continue;
                }
                condition_appearance(
                    self.reference,
                    self.rules,
                    &state.source,
                    &state.kind,
                    state.states,
                    state.color,
                    &mut content.appearance,
                )?;
            }
        }
        Ok(())
    }
}

/// Source `partconditionscript.Refresh` appearance of one condition: renderer materials and colors, then `GoList` activity.
pub fn condition_appearance(
    reference: &crate::reference::WorldReference,
    rules: &SourceRules,
    source: &SourceId,
    kind: &str,
    states: [i32; 3],
    paint: [f32; 4],
    out: &mut Vec<NodeAppearance>,
) -> Result<(), ReferenceError> {
    let record = rules.get(reference, source)?;
    let f = record.fields;
    let mut material = None;
    let mut color = None;
    let mut color_material = None;
    let kind = if flag(f, "useOnlyMaterialTipusForMaterial")? {
        text(f, "materialTipus")?
    } else {
        kind
    };
    let main = rules.get(
        reference,
        &SourceId {
            file: "level1".into(),
            path_id: 64919,
        },
    )?;
    let common = array(main.fields, "conditionmaterials")?
        .iter()
        .find_map(|v| {
            let row = fields(v).ok()?;
            (text(row, "tipus").ok()? == kind).then_some(row)
        });
    let disable = flag(f, "disableColor")?;
    if flag(f, "UseMesList")? && !array(f, "MesList")?.is_empty() {
        return Err(ReferenceError(
            "Supported condition requires a checked mesh variant recipe".into(),
        ));
    }
    if flag(f, "UseMatList")? {
        if let Some(value) = array(f, "MatList")?.get(states[0] as usize) {
            material = Some(value_link(value)?.clone());
        }
        if flag(f, "forceColor")? && !disable {
            color = Some(paint);
        }
    }
    if !flag(f, "DontUseWorn")? && (0..5).contains(&states[0]) {
        let names = ["New", "Used", "Middle", "Old", "Rusty"];
        material = if let Some(row) = common {
            optional(row, names[states[0] as usize])?.cloned()
        } else {
            optional(
                f,
                ["defNew", "defUsed", "defMiddle", "defOld", "defRusty"][states[0] as usize],
            )?
            .cloned()
        };
        if !disable {
            if states[0] < 3 || flag(f, "forceColor")? {
                color = Some(paint);
            } else {
                color_material = if let Some(row) = common {
                    optional(row, "Rusty")?.cloned()
                } else {
                    optional(f, "defRusty")?.cloned()
                };
            }
        }
    }
    if material.is_none() && !disable {
        let value = array(common.unwrap_or(f), "colors")?
            .get(states[0] as usize)
            .ok_or_else(|| ReferenceError("Source condition color index".into()))?;
        let f = fields(value)?;
        color = Some([
            number(f, "r")?,
            number(f, "g")?,
            number(f, "b")?,
            number(f, "a")?,
        ]);
    }
    for renderer in array(f, "renderers")? {
        out.push(NodeAppearance {
            primary: None,
            node: rules.root(reference, value_link(renderer)?)?.clone(),
            active: None,
            material: material.clone(),
            color,
            color_material: color_material.clone(),
            rotation: None,
            scale: None,
        });
    }
    if flag(f, "UseGoList")? {
        let objects = array(f, "GoList")?;
        if !objects.is_empty() && states[2] as usize >= objects.len() {
            return Err(ReferenceError("Source condition object index".into()));
        }
        for (i, object) in objects.iter().enumerate() {
            out.push(NodeAppearance {
                primary: None,
                node: rules.root(reference, value_link(object)?)?.clone(),
                active: Some(i == states[2] as usize),
                material: None,
                color: None,
                color_material: None,
                rotation: None,
                scale: None,
            });
        }
    }
    Ok(())
}
