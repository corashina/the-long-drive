//! Embedded source part slots reuse the active part and mechanism owners.
use super::contents::ContentInstance;
use super::spawn_rules::*;
use crate::interaction::{InteractionState, items::ItemStore, wheels::WheelAssemblies};
use crate::reference::*;
use crate::vehicle::PhysicsWorld;
pub(super) fn bind(
    content: &mut ContentInstance,
    instance: &super::StructureInstance,
    prototype: &WorldPrototypeReference,
    rules: &SourceRules,
    reference: &ReferenceManifest,
    items: &mut ItemStore,
    interaction: &mut InteractionState,
    physics: &mut PhysicsWorld,
    wheels: &mut WheelAssemblies,
) -> Result<(), ReferenceError> {
    if content.nodes.is_empty() {
        return Ok(());
    }
    let strings = |f, name| {
        array(f, name)?
            .iter()
            .map(|v| {
                if let WorldValue::Text(v) = v {
                    Ok(v.clone())
                } else {
                    Err(ReferenceError("Invalid generated slot type".into()))
                }
            })
            .collect::<Result<Vec<_>, _>>()
    };
    for (i, node) in prototype.nodes.iter().enumerate() {
        if !super::spawn_appearance::node_active(prototype, i, &content.content.appearance) {
            continue;
        }
        let Some(slot) = rules.component(&reference.world, &node.source, "partslotscript")? else {
            continue;
        };
        let f = slot.fields;
        let part_types = strings(f, "tipus")?;
        if part_types.iter().any(|s| s == "felni" || s == "gumi") {
            continue;
        }
        let node_id = |source: &SourceId| -> Result<NodeId, ReferenceError> {
            let root = rules.root(&reference.world, source)?;
            prototype
                .nodes
                .iter()
                .position(|n| &n.source == root)
                .map(|i| content.nodes[i])
                .ok_or_else(|| ReferenceError("Generated slot target missing".into()))
        };
        let mount = node_id(optional(f, "T")?.unwrap_or(&node.source))?;
        let detach = optional(f, "DT")?.map(node_id).transpose()?;
        let owner = super::content_parts::nearest_owner(content, prototype, rules, reference, i)?;
        let mut shapes = Vec::new();
        for (source, handle) in &instance.colliders {
            if rules.root(&reference.world, source)? != &node.source {
                continue;
            }
            let mut id = ColliderId((physics.rapier().colliders[*handle].user_data >> 64) as u32);
            if id.0 == 0 {
                id = interaction.allocate_generated_collider()?;
                physics.edit_collider(*handle, |collider| {
                    collider.user_data |= (id.0 as u128) << 64;
                });
            }
            shapes.push((id, *handle));
        }
        let id = interaction.parts.next_generated_slot()?;
        let mut position = vector(f, "VfixDt")?;
        position[2] = -position[2];
        let record = PartSlotReference {
            id,
            source: slot.source.clone(),
            node: content.nodes[i],
            owner,
            part: None,
            mount_anchor: AnchorId(0),
            detach_anchor: None,
            fixed_detach: flag(f, "fixDt")?,
            detach_position: position,
            detach_rotation: super::spawn_building::source_euler(vector(f, "VfixDtR")?),
            main_types: strings(f, "maintipus")?,
            part_types,
            cant_craft: flag(f, "cantCraft")?,
            cant_uncraft: flag(f, "cantUnCraft")?,
            disable_colliders_on_craft: flag(f, "disableCollidersOnCraft")?,
            colliders: shapes.iter().map(|s| s.0).collect(),
        };
        interaction.parts.register_generated_slot(
            record,
            mount,
            detach,
            &shapes,
            &interaction.mechanisms,
            physics,
        )?;
        items.register_generated_slot(id, owner);
        content.slots.push(super::contents::ContentSlot {
            source: slot.source.clone(),
            slot: id,
            initialized: false,
        });
    }
    for node in &prototype.nodes {
        let Some(part) = rules.component(&reference.world, &node.source, "partscript")? else {
            continue;
        };
        let item = if node.source == prototype.source {
            content.item
        } else {
            content
                .bindings
                .iter()
                .find(|b| b.source == *part.source)
                .map(|b| b.item)
        };
        if let (Some(item), Some(cover)) =
            (item, optional(part.fields, "cover_CantCraftIfCovered")?)
        {
            if let Some(slot) = content.slots.iter().find(|s| &s.source == cover) {
                interaction.parts.set_generated_cover(item, slot.slot);
                wheels.set_generated_cover(item, slot.slot);
            }
        }
    }
    Ok(())
}
