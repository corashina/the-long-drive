//! Installed fast bloom pyramid, with persistent targets and source half stores.
use crate::reference::postprocessing::BloomSettings;
use bevy::prelude::*;
use bevy::render::{
    render_resource::{binding_types::*, *},
    renderer::{RenderAdapter, RenderDevice, RenderQueue},
};
#[derive(Debug, PartialEq)]
pub struct BloomLayout {
    pub extents: Vec<[u32; 2]>,
    pub sample_scale: f32,
}
impl BloomLayout {
    pub fn new(extent: [u32; 2], diffusion: f32) -> Result<Self, String> {
        if extent.iter().any(|&v| v < 2 || v > 16384)
            || !diffusion.is_finite()
            || !(1. ..=10.).contains(&diffusion)
        {
            return Err("Invalid source bloom extent or diffusion".into());
        }
        let first = extent.map(|v| v / 2);
        // Mathf.Log(f,2) converts both inputs to double; its result then converts to float.
        let logarithm = ((first[0].max(first[1]) as f64).ln() / 2_f64.ln()) as f32;
        let level = (logarithm + diffusion.min(10.)) - 10.;
        let floor = level.floor();
        let count = (floor as i32).clamp(1, 16) as usize;
        let sample_scale = (0.5 + level) - floor;
        let mut extents = Vec::with_capacity(count);
        let mut next = first;
        for _ in 0..count {
            extents.push(next);
            next = next.map(|v| (v / 2).max(1));
        }
        Ok(Self {
            extents,
            sample_scale,
        })
    }
    pub fn logical_bytes(&self) -> u64 {
        self.extents
            .iter()
            .map(|e| u64::from(e[0]) * u64::from(e[1]) * 16)
            .sum()
    }
}
// Installed GammaToLinearSpace65e80. Keep the >1 branch; it differs from the sRGB middle branch.
pub(super) fn gamma_to_linear(value: f32) -> f32 {
    if value <= 0.040449999272823334 {
        value / 12.920000076293945
    } else if value < 1. {
        ((value + 0.054999999701976776) / 1.0549999475479126).powf(2.4000000953674316)
    } else if value == 1. {
        1.
    } else {
        value.powf(2.200000047683716)
    }
}
#[derive(Clone, Copy, ShaderType)]
struct Params {
    output_size: Vec2,
    sample_scale: f32,
    clamp_value: f32,
    threshold: Vec4,
}
struct Target {
    _texture: Texture,
    view: TextureView,
}
impl Target {
    fn new(device: &RenderDevice, extent: [u32; 2]) -> Self {
        let texture = device.create_texture(&TextureDescriptor {
            label: Some("source_bloom"),
            size: Extent3d {
                width: extent[0],
                height: extent[1],
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: TextureDimension::D2,
            format: TextureFormat::Rgba16Float,
            usage: TextureUsages::TEXTURE_BINDING | TextureUsages::RENDER_ATTACHMENT,
            view_formats: &[],
        });
        let view = texture.create_view(&default());
        Self {
            _texture: texture,
            view,
        }
    }
}
struct Binding {
    _uniform: UniformBuffer<Params>,
    groups: [BindGroup; 2],
}
pub struct BloomPass {
    pub layout: BloomLayout,
    down: Vec<Target>,
    up: Vec<Target>,
    bindings: Vec<Binding>,
    pipelines: [CachedRenderPipelineId; 3],
}
impl BloomPass {
    pub fn new(
        adapter: &RenderAdapter,
        device: &RenderDevice,
        queue: &RenderQueue,
        cache: &PipelineCache,
        shader: Handle<Shader>,
        source: &TextureView,
        exposure: [&TextureView; 2],
        extent: [u32; 2],
        settings: &BloomSettings,
        previous: Option<&Self>,
    ) -> Result<Self, String> {
        if !settings.fast
            || settings.anamorphic_ratio != 0.
            || !settings.threshold.is_finite()
            || settings.threshold < 0.
            || !settings.clamp.is_finite()
            || settings.clamp <= 0.
            || !(0. ..=1.).contains(&settings.soft_knee)
        {
            return Err("Unsupported source bloom profile".into());
        }
        let support = adapter.get_texture_format_features(TextureFormat::Rgba16Float);
        if !support
            .allowed_usages
            .contains(TextureUsages::RENDER_ATTACHMENT | TextureUsages::TEXTURE_BINDING)
            || !support
                .flags
                .contains(TextureFormatFeatureFlags::FILTERABLE)
        {
            return Err("Source bloom requires sampled RGBA16F render targets".into());
        }
        let layout = BloomLayout::new(extent, settings.diffusion)?;
        let down: Vec<_> = layout
            .extents
            .iter()
            .map(|&e| Target::new(device, e))
            .collect();
        // Source allocates both handles at every level, including the unused smallest up target.
        let up: Vec<_> = layout
            .extents
            .iter()
            .map(|&e| Target::new(device, e))
            .collect();
        let bind_layout = BindGroupLayoutDescriptor::new(
            "source_bloom",
            &BindGroupLayoutEntries::sequential(
                ShaderStages::FRAGMENT,
                (
                    uniform_buffer::<Params>(false),
                    texture_2d(TextureSampleType::Float { filterable: true }),
                    texture_2d(TextureSampleType::Float { filterable: true }),
                    texture_2d(TextureSampleType::Float { filterable: false }),
                    sampler(SamplerBindingType::Filtering),
                    sampler(SamplerBindingType::NonFiltering),
                ),
            ),
        );
        let pipelines = previous.map_or_else(
            || {
                std::array::from_fn(|i| {
                    let entry = ["prefilter", "downsample", "upsample"][i];
                    cache.queue_render_pipeline(RenderPipelineDescriptor {
                        label: Some(format!("source_bloom_{entry}").into()),
                        layout: vec![bind_layout.clone()],
                        vertex: VertexState {
                            shader: shader.clone(),
                            entry_point: Some("vertex".into()),
                            ..default()
                        },
                        fragment: Some(FragmentState {
                            shader: shader.clone(),
                            entry_point: Some(entry.into()),
                            targets: vec![Some(ColorTargetState {
                                format: TextureFormat::Rgba16Float,
                                blend: None,
                                write_mask: ColorWrites::ALL,
                            })],
                            ..default()
                        }),
                        ..default()
                    })
                })
            },
            |p| p.pipelines,
        );
        let linear = device.create_sampler(&SamplerDescriptor {
            label: Some("source_bloom_linear_clamp"),
            min_filter: FilterMode::Linear,
            mag_filter: FilterMode::Linear,
            ..default()
        });
        let point = device.create_sampler(&SamplerDescriptor {
            label: Some("source_exposure_point_clamp"),
            ..default()
        });
        let threshold = gamma_to_linear(settings.threshold);
        let knee = threshold * settings.soft_knee + 9.999999747378752e-6;
        let threshold = Vec4::new(threshold, threshold - knee, knee * 2., 0.25 / knee);
        let clamp_value = gamma_to_linear(settings.clamp);
        let mut bindings = Vec::with_capacity(down.len() * 2 - 1);
        let mut bind = |i: usize, input: &TextureView, higher: &TextureView| {
            let mut uniform = UniformBuffer::from(Params {
                output_size: UVec2::from_array(layout.extents[i]).as_vec2(),
                sample_scale: layout.sample_scale,
                clamp_value,
                threshold,
            });
            uniform.write_buffer(device, queue);
            let groups = std::array::from_fn(|exposure_index| {
                device.create_bind_group(
                    "source_bloom",
                    &cache.get_bind_group_layout(&bind_layout),
                    &BindGroupEntries::sequential((
                        uniform.binding().unwrap(),
                        input,
                        higher,
                        exposure[exposure_index],
                        &linear,
                        &point,
                    )),
                )
            });
            bindings.push(Binding {
                _uniform: uniform,
                groups,
            });
        };
        for i in 0..down.len() {
            bind(i, if i == 0 { source } else { &down[i - 1].view }, source);
        }
        for i in (0..down.len() - 1).rev() {
            bind(
                i,
                if i + 2 == down.len() {
                    &down[i + 1].view
                } else {
                    &up[i + 1].view
                },
                &down[i].view,
            );
        }
        Ok(Self {
            layout,
            down,
            up,
            bindings,
            pipelines,
        })
    }
    pub fn output(&self) -> &TextureView {
        if self.down.len() == 1 {
            &self.down[0].view
        } else {
            &self.up[0].view
        }
    }
    pub fn ready(&self, cache: &PipelineCache) -> bool {
        self.pipelines
            .iter()
            .all(|&p| cache.get_render_pipeline(p).is_some())
    }
    pub fn failure(&self, cache: &PipelineCache) -> Option<String> {
        self.pipelines
            .iter()
            .find_map(|&p| match cache.get_render_pipeline_state(p) {
                CachedPipelineState::Err(
                    bevy::shader::ShaderCacheError::ShaderNotLoaded(_)
                    | bevy::shader::ShaderCacheError::ShaderImportNotYetAvailable,
                ) => None,
                CachedPipelineState::Err(e) => Some(format!("Source bloom pipeline: {e}")),
                _ => None,
            })
    }
    pub fn encode(
        &self,
        encoder: &mut CommandEncoder,
        cache: &PipelineCache,
        exposure_index: usize,
    ) -> bool {
        if exposure_index > 1 || !self.ready(cache) {
            return false;
        }
        let count = self.down.len();
        for i in 0..self.bindings.len() {
            let (target, pipeline) = if i < count {
                (&self.down[i], usize::from(i != 0))
            } else {
                (&self.up[2 * count - 2 - i], 2)
            };
            let colors = [Some(RenderPassColorAttachment {
                view: &target.view,
                depth_slice: None,
                resolve_target: None,
                ops: Operations {
                    load: LoadOp::Clear(default()),
                    store: StoreOp::Store,
                },
            })];
            let mut pass = encoder.begin_render_pass(&RenderPassDescriptor {
                label: Some("source_bloom"),
                color_attachments: &colors,
                ..default()
            });
            pass.set_pipeline(cache.get_render_pipeline(self.pipelines[pipeline]).unwrap());
            pass.set_bind_group(0, &self.bindings[i].groups[exposure_index], &[]);
            pass.draw(0..3, 0..1);
        }
        true
    }
}
