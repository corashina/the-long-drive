//! Installed unnamed GrabPass capture and noninstanced pass-cache state.
use super::render_targets::{HDR_FORMAT, MSAA_SAMPLES, SourceColorTargets};
use bevy::ecs::{query::ROQueryItem, system::SystemParamItem};
use bevy::render::{
    render_phase::{PhaseItem, RenderCommand, RenderCommandResult, TrackedRenderPass},
    render_resource::{binding_types::*, *},
    renderer::{RenderDevice, RenderQueue},
};
use bevy::{ecs as bevy_ecs, prelude::*};
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};

/// Feed normalized native fields. Material/pass values are target runtime identities.
/// Selected materials47/48 have no instancing variant.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GrabPassKey {
    pub material: u64,
    pub pass: u64,
    pub normalized_object_flags: u32,
    pub lightmap_index: u32,
    pub light_mode: u8,
    pub instancing_keywords: u32,
    pub batching_key: u64,
    /// Native comparison uses CustomPropsAndHash.hash >> 1.
    pub custom_properties_hash: u32,
}
#[derive(Default, Debug)]
pub struct GrabPassCache {
    applied: Option<GrabPassKey>,
}
impl GrabPassCache {
    /// Call for every ordered pass, including BASE and non-glass passes.
    /// A true result requires flushing earlier draws before applying this pass.
    pub fn apply(&mut self, next: GrabPassKey) -> bool {
        let changed = self.applied.is_none_or(|previous| {
            previous.light_mode == 5
                || next.light_mode == 5
                || previous.material != next.material
                || previous.pass != next.pass
                || previous.normalized_object_flags != next.normalized_object_flags
                || previous.lightmap_index != next.lightmap_index
                || previous.instancing_keywords != next.instancing_keywords
                || previous.batching_key != next.batching_key
                || previous.custom_properties_hash >> 1 != next.custom_properties_hash >> 1
        });
        self.applied = Some(next);
        changed
    }
    /// Clear at Camera::CleanupAfterRendering, including each probe face render.
    pub fn cleanup(&mut self) {
        self.applied = None;
    }
}

#[derive(Clone, Copy, Debug, PartialEq, ShaderType)]
pub struct GlassViewUniform {
    /// Source D3D11 and target share this reverse-Z device matrix.
    pub clip_from_world: Mat4,
    /// Source ProjectionParams.y/z: near/far.
    pub near_far: Vec4,
    /// Source D3D11 grab texels: (1/W,-1/H,W,H).
    pub grab_texel_size: Vec4,
}
impl GlassViewUniform {
    pub fn new(matrix: Mat4, near: f32, far: f32, extent: UVec2) -> Result<Self, &'static str> {
        if extent.x == 0
            || extent.y == 0
            || !matrix.is_finite()
            || !near.is_finite()
            || !far.is_finite()
            || near <= 0.
            || far <= near
        {
            return Err("Source glass view inputs are invalid");
        }
        Ok(Self {
            clip_from_world: matrix,
            near_far: Vec4::new(near, far, 0., 0.),
            grab_texel_size: Vec4::new(
                1. / extent.x as f32,
                -1. / extent.y as f32,
                extent.x as f32,
                extent.y as f32,
            ),
        })
    }
}
pub fn glass_view_layout() -> BindGroupLayoutDescriptor {
    BindGroupLayoutDescriptor::new(
        "source_glass_view",
        &BindGroupLayoutEntries::sequential(
            ShaderStages::VERTEX_FRAGMENT,
            (
                uniform_buffer::<GlassViewUniform>(false),
                texture_2d(TextureSampleType::Float { filterable: true }),
                sampler(SamplerBindingType::Filtering),
            ),
        ),
    )
}

/// Particle view: glass uniform, single-sample camera depth, named `_GrabTexture`, grab sampler.
pub fn particle_view_layout() -> BindGroupLayoutDescriptor {
    BindGroupLayoutDescriptor::new(
        "source_particle_view",
        &BindGroupLayoutEntries::sequential(
            ShaderStages::VERTEX_FRAGMENT,
            (
                uniform_buffer::<GlassViewUniform>(false),
                texture_depth_2d(),
                texture_2d(TextureSampleType::Float { filterable: true }),
                sampler(SamplerBindingType::Filtering),
            ),
        ),
    )
}

#[derive(Clone, Copy, Default, serde::Serialize)]
pub struct GlassAudit {
    pub frame: u32,
    pub token: u32,
    pub captures: u32,
    pub skipped_grabs: u32,
    pub material_draws: [u32; 2],
    pub total_captures: u32,
    pub extent: [u32; 2],
    pub logical_bytes: u64,
}
/// Own one allocation per source view. Only startup and physical resize create it.
#[derive(Component)]
pub struct SourceGlassView {
    texture: Texture,
    _sampled: TextureView,
    _sampler: Sampler,
    uniform: UniformBuffer<GlassViewUniform>,
    bind_group: BindGroup,
    /// Source named `_GrabTexture`: captured once per view frame before the first distortion particle.
    named: Texture,
    _named_sampled: TextureView,
    /// Placeholder camera depth for views whose depth is the transparent attachment.
    _depth_placeholder: Option<Texture>,
    particle_group: BindGroup,
    extent: UVec2,
    valid: AtomicBool,
    captures: AtomicU32,
    audit: std::sync::Mutex<GlassAudit>,
}
impl SourceGlassView {
    pub fn new(
        device: &RenderDevice,
        queue: &RenderQueue,
        cache: &PipelineCache,
        value: GlassViewUniform,
        camera_depth: Option<&TextureView>,
    ) -> Result<Self, &'static str> {
        let extent = UVec2::new(
            value.grab_texel_size.z as u32,
            value.grab_texel_size.w as u32,
        );
        let limit = device.limits().max_texture_dimension_2d;
        if extent.x == 0 || extent.y == 0 || extent.x > limit || extent.y > limit {
            return Err("Source grab physical extent is unsupported");
        }
        let grab = |label| {
            device.create_texture(&TextureDescriptor {
                label: Some(label),
                size: Extent3d {
                    width: extent.x,
                    height: extent.y,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: TextureDimension::D2,
                format: HDR_FORMAT,
                usage: TextureUsages::COPY_DST | TextureUsages::TEXTURE_BINDING,
                view_formats: &[],
            })
        };
        let texture = grab("source_unnamed_grab");
        let sampled = texture.create_view(&TextureViewDescriptor::default());
        let named = grab("source_named_grab");
        let named_sampled = named.create_view(&TextureViewDescriptor::default());
        // Zero-initialized reverse-Z depth is the far plane: soft particles keep full alpha.
        let depth_placeholder = camera_depth.is_none().then(|| {
            device.create_texture(&TextureDescriptor {
                label: Some("source_particle_depth_placeholder"),
                size: Extent3d {
                    width: 1,
                    height: 1,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: TextureDimension::D2,
                format: TextureFormat::Depth32Float,
                usage: TextureUsages::TEXTURE_BINDING,
                view_formats: &[],
            })
        });
        let placeholder_view = depth_placeholder
            .as_ref()
            .map(|texture| texture.create_view(&TextureViewDescriptor::default()));
        let sampler = device.create_sampler(&SamplerDescriptor {
            label: Some("source_grab_bilinear_clamp"),
            address_mode_u: AddressMode::ClampToEdge,
            address_mode_v: AddressMode::ClampToEdge,
            address_mode_w: AddressMode::ClampToEdge,
            mag_filter: FilterMode::Linear,
            min_filter: FilterMode::Linear,
            mipmap_filter: MipmapFilterMode::Nearest,
            ..default()
        });
        let mut uniform = UniformBuffer::from(value);
        uniform.set_label(Some("source_glass_view_uniform"));
        uniform.write_buffer(device, queue);
        let bind_group = device.create_bind_group(
            "source_glass_view",
            &cache.get_bind_group_layout(&glass_view_layout()),
            &BindGroupEntries::sequential((uniform.binding().unwrap(), &sampled, &sampler)),
        );
        let particle_group = device.create_bind_group(
            "source_particle_view",
            &cache.get_bind_group_layout(&particle_view_layout()),
            &BindGroupEntries::sequential((
                uniform.binding().unwrap(),
                camera_depth.or(placeholder_view.as_ref()).unwrap(),
                &named_sampled,
                &sampler,
            )),
        );
        Ok(Self {
            texture,
            _sampled: sampled,
            _sampler: sampler,
            uniform,
            bind_group,
            named,
            _named_sampled: named_sampled,
            _depth_placeholder: depth_placeholder,
            particle_group,
            extent,
            valid: AtomicBool::new(false),
            captures: AtomicU32::new(0),
            audit: std::sync::Mutex::new(GlassAudit::default()),
        })
    }
    pub fn audit(&self) -> GlassAudit {
        *self.audit.lock().unwrap()
    }
    pub fn completed(&self, frame: u32, token: u32, counts: [u32; 4]) {
        *self.audit.lock().unwrap() = GlassAudit {
            frame,
            token,
            captures: counts[0],
            skipped_grabs: counts[1],
            material_draws: [counts[2], counts[3]],
            total_captures: self.captures(),
            extent: self.extent.to_array(),
            logical_bytes: self.logical_bytes(),
        };
        self.cleanup();
    }
    pub fn extent(&self) -> UVec2 {
        self.extent
    }
    /// Unnamed and named grab textures.
    pub fn logical_bytes(&self) -> u64 {
        u64::from(self.extent.x) * u64::from(self.extent.y) * 16
    }
    pub fn valid(&self) -> bool {
        self.valid.load(Ordering::Acquire)
    }
    pub fn captures(&self) -> u32 {
        self.captures.load(Ordering::Relaxed)
    }
    pub fn unchanged(&self, value: &GlassViewUniform) -> bool {
        self.uniform.get() == value
    }
    pub fn write(
        &mut self,
        value: GlassViewUniform,
        device: &RenderDevice,
        queue: &RenderQueue,
    ) -> Result<(), &'static str> {
        if value.grab_texel_size.z != self.extent.x as f32
            || value.grab_texel_size.w != self.extent.y as f32
        {
            return Err("Source glass view requires physical resize");
        }
        if self.unchanged(&value) {
            return Ok(());
        }
        self.uniform.set(value);
        self.uniform.write_buffer(device, queue);
        Ok(())
    }
    /// End preceding draws before calling. Resolve the current active HDR8 surface.
    /// A source unnamed grab refreshes at each executed application.
    pub fn capture(
        &self,
        encoder: &mut CommandEncoder,
        colors: &SourceColorTargets,
    ) -> Result<(), &'static str> {
        self.copy_current(encoder, colors, &self.texture)?;
        self.valid.store(true, Ordering::Release);
        self.captures.fetch_add(1, Ordering::Relaxed);
        Ok(())
    }
    /// End preceding draws before calling. Source named grabs refresh once per view frame.
    pub fn capture_named(
        &self,
        encoder: &mut CommandEncoder,
        colors: &SourceColorTargets,
    ) -> Result<(), &'static str> {
        self.copy_current(encoder, colors, &self.named)
    }
    fn copy_current(
        &self,
        encoder: &mut CommandEncoder,
        colors: &SourceColorTargets,
        target: &Texture,
    ) -> Result<(), &'static str> {
        if self.extent != UVec2::new(colors.width, colors.height)
            || !matches!(colors.samples, 1 | MSAA_SAMPLES)
            || !colors
                .resolved
                .texture
                .usage()
                .contains(TextureUsages::COPY_SRC)
        {
            return Err("Source grab input extent, samples, or copy usage is invalid");
        }
        if colors.samples > 1 {
            let attachments = [Some(RenderPassColorAttachment {
                view: colors.attachment(),
                depth_slice: None,
                resolve_target: colors.resolve_target().map(|view| &**view),
                ops: Operations {
                    load: LoadOp::Load,
                    store: StoreOp::Store,
                },
            })];
            // This empty pass resolves accumulated MSAA content without modifying it.
            drop(encoder.begin_render_pass(&RenderPassDescriptor {
                label: Some("source_grab_current_color_resolve"),
                color_attachments: &attachments,
                ..default()
            }));
        }
        encoder.copy_texture_to_texture(
            TexelCopyTextureInfo {
                texture: &colors.resolved.texture,
                mip_level: 0,
                origin: Origin3d::ZERO,
                aspect: TextureAspect::All,
            },
            TexelCopyTextureInfo {
                texture: target,
                mip_level: 0,
                origin: Origin3d::ZERO,
                aspect: TextureAspect::All,
            },
            Extent3d {
                width: self.extent.x,
                height: self.extent.y,
                depth_or_array_layers: 1,
            },
        );
        Ok(())
    }
    /// Release image validity at the source cleanup boundary; retain allocation.
    pub fn cleanup(&self) {
        self.valid.store(false, Ordering::Release);
    }
}

pub struct SetSourceGlassView;
impl<P: PhaseItem> RenderCommand<P> for SetSourceGlassView {
    type Param = ();
    type ViewQuery = &'static SourceGlassView;
    type ItemQuery = ();
    fn render<'w>(
        _: &P,
        view: ROQueryItem<'w, '_, Self::ViewQuery>,
        _: Option<ROQueryItem<'w, '_, Self::ItemQuery>>,
        _: SystemParamItem<'w, '_, Self::Param>,
        pass: &mut TrackedRenderPass<'w>,
    ) -> RenderCommandResult {
        if !view.valid() {
            return RenderCommandResult::Skip;
        }
        pass.set_bind_group(1, &view.bind_group, &[]);
        RenderCommandResult::Success
    }
}

pub struct SetSourceParticleView;
impl<P: PhaseItem> RenderCommand<P> for SetSourceParticleView {
    type Param = ();
    type ViewQuery = &'static SourceGlassView;
    type ItemQuery = ();
    fn render<'w>(
        _: &P,
        view: ROQueryItem<'w, '_, Self::ViewQuery>,
        _: Option<ROQueryItem<'w, '_, Self::ItemQuery>>,
        _: SystemParamItem<'w, '_, Self::Param>,
        pass: &mut TrackedRenderPass<'w>,
    ) -> RenderCommandResult {
        pass.set_bind_group(1, &view.particle_group, &[]);
        RenderCommandResult::Success
    }
}
