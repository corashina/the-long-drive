//! Preallocated fragment equivalents of the source postprocessing compute passes.
pub mod ao_composite;
pub mod ao_depth;
pub mod ao_render;
pub mod ao_upsample;
pub mod bloom;
pub mod chain;
pub mod copy;
pub mod dither;
pub mod dof;
pub mod exposure;
pub mod heat_haze;
pub mod lut;
pub mod lut_ldr;
pub mod motion_blur;
pub mod taa;
pub mod uber;

use super::render_targets::{HDR_FORMAT, HISTOGRAM_FORMAT};
use crate::reference::postprocessing::HistogramLayout;
use bevy::prelude::*;
use bevy::render::{
    render_resource::{binding_types::*, *},
    renderer::{RenderDevice, RenderQueue},
};
#[derive(Clone, Copy, ShaderType)]
struct HistogramParams {
    source_size: UVec2,
    sample_extent: UVec2,
    scatter_extent: UVec2,
    tile_columns: u32,
    input_tiles: u32,
}
struct HistogramTarget {
    _texture: Texture,
    view: TextureView,
}
struct HistogramBinding {
    _uniform: UniformBuffer<HistogramParams>,
    group: BindGroup,
}
pub struct HistogramPasses {
    pub layout: HistogramLayout,
    scatter: HistogramTarget,
    reductions: Vec<HistogramTarget>,
    bindings: Vec<HistogramBinding>,
    pipelines: [CachedRenderPipelineId; 3],
}
impl HistogramPasses {
    /// Call only at startup or physical resize. All targets and bindings remain stable between calls.
    pub fn new(
        device: &RenderDevice,
        queue: &RenderQueue,
        cache: &PipelineCache,
        shader: Handle<Shader>,
        source: &TextureView,
        width: u32,
        height: u32,
        previous: Option<&Self>,
    ) -> Result<Self, String> {
        let layout = HistogramLayout::new(width, height, device.limits().max_texture_dimension_2d)?;
        let scatter_layout = BindGroupLayoutDescriptor::new(
            "source_histogram_scatter",
            &BindGroupLayoutEntries::sequential(
                ShaderStages::VERTEX,
                (
                    uniform_buffer::<HistogramParams>(false),
                    texture_2d(TextureSampleType::Float { filterable: true }),
                    sampler(SamplerBindingType::Filtering),
                ),
            ),
        );
        let reduction_layout = |binding, kind| {
            BindGroupLayoutDescriptor::new(
                "source_histogram_reduce",
                &BindGroupLayoutEntries::with_indices(
                    ShaderStages::FRAGMENT,
                    (
                        (0, uniform_buffer::<HistogramParams>(false)),
                        (binding, texture_2d(kind)),
                    ),
                ),
            )
        };
        let float_layout = reduction_layout(3, TextureSampleType::Float { filterable: false });
        let uint_layout = reduction_layout(4, TextureSampleType::Uint);
        let make_pipeline = |name: &'static str,
                             vertex: &'static str,
                             layout: BindGroupLayoutDescriptor,
                             format,
                             blend| {
            cache.queue_render_pipeline(RenderPipelineDescriptor {
                label: Some(name.into()),
                layout: vec![layout],
                vertex: VertexState {
                    shader: shader.clone(),
                    entry_point: Some(vertex.into()),
                    ..default()
                },
                fragment: Some(FragmentState {
                    shader: shader.clone(),
                    entry_point: Some(name.into()),
                    targets: vec![Some(ColorTargetState {
                        format,
                        blend,
                        write_mask: ColorWrites::ALL,
                    })],
                    ..default()
                }),
                ..default()
            })
        };
        let addition = BlendComponent {
            src_factor: BlendFactor::One,
            dst_factor: BlendFactor::One,
            operation: BlendOperation::Add,
        };
        let pipelines = previous.map(|p| p.pipelines).unwrap_or_else(|| {
            [
                make_pipeline(
                    "scatter_fragment",
                    "scatter_vertex",
                    scatter_layout.clone(),
                    HDR_FORMAT,
                    Some(BlendState {
                        color: addition,
                        alpha: addition,
                    }),
                ),
                make_pipeline(
                    "reduce_float",
                    "reduce_vertex",
                    float_layout.clone(),
                    HISTOGRAM_FORMAT,
                    None,
                ),
                make_pipeline(
                    "reduce_uint",
                    "reduce_vertex",
                    uint_layout.clone(),
                    HISTOGRAM_FORMAT,
                    None,
                ),
            ]
        });
        let make_target = |extent: [u32; 2], format| {
            let texture = device.create_texture(&TextureDescriptor {
                label: Some("source_histogram"),
                size: Extent3d {
                    width: extent[0],
                    height: extent[1],
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: TextureDimension::D2,
                format,
                usage: TextureUsages::TEXTURE_BINDING | TextureUsages::RENDER_ATTACHMENT,
                view_formats: &[],
            });
            let view = texture.create_view(&default());
            HistogramTarget {
                _texture: texture,
                view,
            }
        };
        let scatter = make_target(layout.scatter, HDR_FORMAT);
        let reductions: Vec<_> = layout
            .reduction_rows
            .iter()
            .map(|rows| make_target([32, *rows], HISTOGRAM_FORMAT))
            .collect();
        let sampler = device.create_sampler(&SamplerDescriptor {
            label: Some("source_histogram_bilinear"),
            mag_filter: FilterMode::Linear,
            min_filter: FilterMode::Linear,
            ..default()
        });
        let parameters = |columns, tiles| HistogramParams {
            source_size: UVec2::from_array(layout.source),
            sample_extent: UVec2::from_array(layout.samples),
            scatter_extent: UVec2::from_array(layout.scatter),
            tile_columns: columns,
            input_tiles: tiles,
        };
        let mut uniform = UniformBuffer::from(parameters(layout.columns, layout.tiles));
        uniform.write_buffer(device, queue);
        let group = device.create_bind_group(
            "source_histogram_scatter",
            &cache.get_bind_group_layout(&scatter_layout),
            &BindGroupEntries::sequential((uniform.binding().unwrap(), source, &sampler)),
        );
        let mut bindings = vec![HistogramBinding {
            _uniform: uniform,
            group,
        }];
        for index in 0..reductions.len() {
            let (input, bind_layout, binding, columns, tiles) = if index == 0 {
                (
                    &scatter.view,
                    &float_layout,
                    3,
                    layout.columns,
                    layout.tiles,
                )
            } else {
                (
                    &reductions[index - 1].view,
                    &uint_layout,
                    4,
                    1,
                    layout.reduction_rows[index - 1],
                )
            };
            let mut uniform = UniformBuffer::from(parameters(columns, tiles));
            uniform.write_buffer(device, queue);
            let group = device.create_bind_group(
                "source_histogram_reduce",
                &cache.get_bind_group_layout(bind_layout),
                &BindGroupEntries::with_indices((
                    (0, uniform.binding().unwrap()),
                    (binding, input),
                )),
            );
            bindings.push(HistogramBinding {
                _uniform: uniform,
                group,
            });
        }
        Ok(Self {
            layout,
            scatter,
            reductions,
            bindings,
            pipelines,
        })
    }
    pub fn ready(&self, cache: &PipelineCache) -> bool {
        self.pipelines
            .iter()
            .all(|id| cache.get_render_pipeline(*id).is_some())
    }
    pub fn failure(&self, cache: &PipelineCache) -> Option<String> {
        pipeline_failure(cache, &self.pipelines)
    }
    pub fn output(&self) -> &TextureView {
        &self.reductions.last().unwrap().view
    }
    pub fn encode(&self, encoder: &mut CommandEncoder, cache: &PipelineCache) -> bool {
        if !self.ready(cache) {
            return false;
        }
        draw_histogram(
            encoder,
            cache.get_render_pipeline(self.pipelines[0]).unwrap(),
            &self.bindings[0].group,
            &self.scatter.view,
            self.layout.sample_count * 6,
        );
        for (index, target) in self.reductions.iter().enumerate() {
            draw_histogram(
                encoder,
                cache
                    .get_render_pipeline(self.pipelines[if index == 0 { 1 } else { 2 }])
                    .unwrap(),
                &self.bindings[index + 1].group,
                &target.view,
                3,
            );
        }
        true
    }
}
fn draw_histogram(
    encoder: &mut CommandEncoder,
    pipeline: &RenderPipeline,
    group: &BindGroup,
    target: &TextureView,
    vertices: u32,
) {
    let attachments = [Some(RenderPassColorAttachment {
        view: target,
        depth_slice: None,
        resolve_target: None,
        ops: Operations {
            load: LoadOp::Clear(LinearRgba::NONE.into()),
            store: StoreOp::Store,
        },
    })];
    let mut pass = encoder.begin_render_pass(&RenderPassDescriptor {
        label: Some("source_histogram"),
        color_attachments: &attachments,
        depth_stencil_attachment: None,
        timestamp_writes: None,
        occlusion_query_set: None,
        ..default()
    });
    pass.set_pipeline(pipeline);
    pass.set_bind_group(0, group, &[]);
    pass.draw(0..vertices, 0..1);
}

pub(super) fn pipeline_failure(
    cache: &PipelineCache,
    ids: &[CachedRenderPipelineId],
) -> Option<String> {
    ids.iter()
        .find_map(|id| match cache.get_render_pipeline_state(*id) {
            CachedPipelineState::Err(
                bevy::shader::ShaderCacheError::ShaderNotLoaded(_)
                | bevy::shader::ShaderCacheError::ShaderImportNotYetAvailable,
            ) => None,
            CachedPipelineState::Err(error) => Some(format!("Source effect pipeline: {error}")),
            _ => None,
        })
}
