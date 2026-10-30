//! Four persistent interleaved AO draws over the source depth phase arrays.
use super::ao_depth::{AoDepthPass, AoDepthTarget};
use bevy::prelude::*;
use bevy::render::{
    render_resource::{binding_types::*, *},
    renderer::{RenderAdapter, RenderDevice, RenderQueue},
};
#[derive(Clone, Copy, ShaderType)]
struct AoParams {
    inverse_thickness: [Vec4; 3],
    weights: [Vec4; 3],
    settings: Vec4,
}
pub struct AoRenderPass {
    pub output: [AoDepthTarget; 4],
    pub logical_bytes: u64,
    pipeline: CachedRenderPipelineId,
    bindings: [BindGroup; 4],
    uniforms: [UniformBuffer<AoParams>; 4],
    thickness: [f32; 12],
    atlas_width: [u32; 4],
}
impl AoRenderPass {
    pub fn new(
        adapter: &RenderAdapter,
        device: &RenderDevice,
        queue: &RenderQueue,
        cache: &PipelineCache,
        shader: Handle<Shader>,
        depth: &AoDepthPass,
        intensity: f32,
        thickness: f32,
        previous: Option<&Self>,
    ) -> Result<Self, String> {
        if !intensity.is_finite() || !thickness.is_finite() || thickness <= 0. {
            return Err("Invalid source AO intensity or thickness".into());
        }
        if !adapter
            .get_texture_format_features(TextureFormat::R8Unorm)
            .allowed_usages
            .contains(TextureUsages::RENDER_ATTACHMENT | TextureUsages::TEXTURE_BINDING)
        {
            return Err("Source AO requires a sampled R8 render attachment".into());
        }
        // MultiScaleVO constructor, installed binary32 radicands. No serialized radius is used.
        let sample_thickness = [
            0.9599999785423279_f32,
            0.8399999737739563,
            0.6399999856948853,
            0.35999995470046997,
            0.9199999570846558,
            0.7999999523162842,
            0.5999999642372131,
            0.31999993324279785,
            0.6799999475479126,
            0.47999995946884155,
            0.19999992847442627,
            0.2799999713897705,
        ]
        .map(f32::sqrt);
        let mut weights = std::array::from_fn::<_, 12, _>(|i| {
            sample_thickness[i]
                * if matches!(i, 5 | 6 | 7 | 9 | 10) {
                    8.
                } else {
                    4.
                }
        });
        for i in [0, 2, 5, 7, 9] {
            weights[i] = 0.;
        }
        let total: f32 = weights.iter().sum();
        for weight in &mut weights {
            *weight /= total;
        }
        let packed = std::array::from_fn(|i| Vec4::from_slice(&weights[i * 4..i * 4 + 4]));
        let output = std::array::from_fn(|i| {
            AoDepthTarget::new(device, depth.low[i].extent, TextureFormat::R8Unorm, false)
        });
        let logical_bytes = output
            .iter()
            .map(|t| u64::from(t.extent[0]) * u64::from(t.extent[1]))
            .sum();
        let layout = BindGroupLayoutDescriptor::new(
            "source_ao_interleaved",
            &BindGroupLayoutEntries::sequential(
                ShaderStages::FRAGMENT,
                (
                    uniform_buffer::<AoParams>(false),
                    texture_2d_array(TextureSampleType::Float { filterable: false }),
                ),
            ),
        );
        let uniforms = std::array::from_fn(|_| {
            let mut b = UniformBuffer::from(AoParams {
                inverse_thickness: [Vec4::ZERO; 3],
                weights: packed,
                settings: Vec4::new(-1. / thickness, intensity, 0., 0.),
            });
            b.write_buffer(device, queue);
            b
        });
        let bindings = std::array::from_fn(|i| {
            device.create_bind_group(
                "source_ao_interleaved",
                &cache.get_bind_group_layout(&layout),
                &BindGroupEntries::sequential((
                    uniforms[i].binding().unwrap(),
                    &depth.tiled[i].view,
                )),
            )
        });
        // Pipeline descriptors do not depend on the physical extent.
        let pipeline = previous.map_or_else(
            || {
                cache.queue_render_pipeline(RenderPipelineDescriptor {
                    label: Some("source_ao_interleaved".into()),
                    layout: vec![layout],
                    vertex: VertexState {
                        shader: shader.clone(),
                        entry_point: Some("vertex".into()),
                        ..default()
                    },
                    fragment: Some(FragmentState {
                        shader,
                        entry_point: Some("render".into()),
                        targets: vec![Some(ColorTargetState {
                            format: TextureFormat::R8Unorm,
                            blend: None,
                            write_mask: ColorWrites::RED,
                        })],
                        ..default()
                    }),
                    ..default()
                })
            },
            |pass| pass.pipeline,
        );
        Ok(Self {
            output,
            logical_bytes,
            pipeline,
            bindings,
            uniforms,
            thickness: sample_thickness,
            atlas_width: depth.tiled.each_ref().map(|t| t.extent[0]),
        })
    }
    pub fn ready(&self, cache: &PipelineCache) -> bool {
        cache.get_render_pipeline(self.pipeline).is_some()
    }
    pub fn failure(&self, cache: &PipelineCache) -> Option<String> {
        match cache.get_render_pipeline_state(self.pipeline) {
            CachedPipelineState::Err(
                bevy::shader::ShaderCacheError::ShaderNotLoaded(_)
                | bevy::shader::ShaderCacheError::ShaderImportNotYetAvailable,
            ) => None,
            CachedPipelineState::Err(e) => Some(format!("Source AO interleaved pipeline: {e}")),
            _ => None,
        }
    }
    pub fn encode(
        &mut self,
        encoder: &mut CommandEncoder,
        device: &RenderDevice,
        queue: &RenderQueue,
        cache: &PipelineCache,
        projection_00: f32,
    ) -> Result<bool, String> {
        if !projection_00.is_finite() || projection_00 <= 0. {
            return Err("Invalid source AO projection element00".into());
        }
        let Some(pipeline) = cache.get_render_pipeline(self.pipeline) else {
            return Ok(false);
        };
        for i in 0..4 {
            let inverse_scale =
                1. / ((2. * (1. / projection_00) * 10.) / self.atlas_width[i] as f32);
            self.uniforms[i].get_mut().inverse_thickness = std::array::from_fn(|j| {
                Vec4::from_array(std::array::from_fn(|k| {
                    inverse_scale / self.thickness[j * 4 + k]
                }))
            });
            self.uniforms[i].write_buffer(device, queue);
            let colors = [Some(RenderPassColorAttachment {
                view: &self.output[i].view,
                depth_slice: None,
                resolve_target: None,
                ops: Operations {
                    load: LoadOp::Clear(Default::default()),
                    store: StoreOp::Store,
                },
            })];
            let mut pass = encoder.begin_render_pass(&RenderPassDescriptor {
                label: Some("source_ao_interleaved"),
                color_attachments: &colors,
                ..default()
            });
            pass.set_pipeline(pipeline);
            pass.set_bind_group(0, &self.bindings[i], &[]);
            pass.draw(0..3, 0..1);
        }
        Ok(true)
    }
}
