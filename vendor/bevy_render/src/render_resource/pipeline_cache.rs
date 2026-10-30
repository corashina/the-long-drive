use bevy_material::descriptor::{
    BindGroupLayoutDescriptor, CachedComputePipelineId, CachedRenderPipelineId,
    ComputePipelineDescriptor, PipelineDescriptor, RenderPipelineDescriptor,
};

use crate::{
    Extract,
    render_resource::*,
    renderer::{RenderAdapter, RenderDevice, WgpuWrapper},
};
use alloc::{borrow::Cow, sync::Arc};
use bevy_asset::{AssetEvent, AssetId, Assets, Handle};
use bevy_ecs::{
    message::MessageReader,
    resource::Resource,
    system::{Res, ResMut},
};
use bevy_log::error;
use bevy_platform::collections::{HashMap, HashSet};
use bevy_shader::{
    CachedPipelineId, Shader, ShaderCache, ShaderCacheError, ShaderCacheSource, ShaderDefVal,
    ValidateShader,
};
use bevy_tasks::Task;
use bevy_utils::default;
use core::{future::Future, mem};
use std::sync::{Mutex, PoisonError};
use wgpu::{PipelineCompilationOptions, VertexBufferLayout as RawVertexBufferLayout};

/// A pipeline defining the data layout and shader logic for a specific GPU task.
///
/// Used to store a heterogenous collection of render and compute pipelines together.
#[derive(Debug)]
pub enum Pipeline {
    RenderPipeline(RenderPipeline),
    ComputePipeline(ComputePipeline),
}

pub struct CachedPipeline {
    pub descriptor: PipelineDescriptor,
    pub state: CachedPipelineState,
}

/// State of a cached pipeline inserted into a [`PipelineCache`].
#[derive(Debug)]
pub enum CachedPipelineState {
    /// The pipeline GPU object is queued for creation.
    Queued,
    /// The pipeline GPU object is being created.
    Creating(Task<Result<Pipeline, ShaderCacheError>>),
    /// The pipeline GPU object was created successfully and is available (allocated on the GPU).
    Ok(Pipeline),
    /// An error occurred while trying to create the pipeline GPU object.
    Err(ShaderCacheError),
}

impl CachedPipelineState {
    /// Convenience method to "unwrap" a pipeline state into its underlying GPU object.
    ///
    /// # Returns
    ///
    /// The method returns the allocated pipeline GPU object.
    ///
    /// # Panics
    ///
    /// This method panics if the pipeline GPU object is not available, either because it is
    /// pending creation or because an error occurred while attempting to create GPU object.
    pub fn unwrap(&self) -> &Pipeline {
        match self {
            CachedPipelineState::Ok(pipeline) => pipeline,
            CachedPipelineState::Queued => {
                panic!("Pipeline has not been compiled yet. It is still in the 'Queued' state.")
            }
            CachedPipelineState::Creating(..) => {
                panic!("Pipeline has not been compiled yet. It is still in the 'Creating' state.")
            }
            CachedPipelineState::Err(err) => panic!("{}", err),
        }
    }
}

type ImmediateSize = u32;
type LayoutCacheKey = (Vec<BindGroupLayoutId>, ImmediateSize);
#[derive(Default)]
struct LayoutCache {
    layouts: HashMap<LayoutCacheKey, Arc<WgpuWrapper<PipelineLayout>>>,
}

impl LayoutCache {
    fn get(
        &mut self,
        render_device: &RenderDevice,
        bind_group_layouts: &[BindGroupLayout],
        immediate_size: u32,
    ) -> Arc<WgpuWrapper<PipelineLayout>> {
        let bind_group_ids = bind_group_layouts.iter().map(BindGroupLayout::id).collect();
        self.layouts
            .entry((bind_group_ids, immediate_size))
            .or_insert_with_key(|(_, immediate_size)| {
                let bind_group_layouts = bind_group_layouts
                    .iter()
                    .map(BindGroupLayout::value)
                    .map(Some)
                    .collect::<Vec<_>>();
                Arc::new(WgpuWrapper::new(render_device.create_pipeline_layout(
                    &PipelineLayoutDescriptor {
                        bind_group_layouts: &bind_group_layouts,
                        immediate_size: *immediate_size,
                        ..default()
                    },
                )))
            })
            .clone()
    }
}

fn load_module(
    render_device: &RenderDevice,
    shader_source: ShaderCacheSource,
    validate_shader: &ValidateShader,
) -> Result<WgpuWrapper<ShaderModule>, ShaderCacheError> {
    let shader_source = match shader_source {
        #[cfg(feature = "shader_format_spirv")]
        ShaderCacheSource::SpirV(data) => wgpu::util::make_spirv(data),
        #[cfg(not(feature = "shader_format_spirv"))]
        ShaderCacheSource::SpirV(_) => {
            unimplemented!("Enable feature \"shader_format_spirv\" to use SPIR-V shaders")
        }
        ShaderCacheSource::Wgsl(src) => ShaderSource::Wgsl(Cow::Owned(src)),
        #[cfg(not(feature = "decoupled_naga"))]
        ShaderCacheSource::Naga(src) => ShaderSource::Naga(Cow::Owned(src)),
    };
    let module_descriptor = ShaderModuleDescriptor {
        label: None,
        source: shader_source,
    };

    let scope = render_device
        .wgpu_device()
        .push_error_scope(wgpu::ErrorFilter::Validation);

    let shader_module = WgpuWrapper::new(match validate_shader {
        ValidateShader::Enabled => {
            render_device.create_and_validate_shader_module(module_descriptor)
        }
        // SAFETY: we are interfacing with shader code, which may contain undefined behavior,
        // such as indexing out of bounds.
        // The checks required are prohibitively expensive and a poor default for game engines.
        ValidateShader::Disabled => unsafe {
            render_device.create_shader_module(module_descriptor)
        },
    });

    let error = scope.pop();

    // `now_or_never` will return Some if the future is ready and None otherwise.
    // On native platforms, wgpu will yield the error immediately while on wasm it may take longer since the browser APIs are asynchronous.
    // So to keep the complexity of the ShaderCache low, we will only catch this error early on native platforms,
    // and on wasm the error will be handled by wgpu and crash the application.
    if let Some(Some(wgpu::Error::Validation { description, .. })) =
        bevy_tasks::futures::now_or_never(error)
    {
        return Err(ShaderCacheError::CreateShaderModule(description));
    }

    Ok(shader_module)
}

#[derive(Default)]
struct BindGroupLayoutCache {
    bgls: HashMap<BindGroupLayoutDescriptor, BindGroupLayout>,
}

impl BindGroupLayoutCache {
    fn get(
        &mut self,
        render_device: &RenderDevice,
        descriptor: BindGroupLayoutDescriptor,
    ) -> BindGroupLayout {
        self.bgls
            .entry(descriptor)
            .or_insert_with_key(|descriptor| {
                render_device
                    .create_bind_group_layout(descriptor.label.as_ref(), &descriptor.entries)
            })
            .clone()
    }
}

/// Cache for render and compute pipelines.
///
/// The cache stores existing render and compute pipelines allocated on the GPU, as well as
/// pending creation. Pipelines inserted into the cache are identified by a unique ID, which
/// can be used to retrieve the actual GPU object once it's ready. The creation of the GPU
/// pipeline object is deferred to the [`RenderSystems::Render`] step, just before the render
/// graph starts being processed, as this requires access to the GPU.
///
/// Note that the cache does not perform automatic deduplication of identical pipelines. It is
/// up to the user not to insert the same pipeline twice to avoid wasting GPU resources.
///
/// [`RenderSystems::Render`]: crate::RenderSystems::Render
#[derive(Resource)]
pub struct PipelineCache {
    layout_cache: Arc<Mutex<LayoutCache>>,
    bindgroup_layout_cache: Arc<Mutex<BindGroupLayoutCache>>,
    shader_cache: Arc<Mutex<ShaderCache<WgpuWrapper<ShaderModule>, RenderDevice>>>,
    device: RenderDevice,
    pipelines: Vec<CachedPipeline>,
    waiting_pipelines: HashSet<CachedPipelineId>,
    held_pipelines: Mutex<HashSet<CachedPipelineId>>,
    owned_queue: Arc<Mutex<OwnedQueueBudget>>,
    new_pipelines: Mutex<Vec<CachedPipeline>>,
    global_shader_defs: Vec<ShaderDefVal>,
    /// If `true`, disables asynchronous pipeline compilation.
    /// This has no effect on macOS, wasm, or without the `multi_threaded` feature.
    pub(crate) synchronous_pipeline_compilation: bool,
    /// If `true`, the shader cache needs to be repopulated from the main world's `Assets<Shader>`.
    needs_shader_reload: bool,
}

#[derive(Default)]
struct OwnedQueueBudget {
    slots: usize,
    active: bool,
    used: bool,
    descriptor_limit: usize,
    bytes: usize,
}
/// Reserves one persistent descriptor slot before material specialization.
/// Drop ends the queue scope. Existing specialization cache hits consume no slot.
pub struct OwnedRenderPipelineQueue(Arc<Mutex<OwnedQueueBudget>>);
impl Drop for OwnedRenderPipelineQueue {
    fn drop(&mut self) {
        self.0.lock().unwrap().active = false;
    }
}
fn descriptor_owned_bytes(d: &RenderPipelineDescriptor) -> usize {
    fn label(v: &Cow<'static, str>) -> usize {
        match v {
            Cow::Owned(s) => s.capacity(),
            _ => 0,
        }
    }
    fn defs(v: &Vec<ShaderDefVal>) -> usize {
        v.capacity() * size_of::<ShaderDefVal>()
            + v.iter()
                .map(|v| match v {
                    ShaderDefVal::Bool(s, _)
                    | ShaderDefVal::Int(s, _)
                    | ShaderDefVal::UInt(s, _) => s.capacity(),
                })
                .sum::<usize>()
    }
    size_of::<RenderPipelineDescriptor>()
        + d.label.as_ref().map_or(0, label)
        + d.layout.capacity() * size_of::<BindGroupLayoutDescriptor>()
        + d.layout
            .iter()
            .map(|l| label(&l.label) + l.entries.capacity() * size_of::<BindGroupLayoutEntry>())
            .sum::<usize>()
        + defs(&d.vertex.shader_defs)
        + d.vertex.entry_point.as_ref().map_or(0, label)
        + d.vertex.buffers.capacity() * size_of::<bevy_mesh::VertexBufferLayout>()
        + d.vertex
            .buffers
            .iter()
            .map(|v| v.attributes.capacity() * size_of::<VertexAttribute>())
            .sum::<usize>()
        + d.fragment.as_ref().map_or(0, |f| {
            defs(&f.shader_defs)
                + f.entry_point.as_ref().map_or(0, label)
                + f.targets.capacity() * size_of::<Option<ColorTargetState>>()
        })
}

impl PipelineCache {
    /// Reserve one owned queue slot. The caller admits the fixed lifetime budget first.
    /// The scope must contain at most one queue call and must not overlap another scope.
    pub fn owned_render_pipeline_queue(
        &self,
        max_slots: usize,
        max_bytes: usize,
        descriptor_limit: usize,
    ) -> Result<OwnedRenderPipelineQueue, &'static str> {
        let mut budget = self.owned_queue.lock().unwrap();
        assert!(!budget.active);
        if budget.slots >= max_slots || budget.bytes + descriptor_limit > max_bytes {
            return Err("Owned pipeline metadata capacity exhausted");
        }
        budget.active = true;
        budget.used = false;
        budget.descriptor_limit = descriptor_limit;
        Ok(OwnedRenderPipelineQueue(self.owned_queue.clone()))
    }
    /// Returns an iterator over the pipelines in the pipeline cache.
    pub fn pipelines(&self) -> impl Iterator<Item = &CachedPipeline> {
        self.pipelines.iter()
    }

    /// Returns a iterator of the IDs of all currently waiting pipelines.
    pub fn waiting_pipelines(&self) -> impl Iterator<Item = CachedPipelineId> + '_ {
        self.waiting_pipelines.iter().copied()
    }

    /// Create a new pipeline cache associated with the given render device.
    pub fn new(
        device: RenderDevice,
        render_adapter: RenderAdapter,
        synchronous_pipeline_compilation: bool,
    ) -> Self {
        let mut global_shader_defs = Vec::new();
        #[cfg(all(feature = "webgl", target_arch = "wasm32", not(feature = "webgpu")))]
        {
            global_shader_defs.push("NO_ARRAY_TEXTURES_SUPPORT".into());
            global_shader_defs.push("NO_CUBE_ARRAY_TEXTURES_SUPPORT".into());
            global_shader_defs.push("SIXTEEN_BYTE_ALIGNMENT".into());
        }

        if cfg!(target_abi = "sim") {
            global_shader_defs.push("NO_CUBE_ARRAY_TEXTURES_SUPPORT".into());
        }

        global_shader_defs.push(ShaderDefVal::UInt(
            String::from("AVAILABLE_STORAGE_BUFFER_BINDINGS"),
            device.limits().max_storage_buffers_per_shader_stage,
        ));

        Self {
            shader_cache: Arc::new(Mutex::new(ShaderCache::new(
                device.clone(),
                device.features(),
                render_adapter.get_downlevel_capabilities().flags,
                load_module,
            ))),
            device,
            layout_cache: default(),
            bindgroup_layout_cache: default(),
            waiting_pipelines: default(),
            held_pipelines: default(),
            owned_queue: default(),
            new_pipelines: default(),
            pipelines: default(),
            global_shader_defs,
            synchronous_pipeline_compilation,
            needs_shader_reload: true,
        }
    }

    /// Get the state of a cached render pipeline.
    ///
    /// See [`PipelineCache::queue_render_pipeline()`].
    #[inline]
    pub fn get_render_pipeline_state(&self, id: CachedRenderPipelineId) -> &CachedPipelineState {
        // If the pipeline id isn't in `pipelines`, it's queued in `new_pipelines`
        self.pipelines
            .get(id.id())
            .map_or(&CachedPipelineState::Queued, |pipeline| &pipeline.state)
    }

    /// Get the state of a cached compute pipeline.
    ///
    /// See [`PipelineCache::queue_compute_pipeline()`].
    #[inline]
    pub fn get_compute_pipeline_state(&self, id: CachedComputePipelineId) -> &CachedPipelineState {
        // If the pipeline id isn't in `pipelines`, it's queued in `new_pipelines`
        self.pipelines
            .get(id.id())
            .map_or(&CachedPipelineState::Queued, |pipeline| &pipeline.state)
    }

    /// Get the render pipeline descriptor a cached render pipeline was inserted from.
    ///
    /// See [`PipelineCache::queue_render_pipeline()`].
    ///
    /// **Note**: Be careful calling this method. It will panic if called with a pipeline that
    /// has been queued but has not yet been processed by [`PipelineCache::process_queue()`].
    #[inline]
    pub fn get_render_pipeline_descriptor(
        &self,
        id: CachedRenderPipelineId,
    ) -> &RenderPipelineDescriptor {
        match &self.pipelines[id.id()].descriptor {
            PipelineDescriptor::RenderPipelineDescriptor(descriptor) => descriptor,
            PipelineDescriptor::ComputePipelineDescriptor(_) => unreachable!(),
        }
    }

    /// Get the compute pipeline descriptor a cached render pipeline was inserted from.
    ///
    /// See [`PipelineCache::queue_compute_pipeline()`].
    ///
    /// **Note**: Be careful calling this method. It will panic if called with a pipeline that
    /// has been queued but has not yet been processed by [`PipelineCache::process_queue()`].
    #[inline]
    pub fn get_compute_pipeline_descriptor(
        &self,
        id: CachedComputePipelineId,
    ) -> &ComputePipelineDescriptor {
        match &self.pipelines[id.id()].descriptor {
            PipelineDescriptor::RenderPipelineDescriptor(_) => unreachable!(),
            PipelineDescriptor::ComputePipelineDescriptor(descriptor) => descriptor,
        }
    }

    /// Try to retrieve a render pipeline GPU object from a cached ID.
    ///
    /// # Returns
    ///
    /// This method returns a successfully created render pipeline if any, or `None` if the pipeline
    /// was not created yet or if there was an error during creation. You can check the actual creation
    /// state with [`PipelineCache::get_render_pipeline_state()`].
    #[inline]
    pub fn get_render_pipeline(&self, id: CachedRenderPipelineId) -> Option<&RenderPipeline> {
        if let CachedPipelineState::Ok(Pipeline::RenderPipeline(pipeline)) =
            &self.pipelines.get(id.id())?.state
        {
            Some(pipeline)
        } else {
            None
        }
    }

    /// Wait for a render pipeline to finish compiling.
    #[inline]
    pub fn block_on_render_pipeline(&mut self, id: CachedRenderPipelineId) {
        if self.pipelines.len() <= id.id() {
            self.process_queue();
        }

        let state = &mut self.pipelines[id.id()].state;
        if let CachedPipelineState::Creating(task) = state {
            *state = match bevy_tasks::block_on(task) {
                Ok(p) => CachedPipelineState::Ok(p),
                Err(e) => CachedPipelineState::Err(e),
            };
        }
    }

    /// Try to retrieve a compute pipeline GPU object from a cached ID.
    ///
    /// # Returns
    ///
    /// This method returns a successfully created compute pipeline if any, or `None` if the pipeline
    /// was not created yet or if there was an error during creation. You can check the actual creation
    /// state with [`PipelineCache::get_compute_pipeline_state()`].
    #[inline]
    pub fn get_compute_pipeline(&self, id: CachedComputePipelineId) -> Option<&ComputePipeline> {
        if let CachedPipelineState::Ok(Pipeline::ComputePipeline(pipeline)) =
            &self.pipelines.get(id.id())?.state
        {
            Some(pipeline)
        } else {
            None
        }
    }

    /// Insert a render pipeline into the cache, and queue its creation.
    ///
    /// The pipeline is always inserted and queued for creation. There is no attempt to deduplicate it with
    /// an already cached pipeline.
    ///
    /// # Returns
    ///
    /// This method returns the unique render shader ID of the cached pipeline, which can be used to query
    /// the caching state with [`get_render_pipeline_state()`] and to retrieve the created GPU pipeline once
    /// it's ready with [`get_render_pipeline()`].
    ///
    /// [`get_render_pipeline_state()`]: PipelineCache::get_render_pipeline_state
    /// [`get_render_pipeline()`]: PipelineCache::get_render_pipeline
    pub fn queue_render_pipeline(
        &self,
        descriptor: RenderPipelineDescriptor,
    ) -> CachedRenderPipelineId {
        let mut new_pipelines = self
            .new_pipelines
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        let id = CachedRenderPipelineId::new(self.pipelines.len() + new_pipelines.len());
        let mut descriptor = descriptor;
        let mut state = CachedPipelineState::Queued;
        let mut budget = self.owned_queue.lock().unwrap();
        if budget.active {
            assert!(
                !budget.used,
                "Owned queue scope must contain at most one descriptor"
            );
            budget.used = true;
            budget.slots += 1;
            self.held_pipelines.lock().unwrap().insert(id.id());
            let charge = descriptor_owned_bytes(&descriptor) * 3 + 1024;
            budget.bytes += charge.min(budget.descriptor_limit);
            if charge > budget.descriptor_limit {
                descriptor = RenderPipelineDescriptor::default();
                state = CachedPipelineState::Err(ShaderCacheError::CreateShaderModule(
                    "Owned pipeline descriptor exceeds admitted metadata slot".into(),
                ));
            }
        }
        new_pipelines.push(CachedPipeline {
            descriptor: PipelineDescriptor::RenderPipelineDescriptor(Box::new(descriptor)),
            state,
        });
        id
    }

    /// Insert a compute pipeline into the cache, and queue its creation.
    ///
    /// The pipeline is always inserted and queued for creation. There is no attempt to deduplicate it with
    /// an already cached pipeline.
    ///
    /// # Returns
    ///
    /// This method returns the unique compute shader ID of the cached pipeline, which can be used to query
    /// the caching state with [`get_compute_pipeline_state()`] and to retrieve the created GPU pipeline once
    /// it's ready with [`get_compute_pipeline()`].
    ///
    /// [`get_compute_pipeline_state()`]: PipelineCache::get_compute_pipeline_state
    /// [`get_compute_pipeline()`]: PipelineCache::get_compute_pipeline
    pub fn queue_compute_pipeline(
        &self,
        descriptor: ComputePipelineDescriptor,
    ) -> CachedComputePipelineId {
        let mut new_pipelines = self
            .new_pipelines
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        let id = CachedComputePipelineId::new(self.pipelines.len() + new_pipelines.len());
        new_pipelines.push(CachedPipeline {
            descriptor: PipelineDescriptor::ComputePipelineDescriptor(Box::new(descriptor)),
            state: CachedPipelineState::Queued,
        });
        id
    }

    pub fn get_bind_group_layout(
        &self,
        bind_group_layout_descriptor: &BindGroupLayoutDescriptor,
    ) -> BindGroupLayout {
        self.bindgroup_layout_cache
            .lock()
            .unwrap()
            .get(&self.device, bind_group_layout_descriptor.clone())
    }

    /// Inserts a [`Shader`] into this cache with the provided [`AssetId`].
    pub fn set_shader(&mut self, id: AssetId<Shader>, shader: Shader) {
        let mut shader_cache = self.shader_cache.lock().unwrap();
        let pipelines_to_queue = shader_cache.set_shader(id, shader);
        for cached_pipeline in pipelines_to_queue {
            self.pipelines[cached_pipeline].state = CachedPipelineState::Queued;
            self.waiting_pipelines.insert(cached_pipeline);
        }
    }

    /// Removes a [`Shader`] from this cache if it exists.
    pub fn remove_shader(&mut self, shader: AssetId<Shader>) {
        let mut shader_cache = self.shader_cache.lock().unwrap();
        let pipelines_to_queue = shader_cache.remove(shader);
        for cached_pipeline in pipelines_to_queue {
            self.pipelines[cached_pipeline].state = CachedPipelineState::Queued;
            self.waiting_pipelines.insert(cached_pipeline);
        }
    }

    fn start_create_render_pipeline(
        &mut self,
        id: CachedPipelineId,
        descriptor: RenderPipelineDescriptor,
    ) -> CachedPipelineState {
        let device = self.device.clone();
        let shader_cache = self.shader_cache.clone();
        let layout_cache = self.layout_cache.clone();
        let mut bindgroup_layout_cache = self.bindgroup_layout_cache.lock().unwrap();
        let bind_group_layout = descriptor
            .layout
            .iter()
            .map(|bind_group_layout_descriptor| {
                bindgroup_layout_cache.get(&self.device, bind_group_layout_descriptor.clone())
            })
            .collect::<Vec<_>>();

        create_pipeline_task(
            async move {
                let mut shader_cache = shader_cache.lock().unwrap();
                let mut layout_cache = layout_cache.lock().unwrap();

                let vertex_module = match shader_cache.get(
                    id,
                    descriptor.vertex.shader.id(),
                    &descriptor.vertex.shader_defs,
                ) {
                    Ok(module) => module,
                    Err(err) => return Err(err),
                };

                let fragment_module = match &descriptor.fragment {
                    Some(fragment) => {
                        match shader_cache.get(id, fragment.shader.id(), &fragment.shader_defs) {
                            Ok(module) => Some(module),
                            Err(err) => return Err(err),
                        }
                    }
                    None => None,
                };

                let layout = if descriptor.layout.is_empty() && descriptor.immediate_size == 0 {
                    None
                } else {
                    Some(layout_cache.get(&device, &bind_group_layout, descriptor.immediate_size))
                };

                drop((shader_cache, layout_cache));

                let vertex_buffer_layouts = descriptor
                    .vertex
                    .buffers
                    .iter()
                    .map(|layout| RawVertexBufferLayout {
                        array_stride: layout.array_stride,
                        attributes: &layout.attributes,
                        step_mode: layout.step_mode,
                    })
                    .collect::<Vec<_>>();

                let fragment_data = descriptor.fragment.as_ref().map(|fragment| {
                    (
                        fragment_module.unwrap(),
                        fragment.entry_point.as_deref(),
                        fragment.targets.as_slice(),
                    )
                });

                // TODO: Expose the rest of this somehow
                let compilation_options = PipelineCompilationOptions {
                    constants: &[],
                    zero_initialize_workgroup_memory: descriptor.zero_initialize_workgroup_memory,
                };

                let descriptor = RawRenderPipelineDescriptor {
                    multiview_mask: None,
                    depth_stencil: descriptor.depth_stencil.clone(),
                    label: descriptor.label.as_deref(),
                    layout: layout.as_ref().map(|layout| -> &PipelineLayout { layout }),
                    multisample: descriptor.multisample,
                    primitive: descriptor.primitive,
                    vertex: RawVertexState {
                        buffers: &vertex_buffer_layouts,
                        entry_point: descriptor.vertex.entry_point.as_deref(),
                        module: &vertex_module,
                        // TODO: Should this be the same as the fragment compilation options?
                        compilation_options: compilation_options.clone(),
                    },
                    fragment: fragment_data
                        .as_ref()
                        .map(|(module, entry_point, targets)| RawFragmentState {
                            entry_point: entry_point.as_deref(),
                            module,
                            targets,
                            // TODO: Should this be the same as the vertex compilation options?
                            compilation_options,
                        }),
                    cache: None,
                };

                Ok(Pipeline::RenderPipeline(
                    device.create_render_pipeline(&descriptor),
                ))
            },
            self.synchronous_pipeline_compilation,
        )
    }

    fn start_create_compute_pipeline(
        &mut self,
        id: CachedPipelineId,
        descriptor: ComputePipelineDescriptor,
    ) -> CachedPipelineState {
        let device = self.device.clone();
        let shader_cache = self.shader_cache.clone();
        let layout_cache = self.layout_cache.clone();
        let mut bindgroup_layout_cache = self.bindgroup_layout_cache.lock().unwrap();
        let bind_group_layout = descriptor
            .layout
            .iter()
            .map(|bind_group_layout_descriptor| {
                bindgroup_layout_cache.get(&self.device, bind_group_layout_descriptor.clone())
            })
            .collect::<Vec<_>>();

        create_pipeline_task(
            async move {
                let mut shader_cache = shader_cache.lock().unwrap();
                let mut layout_cache = layout_cache.lock().unwrap();

                let compute_module =
                    match shader_cache.get(id, descriptor.shader.id(), &descriptor.shader_defs) {
                        Ok(module) => module,
                        Err(err) => return Err(err),
                    };

                let layout = if descriptor.layout.is_empty() && descriptor.immediate_size == 0 {
                    None
                } else {
                    Some(layout_cache.get(&device, &bind_group_layout, descriptor.immediate_size))
                };

                drop((shader_cache, layout_cache));

                let descriptor = RawComputePipelineDescriptor {
                    label: descriptor.label.as_deref(),
                    layout: layout.as_ref().map(|layout| -> &PipelineLayout { layout }),
                    module: &compute_module,
                    entry_point: descriptor.entry_point.as_deref(),
                    // TODO: Expose the rest of this somehow
                    compilation_options: PipelineCompilationOptions {
                        constants: &[],
                        zero_initialize_workgroup_memory: descriptor
                            .zero_initialize_workgroup_memory,
                    },
                    cache: None,
                };

                Ok(Pipeline::ComputePipeline(
                    device.create_compute_pipeline(&descriptor),
                ))
            },
            self.synchronous_pipeline_compilation,
        )
    }

    /// Count descriptors, including entries not yet published to `pipelines()`.
    pub fn queued_pipeline_count(&self) -> usize {
        self.pipelines.len() + self.new_pipelines.lock().unwrap().len()
    }

    /// Keep this ID out of automatic preparation, including shader reload retries.
    /// The caller must own this ID exclusively. Shared accepted IDs must not be held.
    pub fn hold_render_pipeline(&self, id: CachedRenderPipelineId) {
        self.held_pipelines.lock().unwrap().insert(id.id());
    }

    /// Whether an exclusive owner controls preparation of this ID.
    pub fn render_pipeline_is_held(&self, id: CachedRenderPipelineId) -> bool {
        self.held_pipelines.lock().unwrap().contains(&id.id())
    }

    fn publish_queued_descriptors(&mut self) {
        for pipeline in self.new_pipelines.lock().unwrap().drain(..) {
            let id = self.pipelines.len();
            self.pipelines.push(pipeline);
            self.waiting_pipelines.insert(id);
        }
    }

    /// Prepare exactly one held ID synchronously, within the caller's GPU error scope.
    /// Retryable failures remain held and require another explicit call.
    pub fn prepare_held_render_pipeline(&mut self, id: CachedRenderPipelineId) {
        assert!(self.render_pipeline_is_held(id));
        self.publish_queued_descriptors();
        let mut pipelines = mem::take(&mut self.pipelines);
        let synchronous = self.synchronous_pipeline_compilation;
        self.synchronous_pipeline_compilation = true;
        self.process_pipeline(&mut pipelines[id.id()], id.id());
        self.synchronous_pipeline_compilation = synchronous;
        self.waiting_pipelines.remove(&id.id());
        self.pipelines = pipelines;
    }

    /// Drop a held GPU pipeline and retain its stable descriptor ID for later re-use.
    /// Automatic preparation stays disabled until the owner releases this ID.
    pub fn retire_held_render_pipeline(&mut self, id: CachedRenderPipelineId) {
        assert!(self.render_pipeline_is_held(id));
        self.publish_queued_descriptors();
        if !matches!(self.pipelines[id.id()].state, CachedPipelineState::Err(_)) {
            self.pipelines[id.id()].state = CachedPipelineState::Queued;
        }
        self.waiting_pipelines.remove(&id.id());
    }

    /// Return an accepted or explicitly reclaimed ID to normal cache processing.
    pub fn release_held_render_pipeline(&mut self, id: CachedRenderPipelineId) {
        self.publish_queued_descriptors();
        self.held_pipelines.lock().unwrap().remove(&id.id());
        if !matches!(self.pipelines[id.id()].state, CachedPipelineState::Ok(_)) {
            self.waiting_pipelines.insert(id.id());
        }
    }

    /// Process the pipeline queue and create all pending pipelines if possible.
    ///
    /// This is generally called automatically during the [`RenderSystems::Render`] step, but can
    /// be called manually to force creation at a different time.
    ///
    /// [`RenderSystems::Render`]: crate::RenderSystems::Render
    pub fn process_queue(&mut self) {
        let mut waiting_pipelines = mem::take(&mut self.waiting_pipelines);
        let mut pipelines = mem::take(&mut self.pipelines);

        {
            let mut new_pipelines = self
                .new_pipelines
                .lock()
                .unwrap_or_else(PoisonError::into_inner);
            for new_pipeline in new_pipelines.drain(..) {
                let id = pipelines.len();
                pipelines.push(new_pipeline);
                waiting_pipelines.insert(id);
            }
        }

        for id in waiting_pipelines {
            if self.held_pipelines.lock().unwrap().contains(&id) {
                continue;
            }
            self.process_pipeline(&mut pipelines[id], id);
        }

        self.pipelines = pipelines;
    }

    fn process_pipeline(&mut self, cached_pipeline: &mut CachedPipeline, id: usize) {
        match &mut cached_pipeline.state {
            CachedPipelineState::Queued => {
                cached_pipeline.state = match &cached_pipeline.descriptor {
                    PipelineDescriptor::RenderPipelineDescriptor(descriptor) => {
                        self.start_create_render_pipeline(id, *descriptor.clone())
                    }
                    PipelineDescriptor::ComputePipelineDescriptor(descriptor) => {
                        self.start_create_compute_pipeline(id, *descriptor.clone())
                    }
                };
            }

            CachedPipelineState::Creating(task) => match bevy_tasks::futures::check_ready(task) {
                Some(Ok(pipeline)) => {
                    cached_pipeline.state = CachedPipelineState::Ok(pipeline);
                    return;
                }
                Some(Err(err)) => cached_pipeline.state = CachedPipelineState::Err(err),
                _ => (),
            },

            CachedPipelineState::Err(err) => match err {
                // Retry
                ShaderCacheError::ShaderNotLoaded(_)
                | ShaderCacheError::ShaderImportNotYetAvailable => {
                    cached_pipeline.state = CachedPipelineState::Queued;
                }

                // Shader could not be processed ... retrying won't help
                ShaderCacheError::ProcessShaderError(err) => {
                    let error_detail =
                        err.emit_to_string(&self.shader_cache.lock().unwrap().composer);
                    if std::env::var("VERBOSE_SHADER_ERROR")
                        .is_ok_and(|v| !(v.is_empty() || v == "0" || v == "false"))
                    {
                        error!("{}", pipeline_error_context(cached_pipeline));
                    }
                    error!("failed to process shader error:\n{}", error_detail);
                    return;
                }
                ShaderCacheError::CreateShaderModule(description) => {
                    error!("failed to create shader module: {}", description);
                    return;
                }
            },

            CachedPipelineState::Ok(_) => return,
        }

        // Retry
        self.waiting_pipelines.insert(id);
    }

    pub fn process_pipeline_queue_system(mut cache: ResMut<Self>) {
        cache.process_queue();
    }

    pub(crate) fn extract_shaders(
        mut cache: ResMut<Self>,
        shaders: Extract<Res<Assets<Shader>>>,
        mut events: Extract<MessageReader<AssetEvent<Shader>>>,
    ) {
        if cache.needs_shader_reload {
            cache.needs_shader_reload = false;
            for (id, shader) in shaders.iter() {
                let mut shader = shader.clone();
                shader.shader_defs.extend(cache.global_shader_defs.clone());
                cache.set_shader(id, shader);
            }
            // Drain events so we don't double-process shaders we just loaded.
            for _ in events.read() {}
            return;
        }

        for event in events.read() {
            #[expect(
                clippy::match_same_arms,
                reason = "LoadedWithDependencies is marked as a TODO, so it's likely this will no longer lint soon."
            )]
            match event {
                // PERF: Instead of blocking waiting for the shader cache lock, try again next frame if the lock is currently held
                AssetEvent::Added { id } | AssetEvent::Modified { id } => {
                    if let Some(shader) = shaders.get(*id) {
                        let mut shader = shader.clone();
                        shader.shader_defs.extend(cache.global_shader_defs.clone());

                        cache.set_shader(*id, shader);
                    }
                }
                AssetEvent::Removed { id } => cache.remove_shader(*id),
                AssetEvent::Unused { .. } => {}
                AssetEvent::LoadedWithDependencies { .. } => {
                    // TODO: handle this
                }
            }
        }
    }
}

fn pipeline_error_context(cached_pipeline: &CachedPipeline) -> String {
    fn format(
        shader: &Handle<Shader>,
        entry: &Option<Cow<'static, str>>,
        shader_defs: &[ShaderDefVal],
    ) -> String {
        let source = match shader.path() {
            Some(path) => path.path().to_string_lossy().to_string(),
            None => String::new(),
        };
        let entry = match entry {
            Some(entry) => entry.to_string(),
            None => String::new(),
        };
        let shader_defs = shader_defs
            .iter()
            .flat_map(|def| match def {
                ShaderDefVal::Bool(k, v) if *v => Some(k.to_string()),
                ShaderDefVal::Int(k, v) => Some(format!("{k} = {v}")),
                ShaderDefVal::UInt(k, v) => Some(format!("{k} = {v}")),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join(", ");
        format!("{source}:{entry}\nshader defs: {shader_defs}")
    }
    match &cached_pipeline.descriptor {
        PipelineDescriptor::RenderPipelineDescriptor(desc) => {
            let vert = &desc.vertex;
            let vert_str = format(&vert.shader, &vert.entry_point, &vert.shader_defs);
            let Some(frag) = desc.fragment.as_ref() else {
                return vert_str;
            };
            let frag_str = format(&frag.shader, &frag.entry_point, &frag.shader_defs);
            format!("vertex {vert_str}\nfragment {frag_str}")
        }
        PipelineDescriptor::ComputePipelineDescriptor(desc) => {
            format(&desc.shader, &desc.entry_point, &desc.shader_defs)
        }
    }
}

#[cfg(all(
    not(target_arch = "wasm32"),
    not(target_os = "macos"),
    feature = "multi_threaded"
))]
fn create_pipeline_task(
    task: impl Future<Output = Result<Pipeline, ShaderCacheError>> + Send + 'static,
    sync: bool,
) -> CachedPipelineState {
    if !sync {
        return CachedPipelineState::Creating(bevy_tasks::AsyncComputeTaskPool::get().spawn(task));
    }

    match bevy_tasks::block_on(task) {
        Ok(pipeline) => CachedPipelineState::Ok(pipeline),
        Err(err) => CachedPipelineState::Err(err),
    }
}

#[cfg(any(
    target_arch = "wasm32",
    target_os = "macos",
    not(feature = "multi_threaded")
))]
fn create_pipeline_task(
    task: impl Future<Output = Result<Pipeline, ShaderCacheError>> + Send + 'static,
    _sync: bool,
) -> CachedPipelineState {
    match bevy_tasks::block_on(task) {
        Ok(pipeline) => CachedPipelineState::Ok(pipeline),
        Err(err) => CachedPipelineState::Err(err),
    }
}
