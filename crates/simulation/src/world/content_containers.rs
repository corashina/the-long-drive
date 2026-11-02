//! Nested containers use the existing fluid and opening owners.
use super::contents::{ContentBinding, ContentInstance};
use super::spawn_rules::*;
use super::{AbsolutePose, StructureInstance, SupplyDefinition, SupplyPoint};
use crate::interaction::{
    InteractionState,
    items::{ItemLocation, ItemStore},
};
use crate::reference::{ReferenceError, ReferenceManifest, SourceId, WorldPrototypeReference};
use crate::vehicle::{PhysicsWorld, from_rapier, to_rapier};

pub(super) fn bind(
    content: &mut ContentInstance,
    instance: &StructureInstance,
    prototype: &WorldPrototypeReference,
    rules: &SourceRules,
    reference: &ReferenceManifest,
    items: &mut ItemStore,
    interaction: &mut InteractionState,
    physics: &mut PhysicsWorld,
) -> Result<(), ReferenceError> {
    let Some(owner) = content.item else {
        return Ok(());
    };
    let placement = content.content.placement.clone();
    let world = &reference.world;
    let primary = rules
        .component(world, &placement.prototype, "tosaveitemscript")?
        .and_then(|s| array(s.fields, "tanks").ok()?.first())
        .and_then(|v| value_link(v).ok());
    for (index, node) in prototype.nodes.iter().enumerate() {
        if !super::spawn_appearance::node_active(prototype, index, &content.content.appearance) {
            continue;
        }
        let Some(cap) = rules.component(world, &node.source, "tankcapscript")? else {
            continue;
        };
        if !flag(cap.fields, "m_Enabled")? {
            continue;
        }
        let tank_source = link(cap.fields, "Tank")?;
        let tank = rules.get(world, tank_source)?;
        let tank_root = rules.root(world, tank_source)?;
        let tank_index = prototype
            .nodes
            .iter()
            .position(|n| &n.source == tank_root)
            .ok_or_else(|| ReferenceError("Generated cap tank lies outside its instance".into()))?;
        let tank_pose = super::placement::node_pose(prototype, tank_index);
        let cap_pose = super::placement::node_pose(prototype, index);
        let definition = SupplyDefinition {
            tank: tank_source.clone(),
            cap: cap.source.clone(),
            tank_pose,
            cap_pose,
            fluids: Vec::new(),
            capacity: number(fields(value(tank.fields, "F")?)?, "maxC")?,
            no_fill_from: flag(tank.fields, "noFillFrom")?,
            no_fill_this: flag(tank.fields, "noFillThis")?,
            no_pour: flag(cap.fields, "noPour")?,
            emission_min_rate: number(cap.fields, "minRate")?,
            emission_max_rate: number(cap.fields, "maxRate")?,
            explosion: super::TankExplosion::read(tank.fields, prototype)?,
        };
        let point = SupplyPoint::nested(
            if primary == Some(tank_source) {
                placement.pose
            } else {
                super::placement::place_pose(placement.pose, placement.scale, tank_pose)
            },
            super::placement::place_pose(placement.pose, placement.scale, cap_pose),
        );
        let tank_item = if primary == Some(tank_source) {
            owner
        } else {
            auxiliary(
                content,
                tank_source,
                point.tank_pose,
                prototype,
                items,
                physics,
                owner,
            )?
        };
        if items.get(tank_item).unwrap().fluid.is_none() {
            let fluid = if let Some((_, fluid)) = content
                .content
                .tanks
                .iter()
                .rev()
                .find(|(s, _)| s == tank_source)
            {
                fluid.clone()
            } else {
                let f = fields(value(tank.fields, "F")?)?;
                let mut entries = Vec::new();
                for v in array(f, "fluids")? {
                    let v = fields(v)?;
                    let kind = integer(v, "type")?;
                    if !(0..6).contains(&kind) {
                        return Err(ReferenceError("Unsupported source fluid identity".into()));
                    }
                    entries.push(crate::fluids::FluidEntry {
                        fluid_type: kind as u8,
                        amount: number(v, "amount")?,
                    });
                }
                if !crate::fluids::validate_fluid_entries(&entries) {
                    return Err(ReferenceError("Invalid source fluid entries".into()));
                }
                crate::fluids::FluidContainer::new(definition.capacity, &entries)
            };
            items.get_mut(tank_item).unwrap().fluid = Some(fluid);
        }
        let cap_item = auxiliary(
            content,
            cap.source,
            point.cap_pose,
            prototype,
            items,
            physics,
            owner,
        )?;
        interaction.register_generated_container(
            tank_item,
            cap_item,
            &definition,
            &point,
            reference,
        )?;
        let visual_node = |source: &SourceId| -> Result<crate::reference::NodeId, ReferenceError> {
            let root = rules.root(world, source)?;
            prototype
                .nodes
                .iter()
                .position(|n| &n.source == root)
                .map(|i| crate::reference::NodeId(i as u32))
                .ok_or_else(|| {
                    ReferenceError("Generated cap visual is outside its prototype".into())
                })
        };
        let visual_nodes = |name| {
            array(cap.fields, name)?
                .iter()
                .map(|v| visual_node(value_link(v)?))
                .collect::<Result<Vec<_>, ReferenceError>>()
        };
        let euler = |name| -> Result<[f32; 3], ReferenceError> {
            let [x, y, z] = vector(cap.fields, name)?;
            Ok([-x, -y, z])
        };
        content.caps.push(super::contents::GeneratedCapVisual {
            opening: interaction.generated_opening(cap_item).unwrap(),
            visual: crate::reference::CapVisualReference {
                open_nodes: visual_nodes("GOpen")?,
                closed_nodes: visual_nodes("GClosed")?,
                cap_node: optional(cap.fields, "TCap")?.map(visual_node).transpose()?,
                closed_node: optional(cap.fields, "TClosed")?
                    .map(visual_node)
                    .transpose()?,
                open_node: optional(cap.fields, "TOpen")?
                    .map(visual_node)
                    .transpose()?,
                euler_rotation: flag(cap.fields, "frot")?,
                closed_euler_degrees: euler("closedRot")?,
                open_euler_degrees: euler("openRot")?,
            },
        });
        for (source, handle) in &instance.colliders {
            // Source cap interaction is attached to its own GameObject.
            if rules.root(world, source)? == &node.source {
                interaction.bind_generated_cap(*handle, cap_item);
                let user_data = physics.rapier().colliders[*handle].user_data;
                physics.edit_collider(*handle, |collider| {
                    collider.user_data =
                        (user_data & !(u32::MAX as u128)) | (cap_item.0 as u128 + 1);
                });
            }
        }
    }
    for binding in &mut content.bindings {
        binding.baseline = items
            .generated_snapshot(binding.item, interaction.generated_valve(binding.item))
            .unwrap();
    }
    Ok(())
}
fn auxiliary(
    content: &mut ContentInstance,
    source: &SourceId,
    pose: AbsolutePose,
    prototype: &WorldPrototypeReference,
    items: &mut ItemStore,
    physics: &PhysicsWorld,
    owner: crate::reference::ItemId,
) -> Result<crate::reference::ItemId, ReferenceError> {
    if let Some(b) = content.bindings.iter().find(|b| &b.source == source) {
        return Ok(b.item);
    }
    let mut key = content.content.placement.id.clone();
    key.children.push((source.clone(), 0));
    let body = items.get(owner).unwrap().body;
    let item = items.insert_generated(key, prototype, pose)?;
    let state = items.set_body(item, body).unwrap();
    state.location = ItemLocation::Attached(owner);
    if let Some(body) = body {
        state.local_pose = from_rapier(
            &(physics.rapier().bodies[body].position().inverse() * to_rapier(state.pose)),
        );
    }
    content.bindings.push(ContentBinding {
        render_body: None,
        source: source.clone(),
        item,
        baseline: items.generated_snapshot(item, None).unwrap(),
    });
    Ok(item)
}
