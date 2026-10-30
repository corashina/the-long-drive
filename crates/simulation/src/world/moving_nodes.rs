//! Prototype nodes that runtime code writes after spawn.
use crate::reference::{ReferenceError, ReferenceManifest, WorldPrototypeReference};
use crate::world::spawn_rules::*;
/// Prototype nodes whose transform or visibility runtime code writes after spawn.
/// Presentation spawns entities only for these nodes. Every node writer must use a node from this list.
/// Renderers carry node activity (food parts, appearance) themselves, so it is not in this list.
pub(super) fn moving_nodes_of(
    rules: &SourceRules,
    reference: &ReferenceManifest,
    prototype: &WorldPrototypeReference,
) -> Result<Vec<bool>, ReferenceError> {
    let world = &*reference.world;
    let mut moving = vec![false; prototype.nodes.len()];
    // Item scripts, tank caps, spirit levels and mechanisms move or show many nodes.
    // Radio needle, music box, metal detector, dial, cap and valve nodes, level bubbles, mechanism poses.
    let mut scripts = Vec::new();
    for class in [
        "radioscript",
        "musicboxscript",
        "metalDetectorScript",
        "compassscript",
        "clockscript",
        "tankcapscript",
    ] {
        rules.children(world, &prototype.source, class, true, &mut scripts)?;
    }
    // Cars move wheels, doors, needles, pedals and show or hide parts. Guns move and hide parts.
    if !scripts.is_empty()
        || prototype.vehicle.is_some()
        || prototype.source == reference.small_items.spirit_level.prototype
        || reference
            .guns
            .weapons
            .iter()
            .any(|w| w.prototype.0 == prototype.source)
        || world
            .mechanisms
            .iter()
            .any(|m| m.prototype == prototype.source)
    {
        moving.fill(true);
        return Ok(moving);
    }
    for body in &prototype.bodies {
        moving[body.node as usize] = true;
    }
    // Follower nodes: Bus02 springs, axles and rams.
    if let Some(followers) = &prototype.followers {
        for f in &followers.followers {
            moving[f.node() as usize] = true;
        }
    }
    // wheelgraphicsscript.T of trailer wheels.
    if let Some(trailer) = &prototype.trailer {
        for &node in &trailer.graphics {
            moving[node as usize] = true;
        }
    }
    // Bat wing bones.
    if prototype.source == reference.sky.flyer.prefab {
        for track in &reference.sky.flyer.wing.tracks {
            moving[track.node as usize] = true;
        }
    }
    // Spray bottle trigger.
    let trigger = &reference.small_items.spray_bottle.trigger;
    // Lamp buttons: flashlightscript GButton.
    let mut lamps = Vec::new();
    rules.children(
        world,
        &prototype.source,
        "flashlightscript",
        true,
        &mut lamps,
    )?;
    let mut buttons = Vec::with_capacity(lamps.len());
    for lamp in lamps {
        if let Some(button) = optional(rules.get(world, lamp)?.fields, "GButton")? {
            buttons.push(button);
        }
    }
    // Loot brush models slide on their contact.
    let model = reference
        .conditions
        .prototype_tools
        .iter()
        .find(|t| t.prototype == prototype.source)
        .and_then(|t| match &t.kind {
            crate::reference::PrototypeTool::Brush { nodes, .. } => nodes.model.as_ref(),
            crate::reference::PrototypeTool::Spray { .. } => None,
        });
    for (node, flag) in prototype.nodes.iter().zip(&mut moving) {
        *flag |= node.source == *trigger
            || buttons.contains(&&node.source)
            || model == Some(&node.source);
    }
    Ok(moving)
}
