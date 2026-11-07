//! Source exposure history uses two persistent R32Float targets.
use crate::presentation::fullscreen::{self, FullscreenPass};
use crate::presentation::render_targets::EXPOSURE_FORMAT;
use crate::reference::postprocessing::ExposureSettings;
use bevy::prelude::*;
use bevy::render::{
    render_resource::{binding_types::*, *},
    renderer::{RenderDevice, RenderQueue},
};
#[derive(Clone, Copy, ShaderType)]
struct ExposureParams {
    filtering: Vec4,
    adaptation: Vec4,
    control: UVec4,
}
pub struct ExposurePass {
    _textures: [Texture; 2],
    views: [TextureView; 2],
    bindings: [BindGroup; 2],
    uniform: UniformBuffer<ExposureParams>,
    pipeline: CachedRenderPipelineId,
    settings: ExposureSettings,
    output_index: usize,
    initialized: bool,
    pending: Option<usize>,
}
impl ExposurePass {
    pub fn new(
        device: &RenderDevice,
        queue: &RenderQueue,
        cache: &PipelineCache,
        shader: Handle<Shader>,
        histogram: &TextureView,
        settings: ExposureSettings,
    ) -> Result<Self, String> {
        if !settings.validate() {
            return Err("Invalid source exposure settings".into());
        }
        let layout = exposure_layout();
        let pipeline = cache.queue_render_pipeline(fullscreen::descriptor(
            "source_exposure",
            &layout,
            &shader,
            "exposure",
            vec![fullscreen::target(EXPOSURE_FORMAT, ColorWrites::RED)],
        ));
        let textures = std::array::from_fn(|_| {
            device.create_texture(&TextureDescriptor {
                label: Some("source_exposure_history"),
                size: Extent3d {
                    width: 1,
                    height: 1,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: TextureDimension::D2,
                format: EXPOSURE_FORMAT,
                usage: TextureUsages::TEXTURE_BINDING | TextureUsages::RENDER_ATTACHMENT,
                view_formats: &[],
            })
        });
        let views = std::array::from_fn(|index| textures[index].create_view(&default()));
        let mut uniform = UniformBuffer::from(ExposureParams {
            filtering: Vec4::new(
                settings.filtering_percent[0] * 0.01,
                settings.filtering_percent[1] * 0.01,
                settings.luminance_ev[0].exp2(),
                settings.luminance_ev[1].exp2(),
            ),
            adaptation: Vec4::new(settings.speed_down, settings.speed_up, settings.key, 0.),
            control: UVec4::new(1, 0, 0, 0),
        });
        uniform.write_buffer(device, queue);
        let bindings = std::array::from_fn(|index| {
            device.create_bind_group(
                "source_exposure",
                &cache.get_bind_group_layout(&layout),
                &BindGroupEntries::sequential((
                    uniform.binding().unwrap(),
                    histogram,
                    &views[1 - index],
                )),
            )
        });
        Ok(Self {
            _textures: textures,
            views,
            bindings,
            uniform,
            pipeline,
            settings,
            output_index: 0,
            initialized: false,
            pending: None,
        })
    }
    /// Histogram resize retains both one-pixel histories and their ping-pong state.
    pub fn rebind_histogram(
        &mut self,
        device: &RenderDevice,
        cache: &PipelineCache,
        histogram: &TextureView,
    ) {
        self.bindings = std::array::from_fn(|index| {
            device.create_bind_group(
                "source_exposure",
                &cache.get_bind_group_layout(&exposure_layout()),
                &BindGroupEntries::sequential((
                    self.uniform.binding().unwrap(),
                    histogram,
                    &self.views[1 - index],
                )),
            )
        });
    }
    /// Preallocate downstream bind groups for both views. Select output_index without rebuilding them.
    pub fn views(&self) -> [&TextureView; 2] {
        [&self.views[0], &self.views[1]]
    }
    pub fn output_index(&self) -> Option<usize> {
        self.pending
            .or_else(|| self.initialized.then_some(self.output_index))
    }
    pub fn recreate(&mut self) {
        self.reset_history();
        self.output_index = 0;
    }
    pub fn history(&self) -> [u32; 2] {
        [self.output_index as u32, u32::from(self.initialized)]
    }
    pub fn reset_history(&mut self) {
        self.initialized = false;
        self.pending = None;
    }
    pub fn completed(&mut self) {
        if let Some(index) = self.pending.take() {
            self.output_index = index;
            self.initialized = true;
        }
    }
    pub fn encode(
        &mut self,
        encoder: &mut CommandEncoder,
        device: &RenderDevice,
        queue: &RenderQueue,
        cache: &PipelineCache,
        delta_seconds: f32,
    ) -> Result<bool, String> {
        if !delta_seconds.is_finite() || delta_seconds < 0. {
            return Err("Invalid source exposure frame delta".into());
        }
        let Some(pipeline) = cache.get_render_pipeline(self.pipeline) else {
            return Ok(false);
        };
        let next = if self.initialized {
            1 - self.output_index
        } else {
            0
        };
        self.uniform.get_mut().adaptation = Vec4::new(
            self.settings.speed_down,
            self.settings.speed_up,
            self.settings.key,
            delta_seconds,
        );
        self.uniform.get_mut().control.x = u32::from(!self.initialized);
        self.uniform.write_buffer(device, queue);
        fullscreen::draw(
            encoder,
            "source_exposure",
            pipeline,
            &self.bindings[next],
            &[&self.views[next]],
            fullscreen::CLEAR,
        );
        self.pending = Some(next);
        Ok(true)
    }
}
impl FullscreenPass for ExposurePass {
    fn pipeline_ids(&self) -> &[CachedRenderPipelineId] {
        std::slice::from_ref(&self.pipeline)
    }
}

fn exposure_layout() -> BindGroupLayoutDescriptor {
    BindGroupLayoutDescriptor::new(
        "source_exposure",
        &BindGroupLayoutEntries::sequential(
            ShaderStages::FRAGMENT,
            (
                uniform_buffer::<ExposureParams>(false),
                texture_2d(TextureSampleType::Uint),
                texture_2d(TextureSampleType::Float { filterable: false }),
            ),
        ),
    )
}
