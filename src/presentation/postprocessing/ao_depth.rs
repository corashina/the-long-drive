//! Source AO depth storage. Allocate once per physical extent and scale count.
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
    uniforms: Vec<UniformBuffer<DepthParams>>,
}
impl AoDepthPass {
    pub fn new(
        adapter: &RenderAdapter,
        device: &RenderDevice,
        queue: &RenderQueue,
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
                    cache.queue_render_pipeline(RenderPipelineDescriptor {
                        label: Some(format!("source_ao_{entry}_{index}").into()),
                        layout: vec![if index == 0 {
                            copy_layout.clone()
                        } else {
                            layout.clone()
                        }],
                        vertex: VertexState {
                            shader: shader.clone(),
                            entry_point: Some("vertex".into()),
                            ..default()
                        },
                        fragment: Some(FragmentState {
                            shader: shader.clone(),
                            entry_point: Some(entry.into()),
                            targets: formats
                                .iter()
                                .map(|&format| {
                                    Some(ColorTargetState {
                                        format,
                                        blend: None,
                                        write_mask: ColorWrites::ALL,
                                    })
                                })
                                .collect(),
                            ..default()
                        }),
                        ..default()
                    })
                })
            },
            |pass| pass.pipelines,
        );
        // Scales 0 and 1 read the raw copy and linearize. Scales 2 and 3 read the linear scale 1.
        let uniforms: Vec<_> = (0..scales)
            .map(|scale| {
                let stride = if scale % 2 == 0 { 2 } else { 4 };
                let mut buffer = UniformBuffer::from(DepthParams {
                    projection: Vec4::ZERO,
                    control: UVec4::new(stride, u32::from(scale < 2), 0, 0),
                });
                buffer.write_buffer(device, queue);
                buffer
            })
            .collect();
        let mut bindings: Vec<_> = (0..scales)
            .map(|scale| {
                device.create_bind_group(
                    "source_ao_depth",
                    &cache.get_bind_group_layout(&layout),
                    &BindGroupEntries::sequential((
                        uniforms[scale].binding().unwrap(),
                        if scale < 2 { &copy.view } else { &low[1].view },
                        camera_depth,
                    )),
                )
            })
            .collect();
        bindings.push(device.create_bind_group(
            "source_ao_depth_copy",
            &cache.get_bind_group_layout(&copy_layout),
            &BindGroupEntries::with_indices((
                (0, uniforms[0].binding().unwrap()),
                (2, camera_depth),
            )),
        ));
        Ok(Self {
            copy,
            linear,
            low,
            tiled,
            logical_bytes,
            pipelines,
            bindings,
            uniforms,
        })
    }
    pub fn ready(&self, cache: &PipelineCache) -> bool {
        self.pipelines
            .iter()
            .all(|&id| cache.get_render_pipeline(id).is_some())
    }
    pub fn failure(&self, cache: &PipelineCache) -> Option<String> {
        super::named_pipeline_failure(cache, &self.pipelines, "Source AO depth pipeline")
    }
    pub fn encode(
        &mut self,
        encoder: &mut CommandEncoder,
        device: &RenderDevice,
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
        for buffer in &mut self.uniforms {
            buffer.get_mut().projection = Vec4::new(far / near - 1., 1., 0., 0.);
            buffer.write_buffer(device, queue);
        }
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
        views: &[&TextureView],
    ) {
        let attachments: [Option<RenderPassColorAttachment>; 4] = std::array::from_fn(|index| {
            views.get(index).map(|&view| RenderPassColorAttachment {
                view,
                depth_slice: None,
                resolve_target: None,
                ops: Operations {
                    load: LoadOp::Clear(Default::default()),
                    store: StoreOp::Store,
                },
            })
        });
        let mut pass = encoder.begin_render_pass(&RenderPassDescriptor {
            label: Some("source_ao_depth"),
            color_attachments: &attachments[..views.len()],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
        pass.set_pipeline(cache.get_render_pipeline(self.pipelines[pipeline]).unwrap());
        pass.set_bind_group(0, &self.bindings[binding], &[]);
        pass.draw(0..3, 0..1);
    }
}
