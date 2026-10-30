//! Scene instances of the rabbits. Live nodes take the Animator frames of their row. Dead
//! rabbits take their bone rows. The GPU skins the meshes. Evil rabbits swap the eye material.
use super::materials::{SourceMaterial, SourceMaterials, authored_material};
use super::reference::{ReferenceVisuals, bind_node};
use super::sim_frame::SimFrameResource;
use crate::app::{Drive, RuntimeReference};
use crate::reference::SourceId;
use bevy::ecs as bevy_ecs;
use bevy::gltf::GltfExtras;
use bevy::prelude::*;
use bevy::world_serialization::{WorldAssetRoot, WorldInstanceReady};
use simulation::boundary::{CreatureRow, PoseRow};
use simulation::creatures::animator::{Action, ActionPose};
use simulation::creatures::rig::{Rig, RigPose};
use simulation::vehicle::VehiclePose;
use std::collections::HashMap;

#[derive(Component)]
pub(crate) struct CreatureScene;

struct Instance {
    root: Entity,
    ready: bool,
    bound: bool,
    shown: bool,
    /// Rig node index and entity.
    nodes: Vec<(usize, Entity)>,
    primitives: Vec<Entity>,
    /// Primitives under the eye objects.
    eyes: Vec<Entity>,
    /// The eye flags of the current eye material.
    eye_look: u32,
    pose: RigPose,
    /// Bone rows of a dead rabbit: rig node and pose.
    bones: Vec<(usize, rapier3d::math::Pose)>,
}

#[derive(Resource, Default)]
pub struct CreatureVisuals {
    rig: Option<Rig>,
    /// Rabbit id to scene instance.
    instances: HashMap<u32, Instance>,
    /// Converted eye materials by material, mirror and receiver mask.
    eye_materials: Vec<(
        (crate::reference::AssetId, u32),
        bool,
        u32,
        Handle<SourceMaterial>,
    )>,
    generation: u64,
}

impl CreatureVisuals {}

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
    mut creatures: ResMut<CreatureVisuals>,
    extras: Query<(Entity, &GltfExtras)>,
    parents: Query<&ChildOf>,
    primitives: Query<(Entity, &ChildOf), With<Mesh3d>>,
    source_materials: Query<(), With<MeshMaterial3d<SourceMaterial>>>,
    eye_materials: Query<(&MeshMaterial3d<SourceMaterial>, &GlobalTransform)>,
    authored: Res<SourceMaterials>,
    mut materials: ResMut<Assets<SourceMaterial>>,
    mut transforms: Query<&mut Transform>,
    mut visibility: Query<&mut Visibility>,
) {
    if !visuals.bound {
        return;
    }
    let manifest = &reference.0;
    let r = &manifest.creatures.rabbit;
    let creatures = &mut *creatures;
    if creatures.rig.is_none() {
        match Rig::new(&manifest.nodes, r.root, &r.animator) {
            Ok(rig) => creatures.rig = Some(rig),
            Err(error) => {
                error!("Rabbit rig: {error}");
                return;
            }
        }
    }
    let rig = creatures.rig.as_ref().unwrap();
    let rows = &frame.frame.creatures;
    creatures.instances.retain(|id, instance| {
        let keep = rows.iter().any(|row| row.id == *id);
        if !keep {
            commands.entity(instance.root).despawn();
        }
        keep
    });
    for row in rows {
        let Some(instance) = creatures.instances.get_mut(&row.id) else {
            let Some(gltf) = visuals
                .gltfs
                .iter()
                .find(|(id, _)| *id == r.asset)
                .and_then(|(_, handle)| gltfs.get(handle))
            else {
                continue;
            };
            let (id, generation) = (row.id, creatures.generation);
            let root = commands
                .spawn((
                    CreatureScene,
                    WorldAssetRoot(gltf.scenes[0].clone()),
                    Transform::default(),
                    Visibility::Hidden,
                ))
                .observe(
                    move |event: On<WorldInstanceReady>, mut creatures: ResMut<CreatureVisuals>| {
                        if creatures.generation != generation {
                            return;
                        }
                        if let Some(instance) = creatures.instances.get_mut(&id)
                            && instance.root == event.entity
                        {
                            instance.ready = true;
                        }
                    },
                )
                .id();
            creatures.instances.insert(
                row.id,
                Instance {
                    root,
                    ready: false,
                    bound: false,
                    shown: false,
                    nodes: Vec::with_capacity(rig.len()),
                    primitives: Vec::new(),
                    eyes: Vec::new(),
                    eye_look: 0,
                    pose: rig.pose(),
                    bones: Vec::with_capacity(r.ragdoll.bodies.len()),
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
            let eye_nodes: Vec<Entity> = r
                .senses
                .eye_objects
                .iter()
                .filter_map(|node| rig.index(*node))
                .filter_map(|index| instance.nodes.iter().find(|(i, _)| *i == index))
                .map(|(_, entity)| *entity)
                .collect();
            for (primitive, _) in &primitives {
                if descends(primitive, instance.root, &parents) {
                    instance.primitives.push(primitive);
                    if eye_nodes
                        .iter()
                        .any(|eye| descends(primitive, *eye, &parents))
                    {
                        instance.eyes.push(primitive);
                    }
                }
            }
            instance.bound = true;
        }
        if !instance.shown {
            // Show the rabbit once its meshes use source materials, as other source meshes do.
            if instance.primitives.is_empty()
                || !instance
                    .primitives
                    .iter()
                    .all(|primitive| source_materials.contains(*primitive))
            {
                continue;
            }
            instance.shown = true;
        }
        let visible = row.flags & PoseRow::VISIBLE != 0;
        if let Ok(mut v) = visibility.get_mut(instance.root) {
            v.set_if_neq(if visible {
                Visibility::Inherited
            } else {
                Visibility::Hidden
            });
        }
        if !visible {
            continue;
        }
        let pose = drive.rendered_body_pose(VehiclePose {
            position: row.position,
            rotation: row.rotation,
        });
        if let Ok(mut transform) = transforms.get_mut(instance.root) {
            transform.set_if_neq(super::transform(pose).with_scale(Vec3::splat(row.scale)));
        }
        if row.flags & CreatureRow::DEAD != 0 {
            let [start, count] = row.bones.map(|v| v as usize);
            instance.bones.clear();
            instance
                .bones
                .extend(
                    frame.frame.creature_bones[start..start + count]
                        .iter()
                        .map(|b| {
                            (
                                b.node as usize,
                                rapier3d::math::Pose::from_parts(
                                    rapier3d::math::Vector::from_array(b.position),
                                    rapier3d::math::Rotation::from_array(b.rotation),
                                ),
                            )
                        }),
                );
            rig.ragdoll_pose(&instance.bones, row.scale, &mut instance.pose);
        } else {
            let action = match row.action {
                CreatureRow::ROAR => Some(Action::Roar),
                CreatureRow::ATTACK => Some(Action::Attack),
                _ => None,
            }
            .map(|action| ActionPose {
                action,
                time: row.action_time,
                weight: row.action_weight,
            });
            rig.sample(
                &r.animator,
                row.phase,
                row.value,
                action,
                row.twitch,
                1.,
                &mut instance.pose,
            );
        }
        // `GetEvil` sets `EyeEvil` or `EyeGreen` on the eye renderers.
        let look = row.flags & (CreatureRow::EVIL_EYES | CreatureRow::GREEN_EYES);
        if look != instance.eye_look {
            let link = if look & CreatureRow::GREEN_EYES != 0 {
                &r.evil.eye_materials.green
            } else {
                &r.evil.eye_materials.evil
            };
            let key = (link.asset, link.gltf_material);
            for primitive in &instance.eyes {
                let Ok((current, global)) = eye_materials.get(*primitive) else {
                    continue;
                };
                let Some(mask) = materials.get(&current.0).map(|m| m.extension.receiver_mask)
                else {
                    continue;
                };
                let reflected = global.affine().matrix3.determinant() < 0.;
                let cached = creatures
                    .eye_materials
                    .iter()
                    .find(|(k, f, m, _)| (*k, *f, *m) == (key, reflected, mask))
                    .map(|(.., handle)| handle.clone());
                let handle = cached.unwrap_or_else(|| {
                    let handle = materials.add(authored_material(key, reflected, mask, &authored));
                    creatures
                        .eye_materials
                        .push((key, reflected, mask, handle.clone()));
                    handle
                });
                commands.entity(*primitive).insert(MeshMaterial3d(handle));
            }
            instance.eye_look = look;
        }
        // `LooseEyes` deactivates the eye objects.
        let eyes = if row.flags & CreatureRow::NO_EYES != 0 {
            Visibility::Hidden
        } else {
            Visibility::Inherited
        };
        for node in &r.senses.eye_objects {
            let Some(index) = rig.index(*node) else {
                continue;
            };
            if let Some((_, entity)) = instance.nodes.iter().find(|(i, _)| *i == index)
                && let Ok(mut v) = visibility.get_mut(*entity)
            {
                v.set_if_neq(eyes);
            }
        }
        for (index, entity) in &instance.nodes {
            let Ok(mut transform) = transforms.get_mut(*entity) else {
                continue;
            };
            // The scene root carries the root pose and scale.
            let local = if *index == 0 {
                Transform::IDENTITY
            } else {
                let f = &instance.pose.local[*index];
                Transform {
                    translation: Vec3::from_array(f.position.to_array()),
                    rotation: Quat::from_array(f.rotation.to_array()),
                    scale: Vec3::from_array(f.scale.to_array()),
                }
            };
            transform.set_if_neq(local);
        }
    }
}
