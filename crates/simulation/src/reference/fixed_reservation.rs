//! Checked construction bounds for fixed Bevy imports.
use super::{
    FileCatalog, ReferenceError, lighting_scene::CloudMesh, prototype_plan::PrototypePlan, require,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FixedReservation {
    pub mesh_bytes: usize,
    pub cpu: usize,
    pub gpu: usize,
    pub staging: usize,
}
impl FixedReservation {
    pub fn mesh(plan: &PrototypePlan, retained: usize) -> Result<Self, ReferenceError> {
        // The siphon hose prototype is skinned. Joint and weight attributes are in the authored stride.
        for node in &plan.nodes {
            require(
                node.mesh
                    .is_none_or(|mesh| plan.primitives.iter().any(|p| p.mesh == mesh)),
                "fixed mesh primitive coverage",
            )?;
        }
        let mut bytes = 0usize;
        // Bevy imports each primitive, including meshes not used by a scene node.
        for primitive in &plan.primitives {
            require(
                primitive.vertices > 0
                    && primitive.indices > 0
                    && primitive.attributes.iter().any(|a| a == "POSITION"),
                "fixed mesh vertex contract",
            )?;
            let indices = primitive.indices.checked_mul(4).ok_or_else(overflow)?;
            let authored = primitive.bytes.checked_sub(indices).ok_or_else(overflow)?;
            require(
                authored % primitive.vertices == 0,
                "fixed mesh authored stride",
            )?;
            let stride = authored / primitive.vertices;
            require(
                stride >= 12 && stride % 4 == 0,
                "fixed mesh authored attributes",
            )?;
            let expanded = primitive
                .vertices
                .max(primitive.indices)
                .checked_mul(stride.checked_add(32).ok_or_else(overflow)?)
                .and_then(|n| n.checked_add(indices))
                .ok_or_else(overflow)?;
            bytes = bytes.checked_add(expanded).ok_or_else(overflow)?;
        }
        Self::checked(
            plan.json_bytes,
            plan.buffer_bytes,
            bytes.max(plan.mesh_bytes),
            plan.scene_bytes,
            plan.primitives.len(),
            retained,
        )
    }

    pub fn cloud(mesh: &CloudMesh, catalog: &FileCatalog) -> Result<Self, ReferenceError> {
        require(
            mesh.vertex_count == 829 && mesh.index_count == 4596,
            "fixed cloud geometry counts",
        )?;
        let json = catalog.file(&mesh.file)?;
        let buffer = catalog.file(&mesh.buffer)?;
        json.validate()?;
        buffer.validate()?;
        require(
            json.bytes > 0 && buffer.bytes == 829 * (12 + 8) + 4596 * 4,
            "fixed cloud input contract",
        )?;
        // The checked producer writes POSITION f32x3 and TEXCOORD_0 f32x2 only.
        let bytes = 4596usize
            .checked_mul(12 + 8 + 32)
            .and_then(|n| n.checked_add(4596 * 4))
            .ok_or_else(overflow)?;
        Self::checked(
            json.bytes as usize,
            buffer.bytes as usize,
            bytes,
            4096,
            1,
            0,
        )
    }

    fn checked(
        json: usize,
        buffer: usize,
        mesh_bytes: usize,
        scene: usize,
        count: usize,
        retained: usize,
    ) -> Result<Self, ReferenceError> {
        let cpu = json
            .checked_mul(18)
            .and_then(|n| n.checked_add(buffer.checked_mul(2)?))
            .and_then(|n| n.checked_add(mesh_bytes.checked_mul(3)?))
            .and_then(|n| n.checked_add(scene))
            .and_then(|n| n.checked_add(count.checked_mul(4096)?))
            .and_then(|n| n.checked_add(retained))
            .ok_or_else(overflow)?;
        let gpu = count
            .checked_mul(8)
            .and_then(|n| n.checked_add(mesh_bytes))
            .ok_or_else(overflow)?;
        let staging = json
            .checked_add(buffer)
            .and_then(|n| n.checked_mul(2))
            .ok_or_else(overflow)?;
        Ok(Self {
            mesh_bytes,
            cpu,
            gpu,
            staging,
        })
    }
}
fn overflow() -> ReferenceError {
    ReferenceError("Fixed Bevy reservation overflow".into())
}

/// Decoded RGBA8 levels, in ascending authored mip order.
pub fn rgba_payload(width: u32, height: u32, count: u32) -> Result<usize, ReferenceError> {
    require(
        width > 0 && height > 0 && count > 0 && count <= 32 - width.max(height).leading_zeros(),
        "RGBA8 extent or mip count",
    )?;
    (0..count).try_fold(0usize, |sum, level| {
        ((width >> level).max(1) as usize)
            .checked_mul((height >> level).max(1) as usize)
            .and_then(|n| n.checked_mul(4))
            .and_then(|n| n.checked_add(sum))
            .ok_or_else(overflow)
    })
}
pub fn validate_rgba(texture: &super::TextureRecord) -> Result<usize, ReferenceError> {
    let super::TextureStorage::Rgba8 {
        width,
        height,
        input,
    } = &texture.storage
    else {
        return Err(ReferenceError("Expected RGBA8 storage".into()));
    };
    input.validate()?;
    let bytes = rgba_payload(*width, *height, texture.sampler.mip_count)?;
    require(
        input.file.ends_with(".rgba")
            && input.image_size.is_none()
            && input.bytes as usize == bytes,
        "RGBA8 receipt bytes",
    )?;
    Ok(bytes)
}
/// Original single-layer BC blocks, in ascending authored mip order.
pub fn bc_payload(
    format: super::NativeBcFormat,
    width: u32,
    height: u32,
    count: u32,
) -> Result<usize, ReferenceError> {
    require(
        width > 0 && height > 0 && count > 0 && count <= 32 - width.max(height).leading_zeros(),
        "BC extent or mip count",
    )?;
    (0..count).try_fold(0usize, |sum, level| {
        let w = (width >> level).max(1) as usize;
        let h = (height >> level).max(1) as usize;
        w.div_ceil(4)
            .checked_mul(h.div_ceil(4))
            .and_then(|n| n.checked_mul(format.block_bytes()))
            .and_then(|n| n.checked_add(sum))
            .ok_or_else(overflow)
    })
}
/// Fills the top mip of a BC1 or BC3 chain from mip 1 with nearest sampling.
/// Each mip 1 block gives four top blocks with the same endpoints, so no block is decoded.
pub fn upscale_bc(block: usize, width: u32, height: u32, mip1: &[u8], top: &mut [u8]) {
    let (columns, rows) = (width as usize / 4, height as usize / 4);
    let source_columns = ((width as usize / 2).max(1)).div_ceil(4);
    // Two 2-bit color indices of one source row half become one 4-pixel row.
    let color = |row: u8, half: usize| {
        let pair = row >> (4 * half);
        let (a, b) = (pair & 3, (pair >> 2) & 3);
        a | a << 2 | b << 4 | b << 6
    };
    for y in 0..rows {
        for x in 0..columns {
            let source = &mip1[((y / 2) * source_columns + x / 2) * block..][..block];
            let target = &mut top[(y * columns + x) * block..][..block];
            let (half_x, half_y) = (x % 2, (y % 2) * 2);
            if block == 16 {
                // BC3 alpha: two endpoints, then 4 rows of four 3-bit indices.
                let mut bits = [0; 8];
                bits[..6].copy_from_slice(&source[2..8]);
                let indices = u64::from_le_bytes(bits);
                let mut out = 0u64;
                for row in 0..4 {
                    let line = indices >> (12 * (half_y + row / 2)) & 0xfff;
                    let pair = line >> (6 * half_x);
                    let (a, b) = (pair & 7, (pair >> 3) & 7);
                    out |= (a | a << 3 | b << 6 | b << 9) << (12 * row);
                }
                target[..2].copy_from_slice(&source[..2]);
                target[2..8].copy_from_slice(&out.to_le_bytes()[..6]);
            }
            let (source, target) = (&source[block - 8..], &mut target[block - 8..]);
            target[..4].copy_from_slice(&source[..4]);
            for row in 0..4 {
                target[4 + row] = color(source[4 + half_y + row / 2], half_x);
            }
        }
    }
}

pub fn validate_native_bc(texture: &super::TextureRecord) -> Result<usize, ReferenceError> {
    use super::{NativeBcFormat, TextureEncoding, TextureStorage};
    let TextureStorage::NativeBc {
        format,
        source_format,
        width,
        height,
        mip_count,
        image_count,
        dimension,
        mips_stripped,
        input,
        top,
    } = &texture.storage
    else {
        return Err(ReferenceError("Expected native BC storage".into()));
    };
    let normal = matches!(texture.slot.as_str(), "_BumpMap" | "_DetailNormalMap");
    require(
        (texture.encoding == texture.source_encoding
            && match texture.encoding {
                TextureEncoding::Rgba => !normal,
                // A DXT5nm normal keeps X in alpha and Y in green. The player shader uses R * A.
                TextureEncoding::Ag | TextureEncoding::Rag => {
                    normal && *format == NativeBcFormat::Bc3
                }
                _ => false,
            })
            && matches!(
                (format, source_format),
                (NativeBcFormat::Bc1, 10 | 28) | (NativeBcFormat::Bc3, 12 | 29)
            )
            && *image_count == 1
            && *dimension == 2
            && *mips_stripped == 0
            && width % 4 == 0
            && height % 4 == 0
            && *mip_count == texture.sampler.mip_count
            && *mip_count > 0
            && *mip_count <= 32 - width.max(height).leading_zeros()
            && texture.source.is_set(),
        "Native BC source gate",
    )?;
    let space = if texture.color_space == super::TextureColorSpace::Srgb {
        "srgb"
    } else {
        "linear"
    };
    require(
        input.file
            == format!(
                "reference/texture-{}-{}-{space}.bc",
                texture.source.file, texture.source.path_id
            ),
        "Native BC source identity",
    )?;
    input.validate()?;
    let bytes = bc_payload(*format, *width, *height, *mip_count)?;
    // A texture with more than one mip keeps its top mip in a second file.
    let top_bytes = match top {
        None => 0,
        Some(top) => {
            top.validate()?;
            require(
                top.file.strip_suffix(".top.bc") == input.file.strip_suffix(".bc")
                    && top.image_size.is_none()
                    && top.bytes as usize == bc_payload(*format, *width, *height, 1)?,
                "Native BC top mip receipt",
            )?;
            top.bytes as usize
        }
    };
    require(
        top.is_some() == (*mip_count > 1)
            && input.image_size.is_none()
            && input.bytes as usize + top_bytes == bytes,
        "Native BC receipt bytes",
    )?;
    Ok(bytes)
}
/// Validate metadata and receipts before scheduling any checked input.
/// The staging is the checked reader's copy of the raw input. The game decodes nothing.
pub fn primary_staging(
    texture: &super::TextureRecord,
    catalog: &FileCatalog,
) -> Result<usize, ReferenceError> {
    match &texture.storage {
        super::TextureStorage::Rgba8 { input, .. } => {
            let bytes = validate_rgba(texture)?;
            require(catalog.file(&input.file)? == input, "RGBA8 catalog receipt")?;
            Ok(bytes)
        }
        super::TextureStorage::NativeBc { input, top, .. } => {
            let bytes = validate_native_bc(texture)?;
            for receipt in std::iter::once(input).chain(top) {
                require(
                    catalog.file(&receipt.file)? == receipt,
                    "Native BC catalog receipt",
                )?;
            }
            Ok(bytes)
        }
    }
}

/// The final level vector, the reader input, and the upload copies.
pub fn primary_cpu(pixels: usize, max_staging: usize) -> Result<usize, ReferenceError> {
    pixels
        .checked_mul(2)
        .and_then(|n| n.checked_add(max_staging.checked_mul(2)?))
        .ok_or_else(overflow)
}
macro_rules! fixed_shaders {
    ($($name:literal),* $(,)?) => {
        pub const FIXED_SHADER_PATHS: [&str; 39] = [$(concat!("shaders/", $name)),*];
        /// The build includes the compiled shaders, so they always match its pipeline layouts.
        const FIXED_SHADER_SOURCES: [&str; 39] =
            [$(include_str!(concat!("../../../../web/assets/shaders/", $name))),*];
    };
}
fixed_shaders!(
    "source_lod.wgsl",
    "source_float_store.wgsl",
    "source_view_lighting.wgsl",
    "source_caster_math.wgsl",
    "source_world_caster.wgsl",
    "source_lighting.wgsl",
    "source_props.wgsl",
    "source_surface.wgsl",
    "source_mesh.wgsl",
    "source_glass.wgsl",
    "source_sky.wgsl",
    "source_cloud.wgsl",
    "source_render_sky.wgsl",
    "source_copy.wgsl",
    "source_dof.wgsl",
    "source_histogram.wgsl",
    "source_exposure.wgsl",
    "source_probe.wgsl",
    "source_caster.wgsl",
    "source_bloom.wgsl",
    "source_lut.wgsl",
    "source_lut_ldr.wgsl",
    "source_uber.wgsl",
    "source_grain.wgsl",
    "source_ao_depth.wgsl",
    "source_ao_render.wgsl",
    "source_ao_upsample.wgsl",
    "source_ao_composite.wgsl",
    "source_plate_text.wgsl",
    "source_grass_displacement.wgsl",
    "source_grass.wgsl",
    "source_terrain.wgsl",
    "source_impostor.wgsl",
    "source_particles.wgsl",
    "source_decals.wgsl",
    "source_depth_resolve.wgsl",
    "source_props_cull.wgsl",
    "source_motion.wgsl",
    "source_taa.wgsl",
);

pub fn shader_source(path: &str) -> Option<&'static str> {
    FIXED_SHADER_PATHS
        .iter()
        .position(|p| *p == path)
        .map(|i| FIXED_SHADER_SOURCES[i])
}

/// Source and parser copies, retained main/render shader data, and input scratch.
pub fn shader_input(path: &str) -> Result<FixedReservation, ReferenceError> {
    let bytes = shader_source(path)
        .ok_or_else(|| ReferenceError("unselected shader input".into()))?
        .len();
    require(bytes > 0, "empty selected shader input")?;
    Ok(FixedReservation {
        mesh_bytes: 0,
        cpu: bytes
            .checked_mul(18)
            .and_then(|n| n.checked_add(4096))
            .ok_or_else(overflow)?,
        gpu: 0,
        staging: bytes.checked_mul(2).ok_or_else(overflow)?,
    })
}

#[cfg(test)]
mod upscale_tests {
    #[test]
    fn upscale_repeats_each_mip1_pixel_on_2x2_top_pixels() {
        // An 8x8 BC3 top mip: one 4x4 mip 1 block gives four top blocks.
        let mut source = [0u8; 16];
        source[..2].copy_from_slice(&[200, 10]);
        // Alpha index of pixel i is i % 8.
        let alpha: u64 = (0..16).map(|i| (i % 8) << (3 * i)).sum();
        source[2..8].copy_from_slice(&alpha.to_le_bytes()[..6]);
        source[8..12].copy_from_slice(&[1, 2, 3, 4]);
        // Color index of pixel i is i % 4.
        let color: u32 = (0..16).map(|i| (i % 4) << (2 * i)).sum();
        source[12..].copy_from_slice(&color.to_le_bytes());
        let mut top = [0u8; 64];
        super::upscale_bc(16, 8, 8, &source, &mut top);
        for (y, x) in (0..8).flat_map(|y| (0..8).map(move |x| (y, x))) {
            let block = &top[((y / 4) * 2 + x / 4) * 16..][..16];
            let pixel = (y % 4) * 4 + x % 4;
            let expected = ((y / 2) * 4 + x / 2) as u64;
            let mut bits = [0; 8];
            bits[..6].copy_from_slice(&block[2..8]);
            assert_eq!(u64::from_le_bytes(bits) >> (3 * pixel) & 7, expected % 8);
            let color = u32::from_le_bytes(block[12..].try_into().unwrap());
            assert_eq!(color >> (2 * pixel) & 3, expected as u32 % 4);
            assert_eq!(&block[..2], &[200, 10]);
            assert_eq!(&block[8..12], &[1, 2, 3, 4]);
        }
    }
}
