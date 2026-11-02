//! Generated rotation and slide controls use the existing mechanism owner.
use super::contents::{ContentBinding, ContentCollider, ContentInstance};
use super::spawn_rules::*;
use crate::interaction::{
    InteractionState,
    items::{ItemLocation, ItemStore},
};
use crate::reference::*;
use crate::vehicle::{PhysicsWorld, from_rapier, to_rapier};

pub(super) fn bind(
    content: &mut ContentInstance,
    instance: &mut super::StructureInstance,
    prototype: &WorldPrototypeReference,
    reference: &ReferenceManifest,
    rules: &SourceRules,
    items: &mut ItemStore,
    interaction: &mut InteractionState,
    physics: &mut PhysicsWorld,
) -> Result<(), ReferenceError> {
    if !reference
        .world
        .mechanisms
        .iter()
        .any(|r| r.prototype == prototype.source)
    {
        return Ok(());
    }
    let root_item = content.item.unwrap();
    if content.nodes.is_empty() {
        content.nodes = interaction.mechanisms.register_generated(
            prototype,
            items.get(root_item).unwrap().pose,
            content.content.placement.scale,
            root_item,
            0,
        )?;
    }
    if let Some(index) = prototype.bodies.iter().position(|body| body.node == 0) {
        interaction
            .mechanisms
            .set_part_root(content.nodes[0], Some(instance.bodies[index]));
    }
    for row in reference
        .world
        .mechanisms
        .iter()
        .filter(|r| r.prototype == prototype.source)
    {
        let reused = if row.node == 0 {
            Some(root_item)
        } else {
            content
                .bindings
                .iter()
                .find(|b| {
                    rules
                        .root(&reference.world, &b.source)
                        .is_ok_and(|r| *r == prototype.nodes[row.node as usize].source)
                })
                .map(|b| b.item)
        };
        let item = if let Some(item) = reused {
            item
        } else {
            let mut key = content.content.placement.id.clone();
            key.children.push((row.recipe.source.clone(), 0));
            let pose = super::placement::place_pose(
                content.content.placement.pose,
                content.content.placement.scale,
                super::placement::node_pose(prototype, row.node as usize),
            );
            let item = items.insert_generated(key, prototype, pose)?;
            items.get_mut(item).unwrap().location = ItemLocation::Attached(root_item);
            content.bindings.push(ContentBinding {
                source: row.recipe.source.clone(),
                item,
                baseline: items.generated_snapshot(item, None).unwrap(),
                render_body: None,
            });
            interaction
                .mechanisms
                .register_generated_item(item, content.nodes[row.node as usize]);
            item
        };
        let body = if let Some(body) = items
            .get(item)
            .unwrap()
            .body
            .or(items.get(root_item).unwrap().body)
        {
            body
        } else {
            let body = physics.insert_body(
                rapier3d::prelude::RigidBodyBuilder::kinematic_position_based()
                    .pose(to_rapier(items.get(root_item).unwrap().pose))
                    .build(),
            );
            instance.bodies.push(body);
            instance.cpu_bytes += 2048;
            items.set_body(root_item, Some(body));
            body
        };
        items.set_body(item, Some(body));
        interaction.register_generated_mechanism(item, body, &[])?;
    }
    // Wheel subtrees follow the suspension. The wheel slots place their colliders.
    let wheels = prototype
        .nodes
        .iter()
        .map(|n| {
            Ok(rules
                .component(&reference.world, &n.source, "wheelscript")?
                .is_some())
        })
        .collect::<Result<Vec<_>, ReferenceError>>()?;
    let under_wheel = |mut node: usize| loop {
        if wheels[node] {
            break true;
        }
        match prototype.nodes[node].parent {
            Some(parent) => node = parent as usize,
            None => break false,
        }
    };
    for (source, handle) in &instance.colliders {
        let collider = prototype
            .colliders
            .iter()
            .find(|c| &c.source == source)
            .unwrap();
        let node = collider.node as usize;
        // Part shapes follow their part. The part attachments place them.
        if under_wheel(node) || interaction.parts.owns_collider(*handle) {
            continue;
        }
        let pose = interaction
            .mechanisms
            .node_pose(content.nodes[node])
            .unwrap();
        content.mechanism_colliders.push(ContentCollider {
            handle: *handle,
            node,
            local: to_rapier(pose).inverse() * *physics.rapier().colliders[*handle].position(),
        });
    }
    let item_for = |node: usize| -> Result<ItemId, ReferenceError> {
        node_item(content, prototype, &reference.world, rules, node)
            .ok_or_else(|| ReferenceError("Generated control item missing".into()))
    };
    for row in reference
        .world
        .mechanisms
        .iter()
        .filter(|r| r.prototype == prototype.source)
    {
        let mut recipe = row.recipe.clone();
        let local = |n: NodeId| content.nodes[n.0 as usize - 1];
        recipe.item = item_for(row.node as usize)?;
        recipe.id = MechanismId(recipe.item.0);
        recipe.parent_item = recipe
            .parent_item
            .and_then(|p| item_for(p.0 as usize - 1).ok())
            .or(Some(root_item));
        recipe.child = recipe
            .child
            .map(|i| item_for(i.0 as usize - 1))
            .transpose()?;
        for n in &mut recipe.driven_nodes {
            *n = local(*n)
        }
        recipe.drag.node = local(recipe.drag.node);
        recipe.drag.node2 = local(recipe.drag.node2);
        if let EndpointReactionReference::Rotation { node, .. } = &mut recipe.reaction {
            *node = local(*node);
        }
        if let Some(r) = &mut recipe.rotation {
            for target in &mut r.targets {
                target.node = local(target.node)
            }
        }
        if let Some(s) = &mut recipe.slide {
            for path in &mut s.paths {
                path.node = local(path.node);
                for p in &mut path.points {
                    p.node = local(p.node)
                }
            }
            for r in &mut s.rotations {
                r.node = local(r.node);
                r.reference = local(r.reference)
            }
        }
        interaction
            .parts
            .set_generated_usable(recipe.item, recipe.item);
        for (source, handle) in &instance.colliders {
            let c = prototype
                .colliders
                .iter()
                .find(|c| &c.source == source)
                .unwrap();
            let mut node = Some(c.node);
            while let Some(index) = node {
                if let Some(closest) = reference
                    .world
                    .mechanisms
                    .iter()
                    .find(|r| r.prototype == prototype.source && r.node == index)
                {
                    if closest.node == row.node {
                        let mut id = ColliderId(
                            (physics.rapier().colliders[*handle].user_data >> 64) as u32,
                        );
                        if id.0 == 0 {
                            id = interaction.allocate_generated_collider()?;
                            physics.edit_collider(*handle, |collider| {
                                collider.user_data |= (id.0 as u128) << 64;
                            });
                        }
                        interaction.bind_generated_mechanism_surface(recipe.item, *handle, id);
                    }
                    break;
                }
                node = prototype.nodes[index as usize].parent;
            }
        }
        let item = recipe.item;
        interaction.mechanisms.register_generated_control(recipe);
        if interaction.parts.contains(item) {
            interaction.mechanisms.set_mounted(item, false);
        }
    }
    content.poses.reserve_exact(content.nodes.len());
    for n in &content.nodes {
        content
            .poses
            .push(interaction.mechanisms.node_pose(*n).unwrap())
    }
    Ok(())
}
/// The generated item of a prototype node: the root item for node 0, else the binding under the node.
pub(super) fn node_item(
    content: &ContentInstance,
    prototype: &WorldPrototypeReference,
    world: &WorldReference,
    rules: &SourceRules,
    node: usize,
) -> Option<ItemId> {
    if node == 0 {
        return content.item;
    }
    content
        .bindings
        .iter()
        .find(|b| {
            rules
                .root(world, &b.source)
                .is_ok_and(|s| *s == prototype.nodes[node].source)
        })
        .map(|b| b.item)
}
pub(super) fn sync(
    content: &mut ContentInstance,
    items: &mut ItemStore,
    interaction: &InteractionState,
    physics: &mut PhysicsWorld,
) {
    if content.poses.is_empty() {
        return;
    }
    for (node, pose) in content.nodes.iter().zip(&mut content.poses) {
        let next = interaction.mechanisms.node_pose(*node).unwrap();
        if *pose != next {
            *pose = next;
            content.visual_revision = content.visual_revision.wrapping_add(1);
        }
    }
    for binding in &content.bindings {
        if binding.render_body.is_none() && interaction.mechanisms.contains(binding.item) {
            if let Some(node) = interaction.mechanisms.item_node(binding.item) {
                let state = items.get_mut(binding.item).unwrap();
                state.pose = interaction.mechanisms.node_pose(node).unwrap();
                if let Some(body) = state.body {
                    state.local_pose = from_rapier(
                        &(physics.rapier().bodies[body].position().inverse()
                            * to_rapier(state.pose)),
                    );
                }
            }
        }
    }
    for c in &content.mechanism_colliders {
        let world = to_rapier(content.poses[c.node]) * c.local;
        let parent = physics.rapier().colliders[c.handle].parent();
        if let Some(parent) = parent {
            let local = physics.rapier().bodies[parent].position().inverse() * world;
            if crate::vehicle::pose_changed(
                physics.rapier().colliders[c.handle].position_wrt_parent(),
                &local,
            ) {
                physics.edit_collider(c.handle, |collider| collider.set_position_wrt_parent(local));
            }
        } else if physics.rapier().colliders[c.handle].position() != &world {
            physics.edit_collider(c.handle, |collider| collider.set_position(world));
        }
    }
}
