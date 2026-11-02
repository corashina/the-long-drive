//! Compile admitted material variants before collision and visual publication.
use crate::ownership::PresentationLedger;
use crate::ownership::renderer_pending::{
    PIPELINE_BATCH_KEYS, PIPELINE_DIAGNOSTIC_BYTES, PipelineAccounting, PipelineBufferGrowth,
    PipelineContinuation, PipelineOperation, pipeline_diagnostic, pipeline_pair_ready,
    write_pipeline_diagnostic,
};
use crate::ownership::upload_receipt::UploadReceipt;
use crate::{
    presentation::materials::RenderingState, reference::ReferenceError, world::RendererWorld,
};
use bevy::render::renderer::RenderQueue;
use bevy::{
    asset::{AssetId, UntypedAssetId},
    ecs as bevy_ecs,
    material::key::{ErasedMaterialPipelineKey, ErasedMeshPipelineKey},
    pbr::{
        MeshPipelineKey, PreparedMaterial, ViewKeyCache, alpha_mode_pipeline_key, base_specialize,
    },
    prelude::*,
    render as bevy_render,
    render::{
        Render, RenderApp, RenderSystems,
        erased_render_asset::ErasedRenderAssets,
        extract_resource::{ExtractResource, ExtractResourcePlugin},
        mesh::RenderMesh,
        render_asset::RenderAssets,
        render_resource::*,
    },
    shader::ShaderCacheError,
};
use std::sync::{Arc, Mutex};

fn arc_bytes<T>() -> usize {
    std::alloc::Layout::new::<[usize; 2]>()
        .extend(std::alloc::Layout::new::<T>())
        .unwrap()
        .0
        .pad_to_align()
        .size()
}
struct PipelineJob {
    physical: UploadReceipt,
    session: u64,
    mesh: AssetId<Mesh>,
    material: UntypedAssetId,
    material_type: &'static str,
    shadows: bool,
    state: RenderingState,
    waiting: &'static str,
    dispatched: bool,
    spill: bool,
    ids: Vec<CachedRenderPipelineId>,
}
#[derive(Clone)]
pub struct PipelineTicket(Arc<Mutex<PipelineJob>>);
impl PipelineTicket {
    pub fn retry(slot: &mut Option<Self>) {
        if slot.as_ref().is_some_and(|ticket| {
            let job = ticket.0.lock().unwrap();
            job.spill && job.physical.completed()
        }) {
            *slot = None;
        }
    }
    pub fn state(&self) -> RenderingState {
        let job = self.0.lock().unwrap();
        if !pipeline_pair_ready(&job.physical, job.spill) {
            RenderingState::Loading
        } else {
            job.state.clone()
        }
    }
}
type PipelineBatch = crate::ownership::renderer_pending::PipelineBatch<Mutex<PipelineJob>>;
#[derive(Default)]
struct PipelineBank {
    pairs: Vec<PipelineTicket>,
    batches: Vec<PipelineBatch>,
}
#[derive(Resource, Clone, ExtractResource, Default)]
pub struct WorldPipelines {
    jobs: Option<Arc<Mutex<PipelineBank>>>,
    accounting: PipelineAccounting,
    cache_reserved: bool,
    session: u64,
}
pub struct WorldPipelinePlugin;
impl Plugin for WorldPipelinePlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<WorldPipelines>()
            .add_plugins(ExtractResourcePlugin::<WorldPipelines>::default());
        app.sub_app_mut(RenderApp)
            .init_resource::<super::super::pipeline_ownership::PipelineOwners>()
            .add_systems(
                Render,
                super::super::pipeline_ownership::prepare
                    .before(RenderSystems::Render)
                    .after(compile),
            )
            .add_systems(Render, compile.after(RenderSystems::PrepareBindGroups));
    }
}
impl WorldPipelines {
    pub(crate) fn startup_audit(&self) -> serde_json::Value {
        let Some(bank) = &self.jobs else {
            return serde_json::json!({"retained":0,"physical_batches":0});
        };
        let bank = bank.lock().unwrap();
        let mut requests = std::collections::HashSet::new();
        let mut compiled = std::collections::HashSet::new();
        let mut ready = 0;
        for ticket in &bank.pairs {
            let job = ticket.0.lock().unwrap();
            requests.insert((job.mesh, job.material, job.shadows));
            compiled.extend(job.ids.iter().copied());
            ready += usize::from(
                job.state == RenderingState::Ready && pipeline_pair_ready(&job.physical, job.spill),
            );
        }
        serde_json::json!({
            "retained": bank.pairs.len(), "unique_requests":requests.len(),
            "unique_compiled_pipelines":compiled.len(), "ready":ready,
            "physical_batches":bank.batches.len(),
            "incomplete_batches":bank.batches.iter().filter(|b|!b.operation.physical.completed()).count(),
            "batch_pairs":bank.batches.iter().map(|b|b.work.pairs).sum::<usize>(),
            "batch_base_keys":bank.batches.iter().map(|b|b.work.base_keys).sum::<usize>(),
            "batch_base_requests":bank.batches.iter().map(|b|b.dispatched_keys).sum::<usize>(),
            "pending": bank.pairs.iter().filter_map(|ticket| {
                let job = ticket.0.lock().unwrap();
                (job.state != RenderingState::Ready || !pipeline_pair_ready(&job.physical,job.spill)).then(||serde_json::json!({
                    "material":job.material_type,"shadows":job.shadows,"state":format!("{:?}",job.state),
                    "waiting":job.waiting,"pipelines":job.ids.len(),"retry":job.spill,"stopped":job.physical.stopped(),
                }))
            }).collect::<Vec<_>>(),
        })
    }
    pub fn reserve_cache(&mut self, ledger: &mut PresentationLedger) -> Result<(), ReferenceError> {
        if !self.cache_reserved {
            let mut allocation = ledger.allocation();
            allocation.prototype_cpu += super::super::pipeline_ownership::CACHE_BYTES;
            ledger.set_allocation(allocation)?;
            self.cache_reserved = true;
        }
        Ok(())
    }
    pub fn owns(&self, id: CachedRenderPipelineId) -> bool {
        self.jobs.as_ref().is_some_and(|bank| {
            bank.lock()
                .unwrap()
                .pairs
                .iter()
                .any(|ticket| ticket.0.lock().unwrap().ids.contains(&id))
        })
    }
    pub fn settled(&self) -> Result<bool, ReferenceError> {
        let Some(bank) = &self.jobs else {
            return Ok(true);
        };
        for ticket in &bank.lock().unwrap().pairs {
            match ticket.state() {
                RenderingState::Loading => return Ok(false),
                RenderingState::Failed(e) => return Err(ReferenceError(e)),
                RenderingState::Ready => {}
            }
        }
        Ok(true)
    }

    pub(crate) fn request_continuation<M: Material>(
        &mut self,
        stream: &RendererWorld,
        ledger: &mut PresentationLedger,
        parent: &mut PipelineContinuation,
        mesh: &Handle<Mesh>,
        material: &Handle<M>,
        shadows: bool,
    ) -> Result<Option<PipelineTicket>, ReferenceError> {
        self.request(stream, ledger, Some(parent), mesh, material, shadows)
    }
    /// Without a parent, the request is an independent operation.
    pub(crate) fn request<M: Material>(
        &mut self,
        stream: &RendererWorld,
        ledger: &mut PresentationLedger,
        mut parent: Option<&mut PipelineContinuation>,
        mesh: &Handle<Mesh>,
        material: &Handle<M>,
        shadows: bool,
    ) -> Result<Option<PipelineTicket>, ReferenceError> {
        if let Some(parent) = parent.as_ref() {
            parent.validate(stream)?;
        }
        // This local Arc adds no allocation. It keeps the bank lock order bank -> pair.
        let mut shared = self.jobs.clone();
        let mut guard = shared.as_ref().map(|bank| bank.lock().unwrap());
        let append = guard.as_ref().and_then(|bank| {
            parent.as_ref().and_then(|parent| {
                bank.batches.iter().position(|batch| {
                    batch.open
                        && batch.len < PIPELINE_BATCH_KEYS
                        && parent.shares(&batch.operation.physical)
                })
            })
        });
        if append.is_none() && parent.as_ref().is_some_and(|parent| !parent.drained()) {
            return Ok(None);
        }
        let (pair_len, pair_cap, batch_len, batch_cap) =
            guard.as_ref().map_or((0, 0, 0, 0), |bank| {
                (
                    bank.pairs.len(),
                    bank.pairs.capacity(),
                    bank.batches.len(),
                    bank.batches.capacity(),
                )
            });
        let pair_growth_plan =
            PipelineBufferGrowth::new(pair_cap, pair_len + 1, size_of::<PipelineTicket>());
        let batch_growth_plan = PipelineBufferGrowth::new(
            batch_cap,
            batch_len + usize::from(append.is_none()),
            size_of::<PipelineBatch>(),
        );
        let pair_capacity = pair_growth_plan.capacity;
        let batch_capacity = batch_growth_plan.capacity;
        let pair_growth = pair_growth_plan.admitted;
        let batch_growth = batch_growth_plan.admitted;
        let cpu = ticket_bytes()
            + pair_growth
            + batch_growth
            + if shared.is_none() {
                arc_bytes::<Mutex<PipelineBank>>()
            } else {
                0
            }
            + if self.cache_reserved {
                0
            } else {
                super::super::pipeline_ownership::CACHE_BYTES
            }
            + if append.is_none() {
                UploadReceipt::allocation_bytes()
            } else {
                0
            };
        let operation = if let Some(index) = append {
            let operation = &guard.as_ref().unwrap().batches[index].operation;
            operation.reserve_pair(ledger, cpu)?;
            operation.clone()
        } else {
            let operation = match parent.as_mut() {
                Some(parent) => parent.start(
                    stream,
                    ledger,
                    cpu,
                    super::super::pipeline_ownership::DESCRIPTOR_BYTES,
                )?,
                None => PipelineOperation::independent(
                    ledger,
                    cpu,
                    super::super::pipeline_ownership::DESCRIPTOR_BYTES,
                )?,
            };
            let Some(operation) = operation else {
                return Ok(None);
            };
            self.accounting.admitted(&operation);
            operation
        };
        if shared.is_none() {
            drop(guard);
            shared = Some(Arc::new(Mutex::new(PipelineBank::default())));
            self.jobs = shared.clone();
            guard = Some(shared.as_ref().unwrap().lock().unwrap());
        }
        let bank = guard.as_mut().unwrap();
        let mut released = 0;
        if pair_growth != 0 {
            grow(&mut bank.pairs, pair_capacity);
            released += pair_growth_plan.released;
        }
        if batch_growth != 0 {
            grow(&mut bank.batches, batch_capacity);
            released += batch_growth_plan.released;
        }
        let ids = Vec::with_capacity(PIPELINE_BATCH_KEYS);
        assert_eq!(ids.capacity(), PIPELINE_BATCH_KEYS);
        let ticket = PipelineTicket(Arc::new(Mutex::new(PipelineJob {
            physical: operation.physical.clone(),
            session: stream.frame().request.session,
            mesh: mesh.id(),
            material: material.id().untyped(),
            material_type: std::any::type_name::<M>(),
            shadows,
            state: RenderingState::Loading,
            waiting: "queued",
            dispatched: false,
            spill: false,
            ids,
        })));
        let index = if let Some(index) = append {
            index
        } else {
            bank.batches.push(PipelineBatch::new(operation));
            bank.batches.len() - 1
        };
        let batch = &mut bank.batches[index];
        batch.push(&ticket.0);
        bank.pairs.push(ticket.clone());
        self.accounting.pair_admitted(ticket_bytes());
        self.session = stream.frame().request.session;
        self.cache_reserved = true;
        if released != 0 {
            let mut allocation = ledger.allocation();
            allocation.prototype_cpu -= released;
            ledger.set_allocation(allocation)?;
        }
        Ok(Some(ticket))
    }
    pub fn prepare(
        &mut self,
        stream: &RendererWorld,
        ledger: &mut PresentationLedger,
    ) -> Result<(), ReferenceError> {
        self.session = stream.frame().request.session;
        let Some(bank) = &self.jobs else {
            return Ok(());
        };
        let mut bank = bank.lock().unwrap();
        for ticket in &bank.pairs {
            let mut job = ticket.0.lock().unwrap();
            if job.session != self.session {
                job.physical.stop();
                job.waiting = "stale session";
            }
        }
        // Drop batch Weak references before dropping the admitted pair control blocks.
        for batch in &mut bank.batches {
            batch.retire_pairs();
        }
        bank.batches.retain(PipelineBatch::retain);
        bank.pairs.retain(|ticket| {
            let job = ticket.0.lock().unwrap();
            !job.physical.completed()
                || Arc::strong_count(&ticket.0) > 1
                || Arc::weak_count(&ticket.0) != 0
        });
        let cpu = bank.pairs.len() * ticket_bytes()
            + bank.batches.len() * UploadReceipt::allocation_bytes();
        self.accounting.reconcile(
            ledger,
            bank.batches.iter().map(|batch| batch.operation.clone()),
            cpu,
            super::super::pipeline_ownership::DESCRIPTOR_BYTES,
        )
    }
}
fn grow<T>(buffer: &mut Vec<T>, capacity: usize) {
    let mut next = Vec::with_capacity(capacity);
    assert_eq!(next.capacity(), capacity);
    next.append(buffer);
    *buffer = next;
}
fn ticket_bytes() -> usize {
    arc_bytes::<Mutex<PipelineJob>>()
        + PIPELINE_BATCH_KEYS * size_of::<CachedRenderPipelineId>()
        + PIPELINE_DIAGNOSTIC_BYTES
}

// The keys live only on the render stack, then freeze into the dispatched IDs.
type PairKeys = crate::ownership::renderer_pending::PipelineKeys<(MeshPipelineKey, bool)>;
fn pair_keys(world: &World, job: &mut PipelineJob) -> Result<Option<PairKeys>, ReferenceError> {
    if job.material.type_id()
        == std::any::TypeId::of::<crate::presentation::materials::SourceMaterial>()
        && !crate::presentation::caster_material::ready(world, job.material.typed())
    {
        job.waiting = "caster material";
        return Ok(None);
    }
    let Some(mesh) = world.resource::<RenderAssets<RenderMesh>>().get(job.mesh) else {
        job.waiting = "mesh";
        return Ok(None);
    };
    let Some(material) = world
        .resource::<ErasedRenderAssets<PreparedMaterial>>()
        .get(job.material)
    else {
        job.waiting = "material";
        return Ok(None);
    };
    let views = world.resource::<ViewKeyCache>();
    if views.values().next().is_none() {
        job.waiting = "view keys";
        return Ok(None);
    }
    let properties = &material.properties;
    let mesh_bits = MeshPipelineKey::from_bits_retain(mesh.key_bits.bits());
    let mut keys = PairKeys::new();
    for view_key in views.values().copied() {
        let bits: MeshPipelineKey = properties.mesh_pipeline_key_bits.downcast();
        keys.push((
            view_key
                | mesh_bits
                | bits
                | alpha_mode_pipeline_key(
                    properties.alpha_mode,
                    &Msaa::from_samples(view_key.msaa_samples()),
                ),
            false,
        ))?;
    }
    if job.shadows && properties.shadows_enabled {
        for light_key in [
            MeshPipelineKey::DEPTH_PREPASS
                | MeshPipelineKey::VIEW_PROJECTION_ORTHOGRAPHIC
                | MeshPipelineKey::UNCLIPPED_DEPTH_ORTHO,
            MeshPipelineKey::DEPTH_PREPASS | MeshPipelineKey::VIEW_PROJECTION_PERSPECTIVE,
        ] {
            let mut key = light_key | mesh_bits;
            if properties.alpha_mode != AlphaMode::Opaque {
                key |= MeshPipelineKey::MAY_DISCARD;
            }
            if properties
                .get_shader(bevy::pbr::PrepassVertexShader)
                .is_some()
                || properties
                    .get_shader(bevy::pbr::PrepassFragmentShader)
                    .is_some()
            {
                key |= MeshPipelineKey::PREPASS_READS_MATERIAL;
            }
            keys.push((key, true))?;
        }
    }
    Ok(Some(keys))
}
fn compile(world: &mut World) {
    let Some(active) = world.get_resource::<WorldPipelines>().cloned() else {
        return;
    };
    super::super::pipeline_ownership::refresh_isolation(world);
    {
        let Some(shared) = &active.jobs else {
            return;
        };
        let mut shared = shared.lock().unwrap();
        for batch in &mut shared.batches {
            if batch.operation.physical.completed() {
                continue;
            }
            batch.open = false;
            let cancelled = batch.operation.physical.stopped();
            if cancelled {
                batch.stopped = true;
            }
            for pair in &batch.pairs[..batch.next] {
                let pair = pair.upgrade().expect("admitted pair control");
                let mut job = pair.lock().unwrap();
                if cancelled {
                    job.waiting = "cancelled";
                }
                if !cancelled && job.state == RenderingState::Loading {
                    compile_pair(world, &mut job, None);
                }
                if matches!(job.state, RenderingState::Failed(_)) {
                    batch.stopped = true;
                }
            }
            while batch.next < batch.len && !batch.stopped {
                let pair = batch.pairs[batch.next]
                    .upgrade()
                    .expect("admitted pair control");
                let mut job = pair.lock().unwrap();
                if matches!(job.state, RenderingState::Failed(_)) {
                    batch.stopped = true;
                    break;
                }
                match pair_keys(world, &mut job) {
                    Ok(Some(keys)) => match batch.select(Some(keys.len)) {
                        Ok(true) => {
                            job.dispatched = true;
                            batch.dispatched_keys += compile_pair(world, &mut job, Some(&keys));
                            if job.physical.stopped()
                                || matches!(job.state, RenderingState::Failed(_))
                            {
                                batch.stopped = true;
                            }
                        }
                        Ok(false) => {
                            batch.stopped = true;
                        }
                        Err(error) => {
                            job.state = RenderingState::Failed(pipeline_diagnostic(format_args!(
                                "{}",
                                error.0
                            )));
                            batch.stopped = true;
                        }
                    },
                    Ok(None) => {
                        batch
                            .select(None)
                            .expect("waiting has no specialization error");
                        break;
                    }
                    Err(error) => {
                        job.state = RenderingState::Failed(pipeline_diagnostic(format_args!(
                            "{}",
                            error.0
                        )));
                        batch.stopped = true;
                    }
                }
            }
            if batch.stopped {
                for pair in &batch.pairs[batch.next..batch.len] {
                    let pair = pair.upgrade().expect("admitted pair control");
                    let mut job = pair.lock().unwrap();
                    if !matches!(job.state, RenderingState::Failed(_)) {
                        job.spill = true;
                        job.waiting = "batch retry";
                    }
                }
            }
            let cancelled = cancelled || batch.operation.physical.stopped();
            let settled = batch.pairs[..batch.next].iter().map(|pair| {
                let pair = pair.upgrade().expect("admitted pair control");
                let job = pair.lock().unwrap();
                (cancelled || job.state != RenderingState::Loading)
                    && physical_pipelines_settled(world, &job)
            });
            if batch.register(settled.chain(std::iter::once(true))) {
                let receipt = batch.operation.physical.clone();
                world
                    .resource::<RenderQueue>()
                    .on_submitted_work_done(move || receipt.complete());
            }
        }
    }
}
fn compile_pair(world: &mut World, job: &mut PipelineJob, keys: Option<&PairKeys>) -> usize {
    let mut dispatched = 0;
    if let Some(keys) = keys {
        let layout = world
            .resource::<RenderAssets<RenderMesh>>()
            .get(job.mesh)
            .unwrap()
            .layout
            .clone();
        let properties = world
            .resource::<ErasedRenderAssets<PreparedMaterial>>()
            .get(job.material)
            .unwrap()
            .properties
            .clone();
        for (mesh_key, shadow) in keys.iter() {
            let result = super::super::pipeline_ownership::specialize_base(world, |world| {
                let key = ErasedMaterialPipelineKey {
                    type_id: job.material.type_id(),
                    mesh_key: ErasedMeshPipelineKey::new(mesh_key),
                    material_key: properties.material_key.clone(),
                };
                let result = if shadow {
                    let Some(specialize) = properties.prepass_specialize else {
                        return Err(ReferenceError(
                            "Source shadow material has no shader pass".into(),
                        ));
                    };
                    dispatched += 1;
                    specialize(world, key, &layout, &properties)
                } else {
                    dispatched += 1;
                    base_specialize(world, key, &layout, &properties)
                };
                result.map_err(|error| ReferenceError(format!("Source material pipeline: {error}")))
            });
            match result {
                Ok(Some(id)) => {
                    if !job.ids.contains(&id) {
                        job.ids.push(id);
                    }
                }
                Ok(None) => {
                    job.physical.stop();
                    job.waiting = "cancelled";
                    return dispatched;
                }
                Err(error) => {
                    job.state =
                        RenderingState::Failed(pipeline_diagnostic(format_args!("{}", error.0)));
                    break;
                }
            }
        }
    }
    if job.state != RenderingState::Loading {
        return dispatched;
    }
    let published = world.resource::<PipelineCache>().pipelines().count();
    if job.ids.iter().any(|id| id.id() >= published) {
        job.waiting = "pipeline publication";
        return dispatched;
    }
    let mut ready = true;
    let mut waiting = "material pipeline";
    let mut failure = None;
    let cache = world.resource::<PipelineCache>();
    for id in &job.ids {
        match cache.get_render_pipeline_state(*id) {
            CachedPipelineState::Ok(_) => {}
            CachedPipelineState::Err(
                ShaderCacheError::ShaderNotLoaded(_)
                | ShaderCacheError::ShaderImportNotYetAvailable,
            ) => ready = false,
            CachedPipelineState::Err(error) => {
                write_pipeline_diagnostic(
                    &mut failure,
                    format_args!("Source material shader: {error}"),
                );
                break;
            }
            _ => {
                waiting = "shader";
                ready = false;
            }
        }
    }
    for id in &job.ids {
        if let Some(result) = world
            .resource::<crate::presentation::caster::CasterPasses>()
            .companion_state(*id, cache)
        {
            match result {
                Ok(value) => {
                    if !value {
                        waiting = "caster pipeline";
                    }
                    ready &= value;
                }
                Err(error) => {
                    write_pipeline_diagnostic(&mut failure, format_args!("{error}"));
                    break;
                }
            }
        }
        if let Some(result) = world
            .resource::<crate::presentation::surface_pass::SurfacePasses>()
            .companion_state(*id, cache)
        {
            match result {
                Ok(value) => {
                    if !value {
                        waiting = "surface pipeline";
                    }
                    ready &= value;
                }
                Err(error) => {
                    write_pipeline_diagnostic(&mut failure, format_args!("{error}"));
                    break;
                }
            }
        }
        let descriptor = cache.get_render_pipeline_descriptor(*id);
        if descriptor.fragment.as_ref().is_some_and(|fragment|fragment.shader_defs.iter().any(|def|matches!(def,bevy::shader::ShaderDefVal::Bool(name,true) if name=="SOURCE_SURFACE" || name=="SOURCE_WORLD"))) && !world.resource::<crate::presentation::caster::CasterPasses>().classified(*id) {waiting="caster classification";ready=false;}
    }
    job.waiting = waiting;
    if let Some(error) = failure {
        job.state = RenderingState::Failed(error);
    } else if ready {
        job.state = RenderingState::Ready;
    }
    dispatched
}

// A failure flag does not release descriptor staging or the counted owner.
fn physical_pipelines_settled(world: &World, job: &PipelineJob) -> bool {
    let cache = world.resource::<PipelineCache>();
    let published = cache.pipelines().count();
    job.ids.iter().all(|id| {
        id.id() < published
            && match cache.get_render_pipeline_state(*id) {
                CachedPipelineState::Err(
                    ShaderCacheError::ShaderNotLoaded(_)
                    | ShaderCacheError::ShaderImportNotYetAvailable,
                ) => false,
                CachedPipelineState::Ok(_) | CachedPipelineState::Err(_) => true,
                _ => false,
            }
            && world
                .resource::<crate::presentation::caster::CasterPasses>()
                .physically_settled(*id, cache)
            && world
                .resource::<crate::presentation::surface_pass::SurfacePasses>()
                .companion_state(*id, cache)
                .is_none_or(|r| !matches!(r, Ok(false)))
    })
}
pub(crate) fn physical_pipeline_state(
    state: &CachedPipelineState,
) -> crate::ownership::renderer_pending::PhysicalPipelineState {
    use crate::ownership::renderer_pending::PhysicalPipelineState;
    match state {
        CachedPipelineState::Queued => PhysicalPipelineState::Queued,
        CachedPipelineState::Creating(_) => PhysicalPipelineState::Compiling,
        CachedPipelineState::Ok(_) => PhysicalPipelineState::Ready,
        CachedPipelineState::Err(
            ShaderCacheError::ShaderNotLoaded(_) | ShaderCacheError::ShaderImportNotYetAvailable,
        ) => PhysicalPipelineState::Retryable,
        CachedPipelineState::Err(_) => PhysicalPipelineState::Failed,
    }
}
