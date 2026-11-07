//! Installed fast bloom pyramid, with persistent targets and source half stores.
use crate::presentation::fullscreen::{self, FullscreenPass, Samplers};
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
        // Mathf.Log(f,2) converts both inputs to double. Its result then converts to float.
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
// Installed GammaToLinearSpace65e80. Keep the >1 branch. It differs from the sRGB middle branch.
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
/// `BloomRenderer.Render` `_Threshold`: threshold, threshold - knee, knee * 2, 0.25 / knee.
fn threshold_params(threshold: f32, soft_knee: f32) -> Vec4 {
    let threshold = gamma_to_linear(threshold);
    let knee = threshold * soft_knee + 9.999999747378752e-6;
    Vec4::new(threshold, threshold - knee, knee * 2., 0.25 / knee)
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
    uniform: UniformBuffer<Params>,
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
        samplers: &Samplers,
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
        // The source allocates both handles at every level, including the unused smallest up target.
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
                    fullscreen::queue(
                        cache,
                        format!("source_bloom_{entry}"),
                        &bind_layout,
                        &shader,
                        entry,
                        TextureFormat::Rgba16Float,
                    )
                })
            },
            |p| p.pipelines,
        );
        let threshold = threshold_params(settings.threshold, settings.soft_knee);
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
                        &samplers.linear,
                        &samplers.point,
                    )),
                )
            });
            bindings.push(Binding { uniform, groups });
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
    /// Writes the prefilter threshold and knee of a blended volume.
    pub fn set_threshold(
        &mut self,
        device: &RenderDevice,
        queue: &RenderQueue,
        threshold: f32,
        soft_knee: f32,
    ) {
        let prefilter = &mut self.bindings[0].uniform;
        let params = threshold_params(threshold, soft_knee);
        if prefilter.get().threshold != params {
            prefilter.get_mut().threshold = params;
            prefilter.write_buffer(device, queue);
        }
    }
    pub fn output(&self) -> &TextureView {
        if self.down.len() == 1 {
            &self.down[0].view
        } else {
            &self.up[0].view
        }
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
            fullscreen::draw(
                encoder,
                "source_bloom",
                cache.get_render_pipeline(self.pipelines[pipeline]).unwrap(),
                &self.bindings[i].groups[exposure_index],
                &[&target.view],
                fullscreen::CLEAR,
            );
        }
        true
    }
}
impl FullscreenPass for BloomPass {
    const NAME: &'static str = "Source bloom pipeline";
    fn pipeline_ids(&self) -> &[CachedRenderPipelineId] {
        &self.pipelines
    }
}
