//! Post stack v2 DepthOfField with the Small kernel. Allocate only at startup and physical resize.
use super::pipeline_failure;
use crate::presentation::render_targets::HDR_FORMAT;
use crate::reference::postprocessing::Lens;
use bevy::prelude::*;
use bevy::render::{
    render_resource::{binding_types::*, *},
    renderer::{RenderDevice, RenderQueue},
};

/// Height of the 35mm full-frame format.
const FILM_HEIGHT: f32 = 0.024;
/// `CalculateMaxCoCRadius` of the Small kernel: kernel size * 4 + 6 pixels.
const SMALL_KERNEL_PIXELS: f32 = 6.;
const COC_FORMAT: TextureFormat = TextureFormat::R8Unorm;
const PASSES: [&str; 6] = [
    "coc",
    "temporal",
    "prefilter",
    "blur",
    "postfilter",
    "combine",
];

#[derive(Clone, Copy, ShaderType)]
struct Params {
    depth: Vec4,
    coc: Vec4,
    taa: Vec4,
}
struct Target {
    _texture: Texture,
    view: TextureView,
}
impl Target {
    fn new(
        device: &RenderDevice,
        label: &'static str,
        extent: [u32; 2],
        format: TextureFormat,
    ) -> Self {
        let texture = device.create_texture(&TextureDescriptor {
            label: Some(label),
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
        Self {
            _texture: texture,
            view,
        }
    }
}
fn layout() -> BindGroupLayoutDescriptor {
    BindGroupLayoutDescriptor::new(
        "source_dof",
        &BindGroupLayoutEntries::sequential(
            ShaderStages::FRAGMENT,
            (
                uniform_buffer::<Params>(false),
                texture_depth_2d(),
                texture_2d(TextureSampleType::Float { filterable: true }),
                texture_2d(TextureSampleType::Float { filterable: true }),
                texture_2d(TextureSampleType::Float { filterable: true }),
                sampler(SamplerBindingType::Filtering),
                texture_2d(TextureSampleType::Float { filterable: true }),
            ),
        ),
    )
}
pub struct DofPass {
    pub logical_bytes: u64,
    extent: [u32; 2],
    uniform: UniformBuffer<Params>,
    /// The step before DoF writes here. The combine writes the chain scene.
    raw: Target,
    coc: Target,
    /// CoC history pair of the temporal filter. None without TAA.
    history: Option<[Target; 2]>,
    half: [Target; 2],
    /// TAA `motionBlending`. None without TAA.
    motion_blending: Option<f32>,
    /// History written this frame. The other one is read.
    write: usize,
    reset: bool,
    pending: bool,
    pipelines: [CachedRenderPipelineId; 6],
    coc_group: BindGroup,
    temporal_groups: Option<[BindGroup; 2]>,
    /// Indexed by the CoC input: the new CoC, then each history.
    prefilter_groups: Vec<BindGroup>,
    blur_group: BindGroup,
    postfilter_group: BindGroup,
    combine_groups: Vec<BindGroup>,
}
impl DofPass {
    /// `depth` is the resolved camera depth. `temporal` is the TAA motion target and `motionBlending`.
    pub fn new(
        device: &RenderDevice,
        queue: &RenderQueue,
        cache: &PipelineCache,
        shader: Handle<Shader>,
        depth: &TextureView,
        temporal: Option<(&TextureView, f32)>,
        extent: [u32; 2],
        previous: Option<Self>,
    ) -> Self {
        let layout = layout();
        let pipelines = previous.as_ref().map_or_else(
            || {
                std::array::from_fn(|i| {
                    let format = if i < 2 { COC_FORMAT } else { HDR_FORMAT };
                    cache.queue_render_pipeline(RenderPipelineDescriptor {
                        label: Some(format!("source_dof_{}", PASSES[i]).into()),
                        layout: vec![layout.clone()],
                        vertex: VertexState {
                            shader: shader.clone(),
                            entry_point: Some("vertex".into()),
                            ..default()
                        },
                        fragment: Some(FragmentState {
                            shader: shader.clone(),
                            entry_point: Some(PASSES[i].into()),
                            targets: vec![Some(ColorTargetState {
                                format,
                                blend: None,
                                write_mask: ColorWrites::ALL,
                            })],
                            ..default()
                        }),
                        ..default()
                    })
                })
            },
            |old| old.pipelines,
        );
        // The bokeh runs at half resolution.
        let half_extent = extent.map(|n| (n / 2).max(1));
        let raw = Target::new(device, "source_dof_raw", extent, HDR_FORMAT);
        let coc = Target::new(device, "source_dof_coc", extent, COC_FORMAT);
        let history = temporal.map(|_| {
            [0, 1].map(|_| Target::new(device, "source_dof_coc_history", extent, COC_FORMAT))
        });
        let half = [
            Target::new(device, "source_dof_half", half_extent, HDR_FORMAT),
            Target::new(device, "source_dof_temp", half_extent, HDR_FORMAT),
        ];
        let mut uniform = UniformBuffer::from(Params {
            depth: Vec4::ZERO,
            coc: Vec4::ZERO,
            taa: Vec4::ZERO,
        });
        uniform.write_buffer(device, queue);
        let sampler = device.create_sampler(&SamplerDescriptor {
            min_filter: FilterMode::Linear,
            mag_filter: FilterMode::Linear,
            ..default()
        });
        // Main, CoC, then DoF input. No pass reads its own output.
        let motion = temporal.map_or(&raw.view, |(view, _)| view);
        let group = |main: &TextureView, coc: &TextureView, dof: &TextureView| {
            device.create_bind_group(
                "source_dof",
                &cache.get_bind_group_layout(&layout),
                &BindGroupEntries::sequential((
                    uniform.binding().unwrap(),
                    depth,
                    main,
                    coc,
                    dof,
                    &sampler,
                    motion,
                )),
            )
        };
        let cocs: Vec<&TextureView> = std::iter::once(&coc.view)
            .chain(history.iter().flatten().map(|h| &h.view))
            .collect();
        let area = |e: [u32; 2]| u64::from(e[0]) * u64::from(e[1]);
        Self {
            logical_bytes: area(extent) * (9 + 2 * u64::from(history.is_some()))
                + area(half_extent) * 16,
            extent,
            coc_group: group(&raw.view, &half[0].view, &half[1].view),
            temporal_groups: history
                .as_ref()
                .map(|h| [0, 1].map(|write| group(&h[1 - write].view, &coc.view, &half[0].view))),
            prefilter_groups: cocs
                .iter()
                .map(|c| group(&raw.view, c, &half[1].view))
                .collect(),
            blur_group: group(&half[0].view, &coc.view, &half[0].view),
            postfilter_group: group(&half[1].view, &coc.view, &raw.view),
            combine_groups: cocs
                .iter()
                .map(|c| group(&raw.view, c, &half[0].view))
                .collect(),
            uniform,
            raw,
            coc,
            history,
            half,
            motion_blending: temporal.map(|(_, blending)| blending),
            write: 0,
            reset: true,
            pending: false,
            pipelines,
        }
    }
    pub fn raw(&self) -> &TextureView {
        &self.raw.view
    }
    pub fn ready(&self, cache: &PipelineCache) -> bool {
        self.pipelines
            .iter()
            .all(|id| cache.get_render_pipeline(*id).is_some())
    }
    pub fn failure(&self, cache: &PipelineCache) -> Option<String> {
        pipeline_failure(cache, &self.pipelines)
    }
    pub fn reset_history(&mut self) {
        self.reset = true;
        self.pending = false;
    }
    /// The submitted frame wrote its CoC history.
    pub fn completed(&mut self) {
        if std::mem::take(&mut self.pending) {
            self.reset = false;
            self.write = 1 - self.write;
        }
    }
    /// `DepthOfFieldRenderer.Render`. `focus` is in meters. `motion` is the TAA jitter and reset.
    /// The combine writes `output`.
    pub fn encode(
        &mut self,
        encoder: &mut CommandEncoder,
        device: &RenderDevice,
        queue: &RenderQueue,
        cache: &PipelineCache,
        lens: Lens,
        focus: f32,
        near: f32,
        far: f32,
        motion: Option<(Vec2, bool)>,
        output: &TextureView,
    ) -> Result<(), String> {
        if !focus.is_finite() || !near.is_finite() || !far.is_finite() || near <= 0. || far <= near
        {
            return Err("Invalid source depth of field frame".into());
        }
        let [width, height] = self.extent.map(|n| n as f32);
        let f = lens.focal_length / 1000.;
        let s1 = focus.max(f);
        let film_height = FILM_HEIGHT * (height / 1080.);
        let coeff = f * f / (lens.aperture * (s1 - f) * film_height * 2.);
        let max_coc = (SMALL_KERNEL_PIXELS / height).min(0.05);
        let params = self.uniform.get_mut();
        params.depth = Vec4::new(far / near - 1., far, s1, coeff);
        params.coc = Vec4::new(max_coc, 1. / max_coc, height / width, 0.);
        // The CoC temporal filter runs while TAA is active.
        let temporal = match (self.history.as_ref(), self.motion_blending, motion) {
            (Some(history), Some(blending), Some((jitter, reset))) => {
                self.reset |= reset;
                let blend = if self.reset { 0. } else { blending };
                params.taa = Vec4::new(jitter.x, jitter.y, blend, 0.);
                Some(&history[self.write].view)
            }
            (None, None, None) => None,
            _ => return Err("Source depth of field TAA state differs from the view".into()),
        };
        self.uniform.write_buffer(device, queue);
        // CoC input of the later passes: the new CoC, or the history written this frame.
        let coc = temporal.map_or(0, |_| 1 + self.write);
        let temporal = temporal.zip(self.temporal_groups.as_ref());
        let passes = [
            Some((&self.coc_group, &self.coc.view)),
            temporal.map(|(view, groups)| (&groups[self.write], view)),
            Some((&self.prefilter_groups[coc], &self.half[0].view)),
            Some((&self.blur_group, &self.half[1].view)),
            Some((&self.postfilter_group, &self.half[0].view)),
            Some((&self.combine_groups[coc], output)),
        ];
        for (i, pass) in passes.into_iter().enumerate() {
            let Some((group, target)) = pass else {
                continue;
            };
            let pipeline = cache
                .get_render_pipeline(self.pipelines[i])
                .ok_or("Source depth of field pipeline is not ready")?;
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
                label: Some("source_dof"),
                color_attachments: &attachments,
                ..default()
            });
            pass.set_pipeline(pipeline);
            pass.set_bind_group(0, group, &[]);
            pass.draw(0..3, 0..1);
        }
        self.pending = temporal.is_some();
        Ok(())
    }
}
