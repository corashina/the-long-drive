//! Camera-local source effect chains. Allocate only at startup and physical resize.
use super::{
    HistogramPasses,
    ao_composite::AoCompositePass,
    ao_depth::AoDepthPass,
    ao_render::AoRenderPass,
    ao_upsample::AoUpsamplePass,
    bloom::BloomPass,
    copy::CopyPass,
    dof::DofPass,
    exposure::ExposurePass,
    heat_haze::HeatHazePass,
    lut::{CURVE_BYTES, LUT_BYTES, LutPass},
    lut_ldr::{LDR_CURVE_BYTES, LutLdrPass, STRIP_BYTES},
    motion_blur::MotionBlurPass,
    taa::TaaPass,
    uber::{MainInputs, UberPass},
};
use crate::presentation::screen_effects::Lut;
use crate::reference::postprocessing::{
    DeadGrading, DeadProfile, GlassesProfile, GradingSaturation, GradingSettings, Lens,
    PostprocessingRecord, Tonemapper,
};
use bevy::prelude::*;
use bevy::render::{
    render_resource::*,
    renderer::{RenderAdapter, RenderDevice, RenderQueue},
};
use simulation::lighting::CyclePhase;

#[derive(Clone)]
pub struct PostShaders {
    pub copy: Handle<Shader>,
    pub dof: Handle<Shader>,
    pub histogram: Handle<Shader>,
    pub exposure: Handle<Shader>,
    pub bloom: Handle<Shader>,
    pub lut: Handle<Shader>,
    pub lut_ldr: Handle<Shader>,
    pub uber: Handle<Shader>,
    pub heat_haze: Handle<Shader>,
    pub ao_depth: Handle<Shader>,
    pub ao_render: Handle<Shader>,
    pub ao_upsample: Handle<Shader>,
    pub ao_composite: Handle<Shader>,
    pub taa: Handle<Shader>,
    pub motion_blur: Handle<Shader>,
}
/// Passes that read the main motion target. Present when antialiasing or motion blur is on.
pub struct MotionInputs<'a> {
    pub taa: Option<crate::reference::postprocessing::TaaSettings>,
    pub blur: Option<crate::reference::postprocessing::MotionBlurSettings>,
    pub motion: &'a TextureView,
}
/// Main view motion of one frame.
#[derive(Clone, Copy)]
pub struct FrameMotion {
    pub jitter: Vec2,
    /// No valid motion history.
    pub reset: bool,
    /// Shutter angle in degrees. None skips the blur for this frame.
    pub shutter: Option<f32>,
}
pub struct PostTextures<'a> {
    pub noise: [&'a TextureView; 64],
    pub dirt: &'a TextureView,
    pub dirt_extent: [u32; 2],
    /// `BinocularProfile::mask`.
    pub vignette_mask: &'a TextureView,
    /// `LdrLuts` c64 and sepia with their extents.
    pub luts: [(&'a TextureView, [u32; 2]); 2],
    /// `HeatHaze::noise`.
    pub heat_haze_noise: &'a TextureView,
    /// The main view depth.
    pub camera_depth: &'a TextureView,
}
#[derive(Clone, Copy)]
pub struct PostFrame {
    pub token: u32,
    pub delta_seconds: f32,
    /// Main-view grading temperature. None keeps the applied value.
    pub grading_temperature: Option<f32>,
    /// `DeadV` weight over the main grading, 0 to 1.
    pub dead_weight: f32,
    /// `PPVSunGlasses` lens: index in `clothing.glasses_profiles`. None is no lens grading.
    pub glasses: Option<u32>,
    /// The binocular volume applies its vignette, lens distortion, chromatic aberration and DoF.
    pub binoculars: bool,
    /// Day cycle phase and progress for the `postprocesscontrol` saturation.
    pub cycle: (CyclePhase, f32),
    pub lut: Lut,
    pub grayscale: bool,
    /// The ambient temperature while the heat haze setting is on.
    pub heat_haze: Option<f32>,
    /// Shader `_Time.y`.
    pub time: f32,
    /// None when antialiasing and motion blur are off.
    pub motion: Option<FrameMotion>,
    /// DoF focus distance in meters. None is DoF off.
    pub focus: Option<f32>,
    /// Main view clip planes.
    pub near: f32,
    pub far: f32,
}
struct ColorTarget {
    _texture: Texture,
    view: TextureView,
}
impl ColorTarget {
    fn new(device: &RenderDevice, extent: [u32; 2]) -> Self {
        let texture = device.create_texture(&TextureDescriptor {
            label: Some("source_effect_color"),
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
struct MainChain {
    histogram: HistogramPasses,
    exposure: ExposurePass,
    bloom: BloomPass,
    lut: LutPass,
    lut_baked: bool,
    ldr: LutLdrPass,
    /// The user LUT in the baked LDR strip, and the one in flight.
    ldr_baked: Option<usize>,
    ldr_pending: Option<usize>,
    saturation: GradingSaturation,
    /// Main grading with the applied temperature and saturation.
    base: GradingSettings,
    dead: DeadGrading,
    /// `clothing.glasses_profiles`.
    profiles: Vec<GlassesProfile>,
    /// Lens and dead weight of the applied grading.
    glasses: Option<u32>,
    dead_weight: f32,
    /// Grading and tonemapper in the LUT and Uber.
    grading: GradingSettings,
    tonemapper: Tonemapper,
}
pub struct SourcePostChain {
    pub extent: [u32; 2],
    pub logical_bytes: u64,
    pub startup_count: u32,
    pub resize_count: u32,
    initial: ColorTarget,
    copy: CopyPass,
    taa: Option<TaaPass>,
    dof: DofPass,
    /// The `DOF` volume lens, then the `tavcso` lens.
    lenses: [Lens; 2],
    blur: Option<MotionBlurPass>,
    uber: UberPass,
    haze: HeatHazePass,
    main: MainChain,
    stop_nan: bool,
    completed: u32,
    pending: Option<u32>,
}
impl SourcePostChain {
    pub fn new(
        adapter: &RenderAdapter,
        device: &RenderDevice,
        queue: &RenderQueue,
        cache: &PipelineCache,
        shaders: &PostShaders,
        textures: &PostTextures<'_>,
        source: &TextureView,
        depth: &TextureView,
        extent: [u32; 2],
        samples: u32,
        record: &PostprocessingRecord,
        dead: &DeadProfile,
        glasses: &[GlassesProfile],
        stop_nan: bool,
        motion: Option<MotionInputs<'_>>,
        previous: Option<Self>,
    ) -> Result<Self, String> {
        if extent.contains(&0)
            || extent.into_iter().max().unwrap() > device.limits().max_texture_dimension_2d
        {
            return Err("Invalid source postprocessing extent".into());
        }
        let startup_count = previous.as_ref().map_or(1, |old| old.startup_count);
        let resize_count = previous.as_ref().map_or(0, |old| old.resize_count + 1);
        let completed = previous.as_ref().map_or(0, |old| old.completed);
        let initial = ColorTarget::new(device, extent);
        let (mut copy, old_dof, old_main, old_uber, old_taa, old_blur, old_haze) = match previous {
            Some(old) => (
                old.copy,
                Some(old.dof),
                Some(old.main),
                Some(old.uber),
                old.taa,
                old.blur,
                Some(old.haze),
            ),
            None => (
                CopyPass::new(device, cache, shaders.copy.clone(), source),
                None,
                None,
                None,
                None,
                None,
                None,
            ),
        };
        let dof = DofPass::new(
            device,
            queue,
            cache,
            shaders.dof.clone(),
            depth,
            motion
                .as_ref()
                .and_then(|inputs| Some((inputs.motion, inputs.taa?.motion_blending))),
            extent,
            old_dof,
        );
        copy.rebind(device, cache, source);
        let taa = motion
            .as_ref()
            .and_then(|inputs| Some((inputs, inputs.taa?)))
            .map(|(inputs, settings)| {
                TaaPass::new(
                    device,
                    queue,
                    cache,
                    shaders.taa.clone(),
                    &initial.view,
                    depth,
                    inputs.motion,
                    extent,
                    settings,
                    old_taa,
                )
            })
            .transpose()?;
        let scene = taa.as_ref().map_or(&initial.view, |taa| taa.output());
        let blur = motion
            .as_ref()
            .and_then(|inputs| Some((inputs, inputs.blur?)))
            .map(|(inputs, settings)| {
                MotionBlurPass::new(
                    device,
                    queue,
                    cache,
                    shaders.motion_blur.clone(),
                    scene,
                    depth,
                    inputs.motion,
                    extent,
                    &settings,
                    old_blur,
                )
            })
            .transpose()?;
        let scene = blur.as_ref().map_or(scene, |blur| blur.output());
        let main = {
            let (old_histogram, old_exposure, old_bloom, old_lut, lut_baked, old_ldr, grading) =
                match old_main {
                    Some(old) => (
                        Some(old.histogram),
                        Some(old.exposure),
                        Some(old.bloom),
                        Some(old.lut),
                        old.lut_baked,
                        Some((old.ldr, old.ldr_baked)),
                        (
                            old.base,
                            old.dead,
                            old.profiles,
                            old.glasses,
                            old.dead_weight,
                            old.grading,
                            old.tonemapper,
                        ),
                    ),
                    None => {
                        let (dead, ignored) = DeadGrading::new(dead);
                        if !ignored.is_empty() {
                            warn!("DeadV parameters without a main stack effect: {ignored:?}");
                        }
                        (
                            None,
                            None,
                            None,
                            None,
                            false,
                            None,
                            (
                                record.grading,
                                dead,
                                glasses.to_vec(),
                                None,
                                0.,
                                record.grading,
                                Tonemapper::Neutral,
                            ),
                        )
                    }
                };
            let (base, dead, profiles, glasses, dead_weight, grading, tonemapper) = grading;
            let histogram = HistogramPasses::new(
                device,
                queue,
                cache,
                shaders.histogram.clone(),
                scene,
                extent[0],
                extent[1],
                old_histogram.as_ref(),
            )?;
            let exposure = if let Some(mut old) = old_exposure {
                old.rebind_histogram(device, cache, histogram.output());
                old
            } else {
                ExposurePass::new(
                    device,
                    queue,
                    cache,
                    shaders.exposure.clone(),
                    histogram.output(),
                    record.exposure,
                )?
            };
            let bloom = BloomPass::new(
                adapter,
                device,
                queue,
                cache,
                shaders.bloom.clone(),
                scene,
                exposure.views(),
                extent,
                &record.bloom,
                old_bloom.as_ref(),
            )?;
            let lut = if let Some(old) = old_lut {
                old
            } else {
                LutPass::new(
                    adapter,
                    device,
                    queue,
                    cache,
                    shaders.lut.clone(),
                    &record.grading,
                )?
            };
            let (ldr, ldr_baked) = match old_ldr {
                Some(old) => old,
                None => (
                    LutLdrPass::new(
                        device,
                        queue,
                        cache,
                        shaders.lut_ldr.clone(),
                        textures.luts,
                        &grading,
                    )?,
                    None,
                ),
            };
            MainChain {
                histogram,
                exposure,
                bloom,
                lut,
                lut_baked,
                ldr,
                ldr_baked,
                ldr_pending: None,
                saturation: record.saturation,
                base,
                dead,
                profiles,
                glasses,
                dead_weight,
                grading,
                tonemapper,
            }
        };
        let inputs = MainInputs {
            bloom: main.bloom.output(),
            exposure: main.exposure.views(),
            dirt: textures.dirt,
            dirt_extent: textures.dirt_extent,
            vignette_mask: textures.vignette_mask,
            lut: main.lut.output(),
            strip: main.ldr.output(),
            sample_scale: main.bloom.layout.sample_scale,
        };
        let mut uber = UberPass::new(
            device,
            queue,
            cache,
            shaders.uber.clone(),
            scene,
            textures.noise,
            extent,
            samples,
            record,
            inputs,
            old_uber,
        )?;
        uber.set_post_exposure(main.grading.post_exposure);
        let haze = HeatHazePass::new(
            device,
            queue,
            cache,
            shaders.heat_haze.clone(),
            textures.camera_depth,
            textures.heat_haze_noise,
            extent,
            samples,
            &record.heat_haze,
            old_haze.as_ref(),
        );
        let area = u64::from(extent[0]) * u64::from(extent[1]);
        let logical_bytes = area * 16
            + main.histogram.layout.logical_bytes()
            + 8
            + main.bloom.layout.logical_bytes()
            + LUT_BYTES
            + CURVE_BYTES
            + STRIP_BYTES
            + LDR_CURVE_BYTES
            + taa.as_ref().map_or(0, |taa| taa.logical_bytes)
            + dof.logical_bytes
            + blur.as_ref().map_or(0, |blur| blur.layout.logical_bytes());
        Ok(Self {
            extent,
            logical_bytes,
            startup_count,
            resize_count,
            initial,
            copy,
            taa,
            dof,
            lenses: [record.depth_of_field.lens, record.binoculars.depth_of_field],
            blur,
            uber,
            haze,
            main,
            stop_nan,
            completed,
            pending: None,
        })
    }
    pub fn completed_token(&self) -> u32 {
        self.completed
    }
    pub fn pending_token(&self) -> Option<u32> {
        self.pending
    }
    pub fn has_blur(&self) -> bool {
        self.blur.is_some()
    }
    pub fn ready(&self, cache: &PipelineCache) -> bool {
        self.copy.ready(cache)
            && self.taa.as_ref().is_none_or(|taa| taa.ready(cache))
            && self.dof.ready(cache)
            && self.blur.as_ref().is_none_or(|blur| blur.ready(cache))
            && self.uber.ready(cache)
            && self.main.histogram.ready(cache)
            && self.main.exposure.ready(cache)
            && self.main.bloom.ready(cache)
            && self.main.lut.ready(cache)
            && self.main.ldr.ready(cache)
            && self.haze.ready(cache)
    }
    pub fn failure(&self, cache: &PipelineCache) -> Option<String> {
        self.copy
            .failure(cache)
            .or_else(|| self.taa.as_ref().and_then(|taa| taa.failure(cache)))
            .or_else(|| self.dof.failure(cache))
            .or_else(|| self.blur.as_ref().and_then(|blur| blur.failure(cache)))
            .or_else(|| self.uber.failure(cache))
            .or_else(|| self.main.histogram.failure(cache))
            .or_else(|| self.main.exposure.failure(cache))
            .or_else(|| self.main.bloom.failure(cache))
            .or_else(|| self.main.lut.failure(cache))
            .or_else(|| self.main.ldr.failure(cache))
            .or_else(|| self.haze.failure(cache))
    }
    pub fn recreate(&mut self) {
        self.pending = None;
        self.completed = 0;
        self.uber.recreate();
        self.main.exposure.recreate();
        if let Some(taa) = self.taa.as_mut() {
            taa.reset_history();
        }
        self.dof.reset_history();
        self.main.lut_baked = false;
        self.main.ldr_baked = None;
        self.main.ldr_pending = None;
    }
    /// Noise index, exposure output/initialized.
    pub fn history(&self) -> [u32; 3] {
        let exposure = self.main.exposure.history();
        [self.uber.noise_index(), exposure[0], exposure[1]]
    }
    pub fn reset_history(&mut self) {
        self.pending = None;
        self.main.exposure.reset_history();
        if let Some(taa) = self.taa.as_mut() {
            taa.reset_history();
        }
        self.dof.reset_history();
    }
    /// Image histories remain intact during floating-origin transport.
    pub fn rebase(&mut self, _delta: Vec3) {}
    pub fn encode(
        &mut self,
        encoder: &mut CommandEncoder,
        device: &RenderDevice,
        queue: &RenderQueue,
        cache: &PipelineCache,
        frame: PostFrame,
        output: &TextureView,
        resolve: Option<&TextureView>,
    ) -> Result<bool, String> {
        if frame.token == 0 || frame.token <= self.completed {
            return Ok(false);
        }
        if !self.ready(cache) {
            return Ok(false);
        }
        if !frame.delta_seconds.is_finite() || frame.delta_seconds < 0. {
            return Err("Invalid source effect frame".into());
        }
        // Post stack v2 order: TAA, DoF, then motion blur. With DoF on, the step before it writes the DoF input.
        let dof_input = frame.focus.map(|_| self.dof.raw());
        if let Some(taa) = self.taa.as_mut() {
            let motion = frame.motion.ok_or("Source TAA has no view motion")?;
            self.copy
                .encode(encoder, cache, &self.initial.view, self.stop_nan);
            taa.encode(
                encoder,
                device,
                queue,
                cache,
                motion.jitter,
                motion.reset,
                dof_input,
            )?;
        } else {
            self.copy.encode(
                encoder,
                cache,
                dof_input.unwrap_or(&self.initial.view),
                self.stop_nan,
            );
        }
        if let Some(focus) = frame.focus {
            let scene = match self.taa.as_ref() {
                Some(taa) => taa.output(),
                None => &self.initial.view,
            };
            // The CoC temporal filter runs with TAA only.
            let jitter = frame
                .motion
                .filter(|_| self.taa.is_some())
                .map(|m| (m.jitter, m.reset));
            self.dof.encode(
                encoder,
                device,
                queue,
                cache,
                self.lenses[usize::from(frame.binoculars)],
                focus,
                frame.near,
                frame.far,
                jitter,
                scene,
            )?;
        }
        if let Some(blur) = self.blur.as_mut() {
            // Without motion this frame, the blur copies its source.
            let shutter = frame.motion.and_then(|m| m.shutter.filter(|_| !m.reset));
            blur.encode(
                encoder, device, queue, cache, frame.near, frame.far, shutter,
            );
        }
        self.main.histogram.encode(encoder, cache);
        self.main
            .exposure
            .encode(encoder, device, queue, cache, frame.delta_seconds)?;
        let exposure_index = self
            .main
            .exposure
            .output_index()
            .ok_or("Source exposure output is missing")?;
        self.main.bloom.encode(encoder, cache, exposure_index);
        let mut changed =
            frame.glasses != self.main.glasses || frame.dead_weight != self.main.dead_weight;
        if let Some(temperature) = frame.grading_temperature {
            if !temperature.is_finite() {
                return Err("Invalid source grading temperature".into());
            }
            changed |= temperature != self.main.base.temperature;
            self.main.base.temperature = temperature;
        }
        // `postprocesscontrol.Update`: grayscale, else the day or night saturation.
        let saturation = if frame.grayscale {
            -100.
        } else {
            self.main.saturation.at(frame.cycle.0, frame.cycle.1)
        };
        changed |= saturation != self.main.base.saturation;
        self.main.base.saturation = saturation;
        if !(0. ..=1.).contains(&frame.dead_weight) {
            return Err("Invalid source dead weight".into());
        }
        if changed {
            // The lens overrides the main grading, then `DeadV` blends on top.
            let (mut grading, tonemapper) = match frame.glasses {
                Some(index) => self.main.base.with_glasses(
                    self.main
                        .profiles
                        .get(index as usize)
                        .ok_or("Invalid source glasses profile")?,
                ),
                None => (self.main.base, Tonemapper::Neutral),
            };
            self.main.dead.blend(&mut grading, frame.dead_weight);
            self.main.glasses = frame.glasses;
            self.main.dead_weight = frame.dead_weight;
            if grading != self.main.grading || tonemapper != self.main.tonemapper {
                self.main
                    .lut
                    .set_grading(device, queue, &grading, tonemapper);
                self.main.ldr.set_grading(device, queue, &grading);
                self.uber.set_post_exposure(grading.post_exposure);
                self.main.grading = grading;
                self.main.tonemapper = tonemapper;
                self.main.lut_baked = false;
                self.main.ldr_baked = None;
            }
        }
        if !self.main.lut_baked {
            self.main.lut.encode(encoder, cache);
        }
        // ILUT 1 and 2 set LDR grading. The lens and `DeadV` override gradingMode with HDR.
        let ldr = match frame.lut {
            Lut::Default => None,
            Lut::C64 => Some(0),
            Lut::Sepia => Some(1),
        }
        .filter(|_| frame.glasses.is_none() && frame.dead_weight == 0.);
        self.main.ldr_pending = None;
        if let Some(index) = ldr
            && self.main.ldr_baked != Some(index)
        {
            self.main.ldr.encode(encoder, cache, index);
            self.main.ldr_pending = Some(index);
        }
        // The heat haze at weight 0 samples texel centers: the same image. Then it does not run.
        let haze = frame
            .heat_haze
            .map(|temperature| self.haze.weight(temperature))
            .filter(|weight| *weight > 0.);
        self.uber.set_binoculars(frame.binoculars);
        let (target, target_resolve) = match haze {
            Some(_) => (&self.haze.input, None),
            None => (output, resolve),
        };
        if !self.uber.encode(
            encoder,
            device,
            queue,
            cache,
            target,
            target_resolve,
            exposure_index,
            ldr.is_some(),
            haze.is_some(),
        ) {
            return Err("Source Uber did not encode a ready pipeline".into());
        }
        if let Some(weight) = haze
            && !self.haze.encode(
                encoder, device, queue, cache, output, resolve, weight, frame.time,
            )
        {
            return Err("Source heat haze did not encode a ready pipeline".into());
        }
        self.pending = Some(frame.token);
        Ok(true)
    }
    pub fn completed(&mut self, token: u32) -> bool {
        if token <= self.completed || self.pending != Some(token) {
            return false;
        }
        self.main.exposure.completed();
        if let Some(taa) = self.taa.as_mut() {
            taa.completed();
        }
        self.dof.completed();
        self.main.lut_baked = true;
        if let Some(index) = self.main.ldr_pending.take() {
            self.main.ldr_baked = Some(index);
        }
        self.uber.completed();
        self.completed = token;
        self.pending = None;
        true
    }
}

pub struct SourceAoChain {
    pub extent: [u32; 2],
    pub logical_bytes: u64,
    pub startup_count: u32,
    pub resize_count: u32,
    depth: AoDepthPass,
    render: AoRenderPass,
    upsample: AoUpsamplePass,
    composite: AoCompositePass,
}
impl SourceAoChain {
    pub fn new(
        adapter: &RenderAdapter,
        device: &RenderDevice,
        queue: &RenderQueue,
        cache: &PipelineCache,
        shaders: &PostShaders,
        camera_depth: &TextureView,
        extent: [u32; 2],
        samples: u32,
        record: &PostprocessingRecord,
        previous: Option<Self>,
    ) -> Result<Self, String> {
        let depth = AoDepthPass::new(
            adapter,
            device,
            queue,
            cache,
            shaders.ao_depth.clone(),
            camera_depth,
            extent,
            previous.as_ref().map(|old| &old.depth),
        )?;
        let render = AoRenderPass::new(
            adapter,
            device,
            queue,
            cache,
            shaders.ao_render.clone(),
            &depth,
            record.occlusion.intensity,
            record.occlusion.thickness,
            previous.as_ref().map(|old| &old.render),
        )?;
        let upsample = AoUpsamplePass::new(
            device,
            queue,
            cache,
            shaders.ao_upsample.clone(),
            &depth,
            &render,
            &record.occlusion,
            previous.as_ref().map(|old| &old.upsample),
        )?;
        let composite = AoCompositePass::new(
            device,
            queue,
            cache,
            shaders.ao_composite.clone(),
            &depth.copy.view,
            &upsample.output[3].view,
            samples,
            previous.as_ref().map(|old| &old.composite),
        );
        let logical_bytes = depth.logical_bytes + render.logical_bytes + upsample.logical_bytes;
        Ok(Self {
            extent,
            logical_bytes,
            startup_count: previous.as_ref().map_or(1, |old| old.startup_count),
            resize_count: previous.as_ref().map_or(0, |old| old.resize_count + 1),
            depth,
            render,
            upsample,
            composite,
        })
    }
    pub fn ready(&self, cache: &PipelineCache) -> bool {
        self.depth.ready(cache)
            && self.render.ready(cache)
            && self.upsample.ready(cache)
            && self.composite.ready(cache)
    }
    pub fn failure(&self, cache: &PipelineCache) -> Option<String> {
        self.depth
            .failure(cache)
            .or_else(|| self.render.failure(cache))
            .or_else(|| self.upsample.failure(cache))
            .or_else(|| self.composite.failure(cache))
    }
    pub fn encode(
        &mut self,
        encoder: &mut CommandEncoder,
        device: &RenderDevice,
        queue: &RenderQueue,
        cache: &PipelineCache,
        color: &TextureView,
        resolve: Option<&TextureView>,
        near: f32,
        far: f32,
        projection_00: f32,
        fog_density: f32,
    ) -> Result<bool, String> {
        if !self.ready(cache) {
            return Ok(false);
        }
        if !near.is_finite()
            || !far.is_finite()
            || near <= 0.
            || far <= near
            || !projection_00.is_finite()
            || projection_00 <= 0.
            || !fog_density.is_finite()
            || fog_density < 0.
        {
            return Err("Invalid source AO frame".into());
        }
        self.depth
            .encode(encoder, device, queue, cache, near, far)?;
        self.render
            .encode(encoder, device, queue, cache, projection_00)?;
        self.upsample.encode(encoder, cache);
        self.composite.encode(
            encoder,
            device,
            queue,
            cache,
            color,
            resolve,
            near,
            far,
            fog_density,
        )
    }
}
