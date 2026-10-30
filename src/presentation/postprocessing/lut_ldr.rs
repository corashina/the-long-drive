//! Installed Lut2DBaker LutGenLDR: the LDR grading over a user LUT strip, baked into the 1024x32 ARGB32 strip.
//! `postprocesscontrol` ILUT 1 and 2 select `LutC64` and `LutSepia`. Bake on change into persistent storage.
use super::lut::{color_balance, trackball};
use crate::reference::postprocessing::GradingSettings;
use bevy::prelude::*;
use bevy::render::{
    render_resource::{binding_types::*, *},
    renderer::{RenderDevice, RenderQueue},
};
/// PPv2 `k_Lut2DSize`.
const SIZE: u32 = 32;
pub const STRIP_BYTES: u64 = (SIZE * SIZE * SIZE * 4) as u64;
pub const LDR_CURVE_BYTES: u64 = 128 * 2 * 8;
/// `_Lut2D_Params` of the Uber LDR variant: 1/width, 1/height, height - 1.
pub const STRIP_PARAMS: Vec4 = Vec4::new(
    1. / (SIZE * SIZE) as f32,
    1. / SIZE as f32,
    (SIZE - 1) as f32,
    0.,
);
#[derive(Clone, Copy, ShaderType)]
struct Params {
    lut: Vec4,
    user: Vec4,
    balance: Vec4,
    color_filter: Vec4,
    hue_sat_con_brightness: Vec4,
    mixer_r: Vec4,
    mixer_g: Vec4,
    mixer_b: Vec4,
    lift: Vec4,
    inverse_gamma: Vec4,
    gain: Vec4,
}
impl Params {
    /// `ColorGrading.RenderLDRPipeline2D`. Trackballs have no HDR scale here.
    /// The main grading brightness and LUT contribution keep their defaults 0 and 1.
    fn new(s: &GradingSettings, user_extent: [u32; 2]) -> Self {
        let extend = |a: [f32; 3]| Vec3::from_array(a).extend(0.);
        let size = SIZE as f32;
        Self {
            lut: Vec4::new(size, 0.5 / (size * size), 0.5 / size, size / (size - 1.)),
            user: Vec4::new(
                1. / user_extent[0] as f32,
                1. / user_extent[1] as f32,
                user_extent[1] as f32 - 1.,
                1.,
            ),
            balance: extend(color_balance(s.temperature, s.tint)),
            color_filter: Vec4::from_array(s.color_filter),
            hue_sat_con_brightness: Vec4::new(
                s.hue_shift / 360.,
                s.saturation / 100. + 1.,
                s.contrast / 100. + 1.,
                1.,
            ),
            mixer_r: extend(s.channel_mixer[0].map(|x| x * (1. / 100.))),
            mixer_g: extend(s.channel_mixer[1].map(|x| x * (1. / 100.))),
            mixer_b: extend(s.channel_mixer[2].map(|x| x * (1. / 100.))),
            lift: extend(trackball(s.trackballs[0], 1., 0)),
            inverse_gamma: extend(trackball(s.trackballs[1], 1., 1)),
            gain: extend(trackball(s.trackballs[2], 1., 2)),
        }
    }
}
/// Half-float bits of `i / 128`. Every value is exact.
fn eighth_half(i: u32) -> u16 {
    if i == 0 {
        return 0;
    }
    let top = 31 - i.leading_zeros();
    (((top + 8) << 10) | ((i << (10 - top)) & 0x3ff)) as u16
}
pub struct LutLdrPass {
    _texture: Texture,
    view: TextureView,
    _curves: Texture,
    user_extent: [u32; 2],
    uniform: UniformBuffer<Params>,
    /// One bind group for each user LUT.
    groups: [BindGroup; 2],
    pipeline: CachedRenderPipelineId,
}
impl LutLdrPass {
    /// `luts` are the user strips with their extents, in ILUT order from 1.
    pub fn new(
        device: &RenderDevice,
        queue: &RenderQueue,
        cache: &PipelineCache,
        shader: Handle<Shader>,
        luts: [(&TextureView, [u32; 2]); 2],
        settings: &GradingSettings,
    ) -> Result<Self, String> {
        let user_extent = luts[0].1;
        if luts
            .iter()
            .any(|(_, e)| *e != user_extent || e[1] == 0 || e[0] != e[1] * e[1])
        {
            return Err("Source LDR LUTs must be equal strips".into());
        }
        let texture = device.create_texture(&TextureDescriptor {
            label: Some("source_ldr_lut"),
            size: Extent3d {
                width: SIZE * SIZE,
                height: SIZE,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: TextureDimension::D2,
            format: TextureFormat::Rgba8Unorm,
            usage: TextureUsages::RENDER_ATTACHMENT | TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        });
        let view = texture.create_view(&default());
        let curves = device.create_texture(&TextureDescriptor {
            label: Some("source_ldr_grading_curves"),
            size: Extent3d {
                width: 128,
                height: 2,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: TextureDimension::D2,
            format: TextureFormat::Rgba16Float,
            usage: TextureUsages::COPY_DST | TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        });
        // Empty secondary splines return m_ZeroValue=.5. LDR fills the second row with the
        // default identity red, green, blue and master splines: cachedData[i] = i * k_Step.
        let mut pixels = [0u8; LDR_CURVE_BYTES as usize];
        for value in pixels[..1024].chunks_exact_mut(2) {
            value.copy_from_slice(&0x3800u16.to_le_bytes());
        }
        for (i, texel) in pixels[1024..].chunks_exact_mut(8).enumerate() {
            let bits = eighth_half(i as u32).to_le_bytes();
            for channel in texel.chunks_exact_mut(2) {
                channel.copy_from_slice(&bits);
            }
        }
        queue.write_texture(
            TexelCopyTextureInfo {
                texture: &curves,
                mip_level: 0,
                origin: Origin3d::ZERO,
                aspect: TextureAspect::All,
            },
            &pixels,
            TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(1024),
                rows_per_image: Some(2),
            },
            Extent3d {
                width: 128,
                height: 2,
                depth_or_array_layers: 1,
            },
        );
        let curve_view = curves.create_view(&default());
        let linear_clamp = device.create_sampler(&SamplerDescriptor {
            label: Some("source_ldr_lut_linear_clamp"),
            min_filter: FilterMode::Linear,
            mag_filter: FilterMode::Linear,
            ..default()
        });
        let layout = BindGroupLayoutDescriptor::new(
            "source_ldr_lut",
            &BindGroupLayoutEntries::sequential(
                ShaderStages::FRAGMENT,
                (
                    uniform_buffer::<Params>(false),
                    texture_2d(TextureSampleType::Float { filterable: true }),
                    sampler(SamplerBindingType::Filtering),
                    texture_2d(TextureSampleType::Float { filterable: true }),
                    sampler(SamplerBindingType::Filtering),
                ),
            ),
        );
        let pipeline = cache.queue_render_pipeline(RenderPipelineDescriptor {
            label: Some("source_ldr_lut".into()),
            layout: vec![layout.clone()],
            vertex: VertexState {
                shader: shader.clone(),
                entry_point: Some("vertex".into()),
                ..default()
            },
            fragment: Some(FragmentState {
                shader,
                entry_point: Some("bake".into()),
                targets: vec![Some(ColorTargetState {
                    format: TextureFormat::Rgba8Unorm,
                    blend: None,
                    write_mask: ColorWrites::ALL,
                })],
                ..default()
            }),
            ..default()
        });
        let mut uniform = UniformBuffer::from(Params::new(settings, user_extent));
        uniform.write_buffer(device, queue);
        let groups = luts.map(|(lut, _)| {
            device.create_bind_group(
                "source_ldr_lut",
                &cache.get_bind_group_layout(&layout),
                &BindGroupEntries::sequential((
                    uniform.binding().unwrap(),
                    lut,
                    &linear_clamp,
                    &curve_view,
                    &linear_clamp,
                )),
            )
        });
        Ok(Self {
            _texture: texture,
            view,
            _curves: curves,
            user_extent,
            uniform,
            groups,
            pipeline,
        })
    }
    /// Replace the grading. Bake again after this.
    pub fn set_grading(
        &mut self,
        device: &RenderDevice,
        queue: &RenderQueue,
        settings: &GradingSettings,
    ) {
        *self.uniform.get_mut() = Params::new(settings, self.user_extent);
        self.uniform.write_buffer(device, queue);
    }
    pub fn output(&self) -> &TextureView {
        &self.view
    }
    pub fn ready(&self, cache: &PipelineCache) -> bool {
        cache.get_render_pipeline(self.pipeline).is_some()
    }
    pub fn failure(&self, cache: &PipelineCache) -> Option<String> {
        super::pipeline_failure(cache, &[self.pipeline])
    }
    /// `lut` is ILUT - 1.
    pub fn encode(&self, encoder: &mut CommandEncoder, cache: &PipelineCache, lut: usize) -> bool {
        let Some(pipeline) = cache.get_render_pipeline(self.pipeline) else {
            return false;
        };
        let colors = [Some(RenderPassColorAttachment {
            view: &self.view,
            depth_slice: None,
            resolve_target: None,
            ops: Operations {
                load: LoadOp::Clear(default()),
                store: StoreOp::Store,
            },
        })];
        let mut pass = encoder.begin_render_pass(&RenderPassDescriptor {
            label: Some("source_ldr_lut"),
            color_attachments: &colors,
            ..default()
        });
        pass.set_pipeline(pipeline);
        pass.set_bind_group(0, &self.groups[lut], &[]);
        pass.draw(0..3, 0..1);
        true
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn eighth_half_is_exact() {
        for i in 0..128u32 {
            let bits = super::eighth_half(i);
            let exponent = (bits >> 10) as i32 - 15;
            let value = if bits == 0 {
                0.
            } else {
                (1. + f32::from(bits & 0x3ff) / 1024.) * 2f32.powi(exponent)
            };
            assert_eq!(value, i as f32 / 128.);
        }
    }
}
