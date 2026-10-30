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

pub fn png_staging(encoded: usize, width: u32) -> Result<usize, ReferenceError> {
    encoded
        .checked_mul(3)
        .and_then(|n| n.checked_add(2 * 1024 * 1024))
        .and_then(|n| n.checked_add((width as usize).checked_mul(128)?))
        .ok_or_else(overflow)
}
/// Sequential chain overlap includes the retained base reader during each later read.
pub fn png_chain_staging(
    mips: &[super::TextureMip],
    mip_count: u32,
    catalog: &FileCatalog,
) -> Result<usize, ReferenceError> {
    require(
        !mips.is_empty() && mips.len() == mip_count as usize,
        "PNG chain mip count",
    )?;
    let mut previous: Option<(u32, u32)> = None;
    let mut base = 0;
    let mut staging = 0;
    for (index, mip) in mips.iter().enumerate() {
        let receipt = catalog.file(&mip.file)?;
        require(
            receipt.image_size == Some([mip.width, mip.height]),
            "PNG chain receipt dimensions",
        )?;
        if let Some((width, height)) = previous {
            require(
                (width > 1 || height > 1)
                    && mip.width == (width / 2).max(1)
                    && mip.height == (height / 2).max(1),
                "PNG chain mip progression",
            )?;
        }
        let encoded = receipt.bytes as usize;
        let input = png_staging(encoded, mip.width)?;
        if index == 0 {
            base = encoded;
            staging = input;
        } else {
            staging = staging.max(base.checked_add(input).ok_or_else(overflow)?);
        }
        previous = Some((mip.width, mip.height));
    }
    Ok(staging)
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
    } = &texture.storage
    else {
        return Err(ReferenceError("Expected native BC storage".into()));
    };
    require(
        texture.encoding == TextureEncoding::Rgba
            && texture.source_encoding == TextureEncoding::Rgba
            && matches!(
                (format, source_format),
                (NativeBcFormat::Bc1, 10 | 28) | (NativeBcFormat::Bc3, 12 | 29)
            )
            && *image_count == 1
            && *dimension == 2
            && *mips_stripped == 0
            && *mip_count == texture.sampler.mip_count
            && *mip_count == 32 - width.max(height).leading_zeros()
            && !texture.source.file.is_empty()
            && texture.source.path_id != 0,
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
    require(
        input.image_size.is_none() && input.bytes as usize == bytes,
        "Native BC receipt bytes",
    )?;
    Ok(bytes)
}
/// Validate metadata and receipts before scheduling any checked input.
pub fn primary_staging(
    texture: &super::TextureRecord,
    catalog: &FileCatalog,
) -> Result<usize, ReferenceError> {
    match &texture.storage {
        super::TextureStorage::PngChain { mips } => {
            png_chain_staging(mips, texture.sampler.mip_count, catalog)
        }
        super::TextureStorage::NativeBc { width, input, .. } => {
            let bytes = validate_native_bc(texture)?;
            require(
                catalog.file(&input.file)? == input,
                "Native BC catalog receipt",
            )?;
            png_staging(bytes, *width)
        }
    }
}

pub fn png_cpu(pixels: usize, max_staging: usize) -> Result<usize, ReferenceError> {
    pixels
        .checked_mul(2)
        .and_then(|n| n.checked_add(max_staging.checked_mul(2)?))
        .ok_or_else(overflow)
}
pub fn png_pixels(mips: &[super::TextureMip]) -> Result<usize, ReferenceError> {
    require(!mips.is_empty(), "fixed PNG mip count")?;
    mips.iter().try_fold(0usize, |sum, mip| {
        require(mip.width > 0 && mip.height > 0, "fixed PNG mip extent")?;
        (mip.width as usize)
            .checked_mul(mip.height as usize)
            .and_then(|n| n.checked_mul(4))
            .and_then(|n| n.checked_add(sum))
            .ok_or_else(overflow)
    })
}

macro_rules! fixed_shaders {
    ($($name:literal),* $(,)?) => {
        pub const FIXED_SHADER_PATHS: [&str; 40] = [$(concat!("shaders/", $name)),*];
        /// Shaders are compiled into the build, so they always match its pipeline layouts.
        const FIXED_SHADER_SOURCES: [&str; 40] =
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
    "source_heat_haze.wgsl",
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
    "source_motion_blur.wgsl",
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
mod tests {
    /// Every shader that the browser binds must be a fixed shader, or binding panics at start.
    #[test]
    fn bound_shaders_are_fixed() {
        let source = include_str!("../../../../src/presentation/fixed_assets.rs");
        for part in source.split("shader(\"").skip(1) {
            let path = &part[..part.find('"').unwrap()];
            assert!(
                super::FIXED_SHADER_PATHS.contains(&path),
                "{path} is not in FIXED_SHADER_PATHS"
            );
        }
    }
}
