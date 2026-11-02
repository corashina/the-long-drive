//! Persistent source Gaussian cube filtering and final roughness remap.
use super::render_targets::{HDR_FORMAT, ProbeTargets};
use crate::reference::reflections::{CUBE_BASES, source_filter_levels};
use bevy::prelude::*;
use bevy::render::{
    render_resource::{binding_types::*, *},
    renderer::{RenderDevice, RenderQueue},
};

#[derive(Clone, Copy, ShaderType)]
struct FilterParams {
    right: Vec4,
    up: Vec4,
    back: Vec4,
    size: f32,
    texel: f32,
    level: f32,
    padding: f32,
}
struct FilterBinding {
    _uniform: UniformBuffer<FilterParams>,
    group: BindGroup,
}
pub struct ProbeFilter {
    gaussian: CachedRenderPipelineId,
    remap: CachedRenderPipelineId,
    copy: CachedRenderPipelineId,
    capture: CachedRenderPipelineId,
    gaussian_bindings: [[FilterBinding; 6]; 6],
    remap_bindings: [[FilterBinding; 6]; 6],
    copy_bindings: [[FilterBinding; 6]; 7],
    capture_binding: BindGroup,
}
impl ProbeFilter {
    pub fn pipelines(&self) -> [CachedRenderPipelineId; 4] {
        [self.gaussian, self.remap, self.copy, self.capture]
    }
    /// Buffers and bind groups are prepared once. Draws only borrow them.
    pub fn new(
        device: &RenderDevice,
        queue: &RenderQueue,
        cache: &PipelineCache,
        shader: Handle<Shader>,
        targets: &ProbeTargets,
        capture: &TextureView,
        pipelines: Option<[CachedRenderPipelineId; 4]>,
    ) -> Self {
        let layout = BindGroupLayoutDescriptor::new(
            "source_probe_filter",
            &BindGroupLayoutEntries::sequential(
                ShaderStages::FRAGMENT,
                (
                    uniform_buffer::<FilterParams>(false),
                    texture_cube(TextureSampleType::Float { filterable: true }),
                    sampler(SamplerBindingType::Filtering),
                ),
            ),
        );
        let capture_layout = BindGroupLayoutDescriptor::new(
            "source_probe_capture",
            &BindGroupLayoutEntries::with_indices(
                ShaderStages::FRAGMENT,
                ((3, texture_2d(TextureSampleType::Float { filterable: true })),),
            ),
        );
        let pipeline = |entry: &'static str, layout: &BindGroupLayoutDescriptor| {
            cache.queue_render_pipeline(RenderPipelineDescriptor {
                label: Some(entry.into()),
                layout: vec![layout.clone()],
                vertex: VertexState {
                    shader: shader.clone(),
                    entry_point: Some("vertex".into()),
                    ..default()
                },
                fragment: Some(FragmentState {
                    shader: shader.clone(),
                    entry_point: Some(entry.into()),
                    targets: vec![Some(ColorTargetState {
                        format: HDR_FORMAT,
                        blend: None,
                        write_mask: ColorWrites::ALL,
                    })],
                    ..default()
                }),
                ..default()
            })
        };
        let [gaussian, remap, copy, capture_pipeline] = pipelines.unwrap_or_else(|| {
            [
                pipeline("gaussian", &layout),
                pipeline("remap", &layout),
                pipeline("copy", &layout),
                pipeline("capture", &capture_layout),
            ]
        });
        let point = device.create_sampler(&SamplerDescriptor {
            label: Some("source_probe_point"),
            mag_filter: FilterMode::Nearest,
            min_filter: FilterMode::Nearest,
            mipmap_filter: MipmapFilterMode::Nearest,
            ..default()
        });
        let linear = device.create_sampler(&SamplerDescriptor {
            label: Some("source_probe_trilinear"),
            mag_filter: FilterMode::Linear,
            min_filter: FilterMode::Linear,
            mipmap_filter: MipmapFilterMode::Linear,
            ..default()
        });
        let levels = source_filter_levels();
        let make_binding =
            |mip: usize, face: usize, input: &TextureView, sampler: &Sampler, level: f32| {
                let [right, up, back] = CUBE_BASES[face];
                let size = (64 >> mip) as f32;
                let mut uniform = UniformBuffer::from(FilterParams {
                    right: Vec3::from_array(right).extend(0.),
                    up: Vec3::from_array(up).extend(0.),
                    back: Vec3::from_array(back).extend(0.),
                    size,
                    texel: 1. / size,
                    level,
                    padding: 0.,
                });
                uniform.write_buffer(device, queue);
                let group = device.create_bind_group(
                    "source_probe_filter",
                    &cache.get_bind_group_layout(&layout),
                    &BindGroupEntries::sequential((uniform.binding().unwrap(), input, sampler)),
                );
                FilterBinding {
                    _uniform: uniform,
                    group,
                }
            };
        let gaussian_bindings = std::array::from_fn(|index| {
            std::array::from_fn(|face| {
                let source = if index % 2 == 0 {
                    &targets.base
                } else {
                    &targets.auxiliary
                };
                make_binding(index + 1, face, &source.levels[index], &point, 0.)
            })
        });
        let remap_bindings = std::array::from_fn(|index| {
            std::array::from_fn(|face| {
                make_binding(
                    index + 1,
                    face,
                    &targets.auxiliary.cube,
                    &linear,
                    levels[index],
                )
            })
        });
        let copy_bindings = std::array::from_fn(|mip| {
            std::array::from_fn(|face| {
                make_binding(mip, face, &targets.base.levels[mip], &point, 0.)
            })
        });
        let capture_binding = device.create_bind_group(
            "source_probe_capture",
            &cache.get_bind_group_layout(&capture_layout),
            &BindGroupEntries::with_indices(((3, capture),)),
        );
        Self {
            gaussian,
            remap,
            copy,
            capture: capture_pipeline,
            gaussian_bindings,
            remap_bindings,
            copy_bindings,
            capture_binding,
        }
    }
    pub fn ready(&self, cache: &PipelineCache) -> bool {
        self.pipelines()
            .into_iter()
            .all(|id| cache.get_render_pipeline(id).is_some())
    }
    pub fn failure(&self, cache: &PipelineCache) -> Option<String> {
        self.pipelines()
            .into_iter()
            .find_map(|id| match cache.get_render_pipeline_state(id) {
                CachedPipelineState::Err(
                    bevy::shader::ShaderCacheError::ShaderNotLoaded(_)
                    | bevy::shader::ShaderCacheError::ShaderImportNotYetAvailable,
                ) => None,
                CachedPipelineState::Err(error) => Some(format!("Source probe pipeline: {error}")),
                _ => None,
            })
    }
    pub fn capture_face(
        &self,
        encoder: &mut CommandEncoder,
        cache: &PipelineCache,
        targets: &ProbeTargets,
        face: usize,
    ) -> bool {
        let Some(pipeline) = cache.get_render_pipeline(self.capture) else {
            return false;
        };
        draw(
            encoder,
            pipeline,
            &self.capture_binding,
            &targets.base.faces[0][face],
        );
        true
    }
    pub fn publish(
        &self,
        encoder: &mut CommandEncoder,
        cache: &PipelineCache,
        targets: &ProbeTargets,
    ) -> bool {
        let Some(pipeline) = cache.get_render_pipeline(self.copy) else {
            return false;
        };
        for mip in 0..7 {
            for face in 0..6 {
                draw(
                    encoder,
                    pipeline,
                    &self.copy_bindings[mip][face].group,
                    &targets.published_faces[mip][face],
                );
            }
        }
        true
    }
    /// Job seven waits for all pipelines before encoding or advancing.
    pub fn first_mip(
        &self,
        encoder: &mut CommandEncoder,
        cache: &PipelineCache,
        targets: &ProbeTargets,
    ) -> bool {
        if !self.ready(cache) {
            return false;
        }
        self.gaussian_mip(encoder, cache, targets, 1);
        true
    }
    /// Job eight filters all remaining levels. Publication requires a separate validation gate.
    pub fn remaining_mips(
        &self,
        encoder: &mut CommandEncoder,
        cache: &PipelineCache,
        targets: &ProbeTargets,
    ) -> bool {
        if !self.ready(cache) {
            return false;
        }
        for mip in 2..7 {
            self.gaussian_mip(encoder, cache, targets, mip);
        }
        let pipeline = cache.get_render_pipeline(self.copy).unwrap();
        for mip in [0, 2, 4, 6] {
            for face in 0..6 {
                draw(
                    encoder,
                    pipeline,
                    &self.copy_bindings[mip][face].group,
                    &targets.auxiliary.faces[mip][face],
                );
            }
        }
        let pipeline = cache.get_render_pipeline(self.remap).unwrap();
        for index in 0..6 {
            for face in 0..6 {
                draw(
                    encoder,
                    pipeline,
                    &self.remap_bindings[index][face].group,
                    &targets.base.faces[index + 1][face],
                );
            }
        }
        true
    }
    fn gaussian_mip(
        &self,
        encoder: &mut CommandEncoder,
        cache: &PipelineCache,
        targets: &ProbeTargets,
        mip: usize,
    ) {
        let output = if mip % 2 == 1 {
            &targets.auxiliary
        } else {
            &targets.base
        };
        let pipeline = cache.get_render_pipeline(self.gaussian).unwrap();
        for face in 0..6 {
            draw(
                encoder,
                pipeline,
                &self.gaussian_bindings[mip - 1][face].group,
                &output.faces[mip][face],
            );
        }
    }
}
fn draw(
    encoder: &mut CommandEncoder,
    pipeline: &RenderPipeline,
    group: &BindGroup,
    output: &TextureView,
) {
    let attachments = [Some(RenderPassColorAttachment {
        view: output,
        depth_slice: None,
        resolve_target: None,
        ops: Operations {
            load: LoadOp::Clear(LinearRgba::BLACK.into()),
            store: StoreOp::Store,
        },
    })];
    let mut pass = encoder.begin_render_pass(&RenderPassDescriptor {
        label: Some("source_probe_filter"),
        color_attachments: &attachments,
        depth_stencil_attachment: None,
        timestamp_writes: None,
        occlusion_query_set: None,
        ..default()
    });
    pass.set_pipeline(pipeline);
    pass.set_bind_group(0, group, &[]);
    pass.draw(0..3, 0..1);
}

/// Capture completion is separate from the main-world eight-job scheduler.
/// A failed capture leaves the last published cube valid.
#[derive(Default, Debug)]
pub struct ProbePublication {
    faces: u8,
    mips: u8,
    pending: Option<(u32, crate::reference::reflections::ProbeJob)>,
    validation_token: Option<u32>,
    approved_token: Option<u32>,
    publication_token: Option<u32>,
    published: bool,
    generation: u32,
}
impl ProbePublication {
    pub fn face_mask(&self) -> u8 {
        self.faces
    }
    pub fn mip_mask(&self) -> u8 {
        self.mips
    }
    pub fn published(&self) -> bool {
        self.published
    }
    pub fn generation(&self) -> u32 {
        self.generation
    }
    pub fn validation_token(&self) -> Option<u32> {
        self.validation_token
    }
    pub fn waiting(&self) -> bool {
        self.pending.is_some()
            || self.validation_token.is_some()
            || self.approved_token.is_some()
            || self.publication_token.is_some()
    }
    /// Record a fully encoded job. Call before its commands enter a submission.
    pub fn encoded(
        &mut self,
        token: u32,
        job: crate::reference::reflections::ProbeJob,
    ) -> Result<(), &'static str> {
        use crate::reference::reflections::ProbeJob;
        if token == 0 || self.waiting() {
            return Err("Source probe has a pending job or invalid token");
        }
        let valid = match job {
            // Face 0 starts a new capture. It drops the faces of an abandoned capture.
            ProbeJob::Face(0) => {
                self.discard_capture();
                true
            }
            ProbeJob::Face(face) if face < 6 => self.faces == (1u8 << face) - 1 && self.mips == 0,
            ProbeJob::FirstMip => self.faces == 0x3f && self.mips == 1,
            ProbeJob::RemainingMipsAndPublish => self.faces == 0x3f && self.mips == 3,
            _ => false,
        };
        if !valid {
            return Err("Source probe face or mip work is incomplete");
        }
        self.pending = Some((token, job));
        Ok(())
    }
    /// Acknowledge exactly the job whose complete commands entered the queue.
    /// Return true for jobs that the main-world scheduler can advance now.
    pub fn submitted(&mut self, token: u32) -> Result<bool, &'static str> {
        use crate::reference::reflections::ProbeJob;
        let Some((expected, job)) = self.pending else {
            return Err("Source probe submission has no encoded job");
        };
        if token != expected {
            return Err("Source probe submission token differs");
        }
        self.pending = None;
        match job {
            ProbeJob::Face(face) => {
                self.faces |= 1 << face;
                if self.faces == 0x3f {
                    self.mips = 1;
                }
                Ok(true)
            }
            ProbeJob::FirstMip => {
                self.mips |= 2;
                Ok(true)
            }
            ProbeJob::RemainingMipsAndPublish => {
                self.mips = 0x7f;
                self.validation_token = Some(token);
                Ok(false)
            }
        }
    }
    /// Call after deferred device validation and completion for the filter submission.
    pub fn validated(&mut self, token: u32, success: bool) -> Result<(), &'static str> {
        if self.validation_token != Some(token) {
            return Err("Source probe validation token differs");
        }
        if !success {
            self.discard_capture();
            return Err("Source probe capture failed GPU validation");
        }
        if self.faces != 0x3f || self.mips != 0x7f {
            return Err("Source probe publication is incomplete");
        }
        self.validation_token = None;
        self.approved_token = Some(token);
        Ok(())
    }
    /// Encode publication only after capture validation. Retain the texture allocation.
    pub fn encode_publication(
        &mut self,
        encoder: &mut CommandEncoder,
        targets: &ProbeTargets,
        filter: &ProbeFilter,
        cache: &PipelineCache,
    ) -> Result<u32, &'static str> {
        let token = self
            .approved_token
            .ok_or("Source probe capture is not validated")?;
        if !filter.publish(encoder, cache, targets) {
            return Err("Source probe copy pipeline is pending");
        }
        self.approved_token = None;
        self.publication_token = Some(token);
        Ok(token)
    }
    /// After this acknowledgement, advance job eight in the main-world scheduler.
    pub fn publication_submitted(&mut self, token: u32) -> Result<(), &'static str> {
        if self.publication_token != Some(token) {
            return Err("Source probe publication token differs");
        }
        self.generation = self
            .generation
            .checked_add(1)
            .ok_or("Source probe generation exhausted")?;
        self.published = true;
        self.discard_capture();
        Ok(())
    }
    /// Retain the prior valid publication when capture work fails or restarts.
    pub fn discard_capture(&mut self) {
        self.faces = 0;
        self.mips = 0;
        self.pending = None;
        self.validation_token = None;
        self.approved_token = None;
        self.publication_token = None;
    }
}
