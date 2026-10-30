use super::materials::SourceMaterial;
use super::reference::{ReferenceVisuals, bind_node};
use super::sim_frame::SimFrameResource;
use crate::app::RuntimeReference;
use crate::reference::{AssetId, AssetUsage, ItemId, NodeId, SourceId};
use crate::vehicle::{from_rapier, to_rapier};
use bevy::ecs as bevy_ecs;
use bevy::gltf::GltfExtras;
use bevy::prelude::*;
use bevy::world_serialization::{WorldAssetRoot, WorldInstanceReady};
use simulation::boundary::{PoseKey, SpawnData};
use std::collections::HashMap;

#[derive(Component)]
pub(crate) struct ConsumableScene;

struct FoodInstance {
    root: Entity,
    ready: bool,
    bound: bool,
    source_materials_bound: bool,
    nodes: Vec<(NodeId, Entity)>,
    primitives: Vec<Entity>,
    source_root: rapier3d::math::Pose,
}

#[derive(Resource, Default)]
pub struct ConsumableVisuals {
    instances: HashMap<ItemId, FoodInstance>,
    generation: u64,
    /// One far copy of each food glTF. It compiles food pipelines during loading.
    warmups: Vec<FoodInstance>,
    /// Food warmup requests are queued. Material loading seals after this.
    pub warmed: bool,
}

impl ConsumableVisuals {}

#[derive(serde::Deserialize)]
struct Identity {
    reference_id: Option<SourceId>,
}

pub(crate) fn sync(
    mut commands: Commands,
    reference: Res<RuntimeReference>,
    frame: Res<SimFrameResource>,
    visuals: Res<ReferenceVisuals>,
    gltfs: Res<Assets<bevy::gltf::Gltf>>,
    mut food: ResMut<ConsumableVisuals>,
    extras: Query<(Entity, &GltfExtras)>,
    parents: Query<&ChildOf>,
    primitives: Query<(Entity, &ChildOf), With<Mesh3d>>,
    source_materials: Query<(), With<MeshMaterial3d<SourceMaterial>>>,
    mut transforms: Query<&mut Transform, With<ConsumableScene>>,
    mut visibility: Query<&mut Visibility>,
) {
    if !visuals.bound {
        return;
    }
    let manifest = &reference.0;
    let starter = manifest.survival.starter_food;
    // An eaten starter food leaves the frame.
    let starter_food = frame.food(starter);
    if let Some(&root) = visuals.nodes.get(
        &manifest
            .items
            .iter()
            .find(|record| record.id == starter)
            .unwrap()
            .node,
    ) {
        set_visibility(&mut visibility, root, starter_food.is_some());
    }
    for (index, group) in manifest.survival.starter_parts.iter().enumerate() {
        for node in group {
            if let Some(&entity) = visuals.nodes.get(node) {
                set_visibility(
                    &mut visibility,
                    entity,
                    starter_food.is_some_and(|(prototype, bite)| {
                        index == bite as usize
                            && index < manifest.survival.foods[prototype].bites as usize
                    }),
                );
            }
        }
    }
    food.instances.retain(|id, instance| {
        let keep = frame.food(*id).is_some();
        if !keep {
            commands.entity(instance.root).despawn();
        }
        keep
    });
    for (id, data) in frame.items() {
        if id == starter || !matches!(data, SpawnData::Food { .. }) {
            continue;
        }
        let Some((prototype_index, bite)) = frame.food(id) else {
            continue;
        };
        let Some(pose) = frame.interpolated(PoseKey::Item(id)) else {
            continue;
        };
        let prototype = &manifest.survival.foods[prototype_index];
        if !food.instances.contains_key(&id) {
            let Some(gltf) = food_gltf(manifest, &visuals, &gltfs, prototype.asset) else {
                continue;
            };
            let instance = FoodInstance::spawn(
                &mut commands,
                gltf,
                Transform::default(),
                Visibility::Hidden,
                food.generation,
                Some(id),
                to_rapier(crate::interaction::items::node_pose(manifest, prototype.root).unwrap()),
            );
            food.instances.insert(id, instance);
            continue;
        }
        let instance = food.instances.get_mut(&id).unwrap();
        if !instance.bind(
            &mut commands,
            manifest,
            prototype.asset,
            (&extras, &parents, &primitives),
            &source_materials,
        ) {
            continue;
        }
        if let Ok(mut transform) = transforms.get_mut(instance.root) {
            transform.set_if_neq(super::transform(from_rapier(
                &(to_rapier(pose) * instance.source_root.inverse()),
            )));
        }
        set_visibility(&mut visibility, instance.root, true);
        for other in &manifest.survival.foods {
            if let Some(&(_, entity)) = instance.nodes.iter().find(|(node, _)| *node == other.root)
            {
                set_visibility(&mut visibility, entity, other.root == prototype.root);
            }
        }
        for (index, group) in prototype.parts.iter().enumerate() {
            for node in group {
                if let Some(&(_, entity)) = instance.nodes.iter().find(|(id, _)| id == node) {
                    set_visibility(&mut visibility, entity, index == bite as usize);
                }
            }
        }
    }
}

fn food_gltf<'a>(
    manifest: &crate::reference::ReferenceManifest,
    visuals: &ReferenceVisuals,
    gltfs: &'a Assets<bevy::gltf::Gltf>,
    asset: AssetId,
) -> Option<&'a bevy::gltf::Gltf> {
    let asset = manifest
        .assets
        .iter()
        .find(|record| record.id == asset && record.usage == AssetUsage::FoodPrototype)
        .unwrap();
    // Fixed assets start their glTF loads within budget, so the handle can come later.
    let (_, handle) = visuals.gltfs.iter().find(|(id, _)| *id == asset.id)?;
    gltfs.get(handle)
}

impl FoodInstance {
    /// `item` is None for a warmup copy.
    fn spawn(
        commands: &mut Commands,
        gltf: &bevy::gltf::Gltf,
        transform: Transform,
        visibility: Visibility,
        generation: u64,
        item: Option<ItemId>,
        source_root: rapier3d::math::Pose,
    ) -> Self {
        let root = commands
            .spawn((
                ConsumableScene,
                WorldAssetRoot(gltf.scenes[0].clone()),
                transform,
                visibility,
            ))
            .observe(
                move |event: On<WorldInstanceReady>, mut food: ResMut<ConsumableVisuals>| {
                    let food = &mut *food;
                    let instance = match item {
                        Some(id) if food.generation == generation => food.instances.get_mut(&id),
                        Some(_) => None,
                        None => food.warmups.iter_mut().find(|i| i.root == event.entity),
                    };
                    if let Some(instance) = instance
                        && instance.root == event.entity
                    {
                        instance.ready = true;
                    }
                },
            )
            .id();
        Self {
            root,
            ready: false,
            bound: false,
            source_materials_bound: false,
            nodes: Vec::new(),
            primitives: Vec::new(),
            source_root,
        }
    }

    /// Bind the nodes of `asset` and collect the primitives. True once every primitive has
    /// its source material.
    fn bind(
        &mut self,
        commands: &mut Commands,
        manifest: &crate::reference::ReferenceManifest,
        asset: AssetId,
        (extras, parents, primitives): (
            &Query<(Entity, &GltfExtras)>,
            &Query<&ChildOf>,
            &Query<(Entity, &ChildOf), With<Mesh3d>>,
        ),
        source_materials: &Query<(), With<MeshMaterial3d<SourceMaterial>>>,
    ) -> bool {
        if !self.ready {
            return false;
        }
        if !self.bound {
            for (entity, extra) in extras {
                let mut parent = entity;
                let mut belongs = false;
                while let Ok(link) = parents.get(parent) {
                    parent = link.parent();
                    if parent == self.root {
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
                        .find(|node| node.source == source && node.asset == asset)
                }) else {
                    continue;
                };
                if !self.nodes.iter().any(|(id, _)| *id == node.id)
                    && bind_node(commands, node, entity, extra, primitives).is_ok()
                {
                    self.nodes.push((node.id, entity));
                }
            }
            let count = manifest
                .nodes
                .iter()
                .filter(|node| node.asset == asset)
                .count();
            if self.nodes.len() != count {
                return false;
            }
            for (primitive, parent) in primitives {
                let mut ancestor = parent.parent();
                while ancestor != self.root {
                    let Ok(link) = parents.get(ancestor) else {
                        break;
                    };
                    ancestor = link.parent();
                }
                if ancestor == self.root {
                    self.primitives.push(primitive);
                }
            }
            self.bound = true;
        }
        if !self.source_materials_bound {
            if self.primitives.is_empty()
                || !self
                    .primitives
                    .iter()
                    .all(|primitive| source_materials.contains(*primitive))
            {
                return false;
            }
            self.source_materials_bound = true;
        }
        true
    }
}

/// Runtime food (turds, cut pieces) must not make a new material, mesh variant or pipeline:
/// its first draw would be skipped. During loading, draw one far copy of every food glTF
/// with all parts shown, after the startup material pass. Runtime copies then reuse its
/// cached mesh variants, materials and compiled pipelines.
#[allow(clippy::too_many_arguments)]
pub(crate) fn warm_up(
    mut commands: Commands,
    reference: Res<RuntimeReference>,
    visuals: Res<ReferenceVisuals>,
    gltfs: Res<Assets<bevy::gltf::Gltf>>,
    mut food: ResMut<ConsumableVisuals>,
    materials: Res<super::materials::SourceMaterials>,
    loading: Res<super::materials::MaterialLoading>,
    mut lighting: ResMut<super::lighting::SourceLighting>,
    extras: Query<(Entity, &GltfExtras)>,
    parents: Query<&ChildOf>,
    primitives: Query<(Entity, &ChildOf), With<Mesh3d>>,
    source_materials: Query<(), With<MeshMaterial3d<SourceMaterial>>>,
    parts: Query<
        (&Mesh3d, &MeshMaterial3d<SourceMaterial>),
        With<super::renderer_visibility::SourceRenderer>,
    >,
    mut visibility: Query<&mut Visibility>,
) {
    let food = &mut *food;
    if food.warmed {
        // The lighting warmup despawns the primitives. The scene roots stay until then.
        if !lighting.warming {
            for instance in food.warmups.drain(..) {
                commands.entity(instance.root).despawn();
            }
        }
        return;
    }
    if !visuals.bound || !materials.bound {
        return;
    }
    let manifest = &reference.0;
    let mut assets: Vec<AssetId> = manifest.survival.foods.iter().map(|f| f.asset).collect();
    assets.sort_unstable_by_key(|asset| asset.0);
    assets.dedup();
    if food.warmups.is_empty() {
        let mut scenes = Vec::with_capacity(assets.len());
        for asset in &assets {
            let Some(gltf) = food_gltf(manifest, &visuals, &gltfs, *asset) else {
                return;
            };
            scenes.push(gltf);
        }
        for gltf in scenes {
            food.warmups.push(FoodInstance::spawn(
                &mut commands,
                gltf,
                Transform::from_translation(Vec3::splat(1_000_000.)),
                Visibility::Visible,
                0,
                None,
                rapier3d::math::Pose::IDENTITY,
            ));
        }
        return;
    }
    let mut bound = true;
    for (instance, asset) in food.warmups.iter_mut().zip(&assets) {
        bound &= instance.bind(
            &mut commands,
            manifest,
            *asset,
            (&extras, &parents, &primitives),
            &source_materials,
        );
    }
    // Warm the startup mesh variants, not the raw glTF meshes.
    if !bound
        || food
            .warmups
            .iter()
            .flat_map(|instance| &instance.primitives)
            .any(|primitive| !parts.contains(*primitive))
    {
        return;
    }
    for instance in &food.warmups {
        for (_, entity) in &instance.nodes {
            set_visibility(&mut visibility, *entity, true);
        }
        for &primitive in &instance.primitives {
            let (mesh, material) = parts.get(primitive).unwrap();
            loading.warm(mesh.id(), material.id().untyped(), "food prototype".into());
            set_visibility(&mut visibility, primitive, true);
            commands.entity(primitive).insert((
                super::renderer_visibility::SourceMaterialWarmup,
                bevy::camera::visibility::NoFrustumCulling,
            ));
            lighting.warmups.push(primitive);
        }
    }
    food.warmed = true;
}

fn set_visibility(query: &mut Query<&mut Visibility>, entity: Entity, show: bool) {
    if let Ok(mut visibility) = query.get_mut(entity) {
        visibility.set_if_neq(if show {
            Visibility::Inherited
        } else {
            Visibility::Hidden
        });
    }
}
