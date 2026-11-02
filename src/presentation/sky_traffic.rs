//! Scene instances of the sky traffic planes. Planes have no bodies.
use super::materials::SourceMaterial;
use super::reference::{ReferenceVisuals, bind_node};
use super::sim_frame::SimFrameResource;
use crate::app::RuntimeReference;
use crate::reference::{NodeId, SourceId};
use bevy::ecs as bevy_ecs;
use bevy::gltf::GltfExtras;
use bevy::prelude::*;
use bevy::world_serialization::{WorldAssetRoot, WorldInstanceReady};
use std::collections::HashMap;

#[derive(Component)]
pub(crate) struct SkyScene;

struct SkyInstance {
    root: Entity,
    prefab: usize,
    ready: bool,
    bound: bool,
    shown: bool,
    nodes: Vec<(NodeId, Entity)>,
    primitives: Vec<Entity>,
    /// The spin node and its glTF rotation.
    spin: Option<(Entity, Quat)>,
}

#[derive(Resource, Default)]
pub struct SkyTrafficVisuals {
    /// Plane id to scene instance.
    instances: HashMap<u32, SkyInstance>,
    generation: u64,
}

impl SkyTrafficVisuals {}

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
    frame: Res<SimFrameResource>,
    visuals: Res<ReferenceVisuals>,
    gltfs: Res<Assets<bevy::gltf::Gltf>>,
    mut sky: ResMut<SkyTrafficVisuals>,
    extras: Query<(Entity, &GltfExtras)>,
    parents: Query<&ChildOf>,
    primitives: Query<(Entity, &ChildOf), With<Mesh3d>>,
    source_materials: Query<(), With<MeshMaterial3d<SourceMaterial>>>,
    mut transforms: Query<&mut Transform>,
    mut visibility: Query<&mut Visibility, With<SkyScene>>,
) {
    if !visuals.bound {
        return;
    }
    let manifest = &reference.0;
    let planes = &frame.frame.planes;
    let sky = &mut *sky;
    sky.instances.retain(|id, instance| {
        let keep = planes
            .iter()
            .any(|p| p.id == *id && p.prefab as usize == instance.prefab);
        if !keep {
            commands.entity(instance.root).despawn();
        }
        keep
    });
    let anchor = Vec3::from_array(frame.frame.state.sky_anchor);
    for plane in planes {
        let prefab = &manifest.sky.prefabs[plane.prefab as usize];
        let Some(instance) = sky.instances.get_mut(&plane.id) else {
            let Some((_, handle)) = visuals.gltfs.iter().find(|(id, _)| *id == prefab.asset) else {
                continue;
            };
            let Some(gltf) = gltfs.get(handle) else {
                continue;
            };
            let (id, generation) = (plane.id, sky.generation);
            let root = commands
                .spawn((
                    SkyScene,
                    WorldAssetRoot(gltf.scenes[0].clone()),
                    Transform::default(),
                    Visibility::Hidden,
                ))
                .observe(
                    move |event: On<WorldInstanceReady>, mut sky: ResMut<SkyTrafficVisuals>| {
                        if sky.generation != generation {
                            return;
                        }
                        if let Some(instance) = sky.instances.get_mut(&id)
                            && instance.root == event.entity
                        {
                            instance.ready = true;
                        }
                    },
                )
                .id();
            sky.instances.insert(
                plane.id,
                SkyInstance {
                    root,
                    prefab: plane.prefab as usize,
                    ready: false,
                    bound: false,
                    shown: false,
                    nodes: Vec::new(),
                    primitives: Vec::new(),
                    spin: None,
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
                        .find(|node| node.source == source && node.asset == prefab.asset)
                }) else {
                    continue;
                };
                if !instance.nodes.iter().any(|(id, _)| *id == node.id)
                    && bind_node(&mut commands, node, entity, extra, &primitives).is_ok()
                {
                    instance.nodes.push((node.id, entity));
                }
            }
            let count = manifest
                .nodes
                .iter()
                .filter(|node| node.asset == prefab.asset)
                .count();
            if instance.nodes.len() != count {
                continue;
            }
            for (primitive, _) in &primitives {
                if descends(primitive, instance.root, &parents) {
                    instance.primitives.push(primitive);
                }
            }
            instance.spin = prefab.spin.as_ref().and_then(|spin| {
                let (_, entity) = instance.nodes.iter().find(|(id, _)| *id == spin.node)?;
                Some((*entity, transforms.get(*entity).ok()?.rotation))
            });
            instance.bound = true;
        }
        if !instance.shown {
            // Show the plane once its meshes use source materials, as other source meshes do.
            if instance.primitives.is_empty()
                || !instance
                    .primitives
                    .iter()
                    .all(|primitive| source_materials.contains(*primitive))
            {
                continue;
            }
            if let Ok(mut v) = visibility.get_mut(instance.root) {
                *v = Visibility::Inherited;
            }
            instance.shown = true;
        }
        if let Ok(mut transform) = transforms.get_mut(instance.root) {
            transform.set_if_neq(
                Transform::from_translation(anchor + Vec3::from_array(plane.offset))
                    .with_rotation(Quat::from_array(plane.heading)),
            );
        }
        // Source fedoscript: rot.Rotate(rotaxis * rotspeed * deltaTime) in local space.
        if let (Some(spin), Some((entity, base))) = (&prefab.spin, instance.spin)
            && let Ok(mut transform) = transforms.get_mut(entity)
        {
            let axis = Vec3::from_array(spin.axis);
            let speed = f64::from(spin.degrees_per_second * axis.length());
            let degrees = (speed * frame.frame.state.simulation_seconds) % 360.;
            transform.rotation =
                base * Quat::from_axis_angle(axis.normalize(), (degrees as f32).to_radians());
        }
    }
}
