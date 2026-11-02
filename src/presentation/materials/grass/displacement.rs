//! Source RenderTextureDisplacement camera target. Writer meshes are admitted by world presentation.
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
            .iter()
            .find(|r| r.source == input.camera)
            .ok_or_else(|| ReferenceError("Missing Grass camera record".into()))?;
        let target = reference
            .records
            .iter()
            .find(|r| r.source == input.target)
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
    /// Source Update follows the accepted camera. OnPreCull snaps with truncating remainder.
    pub fn frame(self, source_camera: Vec3, runtime_origin: Vec3, time: f32) -> GrassFrameUniform {
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
        // Source +Z looks down after X90; source camera +Y maps toward runtime -Z.
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
            origin_time: runtime_origin.extend(time),
            area,
            clip_from_world: Mat4::from_scale(Vec3::new(1., -1., 1.))
                * projection
                * world_from_view.inverse(),
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
