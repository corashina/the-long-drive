//! Camera-dependent Grass work uses the world publication and allocation gates.
use super::grass_worker::{DOMAIN_BYTES, GrassJobId, GrassPoll, GrassWorker, INITIAL_WASM_BYTES};
use super::pipelines::{PipelineTicket, WorldPipelines};
use crate::ownership::PresentationLedger;
use crate::{
    presentation::materials::{
        RenderingState,
        grass::material::{BLADE_ATTRIBUTE, GrassMaterial},
    },
    reference::ReferenceError,
    world::{PresentationAllocation, RendererWorld},
};
use bevy::platform::collections::HashMap;
use bevy::{
    asset::RenderAssetUsages,
    ecs as bevy_ecs,
    mesh::{Indices, PrimitiveTopology},
    prelude::*,
};
use grass_mesh::{GrassExpansion, GrassPatch, GrassSettings, GrassView};
use std::sync::Arc;
struct Geometry {
    mesh: Handle<Mesh>,
    material: Handle<GrassMaterial>,
    cpu: usize,
    gpu: usize,
    _ticket: PipelineTicket,
}
enum Pending {
    Queued,
    Worker {
        id: GrassJobId,
        view: GrassView,
        approved: bool,
        canceled: bool,
        scratch: usize,
    },
    Upload {
        mesh: Handle<Mesh>,
        material: Handle<GrassMaterial>,
        view: GrassView,
        ticket: Option<PipelineTicket>,
    },
}
struct Instance {
    continuation: Option<crate::ownership::renderer_pending::PipelineContinuation>,
    upload: Option<Arc<crate::presentation::fixed_assets::MeshUpload>>,
    retirement: crate::ownership::renderer_pending::RendererRetirement,
    entity: Entity,
    parent: Entity,
    last_started: u64,
    session: u64,
    settings: GrassSettings,
    patches: Arc<[GrassPatch]>,
    material: Handle<GrassMaterial>,
    shadows: bool,
    bounds: bevy::camera::primitives::Aabb,
    accepted_view: Option<GrassView>,
    accepted: Option<Geometry>,
    pending: Option<Pending>,
    pending_cpu: usize,
    pending_gpu: usize,
    metadata: usize,
    cache_cpu: usize,
    install_cpu: usize,
    ready: bool,
}
#[derive(Resource, Default)]
pub struct GrassJobs {
    instances: Vec<Instance>,
    /// Instance index by entity.
    index: HashMap<Entity, usize>,
    /// Camera view of this frame. Every instance expands for it.
    view: GrassView,
    /// Per parent: a job is pending, and the stale instance that may start next.
    groups: HashMap<Entity, (bool, Option<(u64, usize)>)>,
    worker: GrassWorker,
    actor_cpu: usize,
    revision: u64,
}
fn allocation(
    ledger: &mut PresentationLedger,
    cpu: usize,
    gpu: usize,
    jobs: i32,
    pending: i32,
) -> Result<(), ReferenceError> {
    let mut next = ledger.allocation();
    next.active_cpu += cpu;
    next.active_gpu += gpu;
    next.jobs = next
        .jobs
        .checked_add_signed(jobs as isize)
        .expect("Grass job ownership");
    next.pending = next
        .pending
        .checked_add_signed(pending as isize)
        .expect("Grass pending ownership");
    ledger.set_allocation(next)
}
fn pending_allocation(
    ledger: &mut PresentationLedger,
    cpu: usize,
    gpu: usize,
    jobs: usize,
) -> Result<(), ReferenceError> {
    let mut next = ledger.allocation();
    next.active_cpu += cpu;
    next.active_gpu += gpu;
    next.jobs += jobs;
    next.staging_bytes += cpu;
    ledger.set_allocation(next)
}
impl GrassJobs {
    pub(crate) fn startup_audit(&self) -> serde_json::Value {
        serde_json::json!({
            "idle": self.instances.iter().filter(|i| !i.ready && i.pending.is_none()).count(),
            "queued": self.instances.iter().filter(|i| matches!(i.pending, Some(Pending::Queued))).count(),
            "expanding": self.instances.iter().filter(|i| matches!(i.pending, Some(Pending::Worker { .. }))).count(),
            "uploading": self.instances.iter().filter(|i| matches!(i.pending, Some(Pending::Upload { .. }))).count(),
        })
    }
    pub fn release_all(
        &mut self,
        meshes: &mut Assets<Mesh>,
        materials: &mut Assets<GrassMaterial>,
    ) {
        self.worker.dispose();
        self.actor_cpu = 0;
        self.index.clear();
        for instance in self.instances.drain(..) {
            if let Some(accepted) = instance.accepted {
                meshes.remove(accepted.mesh.id());
                materials.remove(accepted.material.id());
            }
            if let Some(Pending::Upload { mesh, material, .. }) = instance.pending {
                meshes.remove(mesh.id());
                materials.remove(material.id());
            }
        }
    }
    pub fn settled(&self) -> bool {
        self.instances
            .iter()
            .all(|i| i.ready && i.pending.is_none())
    }

    pub(crate) fn measurement_meshes(
        &self,
    ) -> impl Iterator<Item = (bevy::asset::AssetId<Mesh>, bool)> + '_ {
        self.instances.iter().flat_map(|i| {
            i.accepted
                .iter()
                .map(|g| (g.mesh.id(), false))
                .chain(i.pending.iter().filter_map(|p| match p {
                    Pending::Upload { mesh, .. } => Some((mesh.id(), true)),
                    _ => None,
                }))
        })
    }

    pub fn release_domains(
        &mut self,
        ledger: &mut PresentationLedger,
    ) -> Result<(), ReferenceError> {
        if self.instances.is_empty() && self.actor_cpu > 0 {
            self.release_worker(ledger)?;
        }
        Ok(())
    }

    fn get(&self, entity: Entity) -> Option<&Instance> {
        self.index.get(&entity).map(|&index| &self.instances[index])
    }
    /// Sets the camera view of this frame for all instances.
    pub fn set_view(&mut self, view: GrassView) {
        self.view = view;
    }
    pub fn set_fade(&self, entity: Entity, fade: f32, materials: &mut Assets<GrassMaterial>) {
        if let Some(instance) = self.get(entity) {
            if let Some(accepted) = &instance.accepted {
                if let Some(mut material) = materials.get_mut(&accepted.material) {
                    if material.uniform.lod.x != fade {
                        material.uniform.lod.x = fade;
                    }
                }
            }
            if let Some(Pending::Upload { material, .. }) = &instance.pending {
                if let Some(mut material) = materials.get_mut(material) {
                    if material.uniform.lod.x != fade {
                        material.uniform.lod.x = fade;
                    }
                }
            }
        }
    }
    pub fn ready(&self, entity: Entity) -> bool {
        self.get(entity).is_some_and(|i| i.ready)
    }
    pub fn request(
        &mut self,
        stream: &RendererWorld,
        ledger: &mut PresentationLedger,
        entity: Entity,
        parent: Entity,
        patches: Arc<[GrassPatch]>,
        settings: GrassSettings,
        material: Handle<GrassMaterial>,
        shadows: bool,
        bounds: bevy::camera::primitives::Aabb,
    ) -> Result<(), ReferenceError> {
        let group_idle = !self
            .instances
            .iter()
            .any(|i| i.parent == parent && i.pending.is_some());
        if let Some(&index) = self.index.get(&entity) {
            let instance = &mut self.instances[index];
            if !Arc::ptr_eq(&instance.patches, &patches) || instance.settings != settings {
                return Err(ReferenceError(
                    "Grass entity source changed before retirement".into(),
                ));
            }
            if group_idle
                && instance.pending.is_none()
                && instance.accepted_view != Some(self.view)
                && ledger.can_request_independent(1)
            {
                instance.continuation =
                    Some(ledger.claim_grass(stream.frame().request, entity.to_bits()));
                instance.pending = Some(Pending::Queued);
            }
            return Ok(());
        }
        if self.instances.len() >= 8192 {
            return Err(ReferenceError("Grass instance count cap".into()));
        }
        // The index and group maps hold at most one entry per instance at half load.
        let metadata = std::mem::size_of::<Instance>()
            + 2 * (std::mem::size_of::<(Entity, usize)>()
                + std::mem::size_of::<(Entity, (bool, Option<(u64, usize)>))>()
                + 2);
        let continuation = if group_idle && ledger.can_request_independent(1) {
            allocation(ledger, metadata, 0, 0, 0)?;
            Some(ledger.claim_grass(stream.frame().request, entity.to_bits()))
        } else {
            allocation(ledger, metadata, 0, 0, 0)?;
            None
        };
        let queued = continuation.is_some();
        self.index.insert(entity, self.instances.len());
        self.instances.push(Instance {
            continuation,
            upload: None,
            retirement: Default::default(),
            entity,
            parent,
            last_started: 0,
            session: stream.frame().request.session,
            settings,
            patches,
            material,
            shadows,
            bounds,
            accepted_view: None,
            accepted: None,
            pending: queued.then_some(Pending::Queued),
            pending_cpu: 0,
            pending_gpu: 0,
            metadata,
            cache_cpu: 0,
            install_cpu: 0,
            ready: false,
        });
        Ok(())
    }
    fn reserve_capacity(
        worker: &GrassWorker,
        actor: &mut usize,
        ledger: &mut PresentationLedger,
        staging: usize,
        extra: usize,
    ) -> Result<(), ReferenceError> {
        let mut next = ledger.allocation();
        let logical = next
            .grass_worker_cache_bytes
            .checked_add(staging)
            .and_then(|v| v.checked_add(extra))
            .ok_or_else(|| ReferenceError("Grass capacity overflow".into()))?;
        // Estimate allocator growth separately from logical buffers. Retain the admitted high-water charge.
        let capacity = logical
            .checked_mul(2)
            .and_then(|v| v.checked_add(INITIAL_WASM_BYTES))
            .and_then(usize::checked_next_power_of_two)
            .ok_or_else(|| ReferenceError("Grass capacity overflow".into()))?
            .max(next.grass_worker_reserved_bytes);
        let delta = capacity - next.grass_worker_reserved_bytes;
        next.active_cpu += delta;
        next.grass_worker_reserved_bytes = capacity;
        ledger.set_allocation(next)?;
        *actor += delta;
        worker.capacity_limit(capacity)
    }
    fn release_worker(&mut self, ledger: &mut PresentationLedger) -> Result<(), ReferenceError> {
        self.worker.dispose();
        let mut next = ledger.allocation();
        next.active_cpu -= self.actor_cpu;
        self.actor_cpu = 0;
        for i in &mut self.instances {
            next.active_cpu -= i.cache_cpu + i.install_cpu;
            next.staging_bytes -= i.install_cpu;
            next.jobs -= usize::from(i.install_cpu > 0);
            i.cache_cpu = 0;
            i.install_cpu = 0;
            if matches!(i.pending, Some(Pending::Worker { .. })) {
                next.active_cpu -= i.pending_cpu;
                next.staging_bytes -= i.pending_cpu;
                next.active_gpu -= i.pending_gpu;
                next.jobs -= 1;
                i.pending_cpu = 0;
                i.pending_gpu = 0;
                i.pending = Some(Pending::Queued);
            }
        }
        next.grass_worker_reserved_bytes = 0;
        next.grass_worker_wasm_bytes = 0;
        next.grass_worker_cache_bytes = 0;
        ledger.set_allocation(next)
    }
    pub fn prepare(
        &mut self,
        stream: &RendererWorld,
        ledger: &mut PresentationLedger,
        writers_ready: bool,
        commands: &mut Commands,
        meshes: &mut Assets<Mesh>,
        materials: &mut Assets<GrassMaterial>,
        pipelines: &mut WorldPipelines,
        fixed: &crate::presentation::fixed_assets::SourceFixedAssets,
    ) -> Result<(), ReferenceError> {
        let result = self.prepare_inner(
            stream,
            ledger,
            writers_ready,
            commands,
            meshes,
            materials,
            pipelines,
            fixed,
        );
        if result.is_err() {
            self.release_worker(ledger)?;
        }
        result
    }
    fn prepare_inner(
        &mut self,
        stream: &RendererWorld,
        ledger: &mut PresentationLedger,
        writers_ready: bool,
        commands: &mut Commands,
        meshes: &mut Assets<Mesh>,
        materials: &mut Assets<GrassMaterial>,
        pipelines: &mut WorldPipelines,
        fixed: &crate::presentation::fixed_assets::SourceFixedAssets,
    ) -> Result<(), ReferenceError> {
        if !self.instances.is_empty() {
            if self.actor_cpu == 0 {
                let mut next = ledger.allocation();
                next.active_cpu += INITIAL_WASM_BYTES + DOMAIN_BYTES;
                next.grass_worker_reserved_bytes = INITIAL_WASM_BYTES;
                next.grass_worker_cache_bytes = DOMAIN_BYTES;
                ledger.set_allocation(next)?;
                self.actor_cpu = INITIAL_WASM_BYTES + DOMAIN_BYTES;
            }
            if !self.worker.initialize()? {
                return Ok(());
            }
        }
        let poll = self.worker.poll()?;
        let mut next = ledger.allocation();
        if next.grass_worker_wasm_bytes != self.worker.wasm_bytes() {
            next.grass_worker_wasm_bytes = self.worker.wasm_bytes();
            ledger.set_allocation(next)?;
        }
        let candidate = if self.worker.busy() {
            None
        } else {
            self.instances
                .iter()
                .enumerate()
                .filter(|(_, i)| {
                    !i.retirement.started()
                        && i.session == stream.frame().request.session
                        && matches!(i.pending, Some(Pending::Queued))
                })
                .min_by_key(|(index, i)| (i.last_started, *index))
                .map(|(index, _)| index)
        };
        let worker_staging: usize = self
            .instances
            .iter()
            .map(|i| {
                i.install_cpu
                    + if matches!(i.pending, Some(Pending::Worker { .. })) {
                        i.pending_cpu
                    } else {
                        0
                    }
            })
            .sum();
        // One stale instance per parent starts a job, the least recently started first,
        // and only while no job of that parent is pending.
        let view = self.view;
        self.groups.clear();
        for (index, i) in self.instances.iter().enumerate() {
            let group = self.groups.entry(i.parent).or_default();
            group.0 |= i.pending.is_some();
            if i.accepted_view != Some(view) {
                let key = (i.last_started, index);
                group.1 = Some(group.1.map_or(key, |first| first.min(key)));
            }
        }
        for index in 0..self.instances.len() {
            let needs_child = self.instances[index].pending.is_none()
                && self.instances[index].accepted_view != Some(view);
            let group_idle = needs_child
                && self.groups[&self.instances[index].parent]
                    == (false, Some((self.instances[index].last_started, index)));
            let instance = &mut self.instances[index];
            if instance.retirement.started() || instance.session != stream.frame().request.session {
                continue;
            }
            if group_idle
                && instance.pending.is_none()
                && instance.accepted_view != Some(view)
                && ledger.can_request_independent(1)
            {
                instance.continuation =
                    Some(ledger.claim_grass(stream.frame().request, instance.entity.to_bits()));
                instance.pending = Some(Pending::Queued);
            }
            if instance.install_cpu > 0 && self.worker.installed(instance.entity.to_bits())? {
                let bytes = instance.install_cpu;
                let mut next = ledger.allocation();
                next.active_cpu -= bytes;
                next.staging_bytes -= bytes;
                next.jobs -= 1;
                ledger.set_allocation(next)?;
                instance.install_cpu = 0;
            }
            if writers_ready
                && candidate == Some(index)
                && matches!(instance.pending, Some(Pending::Queued))
                && ledger.available_external_jobs() > 0
            {
                if instance.cache_cpu == 0 {
                    let encoded = grass_mesh::dataset_encoded_len(&instance.patches)
                        .map_err(ReferenceError)?;
                    // Decoded Vec, Arc conversion overlap, encoded main/JS/WASM copies, and metadata.
                    let cache = instance
                        .patches
                        .len()
                        .checked_mul(std::mem::size_of::<GrassPatch>())
                        .and_then(|v| v.checked_add(256))
                        .ok_or_else(|| ReferenceError("Grass cache overflow".into()))?;
                    let copies = encoded
                        .checked_mul(3)
                        .and_then(|v| v.checked_add(cache))
                        .ok_or_else(|| ReferenceError("Grass install overflow".into()))?;
                    Self::reserve_capacity(
                        &self.worker,
                        &mut self.actor_cpu,
                        ledger,
                        worker_staging,
                        cache + copies,
                    )?;
                    let mut next = ledger.allocation();
                    next.active_cpu += cache + copies;
                    next.staging_bytes += copies;
                    next.jobs += 1;
                    next.grass_worker_cache_bytes += cache;
                    ledger.set_allocation(next)?;
                    instance.cache_cpu = cache;
                    instance.install_cpu = copies;
                    self.revision = self
                        .revision
                        .checked_add(1)
                        .ok_or_else(|| ReferenceError("Grass dataset revision exhausted".into()))?;
                    self.worker.packing_started()?;
                    let mut input = Vec::with_capacity(encoded);
                    grass_mesh::encode_dataset(&instance.patches, instance.settings, &mut input);
                    self.worker.packing_ended();
                    self.worker.install(
                        GrassJobId {
                            session: instance.session,
                            revision: self.revision,
                            entity: instance.entity.to_bits(),
                        },
                        &input,
                        GrassExpansion::planning_bytes(instance.patches.len())
                            .map_err(ReferenceError)?,
                    )?;
                }
                if self.worker.installed(instance.entity.to_bits())? {
                    self.revision = self
                        .revision
                        .checked_add(1)
                        .ok_or_else(|| ReferenceError("Grass view revision exhausted".into()))?;
                    instance.last_started = self.revision;
                    let scratch = GrassExpansion::planning_bytes(instance.patches.len())
                        .map_err(ReferenceError)?;
                    // Include captured view copies and transport control storage through physical release.
                    let cpu = scratch
                        .checked_add(1024)
                        .ok_or_else(|| ReferenceError("Grass plan overflow".into()))?;
                    Self::reserve_capacity(
                        &self.worker,
                        &mut self.actor_cpu,
                        ledger,
                        worker_staging,
                        cpu,
                    )?;
                    pending_allocation(ledger, cpu, 0, 1)?;
                    instance.pending_cpu = cpu;
                    let id = GrassJobId {
                        session: instance.session,
                        revision: self.revision,
                        entity: instance.entity.to_bits(),
                    };
                    instance.pending = Some(Pending::Worker {
                        id,
                        view,
                        approved: false,
                        canceled: false,
                        scratch,
                    });
                    self.worker.submit(id, view)?;
                }
            }
            if let Some(Pending::Worker {
                id,
                approved,
                canceled,
                scratch,
                ..
            }) = &mut instance.pending
            {
                if *canceled {
                    if !self.worker.owns(*id) {
                        let mut next = ledger.allocation();
                        next.active_cpu -= instance.pending_cpu;
                        next.staging_bytes -= instance.pending_cpu;
                        next.jobs -= 1;
                        ledger.set_allocation(next)?;
                        instance.pending_cpu = 0;
                        instance.pending_gpu = 0;
                        instance.pending = Some(Pending::Queued);
                    }
                } else if let GrassPoll::Planned(owner, plan) = poll {
                    if owner == *id && !*approved {
                        if plan.scratch_bytes != *scratch {
                            return Err(ReferenceError("Grass scratch plan mismatch".into()));
                        }
                        let cpu = plan
                            .output_bytes
                            .checked_mul(2)
                            .ok_or_else(|| ReferenceError("Grass output overflow".into()))?;
                        // Output that does not fit the credit keeps its plan. The worker
                        // reports the plan again, so the next frame asks for the credit again.
                        let output = PresentationAllocation {
                            active_cpu: cpu,
                            active_gpu: plan.gpu_bytes,
                            staging_bytes: cpu,
                            ..Default::default()
                        };
                        if !ledger.request(output) {
                            continue;
                        }
                        if Self::reserve_capacity(
                            &self.worker,
                            &mut self.actor_cpu,
                            ledger,
                            worker_staging,
                            cpu,
                        )
                        .is_err()
                            || pending_allocation(ledger, cpu, plan.gpu_bytes, 0).is_err()
                        {
                            self.worker.discard();
                            *canceled = true;
                        } else {
                            instance.pending_cpu += cpu;
                            instance.pending_gpu += plan.gpu_bytes;
                            self.worker.approve(*id, plan.output_bytes)?;
                            *approved = true;
                        }
                    }
                }
            }
            let completed = matches!((&instance.pending,poll),(Some(Pending::Worker{id,canceled:false,..}),GrassPoll::Ready(owner)) if *id==owner);
            if completed {
                pending_allocation(ledger, 4096, 8, 0)?;
                instance.pending_cpu += 4096;
                instance.pending_gpu += 8;
                let Some(Pending::Worker { id, view, .. }) = instance.pending.as_ref() else {
                    unreachable!()
                };
                let (id, view) = (*id, *view);
                let mut material = materials
                    .get(&instance.material)
                    .ok_or_else(|| ReferenceError("Missing Grass material owner".into()))?
                    .clone();
                let output = self.worker.consume(id)?;
                // consume physically releases the worker plan, worker output and transferred arrays.
                let retained = output.retained_bytes();
                let released = instance
                    .pending_cpu
                    .checked_sub(retained + 4096)
                    .ok_or_else(|| ReferenceError("Grass result reservation mismatch".into()))?;
                let mut next = ledger.allocation();
                next.active_cpu -= released;
                next.staging_bytes -= released;
                next.jobs -= 1;
                ledger.set_allocation(next)?;
                instance.pending_cpu -= released;
                let mut mesh = Mesh::new(
                    PrimitiveTopology::TriangleList,
                    RenderAssetUsages::MAIN_WORLD | RenderAssetUsages::RENDER_WORLD,
                );
                mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, output.anchors);
                mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, output.uvs);
                mesh.insert_attribute(BLADE_ATTRIBUTE, output.blades);
                mesh.insert_indices(Indices::U32(output.indices));

                material.uniform.camera = view.position.extend(view.screen_height);
                let mesh = meshes.add(mesh);
                let upload = fixed.watch_meshes(std::iter::once(mesh.clone()));
                instance
                    .continuation
                    .as_mut()
                    .unwrap()
                    .watch_upload(upload.receipt.clone());
                instance.upload = Some(upload);
                instance.pending = Some(Pending::Upload {
                    mesh,
                    material: materials.add(material),
                    view,
                    ticket: None,
                });
            }
            if let Some(Pending::Upload {
                mesh,
                material,
                ticket,
                ..
            }) = &mut instance.pending
            {
                PipelineTicket::retry(ticket);
                if ticket.is_none() {
                    *ticket = pipelines.request_continuation(
                        stream,
                        ledger,
                        instance.continuation.as_mut().unwrap(),
                        mesh,
                        material,
                        instance.shadows,
                    )?;
                }
                if let Some(ticket) = ticket {
                    match ticket.state() {
                        RenderingState::Failed(error) => return Err(ReferenceError(error)),
                        RenderingState::Loading => continue,
                        RenderingState::Ready => {}
                    }
                    if !instance.continuation.as_ref().unwrap().drained()
                        || !writers_ready
                        || !ledger.take_publication()
                    {
                        continue;
                    }
                    let Some(Pending::Upload {
                        mesh,
                        material,
                        view,
                        ticket,
                    }) = instance.pending.take()
                    else {
                        unreachable!()
                    };
                    if let Some(old) = instance.accepted.take() {
                        meshes.remove(old.mesh.id());
                        materials.remove(old.material.id());
                        let mut next = ledger.allocation();
                        next.active_cpu = next.active_cpu.saturating_sub(old.cpu);
                        next.active_gpu = next.active_gpu.saturating_sub(old.gpu);
                        ledger.set_allocation(next)?;
                    }
                    self.worker.publication_started(instance.entity.to_bits());
                    commands.entity(instance.entity).insert((
                        Mesh3d(mesh.clone()),
                        MeshMaterial3d(material.clone()),
                        instance.bounds,
                    ));
                    instance.accepted_view = Some(view);
                    instance.accepted = Some(Geometry {
                        mesh,
                        material,
                        cpu: instance.pending_cpu,
                        gpu: instance.pending_gpu,
                        _ticket: ticket.unwrap(),
                    });
                    let mut next = ledger.allocation();
                    next.staging_bytes = next.staging_bytes.saturating_sub(instance.pending_cpu);
                    ledger.set_allocation(next)?;
                    instance.pending_cpu = 0;
                    instance.pending_gpu = 0;
                    instance.ready = true;
                    ledger.release_claim(instance.continuation.as_mut().unwrap())?;
                    instance.continuation = None;
                    instance.upload = None;
                    self.worker
                        .publication_ended(instance.entity.to_bits(), true);
                }
            }
        }
        Ok(())
    }
    pub fn retire(
        &mut self,
        ledger: &mut PresentationLedger,
        entity: Entity,
        meshes: &mut Assets<Mesh>,
        materials: &mut Assets<GrassMaterial>,
    ) -> Result<bool, ReferenceError> {
        let result = self.retire_inner(ledger, entity, meshes, materials);
        if result.is_err() {
            self.release_worker(ledger)?;
        }
        result
    }
    fn retire_inner(
        &mut self,
        ledger: &mut PresentationLedger,
        entity: Entity,
        meshes: &mut Assets<Mesh>,
        materials: &mut Assets<GrassMaterial>,
    ) -> Result<bool, ReferenceError> {
        let Some(&index) = self.index.get(&entity) else {
            return Ok(true);
        };
        self.instances[index].retirement.keep(false);
        if let Some(parent) = &self.instances[index].continuation {
            parent.stop_flight();
        }
        if let Some(Pending::Worker { id, canceled, .. }) = &mut self.instances[index].pending {
            if !*canceled {
                self.worker.discard();
                *canceled = true;
            }
            self.worker.poll()?;
            if self.worker.owns(*id) {
                return Ok(false);
            }
        }
        let entity_bits = self.instances[index].entity.to_bits();
        if self.instances[index].cache_cpu > 0 {
            if !self.worker.retiring(entity_bits)? {
                return Ok(false);
            }
            self.worker.remove(entity_bits);
        }
        let physical = self.instances[index]
            .continuation
            .as_ref()
            .is_none_or(|c| c.drained());
        if !self.instances[index].retirement.drained(physical, true) {
            return Ok(false);
        }
        let mut instance = self.instances.swap_remove(index);
        self.index.remove(&instance.entity);
        if let Some(moved) = self.instances.get(index) {
            self.index.insert(moved.entity, index);
        }
        self.worker
            .publication_ended(instance.entity.to_bits(), false);
        let mut cpu =
            instance.metadata + instance.pending_cpu + instance.cache_cpu + instance.install_cpu;
        let mut gpu = instance.pending_gpu;
        if let Some(old) = instance.accepted {
            cpu += old.cpu;
            gpu += old.gpu;
            meshes.remove(old.mesh.id());
            materials.remove(old.material.id());
        }
        if let Some(Pending::Upload { mesh, material, .. }) = &instance.pending {
            meshes.remove(mesh.id());
            materials.remove(material.id());
        }
        let mut next = ledger.allocation();
        next.active_cpu = next.active_cpu.saturating_sub(cpu);
        next.staging_bytes = next
            .staging_bytes
            .saturating_sub(instance.pending_cpu + instance.install_cpu);
        next.grass_worker_cache_bytes -= instance.cache_cpu;
        next.jobs -= usize::from(instance.install_cpu > 0);
        next.active_gpu = next.active_gpu.saturating_sub(gpu);
        next.jobs = next.jobs.saturating_sub(usize::from(matches!(
            instance.pending,
            Some(Pending::Worker { .. })
        )));
        ledger.set_allocation(next)?;
        if let Some(claim) = instance.continuation.as_mut() {
            ledger.release_claim(claim)?;
        }
        if self.instances.is_empty() {
            self.release_worker(ledger)?;
        }
        Ok(true)
    }
}

impl Drop for GrassJobs {
    fn drop(&mut self) {
        self.worker.dispose();
    }
}
