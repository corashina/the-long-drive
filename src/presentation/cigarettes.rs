//! Cigarette ember emission, ember light, and burn-down parts (`emberScript`, `UpdParts`).
use super::lighting::{SourceLighting, ember_light};
use super::materials::SourceMaterial;
use super::sim_frame::SimFrameResource;
use super::spawned::{SpawnedScene, SpawnedVisuals};
use crate::app::RuntimeReference;
use crate::interaction::cigarettes::part_pose;
use crate::reference::{ItemId, NodeId, Transform as SourceTransform};
use bevy::prelude::*;

/// Checked cigarette nodes: each part with its start and end local pose.
pub(crate) struct CigaretteNodes {
    parts: Vec<(NodeId, SourceTransform, SourceTransform)>,
    renderer: NodeId,
    light: Option<NodeId>,
}

impl CigaretteNodes {
    fn new(reference: &crate::reference::ReferenceManifest) -> Option<Self> {
        let consumable = reference
            .item_prototypes
            .iter()
            .find(|p| p.item_id == Some(reference.acquisition.cigarette_item_id))?
            .consumable
            .as_ref()?;
        let local = |id: NodeId| {
            reference
                .nodes
                .iter()
                .find(|n| n.id == id)
                .map(|n| n.transform)
        };
        let parts = consumable
            .parts
            .iter()
            .zip(&consumable.start)
            .zip(&consumable.end)
            .map(|((part, start), end)| Some((*part, local(*start)?, local(*end)?)))
            .collect::<Option<Vec<_>>>()?;
        Some(Self {
            parts,
            renderer: consumable.ember.renderer,
            light: consumable.ember.light.node,
        })
    }
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn sync(
    reference: Res<RuntimeReference>,
    frame: Res<SimFrameResource>,
    mut nodes: Local<Option<CigaretteNodes>>,
    mut spawned: ResMut<SpawnedVisuals>,
    mut assets: ResMut<Assets<SourceMaterial>>,
    mut bound: Query<(&ChildOf, &mut MeshMaterial3d<SourceMaterial>)>,
    mut transforms: Query<&mut Transform, Without<SpawnedScene>>,
    mut lighting: ResMut<SourceLighting>,
) {
    let manifest = &reference.0;
    if nodes.is_none() {
        *nodes = CigaretteNodes::new(manifest);
    }
    let Some(nodes) = nodes.as_ref() else {
        return;
    };
    for row in &frame.frame.cigarettes {
        let id = &ItemId(row.item);
        let blend = row.blend;
        for (part, start, end) in &nodes.parts {
            let Some(entity) = spawned.node(*id, *part) else {
                continue;
            };
            if let Ok(mut transform) = transforms.get_mut(entity) {
                let pose = part_pose(start, end, blend);
                transform.set_if_neq(Transform {
                    translation: Vec3::from_array(pose.translation),
                    rotation: Quat::from_array(pose.rotation),
                    scale: Vec3::from_array(pose.scale),
                });
            }
        }
        if let Err(error) = spawned.emission(
            *id,
            nodes.renderer,
            row.color,
            None,
            &mut assets,
            &mut bound,
        ) {
            error!("Cigarette ember emission: {error}");
        }
    }
    // One ember light: the worn cigarette, then the held one, then any lit one.
    let Some(record) = ember_light(manifest) else {
        return;
    };
    let chosen = frame.frame.state.ember_light.checked_sub(1).map(ItemId);
    let Some(light) = lighting
        .lights
        .iter_mut()
        .find(|b| b.id == record.id && b.world.is_some())
    else {
        return;
    };
    let node = chosen.and_then(|id| nodes.light.and_then(|n| spawned.node(id, n)));
    let world = light.world.as_mut().unwrap();
    match (chosen, node) {
        (Some(id), Some(node)) => {
            light.item = Some(id);
            light.node = Some(node);
            world.output.enabled = true;
            world.output.intensity = frame
                .frame
                .cigarettes
                .iter()
                .find(|row| row.item == id.0)
                .map_or(0., |row| row.light_intensity);
        }
        _ => {
            light.item = None;
            world.output.enabled = false;
        }
    }
}
