//! Generated scenes are prepared offscreen before their collision recipes publish.
use super::{
    gpu_props::{BatchKey, GpuProps, PropRow, source_position, world_bounds},
    grass_jobs::GrassJobs,
    instance_materials::{InstanceMaterial, InstanceMaterials, RendererMaterial, ready},
    lod::{LodInstance, LodOwner, LodSchedule, LodTable},
    node_plan::{NodePlan, anchor_local},
    pipelines::{PipelineTicket, WorldPipelines},
    prototype_loader::SourceGltf,
    prototypes::SourcePrototypes,
};
use crate::ownership::PresentationLedger;
use crate::world::stream::content_index::InstanceSelection;
use crate::world::{RendererFact, RendererFacts};
use crate::{
    presentation::materials::{SourceShadowMode, grass::settings::settings_from_source},
    presentation::renderer_visibility::CameraHidden,
    reference::{ReferenceError, ReferenceManifest, WorldPrototypeReference},
    world::{Placement, RendererWorld},
};
use bevy::math::{Affine3A, DMat4, DVec3};
use bevy::platform::collections::HashMap;
use bevy::{
    camera::visibility::RenderLayers,
    ecs as bevy_ecs,
    mesh::{VertexAttributeValues, skinning::SkinnedMesh},
    prelude::*,
};
use grass_mesh::{GrassPatch, GrassSettings, GrassView};
use std::sync::Arc;
/// The prototype node is a `wear.Renderers` node of a wearable item.
fn worn_renderer(
    reference: &ReferenceManifest,
    prototype: &WorldPrototypeReference,
    node: usize,
) -> bool {
    let source = &prototype.nodes[node].source;
    reference.clothing.wearables.iter().any(|w| {
        reference
            .item_prototypes
            .iter()
            .any(|p| p.id == w.prototype && p.source == prototype.source)
            && w.renderers.iter().any(|id| {
                reference
                    .nodes
                    .iter()
                    .any(|n| n.id == *id && n.source == *source)
            })
    })
}
struct MaterialVariant {
    source: Option<crate::reference::SourceId>,
    binding: RendererMaterial,
    ticket: Option<PipelineTicket>,
}
/// A renderer is an entity or a GPU prop row.
#[derive(Clone, Copy)]
enum Target {
    Entity(Entity),
    /// The glTF node and the local mesh bounds rebuild the row world on a rebase.
    Row {
        slot: u32,
        node: u32,
        center: [f32; 3],
        half_extents: [f32; 3],
    },
}
/// Bind inputs of a partconditionscript renderer. A changed state binds its material lazily.
struct Rebind {
    primitive: crate::reference::prototype_plan::PlanPrimitive,
    record: crate::reference::render_lighting::RendererRecord,
    layer: u8,
    receive: bool,
    reflected: bool,
    has_lod: bool,
    source_position: Vec3,
}
struct Renderer {
    material_source: Option<crate::reference::SourceId>,
    rebind: Option<Box<Rebind>>,
    /// Last applied condition color.
    color: Option<[f32; 4]>,
    variants: Vec<MaterialVariant>,
    node: usize,
    target: Target,
    mesh: Handle<Mesh>,
    binding: RendererMaterial,
    ticket: Option<PipelineTicket>,
    /// A changed condition material that waits for its pipelines.
    pending: Option<MaterialVariant>,
    shadow: bool,
    lod_state: Option<(bool, f32)>,
    // The static node chain of the renderer is active. Spawned nodes carry their own visibility.
    active: bool,
    settings: Option<GrassSettings>,
    /// The genie lamp renderer: a private material that takes the lamp emission.
    emissive: bool,
    /// The renderer node is an enabled Grass displacement writer.
    writer: bool,
    /// A `mirrorscript` glass renderer. The mirror selection picks its material, not the frame.
    mirror: bool,
}
impl Renderer {
    /// Shows the material variant of `selected`. Returns whether the material changed.
    fn select(
        &mut self,
        selected: Option<&crate::reference::SourceId>,
        commands: &mut Commands,
    ) -> bool {
        if selected == self.material_source.as_ref() {
            return false;
        }
        let Some(variant) = self
            .variants
            .iter_mut()
            .find(|v| v.source.as_ref() == selected)
        else {
            return false;
        };
        std::mem::swap(&mut self.binding, &mut variant.binding);
        std::mem::swap(&mut self.material_source, &mut variant.source);
        std::mem::swap(&mut self.ticket, &mut variant.ticket);
        let entity = self.entity().expect("lamp variants are entities");
        self.binding.visible.replace(entity, commands);
        self.binding.apply_sort(entity, commands);
        true
    }
    fn entity(&self) -> Option<Entity> {
        match self.target {
            Target::Entity(entity) => Some(entity),
            Target::Row { .. } => None,
        }
    }
    fn visible(&self) -> bool {
        self.active && self.lod_state.is_none_or(|(visible, _)| visible)
    }
    /// A row has no parent entity, so it also takes the visibility of its instance root.
    fn show(&self, root: bool, commands: &mut Commands, props: &mut GpuProps) {
        match self.target {
            Target::Entity(entity) => {
                commands.entity(entity).insert(if self.visible() {
                    Visibility::Inherited
                } else {
                    Visibility::Hidden
                });
            }
            Target::Row { slot, .. } => props.set_hidden(slot, !(root && self.visible())),
        }
    }
}
struct Instance {
    retirement: crate::ownership::renderer_pending::RendererRetirement,
    continuation: Option<crate::ownership::renderer_pending::PipelineContinuation>,
    upload: Option<Arc<crate::presentation::fixed_assets::MeshUpload>>,
    caps_bound: bool,
    placement: Placement,
    prototype: usize,
    /// Structure slot of the placement in the simulation frame.
    slot: Option<u32>,
    sync_pending: bool,
    /// A changed condition material waits for its textures. Sync visits the instance again.
    material_pending: bool,
    session: u64,
    root: Entity,
    // Entity of each spawned glTF node. Static nodes have none.
    nodes: Vec<Option<Entity>>,
    // Nearest spawned ancestor of each glTF node. `None` is the root.
    anchors: Vec<Option<u32>>,
    active_nodes: Vec<bool>,
    // Local render frame; retain double precision across distant rebases.
    worlds: Vec<DMat4>,
    // Frozen source frame for procedural color, wind, and Grass inputs only.
    source_worlds: Vec<Mat4>,
    // VehiclePose changes position and rotation. Retain the original composed scales.
    scales: Vec<(Vec3, Vec3)>,
    renderers: Vec<Renderer>,
    lod: LodInstance,
    plan: Arc<NodePlan>,
    ready: bool,
    cpu: usize,
    gpu: usize,
    objects: usize,
    visible: bool,
    /// `StructureRow::WORN` and `StructureRow::CAMERA_HIDDEN` of the last view.
    hidden: u32,
    origin: [f64; 3],
}
impl Instance {
    /// Apply a root visibility change to the rows of this instance.
    fn show_rows(&self, props: &mut GpuProps) {
        for renderer in &self.renderers {
            if let Target::Row { slot, .. } = renderer.target {
                props.set_hidden(slot, !(self.visible && renderer.visible()));
            }
        }
    }
    fn entity(&self, node: usize) -> Result<Entity, ReferenceError> {
        self.nodes[node]
            .ok_or_else(|| ReferenceError("Generated node writer targets a static node".into()))
    }
    /// World matrix of the nearest spawned ancestor of a glTF node.
    fn anchor_world(&self, node: usize, origin: crate::world::WorldOrigin) -> DMat4 {
        self.anchors[node].map_or_else(
            || local_root(&self.placement, origin),
            |anchor| self.worlds[anchor as usize],
        )
    }
    /// Writes a dynamic glTF node pose given relative to its glTF parent.
    fn write_local(
        &self,
        transforms: &mut super::runtime::WorldRoots,
        node: usize,
        local: Transform,
        origin: crate::world::WorldOrigin,
    ) -> Result<(), ReferenceError> {
        let entity = self.entity(node)?;
        let parent = self.plan.parents[node]
            .filter(|&parent| Some(parent) != self.anchors[node])
            .map(|parent| {
                (
                    self.anchor_world(node, origin),
                    self.worlds[parent as usize],
                )
            });
        if let Ok(mut transform) = transforms.get_mut(entity) {
            transform.set_if_neq(anchor_local(parent, local));
        }
        Ok(())
    }
}
impl LodOwner for Instance {
    fn lod(&mut self) -> Option<(&mut LodInstance, &[DMat4])> {
        (!self.retirement.started()).then_some((&mut self.lod, &self.worlds))
    }
}
#[derive(Resource, Default)]
pub struct SourceInstances {
    instances: Vec<Instance>,
    pending: Option<Placement>,
    bindings_dirty: bool,
    origin: Option<[f64; 3]>,
    /// Instances of each structure slot.
    slot_instances: HashMap<u32, Vec<usize>>,
    selection: InstanceSelection,
    lods: LodSchedule,
    cache_bytes: usize,
    props: GpuProps,
    /// Changes when an instance is added or removed.
    revision: u64,
    /// `RendererWorld::structure_membership` of the last full retirement pass.
    retire_key: Option<(u64, u64, u64)>,
    /// Indices of instances whose retirement started.
    retiring: Vec<usize>,
    /// Indices of instances that are not ready.
    not_ready: Vec<usize>,
    /// Indices of instances with a Grass displacement writer.
    writer_instances: Vec<usize>,
    writers_dirty: bool,
    /// glTF nodes that move with each body, per prototype index.
    body_nodes: HashMap<usize, Vec<Vec<u32>>>,
}
/// The glTF nodes of each body node and its descendants, in prototype node order.
fn body_nodes(prototype: &crate::reference::WorldPrototypeReference) -> Vec<Vec<u32>> {
    prototype
        .bodies
        .iter()
        .map(|body| {
            let gltf_node = prototype.nodes[body.node as usize].gltf_node;
            prototype
                .nodes
                .iter()
                .filter(|descendant| {
                    let mut cursor = descendant.parent;
                    let mut belongs = descendant.gltf_node == gltf_node;
                    for _ in 0..prototype.nodes.len() {
                        let Some(parent) = cursor else { break };
                        if parent == body.node {
                            belongs = true;
                            break;
                        }
                        cursor = prototype.nodes[parent as usize].parent;
                    }
                    belongs
                })
                .map(|descendant| descendant.gltf_node)
                .collect()
        })
        .collect()
}
impl SourceInstances {
    pub fn props(&self) -> &GpuProps {
        &self.props
    }
    pub(crate) fn props_mut(&mut self) -> &mut GpuProps {
        &mut self.props
    }
    pub(crate) fn cache_bytes(&self) -> usize {
        self.cache_bytes
    }
    fn measure_cache_bytes(&self) -> usize {
        use crate::interaction::memory::platform_map;
        platform_map(&self.slot_instances)
            + self
                .slot_instances
                .values()
                .map(|owners| owners.capacity() * std::mem::size_of::<usize>())
                .sum::<usize>()
            + self.selection.retained_bytes()
            + self.lods.retained_bytes()
            + self.props.retained_bytes()
    }
    pub(crate) fn startup_audit(&self, grass: &GrassJobs) -> serde_json::Value {
        serde_json::json!({
            "pending_owner": self.pending.as_ref().map(|p| &p.prototype),
            "unready": self.instances.iter().filter(|i| !i.ready).count(),
            "grass_waits": self.instances.iter().filter(|i| !i.ready).flat_map(|i| &i.renderers)
                .filter(|r| r.entity().is_some_and(|e| r.settings.is_some() && !grass.ready(e))).count(),
            "unready_renderers": self.instances.iter().filter(|i| !i.ready).flat_map(|i| &i.renderers)
                .take(4).map(|r| serde_json::json!({
                    "grass": r.settings.is_some(),
                    "ticket": r.ticket.as_ref().map(|t| format!("{:?}", t.state())),
                })).collect::<Vec<_>>(),
        })
    }
    pub fn ready(&self) -> bool {
        self.instances
            .iter()
            .all(|i| !i.retirement.started() && i.ready && i.visible)
    }
    /// Ready visible instances: root entity, placement, prototype index and node entities by
    /// glTF node.
    pub(crate) fn shown(
        &self,
    ) -> impl Iterator<Item = (Entity, &Placement, usize, &[Option<Entity>])> {
        self.instances
            .iter()
            .filter(|i| i.ready && i.visible && !i.retirement.started())
            .map(|i| (i.root, &i.placement, i.prototype, i.nodes.as_slice()))
    }
    /// Ready visible instances with `mirrorscript` renderers: instance index, root entity,
    /// prototype index and node entities by glTF node.
    pub(crate) fn mirror_cars(
        &self,
    ) -> impl Iterator<Item = (usize, Entity, usize, &[Option<Entity>])> {
        self.instances
            .iter()
            .enumerate()
            .filter(|(_, i)| {
                i.ready
                    && i.visible
                    && !i.retirement.started()
                    && i.renderers.iter().any(|r| r.mirror)
            })
            .map(|(index, i)| (index, i.root, i.prototype, i.nodes.as_slice()))
    }
    /// Entity of glTF node `node` in instance `index`. Static nodes have none.
    pub(crate) fn node_entity(&self, index: usize, node: u32) -> Option<Entity> {
        self.instances[index].nodes[node as usize]
    }
    /// Entities of the shown mirror renderers of prototype node `node` in instance `index`.
    pub(crate) fn mirror_renderers(
        &self,
        index: usize,
        node: u32,
    ) -> impl Iterator<Item = Entity> + '_ {
        self.instances[index]
            .renderers
            .iter()
            .filter(move |r| r.mirror && r.node == node as usize && r.visible())
            .filter_map(Renderer::entity)
    }
    /// `mirror.Turn`: the renderers of prototype node `node` in instance `index` show `material`
    /// with `image`, or with None the glass.
    pub(crate) fn show_mirror(
        &mut self,
        index: usize,
        node: u32,
        live: Option<(&crate::reference::SourceId, &Handle<Image>)>,
        materials: &mut InstanceMaterials,
        commands: &mut Commands,
    ) -> Result<(), ReferenceError> {
        let instance = &mut self.instances[index];
        let mut changed = false;
        for renderer in instance
            .renderers
            .iter_mut()
            .filter(|r| r.mirror && r.node == node as usize)
        {
            if renderer.select(live.map(|(material, _)| material), commands) {
                renderer.lod_state = None;
                changed = true;
            }
            if let Some((_, image)) = live
                && materials.mirror_image(&mut renderer.binding, image)?
            {
                let entity = renderer.entity().expect("lamp variants are entities");
                renderer.binding.visible.replace(entity, commands);
            }
        }
        if changed {
            self.lods.queue(&mut instance.lod, index);
        }
        Ok(())
    }
    /// Whether an instance with root entity `root` exists.
    pub(crate) fn has_root(&self, root: Entity) -> bool {
        self.instances.iter().any(|i| i.root == root)
    }
    /// Changes when an instance is added or removed.
    pub(crate) fn revision(&self) -> u64 {
        self.revision
    }
    /// Prototype index of each instance.
    pub(crate) fn prototypes(&self) -> impl Iterator<Item = usize> + '_ {
        self.instances.iter().map(|i| i.prototype)
    }
    pub fn roots(&self) -> impl Iterator<Item = Entity> + '_ {
        self.instances.iter().map(|i| i.root)
    }
    pub fn release_all(&mut self, commands: &mut Commands, materials: &mut InstanceMaterials) {
        for instance in self.instances.drain(..) {
            commands.entity(instance.root).despawn();
            for renderer in instance.renderers {
                materials.release(renderer.binding);
                for variant in renderer.variants.into_iter().chain(renderer.pending) {
                    materials.release(variant.binding);
                }
            }
        }
        materials.cache.generated.reset();
        self.props.clear();
        self.pending = None;
        self.bindings_dirty = true;
        self.slot_instances.clear();
        self.selection.reset(0);
        self.lods.reset();
        self.origin = None;
        self.revision = self.revision.wrapping_add(1);
        self.retire_key = None;
        self.retiring.clear();
        self.not_ready.clear();
        self.writer_instances.clear();
        self.writers_dirty = true;
        self.cache_bytes = self.measure_cache_bytes();
    }

    fn rebase(&mut self, origin: crate::world::WorldOrigin, commands: &mut Commands) {
        if self.origin == Some(origin.offset) {
            return;
        }
        self.origin = Some(origin.offset);
        for (index, instance) in self.instances.iter_mut().enumerate() {
            if instance.origin == origin.offset {
                continue;
            }
            let shift = DVec3::from_array(std::array::from_fn(|axis| {
                instance.origin[axis] - origin.offset[axis]
            }));
            for world in &mut instance.worlds {
                world.w_axis += shift.extend(0.);
            }
            for renderer in &instance.renderers {
                if let Target::Row {
                    slot,
                    node,
                    center,
                    half_extents,
                } = renderer.target
                {
                    let world = instance.worlds[node as usize].as_mat4();
                    let bounds = world_bounds(Affine3A::from_mat4(world), center, half_extents);
                    self.props.set_world(slot, world, bounds);
                }
            }
            commands.entity(instance.root).insert(Transform {
                translation: Vec3::from_array(origin.local(instance.placement.pose.position)),
                rotation: Quat::from_array(instance.placement.pose.rotation),
                scale: Vec3::from_array(instance.placement.scale),
            });
            instance.origin = origin.offset;
            instance.sync_pending = true;
            self.selection.push(index);
        }
    }
    pub(crate) fn measurement_meshes(
        &self,
    ) -> impl Iterator<Item = (bevy::asset::AssetId<Mesh>, bool)> + '_ {
        self.instances
            .iter()
            .flat_map(|i| i.renderers.iter().map(move |r| (r.mesh.id(), !i.ready)))
    }

    pub fn request_descriptors(
        &self,
        stream: &RendererWorld,
        ledger: &mut PresentationLedger,
        reference: &ReferenceManifest,
        prototypes: &mut SourcePrototypes,
    ) -> Result<(), ReferenceError> {
        // Discover selected descriptors before material or payload readiness.
        let required_ready = stream.required_ready();
        for index in 0..stream.pending_structures().len() {
            let placement = &stream.pending_structures()[index];
            if !stream.structure_required(&placement.id)
                && !stream.structure_replay_required(&placement.id)
                && !required_ready
            {
                continue;
            }
            let source = &placement.prototype;
            if prototypes.requested(source) {
                continue;
            }
            let prototype =
                reference.world.prototypes.get(source).ok_or_else(|| {
                    ReferenceError("Missing generated structure prototype".into())
                })?;
            prototypes.request(stream, ledger, &prototype.source)?;
        }
        Ok(())
    }
    pub fn prepare(
        &mut self,
        stream: &RendererWorld,
        ledger: &mut PresentationLedger,
        facts: &mut RendererFacts,
        reference: &ReferenceManifest,
        prototypes: &mut SourcePrototypes,
        server: &AssetServer,
        assets: &Assets<SourceGltf>,
        meshes: &mut Assets<Mesh>,
        materials: &mut InstanceMaterials,
        grass: &mut GrassJobs,
        pipelines: &mut WorldPipelines,
        fixed: &crate::presentation::fixed_assets::SourceFixedAssets,
        commands: &mut Commands,
        view: GrassView,
    ) -> Result<(), ReferenceError> {
        self.rebase(stream.origin, commands);
        let required_ready = stream.required_ready();
        // Descriptors are requested only for these placements. See `request_descriptors`.
        let selectable = |p: &crate::world::Placement| {
            stream.contents_initialized(&p.id)
                && (stream.structure_required(&p.id)
                    || stream.structure_replay_required(&p.id)
                    || required_ready)
        };
        // A new required structure clears required_ready. A pick that is not selectable then
        // never gets its descriptor and would block the required structure.
        if self.pending.as_ref().is_some_and(|p| {
            !selectable(p)
                || !stream
                    .pending_structures()
                    .iter()
                    .any(|wanted| wanted.id == p.id)
        }) {
            self.pending = None;
        }
        if self.pending.is_none() {
            self.pending = stream
                .pending_structures()
                .iter()
                .filter(|p| selectable(p) && !self.instances.iter().any(|i| i.placement.id == p.id))
                .min_by(|a, b| {
                    stream
                        .focus_distance_squared(a.pose.position)
                        .total_cmp(&stream.focus_distance_squared(b.pose.position))
                })
                .cloned();
        }
        // Full pending ownership must drain through publication before another instance starts.
        // Continue polling existing renderer work below, including its readiness transfer.
        if let Some(placement) = self.pending.as_ref().filter(|placement| {
            let child = reference
                .world
                .prototypes
                .get(&placement.prototype)
                .is_some_and(|p| {
                    p.deferred_materials
                        .iter()
                        .any(|m| m.shader.name == "Stix Games/Grass")
                });
            ledger.can_request_independent(1 + usize::from(child))
        }) {
            let prototype_index = reference
                .world
                .prototypes
                .index(&placement.prototype)
                .ok_or_else(|| ReferenceError("Missing generated structure prototype".into()))?;
            let prototype = &reference.world.prototypes[prototype_index];
            let admitted = prototypes
                .plan(stream.frame().request.session, &placement.prototype)
                .is_some()
                && materials.admit(
                    stream,
                    ledger,
                    reference,
                    prototype,
                    placement,
                    prototypes
                        .plan(stream.frame().request.session, &placement.prototype)
                        .unwrap(),
                )?;
            if admitted {
                prototypes.admit_payload(
                    stream,
                    ledger,
                    reference,
                    server,
                    &placement.prototype,
                )?;
                let lods = prototypes.lods(
                    stream.frame().request.session,
                    &placement.prototype,
                    || {
                        LodTable::for_prototype(&reference.world, prototype, |source| {
                            stream.lod_owner(source)
                        })
                    },
                )?;
                let node_plan = prototypes.nodes(
                    stream.frame().request.session,
                    &placement.prototype,
                    |loaded| {
                        let asset = assets.get(&loaded.asset).ok_or_else(|| {
                            ReferenceError("Missing generated prototype asset".into())
                        })?;
                        let count = loaded.plan.nodes.len();
                        let mut sources = vec![None; count];
                        let mut dynamic = vec![false; count];
                        let moving = stream.moving_nodes(reference, prototype)?;
                        for (index, node) in prototype.nodes.iter().enumerate() {
                            sources[node.gltf_node as usize] = Some(index as u32);
                            dynamic[node.gltf_node as usize] |= moving[index];
                        }
                        // Skin joints move with their nodes.
                        for &joint in asset.joints.iter().flatten() {
                            dynamic[joint] = true;
                        }
                        let children = loaded
                            .plan
                            .nodes
                            .iter()
                            .map(|n| n.children.as_slice())
                            .collect::<Vec<_>>();
                        Ok(NodePlan::new(&children, sources, dynamic))
                    },
                )?;
                if let (Some(loaded), Some(lods), Some(node_plan)) = (
                    prototypes.loaded(stream.frame().request.session, &placement.prototype),
                    lods,
                    node_plan,
                ) {
                    if let Some(asset) = assets.get(&loaded.asset) {
                        let count = loaded.plan.nodes.len();
                        let primitives = loaded
                            .plan
                            .nodes
                            .iter()
                            .map(|n| {
                                loaded
                                    .plan
                                    .primitives
                                    .iter()
                                    .filter(|p| Some(p.mesh) == n.mesh)
                                    .count()
                            })
                            .sum::<usize>();
                        // Reservations include ECS storage, transforms, material bind groups, and LOD membership.
                        let mut cpu = count
                            * (2048
                                + std::mem::size_of::<DMat4>()
                                + std::mem::size_of::<(Vec3, Vec3)>()
                                + std::mem::size_of::<Option<u32>>())
                            + std::mem::size_of::<Vec<()>>()
                            + primitives * (8192 + crate::presentation::caster_material::CPU_BYTES);
                        let mut gpu =
                            primitives * (2048 + crate::presentation::caster_material::gpu_bytes());
                        let objects = count + primitives;
                        let request = stream.frame().request;
                        if !stream.structure_wanted(request, &placement.id) {
                            return Err(ReferenceError("Stale instance renderer admission".into()));
                        }
                        let mut a = ledger.allocation();
                        a.active_cpu += cpu;
                        a.active_gpu += gpu;
                        a.objects += objects;
                        ledger.set_allocation(a)?;
                        let mut continuation =
                            ledger.claim_structure(request, placement.id.clone());
                        let root_pose = local_root(placement, stream.origin);
                        let root = commands
                            .spawn((
                                Transform::from_matrix(root_pose.as_mat4()),
                                Visibility::Hidden,
                            ))
                            .id();
                        let mut owned_bindings = Vec::with_capacity(primitives);
                        let mut owned_rows = Vec::new();
                        let props = &mut self.props;
                        // None: a material texture is not ready. The placement waits and tries again.
                        let construction = (|| -> Result<Option<Instance>, ReferenceError> {
                            let source_root = Mat4::from_scale_rotation_translation(
                                Vec3::from_array(placement.scale),
                                Quat::from_array(placement.pose.rotation),
                                Vec3::from_array(placement.pose.position.map(|v| v as f32)),
                            );
                            let appearance = stream.appearance(&placement.id);
                            let locals = loaded
                                .plan
                                .nodes
                                .iter()
                                .map(|node| {
                                    let local = if let Some(matrix) = node.matrix {
                                        Mat4::from_cols_array(&matrix)
                                    } else {
                                        Mat4::from_scale_rotation_translation(
                                            Vec3::from_array(node.scale),
                                            Quat::from_array(node.rotation),
                                            Vec3::from_array(node.translation),
                                        )
                                    };
                                    match node_plan.sources[node.index as usize] {
                                        Some(source) => override_local(
                                            local,
                                            appearance,
                                            &prototype.nodes[source as usize].source,
                                        ),
                                        None => local,
                                    }
                                })
                                .collect::<Vec<_>>();
                            let active_nodes = (0..prototype.nodes.len())
                                .map(|i| {
                                    crate::starting_area::structure_frame::active(
                                        prototype, i, appearance,
                                    )
                                })
                                .collect::<Vec<_>>();
                            let flat = node_plan.flatten(&locals);
                            let worlds = flat
                                .chain
                                .iter()
                                .map(|chain| root_pose * chain.as_dmat4())
                                .collect::<Vec<_>>();
                            let source_worlds = flat
                                .chain
                                .iter()
                                .map(|chain| source_root * *chain)
                                .collect::<Vec<_>>();
                            // Parents spawn before children, so each anchor entity exists.
                            let mut nodes = vec![None; count];
                            for &node in &node_plan.order {
                                let node = node as usize;
                                if !flat.spawned[node] {
                                    continue;
                                }
                                let parent = flat.anchors[node].map_or(root, |a| {
                                    nodes[a as usize].expect("anchor spawns first")
                                });
                                let visible = node_plan.sources[node]
                                    .is_none_or(|source| active_nodes[source as usize]);
                                nodes[node] = Some(
                                    commands
                                        .spawn((
                                            Transform::from_matrix(flat.rest[node]),
                                            if visible {
                                                Visibility::Inherited
                                            } else {
                                                Visibility::Hidden
                                            },
                                            ChildOf(parent),
                                        ))
                                        .id(),
                                );
                            }
                            let mut renderers = Vec::with_capacity(primitives);
                            for (source_index, source_node) in prototype.nodes.iter().enumerate() {
                                if !source_node.renderer_enabled {
                                    continue;
                                }
                                let node = &loaded.plan.nodes[source_node.gltf_node as usize];
                                let Some(mesh_index) = node.mesh else {
                                    continue;
                                };
                                let extras = node.extras.as_ref().ok_or_else(|| {
                                    ReferenceError("Missing generated renderer metadata".into())
                                })?;
                                let shadow = extras.cast_shadows.ok_or_else(|| {
                                    ReferenceError("Missing generated shadow mode".into())
                                })?;
                                let receive = extras.receive_shadows.ok_or_else(|| {
                                    ReferenceError("Missing generated receiver state".into())
                                })?;
                                // LOD uniforms and warm tickets retain each renderer's original handles.
                                let has_lod = !lods.levels(source_index).is_empty();
                                for primitive in loaded
                                    .plan
                                    .primitives
                                    .iter()
                                    .filter(|p| p.mesh == mesh_index)
                                {
                                    let index = asset
                                        .primitives
                                        .iter()
                                        .position(|&(n, m, p)| {
                                            n == node.index as usize
                                                && m == primitive.mesh as usize
                                                && p == primitive.primitive as usize
                                        })
                                        .ok_or_else(|| {
                                            ReferenceError(
                                                "Missing generated primitive mesh".into(),
                                            )
                                        })?;
                                    let mesh = asset.meshes[index].clone();
                                    let world = worlds[node.index as usize].as_mat4();
                                    let source_position =
                                        source_position(source_worlds[node.index as usize]);
                                    let appearance =
                                        stream.appearance(&placement.id).iter().rev().find(|a| {
                                            a.node == source_node.source
                                                && (a.primary.is_some()
                                                    || a.material.is_some()
                                                    || a.color.is_some()
                                                    || a.color_material.is_some())
                                        });
                                    let color = appearance.and_then(|appearance| {
                                        appearance.color.or_else(|| {
                                            appearance.color_material.as_ref().and_then(|id| {
                                                reference
                                                    .rendering
                                                    .materials
                                                    .iter()
                                                    .find(|m| &m.source == id)
                                                    .map(|m| m.color)
                                                    .or_else(|| {
                                                        prototype
                                                            .deferred_materials
                                                            .iter()
                                                            .find(|m| &m.source == id)
                                                            .and_then(|m| {
                                                                m.colors.get("_Color").copied()
                                                            })
                                                    })
                                            })
                                        })
                                    });
                                    let mut variant_ids = stream
                                        .lamp_materials(&prototype.source, &source_node.source)?
                                        .to_vec();
                                    let (initial, selected) =
                                        super::instance_materials::biome_material(
                                            stream,
                                            reference,
                                            prototype,
                                            placement,
                                            source_index,
                                            primitive,
                                            super::instance_materials::selected_material(
                                                stream,
                                                reference,
                                                prototype,
                                                placement,
                                                &source_node.source,
                                            ),
                                        );
                                    variant_ids.retain(|id| Some(id) != selected);
                                    // A static root renderer with one shared material can be a GPU prop row.
                                    // A row takes its LOD state, color and source position from the row data,
                                    // so its material stays shared.
                                    let record =
                                        extras.source_renderer.as_ref().ok_or_else(|| {
                                            ReferenceError("Missing source renderer record".into())
                                        })?;
                                    let emissive = super::instance_materials::emissive_renderer(
                                        reference,
                                        &record.source,
                                    );
                                    let local = meshes
                                        .get(&mesh)
                                        .filter(|m| m.indices().is_some() && !m.has_morph_targets())
                                        .and_then(bevy::camera::primitives::MeshAabb::compute_aabb);
                                    let writer =
                                        reference.world.grass_displacement.writers.iter().any(
                                            |w| w.enabled && w.transform == source_node.source,
                                        );
                                    let row = variant_ids.is_empty()
                                        && !emissive
                                        && node.skin.is_none()
                                        && flat.on_root(node.index as usize)
                                        && local.is_some()
                                        && !writer;
                                    let rebind = stream
                                        .appearance(&placement.id)
                                        .iter()
                                        .any(|a| {
                                            a.node == source_node.source
                                                && a.active.is_none()
                                                && (a.material.is_some()
                                                    || a.color.is_some()
                                                    || a.color_material.is_some())
                                        })
                                        .then(|| {
                                            Box::new(Rebind {
                                                primitive: primitive.clone(),
                                                record: record.clone(),
                                                layer: source_node.layer,
                                                receive,
                                                reflected: world.determinant() < 0.,
                                                has_lod,
                                                source_position,
                                            })
                                        });
                                    let Some(mut binding) = materials.bind(
                                        stream.frame().request.session,
                                        prototype,
                                        primitive,
                                        record,
                                        initial,
                                        selected,
                                        source_node.layer,
                                        receive,
                                        world.determinant() < 0.,
                                        source_position,
                                        view,
                                        row,
                                    )?
                                    else {
                                        return Ok(None);
                                    };
                                    let material_source = selected.cloned();
                                    if binding.gpu_props() {
                                        let material = match &binding.visible {
                                            InstanceMaterial::Primary(h) => h.id().untyped(),
                                            InstanceMaterial::Specular(h) => h.id().untyped(),
                                            InstanceMaterial::Impostor(h) => h.id().untyped(),
                                            InstanceMaterial::Grass(_) => {
                                                unreachable!("Grass renderers are entities")
                                            }
                                        };
                                        let color = materials.row_color(&binding, color)?;
                                        let aabb = local.unwrap();
                                        let world = worlds[node.index as usize].as_mat4();
                                        let (center, half_extents) =
                                            (aabb.center.to_array(), aabb.half_extents.to_array());
                                        let slot = props.alloc(
                                            BatchKey {
                                                mesh: mesh.id(),
                                                material,
                                            },
                                            PropRow {
                                                world,
                                                bounds: world_bounds(
                                                    Affine3A::from_mat4(world),
                                                    center,
                                                    half_extents,
                                                ),
                                                layer: source_node.layer,
                                                cast_shadows: record.cast_shadows,
                                                camera_hidden: false,
                                                hidden: true,
                                                fade: 1.0,
                                                color,
                                                source_position,
                                                batch: 0,
                                            },
                                        );
                                        owned_rows.push(slot);
                                        owned_bindings.push((None, binding.clone()));
                                        renderers.push(Renderer {
                                            material_source,
                                            rebind,
                                            // The row color already holds it; the first view rewrites the same value.
                                            color: None,
                                            variants: Vec::new(),
                                            node: source_index,
                                            target: Target::Row {
                                                slot,
                                                node: node.index,
                                                center,
                                                half_extents,
                                            },
                                            mesh,
                                            binding,
                                            ticket: None,
                                            pending: None,
                                            lod_state: None,
                                            active: active_nodes[source_index],
                                            shadow: shadow != 0,
                                            settings: None,
                                            emissive: false,
                                            writer: false,
                                            mirror: false,
                                        });
                                        continue;
                                    }
                                    if let Some(color) = color {
                                        materials.color(&mut binding, color)?;
                                    }
                                    if has_lod || emissive {
                                        materials.make_private(&mut binding);
                                    }
                                    let is_grass =
                                        matches!(binding.visible, InstanceMaterial::Grass(_));
                                    // A static node bakes its matrix into the renderer.
                                    let (parent, transform) = match nodes[node.index as usize] {
                                        Some(entity) => (entity, Transform::default()),
                                        None => (
                                            flat.anchors[node.index as usize]
                                                .map_or(root, |a| nodes[a as usize].unwrap()),
                                            Transform::from_matrix(flat.rest[node.index as usize]),
                                        ),
                                    };
                                    let chain_active = nodes[node.index as usize].is_some()
                                        || active_nodes[source_index];
                                    let entity = commands
                                        .spawn((
                                            transform,
                                            if chain_active {
                                                Visibility::Inherited
                                            } else {
                                                Visibility::Hidden
                                            },
                                            ChildOf(parent),
                                            RenderLayers::layer(source_node.layer as usize),
                                            SourceShadowMode(shadow),
                                            crate::presentation::renderer_visibility::SourceRenderer(
                                                extras.source_renderer.clone().ok_or_else(
                                                    || {
                                                        ReferenceError(
                                                            "Missing source renderer record".into(),
                                                        )
                                                    },
                                                )?,
                                            ),
                                            bevy::light::NotShadowCaster,
                                        ))
                                        .id();
                                    owned_bindings.push((Some(entity), binding.clone()));
                                    let mut variants = Vec::with_capacity(variant_ids.len());
                                    if !variant_ids.is_empty() {
                                        let extra_cpu = variant_ids.len()
                                            * (8192
                                                + crate::presentation::caster_material::CPU_BYTES);
                                        let extra_gpu = variant_ids.len()
                                            * (2048
                                                + crate::presentation::caster_material::gpu_bytes(
                                                ));
                                        let mut allocation = ledger.allocation();
                                        allocation.active_cpu += extra_cpu;
                                        allocation.active_gpu += extra_gpu;
                                        ledger.set_allocation(allocation)?;
                                        cpu += extra_cpu;
                                        gpu += extra_gpu;
                                    }
                                    for source in variant_ids {
                                        let initial = super::instance_materials::source_primary(
                                            reference, prototype, &source,
                                        );
                                        let Some(mut variant) = materials.bind(
                                            stream.frame().request.session,
                                            prototype,
                                            primitive,
                                            extras.source_renderer.as_ref().ok_or_else(|| {
                                                ReferenceError(
                                                    "Missing source renderer record".into(),
                                                )
                                            })?,
                                            initial,
                                            Some(&source),
                                            source_node.layer,
                                            receive,
                                            world.determinant() < 0.,
                                            source_position,
                                            view,
                                            false,
                                        )?
                                        else {
                                            return Ok(None);
                                        };
                                        if has_lod {
                                            materials.make_private(&mut variant);
                                        }
                                        owned_bindings.push((Some(entity), variant.clone()));
                                        variants.push(MaterialVariant {
                                            source: Some(source),
                                            binding: variant,
                                            ticket: None,
                                        });
                                    }
                                    if !receive {
                                        commands
                                            .entity(entity)
                                            .insert(bevy::light::NotShadowReceiver);
                                    }
                                    if !is_grass {
                                        commands.entity(entity).insert(Mesh3d(mesh.clone()));
                                        binding.visible.insert(entity, commands);
                                        binding.apply_sort(entity, commands);
                                    }
                                    if let Some(skin) = node.skin {
                                        let joints = asset
                                            .joints
                                            .get(skin as usize)
                                            .ok_or_else(|| {
                                                ReferenceError(
                                                    "Missing generated skin joints".into(),
                                                )
                                            })?
                                            .iter()
                                            .map(|&j| {
                                                nodes[j].ok_or_else(|| {
                                                    ReferenceError(
                                                        "Generated skin joint is static".into(),
                                                    )
                                                })
                                            })
                                            .collect::<Result<_, _>>()?;
                                        let binding = SkinnedMesh {
                                            inverse_bindposes: asset.inverse_bindposes
                                                [skin as usize]
                                                .clone(),
                                            joints,
                                        };
                                        commands.entity(entity).insert(binding);
                                    }
                                    let (_patches, settings) = if is_grass {
                                        let record = prototype
                                            .deferred_materials
                                            .iter()
                                            .find(|m| {
                                                Some(&m.source)
                                                    == primitive.source_material.as_ref()
                                            })
                                            .ok_or_else(|| {
                                                ReferenceError(
                                                    "Missing Grass source material".into(),
                                                )
                                            })?;
                                        let mesh_data = meshes.get(&mesh).ok_or_else(|| {
                                            ReferenceError("Missing Grass prototype mesh".into())
                                        })?;
                                        let patch_bytes = mesh_data
                                            .indices()
                                            .ok_or_else(|| {
                                                ReferenceError(
                                                    "Missing Grass source triangles".into(),
                                                )
                                            })?
                                            .len()
                                            / 3
                                            * std::mem::size_of::<GrassPatch>()
                                            * 2;
                                        let mut a = ledger.allocation();
                                        a.active_cpu += patch_bytes;
                                        ledger.set_allocation(a)?;
                                        cpu += patch_bytes;
                                        let source_world = source_worlds[node.index as usize];
                                        let patches = grass_patches(mesh_data, source_world)?;
                                        let settings = settings_from_source(record)?;
                                        if let InstanceMaterial::Grass(handle) = &binding.visible {
                                            let bounds = grass_bounds(
                                                &patches,
                                                source_world,
                                                materials
                                                    .grass
                                                    .get(handle)
                                                    .expect("bound Grass material")
                                                    .displacement_radius(),
                                            );
                                            grass.request(
                                                stream,
                                                ledger,
                                                entity,
                                                root,
                                                Arc::clone(&patches),
                                                settings,
                                                handle.clone(),
                                                shadow != 0,
                                                bounds,
                                            )?;
                                        }
                                        (Some(patches), Some(settings))
                                    } else {
                                        (None, None)
                                    };
                                    renderers.push(Renderer {
                                        material_source,
                                        rebind,
                                        color,
                                        variants,
                                        node: source_index,
                                        target: Target::Entity(entity),
                                        mesh,
                                        binding,
                                        ticket: None,
                                        pending: None,
                                        lod_state: None,
                                        active: chain_active,
                                        shadow: shadow != 0,
                                        settings,
                                        emissive,
                                        writer,
                                        mirror: prototype.vehicle.as_ref().is_some_and(|v| {
                                            v.mirrors
                                                .iter()
                                                .any(|m| m.renderer as usize == source_index)
                                        }),
                                    });
                                }
                            }
                            Ok(Some(Instance {
                                retirement: Default::default(),
                                continuation: None,
                                upload: None,
                                caps_bound: false,
                                placement: placement.clone(),
                                prototype: prototype_index,
                                slot: None,
                                sync_pending: true,
                                material_pending: false,
                                session: stream.frame().request.session,
                                root,
                                active_nodes,
                                nodes,
                                anchors: flat.anchors,
                                scales: worlds
                                    .iter()
                                    .zip(&source_worlds)
                                    .map(|(world, source)| {
                                        (
                                            world.as_mat4().to_scale_rotation_translation().0,
                                            source.to_scale_rotation_translation().0,
                                        )
                                    })
                                    .collect(),
                                worlds,
                                source_worlds,
                                renderers,
                                lod: LodInstance::new(lods.clone()),
                                plan: node_plan.clone(),
                                ready: false,
                                cpu,
                                gpu,
                                objects,
                                visible: false,
                                hidden: 0,
                                origin: stream.origin.offset,
                            }))
                        })();
                        match construction {
                            Ok(Some(mut instance)) => {
                                let upload = fixed.watch_meshes(
                                    instance.renderers.iter().map(|r| r.mesh.clone()),
                                );
                                continuation.watch_upload(upload.receipt.clone());
                                instance.continuation = Some(continuation);
                                instance.upload = Some(upload);
                                self.lods.spawn(&mut instance.lod, self.instances.len());
                                self.not_ready.push(self.instances.len());
                                self.instances.push(instance);
                                self.bindings_dirty = true;
                                self.writers_dirty = true;
                                self.revision = self.revision.wrapping_add(1);
                                self.pending = None;
                            }
                            result => {
                                commands.entity(root).despawn();
                                for slot in owned_rows {
                                    props.free(slot);
                                }
                                for (entity, binding) in owned_bindings {
                                    if let Some(entity) = entity {
                                        grass.retire(
                                            ledger,
                                            entity,
                                            meshes,
                                            &mut materials.grass,
                                        )?;
                                    }
                                    materials.release(binding);
                                }
                                let mut a = ledger.allocation();
                                a.active_cpu = a.active_cpu.saturating_sub(cpu);
                                a.active_gpu = a.active_gpu.saturating_sub(gpu);
                                a.objects = a.objects.saturating_sub(objects);
                                ledger.set_allocation(a)?;
                                ledger.release_claim(&mut continuation)?;
                                result?;
                            }
                        }
                    }
                }
            }
        }
        // Visit not ready instances in instance order.
        self.not_ready.sort_unstable();
        let mut at = 0;
        while at < self.not_ready.len() {
            let instance = &mut self.instances[self.not_ready[at]];
            if instance.retirement.started() {
                self.not_ready.remove(at);
                continue;
            }
            at += 1;
            if instance.session != stream.frame().request.session
                || !stream
                    .pending_structures()
                    .iter()
                    .any(|p| p.id == instance.placement.id)
            {
                continue;
            }
            let mut complete = true;
            for renderer in &mut instance.renderers {
                if let (InstanceMaterial::Grass(_), Some(entity)) =
                    (&renderer.binding.visible, renderer.entity())
                {
                    complete &= grass.ready(entity);
                    continue;
                }
                PipelineTicket::retry(&mut renderer.ticket);
                if renderer.ticket.is_none() {
                    renderer.ticket = renderer.binding.visible.warm(
                        stream,
                        ledger,
                        pipelines,
                        instance.continuation.as_mut(),
                        &renderer.mesh,
                        renderer.shadow,
                    )?;
                }
                complete &= ready(&renderer.ticket)?;
                for variant in &mut renderer.variants {
                    PipelineTicket::retry(&mut variant.ticket);
                    if variant.ticket.is_none() {
                        variant.ticket = variant.binding.visible.warm(
                            stream,
                            ledger,
                            pipelines,
                            instance.continuation.as_mut(),
                            &renderer.mesh,
                            renderer.shadow,
                        )?;
                    }
                    complete &= ready(&variant.ticket)?;
                }
            }
            if complete && instance.continuation.as_ref().unwrap().drained() {
                let source = &instance.placement.prototype;
                facts.push(RendererFact::BindPrototype(source.clone()));
                facts.push(RendererFact::BindRecipe(source.clone()));
                let claim = instance.continuation.as_mut().unwrap();
                facts.push(RendererFact::AcceptStructure(
                    claim.request(),
                    instance.placement.id.clone(),
                ));
                ledger.hand_over_claim(claim)?;
                instance.ready = true;
                instance.continuation = None;
                instance.upload = None;
                at -= 1;
                self.not_ready.remove(at);
            }
        }
        Ok(())
    }
    /// Applies the structure views of the simulation frame.
    pub(crate) fn sync(
        &mut self,
        sim: &mut crate::presentation::sim_frame::SimFrameResource,
        reference: &ReferenceManifest,
        commands: &mut Commands,
        transforms: &mut super::runtime::WorldRoots,
        materials: &mut InstanceMaterials,
        screens: &crate::presentation::computer::ComputerScreens,
        ledger: &mut PresentationLedger,
        stream: Option<&RendererWorld>,
        pipelines: &mut WorldPipelines,
    ) -> Result<(), ReferenceError> {
        use simulation::boundary::{NodePoseRow, StructureRow};
        let origin = crate::world::WorldOrigin {
            offset: sim.frame.state.stream_origin,
        };
        self.rebase(origin, commands);
        if self.bindings_dirty || std::mem::take(&mut sim.structures_moved) {
            self.selection.reset(self.instances.len());
            for owners in self.slot_instances.values_mut() {
                owners.clear();
            }
            for (index, instance) in self.instances.iter_mut().enumerate() {
                let slot = sim.structure_slot(&instance.placement.id);
                instance.sync_pending |= instance.slot != slot;
                instance.slot = slot;
                if let Some(slot) = slot {
                    self.slot_instances.entry(slot).or_default().push(index);
                }
                if instance.sync_pending {
                    self.selection.push(index);
                }
            }
            self.slot_instances.retain(|_, owners| !owners.is_empty());
            self.bindings_dirty = false;
            self.cache_bytes = self.measure_cache_bytes();
        }
        for slot in sim.changed_structures.drain(..) {
            for &index in self.slot_instances.get(&slot).into_iter().flatten() {
                self.selection.push(index);
            }
        }
        let sim = &*sim;
        let selected = self.selection.len();
        for selected_index in 0..selected {
            let instance_index = self.selection.at(selected_index);
            let instance = &mut self.instances[instance_index];
            if instance.retirement.started() {
                continue;
            }
            let view = instance
                .slot
                .map(|slot| &sim.structure_views[slot as usize]);
            if view.is_some_and(|v| v.flags & StructureRow::READY == 0) {
                if instance.visible {
                    commands.entity(instance.root).insert(Visibility::Hidden);
                    instance.visible = false;
                    instance.show_rows(&mut self.props);
                }
                continue;
            }
            if !instance.visible {
                commands.entity(instance.root).insert(Visibility::Inherited);
                instance.visible = true;
                instance.show_rows(&mut self.props);
            }
            let Some(view) = view else {
                continue;
            };
            let prototype = &reference.world.prototypes[instance.prototype];
            if !instance.caps_bound {
                for cap in &view.caps {
                    crate::presentation::reference::bind_generated_cap(
                        commands,
                        cap,
                        prototype,
                        &instance.nodes,
                    )?;
                }
                instance.caps_bound = true;
            }
            for row in &view.poses {
                let rotation = Quat::from_array(row.rotation);
                match row.kind {
                    NodePoseRow::LOCAL => {
                        let node = prototype.nodes[row.node as usize].gltf_node as usize;
                        let pose = Transform {
                            translation: Vec3::from_array(row.translation),
                            rotation,
                            scale: Vec3::from_array(row.scale),
                        };
                        instance.write_local(transforms, node, pose, origin)?;
                    }
                    NodePoseRow::ROTATION => {
                        let node = prototype.nodes[row.node as usize].gltf_node as usize;
                        if let Ok(mut transform) = transforms.get_mut(instance.entity(node)?) {
                            if transform.rotation != rotation {
                                transform.rotation = rotation;
                            }
                        }
                    }
                    NodePoseRow::BODY => {
                        let body = &prototype.bodies[row.node as usize];
                        let translation = Vec3::from_array(row.translation);
                        let world = Mat4::from_rotation_translation(rotation, translation);
                        let node = &prototype.nodes[body.node as usize];
                        let index = node.gltf_node as usize;
                        let scale = Vec3::from_array(body.pose.scale)
                            * Vec3::from_array(instance.placement.scale);
                        let world = (world * Mat4::from_scale(scale)).as_dmat4();
                        let source_world = Mat4::from_rotation_translation(
                            rotation,
                            Vec3::from_array(origin.absolute(row.translation).map(|v| v as f32)),
                        ) * Mat4::from_scale(scale);
                        if instance.worlds[index] != world {
                            self.lods.queue(&mut instance.lod, instance_index);
                            let delta = world * instance.worlds[index].inverse();
                            let source_changed = source_world != instance.source_worlds[index];
                            let source_delta =
                                source_world * instance.source_worlds[index].inverse();
                            let targets = &self
                                .body_nodes
                                .entry(instance.prototype)
                                .or_insert_with(|| body_nodes(prototype))
                                [row.node as usize];
                            for &target in targets {
                                let target = target as usize;
                                instance.worlds[target] = delta * instance.worlds[target];
                                if source_changed {
                                    instance.source_worlds[target] =
                                        source_delta * instance.source_worlds[target];
                                }
                            }
                            instance.worlds[index] = world;
                            instance.source_worlds[index] = source_world;
                            let anchor = instance.anchor_world(index, origin);
                            if let Ok(mut transform) = transforms.get_mut(instance.entity(index)?) {
                                *transform =
                                    Transform::from_matrix((anchor.inverse() * world).as_mat4());
                            }
                        }
                    }
                    _ => {
                        let index = prototype.nodes[row.node as usize].gltf_node as usize;
                        let world = Mat4::from_scale_rotation_translation(
                            instance.scales[index].0,
                            rotation,
                            Vec3::from_array(row.translation),
                        )
                        .as_dmat4();
                        if instance.worlds[index] != world {
                            self.lods.queue(&mut instance.lod, instance_index);
                        }
                        instance.worlds[index] = world;
                        instance.source_worlds[index] = Mat4::from_scale_rotation_translation(
                            instance.scales[index].1,
                            rotation,
                            Vec3::from_array(origin.absolute(row.translation).map(|v| v as f32)),
                        );
                        let anchor = instance.anchor_world(index, origin);
                        if let Ok(mut transform) = transforms.get_mut(instance.entity(index)?) {
                            transform.set_if_neq(Transform::from_matrix(
                                (anchor.inverse() * world).as_mat4(),
                            ));
                        }
                    }
                }
            }
            if view.flags & StructureRow::LAMP != 0 {
                for renderer in instance.renderers.iter_mut().filter(|r| r.emissive) {
                    materials.emission(&mut renderer.binding, view.lamp, None)?;
                }
            }
            // displayscript.Upd: on shows defMaterial with the computer output as _EmissionMap.
            if view.flags & StructureRow::DISPLAY != 0 {
                let color = if view.flags & StructureRow::DISPLAY_ON != 0 {
                    reference.computer.display.emission
                } else {
                    [0., 0., 0., 1.]
                };
                let image = view
                    .display_map
                    .checked_sub(1)
                    .and_then(|item| screens.image(crate::reference::ItemId(item)));
                for renderer in instance.renderers.iter_mut().filter(|r| r.emissive) {
                    materials.emission(&mut renderer.binding, color, image.cloned())?;
                    if image.is_none() {
                        materials.clear_emission_map(&mut renderer.binding)?;
                    }
                }
            }
            for (i, node) in prototype.nodes.iter().enumerate() {
                let enabled = !view.hidden.contains(&(i as u32));
                if enabled != instance.active_nodes[i] {
                    instance.active_nodes[i] = enabled;
                    if let Some(entity) = instance.nodes[node.gltf_node as usize] {
                        commands.entity(entity).insert(if enabled {
                            Visibility::Inherited
                        } else {
                            Visibility::Hidden
                        });
                    } else {
                        // A static node hides its own renderers. Static descendants change in this loop too.
                        let root = instance.visible;
                        for renderer in instance.renderers.iter_mut().filter(|r| r.node == i) {
                            renderer.active = enabled;
                            renderer.show(root, commands, &mut self.props);
                        }
                    }
                }
            }
            let hidden = view.flags & (StructureRow::WORN | StructureRow::CAMERA_HIDDEN);
            if hidden != instance.hidden {
                instance.hidden = hidden;
                for renderer in &instance.renderers {
                    // Only these flags hide structure renderers, so each renderer takes the new state.
                    let camera_hidden = hidden & StructureRow::CAMERA_HIDDEN != 0
                        || (hidden & StructureRow::WORN != 0
                            && worn_renderer(reference, prototype, renderer.node));
                    match renderer.target {
                        Target::Entity(entity) if camera_hidden => {
                            commands.entity(entity).insert(CameraHidden);
                        }
                        Target::Entity(entity) => {
                            commands.entity(entity).remove::<CameraHidden>();
                        }
                        Target::Row { slot, .. } => {
                            self.props.set_camera_hidden(slot, camera_hidden)
                        }
                    }
                }
            }
            for renderer in instance.renderers.iter_mut().filter(|r| !r.mirror) {
                let selected = view
                    .materials
                    .iter()
                    .find(|m| m.node as usize == renderer.node)
                    .map(|m| &sim.material_sources[m.source as usize]);
                if renderer.select(selected, commands) {
                    renderer.lod_state = None;
                    self.lods.queue(&mut instance.lod, instance_index);
                }
            }
            // partconditionscript renderers: a changed state binds its material on change.
            let mut material_pending = false;
            for renderer in &mut instance.renderers {
                let Some(rebind) = renderer.rebind.as_deref() else {
                    continue;
                };
                let selected = view
                    .materials
                    .iter()
                    .find(|m| m.node as usize == renderer.node)
                    .map(|m| &sim.material_sources[m.source as usize]);
                let color = view
                    .colors
                    .iter()
                    .find(|c| c.node as usize == renderer.node)
                    .map(|c| c.color);
                if selected != renderer.material_source.as_ref()
                    && !renderer
                        .variants
                        .iter()
                        .any(|v| v.source.as_ref() == selected)
                {
                    if renderer
                        .pending
                        .as_ref()
                        .is_some_and(|p| p.source.as_ref() != selected)
                    {
                        materials.release(renderer.pending.take().unwrap().binding);
                    }
                    if renderer.pending.is_none() {
                        let initial = selected.and_then(|source| {
                            super::instance_materials::source_primary(reference, prototype, source)
                        });
                        // The new material was not admitted with the instance. Admit it first.
                        let admitted = materials.admit_binding(
                            ledger,
                            reference,
                            prototype,
                            &rebind.primitive,
                            initial,
                            selected,
                        )?;
                        let row = matches!(renderer.target, Target::Row { .. });
                        let binding = if admitted {
                            materials.bind(
                                instance.session,
                                prototype,
                                &rebind.primitive,
                                &rebind.record,
                                initial,
                                selected,
                                rebind.layer,
                                rebind.receive,
                                rebind.reflected,
                                rebind.source_position,
                                Default::default(),
                                row,
                            )?
                        } else {
                            None
                        };
                        material_pending |= binding.is_none();
                        if let Some(mut binding) = binding {
                            if rebind.has_lod && !row {
                                materials.make_private(&mut binding);
                            }
                            renderer.pending = Some(MaterialVariant {
                                source: selected.cloned(),
                                binding,
                                ticket: None,
                            });
                        }
                    }
                    // A compiling pipeline stops the whole view. The old material shows until the new one compiles.
                    if let Some(pending) = &mut renderer.pending {
                        PipelineTicket::retry(&mut pending.ticket);
                        if pending.ticket.is_none()
                            && let Some(stream) = stream
                        {
                            pending.ticket = pending.binding.visible.warm(
                                stream,
                                ledger,
                                pipelines,
                                None,
                                &renderer.mesh,
                                renderer.shadow,
                            )?;
                        }
                        if ready(&pending.ticket)? {
                            let MaterialVariant {
                                binding, ticket, ..
                            } = renderer.pending.take().unwrap();
                            match &mut renderer.target {
                                Target::Entity(entity) => {
                                    binding.visible.replace(*entity, commands);
                                    binding.apply_sort(*entity, commands);
                                    renderer.lod_state = None;
                                    self.lods.queue(&mut instance.lod, instance_index);
                                }
                                Target::Row { slot, .. } => {
                                    let mut data = self.props.rows()[*slot as usize];
                                    data.color = materials.row_color(&binding, color)?;
                                    self.props.free(*slot);
                                    *slot = self.props.alloc(
                                        BatchKey {
                                            mesh: renderer.mesh.id(),
                                            material: batch_material(&binding),
                                        },
                                        data,
                                    );
                                }
                            }
                            materials.release(std::mem::replace(&mut renderer.binding, binding));
                            renderer.ticket = ticket;
                            renderer.material_source = selected.cloned();
                            // The new material starts from its own color.
                            renderer.color = None;
                        } else {
                            material_pending = true;
                        }
                    }
                } else if let Some(pending) = renderer.pending.take() {
                    materials.release(pending.binding);
                }
                if color != renderer.color {
                    if let Some(c) = color {
                        match renderer.target {
                            Target::Entity(entity) => {
                                materials.color(&mut renderer.binding, c)?;
                                renderer.binding.visible.replace(entity, commands);
                            }
                            Target::Row { slot, .. } => self
                                .props
                                .set_color(slot, materials.row_color(&renderer.binding, Some(c))?),
                        }
                    }
                    renderer.color = color;
                }
            }
            instance.material_pending = material_pending;
            for renderer in &instance.renderers {
                // A row impostor shares its material. Its row holds the source position.
                if renderer.entity().is_none() {
                    continue;
                }
                let node = &prototype.nodes[renderer.node];
                let position = source_position(instance.source_worlds[node.gltf_node as usize]);
                if let InstanceMaterial::Impostor(handle) = &renderer.binding.visible {
                    if let Some(mut material) = materials.impostors.get_mut(handle) {
                        if material.uniform.source_position.truncate() != position {
                            material.uniform.source_position = position.extend(0.);
                        }
                    }
                }
            }
        }
        for selected_index in 0..selected {
            let instance = &mut self.instances[self.selection.at(selected_index)];
            instance.material_pending &= !instance.retirement.started();
            instance.sync_pending = instance.material_pending;
            self.bindings_dirty |= instance.material_pending;
        }
        self.selection.clear();
        Ok(())
    }
    pub fn select_lods(
        &mut self,
        reference: &ReferenceManifest,
        view: super::lod::LodView,
        commands: &mut Commands,
        materials: &mut InstanceMaterials,
        grass: &mut GrassJobs,
    ) {
        let props = &mut self.props;
        self.lods.update(
            view,
            reference.world.lod.lod_bias,
            reference.world.lod.maximum_lod as usize,
            self.origin,
            &mut self.instances,
            |instance| {
                let root = instance.visible;
                for renderer in &mut instance.renderers {
                    let state = instance.lod.renderer(renderer.node);
                    if renderer.lod_state == Some(state) {
                        continue;
                    }
                    renderer.lod_state = Some(state);
                    let fade = state.1;
                    let entity = match renderer.target {
                        Target::Entity(entity) => entity,
                        // The row data holds the visibility and fade.
                        Target::Row { slot, .. } => {
                            props.set_lod(slot, root && renderer.active, state);
                            continue;
                        }
                    };
                    renderer.show(root, commands, props);
                    materials.fade(&renderer.binding, fade);
                    grass.set_fade(entity, fade, &mut materials.grass);
                }
            },
        );
        crate::world::stream_profile::count(
            crate::world::stream_profile::Stage::Lods,
            self.lods.evaluated(),
        );
    }
    pub fn request_writers(
        &mut self,
        ledger: &mut PresentationLedger,
        reference: &ReferenceManifest,
        writers: &mut crate::presentation::materials::grass::writers::GrassWriters,
        meshes: &Assets<Mesh>,
    ) -> Result<(), ReferenceError> {
        if self.writers_dirty {
            self.writers_dirty = false;
            self.writer_instances.clear();
            for (index, instance) in self.instances.iter().enumerate() {
                if instance.renderers.iter().any(|r| r.writer) {
                    self.writer_instances.push(index);
                }
            }
        }
        // Each request also carries the current writer pose and retries a deferred writer.
        for &index in &self.writer_instances {
            let instance = &self.instances[index];
            if instance.retirement.started() {
                continue;
            }
            let prototype = &reference.world.prototypes[instance.prototype];
            for renderer in instance.renderers.iter().filter(|r| r.writer) {
                let Some(entity) = renderer.entity() else {
                    continue;
                };
                let node = &prototype.nodes[renderer.node];
                writers.request(
                    ledger,
                    entity,
                    renderer.mesh.clone(),
                    instance.worlds[node.gltf_node as usize].as_mat4(),
                    meshes,
                )?;
            }
        }
        Ok(())
    }
    pub fn retire(
        &mut self,
        stream: &RendererWorld,
        ledger: &mut PresentationLedger,
        commands: &mut Commands,
        meshes: &mut Assets<Mesh>,
        materials: &mut InstanceMaterials,
        grass: &mut GrassJobs,
        writers: &mut crate::presentation::materials::grass::writers::GrassWriters,
    ) -> Result<(), ReferenceError> {
        // Wanted instances change only with the structure lists or the session.
        let key = stream.structure_membership();
        if self.retire_key != Some(key) {
            self.retire_key = Some(key);
            let frame = stream.frame();
            let published = frame
                .structures
                .iter()
                .map(|s| &s.placement.id)
                .chain(stream.pending_structures().iter().map(|p| &p.id))
                .collect::<bevy::platform::collections::HashSet<_>>();
            for (index, instance) in self.instances.iter_mut().enumerate() {
                if instance.retirement.started() {
                    continue;
                }
                let wanted = instance.session == frame.request.session
                    && published.contains(&instance.placement.id);
                if !instance.retirement.keep(wanted) {
                    self.retiring.push(index);
                }
            }
        }
        // Descending order: `swap_remove` on this list only moves a visited entry.
        self.retiring.sort_unstable();
        let mut at = self.retiring.len();
        while at > 0 {
            at -= 1;
            let index = self.retiring[at];
            if let Some(parent) = &self.instances[index].continuation {
                parent.stop_flight();
            }
            let mut grass_drained = true;
            for entity in self.instances[index]
                .renderers
                .iter()
                .filter_map(Renderer::entity)
            {
                grass_drained &= grass.retire(ledger, entity, meshes, &mut materials.grass)?;
            }
            let parent_drained = self.instances[index]
                .continuation
                .as_ref()
                .is_none_or(|c| c.drained());
            if !self.instances[index]
                .retirement
                .drained(parent_drained, grass_drained)
            {
                continue;
            }
            self.retiring.swap_remove(at);
            self.not_ready.retain(|&i| i != index);
            let mut instance = self.instances.swap_remove(index);
            self.lods.remove(&instance.lod);
            let last = self.instances.len();
            if let Some(moved) = self.instances.get_mut(index) {
                self.lods.relocate(&mut moved.lod, index);
                for i in self.retiring.iter_mut().chain(&mut self.not_ready) {
                    if *i == last {
                        *i = index;
                    }
                }
            }
            self.bindings_dirty = true;
            self.writers_dirty = true;
            self.revision = self.revision.wrapping_add(1);
            commands.entity(instance.root).despawn();
            for renderer in instance.renderers {
                match renderer.target {
                    Target::Entity(entity) => {
                        writers.retire(ledger, entity)?;
                        grass.retire(ledger, entity, meshes, &mut materials.grass)?;
                    }
                    Target::Row { slot, .. } => self.props.free(slot),
                }
                materials.release(renderer.binding);
                for variant in renderer.variants.into_iter().chain(renderer.pending) {
                    materials.release(variant.binding);
                }
            }
            let mut a = ledger.allocation();
            a.active_cpu = a.active_cpu.saturating_sub(instance.cpu);
            a.active_gpu = a.active_gpu.saturating_sub(instance.gpu);
            a.objects = a.objects.saturating_sub(instance.objects);
            ledger.set_allocation(a)?;
            if let Some(claim) = instance.continuation.as_mut() {
                ledger.release_claim(claim)?;
            }
        }
        Ok(())
    }
}
/// Applies the last rotation and the last scale override of a node to its local transform.
fn override_local(
    local: Mat4,
    appearance: &[crate::world::NodeAppearance],
    node: &crate::reference::SourceId,
) -> Mat4 {
    let rotation = appearance
        .iter()
        .rev()
        .find(|a| &a.node == node && a.rotation.is_some())
        .and_then(|a| a.rotation);
    let scale = appearance
        .iter()
        .rev()
        .find(|a| &a.node == node && a.scale.is_some())
        .and_then(|a| a.scale);
    if rotation.is_none() && scale.is_none() {
        return local;
    }
    let mut transform = Transform::from_matrix(local);
    if let Some(rotation) = rotation {
        transform.rotation = Quat::from_array(rotation);
    }
    if let Some(scale) = scale {
        transform.scale = Vec3::from_array(scale);
    }
    transform.to_matrix()
}
fn grass_patches(mesh: &Mesh, world: Mat4) -> Result<Arc<[GrassPatch]>, ReferenceError> {
    let Some(VertexAttributeValues::Float32x3(positions)) =
        mesh.attribute(Mesh::ATTRIBUTE_POSITION)
    else {
        return Err(ReferenceError("Missing Grass source POSITION".into()));
    };
    let Some(VertexAttributeValues::Float32x2(uvs)) = mesh.attribute(Mesh::ATTRIBUTE_UV_0) else {
        return Err(ReferenceError("Missing Grass source UV0".into()));
    };
    let indices = mesh
        .indices()
        .ok_or_else(|| ReferenceError("Missing Grass source indices".into()))?;
    let mut result = Vec::with_capacity(indices.len() / 3);
    let mut iter = indices.iter();
    while let Some(a) = iter.next() {
        let b = iter
            .next()
            .ok_or_else(|| ReferenceError("Incomplete Grass triangle".into()))?;
        let c = iter
            .next()
            .ok_or_else(|| ReferenceError("Incomplete Grass triangle".into()))?;
        let ids = [a, b, c];
        result.push(GrassPatch {
            positions: ids.map(|i| {
                world.transform_point3(Vec3::from_array(positions[i])) * Vec3::new(1., 1., -1.)
            }),
            uvs: ids.map(|i| Vec2::from_array(uvs[i])),
        });
    }
    Ok(result.into())
}

fn grass_bounds(
    patches: &[GrassPatch],
    world: Mat4,
    radius: f32,
) -> bevy::camera::primitives::Aabb {
    let inverse = world.inverse();
    let mut min = Vec3::splat(f32::MAX);
    let mut max = Vec3::splat(f32::MIN);
    for point in patches.iter().flat_map(|p| p.positions) {
        let local = inverse.transform_point3(point * Vec3::new(1., 1., -1.));
        min = min.min(local);
        max = max.max(local);
    }
    if patches.is_empty() {
        min = Vec3::ZERO;
        max = Vec3::ZERO;
    }
    let padding = (inverse.x_axis.truncate().abs()
        + inverse.y_axis.truncate().abs()
        + inverse.z_axis.truncate().abs())
        * radius;
    bevy::camera::primitives::Aabb::from_min_max(min - padding, max + padding)
}

fn local_root(placement: &Placement, origin: crate::world::WorldOrigin) -> DMat4 {
    let mut root = Mat4::from_scale_rotation_translation(
        Vec3::from_array(placement.scale),
        Quat::from_array(placement.pose.rotation),
        Vec3::ZERO,
    )
    .as_dmat4();
    root.w_axis = DVec3::from_array(std::array::from_fn(|axis| {
        placement.pose.position[axis] - origin.offset[axis]
    }))
    .extend(1.);
    root
}

/// The shared material of a GPU prop row batch.
fn batch_material(binding: &RendererMaterial) -> bevy::asset::UntypedAssetId {
    match &binding.visible {
        InstanceMaterial::Primary(h) => h.id().untyped(),
        InstanceMaterial::Specular(h) => h.id().untyped(),
        InstanceMaterial::Impostor(h) => h.id().untyped(),
        InstanceMaterial::Grass(_) => unreachable!("Grass renderers are entities"),
    }
}
