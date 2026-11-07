//! Source RenderTextureDisplacement camera target. World presentation admits the writer meshes.
use crate::reference::{ReferenceError, WorldReference, WorldValue};
use bevy::{
    ecs as bevy_ecs,
    prelude::*,
    render::{
        render_resource::*,
        renderer::{RenderDevice, RenderQueue},
    },
};
#[derive(Clone, Copy, Debug, ShaderType)]
pub struct GrassFrameUniform {
    pub origin_time: Vec4,
    pub area: Vec4,
    pub clip_from_world: Mat4,
    /// Source camera position of this frame. The color pass measures blade distance and facing from it.
    pub camera: Vec4,
    /// xy: the bend rotation from the sim wind.
    pub wind: Vec4,
}
/// Grass wind from the sim wind. At 2 m/s the waves keep the source speed.
#[derive(Default)]
pub struct GrassWind {
    last: Option<f64>,
    /// Wind time on top of the source shader time, in seconds.
    extra: f32,
}
impl GrassWind {
    /// `time` is the source shader time. `wind` is the sim wind in m/s along world `x`, `z`.
    /// Returns the bend rotation in `xy` and the wind time in `z`.
    pub fn advance(&mut self, time: f64, wind: [f32; 2]) -> Vec4 {
        let dt = self
            .last
            .map_or(0., |last| (time - last).clamp(0., 1.) as f32);
        self.last = Some(time);
        let speed = wind[0].hypot(wind[1]);
        // The source speed and turbulence scale by `1 + speed / 6`, divided by its value at 2 m/s.
        self.extra += dt * ((6. + speed) / 8. - 1.);
        // Source x, z is world x, -z. The base bend is 45 degrees from the rotation.
        let angle = (-wind[1]).atan2(wind[0]) - std::f32::consts::FRAC_PI_4;
        Vec4::new(angle.cos(), angle.sin(), time as f32 + self.extra, 0.)
    }
}
#[derive(Resource, Default)]
pub struct GrassGpu(pub std::collections::HashMap<u64, GrassGpuData>);
pub struct GrassGpuData {
    pub color: Texture,
    pub color_view: TextureView,
    pub depth: Texture,
    pub depth_view: TextureView,
    pub sampler: Sampler,
    pub uniform: Buffer,
}
#[derive(Clone, Copy)]
pub struct GrassCamera {
    pub height: f32,
    pub size: f32,
    pub near: f32,
    pub far: f32,
    pub width: u32,
    pub pixels_high: u32,
}
impl GrassCamera {
    pub fn from_source(reference: &WorldReference) -> Result<Self, ReferenceError> {
        let input = &reference.grass_displacement;
        let camera = reference
            .records
            .find(&input.camera)
            .ok_or_else(|| ReferenceError("Missing Grass camera record".into()))?;
        let target = reference
            .records
            .find(&input.target)
            .ok_or_else(|| ReferenceError("Missing Grass target record".into()))?;
        let number = |record: &crate::reference::WorldSourceRecord, name: &str| match record
            .fields
            .get(name)
        {
            Some(WorldValue::Number(x)) => Ok(*x as f32),
            Some(WorldValue::Integer(x)) => Ok(*x as f32),
            _ => Err(ReferenceError(format!("Missing Grass producer {name}"))),
        };
        let width = number(target, "m_Width")?;
        let height = number(target, "m_Height")?;
        if width != 256.
            || height != 256.
            || number(target, "m_ColorFormat")? != 8.
            || number(target, "m_DepthStencilFormat")? != 92.
        {
            return Err(ReferenceError(
                "Grass displacement format differs from the checked RGBA8/D24S8 target".into(),
            ));
        }
        Ok(Self {
            height: input.follow_height,
            size: number(camera, "orthographic size")?,
            near: number(camera, "near clip plane")?,
            far: number(camera, "far clip plane")?,
            width: width as u32,
            pixels_high: height as u32,
        })
    }
    /// Source Update follows the accepted camera. OnPreCull snaps with a truncating remainder.
    /// `wind` comes from `GrassWind::advance`.
    pub fn frame(self, source_camera: Vec3, runtime_origin: Vec3, wind: Vec4) -> GrassFrameUniform {
        let mut position = source_camera + Vec3::Y * self.height;
        let step_x = 2. * self.size / self.width as f32;
        let step_z = 2. * self.size / self.pixels_high as f32;
        position.x -= position.x % step_x;
        position.z -= position.z % step_z;
        let aspect = self.width as f32 / self.pixels_high as f32;
        let area = Vec4::new(
            position.x - self.size * aspect,
            position.z - self.size,
            2. * self.size * aspect,
            2. * self.size / aspect,
        );
        let runtime = Vec3::new(position.x, position.y, -position.z) - runtime_origin;
        // Source +Z looks down after X90. Source camera +Y maps toward runtime -Z.
        let world_from_view = Mat4::from_rotation_translation(
            Quat::from_rotation_x(-std::f32::consts::FRAC_PI_2),
            runtime,
        );
        let projection = Mat4::orthographic_rh(
            -self.size * aspect,
            self.size * aspect,
            -self.size,
            self.size,
            self.near,
            self.far,
        );
        GrassFrameUniform {
            origin_time: runtime_origin.extend(wind.z),
            area,
            clip_from_world: Mat4::from_scale(Vec3::new(1., -1., 1.))
                * projection
                * world_from_view.inverse(),
            camera: source_camera.extend(1.),
            wind,
        }
    }
}
impl GrassGpuData {
    pub fn new(device: &RenderDevice) -> Self {
        let extent = Extent3d {
            width: 256,
            height: 256,
            depth_or_array_layers: 1,
        };
        let color = device.create_texture(&TextureDescriptor {
            label: Some("source Grass RenderTexture762"),
            size: extent,
            mip_level_count: 1,
            sample_count: 1,
            dimension: TextureDimension::D2,
            format: TextureFormat::Rgba8Unorm,
            usage: TextureUsages::RENDER_ATTACHMENT | TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        });
        let depth = device.create_texture(&TextureDescriptor {
            label: Some("source Grass depth"),
            size: extent,
            mip_level_count: 1,
            sample_count: 1,
            dimension: TextureDimension::D2,
            format: TextureFormat::Depth24PlusStencil8,
            usage: TextureUsages::RENDER_ATTACHMENT,
            view_formats: &[],
        });
        let sampler = device.create_sampler(&SamplerDescriptor {
            label: Some("source Grass clamp bilinear"),
            address_mode_u: AddressMode::ClampToEdge,
            address_mode_v: AddressMode::ClampToEdge,
            address_mode_w: AddressMode::ClampToEdge,
            mag_filter: FilterMode::Linear,
            min_filter: FilterMode::Linear,
            mipmap_filter: MipmapFilterMode::Nearest,
            ..default()
        });
        let uniform = device.create_buffer(&BufferDescriptor {
            label: Some("source Grass frame"),
            size: 128,
            usage: BufferUsages::UNIFORM | BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let color_view = color.create_view(&default());
        let depth_view = depth.create_view(&default());
        Self {
            color,
            color_view,
            depth,
            depth_view,
            sampler,
            uniform,
        }
    }
    pub fn update(&self, queue: &RenderQueue, frame: GrassFrameUniform) {
        let mut bytes = encase::UniformBuffer::new([0u8; 128]);
        bytes.write(&frame).expect("Grass frame uniform");
        queue.write_buffer(&self.uniform, 0, bytes.as_ref());
    }
    pub fn gpu_bytes(&self) -> usize {
        256 * 256 * 8 + 128
    }
}
