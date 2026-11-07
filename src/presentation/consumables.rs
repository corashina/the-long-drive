use super::bind_state::BindState;
use super::materials::SourceMaterial;
use super::reference::ReferenceVisuals;
use super::scene_nodes::SceneGraph;
use super::sim_frame::SimFrameResource;
use crate::app::RuntimeReference;
use crate::reference::{AssetId, AssetUsage, ItemId, NodeId};
use crate::vehicle::{from_rapier, to_rapier};
use bevy::ecs as bevy_ecs;
use bevy::prelude::*;
use bevy::world_serialization::{WorldAssetRoot, WorldInstanceReady};
use simulation::boundary::{PoseKey, SpawnData};
use std::collections::HashMap;

#[derive(Component)]
pub(crate) struct ConsumableScene;

struct FoodInstance {
    root: Entity,
    state: BindState,
    source_materials_bound: bool,
    nodes: Vec<(NodeId, Entity)>,
    primitives: Vec<Entity>,
    source_root: rapier3d::math::Pose,
    /// The look material is on the renderer primitives.
    look_applied: bool,
}

#[derive(Resource, Default)]
pub struct ConsumableVisuals {
    instances: HashMap<ItemId, FoodInstance>,
    generation: u64,
    /// One far copy of each food glTF. It compiles food pipelines during loading.
    warmups: Vec<FoodInstance>,
    /// Food warmup requests are queued. Material loading seals after this.
    pub warmed: bool,
    /// Look materials by food prototype and look state. Empty for a food without a look.
    looks: Vec<Vec<Handle<SourceMaterial>>>,
}

impl ConsumableVisuals {}

pub(crate) fn sync(
    mut commands: Commands,
    reference: Res<RuntimeReference>,
    frame: Res<SimFrameResource>,
    visuals: Res<ReferenceVisuals>,
    gltfs: Res<Assets<bevy::gltf::Gltf>>,
    mut food: ResMut<ConsumableVisuals>,
    graph: SceneGraph,
    source_materials: Query<(), With<MeshMaterial3d<SourceMaterial>>>,
    mut transforms: Query<&mut Transform, With<ConsumableScene>>,
    mut visibility: Query<&mut Visibility>,
) {
    if !visuals.bound {
        return;
    }
    let manifest = &reference.0;
    let food = &mut *food;
    let starter = manifest.survival.starter_food;
    // An eaten starter food leaves the frame.
    let starter_food = frame.food(starter);
    if let Some(&root) = visuals.nodes.get(&manifest.item(starter).unwrap().node) {
        set_visibility(&mut visibility, root, starter_food.is_some());
    }
    for (index, group) in manifest.survival.starter_parts.iter().enumerate() {
        for node in group {
            if let Some(&entity) = visuals.nodes.get(node) {
                set_visibility(
                    &mut visibility,
                    entity,
                    starter_food.is_some_and(|(prototype, bite, _)| {
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
        let Some((prototype_index, bite, look)) = frame.food(id) else {
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
            &graph,
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
        // Source `partconditionscript.Refresh`: the state material on every renderer, one variant shown.
        if let Some(reference) = &prototype.look {
            for (index, node) in reference.variants.iter().enumerate() {
                if let Some(&(_, entity)) = instance.nodes.iter().find(|(id, _)| id == node) {
                    set_visibility(&mut visibility, entity, index == usize::from(look.variant));
                }
            }
            if !instance.look_applied
                && let Some(material) = food
                    .looks
                    .get(prototype_index)
                    .and_then(|v| v.get(usize::from(look.state)))
            {
                for node in &reference.renderers {
                    if let Some(&(_, entity)) = instance.nodes.iter().find(|(id, _)| id == node) {
                        for primitive in graph.primitives(entity) {
                            commands
                                .entity(primitive)
                                .insert(MeshMaterial3d(material.clone()));
                        }
                    }
                }
                instance.look_applied = true;
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
                        && instance.state == BindState::Loading
                    {
                        instance.state = BindState::Loaded;
                    }
                },
            )
            .id();
        Self {
            root,
            state: BindState::Loading,
            source_materials_bound: false,
            nodes: Vec::new(),
            primitives: Vec::new(),
            source_root,
            look_applied: false,
        }
    }

    /// Bind the nodes of `asset` and collect the primitives. True once every primitive has
    /// its source material.
    fn bind(
        &mut self,
        commands: &mut Commands,
        manifest: &crate::reference::ReferenceManifest,
        asset: AssetId,
        graph: &SceneGraph,
        source_materials: &Query<(), With<MeshMaterial3d<SourceMaterial>>>,
    ) -> bool {
        if matches!(self.state, BindState::Loading | BindState::Failed) {
            return false;
        }
        if self.state == BindState::Loaded {
            let binding = graph.bind_instance(commands, manifest, self.root, asset);
            if !binding.complete() {
                error!("Food asset {}: {}", asset.0, binding.failure());
                self.state = BindState::Failed;
                return false;
            }
            self.nodes = binding.nodes;
            self.primitives = binding.primitives;
            self.state = BindState::Bound;
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

/// Runtime food (turds, cut pieces) must not make a new material, mesh variant or pipeline.
/// The renderer would skip its first draw. During loading, draw one far copy of every food glTF
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
    graph: SceneGraph,
    source_materials: Query<(), With<MeshMaterial3d<SourceMaterial>>>,
    parts: Query<
        (&Mesh3d, &MeshMaterial3d<SourceMaterial>),
        With<super::renderer_visibility::SourceRenderer>,
    >,
    mut visibility: Query<&mut Visibility>,
    placement: Query<(
        &GlobalTransform,
        Option<&bevy::camera::visibility::RenderLayers>,
    )>,
    mut source_assets: ResMut<Assets<SourceMaterial>>,
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
        bound &= instance.bind(&mut commands, manifest, *asset, &graph, &source_materials);
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
    // Look materials draw on no primitive at start. Make them now and warm their pipelines far away.
    food.looks = Vec::with_capacity(manifest.survival.foods.len());
    for prototype in &manifest.survival.foods {
        let Some(look) = &prototype.look else {
            food.looks.push(Vec::new());
            continue;
        };
        let instance = &food.warmups[assets.iter().position(|a| *a == prototype.asset).unwrap()];
        let receivers: Vec<(Entity, Handle<Mesh>, bool, u32)> = look
            .renderers
            .iter()
            .filter_map(|node| instance.nodes.iter().find(|(id, _)| id == node))
            .flat_map(|(_, entity)| graph.primitives(*entity))
            .map(|primitive| {
                let (global, layers) = placement.get(primitive).unwrap();
                let mask = layers.map_or(1, |layers| {
                    layers.iter().fold(0u32, |bits, layer| {
                        bits | 1u32.checked_shl(layer as u32).unwrap_or(0)
                    })
                });
                let mesh = parts.get(primitive).unwrap().0.0.clone();
                (
                    primitive,
                    mesh,
                    global.affine().matrix3.determinant() < 0.,
                    mask,
                )
            })
            .collect();
        let Some(&(_, _, reflected, mask)) = receivers.first() else {
            error!("Food look {}: no renderer primitives", prototype.root.0);
            food.looks.push(Vec::new());
            continue;
        };
        if receivers.iter().any(|r| (r.2, r.3) != (reflected, mask)) {
            error!("Food look {}: mixed renderer layers", prototype.root.0);
        }
        let handles: Vec<_> = look
            .materials
            .iter()
            .map(|m| {
                source_assets.add(super::materials::authored_material(
                    (m.asset, m.gltf_material),
                    reflected,
                    mask,
                    &materials,
                ))
            })
            .collect();
        for (_, mesh, _, _) in &receivers {
            for handle in &handles {
                let warm = commands
                    .spawn((
                        Mesh3d(mesh.clone()),
                        MeshMaterial3d(handle.clone()),
                        super::renderer_visibility::SourceMaterialWarmup,
                        Transform::from_translation(Vec3::splat(1_000_000.)),
                        Visibility::Visible,
                        bevy::light::NotShadowCaster,
                        bevy::camera::visibility::NoFrustumCulling,
                    ))
                    .id();
                loading.warm(mesh.id(), handle.id().untyped(), "food look".into());
                lighting.warmups.push(warm);
            }
        }
        food.looks.push(handles);
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
