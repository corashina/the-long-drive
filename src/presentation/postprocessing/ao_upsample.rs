//! Persistent float32 blur halos preserve the source raw-fetch clamp order.
use super::{
    ao_depth::{AoDepthPass, AoDepthTarget},
    ao_render::AoRenderPass,
};
use crate::reference::postprocessing::OcclusionSettings;
use bevy::prelude::*;
use bevy::render::{
    render_resource::{binding_types::*, *},
    renderer::{RenderDevice, RenderQueue},
};
#[derive(Clone, Copy, ShaderType)]
struct UpsampleParams {
    settings: Vec4,
    control: UVec4,
}
pub struct AoUpsamplePass {
    /// Combined3, Combined2, Combined1, then the full-resolution visibility.
    pub output: [AoDepthTarget; 4],
    pub logical_bytes: u64,
    horizontal: [AoDepthTarget; 4],
    vertical: [AoDepthTarget; 4],
    _uniforms: [UniformBuffer<UpsampleParams>; 4],
    bindings: [[BindGroup; 3]; 4],
    pipelines: [CachedRenderPipelineId; 3],
}
impl AoUpsamplePass {
    pub fn new(
        device: &RenderDevice,
        queue: &RenderQueue,
        cache: &PipelineCache,
        shader: Handle<Shader>,
        depth: &AoDepthPass,
        ao: &AoRenderPass,
        settings: &OcclusionSettings,
        previous: Option<&Self>,
    ) -> Result<Self, String> {
        let low_extents = std::array::from_fn::<_, 4, _>(|i| depth.low[3 - i].extent);
        let maximum = device.limits().max_texture_dimension_2d;
        if low_extents
            .iter()
            .any(|s| s[0] + 2 > maximum || s[1] + 6 > maximum)
        {
            return Err("Source AO blur halo exceeds the device texture extent".into());
        }
        let horizontal = low_extents.map(|s| {
            AoDepthTarget::new(device, [s[0] + 2, s[1] + 6], TextureFormat::R32Float, false)
        });
        let vertical = low_extents.map(|s| {
            AoDepthTarget::new(device, [s[0] + 2, s[1] + 2], TextureFormat::R32Float, false)
        });
        let output = std::array::from_fn(|i| {
            AoDepthTarget::new(
                device,
                if i == 3 {
                    depth.linear.extent
                } else {
                    depth.low[2 - i].extent
                },
                TextureFormat::R8Unorm,
                false,
            )
        });
        let area = |t: &AoDepthTarget| u64::from(t.extent[0]) * u64::from(t.extent[1]);
        let logical_bytes = output.iter().map(area).sum::<u64>()
            + horizontal
                .iter()
                .chain(vertical.iter())
                .map(|t| area(t) * 4)
                .sum::<u64>();
        let uniforms = std::array::from_fn(|i| {
            let step = 1920. / low_extents[i][0] as f32;
            let difference = 1. - 10_f32.powf(settings.blur_tolerance) * step;
            let upsample = 10_f32.powf(settings.upsample_tolerance);
            let mut buffer = UniformBuffer::from(UpsampleParams {
                settings: Vec4::new(
                    1. / (10_f32.powf(settings.noise_tolerance) + upsample),
                    step,
                    difference * difference,
                    upsample,
                ),
                control: UVec4::new(u32::from(i < 3), 0, 0, 0),
            });
            buffer.write_buffer(device, queue);
            buffer
        });
        let sample = || texture_2d(TextureSampleType::Float { filterable: false });
        let layout = BindGroupLayoutDescriptor::new(
            "source_ao_upsample",
            &BindGroupLayoutEntries::sequential(
                ShaderStages::FRAGMENT,
                (
                    uniform_buffer::<UpsampleParams>(false),
                    sample(),
                    sample(),
                    sample(),
                    sample(),
                    sample(),
                ),
            ),
        );
        let bindings = std::array::from_fn(|i| {
            std::array::from_fn(|stage| {
                let low_ao = if i == 0 {
                    &ao.output[3].view
                } else {
                    &output[i - 1].view
                };
                let blur = match stage {
                    0 => low_ao,
                    1 => &horizontal[i].view,
                    _ => &vertical[i].view,
                };
                let high_depth = if i == 3 {
                    &depth.linear.view
                } else {
                    &depth.low[2 - i].view
                };
                let high_ao = &ao.output[2_usize.saturating_sub(i)].view;
                device.create_bind_group(
                    "source_ao_upsample",
                    &cache.get_bind_group_layout(&layout),
                    &BindGroupEntries::sequential((
                        uniforms[i].binding().unwrap(),
                        &depth.low[3 - i].view,
                        low_ao,
                        blur,
                        high_depth,
                        high_ao,
                    )),
                )
            })
        });
        // Pipeline descriptors do not depend on the physical extent.
        let pipelines = previous.map_or_else(
            || {
                std::array::from_fn(|i| {
                    let name = ["horizontal", "vertical", "bilateral"][i];
                    cache.queue_render_pipeline(RenderPipelineDescriptor {
                        label: Some(format!("source_ao_{name}").into()),
                        layout: vec![layout.clone()],
                        vertex: VertexState {
                            shader: shader.clone(),
                            entry_point: Some("vertex".into()),
                            ..default()
                        },
                        fragment: Some(FragmentState {
                            shader: shader.clone(),
                            entry_point: Some(name.into()),
                            targets: vec![Some(ColorTargetState {
                                format: if i == 2 {
                                    TextureFormat::R8Unorm
                                } else {
                                    TextureFormat::R32Float
                                },
                                blend: None,
                                write_mask: ColorWrites::RED,
                            })],
                            ..default()
                        }),
                        ..default()
                    })
                })
            },
            |pass| pass.pipelines,
        );
        Ok(Self {
            output,
            logical_bytes,
            horizontal,
            vertical,
            _uniforms: uniforms,
            bindings,
            pipelines,
        })
    }
    pub fn ready(&self, cache: &PipelineCache) -> bool {
        self.pipelines
            .iter()
            .all(|&id| cache.get_render_pipeline(id).is_some())
    }
    pub fn failure(&self, cache: &PipelineCache) -> Option<String> {
        self.pipelines
            .iter()
            .find_map(|&id| match cache.get_render_pipeline_state(id) {
                CachedPipelineState::Err(
                    bevy::shader::ShaderCacheError::ShaderNotLoaded(_)
                    | bevy::shader::ShaderCacheError::ShaderImportNotYetAvailable,
                ) => None,
                CachedPipelineState::Err(e) => Some(format!("Source AO upsample pipeline: {e}")),
                _ => None,
            })
    }
    pub fn encode(&self, encoder: &mut CommandEncoder, cache: &PipelineCache) -> bool {
        if !self.ready(cache) {
            return false;
        }
        for i in 0..4 {
            for (stage, target) in [&self.horizontal[i], &self.vertical[i], &self.output[i]]
                .into_iter()
                .enumerate()
            {
                let colors = [Some(RenderPassColorAttachment {
                    view: &target.view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: Operations {
                        load: LoadOp::Clear(Default::default()),
                        store: StoreOp::Store,
                    },
                })];
                let mut pass = encoder.begin_render_pass(&RenderPassDescriptor {
                    label: Some("source_ao_upsample"),
                    color_attachments: &colors,
                    ..default()
                });
                pass.set_pipeline(cache.get_render_pipeline(self.pipelines[stage]).unwrap());
                pass.set_bind_group(0, &self.bindings[i][stage], &[]);
                pass.draw(0..3, 0..1);
            }
        }
        true
    }
}
