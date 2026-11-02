//! Scene instances of the human monsters. The bone rows pose the Animator nodes of a live human
//! and the ragdoll bones of a dead one.
use super::creatures::CreatureScene;
use super::reference::{ReferenceVisuals, bind_node};
use super::sim_frame::SimFrameResource;
use crate::app::{Drive, RuntimeReference};
use crate::reference::SourceId;
use bevy::ecs as bevy_ecs;
use bevy::gltf::GltfExtras;
use bevy::prelude::*;
use bevy::world_serialization::{WorldAssetRoot, WorldInstanceReady};
use simulation::creatures::rig::{Rig, RigPose};
use simulation::vehicle::VehiclePose;
use std::collections::HashMap;

struct Instance {
    root: Entity,
    ready: bool,
    bound: bool,
    shown: bool,
    /// Rig node index and entity.
    nodes: Vec<(usize, Entity)>,
    primitives: Vec<Entity>,
    pose: RigPose,
    /// Bone rows: rig node and pose.
    bones: Vec<(usize, rapier3d::math::Pose)>,
}

#[derive(Resource, Default)]
pub struct HumanVisuals {
    rig: Option<Rig>,
    /// Human id to scene instance.
    instances: HashMap<u32, Instance>,
}

#[derive(serde::Deserialize)]
struct Identity {
    reference_id: Option<SourceId>,
}

fn descends(mut entity: Entity, root: Entity, parents: &Query<&ChildOf>) -> bool {
    while let Ok(link) = parents.get(entity) {
        entity = link.parent();
        if entity == root {
            return true;
        }
    }
    false
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn sync(
    mut commands: Commands,
    reference: Res<RuntimeReference>,
    drive: Res<Drive>,
    frame: Res<SimFrameResource>,
    visuals: Res<ReferenceVisuals>,
    gltfs: Res<Assets<bevy::gltf::Gltf>>,
    mut humans: ResMut<HumanVisuals>,
    extras: Query<(Entity, &GltfExtras)>,
    parents: Query<&ChildOf>,
    primitives: Query<(Entity, &ChildOf), With<Mesh3d>>,
    source_materials: Query<(), With<MeshMaterial3d<super::materials::SourceMaterial>>>,
    mut transforms: Query<&mut Transform>,
    mut visibility: Query<&mut Visibility>,
) {
    if !visuals.bound {
        return;
    }
    let manifest = &reference.0;
    let r = &manifest.creatures.human;
    let humans = &mut *humans;
    if humans.rig.is_none() {
        match Rig::rest(&manifest.nodes, r.root) {
            Ok(rig) => humans.rig = Some(rig),
            Err(error) => {
                error!("Human rig: {error}");
                return;
            }
        }
    }
    let rig = humans.rig.as_ref().unwrap();
    let rows = &frame.frame.humans;
    humans.instances.retain(|id, instance| {
        let keep = rows.iter().any(|row| row.id == *id);
        if !keep {
            commands.entity(instance.root).despawn();
        }
        keep
    });
    for row in rows {
        let Some(instance) = humans.instances.get_mut(&row.id) else {
            let Some(gltf) = visuals
                .gltfs
                .iter()
                .find(|(id, _)| *id == r.asset)
                .and_then(|(_, handle)| gltfs.get(handle))
            else {
                continue;
            };
            let id = row.id;
            let root = commands
                .spawn((
                    CreatureScene,
                    WorldAssetRoot(gltf.scenes[0].clone()),
                    Transform::default(),
                    Visibility::Hidden,
                ))
                .observe(
                    move |event: On<WorldInstanceReady>, mut humans: ResMut<HumanVisuals>| {
                        if let Some(instance) = humans.instances.get_mut(&id)
                            && instance.root == event.entity
                        {
                            instance.ready = true;
                        }
                    },
                )
                .id();
            humans.instances.insert(
                row.id,
                Instance {
                    root,
                    ready: false,
                    bound: false,
                    shown: false,
                    nodes: Vec::with_capacity(rig.len()),
                    primitives: Vec::new(),
                    pose: rig.pose(),
                    bones: Vec::with_capacity(rig.len()),
                },
            );
            continue;
        };
        if !instance.ready {
            continue;
        }
        if !instance.bound {
            for (entity, extra) in &extras {
                if !descends(entity, instance.root, &parents) {
                    continue;
                }
                let Ok(identity) = serde_json::from_str::<Identity>(&extra.value) else {
                    continue;
                };
                let Some(node) = identity.reference_id.and_then(|source| {
                    manifest
                        .nodes
                        .iter()
                        .find(|node| node.source == source && node.asset == r.asset)
                }) else {
                    continue;
                };
                let Some(index) = rig.index(node.id) else {
                    continue;
                };
                if !instance.nodes.iter().any(|(i, _)| *i == index)
                    && bind_node(&mut commands, node, entity, extra, &primitives).is_ok()
                {
                    instance.nodes.push((index, entity));
                }
            }
            if instance.nodes.len() != rig.len() {
                continue;
            }
            for (primitive, _) in &primitives {
                if descends(primitive, instance.root, &parents) {
                    instance.primitives.push(primitive);
                }
            }
            // The scene root carries the root pose.
            if let Some((_, entity)) = instance.nodes.iter().find(|(i, _)| *i == 0)
                && let Ok(mut transform) = transforms.get_mut(*entity)
            {
                *transform = Transform::IDENTITY;
            }
            instance.bound = true;
        }
        if !instance.shown {
            // Show the human once its meshes use source materials, as other source meshes do.
            if instance.primitives.is_empty()
                || !instance
                    .primitives
                    .iter()
                    .all(|primitive| source_materials.contains(*primitive))
            {
                continue;
            }
            instance.shown = true;
            if let Ok(mut v) = visibility.get_mut(instance.root) {
                *v = Visibility::Inherited;
            }
        }
        let pose = drive.rendered_body_pose(VehiclePose {
            position: row.position,
            rotation: row.rotation,
        });
        if let Ok(mut transform) = transforms.get_mut(instance.root) {
            transform.set_if_neq(super::transform(pose));
        }
        let [start, count] = row.bones.map(|v| v as usize);
        instance.bones.clear();
        instance.bones.extend(
            frame.frame.human_bones[start..start + count]
                .iter()
                .filter_map(|b| {
                    Some((
                        rig.index(simulation::reference::NodeId(b.node))?,
                        rapier3d::math::Pose::from_parts(
                            rapier3d::math::Vector::from_array(b.position),
                            rapier3d::math::Rotation::from_array(b.rotation),
                        ),
                    ))
                }),
        );
        rig.ragdoll_pose(&instance.bones, 1., &mut instance.pose);
        for (index, entity) in &instance.nodes {
            if *index == 0 {
                continue;
            }
            let Ok(mut transform) = transforms.get_mut(*entity) else {
                continue;
            };
            let f = &instance.pose.local[*index];
            transform.set_if_neq(Transform {
                translation: Vec3::from_array(f.position.to_array()),
                rotation: Quat::from_array(f.rotation.to_array()),
                scale: Vec3::from_array(f.scale.to_array()),
            });
        }
    }
}
