//! Persistent source Gaussian cube filtering and final roughness remap.
use super::fullscreen::{self, FullscreenPass, Samplers};
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
const GAUSSIAN: usize = 0;
const REMAP: usize = 1;
const COPY: usize = 2;
const CAPTURE: usize = 3;
pub struct ProbeFilter {
    pipelines: [CachedRenderPipelineId; 4],
    gaussian_bindings: [[FilterBinding; 6]; 6],
    remap_bindings: [[FilterBinding; 6]; 6],
    copy_bindings: [[FilterBinding; 6]; 7],
    capture_binding: BindGroup,
}
impl ProbeFilter {
    pub fn pipelines(&self) -> [CachedRenderPipelineId; 4] {
        self.pipelines
    }
    /// Buffers and bind groups are prepared once. Draws only borrow them.
    pub fn new(
        device: &RenderDevice,
        queue: &RenderQueue,
        cache: &PipelineCache,
        samplers: &Samplers,
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
            fullscreen::queue(cache, entry, layout, &shader, entry, HDR_FORMAT)
        };
        let pipelines = pipelines.unwrap_or_else(|| {
            [
                pipeline("gaussian", &layout),
                pipeline("remap", &layout),
                pipeline("copy", &layout),
                pipeline("capture", &capture_layout),
            ]
        });
        let point = &samplers.point;
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
                make_binding(index + 1, face, &source.levels[index], point, 0.)
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
                make_binding(mip, face, &targets.base.levels[mip], point, 0.)
            })
        });
        let capture_binding = device.create_bind_group(
            "source_probe_capture",
            &cache.get_bind_group_layout(&capture_layout),
            &BindGroupEntries::with_indices(((3, capture),)),
        );
        Self {
            pipelines,
            gaussian_bindings,
            remap_bindings,
            copy_bindings,
            capture_binding,
        }
    }
    pub fn capture_face(
        &self,
        encoder: &mut CommandEncoder,
        cache: &PipelineCache,
        targets: &ProbeTargets,
        face: usize,
    ) -> bool {
        let Some(pipeline) = cache.get_render_pipeline(self.pipelines[CAPTURE]) else {
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
        let Some(pipeline) = cache.get_render_pipeline(self.pipelines[COPY]) else {
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
        let pipeline = cache.get_render_pipeline(self.pipelines[COPY]).unwrap();
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
        let pipeline = cache.get_render_pipeline(self.pipelines[REMAP]).unwrap();
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
        let pipeline = cache.get_render_pipeline(self.pipelines[GAUSSIAN]).unwrap();
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
impl FullscreenPass for ProbeFilter {
    const NAME: &'static str = "Source probe pipeline";
    fn pipeline_ids(&self) -> &[CachedRenderPipelineId] {
        &self.pipelines
    }
}
fn draw(
    encoder: &mut CommandEncoder,
    pipeline: &RenderPipeline,
    group: &BindGroup,
    output: &TextureView,
) {
    fullscreen::draw(
        encoder,
        "source_probe_filter",
        pipeline,
        group,
        &[output],
        Some(LinearRgba::BLACK),
    );
}
