//! Persistent source render targets. Allocation is restricted to startup or resize.
use super::lighting_scene::PROBE_COUNT;
use super::surface_views::{MIRROR_EXTENT, MIRROR_START, VIEW_COUNT, WORLD_VIEWS};

use bevy::render::{
    render_resource::*,
    renderer::{RenderDevice, RenderQueue},
};

pub const HDR_FORMAT: TextureFormat = TextureFormat::Rgba16Float;
pub const DEPTH_FORMAT: TextureFormat = TextureFormat::Depth24PlusStencil8;
pub const SHADOW_FORMAT: TextureFormat = TextureFormat::Depth16Unorm;
pub const EXPOSURE_FORMAT: TextureFormat = TextureFormat::R32Float;
pub const HISTOGRAM_FORMAT: TextureFormat = TextureFormat::Rgba32Uint;
/// World view samples with antialiasing on. The source uses 8. WebGPU allows only 1 or 4.
pub const MSAA_SAMPLES: u32 = 4;

pub struct Target {
    pub texture: Texture,
    pub attachment: TextureView,
    pub sampled: Option<TextureView>,
}
impl Target {
    fn new(
        device: &RenderDevice,
        label: &'static str,
        width: u32,
        height: u32,
        samples: u32,
        format: TextureFormat,
        sampled: bool,
    ) -> Self {
        // Only single-sample textures support copy operations.
        let usage = TextureUsages::RENDER_ATTACHMENT
            | if samples == 1 {
                TextureUsages::COPY_SRC
            } else {
                TextureUsages::empty()
            }
            | if sampled {
                TextureUsages::TEXTURE_BINDING
            } else {
                TextureUsages::empty()
            };
        let texture = device.create_texture(&TextureDescriptor {
            label: Some(label),
            size: Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: samples,
            dimension: TextureDimension::D2,
            format,
            usage,
            view_formats: &[],
        });
        // Combined depth/stencil attachments must retain both aspects.
        let attachment = texture.create_view(&TextureViewDescriptor::default());
        let sampled = sampled.then(|| {
            if format == DEPTH_FORMAT {
                texture.create_view(&TextureViewDescriptor {
                    aspect: TextureAspect::DepthOnly,
                    ..Default::default()
                })
            } else {
                attachment.clone()
            }
        });
        Self {
            texture,
            attachment,
            sampled,
        }
    }
}

/// The world view owns one color pair. Probe faces reuse a second pair.
pub struct SourceColorTargets {
    pub width: u32,
    pub height: u32,
    pub samples: u32,
    pub resolved: Target,
    pub multisample: Option<Target>,
    pub logical_bytes: u64,
}
impl SourceColorTargets {
    pub fn new(
        device: &RenderDevice,
        width: u32,
        height: u32,
        samples: u32,
    ) -> Result<Self, String> {
        let limit = device.limits().max_texture_dimension_2d;
        if width == 0
            || height == 0
            || width > limit
            || height > limit
            || !matches!(samples, 1 | MSAA_SAMPLES)
        {
            return Err("Source color extent or sample count is unsupported".into());
        }
        let resolved = Target::new(device, "source_hdr", width, height, 1, HDR_FORMAT, true);
        let multisample = (samples > 1).then(|| {
            Target::new(
                device,
                "source_msaa_hdr",
                width,
                height,
                samples,
                HDR_FORMAT,
                false,
            )
        });
        Ok(Self {
            width,
            height,
            samples,
            resolved,
            multisample,
            logical_bytes: u64::from(width)
                * u64::from(height)
                * 8
                * (1 + if samples > 1 { u64::from(samples) } else { 0 }),
        })
    }
    pub fn attachment(&self) -> &TextureView {
        self.multisample
            .as_ref()
            .map_or(&self.resolved.attachment, |target| &target.attachment)
    }
    pub fn resolve_target(&self) -> Option<&TextureView> {
        self.multisample.as_ref().map(|_| &self.resolved.attachment)
    }
}

/// Depth, attenuation, and shadow storage belong to exactly one surface view.
/// Color ownership is shared without sharing any writable view uniform.
pub struct SourceViewTargets {
    pub colors: std::sync::Arc<SourceColorTargets>,
    pub atlas_size: u32,
    pub multisample_depth: Option<Target>,
    pub depth: Target,
    pub atlas: Target,
    /// Static casters of each cascade tile. The atlas tile copies it, then animated casters draw.
    pub static_atlas: Target,
    /// Excludes shared color storage. Count each color pair once in the graph total.
    pub logical_bytes: u64,
}
impl SourceViewTargets {
    pub fn new(
        device: &RenderDevice,
        colors: std::sync::Arc<SourceColorTargets>,
        atlas_size: u32,
    ) -> Result<Self, String> {
        if atlas_size == 0 || atlas_size > device.limits().max_texture_dimension_2d {
            return Err("Source shadow atlas extent is unsupported".into());
        }
        let SourceColorTargets {
            width,
            height,
            samples,
            ..
        } = *colors;
        let multisample_depth = (samples > 1).then(|| {
            Target::new(
                device,
                "source_msaa_depth",
                width,
                height,
                samples,
                DEPTH_FORMAT,
                true,
            )
        });
        let depth = Target::new(device, "source_depth", width, height, 1, DEPTH_FORMAT, true);
        let atlas = Target::new(
            device,
            "source_shadow_atlas",
            atlas_size,
            atlas_size,
            1,
            SHADOW_FORMAT,
            true,
        );
        let static_atlas = Target::new(
            device,
            "source_shadow_static_atlas",
            atlas_size,
            atlas_size,
            1,
            SHADOW_FORMAT,
            true,
        );
        // D24S8 uses four logical bytes. Backend allocation overhead is excluded.
        let logical_bytes = u64::from(width)
            * u64::from(height)
            * (4 + if samples > 1 {
                4 * u64::from(samples)
            } else {
                0
            })
            + 2 * 2 * u64::from(atlas_size).pow(2);
        Ok(Self {
            colors,
            atlas_size,
            multisample_depth,
            depth,
            atlas,
            static_atlas,
            logical_bytes,
        })
    }
}

/// The world view owns color storage; six capture views share single-sample scratch.
/// The mirror views share another scratch. Depth and shadow atlas storage never alias between views.
pub struct SurfaceTargets {
    pub world: std::sync::Arc<SourceColorTargets>,
    pub capture: std::sync::Arc<SourceColorTargets>,
    pub mirror: std::sync::Arc<SourceColorTargets>,
    pub views: [std::sync::Arc<SourceViewTargets>; VIEW_COUNT],
    /// Lamp shadow atlas of the world view. Every view binds it.
    pub lamp_atlas: Target,
    pub logical_bytes: u64,
}
impl SurfaceTargets {
    pub fn new(
        device: &RenderDevice,
        width: u32,
        height: u32,
        samples: u32,
        atlas: u32,
    ) -> Result<Self, String> {
        use std::sync::Arc;
        let capture_atlas = crate::reference::shadows::source_atlas_size(64, 64).unwrap();
        let world = Arc::new(SourceColorTargets::new(device, width, height, samples)?);
        let capture = Arc::new(SourceColorTargets::new(device, 64, 64, 1)?);
        let mirror_atlas =
            crate::reference::shadows::source_atlas_size(MIRROR_EXTENT, MIRROR_EXTENT).unwrap();
        let mirror = Arc::new(SourceColorTargets::new(
            device,
            MIRROR_EXTENT,
            MIRROR_EXTENT,
            1,
        )?);
        let mut views = Vec::with_capacity(VIEW_COUNT);
        for index in 0..VIEW_COUNT {
            let (color, atlas_size) = if index < WORLD_VIEWS {
                (&world, atlas)
            } else if index < MIRROR_START {
                (&capture, capture_atlas)
            } else {
                (&mirror, mirror_atlas)
            };
            views.push(Arc::new(SourceViewTargets::new(
                device,
                color.clone(),
                atlas_size,
            )?));
        }
        let lamp = super::lamp_shadows::ATLAS;
        let lamp_atlas = Target::new(
            device,
            "source_lamp_shadow_atlas",
            lamp.x,
            lamp.y,
            1,
            SHADOW_FORMAT,
            true,
        );
        let logical_bytes = world.logical_bytes
            + capture.logical_bytes
            + mirror.logical_bytes
            + views.iter().map(|view| view.logical_bytes).sum::<u64>()
            + lamp_atlas_bytes();
        Ok(Self {
            world,
            capture,
            mirror,
            views: views.try_into().ok().unwrap(),
            lamp_atlas,
            logical_bytes,
        })
    }
    /// Main resize preserves fixed capture and mirror targets and published probe storage.
    pub fn resize_world(
        &mut self,
        device: &RenderDevice,
        width: u32,
        height: u32,
        samples: u32,
        atlas: u32,
    ) -> Result<(), String> {
        let world = std::sync::Arc::new(SourceColorTargets::new(device, width, height, samples)?);
        for index in 0..WORLD_VIEWS {
            self.views[index] =
                std::sync::Arc::new(SourceViewTargets::new(device, world.clone(), atlas)?);
        }
        self.world = world;
        self.logical_bytes = self.world.logical_bytes
            + self.capture.logical_bytes
            + self.mirror.logical_bytes
            + self
                .views
                .iter()
                .map(|view| view.logical_bytes)
                .sum::<u64>()
            + lamp_atlas_bytes();
        Ok(())
    }
}
fn lamp_atlas_bytes() -> u64 {
    let lamp = super::lamp_shadows::ATLAS;
    2 * u64::from(lamp.x) * u64::from(lamp.y)
}

/// A scratch cube has fixed per-face render views and per-mip sample views.
/// The latter prevent attachment and sampled-subresource overlap during filtering.
pub struct ScratchCube {
    _texture: Texture,
    pub faces: [[TextureView; 6]; 7],
    pub levels: [TextureView; 7],
    pub cube: TextureView,
}
fn probe_texture(device: &RenderDevice, label: &'static str, layers: u32) -> Texture {
    device.create_texture(&TextureDescriptor {
        label: Some(label),
        size: Extent3d {
            width: 64,
            height: 64,
            depth_or_array_layers: layers,
        },
        mip_level_count: 7,
        sample_count: 1,
        dimension: TextureDimension::D2,
        format: HDR_FORMAT,
        usage: TextureUsages::RENDER_ATTACHMENT
            | TextureUsages::TEXTURE_BINDING
            | TextureUsages::COPY_DST,
        view_formats: &[],
    })
}
fn cube_view(texture: &Texture, level: u32, count: u32) -> TextureView {
    texture.create_view(&TextureViewDescriptor {
        dimension: Some(TextureViewDimension::Cube),
        base_mip_level: level,
        mip_level_count: Some(count),
        array_layer_count: Some(6),
        ..Default::default()
    })
}
impl ScratchCube {
    fn new(device: &RenderDevice, label: &'static str) -> Self {
        let texture = probe_texture(device, label, 6);
        let faces = face_views(&texture, 0);
        let levels = std::array::from_fn(|mip| cube_view(&texture, mip as u32, 1));
        let cube = cube_view(&texture, 0, 7);
        Self {
            _texture: texture,
            faces,
            levels,
            cube,
        }
    }
}
fn face_views(texture: &Texture, first_layer: u32) -> [[TextureView; 6]; 7] {
    std::array::from_fn(|mip| {
        std::array::from_fn(|face| {
            texture.create_view(&TextureViewDescriptor {
                dimension: Some(TextureViewDimension::D2),
                base_mip_level: mip as u32,
                mip_level_count: Some(1),
                base_array_layer: first_layer + face as u32,
                array_layer_count: Some(1),
                ..Default::default()
            })
        })
    })
}
/// The published cubes of every probe slot in one cube array. Scene materials sample it.
/// Fixed textures and views are allocated once.
pub struct PublishedProbes {
    texture: Texture,
    pub view: TextureView,
    _capture: Texture,
    /// Source reflectionBounces=1 keeps candidate selection, but substitutes
    /// the builtin black cube for every registered cube probe during capture.
    /// Its pixels have zero color and alpha; decode is (1,1,0,0).
    pub capture_view: TextureView,
}
impl PublishedProbes {
    pub fn new(device: &RenderDevice, queue: &RenderQueue) -> Self {
        let layers = 6 * PROBE_COUNT as u32;
        let texture = probe_texture(device, "source_probe_published", layers);
        let view = cube_array_view(&texture, 7);
        let capture = device.create_texture(&TextureDescriptor {
            label: Some("source_capture_black_cube"),
            size: Extent3d {
                width: 1,
                height: 1,
                depth_or_array_layers: layers,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: TextureDimension::D2,
            format: TextureFormat::Rgba8UnormSrgb,
            usage: TextureUsages::TEXTURE_BINDING | TextureUsages::COPY_DST,
            view_formats: &[],
        });
        queue.write_texture(
            TexelCopyTextureInfo {
                texture: &capture,
                mip_level: 0,
                origin: Origin3d::ZERO,
                aspect: TextureAspect::All,
            },
            &[0; 4 * 6 * PROBE_COUNT],
            TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(4),
                rows_per_image: Some(1),
            },
            Extent3d {
                width: 1,
                height: 1,
                depth_or_array_layers: layers,
            },
        );
        let capture_view = cube_array_view(&capture, 1);
        Self {
            texture,
            view,
            _capture: capture,
            capture_view,
        }
    }
}
fn cube_array_view(texture: &Texture, mips: u32) -> TextureView {
    texture.create_view(&TextureViewDescriptor {
        dimension: Some(TextureViewDimension::CubeArray),
        mip_level_count: Some(mips),
        ..Default::default()
    })
}
/// Capture and filtering never write into the cube sampled by scene materials.
/// Publication copies all seven HDR levels into the slot's layers of the published array.
pub struct ProbeTargets {
    pub base: ScratchCube,
    pub auxiliary: ScratchCube,
    published: Texture,
    first_layer: u32,
    pub published_faces: [[TextureView; 6]; 7],
}
impl ProbeTargets {
    pub fn new(device: &RenderDevice, published: &PublishedProbes, slot: usize) -> Self {
        let first_layer = 6 * slot as u32;
        Self {
            base: ScratchCube::new(device, "source_probe_base"),
            auxiliary: ScratchCube::new(device, "source_probe_auxiliary"),
            published: published.texture.clone(),
            first_layer,
            published_faces: face_views(&published.texture, first_layer),
        }
    }
    /// Scene recreation invalidates the retained publication before any new scene samples it.
    pub fn clear_publication(&self, queue: &RenderQueue) {
        const ZERO_FACE: [u8; 64 * 64 * 8] = [0; 64 * 64 * 8];
        for mip in 0..7 {
            let side = 64 >> mip;
            for face in 0..6 {
                queue.write_texture(
                    TexelCopyTextureInfo {
                        texture: &self.published,
                        mip_level: mip,
                        origin: Origin3d {
                            x: 0,
                            y: 0,
                            z: self.first_layer + face,
                        },
                        aspect: TextureAspect::All,
                    },
                    &ZERO_FACE[..(side * side * 8) as usize],
                    TexelCopyBufferLayout {
                        offset: 0,
                        bytes_per_row: Some(side * 8),
                        rows_per_image: Some(side),
                    },
                    Extent3d {
                        width: side,
                        height: side,
                        depth_or_array_layers: 1,
                    },
                );
            }
        }
    }
}
