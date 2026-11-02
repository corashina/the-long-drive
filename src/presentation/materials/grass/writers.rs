//! Dedicated source layer18 draw pass, before the camera passes.
use super::displacement::{GrassCamera, GrassFrameUniform, GrassGpu, GrassGpuData};
use bevy::{
    core_pipeline::schedule::RootNonCameraView,
    ecs::{self as bevy_ecs, schedule::ScheduleLabel},
    mesh::VertexBufferLayout,
    prelude::*,
    render::{
        Extract, ExtractSchedule, Render, RenderApp, RenderStartup, RenderSystems,
        render_resource::*,
        renderer::{RenderContext, RenderDevice, RenderQueue},
    },
    shader::ShaderCacheError,
};
use std::{
    collections::HashMap,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
};
#[derive(Clone)]
pub struct GrassWriterInput {
    pub entity: Entity,
    pub vertices: Arc<[u8]>,
    pub indices: Arc<[u8]>,
    pub world_from_local: Mat4,
}
#[derive(Resource, Default)]
pub struct GrassWriters {
    camera: Option<GrassCamera>,
    pub admitted: bool,
    status: Arc<Mutex<Option<Result<(), String>>>>,
    revision: Arc<AtomicU64>,
    pending_gpu: Arc<AtomicBool>,
    pub session: u64,
    pub frame: Option<GrassFrameUniform>,
    pub writers: Vec<GrassWriterInput>,
    pending: Vec<WriterPreparation>,
    deferred: bool,
    allocations: Vec<(Entity, usize, usize)>,
}
struct WriterPreparation {
    entity: Entity,
    mesh: Handle<Mesh>,
    pose: Mat4,
    vertices: Vec<u8>,
    indices: Vec<u8>,
    vertex: usize,
    index: usize,
    /// Holds a renderer job.
    running: bool,
    /// The mesh copy is complete. The writer waits for a publication slot.
    copied: bool,
}
struct WriterGpu {
    vertices: Buffer,
    indices: Buffer,
    count: u32,
    pose: Mat4,
    uniform: Buffer,
    group: BindGroup,
}
#[derive(Resource)]
struct WriterPass {
    revision: Arc<AtomicU64>,
    pending_gpu: Arc<AtomicBool>,
    session: u64,
    active: bool,
    pipeline: CachedRenderPipelineId,
    frame_group: Option<BindGroup>,
    frame_layout: BindGroupLayout,
    status: Arc<Mutex<Option<Result<(), String>>>>,
    encoded: AtomicBool,
    callback_registered: AtomicBool,
    submitted_revision: AtomicU64,
    writer_layout: BindGroupLayout,
    writers: HashMap<Entity, WriterGpu>,
}
pub struct GrassDisplacementPlugin;
impl Plugin for GrassDisplacementPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<GrassWriters>();
        app.sub_app_mut(RenderApp)
            .init_resource::<GrassGpu>()
            .add_systems(
                ExtractSchedule,
                (init, extract)
                    .chain()
                    .after(crate::presentation::fixed_assets::FixedShaderExtraction),
            )
            .add_systems(RenderStartup, spawn_pass)
            .add_systems(Render, complete.in_set(RenderSystems::Cleanup))
            .add_systems(GrassWriterPass, draw);
    }
}
#[derive(ScheduleLabel, Clone, Debug, PartialEq, Eq, Hash)]
struct GrassWriterPass;
fn spawn_pass(mut commands: Commands) {
    commands.spawn(RootNonCameraView(GrassWriterPass.intern()));
}
fn init(
    mut commands: Commands,
    _device: Res<RenderDevice>,
    cache: Res<PipelineCache>,
    shaders: Res<crate::presentation::fixed_assets::AdmittedShaders>,
    pass: Option<Res<WriterPass>>,
) {
    if pass.is_some() {
        return;
    }
    let Some(shader) = shaders.get("shaders/source_grass_displacement.wgsl") else {
        return;
    };
    use binding_types::*;

    let frame_layout = BindGroupLayoutDescriptor::new(
        "Grass displacement camera",
        &BindGroupLayoutEntries::single(
            ShaderStages::VERTEX,
            uniform_buffer::<GrassFrameUniform>(false),
        ),
    );
    let writer_layout = BindGroupLayoutDescriptor::new(
        "Grass displacement writer",
        &BindGroupLayoutEntries::single(ShaderStages::VERTEX, uniform_buffer::<Mat4>(false)),
    );
    let frame_group_layout = cache.get_bind_group_layout(&frame_layout);
    let writer_group_layout = cache.get_bind_group_layout(&writer_layout);
    let pipeline = cache.queue_render_pipeline(RenderPipelineDescriptor {
        label: Some("source Unlit/Color displacement".into()),
        layout: vec![frame_layout, writer_layout],
        vertex: VertexState {
            shader: shader.clone(),
            entry_point: Some("vertex".into()),
            buffers: vec![VertexBufferLayout {
                array_stride: 12,
                step_mode: VertexStepMode::Vertex,
                attributes: vec![VertexAttribute {
                    format: VertexFormat::Float32x3,
                    offset: 0,
                    shader_location: 0,
                }],
            }],
            ..default()
        },
        fragment: Some(FragmentState {
            shader: shader.clone(),
            entry_point: Some("fragment".into()),
            targets: vec![Some(ColorTargetState {
                format: TextureFormat::Rgba8Unorm,
                blend: None,
                write_mask: ColorWrites::ALL,
            })],
            ..default()
        }),
        primitive: PrimitiveState {
            cull_mode: Some(Face::Back),
            front_face: FrontFace::Cw,
            ..default()
        },
        depth_stencil: Some(DepthStencilState {
            format: TextureFormat::Depth24PlusStencil8,
            depth_write_enabled: Some(true),
            depth_compare: Some(CompareFunction::LessEqual),
            stencil: default(),
            bias: default(),
        }),
        ..default()
    });
    commands.insert_resource(WriterPass {
        session: 0,
        active: false,
        pipeline,
        frame_group: None,
        frame_layout: frame_group_layout,
        status: Arc::default(),
        revision: Arc::default(),
        pending_gpu: Arc::default(),
        encoded: AtomicBool::new(false),
        callback_registered: AtomicBool::new(false),
        submitted_revision: AtomicU64::new(0),
        writer_layout: writer_group_layout,
        writers: HashMap::new(),
    });
}
fn extract(
    input: Extract<Res<GrassWriters>>,
    pass: Option<ResMut<WriterPass>>,
    mut gpu: ResMut<GrassGpu>,
    device: Res<RenderDevice>,
    queue: Res<RenderQueue>,
) {
    let Some(mut pass) = pass else {
        return;
    };
    extract_one(&input, &mut pass, &mut gpu, &device, &queue);
    gpu.0.retain(|session, _| *session == input.session);
}
fn extract_one(
    input: &GrassWriters,
    pass: &mut WriterPass,
    gpu: &mut GrassGpu,
    device: &RenderDevice,
    queue: &RenderQueue,
) {
    if !input.admitted {
        pass.active = false;
        return;
    }
    if !gpu.0.contains_key(&input.session) {
        let created = GrassGpuData::new(&device);
        pass.frame_group = Some(device.create_bind_group(
            "Grass displacement camera",
            &pass.frame_layout,
            &BindGroupEntries::single(created.uniform.as_entire_binding()),
        ));
        gpu.0.insert(input.session, created);
    }
    let gpu = gpu.0.get(&input.session).expect("admitted Grass target");
    pass.status = Arc::clone(&input.status);
    pass.revision = Arc::clone(&input.revision);
    pass.pending_gpu = Arc::clone(&input.pending_gpu);
    if pass.submitted_revision.load(Ordering::Acquire) != input.revision.load(Ordering::Acquire)
        && !input.pending_gpu.load(Ordering::Acquire)
    {
        pass.encoded.store(false, Ordering::Release);
        pass.callback_registered.store(false, Ordering::Release);
    }
    if pass.session != input.session {
        pass.writers.clear();
        pass.session = input.session;
    }
    pass.active = input.frame.is_some();
    if let Some(frame) = input.frame {
        gpu.update(&queue, frame);
    }
    pass.writers
        .retain(|entity, _| input.writers.iter().any(|writer| writer.entity == *entity));
    for writer in &input.writers {
        if let Some(current) = pass.writers.get_mut(&writer.entity) {
            if current.pose != writer.world_from_local {
                let mut bytes = encase::UniformBuffer::new([0u8; 64]);
                bytes
                    .write(&writer.world_from_local)
                    .expect("Grass writer pose");
                queue.write_buffer(&current.uniform, 0, bytes.as_ref());
                current.pose = writer.world_from_local;
            }
            continue;
        }
        let vertices = device.create_buffer_with_data(&BufferInitDescriptor {
            label: Some("source Grass writer vertices"),
            contents: &writer.vertices,
            usage: BufferUsages::VERTEX,
        });
        let indices = device.create_buffer_with_data(&BufferInitDescriptor {
            label: Some("source Grass writer indices"),
            contents: &writer.indices,
            usage: BufferUsages::INDEX,
        });
        let mut bytes = encase::UniformBuffer::new([0u8; 64]);
        bytes
            .write(&writer.world_from_local)
            .expect("Grass writer pose");
        let uniform = device.create_buffer_with_data(&BufferInitDescriptor {
            label: Some("source Grass writer pose"),
            contents: bytes.as_ref(),
            usage: BufferUsages::UNIFORM | BufferUsages::COPY_DST,
        });
        let group = device.create_bind_group(
            "source Grass writer",
            &pass.writer_layout,
            &BindGroupEntries::single(uniform.as_entire_binding()),
        );
        pass.writers.insert(
            writer.entity,
            WriterGpu {
                vertices,
                indices,
                count: (writer.indices.len() / 4) as u32,
                pose: writer.world_from_local,
                uniform,
                group,
            },
        );
    }
}
fn draw(
    pass: Option<Res<WriterPass>>,
    gpu: Res<GrassGpu>,
    cache: Res<PipelineCache>,
    mut context: RenderContext,
) {
    let Some(pass) = pass else {
        return;
    };
    draw_one(&pass, &gpu, &cache, &mut context);
}
fn draw_one(pass: &WriterPass, gpu: &GrassGpu, cache: &PipelineCache, context: &mut RenderContext) {
    if !pass.active {
        return;
    }
    let Some(gpu) = gpu.0.get(&pass.session) else {
        return;
    };
    let Some(pipeline) = cache.get_render_pipeline(pass.pipeline) else {
        match cache.get_render_pipeline_state(pass.pipeline) {
            CachedPipelineState::Err(
                ShaderCacheError::ShaderNotLoaded(_)
                | ShaderCacheError::ShaderImportNotYetAvailable,
            ) => {}
            CachedPipelineState::Err(error) => {
                *pass.status.lock().unwrap() = Some(Err(error.to_string()));
            }
            _ => {}
        }
        return;
    };
    let mut render = context.begin_tracked_render_pass(RenderPassDescriptor {
        label: Some("source Grass displacement"),
        color_attachments: &[Some(RenderPassColorAttachment {
            view: &gpu.color_view,
            depth_slice: None,
            resolve_target: None,
            ops: Operations {
                load: LoadOp::Clear(LinearRgba::new(0., 0., 1., 1.).into()),
                store: StoreOp::Store,
            },
        })],
        depth_stencil_attachment: Some(RenderPassDepthStencilAttachment {
            view: &gpu.depth_view,
            depth_ops: Some(Operations {
                load: LoadOp::Clear(1.),
                store: StoreOp::Discard,
            }),
            stencil_ops: Some(Operations {
                load: LoadOp::Clear(0),
                store: StoreOp::Discard,
            }),
        }),
        timestamp_writes: None,
        occlusion_query_set: None,
        multiview_mask: None,
    });
    encode(pass, pipeline, &mut render);
    if pass.status.lock().unwrap().is_none() {
        pass.encoded.store(true, Ordering::Release);
    }
}
fn encode<'a>(
    pass: &'a WriterPass,
    pipeline: &'a RenderPipeline,
    render: &mut bevy::render::render_phase::TrackedRenderPass<'a>,
) {
    render.set_render_pipeline(pipeline);
    render.set_bind_group(
        0,
        pass.frame_group.as_ref().expect("admitted Grass frame"),
        &[],
    );
    for writer in pass.writers.values() {
        render.set_bind_group(1, &writer.group, &[]);
        render.set_vertex_buffer(0, writer.vertices.slice(..));
        render.set_index_buffer(writer.indices.slice(..), IndexFormat::Uint32);
        render.draw_indexed(0..writer.count, 0, 0..1);
    }
}

fn complete(pass: Option<Res<WriterPass>>, queue: Res<RenderQueue>) {
    let Some(pass) = pass else {
        return;
    };
    for pass in std::iter::once(&*pass) {
        if !pass.encoded.load(Ordering::Acquire)
            || pass.callback_registered.swap(true, Ordering::AcqRel)
        {
            continue;
        }
        pass.pending_gpu.store(true, Ordering::Release);
        let token = pass.revision.load(Ordering::Acquire);
        pass.submitted_revision.store(token, Ordering::Release);
        let revision = pass.revision.clone();
        let pending = pass.pending_gpu.clone();
        let status = pass.status.clone();
        queue.on_submitted_work_done(move || {
            if revision.load(Ordering::Acquire) == token && status.lock().unwrap().is_none() {
                *status.lock().unwrap() = Some(Ok(()));
            }
            pending.store(false, Ordering::Release);
        });
    }
}

impl GrassWriters {
    pub(crate) fn startup_audit(&self) -> serde_json::Value {
        serde_json::json!({
            "revision": self.revision.load(Ordering::Acquire),
            "deferred": self.deferred,
            "pending": self.pending.len(),
            "writers": self.writers.len(),
            "gpu_pending": self.pending_gpu.load(Ordering::Acquire),
            "status": format!("{:?}", *self.status.lock().unwrap()),
            "ready": self.ready().ok(),
        })
    }
    pub fn release_all(&mut self) {
        self.writers.clear();
        self.pending.clear();
        self.allocations.clear();
        self.admitted = false;
        self.frame = None;
    }
    pub fn drained(&self) -> bool {
        !self.pending_gpu.load(Ordering::Acquire)
    }
    fn invalidate(&self) {
        self.revision.fetch_add(1, Ordering::AcqRel);
        *self.status.lock().unwrap() = None;
    }
    pub fn configure(
        &mut self,
        stream: &crate::world::RendererWorld,
        ledger: &mut crate::ownership::PresentationLedger,
        reference: &crate::reference::WorldReference,
    ) -> Result<(), crate::reference::ReferenceError> {
        let session = stream.frame().request.session;
        if self.admitted && self.session == session {
            return Ok(());
        }
        let camera = GrassCamera::from_source(reference)?;
        let mut allocation = ledger.allocation();
        allocation.active_gpu += 256 * 256 * 8 + 128;
        allocation.active_cpu += std::mem::size_of::<Self>() + std::mem::size_of::<WriterPass>();
        ledger.set_allocation(allocation)?;
        self.camera = Some(camera);
        self.session = session;
        self.admitted = true;
        self.writers.clear();
        self.pending.clear();
        self.allocations.clear();
        self.invalidate();
        Ok(())
    }
    pub fn update_frame(&mut self, source_camera: Vec3, origin: Vec3, time: f32) {
        self.deferred = false;
        if let Some(camera) = self.camera {
            self.frame = Some(camera.frame(source_camera, origin, time));
        }
    }
    pub fn ready(&self) -> Result<bool, crate::reference::ReferenceError> {
        if self.deferred || !self.pending.is_empty() {
            return Ok(false);
        }
        match &*self.status.lock().unwrap() {
            Some(Ok(())) => Ok(true),
            Some(Err(error)) => Err(crate::reference::ReferenceError(error.clone())),
            None => Ok(false),
        }
    }
}

impl GrassWriters {
    pub fn request(
        &mut self,
        ledger: &mut crate::ownership::PresentationLedger,
        entity: Entity,
        mesh: Handle<Mesh>,
        pose: Mat4,
        meshes: &Assets<Mesh>,
    ) -> Result<(), crate::reference::ReferenceError> {
        use crate::reference::ReferenceError;
        if let Some(writer) = self.writers.iter_mut().find(|w| w.entity == entity) {
            if writer.world_from_local != pose {
                writer.world_from_local = pose;
                self.invalidate();
            }
            return Ok(());
        }
        if let Some(writer) = self.pending.iter_mut().find(|w| w.entity == entity) {
            writer.pose = pose;
            return Ok(());
        }
        if !ledger.can_request_independent(1) {
            self.deferred = true;
            return Ok(());
        }
        let mesh_data = meshes
            .get(&mesh)
            .ok_or_else(|| ReferenceError("Missing admitted displacement mesh".into()))?;
        let vertices = match mesh_data.attribute(Mesh::ATTRIBUTE_POSITION) {
            Some(bevy::mesh::VertexAttributeValues::Float32x3(vertices)) => vertices.len(),
            _ => {
                return Err(ReferenceError(
                    "Displacement POSITION is not Float32x3".into(),
                ));
            }
        };
        let indices = mesh_data
            .indices()
            .ok_or_else(|| ReferenceError("Missing displacement triangle indices".into()))?
            .len();
        if indices % 3 != 0 {
            return Err(ReferenceError("Invalid displacement triangles".into()));
        }
        let bytes = vertices * 12 + indices * 4;
        let cpu = bytes * 2
            + std::mem::size_of::<WriterPreparation>()
            + std::mem::size_of::<GrassWriterInput>()
            + std::mem::size_of::<WriterGpu>()
            + 128;
        let gpu = bytes + 64;
        let mut allocation = ledger.allocation();
        allocation.active_cpu += cpu;
        allocation.active_gpu += gpu;
        allocation.pending += 1;
        ledger.set_allocation(allocation)?;
        self.pending.push(WriterPreparation {
            entity,
            mesh,
            pose,
            vertices: Vec::with_capacity(vertices * 12),
            indices: Vec::with_capacity(indices * 4),
            vertex: 0,
            index: 0,
            running: false,
            copied: false,
        });
        self.allocations.push((entity, cpu, gpu));
        Ok(())
    }
    pub fn prepare(
        &mut self,
        ledger: &mut crate::ownership::PresentationLedger,
        meshes: &Assets<Mesh>,
    ) -> Result<(), crate::reference::ReferenceError> {
        use crate::reference::ReferenceError;
        for index in 0..self.pending.len() {
            let job = &mut self.pending[index];
            if !job.copied {
                if !job.running {
                    if ledger.available_external_jobs() == 0 {
                        continue;
                    }
                    let mut a = ledger.allocation();
                    a.jobs += 1;
                    ledger.set_allocation(a)?;
                    job.running = true;
                }
                let mesh = meshes.get(&job.mesh).ok_or_else(|| {
                    ReferenceError("Retired displacement mesh during preparation".into())
                })?;
                let Some(bevy::mesh::VertexAttributeValues::Float32x3(positions)) =
                    mesh.attribute(Mesh::ATTRIBUTE_POSITION)
                else {
                    return Err(ReferenceError(
                        "Changed displacement position stream".into(),
                    ));
                };
                let end = (job.vertex + 4096).min(positions.len());
                for position in &positions[job.vertex..end] {
                    for value in position {
                        job.vertices.extend_from_slice(&value.to_le_bytes());
                    }
                }
                job.vertex = end;
                let indices = mesh
                    .indices()
                    .ok_or_else(|| ReferenceError("Changed displacement index stream".into()))?;
                let end = (job.index + 4096 * 3).min(indices.len());
                match indices {
                    bevy::mesh::Indices::U16(values) => {
                        for value in &values[job.index..end] {
                            job.indices
                                .extend_from_slice(&u32::from(*value).to_le_bytes());
                        }
                    }
                    bevy::mesh::Indices::U32(values) => {
                        for value in &values[job.index..end] {
                            job.indices.extend_from_slice(&value.to_le_bytes());
                        }
                    }
                }
                job.index = end;
                if job.vertex != positions.len() || job.index != indices.len() {
                    continue;
                }
                // The copy is done. Its job goes back before the writer waits for a publication
                // slot. The stream lends the slot only when renderer jobs finished its ready work.
                job.running = false;
                job.copied = true;
                let mut a = ledger.allocation();
                a.jobs -= 1;
                ledger.set_allocation(a)?;
            }
            if !ledger.take_publication() {
                continue;
            }
            let job = self.pending.remove(index);
            let mut a = ledger.allocation();
            a.pending -= 1;
            ledger.set_allocation(a)?;
            self.writers.push(GrassWriterInput {
                entity: job.entity,
                vertices: job.vertices.into(),
                indices: job.indices.into(),
                world_from_local: job.pose,
            });
            self.invalidate();
            break;
        }
        Ok(())
    }
    pub fn retire(
        &mut self,
        ledger: &mut crate::ownership::PresentationLedger,
        entity: Entity,
    ) -> Result<(), crate::reference::ReferenceError> {
        let Some(index) = self.allocations.iter().position(|a| a.0 == entity) else {
            return Ok(());
        };
        let (_, cpu, gpu) = self.allocations.swap_remove(index);
        let mut a = ledger.allocation();
        a.active_cpu = a.active_cpu.saturating_sub(cpu);
        a.active_gpu = a.active_gpu.saturating_sub(gpu);
        if let Some(index) = self.pending.iter().position(|p| p.entity == entity) {
            let job = self.pending.swap_remove(index);
            a.pending = a.pending.saturating_sub(1);
            a.jobs = a.jobs.saturating_sub(usize::from(job.running));
        }
        self.writers.retain(|w| w.entity != entity);
        self.invalidate();
        ledger.set_allocation(a)
    }
}
