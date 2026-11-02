//! Scene instances of the sky traffic planes. Planes have no bodies.
use super::materials::SourceMaterial;
use super::reference::ReferenceVisuals;
use super::scene_nodes::SceneGraph;
use super::sim_frame::SimFrameResource;
use crate::app::RuntimeReference;
use crate::reference::NodeId;
use bevy::ecs as bevy_ecs;
use bevy::prelude::*;
use bevy::world_serialization::{WorldAssetRoot, WorldInstanceReady};
use simulation::boundary::PlaneRow;
use std::collections::HashMap;

#[derive(Component)]
pub(crate) struct SkyScene;

struct SkyInstance {
    root: Entity,
    prefab: usize,
    ready: bool,
    bound: bool,
    /// Binding failed. The plane stays hidden and binding does not run again.
    failed: bool,
    shown: bool,
    nodes: Vec<(NodeId, Entity)>,
    primitives: Vec<Entity>,
    /// The spin node and its glTF rotation.
    spin: Option<(Entity, Quat)>,
    /// fedoscript `seatedObj` node and whether it shows.
    seated: Option<(Entity, bool)>,
}

#[derive(Resource, Default)]
pub struct SkyTrafficVisuals {
    /// Plane id to scene instance.
    instances: HashMap<u32, SkyInstance>,
    generation: u64,
}

impl SkyTrafficVisuals {}

#[allow(clippy::too_many_arguments)]
pub(crate) fn sync(
    mut commands: Commands,
    reference: Res<RuntimeReference>,
    frame: Res<SimFrameResource>,
    visuals: Res<ReferenceVisuals>,
    gltfs: Res<Assets<bevy::gltf::Gltf>>,
    mut sky: ResMut<SkyTrafficVisuals>,
    graph: SceneGraph,
    source_materials: Query<(), With<MeshMaterial3d<SourceMaterial>>>,
    mut transforms: Query<&mut Transform>,
    mut visibility: Query<&mut Visibility, With<SkyScene>>,
    mut node_visibility: Query<&mut Visibility, Without<SkyScene>>,
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
                    failed: false,
                    shown: false,
                    nodes: Vec::new(),
                    primitives: Vec::new(),
                    spin: None,
                    seated: None,
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
            let binding = graph.bind_instance(&mut commands, manifest, instance.root, prefab.asset);
            if !binding.complete() {
                error!("Sky plane {}: {}", plane.id, binding.failure());
                instance.failed = true;
                continue;
            }
            instance.nodes = binding.nodes;
            instance.primitives = binding.primitives;
            instance.spin = prefab.spin.as_ref().and_then(|spin| {
                let (_, entity) = instance.nodes.iter().find(|(id, _)| *id == spin.node)?;
                Some((*entity, transforms.get(*entity).ok()?.rotation))
            });
            instance.seated = prefab.seated.and_then(|node| {
                let (_, entity) = instance.nodes.iter().find(|(id, _)| *id == node)?;
                Some((*entity, true))
            });
            instance.bound = true;
        }
        // Source fedoscript: `seatedObj.SetActive(seat.inUse)`.
        if let Some((entity, shown)) = &mut instance.seated {
            let seated = plane.flags & PlaneRow::SEATED != 0;
            if *shown != seated
                && let Ok(mut v) = node_visibility.get_mut(*entity)
            {
                *v = if seated {
                    Visibility::Inherited
                } else {
                    Visibility::Hidden
                };
                *shown = seated;
            }
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
