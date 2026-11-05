//! Scene instances of the human monsters. The bone rows pose the Animator nodes of a live human
//! and the ragdoll bones of a dead one.
use super::creatures::CreatureScene;
use super::reference::ReferenceVisuals;
use super::scene_nodes::SceneGraph;
use super::sim_frame::SimFrameResource;
use crate::app::{Drive, RuntimeReference};
use bevy::ecs as bevy_ecs;
use bevy::prelude::*;
use bevy::world_serialization::{WorldAssetRoot, WorldInstanceReady};
use simulation::creatures::rig::{Rig, RigPose};
use simulation::vehicle::VehiclePose;
use std::collections::HashMap;

struct Instance {
    root: Entity,
    ready: bool,
    bound: bool,
    /// Binding failed. The human stays hidden and binding does not run again.
    failed: bool,
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
    rig: Option<Result<Rig, ()>>,
    /// Human id to scene instance.
    instances: HashMap<u32, Instance>,
    /// World session of `instances`.
    session: u64,
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
    graph: SceneGraph,
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
    if frame.new_session(&mut humans.session) {
        for (_, instance) in humans.instances.drain() {
            commands.entity(instance.root).despawn();
        }
    }
    // The renderer logs a rig error once and stops the human visuals.
    let rig = humans.rig.get_or_insert_with(|| {
        Rig::rest(&manifest.nodes, r.root)
            .inspect_err(|error| error!("Human rig: {error}"))
            .map_err(drop)
    });
    let Ok(rig) = rig.as_ref() else {
        return;
    };
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
                    failed: false,
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
        if instance.failed {
            continue;
        }
        if !instance.bound {
            let binding = graph.bind_instance(&mut commands, manifest, instance.root, r.asset);
            instance.nodes.extend(
                binding
                    .nodes
                    .iter()
                    .filter_map(|(id, entity)| Some((rig.index(*id)?, *entity))),
            );
            if instance.nodes.len() != rig.len() {
                error!(
                    "Human {}: {} rig nodes, {}",
                    row.id,
                    rig.len(),
                    binding.failure()
                );
                instance.failed = true;
                continue;
            }
            instance.primitives = binding.primitives;
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
