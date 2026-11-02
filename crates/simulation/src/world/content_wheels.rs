//! Generated rims and tires use the existing wheel assembly owner.
use super::contents::ContentInstance;
use super::spawn_rules::*;
use crate::interaction::{InteractionState, items::ItemStore, wheels::WheelAssemblies};
use crate::reference::*;
use crate::vehicle::{PhysicsWorld, STEP_SURFACE_MASK};
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
    let owner = content.item.unwrap();
    for (node_index, node) in prototype.nodes.iter().enumerate() {
        if !super::spawn_appearance::node_active(prototype, node_index, &content.content.appearance)
        {
            continue;
        }
        let root = &node.source;
        let rim = rules.component(&reference.world, root, "wheelscript")?;
        let tire = rules.component(&reference.world, root, "gumiscript")?;
        if rim.is_none() && tire.is_none() {
            continue;
        }
        let part = rules
            .component(&reference.world, root, "partscript")?
            .ok_or_else(|| ReferenceError("Generated wheel has no source part".into()))?;
        let strings = |f, name| {
            array(f, name)?
                .iter()
                .map(|v| {
                    if let WorldValue::Text(s) = v {
                        Ok(s.clone())
                    } else {
                        Err(ReferenceError("Invalid source wheel type".into()))
                    }
                })
                .collect::<Result<Vec<_>, _>>()
        };
        let body_index = prototype
            .bodies
            .iter()
            .position(|b| b.node as usize == node_index)
            .ok_or_else(|| ReferenceError("Generated wheel body missing".into()))?;
        let body = instance.bodies[body_index];
        let item = if node_index == 0 {
            owner
        } else {
            let pickup = rules
                .component(&reference.world, root, "pickupable")?
                .ok_or_else(|| ReferenceError("Embedded wheel pickup missing".into()))?;
            let mut key = content.content.placement.id.clone();
            key.children.push((part.source.clone(), 0));
            let pose = super::placement::place_pose(
                content.content.placement.pose,
                content.content.placement.scale,
                super::placement::node_pose(prototype, node_index),
            );
            let item = items.insert_generated(key, prototype, pose)?;
            let hit = super::stream::content_bindings::hit_sound(&reference.world, pickup.source);
            let state = items.set_body(item, Some(body)).unwrap();
            state.hit_sound = hit;
            state.local_pose = crate::vehicle::from_rapier(
                &(physics.rapier().bodies[body].position().inverse()
                    * crate::vehicle::to_rapier(state.pose)),
            );
            let local = state.local_pose;
            content.bindings.push(super::contents::ContentBinding {
                source: part.source.clone(),
                item,
                baseline: items.generated_snapshot(item, None).unwrap(),
                render_body: Some((body, local)),
            });
            let mut layers = 0;
            for (source, handle) in &instance.colliders {
                let collider = prototype
                    .colliders
                    .iter()
                    .find(|c| c.source == *source)
                    .unwrap();
                if collider.body != Some(body_index as u32) {
                    continue;
                }
                layers |= 1 << prototype.nodes[collider.node as usize].layer;
                let id = interaction.allocate_generated_collider()?;
                physics.edit_collider(*handle, |collider| {
                    collider.user_data = (collider.user_data & STEP_SURFACE_MASK)
                        | ((id.0 as u128) << 64)
                        | (item.0 as u128 + 1);
                    collider.set_active_events(super::stream::content_bindings::hit_events(hit));
                });
                interaction.bind_surface(
                    *handle,
                    ActionBinding {
                        collider: id,
                        use_owner: None,
                        pickup_owner: Some(item),
                        removal_owner: None,
                        push_owner: None,
                    },
                );
            }
            interaction.register_generated_pickup(
                item,
                super::stream::content_bindings::pickup_rule(pickup)?,
                layers,
                false,
            );
            // Slots under the wheel take its pose.
            interaction
                .mechanisms
                .register_generated_item(item, content.nodes[node_index]);
            item
        };
        let shapes: Vec<_> = instance
            .colliders
            .iter()
            .filter_map(|(_, handle)| {
                let c = &physics.rapier().colliders[*handle];
                (c.parent() == Some(body))
                    .then_some((ColliderId((c.user_data >> 64) as u32), *handle))
            })
            .collect();
        let (radius, tire_mount, tire_main_types, tire_types) = if let Some(rim) = rim {
            let slot = rules.get(&reference.world, link(rim.fields, "gumi")?)?;
            let target = link(slot.fields, "T")?;
            let index = prototype
                .nodes
                .iter()
                .position(|n| &n.source == target)
                .ok_or_else(|| {
                    ReferenceError("Generated rim mount is outside its prototype".into())
                })?;
            (
                number(rim.fields, "felniRadius")?,
                Some(rim_local(prototype, node_index, index)),
                strings(slot.fields, "maintipus")?,
                strings(slot.fields, "tipus")?,
            )
        } else {
            (
                number(tire.unwrap().fields, "radius")?,
                None,
                Vec::new(),
                Vec::new(),
            )
        };
        let recipe = WheelPartReference {
            cover_slot: None,
            item,
            source: part.source.clone(),
            radius,
            main_types: strings(part.fields, "maintipus")?,
            part_types: strings(part.fields, "tipus")?,
            colliders: shapes.iter().map(|v| v.0).collect(),
            tire_slot_anchor: None,
            tire_mount,
            tire_main_types,
            tire_types,
            local_rotation_after_mount: super::spawn_building::source_euler(vector(
                part.fields,
                "localRotationAfterMount",
            )?),
            fall_off_category: super::content_parts::fall_off_category(part.fields)?,
        };
        interaction.register_generated_wheel(&recipe, rim.is_some(), wheels.rules());
        wheels.register_generated(
            recipe,
            rim.is_some(),
            physics.rapier().bodies[body].mass(),
            &shapes,
            items,
            physics,
        )?;
    }
    // The checked source rim slot identifies the serialized tire. Reuse assembly relocation.
    // Each rim of the prototype takes its embedded tire: a loose rim or the rims of a car.
    for (node_index, node) in prototype.nodes.iter().enumerate() {
        if !super::spawn_appearance::node_active(prototype, node_index, &content.content.appearance)
        {
            continue;
        }
        let Some(rim) = rules.component(&reference.world, &node.source, "wheelscript")? else {
            continue;
        };
        let rim_item = if node_index == 0 {
            owner
        } else {
            let part = rules
                .component(&reference.world, &node.source, "partscript")?
                .ok_or_else(|| ReferenceError("Generated wheel has no source part".into()))?;
            match content.bindings.iter().find(|b| b.source == *part.source) {
                Some(binding) => binding.item,
                None => continue,
            }
        };
        let slot = rules.get(&reference.world, link(rim.fields, "gumi")?)?;
        let Some(part) = optional(slot.fields, "part")? else {
            continue;
        };
        let Some(index) = content.bindings.iter().position(|b| &b.source == part) else {
            continue;
        };
        let tire = content.bindings[index].item;
        wheels.assemble(rim_item, tire, items, physics)?;
        content.bindings[index].baseline = items.generated_snapshot(tire, None).unwrap();
    }
    Ok(())
}

/// The pose of prototype node `index` in the frame of wheel node `wheel`.
fn rim_local(prototype: &WorldPrototypeReference, wheel: usize, index: usize) -> Transform {
    use crate::vehicle::{VehiclePose, from_rapier, to_rapier};
    let pose = |i| {
        let t = super::placement::node_pose(prototype, i);
        to_rapier(VehiclePose {
            position: t.translation,
            rotation: t.rotation,
        })
    };
    let local = from_rapier(&(pose(wheel).inverse() * pose(index)));
    Transform {
        translation: local.position,
        rotation: local.rotation,
        scale: [1.; 3],
    }
}
