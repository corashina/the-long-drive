//! Source PPv2 motion blur: velocity, tile max, neighbor max, reconstruction. After TAA, before histogram.
//! Targets and bindings are allocated only at startup, physical resize, or a setting change.
use crate::reference::postprocessing::MotionBlurSettings;
use bevy::prelude::*;
use bevy::render::{
    render_resource::{binding_types::*, *},
    renderer::{RenderDevice, RenderQueue},
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MotionLayout {
    /// Velocity, three halvings, tile max, neighbor max.
    pub extents: [[u32; 2]; 6],
    pub radius: u32,
    pub tile_loop: u32,
}
impl MotionLayout {
    pub fn new(extent: [u32; 2]) -> Result<Self, String> {
        // Source `maxBlurPixels = (int)(5f * height / 100f)`.
        let radius = ((5.0_f32 * extent[1] as f32) / 100.0) as u32;
        // Source integer division truncates towards zero.
        let tile_size = (((radius as i32 - 1) / 8 + 1) * 8) as u32;
        let extents = [
            extent,
            extent.map(|n| n / 2),
            extent.map(|n| n / 4),
            extent.map(|n| n / 8),
            extent.map(|n| n / tile_size),
            extent.map(|n| n / tile_size),
        ];
        if radius == 0 || extents.iter().flatten().any(|&n| n == 0) {
            return Err("Source motion blur extent is too small".into());
        }
        Ok(Self {
            extents,
            radius,
            tile_loop: tile_size / 8,
        })
    }
    pub fn logical_bytes(&self) -> u64 {
        let pixels = |e: [u32; 2]| u64::from(e[0]) * u64::from(e[1]);
        self.extents.iter().map(|&e| pixels(e) * 4).sum::<u64>() + pixels(self.extents[0]) * 8
    }
}
#[derive(Clone, Copy, ShaderType)]
struct MotionParams {
    output_size: Vec2,
    screen_size: Vec2,
    settings: Vec4,
    depth: Vec4,
    control: UVec4,
}
struct Target {
    _texture: Texture,
    view: TextureView,
}
impl Target {
    fn new(device: &RenderDevice, extent: [u32; 2], format: TextureFormat) -> Self {
        let texture = device.create_texture(&TextureDescriptor {
            label: Some("source_motion_blur"),
            size: Extent3d {
                width: extent[0],
                height: extent[1],
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: TextureDimension::D2,
            format,
            usage: TextureUsages::RENDER_ATTACHMENT | TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        });
        let view = texture.create_view(&default());
        Self {
            _texture: texture,
            view,
        }
    }
}
const ENTRIES: [&str; 7] = [
    "velocity",
    "tile_decode",
    "tile_reduce",
    "tile_variable",
    "neighbor",
    "reconstruct",
    "reset_copy",
];
fn format(pipeline: usize) -> TextureFormat {
    match pipeline {
        0 => TextureFormat::Rgb10a2Unorm,
        1..5 => TextureFormat::Rg16Float,
        _ => TextureFormat::Rgba16Float,
    }
}
pub struct MotionBlurPass {
    pub layout: MotionLayout,
    targets: [Target; 6],
    output: Target,
    uniforms: [UniformBuffer<MotionParams>; 7],
    bindings: [BindGroup; 8],
    pipelines: [CachedRenderPipelineId; 7],
}
impl MotionBlurPass {
    pub fn new(
        device: &RenderDevice,
        queue: &RenderQueue,
        cache: &PipelineCache,
        shader: Handle<Shader>,
        source: &TextureView,
        depth: &TextureView,
        motion: &TextureView,
        extent: [u32; 2],
        settings: &MotionBlurSettings,
        previous: Option<Self>,
    ) -> Result<Self, String> {
        let layout = MotionLayout::new(extent)?;
        let targets =
            std::array::from_fn(|i| Target::new(device, layout.extents[i], format(i.min(4))));
        let output = Target::new(device, extent, format(5));
        let descriptor = BindGroupLayoutDescriptor::new(
            "source_motion_blur",
            &BindGroupLayoutEntries::sequential(
                ShaderStages::FRAGMENT,
                (
                    uniform_buffer::<MotionParams>(false),
                    texture_2d(TextureSampleType::Float { filterable: true }),
                    texture_2d(TextureSampleType::Float { filterable: true }),
                    texture_2d(TextureSampleType::Float { filterable: true }),
                    texture_depth_2d(),
                    texture_2d(TextureSampleType::Float { filterable: true }),
                    sampler(SamplerBindingType::NonFiltering),
                    sampler(SamplerBindingType::Filtering),
                ),
            ),
        );
        let pipelines = previous.map_or_else(
            || {
                std::array::from_fn(|i| {
                    cache.queue_render_pipeline(RenderPipelineDescriptor {
                        label: Some(format!("source_motion_{}", ENTRIES[i]).into()),
                        layout: vec![descriptor.clone()],
                        vertex: VertexState {
                            shader: shader.clone(),
                            entry_point: Some("vertex".into()),
                            ..default()
                        },
                        fragment: Some(FragmentState {
                            shader: shader.clone(),
                            entry_point: Some(ENTRIES[i].into()),
                            targets: vec![Some(ColorTargetState {
                                format: format(i),
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
        // Source `_LoopCount = Mathf.Clamp(sampleCount / 2, 1, 64)`.
        let loop_count = (settings.samples / 2).clamp(1, 64) as f32;
        // Draw i writes extent i. Draw 6, the reconstruction and the reset copy write the output.
        let uniforms = std::array::from_fn(|i| {
            let out = if i < 6 { layout.extents[i] } else { extent };
            let mut buffer = UniformBuffer::from(MotionParams {
                output_size: Vec2::new(out[0] as f32, out[1] as f32),
                screen_size: Vec2::new(extent[0] as f32, extent[1] as f32),
                settings: Vec4::new(
                    layout.radius as f32,
                    1.0 / layout.radius as f32,
                    0.,
                    loop_count,
                ),
                depth: Vec4::new(0., 1., (layout.tile_loop as f32 - 1.) * -0.5, 0.),
                control: UVec4::new(layout.tile_loop, 0, 0, 0),
            });
            buffer.write_buffer(device, queue);
            buffer
        });
        let point = device.create_sampler(&SamplerDescriptor {
            label: Some("source_motion_point_clamp"),
            ..default()
        });
        let linear = device.create_sampler(&SamplerDescriptor {
            label: Some("source_motion_linear_clamp"),
            min_filter: FilterMode::Linear,
            mag_filter: FilterMode::Linear,
            ..default()
        });
        // Bindings 0-6 are the draws. Binding 7 is the reset copy.
        let bindings = std::array::from_fn(|i| {
            let draw = i.min(6);
            device.create_bind_group(
                "source_motion_blur",
                &cache.get_bind_group_layout(&descriptor),
                &BindGroupEntries::sequential((
                    uniforms[draw].binding().unwrap(),
                    if (1..6).contains(&i) {
                        &targets[i - 1].view
                    } else {
                        source
                    },
                    if i == 6 { &targets[0].view } else { source },
                    if i == 6 { &targets[5].view } else { source },
                    depth,
                    motion,
                    &point,
                    &linear,
                )),
            )
        });
        Ok(Self {
            layout,
            targets,
            output,
            uniforms,
            bindings,
            pipelines,
        })
    }
    /// Histogram, bloom and Uber read this view.
    pub fn output(&self) -> &TextureView {
        &self.output.view
    }
    pub fn failure(&self, cache: &PipelineCache) -> Option<String> {
        super::pipeline_failure(cache, &self.pipelines)
    }
    pub fn ready(&self, cache: &PipelineCache) -> bool {
        self.pipelines
            .iter()
            .all(|id| cache.get_render_pipeline(*id).is_some())
    }
    /// `shutter` in degrees. None copies the source: no valid motion, or a screenshot frame.
    pub fn encode(
        &mut self,
        encoder: &mut CommandEncoder,
        device: &RenderDevice,
        queue: &RenderQueue,
        cache: &PipelineCache,
        near: f32,
        far: f32,
        shutter: Option<f32>,
    ) {
        // Source `IsEnabledAndSupported` requires a positive shutter angle.
        let Some(shutter) = shutter.filter(|&s| s > 0.) else {
            draw(
                encoder,
                cache.get_render_pipeline(self.pipelines[6]).unwrap(),
                &self.bindings[7],
                &self.output.view,
            );
            return;
        };
        for uniform in &mut self.uniforms {
            let value = uniform.get_mut();
            value.settings.z = shutter / 360.0;
            value.depth.x = far / near - 1.0;
            uniform.write_buffer(device, queue);
        }
        for (i, pipeline) in [0, 1, 2, 2, 3, 4, 5].into_iter().enumerate() {
            draw(
                encoder,
                cache.get_render_pipeline(self.pipelines[pipeline]).unwrap(),
                &self.bindings[i],
                self.targets.get(i).map_or(&self.output.view, |t| &t.view),
            );
        }
    }
}
fn draw(
    encoder: &mut CommandEncoder,
    pipeline: &RenderPipeline,
    binding: &BindGroup,
    output: &TextureView,
) {
    let colors = [Some(RenderPassColorAttachment {
        view: output,
        depth_slice: None,
        resolve_target: None,
        ops: Operations {
            load: LoadOp::Clear(LinearRgba::NONE.into()),
            store: StoreOp::Store,
        },
    })];
    let mut pass = encoder.begin_render_pass(&RenderPassDescriptor {
        label: Some("source_motion_blur"),
        color_attachments: &colors,
        ..default()
    });
    pass.set_pipeline(pipeline);
    pass.set_bind_group(0, binding, &[]);
    pass.draw(0..3, 0..1);
}
