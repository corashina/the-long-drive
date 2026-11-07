//! Camera-dependent Grass work uses the world publication and allocation gates.
use super::grass_worker::{GrassJobId, GrassPoll, GrassWorker};
use super::pipelines::{PipelineTicket, WorldPipelines};
use crate::ownership::PresentationLedger;
use crate::{
    presentation::materials::{
        RenderingState,
        grass::material::{BLADE_ATTRIBUTE, GrassMaterial},
    },
    reference::ReferenceError,
    world::RendererWorld,
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
/// Worker copy of the instance patch dataset.
#[derive(Clone, Copy)]
enum Dataset {
    None,
    /// Sent to the worker. The install job runs until the worker reports it installed.
    Installing,
    Cached,
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
    dataset: Dataset,
    ready: bool,
}
#[derive(Resource, Default)]
pub struct GrassJobs {
    instances: Vec<Instance>,
    /// Instance index by entity.
    index: HashMap<Entity, usize>,
    /// Camera view of this frame. An instance expands again when its view no longer covers it.
    view: GrassView,
    /// Per parent, whether a job is pending, and the stale instance that may start next.
    groups: HashMap<Entity, (bool, Option<(u64, usize)>)>,
    worker: GrassWorker,
    /// The worker started.
    started: bool,
    revision: u64,
}
/// Whether the instance needs a new expansion for this view.
fn stale(accepted: Option<GrassView>, view: &GrassView) -> bool {
    !accepted.is_some_and(|accepted| accepted.covers(view))
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
        if self.instances.is_empty() && self.started {
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
            if let Some(accepted) = &instance.accepted
                && let Some(mut material) = materials.get_mut(&accepted.material)
                && material.uniform.lod.x != fade
            {
                material.uniform.lod.x = fade;
            }
            if let Some(Pending::Upload { material, .. }) = &instance.pending
                && let Some(mut material) = materials.get_mut(material)
                && material.uniform.lod.x != fade
            {
                material.uniform.lod.x = fade;
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
                && stale(instance.accepted_view, &self.view)
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
        let continuation = (group_idle && ledger.can_request_independent(1))
            .then(|| ledger.claim_grass(stream.frame().request, entity.to_bits()));
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
            dataset: Dataset::None,
            ready: false,
        });
        Ok(())
    }
    fn release_worker(&mut self, ledger: &mut PresentationLedger) -> Result<(), ReferenceError> {
        self.worker.dispose();
        self.started = false;
        for i in &mut self.instances {
            ledger.end_jobs(usize::from(matches!(i.dataset, Dataset::Installing)))?;
            i.dataset = Dataset::None;
            if matches!(i.pending, Some(Pending::Worker { .. })) {
                ledger.end_jobs(1)?;
                i.pending = Some(Pending::Queued);
            }
        }
        Ok(())
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
        uploads: &mut crate::presentation_logic::upload_budget::UploadBudget,
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
            uploads,
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
        uploads: &mut crate::presentation_logic::upload_budget::UploadBudget,
    ) -> Result<(), ReferenceError> {
        if !self.instances.is_empty() {
            self.started = true;
            if !self.worker.initialize()? {
                return Ok(());
            }
        }
        let poll = self.worker.poll()?;
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
        // One stale instance per parent starts a job, the least recently started first,
        // and only while no job of that parent is pending.
        let view = self.view;
        self.groups.clear();
        for (index, i) in self.instances.iter().enumerate() {
            let group = self.groups.entry(i.parent).or_default();
            group.0 |= i.pending.is_some();
            if stale(i.accepted_view, &view) {
                let key = (i.last_started, index);
                group.1 = Some(group.1.map_or(key, |first| first.min(key)));
            }
        }
        for index in 0..self.instances.len() {
            let needs_child = self.instances[index].pending.is_none()
                && stale(self.instances[index].accepted_view, &view);
            let group_idle = needs_child
                && self.groups[&self.instances[index].parent]
                    == (false, Some((self.instances[index].last_started, index)));
            let instance = &mut self.instances[index];
            if instance.retirement.started() || instance.session != stream.frame().request.session {
                continue;
            }
            if group_idle
                && instance.pending.is_none()
                && stale(instance.accepted_view, &view)
                && ledger.can_request_independent(1)
            {
                instance.continuation =
                    Some(ledger.claim_grass(stream.frame().request, instance.entity.to_bits()));
                instance.pending = Some(Pending::Queued);
            }
            if matches!(instance.dataset, Dataset::Installing)
                && self.worker.installed(instance.entity.to_bits())?
            {
                ledger.end_jobs(1)?;
                instance.dataset = Dataset::Cached;
            }
            if writers_ready
                && candidate == Some(index)
                && matches!(instance.pending, Some(Pending::Queued))
                && ledger.available_external_jobs() > 0
            {
                if matches!(instance.dataset, Dataset::None) {
                    let encoded = grass_mesh::dataset_encoded_len(&instance.patches)
                        .map_err(ReferenceError)?;
                    ledger.start_jobs(1);
                    instance.dataset = Dataset::Installing;
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
                    ledger.start_jobs(1);
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
                        ledger.end_jobs(1)?;
                        instance.pending = Some(Pending::Queued);
                    }
                } else if let GrassPoll::Planned(owner, plan) = poll
                    && owner == *id
                    && !*approved
                {
                    if plan.scratch_bytes != *scratch {
                        return Err(ReferenceError("Grass scratch plan mismatch".into()));
                    }
                    self.worker.approve(*id, plan.output_bytes)?;
                    *approved = true;
                }
            }
            let completed = matches!((&instance.pending,poll),(Some(Pending::Worker{id,canceled:false,..}),GrassPoll::Ready(owner)) if *id==owner);
            if completed {
                let Some(Pending::Worker { id, view, .. }) = instance.pending.as_ref() else {
                    unreachable!()
                };
                let (id, view) = (*id, *view);
                let output = self.worker.consume(id)?;
                // The view or the distance can leave no blades. An empty mesh never uploads, so
                // the instance draws nothing and is ready at once.
                ledger.end_jobs(1)?;
                if output.indices.is_empty() {
                    if let Some(old) = instance.accepted.take() {
                        meshes.remove(old.mesh.id());
                        materials.remove(old.material.id());
                    }
                    commands
                        .entity(instance.entity)
                        .remove::<(Mesh3d, MeshMaterial3d<GrassMaterial>)>();
                    self.worker.publication_started(instance.entity.to_bits());
                    instance.pending = None;
                    instance.accepted_view = Some(view);
                    instance.ready = true;
                    ledger.release_claim(instance.continuation.as_mut().unwrap())?;
                    instance.continuation = None;
                    instance.upload = None;
                    self.worker
                        .publication_ended(instance.entity.to_bits(), true);
                    continue;
                }
                let mut mesh = Mesh::new(
                    PrimitiveTopology::TriangleList,
                    RenderAssetUsages::RENDER_WORLD,
                );
                mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, output.anchors);
                mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, output.uvs);
                mesh.insert_attribute(BLADE_ATTRIBUTE, output.blades);
                mesh.insert_indices(Indices::U32(output.indices));
                // The material holds no view, so a rebuild keeps the accepted material.
                let material = match &instance.accepted {
                    Some(accepted) => accepted.material.clone(),
                    None => materials.add(
                        materials
                            .get(&instance.material)
                            .ok_or_else(|| ReferenceError("Missing Grass material owner".into()))?
                            .clone(),
                    ),
                };
                let mesh = uploads.add(meshes, mesh);
                let upload = fixed.watch_meshes(std::iter::once(mesh.clone()));
                instance
                    .continuation
                    .as_mut()
                    .unwrap()
                    .watch_upload(upload.receipt.clone());
                instance.upload = Some(upload);
                instance.pending = Some(Pending::Upload {
                    mesh,
                    material,
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
                        if old.material != material {
                            materials.remove(old.material.id());
                        }
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
                        _ticket: ticket.unwrap(),
                    });
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
        if !matches!(self.instances[index].dataset, Dataset::None) {
            if !self.worker.retiring(entity_bits)? {
                return Ok(false);
            }
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
        if let Some(old) = instance.accepted {
            meshes.remove(old.mesh.id());
            materials.remove(old.material.id());
        }
        if let Some(Pending::Upload { mesh, material, .. }) = &instance.pending {
            meshes.remove(mesh.id());
            materials.remove(material.id());
        }
        ledger.end_jobs(
            usize::from(matches!(instance.dataset, Dataset::Installing))
                + usize::from(matches!(instance.pending, Some(Pending::Worker { .. }))),
        )?;
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
