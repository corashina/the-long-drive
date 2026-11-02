//! Scene instances of spawned item prototype bodies.
use super::materials::SourceMaterial;
use super::reference::{ReferenceVisuals, bind_node};
use super::sim_frame::SimFrameResource;
use crate::app::RuntimeReference;
use crate::reference::{AssetUsage, ItemId, ItemPrototype, NodeId, ReferenceManifest, SourceId};
use crate::starting_area::spawned::SpawnedItem;
use crate::vehicle::{VehiclePose, from_rapier, to_rapier};
use bevy::ecs as bevy_ecs;
use bevy::ecs::system::SystemParam;
use bevy::gltf::GltfExtras;
use bevy::mesh::skinning::SkinnedMesh;
use bevy::prelude::*;
use bevy::world_serialization::{WorldAssetRoot, WorldInstanceReady};
use simulation::boundary::{PoseKey, PoseRow};
use std::collections::HashMap;

#[derive(Component)]
pub(crate) struct SpawnedScene;

struct SpawnedInstance {
    root: Entity,
    ready: bool,
    bound: bool,
    source_materials_bound: bool,
    nodes: Vec<(NodeId, Entity)>,
    primitives: Vec<Entity>,
    /// This instance shows the prototype skin and its joints are not yet the sibling items' bodies.
    skin_pending: bool,
    /// Scene root pose relative to the item body pose.
    body_to_scene: rapier3d::math::Pose,
    /// Private materials of emissive renderer nodes.
    emissive: Vec<(NodeId, Handle<SourceMaterial>)>,
}

#[derive(Resource, Default)]
pub struct SpawnedVisuals {
    instances: HashMap<ItemId, SpawnedInstance>,
    generation: u64,
}

impl SpawnedVisuals {
    /// Bound scene node of a spawned item, for item-specific presentation.
    pub fn node(&self, item: ItemId, node: NodeId) -> Option<Entity> {
        let instance = self.instances.get(&item)?;
        instance
            .bound
            .then(|| instance.nodes.iter().find(|(id, _)| *id == node))
            .flatten()
            .map(|(_, entity)| *entity)
    }
    /// Source `SetColor("_EmissionColor")` on the renderer of `node`, and with `map`
    /// `SetTexture("_EmissionMap")`. The first call gives the node primitives one private
    /// material. Only a changed value touches it. Returns false while the instance is unbound.
    pub fn emission(
        &mut self,
        item: ItemId,
        node: NodeId,
        color: [f32; 4],
        map: Option<Handle<Image>>,
        assets: &mut Assets<SourceMaterial>,
        bound: &mut Query<(&ChildOf, &mut MeshMaterial3d<SourceMaterial>)>,
    ) -> Result<bool, String> {
        let Some(instance) = self.instances.get_mut(&item) else {
            return Ok(false);
        };
        if !instance.source_materials_bound {
            return Ok(false);
        }
        if !instance.emissive.iter().any(|(id, _)| *id == node) {
            let entity = instance
                .nodes
                .iter()
                .find(|(id, _)| *id == node)
                .map(|(_, entity)| *entity)
                .ok_or("Emissive node is not in its spawned scene")?;
            let mut private: Option<Handle<SourceMaterial>> = None;
            for &primitive in &instance.primitives {
                let Ok((parent, mut material)) = bound.get_mut(primitive) else {
                    continue;
                };
                if parent.parent() != entity {
                    continue;
                }
                let handle = match &private {
                    Some(handle) => handle.clone(),
                    None => {
                        let copy = assets
                            .get(&material.0)
                            .ok_or("Emissive node material is not loaded")?
                            .clone();
                        private.insert(assets.add(copy)).clone()
                    }
                };
                material.0 = handle;
            }
            let handle = private.ok_or("Emissive node has no source material primitive")?;
            instance.emissive.push((node, handle));
        }
        let handle = &instance
            .emissive
            .iter()
            .find(|(id, _)| *id == node)
            .unwrap()
            .1;
        super::materials::write_emission(
            assets,
            handle,
            crate::presentation::world::gpu_props::linear_color(color),
            map,
        )?;
        Ok(true)
    }
}

#[derive(serde::Deserialize)]
struct Identity {
    reference_id: Option<SourceId>,
}

/// The body node that owns `node` in its prototype, if any.
fn owning_body(
    manifest: &ReferenceManifest,
    prototype: &crate::reference::ItemPrototype,
    mut node: NodeId,
) -> Option<NodeId> {
    for _ in 0..=manifest.nodes.len() {
        if prototype.bodies.iter().any(|b| b.node == node) {
            return Some(node);
        }
        node = manifest.nodes.iter().find(|n| n.id == node)?.parent?;
    }
    None
}

/// Body node whose instance shows the prototype skin. Other instances hide it.
fn skin_body(manifest: &ReferenceManifest, prototype: &ItemPrototype) -> Option<NodeId> {
    let hose = prototype.hose.as_ref()?;
    owning_body(manifest, prototype, hose.skin.node).or_else(|| {
        prototype
            .bodies
            .iter()
            .find(|b| b.id == hose.mains[0])
            .map(|b| b.node)
    })
}

/// Bound scene node of the sibling item (same spawn serial) that owns `body`.
fn sibling_node(
    spawned: &SpawnedVisuals,
    items: &[SpawnedItem],
    prototype: &ItemPrototype,
    item: &SpawnedItem,
    body: &crate::reference::BodyRecord,
) -> Option<Entity> {
    let sibling = items.iter().find(|s| {
        s.serial == item.serial
            && s.prototype == item.prototype
            && prototype.pickups[s.pickup].body == body.id
    })?;
    spawned.node(sibling.id, body.node)
}

/// Point the skin joints of `item` at the body nodes of the sibling items. Each bone is a body,
/// and each body is its own item. Returns false while a sibling is not bound.
fn bind_skin(
    commands: &mut Commands,
    spawned: &SpawnedVisuals,
    items: &[SpawnedItem],
    prototype: &ItemPrototype,
    item: &SpawnedItem,
    primitives: &Query<(Entity, &ChildOf), With<Mesh3d>>,
    skins: &Query<&SkinnedMesh>,
) -> bool {
    let Some(skin) = prototype.hose.as_ref().map(|hose| &hose.skin) else {
        return true;
    };
    let Some(mesh) = spawned.node(item.id, skin.node) else {
        return false;
    };
    let bodies = skin
        .bones
        .iter()
        .map(|bone| prototype.bodies.iter().find(|b| b.id == *bone));
    if !bodies.clone().all(|body| {
        body.is_some_and(|body| sibling_node(spawned, items, prototype, item, body).is_some())
    }) {
        return false;
    }
    let joints: Vec<Entity> = bodies
        .map(|body| sibling_node(spawned, items, prototype, item, body.unwrap()).unwrap())
        .collect();
    for (primitive, parent) in primitives {
        if parent.parent() != mesh {
            continue;
        }
        let Ok(current) = skins.get(primitive) else {
            error!("Spawned skin primitive lacks its glTF skin");
            continue;
        };
        if current.joints.len() != joints.len() {
            error!("Spawned skin joint count differs from the exported bones");
            continue;
        }
        // The glTF inverse bind poses are the exported `bind_poses` in the same bone order.
        commands.entity(primitive).insert(SkinnedMesh {
            inverse_bindposes: current.inverse_bindposes.clone(),
            joints: joints.clone(),
        });
    }
    true
}

/// Scene graph queries that bind a prototype instance.
#[derive(SystemParam)]
pub(crate) struct SceneQueries<'w, 's> {
    extras: Query<'w, 's, (Entity, &'static GltfExtras)>,
    parents: Query<'w, 's, &'static ChildOf>,
    primitives: Query<'w, 's, (Entity, &'static ChildOf), With<Mesh3d>>,
    source_materials: Query<'w, 's, (), With<MeshMaterial3d<SourceMaterial>>>,
    skins: Query<'w, 's, &'static SkinnedMesh>,
    transforms: Query<'w, 's, &'static mut Transform, With<SpawnedScene>>,
    visibility: Query<'w, 's, &'static mut Visibility>,
}

pub(crate) fn sync(
    mut commands: Commands,
    reference: Res<RuntimeReference>,
    frame: Res<SimFrameResource>,
    visuals: Res<ReferenceVisuals>,
    gltfs: Res<Assets<bevy::gltf::Gltf>>,
    mut spawned: ResMut<SpawnedVisuals>,
    scene: SceneQueries,
) {
    let SceneQueries {
        extras,
        parents,
        primitives,
        source_materials,
        skins,
        mut transforms,
        mut visibility,
    } = scene;
    if !visuals.bound {
        return;
    }
    let manifest = &reference.0;
    let items = frame.spawned();
    let stale: Vec<ItemId> = spawned
        .instances
        .keys()
        .copied()
        .filter(|id| !items.iter().any(|item| item.id == *id))
        .collect();
    for id in stale {
        let instance = spawned.instances.remove(&id).unwrap();
        commands.entity(instance.root).despawn();
    }
    for item in items {
        let Some((_, current)) = frame.pose(PoseKey::Item(item.id)) else {
            continue;
        };
        let prototype = &manifest.item_prototypes[item.prototype];
        if !spawned.instances.contains_key(&item.id) {
            let Some(asset) = manifest
                .assets
                .iter()
                .find(|a| a.id == prototype.asset && a.usage == AssetUsage::ItemPrototype)
            else {
                continue;
            };
            let Some((_, handle)) = visuals.gltfs.iter().find(|(id, _)| *id == asset.id) else {
                continue;
            };
            let Some(gltf) = gltfs.get(handle) else {
                continue;
            };
            let Ok(root_pose) = crate::interaction::items::node_pose(manifest, prototype.root)
            else {
                continue;
            };
            let body = prototype
                .bodies
                .iter()
                .find(|b| b.id == prototype.pickups[item.pickup].body)
                .unwrap();
            let body_local = to_rapier(VehiclePose {
                position: body.pose.translation,
                rotation: body.pose.rotation,
            });
            let id = item.id;
            let generation = spawned.generation;
            let root = commands
                .spawn((
                    SpawnedScene,
                    WorldAssetRoot(gltf.scenes[0].clone()),
                    Transform::default(),
                    Visibility::Hidden,
                ))
                .observe(
                    move |event: On<WorldInstanceReady>, mut spawned: ResMut<SpawnedVisuals>| {
                        if spawned.generation != generation {
                            return;
                        }
                        if let Some(instance) = spawned.instances.get_mut(&id)
                            && instance.root == event.entity
                        {
                            instance.ready = true;
                        }
                    },
                )
                .id();
            spawned.instances.insert(
                item.id,
                SpawnedInstance {
                    root,
                    ready: false,
                    bound: false,
                    source_materials_bound: false,
                    nodes: Vec::new(),
                    primitives: Vec::new(),
                    skin_pending: skin_body(manifest, prototype) == Some(body.node),
                    body_to_scene: body_local.inverse() * to_rapier(root_pose).inverse(),
                    emissive: Vec::new(),
                },
            );
            continue;
        }
        let instance = spawned.instances.get_mut(&item.id).unwrap();
        if !instance.ready {
            continue;
        }
        if !instance.bound {
            for (entity, extra) in &extras {
                let mut parent = entity;
                let mut belongs = false;
                while let Ok(link) = parents.get(parent) {
                    parent = link.parent();
                    if parent == instance.root {
                        belongs = true;
                        break;
                    }
                }
                if !belongs {
                    continue;
                }
                let Ok(identity) = serde_json::from_str::<Identity>(&extra.value) else {
                    continue;
                };
                let Some(node) = identity.reference_id.and_then(|source| {
                    manifest
                        .nodes
                        .iter()
                        .find(|node| node.source == source && node.asset == prototype.asset)
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
                .filter(|node| node.asset == prototype.asset)
                .count();
            if instance.nodes.len() != count {
                continue;
            }
            // Other pickup bodies of the prototype are separate items. One item shows the skin.
            let own = prototype
                .bodies
                .iter()
                .find(|b| b.id == prototype.pickups[item.pickup].body)
                .unwrap()
                .node;
            let skin = prototype.hose.as_ref().map(|hose| hose.skin.node);
            let skin_body = skin_body(manifest, prototype);
            for &(node, entity) in &instance.nodes {
                let owner = if Some(node) == skin {
                    skin_body
                } else {
                    owning_body(manifest, prototype, node)
                };
                if owner.is_some_and(|body| body != own) {
                    commands.entity(entity).insert(Visibility::Hidden);
                }
            }
            for (primitive, parent) in &primitives {
                let mut ancestor = parent.parent();
                while ancestor != instance.root {
                    let Ok(link) = parents.get(ancestor) else {
                        break;
                    };
                    ancestor = link.parent();
                }
                if ancestor == instance.root {
                    instance.primitives.push(primitive);
                }
            }
            instance.bound = true;
        }
        if !instance.source_materials_bound {
            if instance.primitives.is_empty()
                || !instance
                    .primitives
                    .iter()
                    .all(|primitive| source_materials.contains(*primitive))
            {
                continue;
            }
            instance.source_materials_bound = true;
        }
        let pose = frame
            .interpolated(PoseKey::Item(item.id))
            .expect("indexed spawned item");
        if let Ok(mut transform) = transforms.get_mut(instance.root) {
            // scaleDestroyScript scales the gib about its body node.
            let scale = frame.item(item.id).map_or(1., |row| row.scale);
            let mut body_to_scene = instance.body_to_scene;
            body_to_scene.translation *= scale;
            transform.set_if_neq(
                super::transform(from_rapier(&(to_rapier(pose) * body_to_scene)))
                    .with_scale(Vec3::splat(scale)),
            );
        }
        let shown = current.flags & PoseRow::STORED == 0;
        if let Ok(mut v) = visibility.get_mut(instance.root) {
            v.set_if_neq(if shown {
                Visibility::Inherited
            } else {
                Visibility::Hidden
            });
        }
        if instance.skin_pending
            && bind_skin(
                &mut commands,
                &spawned,
                items,
                prototype,
                item,
                &primitives,
                &skins,
            )
        {
            spawned.instances.get_mut(&item.id).unwrap().skin_pending = false;
        }
    }
}
