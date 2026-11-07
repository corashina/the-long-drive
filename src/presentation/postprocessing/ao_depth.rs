//! Source AO depth storage. Allocate once per physical extent and scale count.
use crate::presentation::fullscreen::{self, FullscreenPass};
use crate::presentation::uniform_slots::UniformSlots;
use bevy::prelude::*;
use bevy::render::{
    render_resource::{binding_types::*, *},
    renderer::{RenderAdapter, RenderDevice, RenderQueue},
};

#[derive(Clone, Copy, ShaderType)]
struct DepthParams {
    projection: Vec4,
    control: UVec4,
}

pub struct AoDepthTarget {
    _texture: Texture,
    pub view: TextureView,
    pub extent: [u32; 2],
    layers: Option<[TextureView; 4]>,
}
impl AoDepthTarget {
    pub(super) fn new(
        device: &RenderDevice,
        extent: [u32; 2],
        format: TextureFormat,
        array: bool,
    ) -> Self {
        let texture = device.create_texture(&TextureDescriptor {
            label: Some("source_ao_depth"),
            size: Extent3d {
                width: extent[0],
                height: extent[1],
                depth_or_array_layers: if array { 4 } else { 1 },
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: TextureDimension::D2,
            format,
            usage: TextureUsages::TEXTURE_BINDING | TextureUsages::RENDER_ATTACHMENT,
            view_formats: &[],
        });
        let view = texture.create_view(&TextureViewDescriptor {
            dimension: Some(if array {
                TextureViewDimension::D2Array
            } else {
                TextureViewDimension::D2
            }),
            ..default()
        });
        let layers = array.then(|| {
            std::array::from_fn(|index| {
                texture.create_view(&TextureViewDescriptor {
                    dimension: Some(TextureViewDimension::D2),
                    base_array_layer: index as u32,
                    array_layer_count: Some(1),
                    ..default()
                })
            })
        });
        Self {
            _texture: texture,
            view,
            extent,
            layers,
        }
    }
}

pub struct AoDepthPass {
    pub copy: AoDepthTarget,
    pub linear: AoDepthTarget,
    /// One target per scale, finest first.
    pub low: Vec<AoDepthTarget>,
    pub tiled: Vec<AoDepthTarget>,
    pub logical_bytes: u64,
    /// The copy (raw and linear depth in one pass), the downsample and the tile pipelines.
    pipelines: [CachedRenderPipelineId; 3],
    /// One binding per scale, then the copy binding.
    bindings: Vec<BindGroup>,
    params: Vec<DepthParams>,
    /// One slot per scale. One write per frame.
    uniforms: UniformSlots<DepthParams>,
}
impl AoDepthPass {
    pub fn new(
        adapter: &RenderAdapter,
        device: &RenderDevice,
        cache: &PipelineCache,
        shader: Handle<Shader>,
        camera_depth: &TextureView,
        extent: [u32; 2],
        scales: usize,
        previous: Option<&Self>,
    ) -> Result<Self, String> {
        if !(1..=4).contains(&scales) {
            return Err("Source AO requires 1 to 4 scales".into());
        }
        if extent.iter().any(|&x| x == 0 || x > 16384) {
            return Err("Source AO requires a positive physical extent at most 16384".into());
        }
        for format in [
            TextureFormat::R32Float,
            TextureFormat::R16Float,
            TextureFormat::Rgba16Float,
        ] {
            if !adapter
                .get_texture_format_features(format)
                .allowed_usages
                .contains(TextureUsages::RENDER_ATTACHMENT | TextureUsages::TEXTURE_BINDING)
            {
                return Err(format!(
                    "Source AO requires sampled render attachment {format:?}"
                ));
            }
        }
        if device.limits().max_color_attachments < 4
            || device.limits().max_color_attachment_bytes_per_sample < 32
        {
            return Err("Source AO requires four RGBA16F color attachments".into());
        }
        let dimensions: [[u32; 2]; 7] =
            std::array::from_fn(|level| extent.map(|n| n.div_ceil(1 << level)));
        let copy = AoDepthTarget::new(device, extent, TextureFormat::R32Float, false);
        let linear = AoDepthTarget::new(device, extent, TextureFormat::R16Float, false);
        let low: Vec<_> = (0..scales)
            .map(|i| AoDepthTarget::new(device, dimensions[i + 1], TextureFormat::R32Float, false))
            .collect();
        let tiled: Vec<_> = (0..scales)
            .map(|i| {
                AoDepthTarget::new(device, dimensions[i + 3], TextureFormat::Rgba16Float, true)
            })
            .collect();
        let area = |e: [u32; 2]| u64::from(e[0]) * u64::from(e[1]);
        let logical_bytes = area(extent) * 6
            + low.iter().map(|t| area(t.extent) * 4).sum::<u64>()
            + tiled.iter().map(|t| area(t.extent) * 32).sum::<u64>();
        let layout = BindGroupLayoutDescriptor::new(
            "source_ao_depth",
            &BindGroupLayoutEntries::sequential(
                ShaderStages::FRAGMENT,
                (
                    uniform_buffer::<DepthParams>(false),
                    texture_2d(TextureSampleType::Float { filterable: false }),
                    texture_2d(TextureSampleType::Float { filterable: false }),
                ),
            ),
        );
        let copy_layout = BindGroupLayoutDescriptor::new(
            "source_ao_depth_copy",
            &BindGroupLayoutEntries::with_indices(
                ShaderStages::FRAGMENT,
                (
                    (0, uniform_buffer::<DepthParams>(false)),
                    (
                        2,
                        texture_2d(TextureSampleType::Float { filterable: false }),
                    ),
                ),
            ),
        );
        // Pipeline descriptors do not depend on the physical extent.
        let pipelines = previous.map_or_else(
            || {
                std::array::from_fn(|index| {
                    let (entry, formats): (_, &[_]) = match index {
                        0 => (
                            "copy_depth",
                            &[TextureFormat::R32Float, TextureFormat::R16Float],
                        ),
                        1 => ("downsample", &[TextureFormat::R32Float]),
                        _ => ("tile_depth", &[TextureFormat::Rgba16Float; 4]),
                    };
                    cache.queue_render_pipeline(fullscreen::descriptor(
                        format!("source_ao_{entry}_{index}"),
                        if index == 0 { &copy_layout } else { &layout },
                        &shader,
                        entry,
                        formats
                            .iter()
                            .map(|&format| fullscreen::target(format, ColorWrites::ALL))
                            .collect(),
                    ))
                })
            },
            |pass| pass.pipelines,
        );
        // Scales 0 and 1 read the raw copy and linearize. Scales 2 and 3 read the linear scale 1.
        let params: Vec<_> = (0..scales)
            .map(|scale| {
                let stride = if scale % 2 == 0 { 2 } else { 4 };
                DepthParams {
                    projection: Vec4::ZERO,
                    control: UVec4::new(stride, u32::from(scale < 2), 0, 0),
                }
            })
            .collect();
        let mut uniforms = UniformSlots::new(device, "source_ao_depth", scales);
        for (scale, value) in params.iter().enumerate() {
            uniforms.set(scale, value);
        }
        let mut bindings: Vec<_> = (0..scales)
            .map(|scale| {
                device.create_bind_group(
                    "source_ao_depth",
                    &cache.get_bind_group_layout(&layout),
                    &BindGroupEntries::sequential((
                        uniforms.binding(scale),
                        if scale < 2 { &copy.view } else { &low[1].view },
                        camera_depth,
                    )),
                )
            })
            .collect();
        bindings.push(device.create_bind_group(
            "source_ao_depth_copy",
            &cache.get_bind_group_layout(&copy_layout),
            &BindGroupEntries::with_indices(((0, uniforms.binding(0)), (2, camera_depth))),
        ));
        Ok(Self {
            copy,
            linear,
            low,
            tiled,
            logical_bytes,
            pipelines,
            bindings,
            params,
            uniforms,
        })
    }
    pub fn encode(
        &mut self,
        encoder: &mut CommandEncoder,
        queue: &RenderQueue,
        cache: &PipelineCache,
        near: f32,
        far: f32,
    ) -> Result<bool, String> {
        if !near.is_finite() || !far.is_finite() || near <= 0. || far <= near {
            return Err("Invalid source AO projection range".into());
        }
        if !self.ready(cache) {
            return Ok(false);
        }
        for (scale, value) in self.params.iter_mut().enumerate() {
            value.projection = Vec4::new(far / near - 1., 1., 0., 0.);
            self.uniforms.set(scale, value);
        }
        self.uniforms.upload(queue);
        let scales = self.low.len();
        self.draw(
            encoder,
            cache,
            0,
            scales,
            &[&self.copy.view, &self.linear.view],
        );
        for scale in 0..scales {
            self.draw(encoder, cache, 1, scale, &[&self.low[scale].view]);
            let layers = self.tiled[scale].layers.as_ref().unwrap();
            self.draw(
                encoder,
                cache,
                2,
                scale,
                &[&layers[0], &layers[1], &layers[2], &layers[3]],
            );
        }
        Ok(true)
    }
    fn draw(
        &self,
        encoder: &mut CommandEncoder,
        cache: &PipelineCache,
        pipeline: usize,
        binding: usize,
        views: &[&WgpuTextureView],
    ) {
        fullscreen::draw(
            encoder,
            "source_ao_depth",
            cache.get_render_pipeline(self.pipelines[pipeline]).unwrap(),
            &self.bindings[binding],
            views,
            fullscreen::CLEAR,
        );
    }
}
impl FullscreenPass for AoDepthPass {
    const NAME: &'static str = "Source AO depth pipeline";
    fn pipeline_ids(&self) -> &[CachedRenderPipelineId] {
        &self.pipelines
    }
}
