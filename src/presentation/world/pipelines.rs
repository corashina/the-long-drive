//! Compile admitted material variants before collision and visual publication.
use crate::ownership::PresentationLedger;
use crate::ownership::renderer_pending::{
    PIPELINE_BATCH_KEYS, PipelineAccounting, PipelineContinuation, PipelineOperation,
    pipeline_diagnostic, pipeline_pair_ready, write_pipeline_diagnostic,
};
use crate::ownership::upload_receipt::UploadReceipt;
use crate::{
    presentation::materials::RenderingState, reference::ReferenceError, world::RendererWorld,
};
use bevy::material::{MaterialProperties, labels::ShaderLabel};
use bevy::mesh::MeshVertexBufferLayoutRef;
use bevy::render::renderer::RenderQueue;
use bevy::render::{erased_render_asset::PrepareNextFrameAssets, renderer::RenderDevice};
use bevy::shader::ShaderRef;
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
use std::hash::Hash;
use std::sync::{Arc, Mutex};

struct PipelineJob {
    physical: UploadReceipt,
    session: u64,
    mesh: AssetId<Mesh>,
    material: UntypedAssetId,
    material_type: &'static str,
    early: EarlyProperties,
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
    session: u64,
}
pub struct WorldPipelinePlugin;
impl Plugin for WorldPipelinePlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<WorldPipelines>()
            .add_plugins(ExtractResourcePlugin::<WorldPipelines>::default());
        app.sub_app_mut(RenderApp)
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

    pub(crate) fn request_continuation<M: Material>(
        &mut self,
        stream: &RendererWorld,
        ledger: &mut PresentationLedger,
        parent: &mut PipelineContinuation,
        mesh: &Handle<Mesh>,
        material: &Handle<M>,
        shadows: bool,
    ) -> Result<Option<PipelineTicket>, ReferenceError>
    where
        M::Data: PartialEq + Eq + Hash + Clone,
    {
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
    ) -> Result<Option<PipelineTicket>, ReferenceError>
    where
        M::Data: PartialEq + Eq + Hash + Clone,
    {
        if let Some(parent) = parent.as_ref() {
            parent.validate(stream)?;
        }
        // This local Arc adds no allocation. It keeps the bank lock order of bank, then pair.
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
        let operation = if let Some(index) = append {
            let operation = &guard.as_ref().unwrap().batches[index].operation;
            operation.live()?;
            operation.clone()
        } else {
            let operation = match parent.as_mut() {
                Some(parent) => parent.start(stream, ledger)?,
                None => PipelineOperation::independent(ledger),
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
        let ids = Vec::with_capacity(PIPELINE_BATCH_KEYS);
        assert_eq!(ids.capacity(), PIPELINE_BATCH_KEYS);
        let ticket = PipelineTicket(Arc::new(Mutex::new(PipelineJob {
            physical: operation.physical.clone(),
            session: stream.frame().request.session,
            mesh: mesh.id(),
            material: material.id().untyped(),
            material_type: std::any::type_name::<M>(),
            early: early_properties::<M>,
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
        self.session = stream.frame().request.session;
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
        self.accounting.reconcile(
            ledger,
            bank.batches.iter().map(|batch| batch.operation.clone()),
        )
    }
}

// The keys live only on the render stack, then freeze into the dispatched IDs.
type PairKeys = crate::ownership::renderer_pending::PipelineKeys<(MeshPipelineKey, bool)>;
/// The keys need the material properties, but not its textures. A material that waits for its
/// textures gives early properties, so its pipelines compile while the textures upload.
fn pair_keys(
    world: &World,
    job: &mut PipelineJob,
) -> Result<Option<(PairKeys, Arc<MaterialProperties>)>, ReferenceError> {
    let Some(mesh) = world.resource::<RenderAssets<RenderMesh>>().get(job.mesh) else {
        job.waiting = "mesh";
        return Ok(None);
    };
    let shared = match world
        .resource::<ErasedRenderAssets<PreparedMaterial>>()
        .get(job.material)
    {
        Some(material) => material.properties.clone(),
        None => {
            let Some(properties) = (job.early)(world, job.material) else {
                job.waiting = "material";
                return Ok(None);
            };
            properties
        }
    };
    let views = world.resource::<ViewKeyCache>();
    if views.values().next().is_none() {
        job.waiting = "view keys";
        return Ok(None);
    }
    let properties = &*shared;
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
    Ok(Some((keys, shared)))
}
fn compile(world: &mut World) {
    let Some(active) = world.get_resource::<WorldPipelines>().cloned() else {
        return;
    };
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
                    Ok(Some((keys, properties))) => match batch.select(Some(keys.len)) {
                        Ok(true) => {
                            job.dispatched = true;
                            batch.dispatched_keys +=
                                compile_pair(world, &mut job, Some((&keys, &properties)));
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
fn compile_pair(
    world: &mut World,
    job: &mut PipelineJob,
    keys: Option<(&PairKeys, &Arc<MaterialProperties>)>,
) -> usize {
    let mut dispatched = 0;
    if let Some((keys, properties)) = keys {
        let layout = world
            .resource::<RenderAssets<RenderMesh>>()
            .get(job.mesh)
            .unwrap()
            .layout
            .clone();
        for (mesh_key, shadow) in keys.iter() {
            let key = ErasedMaterialPipelineKey {
                type_id: job.material.type_id(),
                mesh_key: ErasedMeshPipelineKey::new(mesh_key),
                material_key: properties.material_key.clone(),
            };
            let result = if shadow {
                properties
                    .prepass_specialize
                    .ok_or_else(|| "Source shadow material has no shader pass".to_string())
                    .and_then(|specialize| {
                        dispatched += 1;
                        specialize(world, key, &layout, &properties)
                            .map_err(|error| format!("Source material pipeline: {error}"))
                    })
            } else {
                dispatched += 1;
                base_specialize(world, key, &layout, &properties)
                    .map_err(|error| format!("Source material pipeline: {error}"))
            };
            match result {
                Ok(id) => {
                    if !job.ids.contains(&id) {
                        job.ids.push(id);
                    }
                }
                Err(error) => {
                    job.state =
                        RenderingState::Failed(pipeline_diagnostic(format_args!("{error}")));
                    break;
                }
            }
        }
    }
    if job.state != RenderingState::Loading {
        return dispatched;
    }
    if world
        .resource::<ErasedRenderAssets<PreparedMaterial>>()
        .get(job.material)
        .is_none()
    {
        job.waiting = "material";
        return dispatched;
    }
    if job.material.type_id()
        == std::any::TypeId::of::<crate::presentation::materials::SourceMaterial>()
        && !crate::presentation::caster_material::ready(world, job.material.typed())
    {
        job.waiting = "caster material";
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

type EarlyProperties = fn(&World, UntypedAssetId) -> Option<Arc<MaterialProperties>>;

/// Same properties as Bevy's material `prepare_asset`, for a material that waits for a later
/// prepare. Draw functions and the render method only matter for queues, so they stay default.
/// The pipeline key holds the same material key, so the later prepared material finds these
/// pipelines in the specializer cache.
fn early_properties<M: Material>(
    world: &World,
    id: UntypedAssetId,
) -> Option<Arc<MaterialProperties>>
where
    M::Data: PartialEq + Eq + Hash + Clone,
{
    let material = world
        .get_resource::<PrepareNextFrameAssets<MeshMaterial3d<M>>>()?
        .get(id.typed::<M>())?;
    let device = world.resource::<RenderDevice>();
    let server = world.resource::<AssetServer>();
    let mut properties = MaterialProperties {
        alpha_mode: material.alpha_mode(),
        depth_bias: material.depth_bias(),
        reads_view_transmission_texture: material.reads_view_transmission_texture(),
        material_layout: Some(M::bind_group_layout_descriptor(device)),
        bindless: bevy::pbr::material_uses_bindless_resources::<M>(device),
        base_specialize: Some(base_specialize),
        prepass_specialize: Some(prepass_specialize),
        user_specialize: Some(user_specialize::<M>),
        material_key: bevy::material::key::ErasedMaterialKey::new(material.bind_group_data()),
        shadows_enabled: M::enable_shadows(),
        prepass_enabled: M::enable_prepass(),
        ..default()
    };
    let mut bits = MeshPipelineKey::empty();
    bits.set(
        MeshPipelineKey::READS_VIEW_TRANSMISSION_TEXTURE,
        properties.reads_view_transmission_texture,
    );
    properties.mesh_pipeline_key_bits = ErasedMeshPipelineKey::new(bits);
    for (label, shader) in [
        (bevy::pbr::MaterialVertexShader.intern(), M::vertex_shader()),
        (
            bevy::pbr::MaterialFragmentShader.intern(),
            M::fragment_shader(),
        ),
        (
            bevy::pbr::PrepassVertexShader.intern(),
            M::prepass_vertex_shader(),
        ),
        (
            bevy::pbr::PrepassFragmentShader.intern(),
            M::prepass_fragment_shader(),
        ),
        (
            bevy::pbr::DeferredVertexShader.intern(),
            M::deferred_vertex_shader(),
        ),
        (
            bevy::pbr::DeferredFragmentShader.intern(),
            M::deferred_fragment_shader(),
        ),
    ] {
        match shader {
            ShaderRef::Default => {}
            ShaderRef::Handle(handle) => properties.shaders.push((label, handle)),
            ShaderRef::Path(path) => properties.shaders.push((label, server.load(path))),
        }
    }
    Some(Arc::new(properties))
}
fn prepass_specialize(
    world: &mut World,
    key: ErasedMaterialPipelineKey,
    layout: &MeshVertexBufferLayoutRef,
    properties: &Arc<MaterialProperties>,
) -> Result<CachedRenderPipelineId, SpecializedMeshPipelineError> {
    world.resource_scope(
        |world, mut pipelines: Mut<SpecializedMeshPipelines<bevy::pbr::PrepassPipelineSpecializer>>| {
            let specializer = bevy::pbr::PrepassPipelineSpecializer {
                pipeline: world.resource::<bevy::pbr::PrepassPipeline>().clone(),
                properties: properties.clone(),
            };
            pipelines.specialize(world.resource::<PipelineCache>(), &specializer, key, layout)
        },
    )
}
fn user_specialize<M: Material>(
    pipeline: &dyn std::any::Any,
    descriptor: &mut RenderPipelineDescriptor,
    layout: &MeshVertexBufferLayoutRef,
    key: ErasedMaterialPipelineKey,
) -> Result<(), SpecializedMeshPipelineError>
where
    M::Data: Hash + Clone,
{
    M::specialize(
        pipeline
            .downcast_ref::<bevy::pbr::MaterialPipeline>()
            .unwrap(),
        descriptor,
        layout,
        bevy::pbr::MaterialPipelineKey {
            mesh_key: key.mesh_key.downcast(),
            bind_group_data: key.material_key.to_key(),
        },
    )
}
