//! Source partconditionscript on generated items: store entries, tool targets and live appearance.
use super::WorldStream;
use super::content_index::ContentSlot;
use crate::conditions::{ConditionRule, ConditionState};
use crate::interaction::InteractionState;
use crate::interaction::items::ItemStore;
use crate::reference::{ItemId, ReferenceError, SourceId, WorldPrototypeReference};
use crate::save::SourceKey;
use crate::world::contents::ContentInstance;
use crate::world::spawn_appearance::{NodeAppearance, condition_appearance};
use crate::world::spawn_rules::*;

/// Adds the conditions of a bound content to the store and maps its colliders to them.
pub(super) fn bind(
    content: &mut ContentInstance,
    instance: &crate::world::StructureInstance,
    prototype: &WorldPrototypeReference,
    rules: &SourceRules,
    world: &crate::reference::WorldReference,
    interaction: &mut InteractionState,
) -> Result<(), ReferenceError> {
    let invalid = |s: &str| ReferenceError(format!("Generated condition: {s}"));
    let node_of = |source: &SourceId| -> Result<Option<usize>, ReferenceError> {
        let root = rules.root(world, source)?;
        Ok(prototype.nodes.iter().position(|n| &n.source == root))
    };
    // Items by the node of their pickup.
    let mut owners: Vec<(usize, ItemId)> = Vec::with_capacity(1 + content.bindings.len());
    if let Some(item) = content.item {
        let root = match rules.component(world, &prototype.source, "childunparent")? {
            Some(unparent) => link(unparent.fields, "g")?,
            None => &prototype.source,
        };
        if let Some(node) = node_of(root)? {
            owners.push((node, item));
        }
    }
    for b in &content.bindings {
        if let Some(node) = node_of(&b.source)? {
            owners.push((node, b.item));
        }
    }
    let id = content.content.placement.id.clone();
    for (index, input) in content.content.inputs.conditions.iter().enumerate() {
        if input.parent != id {
            continue;
        }
        let f = rules.get(world, &input.source)?.fields;
        let node = node_of(&input.source)?.ok_or_else(|| invalid("node outside its prototype"))?;
        let [state, state2, state3] = input.states;
        let permission = integer(f, "RepairProperty")?;
        if !(0..=4).contains(&state) || state2 < 0 || state3 < 0 || permission < 0 {
            return Err(invalid("state"));
        }
        // The nearest item node above the condition saves it.
        let mut parent = Some(node);
        let mut saver = content.item;
        for _ in 0..prototype.nodes.len() {
            let Some(n) = parent else {
                break;
            };
            if let Some(&(_, item)) = owners.iter().find(|(o, _)| *o == n) {
                saver = Some(item);
                break;
            }
            parent = prototype.nodes[n].parent.map(|p| p as usize);
        }
        let entry = interaction.conditions.add_world(
            ConditionRule {
                permission_index: permission as u32,
                children: Vec::new(),
                is_child: flag(f, "isChild")?,
                no_inherit_state: flag(f, "noInheritState")?,
                same_state_as_parent: flag(f, "sameStateAsParent")?,
                random_start_state3: flag(f, "randomStartState3InFStart")?,
            },
            ConditionState {
                state: state as u8,
                state2: state2 as u32,
                state3: state3 as u32,
                repair_progress: number(f, "repairValue")?,
                color: input.color,
            },
            owners
                .iter()
                .find(|(n, _)| *n == node)
                .map(|(_, item)| *item),
            saver.map(|item| (item, SourceKey::from(&input.source))),
        )?;
        content.conditions.push((entry, index, node));
    }
    let mut children = Vec::new();
    for &(entry, index, _) in &content.conditions {
        children.clear();
        let source = &content.content.inputs.conditions[index].source;
        for child in array(rules.get(world, source)?.fields, "childs")? {
            let child = rules.get(world, value_link(child)?)?.source;
            if let Some(&(c, ..)) = content
                .conditions
                .iter()
                .find(|(_, i, _)| &content.content.inputs.conditions[*i].source == child)
            {
                children.push(c);
            }
        }
        interaction.conditions.set_children(entry, &children);
    }
    // Source tools take the condition of the nearest parent node of the hit collider.
    for (source, handle) in &instance.colliders {
        let Some(collider) = prototype.colliders.iter().find(|c| &c.source == source) else {
            continue;
        };
        let mut node = Some(collider.node as usize);
        for _ in 0..prototype.nodes.len() {
            let Some(n) = node else {
                break;
            };
            if let Some(&(entry, ..)) = content.conditions.iter().find(|(.., c)| *c == n) {
                interaction.cleaning.bind_world_condition(*handle, entry);
                break;
            }
            node = prototype.nodes[n].parent.map(|p| p as usize);
        }
    }
    Ok(())
}

/// Removes the store entries and collider targets of a content.
pub(super) fn unbind(content: &mut ContentInstance, interaction: &mut InteractionState) {
    for (entry, ..) in content.conditions.drain(..) {
        interaction.conditions.remove_world(entry);
        interaction.cleaning.unbind_world_condition(entry);
    }
}

/// The last entry of `node` that `look` writes, as source refresh overwrites it.
fn merge(appearance: &mut Vec<NodeAppearance>, look: &NodeAppearance) {
    let found = appearance.iter_mut().rev().find(|a| {
        a.node == look.node
            && if look.active.is_some() {
                a.active.is_some()
            } else {
                a.material.is_some() || a.color.is_some() || a.color_material.is_some()
            }
    });
    match found {
        Some(a) if look.active.is_some() => a.active = look.active,
        Some(a) => {
            if a.material != look.material {
                a.material.clone_from(&look.material);
            }
            if a.color_material != look.color_material {
                a.color_material.clone_from(&look.color_material);
            }
            a.color = look.color;
        }
        None => appearance.push(look.clone()),
    }
}

impl WorldStream {
    /// Rebuilds the condition appearance of contents whose conditions changed and marks their items for save.
    pub(super) fn sync_condition_appearance(
        &mut self,
        items: &mut ItemStore,
        interaction: &mut InteractionState,
    ) -> Result<(), ReferenceError> {
        self.condition_changes.clear();
        while let Some(id) = interaction.conditions.take_world_changed() {
            if let Some(item) = interaction.conditions.world_owner(id) {
                items.mark_dirty(item);
            }
            self.condition_changes.push(id);
        }
        if self.condition_changes.is_empty() {
            return Ok(());
        }
        for (slot, content) in self.contents.active.iter_mut().enumerate() {
            if !content
                .conditions
                .iter()
                .any(|(id, ..)| self.condition_changes.contains(id))
            {
                continue;
            }
            for &(entry, index, _) in &content.conditions {
                let input = &content.content.inputs.conditions[index];
                let s = interaction.conditions.get(entry);
                self.condition_looks.clear();
                condition_appearance(
                    &self.reference,
                    &self.contents.rules,
                    &input.source,
                    &input.kind,
                    [s.state as i32, s.state2 as i32, s.state3 as i32],
                    s.color,
                    &mut self.condition_looks,
                )?;
                for look in &self.condition_looks {
                    merge(&mut content.content.appearance, look);
                }
            }
            let slot = ContentSlot::Active(slot);
            if !self.presentation_changes.contains(&slot) {
                self.presentation_changes.push(slot);
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn look(node: i64, material: Option<i64>, color: Option<[f32; 4]>) -> NodeAppearance {
        let id = |path_id| SourceId {
            file: "f".into(),
            path_id,
        };
        NodeAppearance {
            node: id(node),
            primary: None,
            scale: None,
            rotation: None,
            active: None,
            material: material.map(id),
            color,
            color_material: None,
        }
    }

    #[test]
    fn merge_overwrites_the_last_look_of_a_node() {
        let mut appearance = vec![look(1, Some(10), None), look(1, Some(11), Some([1.; 4]))];
        merge(&mut appearance, &look(1, Some(12), Some([0.5; 4])));
        assert_eq!(appearance.len(), 2);
        assert_eq!(appearance[1], look(1, Some(12), Some([0.5; 4])));
        merge(&mut appearance, &look(2, None, Some([0.; 4])));
        assert_eq!(appearance.len(), 3);
    }
}
