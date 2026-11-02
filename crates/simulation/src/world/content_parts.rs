//! Generated general parts join the existing source slot and mechanism owners.
use super::contents::ContentInstance;
use super::spawn_rules::*;
use crate::interaction::{
    InteractionState,
    items::{ItemLocation, ItemStore},
};
use crate::reference::*;
use crate::vehicle::{PhysicsWorld, STEP_SURFACE_MASK};
pub(super) fn bind(
    content: &mut ContentInstance,
    instance: &super::StructureInstance,
    prototype: &WorldPrototypeReference,
    rules: &SourceRules,
    reference: &ReferenceManifest,
    pickup: Option<PickupReference>,
    layers: u32,
    items: &mut ItemStore,
    interaction: &mut InteractionState,
    physics: &mut PhysicsWorld,
) -> Result<(), ReferenceError> {
    let owner = content.item.unwrap();
    if !prototype.nodes.iter().any(|n| {
        rules
            .component(&reference.world, &n.source, "partscript")
            .ok()
            .flatten()
            .is_some()
            || rules
                .component(&reference.world, &n.source, "partslotscript")
                .ok()
                .flatten()
                .is_some()
    }) {
        return Ok(());
    }
    if content.nodes.is_empty() {
        let state = items.get(owner).unwrap();
        content.nodes = interaction.mechanisms.register_generated(
            prototype,
            state.pose,
            content.content.placement.scale,
            owner,
            0,
        )?;
    }
    let attached = attached_at_start(content, prototype, rules, reference)?;
    // An attached body rides on its target body.
    let rides_on = |c: &WorldColliderReference, body_index: usize| {
        c.body == Some(body_index as u32)
            || attached
                .iter()
                .any(|&(plate, target)| target == body_index && c.body == Some(plate as u32))
    };
    for (part_node, node) in prototype.nodes.iter().enumerate() {
        if !super::spawn_appearance::node_active(prototype, part_node, &content.content.appearance)
        {
            continue;
        }
        let root = &node.source;
        let Some(part) = rules.component(&reference.world, root, "partscript")? else {
            continue;
        };
        if rules
            .component(&reference.world, root, "wheelscript")?
            .is_some()
            || rules
                .component(&reference.world, root, "gumiscript")?
                .is_some()
        {
            continue;
        }
        let main_types = texts(part.fields, "maintipus")?;
        let part_types = texts(part.fields, "tipus")?;
        // Every part registers: a slot of any car can take it, even one that loads later.
        let pickup_record = rules.get(&reference.world, link(part.fields, "p")?)?;
        // Car roots have no pickupable. Only a root that is itself a part needs one.
        let pickup = if part_node == 0 {
            pickup
                .clone()
                .ok_or_else(|| ReferenceError("Generated part owner has no pickupable".into()))?
        } else {
            super::stream::content_bindings::pickup_rule(pickup_record)?
        };
        let pickup_root = rules.root(&reference.world, &pickup.source)?;
        let pickup_node = prototype
            .nodes
            .iter()
            .position(|n| &n.source == pickup_root)
            .ok_or_else(|| ReferenceError("Generated part pickup root missing".into()))?;
        let body_index = prototype
            .bodies
            .iter()
            .position(|b| b.node as usize == pickup_node)
            .ok_or_else(|| ReferenceError("Generated part body missing".into()))?;
        let body = instance.bodies[body_index];
        let item = if part_node == 0 {
            owner
        } else {
            let mut key = content.content.placement.id.clone();
            key.children.push((part.source.clone(), 0));
            let pose = super::placement::place_pose(
                content.content.placement.pose,
                content.content.placement.scale,
                super::placement::node_pose(prototype, pickup_node),
            );
            let item = items.insert_generated(key, prototype, pose)?;
            let hit = super::stream::content_bindings::hit_sound(&reference.world, &pickup.source);
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
                let c = prototype
                    .colliders
                    .iter()
                    .find(|c| c.source == *source)
                    .unwrap();
                if !rides_on(c, body_index) {
                    continue;
                }
                layers |= 1 << prototype.nodes[c.node as usize].layer;
                let id = interaction.allocate_generated_collider()?;
                physics.edit_collider(*handle, |collider| {
                    collider.user_data = (collider.user_data & STEP_SURFACE_MASK)
                        | ((id.0 as u128) << 64)
                        | (item.0 as u128 + 1);
                    collider.set_active_events(super::stream::content_bindings::hit_events(hit));
                });
                // Keep the cap binding: source looks at the cap collider on the part.
                let use_owner = interaction.use_owner(*handle);
                interaction.bind_surface(
                    *handle,
                    ActionBinding {
                        collider: id,
                        use_owner,
                        pickup_owner: Some(item),
                        removal_owner: Some(item),
                        push_owner: None,
                    },
                );
            }
            interaction.register_generated_pickup(item, pickup.clone(), layers, false);
            interaction
                .mechanisms
                .register_generated_item(item, content.nodes[pickup_node]);

            item
        };
        let linked_nodes = |name| {
            array(part.fields, name)?
                .iter()
                .map(|v| {
                    let source = rules.root(&reference.world, value_link(v)?)?;
                    prototype
                        .nodes
                        .iter()
                        .position(|n| &n.source == source)
                        .map(|i| content.nodes[i])
                        .ok_or_else(|| {
                            ReferenceError("Generated part activation target missing".into())
                        })
                })
                .collect::<Result<Vec<_>, _>>()
        };
        let shape_handles: Vec<_> = instance
            .colliders
            .iter()
            .filter_map(|(_, handle)| {
                let c = &physics.rapier().colliders[*handle];
                let parent = c.parent()?;
                (parent == body
                    || attached.iter().any(|&(plate, target)| {
                        instance.bodies[target] == body && instance.bodies[plate] == parent
                    }))
                .then_some((ColliderId((c.user_data >> 64) as u32), *handle))
            })
            .collect();
        let rigid = &physics.rapier().bodies[body];
        let mass = rigid.mass();
        let recipe = PartReference {
            scan_radius: number(part.fields, "scanRadius")?,
            usable: None,
            item,
            source: part.source.clone(),
            node: content.nodes[part_node],
            main_types,
            part_types,
            cover_slot: None,
            local_rotation_after_mount: super::spawn_building::source_euler(vector(
                part.fields,
                "localRotationAfterMount",
            )?),
            colliders: shape_handles.iter().map(|c| c.0).collect(),
            linear_damping: rigid.linear_damping(),
            angular_damping: rigid.angular_damping(),
            enable_if_mounted: linked_nodes("enableIfMounted")?,
            enable_if_dismounted: linked_nodes("enableIfDismounted")?,
            disable_if_mounted: linked_nodes("disableIfMounted")?,
            disable_if_dismounted: linked_nodes("disableIfDismounted")?,
            fall_off_category: fall_off_category(part.fields)?,
        };
        let pose = interaction.mechanisms.node_pose(recipe.node).unwrap();
        let mut members = vec![(item, content.nodes[pickup_node], mass)];
        for binding in &content.bindings {
            if binding.item == item || binding.render_body.is_some() {
                continue;
            }
            let root = rules.root(&reference.world, &binding.source)?;
            let node = prototype
                .nodes
                .iter()
                .position(|n| &n.source == root)
                .ok_or_else(|| ReferenceError("Generated part member missing".into()))?;
            if nearest_owner(content, prototype, rules, reference, node)? == Some(item) {
                members.push((binding.item, content.nodes[node], 0.));
                // A tank or cap in the part, such as the radiator coolant, leaves with the part.
                let state = items.get_mut(binding.item).unwrap();
                if state.location == ItemLocation::Attached(owner) {
                    state.location = ItemLocation::Attached(item);
                }
            }
        }
        interaction.parts.register_generated(
            recipe,
            pickup,
            body,
            &shape_handles,
            pose,
            members,
            mass,
            items,
            physics,
        )?;
        interaction.register_generated_part_kind(item, layers);
    }
    for &(plate, target) in &attached {
        let plate = instance.bodies[plate];
        let target = instance.bodies[target];
        let pose = *physics.rapier().bodies[target].position();
        for (_, handle) in &instance.colliders {
            if physics.rapier().colliders[*handle].parent() != Some(plate) {
                continue;
            }
            let local = pose.inverse() * *physics.rapier().colliders[*handle].position();
            physics.set_collider_parent(*handle, Some(target));
            physics.edit_collider(*handle, |c| {
                c.set_position_wrt_parent(local);
                c.set_density(0.);
            });
        }
        physics.edit_body(plate, |b| b.set_enabled(false));
    }
    Ok(())
}

/// Source `attachablescript.Attach(attachAtStartTarget)` locks the body to the target body.
/// Returns (attached body, target body) prototype body indices.
fn attached_at_start(
    content: &ContentInstance,
    prototype: &WorldPrototypeReference,
    rules: &SourceRules,
    reference: &ReferenceManifest,
) -> Result<Vec<(usize, usize)>, ReferenceError> {
    let body_of = |node: usize| {
        prototype
            .bodies
            .iter()
            .position(|b| b.node as usize == node)
    };
    let mut attached = Vec::new();
    for (i, node) in prototype.nodes.iter().enumerate() {
        if !super::spawn_appearance::node_active(prototype, i, &content.content.appearance) {
            continue;
        }
        let Some(attach) = rules.component(&reference.world, &node.source, "attachablescript")?
        else {
            continue;
        };
        let Some(target) = optional(attach.fields, "attachAtStartTarget")? else {
            continue;
        };
        let Some(body) = body_of(i) else {
            continue;
        };
        let root = rules.root(&reference.world, target)?;
        let mut node = prototype
            .nodes
            .iter()
            .position(|n| &n.source == root)
            .ok_or_else(|| ReferenceError("Generated attach target missing".into()))?;
        let target = loop {
            if let Some(b) = body_of(node) {
                break b;
            }
            node = prototype.nodes[node]
                .parent
                .ok_or_else(|| ReferenceError("Generated attach target has no body".into()))?
                as usize;
        };
        attached.push((body, target));
    }
    Ok(attached)
}

pub(super) fn nearest_owner(
    content: &ContentInstance,
    prototype: &WorldPrototypeReference,
    rules: &SourceRules,
    reference: &ReferenceManifest,
    node: usize,
) -> Result<Option<ItemId>, ReferenceError> {
    let mut node = Some(node);
    while let Some(i) = node {
        if i == 0 {
            return Ok(content.item);
        }
        if let Some(part) =
            rules.component(&reference.world, &prototype.nodes[i].source, "partscript")?
        {
            if let Some(binding) = content.bindings.iter().find(|b| &b.source == part.source) {
                return Ok(Some(binding.item));
            }
        }
        node = prototype.nodes[i].parent.map(|n| n as usize);
    }
    Ok(content.item)
}

/// Source `partscript.fallOffCategory`: a `mainscript.defFallOffChance` index.
pub(super) fn fall_off_category<'a>(f: impl FieldAccess<'a>) -> Result<u8, ReferenceError> {
    u8::try_from(integer(f, "fallOffCategory")?)
        .ok()
        .filter(|c| usize::from(*c) < FALL_OFF_CATEGORIES)
        .ok_or_else(|| ReferenceError("Invalid source fallOffCategory".into()))
}
