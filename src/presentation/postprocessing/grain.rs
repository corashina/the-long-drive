//! Post stack v2 `GrainRenderer`: the colored grain lookup and its Halton offset for each grain frame.
use crate::presentation::fullscreen::{self, FullscreenPass};
use bevy::prelude::*;
use bevy::render::{
    render_resource::{binding_types::*, *},
    renderer::{RenderDevice, RenderQueue},
};
#[derive(Clone, Copy, ShaderType)]
struct Params {
    noise: Vec4,
}
/// `m_GrainLookupRT` edge.
pub const GRAIN_SIZE: u32 = 128;
/// The ARGBHalf lookup.
pub const GRAIN_BYTES: u64 = GRAIN_SIZE as u64 * GRAIN_SIZE as u64 * 8;
pub struct GrainPass {
    _texture: Texture,
    view: TextureView,
    uniform: UniformBuffer<Params>,
    binding: BindGroup,
    pipeline: CachedRenderPipelineId,
    /// `m_SampleIndex`, and the next index of the frame in flight.
    sample: u32,
    pending: Option<u32>,
}
impl GrainPass {
    pub fn new(
        device: &RenderDevice,
        queue: &RenderQueue,
        cache: &PipelineCache,
        shader: Handle<Shader>,
    ) -> Self {
        let texture = device.create_texture(&TextureDescriptor {
            label: Some("source_grain_lookup"),
            size: Extent3d {
                width: GRAIN_SIZE,
                height: GRAIN_SIZE,
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
        let layout = BindGroupLayoutDescriptor::new(
            "source_grain",
            &BindGroupLayoutEntries::single(
                ShaderStages::FRAGMENT,
                uniform_buffer::<Params>(false),
            ),
        );
        let pipeline = fullscreen::queue(
            cache,
            "source_grain",
            &layout,
            &shader,
            "grain",
            TextureFormat::Rgba16Float,
        );
        // `GrainNoiseParameters` are fixed. Each frame writes the phase.
        let mut uniform = UniformBuffer::from(Params {
            noise: Vec4::new(0., 12.9898, 78.233, 43758.547),
        });
        uniform.write_buffer(device, queue);
        let binding = device.create_bind_group(
            "source_grain",
            &cache.get_bind_group_layout(&layout),
            &BindGroupEntries::single(uniform.binding().unwrap()),
        );
        Self {
            _texture: texture,
            view,
            uniform,
            binding,
            pipeline,
            sample: 0,
            pending: None,
        }
    }
    pub fn output(&self) -> &TextureView {
        &self.view
    }
    pub fn completed(&mut self) {
        if let Some(sample) = self.pending.take() {
            self.sample = sample;
        }
    }
    /// Draws the lookup at `time` seconds. Returns `Grain_Params2.zw`, or None without a ready pipeline.
    pub fn encode(
        &mut self,
        encoder: &mut CommandEncoder,
        device: &RenderDevice,
        queue: &RenderQueue,
        cache: &PipelineCache,
        time: f32,
    ) -> Option<[f32; 2]> {
        let pipeline = cache.get_render_pipeline(self.pipeline)?;
        let index = self.sample & 0x3ff;
        let offset = [halton(index, 2), halton(index, 3)];
        self.pending = Some(if self.sample + 1 >= 1024 {
            0
        } else {
            self.sample + 1
        });
        self.uniform.get_mut().noise.x = time % 10.;
        self.uniform.write_buffer(device, queue);
        fullscreen::draw(
            encoder,
            "source_grain",
            pipeline,
            &self.binding,
            &[&self.view],
            fullscreen::CLEAR,
        );
        Some(offset)
    }
}
impl FullscreenPass for GrainPass {
    fn pipeline_ids(&self) -> &[CachedRenderPipelineId] {
        std::slice::from_ref(&self.pipeline)
    }
}
/// `HaltonSeq.Get`.
fn halton(mut index: u32, radix: u32) -> f32 {
    let mut result = 0.;
    let mut fraction = 1. / radix as f32;
    while index > 0 {
        result += (index % radix) as f32 * fraction;
        index /= radix;
        fraction /= radix as f32;
    }
    result
}
