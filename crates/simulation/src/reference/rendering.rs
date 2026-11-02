//! Source material bindings and authored texture levels.

use super::{AssetId, AssetRecord, AssetUsage, ReferenceError, SourceId, finite, require};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashSet};

pub const MATERIAL_TEXTURE_SLOTS: [&str; 9] = [
    "_MainTex",
    "_BumpMap",
    "_MetallicGlossMap",
    "_OcclusionMap",
    "_EmissionMap",
    "_ParallaxMap",
    "_DetailAlbedoMap",
    "_DetailMask",
    "_DetailNormalMap",
];
pub const MATERIAL_TEXTURE_BANKS: usize = 7;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RenderingRecord {
    pub materials: Vec<MaterialRecord>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ShaderFamily {
    Standard,
    UnlitColor,
    StainedGlass,
    AmplifyRope,
    PlayerModel,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MaterialRecord {
    pub asset: AssetId,
    pub gltf_material: u32,
    pub source: SourceId,
    pub shader_family: ShaderFamily,
    pub enable_instancing: bool,
    pub color: [f32; 4],
    pub condition: Option<MaterialCondition>,
    pub keywords: Vec<String>,
    pub state: MaterialState,
    pub properties: ShaderProperties,
}

impl MaterialRecord {
    /// Keep MainTex at bank zero for the existing alpha caster material.
    pub fn texture_banks(&self) -> Result<[Option<u8>; 9], ReferenceError> {
        let mut banks = [None; 9];
        let mut next = 1u8;
        for (slot, property) in MATERIAL_TEXTURE_SLOTS.iter().enumerate() {
            if !self
                .properties
                .textures
                .iter()
                .any(|texture| texture.slot == *property)
            {
                continue;
            }
            if slot == 0 {
                banks[slot] = Some(0);
                continue;
            }
            require(
                usize::from(next) < MATERIAL_TEXTURE_BANKS,
                &format!(
                    "material {}:{}: exceeds seven texture banks",
                    self.source.file, self.source.path_id
                ),
            )?;
            banks[slot] = Some(next);
            next += 1;
        }
        Ok(banks)
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MaterialCondition {
    pub source: SourceId,
    pub state: u8,
    pub quality: f32,
    pub range: Option<[u8; 2]>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AlphaMode {
    Opaque,
    Mask,
    Blend,
    Premultiply,
    Refract,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MaterialState {
    pub alpha_mode: AlphaMode,
    pub cull: u8,
    pub z_write: bool,
    pub z_test: u8,
    pub src_blend: u8,
    pub dst_blend: u8,
    pub render_queue: i32,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ShaderProperties {
    pub floats: BTreeMap<String, f32>,
    pub colors: BTreeMap<String, [f32; 4]>,
    pub textures: Vec<TextureRecord>,
    pub defaults: BTreeMap<String, TextureDefault>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TextureDefault {
    White,
    Grey,
    Black,
    Bump,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TextureColorSpace {
    Srgb,
    Linear,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TextureEncoding {
    Rgba,
    Rgb,
    Ag,
    Rag,
    Rg,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TextureRecord {
    pub slot: String,
    pub source: SourceId,
    pub storage: TextureStorage,
    pub color_space: TextureColorSpace,
    pub encoding: TextureEncoding,
    pub source_encoding: TextureEncoding,
    pub uv_channel: u8,
    pub scale: [f32; 2],
    pub offset: [f32; 2],
    pub sampler: SamplerRecord,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum TextureStorage {
    PngChain {
        mips: Vec<TextureMip>,
    },
    NativeBc {
        format: NativeBcFormat,
        source_format: u32,
        width: u32,
        height: u32,
        mip_count: u32,
        image_count: u32,
        dimension: u32,
        mips_stripped: u32,
        input: super::FileReceipt,
    },
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NativeBcFormat {
    Bc1,
    Bc3,
}
impl NativeBcFormat {
    pub fn block_bytes(self) -> usize {
        match self {
            Self::Bc1 => 8,
            Self::Bc3 => 16,
        }
    }
}
impl TextureRecord {
    pub fn file(&self) -> &str {
        match &self.storage {
            TextureStorage::PngChain { mips } => mips.first().map_or("", |m| m.file.as_str()),
            TextureStorage::NativeBc { input, .. } => &input.file,
        }
    }
    pub fn payload_bytes(&self) -> Result<usize, ReferenceError> {
        match &self.storage {
            TextureStorage::PngChain { mips } => super::fixed_reservation::png_pixels(mips),
            TextureStorage::NativeBc {
                format,
                width,
                height,
                mip_count,
                ..
            } => super::fixed_reservation::bc_payload(*format, *width, *height, *mip_count),
        }
    }
    pub fn retained_cpu_bytes(&self) -> usize {
        std::mem::size_of::<Self>()
            + self.slot.capacity()
            + self.source.file.capacity()
            + match &self.storage {
                TextureStorage::PngChain { mips } => {
                    mips.capacity() * std::mem::size_of::<TextureMip>()
                        + mips.iter().map(|m| m.file.capacity()).sum::<usize>()
                }
                TextureStorage::NativeBc { input, .. } => {
                    input.file.capacity() + input.sha256.capacity()
                }
            }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SamplerRecord {
    pub filter: u8,
    pub wrap_u: u8,
    pub wrap_v: u8,
    pub aniso: u8,
    pub mip_bias: f32,
    pub mip_count: u32,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TextureMip {
    pub file: String,
    pub width: u32,
    pub height: u32,
}

fn source(id: &SourceId) -> bool {
    !id.file.is_empty() && id.path_id != 0
}
pub(super) fn safe_path(path: &str) -> bool {
    path.starts_with("reference/")
        && !path.contains(['\\', ':', '?', '#', '\0'])
        && path
            .split('/')
            .all(|part| !part.is_empty() && part != "." && part != "..")
}
fn known_slot(slot: &str) -> bool {
    matches!(
        slot,
        "_MainTex"
            | "_BumpMap"
            | "_DetailAlbedoMap"
            | "_DetailMask"
            | "_DetailNormalMap"
            | "_MetallicGlossMap"
            | "_OcclusionMap"
            | "_EmissionMap"
            | "_ParallaxMap"
    )
}

impl RenderingRecord {
    pub fn validate(
        &self,
        assets: &[AssetRecord],
        sky_asset: AssetId,
    ) -> Result<(), ReferenceError> {
        let mut bindings = HashSet::new();
        for material in &self.materials {
            let label = format!(
                "material {}:{}",
                material.source.file, material.source.path_id
            );
            let asset = assets
                .iter()
                .find(|asset| asset.id == material.asset)
                .ok_or_else(|| ReferenceError(format!("{label}: missing asset")))?;
            require(
                bindings.insert((material.asset, material.gltf_material)),
                &format!("{label}: duplicate binding"),
            )?;
            require(source(&material.source) && finite(&material.color), &label)?;
            if let Some(condition) = &material.condition {
                require(
                    source(&condition.source)
                        && condition.state <= 4
                        && condition.quality.is_finite()
                        && (condition.quality - (1. - f32::from(condition.state) / 4.)).abs()
                            < 0.0001
                        && condition.range.is_none_or(|[a, b]| a <= b && b <= 4),
                    &format!("{label}: condition"),
                )?;
            }
            let player_source = material.source.file == "sharedassets0.assets"
                && matches!(material.source.path_id, 54 | 55 | 57..=75);
            require(
                !player_source || material.shader_family == ShaderFamily::PlayerModel,
                &format!("{label}: player shader family"),
            )?;
            let state = &material.state;
            require(
                state.cull <= 2
                    && state.z_test <= 8
                    && state.src_blend <= 10
                    && state.dst_blend <= 10
                    && (-1..=5000).contains(&state.render_queue),
                &format!("{label}: state"),
            )?;
            require(
                material
                    .properties
                    .floats
                    .values()
                    .all(|value| value.is_finite())
                    && material
                        .properties
                        .colors
                        .values()
                        .all(|value| finite(value)),
                &format!("{label}: properties"),
            )?;
            let keywords: HashSet<_> = material.keywords.iter().map(String::as_str).collect();
            require(
                keywords.len() == material.keywords.len(),
                &format!("{label}: duplicate keywords"),
            )?;
            require(
                material.shader_family == ShaderFamily::AmplifyRope
                    || material.properties.colors.contains_key("_Color"),
                &format!("{label}: _Color"),
            )?;
            let mut active = HashSet::new();
            match material.shader_family {
                ShaderFamily::Standard => {
                    require(
                        keywords.iter().all(|key| {
                            matches!(
                                *key,
                                "_NORMALMAP"
                                    | "_DETAIL_MULX2"
                                    | "_METALLICGLOSSMAP"
                                    | "_EMISSION"
                                    | "_PARALLAXMAP"
                                    | "_SMOOTHNESS_TEXTURE_ALBEDO_CHANNEL_A"
                                    | "_SPECULARHIGHLIGHTS_OFF"
                                    | "_GLOSSYREFLECTIONS_OFF"
                                    | "_ALPHATEST_ON"
                                    | "_ALPHABLEND_ON"
                                    | "_ALPHAPREMULTIPLY_ON"
                            )
                        }),
                        &format!("{label}: keyword"),
                    )?;
                    for field in [
                        "_Cutoff",
                        "_Glossiness",
                        "_GlossMapScale",
                        "_Metallic",
                        "_BumpScale",
                        "_OcclusionStrength",
                        "_DetailNormalMapScale",
                        "_UVSec",
                        "_SrcBlend",
                        "_DstBlend",
                        "_ZWrite",
                    ] {
                        require(
                            material.properties.floats.contains_key(field),
                            &format!("{label}: {field}"),
                        )?;
                    }
                    if keywords.contains("_EMISSION") {
                        require(
                            material.properties.colors.contains_key("_EmissionColor"),
                            &format!("{label}: _EmissionColor"),
                        )?;
                    }
                    if keywords.contains("_PARALLAXMAP") {
                        require(
                            material.properties.floats.contains_key("_Parallax"),
                            &format!("{label}: _Parallax"),
                        )?;
                    }
                    let alpha = if keywords.contains("_ALPHAPREMULTIPLY_ON") {
                        AlphaMode::Premultiply
                    } else if keywords.contains("_ALPHABLEND_ON") {
                        AlphaMode::Blend
                    } else if keywords.contains("_ALPHATEST_ON") {
                        AlphaMode::Mask
                    } else {
                        AlphaMode::Opaque
                    };
                    require(state.alpha_mode == alpha, &format!("{label}: alpha mode"))?;
                    active.extend(["_MainTex", "_OcclusionMap"]);
                    if keywords.contains("_NORMALMAP") {
                        active.insert("_BumpMap");
                    }
                    if keywords.contains("_DETAIL_MULX2") {
                        active.extend(["_DetailAlbedoMap", "_DetailMask"]);
                        if keywords.contains("_NORMALMAP") {
                            active.insert("_DetailNormalMap");
                        }
                    }
                    for (keyword, slot) in [
                        ("_METALLICGLOSSMAP", "_MetallicGlossMap"),
                        ("_EMISSION", "_EmissionMap"),
                        ("_PARALLAXMAP", "_ParallaxMap"),
                    ] {
                        if keywords.contains(keyword) {
                            active.insert(slot);
                        }
                    }
                }
                ShaderFamily::PlayerModel => {
                    require(
                        player_source
                            && matches!(
                                asset.usage,
                                AssetUsage::PlayerPrototype | AssetUsage::RagdollPrototype
                            ),
                        &format!("{label}: player prototype usage"),
                    )?;
                    let floats = &material.properties.floats;
                    let compact = matches!(material.source.path_id, 63 | 64);
                    let default_normal =
                        matches!(material.source.path_id, 54 | 55 | 58 | 62 | 63 | 64 | 67);
                    for field in [
                        "_Dist",
                        "_Cutoff",
                        "_BumpStrength",
                        "_Glossiness",
                        "_Metallic",
                        "_BumpScale",
                        "_DetailNormalMapScale",
                        "_DstBlend",
                        "_GlossMapScale",
                        "_GlossyReflections",
                        "_Mode",
                        "_OcclusionStrength",
                        "_Parallax",
                        "_SmoothnessTextureChannel",
                        "_SpecularHighlights",
                        "_SrcBlend",
                        "_UVSec",
                        "_ZWrite",
                    ] {
                        require(
                            floats.contains_key(field)
                                == (!compact
                                    || matches!(
                                        field,
                                        "_Dist"
                                            | "_Cutoff"
                                            | "_BumpStrength"
                                            | "_Glossiness"
                                            | "_Metallic"
                                    )),
                            &format!("{label}: {field}"),
                        )?;
                    }
                    require(
                        floats.keys().all(|field| {
                            matches!(
                                field.as_str(),
                                "_Dist"
                                    | "_Cutoff"
                                    | "_BumpStrength"
                                    | "_Glossiness"
                                    | "_Metallic"
                                    | "_BumpScale"
                                    | "_DetailNormalMapScale"
                                    | "_DstBlend"
                                    | "_GlossMapScale"
                                    | "_GlossyReflections"
                                    | "_Mode"
                                    | "_OcclusionStrength"
                                    | "_Parallax"
                                    | "_SmoothnessTextureChannel"
                                    | "_SpecularHighlights"
                                    | "_SrcBlend"
                                    | "_UVSec"
                                    | "_ZWrite"
                                    | "_Red"
                            )
                        }),
                        &format!("{label}: player properties"),
                    )?;
                    require(
                        (0. ..=10.).contains(&floats["_Dist"])
                            && (0. ..=1.).contains(&floats["_Cutoff"])
                            && (0. ..=5.).contains(&floats["_BumpStrength"])
                            && (0. ..=1.).contains(&floats["_Glossiness"])
                            && (0. ..=1.).contains(&floats["_Metallic"])
                            && floats.contains_key("_Red")
                                == matches!(material.source.path_id, 61 | 69)
                            && material.properties.colors.contains_key("_EmissionColor")
                                == !compact
                            && material
                                .properties
                                .colors
                                .keys()
                                .all(|field| matches!(field.as_str(), "_Color" | "_EmissionColor"))
                            && keywords.is_empty()
                            && !material.enable_instancing
                            && material.condition.is_none()
                            && if default_normal {
                                material.properties.defaults.len() == 1
                                    && material
                                        .properties
                                        .defaults
                                        .get("_BumpMap")
                                        .is_some_and(|v| *v == TextureDefault::Bump)
                            } else {
                                material.properties.defaults.is_empty()
                            }
                            && state.alpha_mode == AlphaMode::Mask
                            && state.cull == 2
                            && state.z_write
                            && state.z_test == 4
                            && state.src_blend == 1
                            && state.dst_blend == 0
                            && state.render_queue == -1,
                        &format!("{label}: player contract"),
                    )?;
                    active.extend(["_MainTex", "_BumpMap"]);
                    for texture in &material.properties.textures {
                        let (main, bump) = match material.source.path_id {
                            54 => (155, 0),
                            55 => (169, 0),
                            57 => (346, 168),
                            58 => (169, 0),
                            59 => (284, 168),
                            60 => (346, 168),
                            61 => (219, 168),
                            62 => (169, 0),
                            63 => (357, 0),
                            64 => (355, 0),
                            65 => (175, 168),
                            66 => (233, 168),
                            67 => (169, 0),
                            68 => (290, 168),
                            69 => (270, 168),
                            70 => (362, 236),
                            71 => (281, 350),
                            72 => (281, 350),
                            73 => (329, 236),
                            74 => (212, 225),
                            75 => (212, 225),
                            _ => (0, 0),
                        };
                        let expected = match texture.slot.as_str() {
                            "_MainTex" => main,
                            "_BumpMap" => bump,
                            _ => 0,
                        };
                        require(
                            texture.source.file == "sharedassets0.assets"
                                && texture.source.path_id == expected
                                && expected != 0
                                && texture.uv_channel == 0
                                && texture.color_space
                                    == if texture.slot == "_BumpMap" {
                                        TextureColorSpace::Linear
                                    } else {
                                        TextureColorSpace::Srgb
                                    }
                                && texture.source_encoding
                                    == if texture.slot == "_BumpMap" {
                                        TextureEncoding::Rag
                                    } else {
                                        TextureEncoding::Rgba
                                    },
                            &format!("{label}: player texture"),
                        )?;
                    }
                }
                ShaderFamily::StainedGlass => {
                    active.extend(["_MainTex", "_BumpMap"]);
                    require(
                        material.properties.floats.contains_key("_BumpAmt")
                            && state.alpha_mode == AlphaMode::Refract,
                        &label,
                    )?;
                }
                ShaderFamily::UnlitColor => {
                    require(state.alpha_mode == AlphaMode::Opaque, &label)?;
                }
                ShaderFamily::AmplifyRope => {
                    require(
                        material.keywords.is_empty()
                            && material.properties.colors.is_empty()
                            && material.properties.textures.is_empty()
                            && material.properties.defaults.is_empty()
                            && ["_Sag", "_SwaySpeed", "_SwayStrength"]
                                .iter()
                                .all(|key| material.properties.floats.contains_key(*key))
                            && state.alpha_mode == AlphaMode::Opaque,
                        &format!("{label}: rope contract"),
                    )?;
                }
            }
            material.texture_banks()?;
            let mut slots = HashSet::new();
            for texture in &material.properties.textures {
                let field = format!("{label}: {}", texture.slot);
                require(
                    known_slot(&texture.slot)
                        && active.contains(texture.slot.as_str())
                        && slots.insert(texture.slot.as_str()),
                    &field,
                )?;
                require(
                    (texture.source_encoding != TextureEncoding::Rag
                        || material.shader_family == ShaderFamily::PlayerModel)
                        && source(&texture.source)
                        && safe_path(texture.file())
                        && texture.uv_channel <= 1
                        && finite(&texture.scale)
                        && finite(&texture.offset),
                    &field,
                )?;
                let normal = matches!(texture.slot.as_str(), "_BumpMap" | "_DetailNormalMap");
                require(
                    texture.encoding
                        == if normal {
                            TextureEncoding::Rgb
                        } else {
                            TextureEncoding::Rgba
                        }
                        && (!normal || texture.color_space == TextureColorSpace::Linear),
                    &field,
                )?;
                let sampler = &texture.sampler;
                require(
                    sampler.filter <= 2
                        && sampler.wrap_u <= 3
                        && sampler.wrap_v <= 3
                        && sampler.aniso <= 16
                        && sampler.mip_bias.is_finite()
                        && sampler.mip_count > 0,
                    &field,
                )?;
                match &texture.storage {
                    TextureStorage::PngChain { mips } => {
                        require(mips.len() == sampler.mip_count as usize, &field)?;
                        let mut dimensions: Option<(u32, u32)> = None;
                        let mut paths = HashSet::new();
                        for mip in mips {
                            require(
                                safe_path(&mip.file)
                                    && paths.insert(&mip.file)
                                    && mip.width > 0
                                    && mip.height > 0
                                    && asset.dependencies.iter().any(|dependency| {
                                        dependency.file == mip.file
                                            && dependency.image_size
                                                == Some([mip.width, mip.height])
                                    }),
                                &field,
                            )?;
                            if let Some((width, height)) = dimensions {
                                require(
                                    (width > 1 || height > 1)
                                        && mip.width == (width / 2).max(1)
                                        && mip.height == (height / 2).max(1),
                                    &field,
                                )?;
                            }
                            dimensions = Some((mip.width, mip.height));
                        }
                    }
                    TextureStorage::NativeBc { input, .. } => {
                        super::fixed_reservation::validate_native_bc(texture)?;
                        require(
                            asset
                                .dependencies
                                .iter()
                                .any(|dependency| dependency == input),
                            &field,
                        )?;
                    }
                }
            }
            for (slot, default) in &material.properties.defaults {
                require(
                    known_slot(slot)
                        && active.contains(slot.as_str())
                        && slots.insert(slot.as_str()),
                    &label,
                )?;
                let expected = match slot.as_str() {
                    "_BumpMap" | "_DetailNormalMap" => TextureDefault::Bump,
                    "_DetailAlbedoMap" => TextureDefault::Grey,
                    "_ParallaxMap" => TextureDefault::Black,
                    _ => TextureDefault::White,
                };
                require(*default == expected, &format!("{label}: {slot} default"))?;
            }
            require(
                slots == active,
                &format!("{label}: missing active texture/default"),
            )?;
        }
        for asset in assets {
            if asset.id == sky_asset {
                continue;
            }
            let count = self
                .materials
                .iter()
                .filter(|record| record.asset == asset.id)
                .count();
            require(
                // Particle-only gibs have no meshes and no materials.
                (count > 0
                    || asset.material_count == 0
                    || asset.usage == super::AssetUsage::WorldPrototype)
                    && (asset.usage != super::AssetUsage::WorldPrototype
                        || count == asset.material_count as usize)
                    && (0..count as u32).all(|index| bindings.contains(&(asset.id, index))),
                "missing glTF material binding",
            )?;
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ResourceReference {
    pub source: SourceId,
    pub path: String,
    pub sha256: String,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ShaderReference {
    pub compiled_data: ResourceReference,
    pub source: SourceId,
    pub path: String,
    pub sha256: String,
    pub name: String,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LightingTextureChunk {
    pub data: ResourceReference,
    pub mip: u32,
    pub row: u32,
    pub width: u32,
    pub rows: u32,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LightingTextureRecord {
    pub batches: Vec<super::texture_batch::TextureBatchReference>,
    #[serde(default)]
    pub chunks: Vec<LightingTextureChunk>,
    pub mip_data: ResourceReference,
    pub source_data: ResourceReference,
    pub id: u32,
    pub source: SourceId,
    pub path: String,
    pub sha256: String,
    pub width: u32,
    pub height: u32,
    pub source_format: u32,
    pub sample_channel: String,
    pub color_space: u8,
    pub filter_mode: u8,
    pub wrap_u: u8,
    pub wrap_v: u8,
    pub mip_count: u32,
    pub aniso: u32,
    pub mip_bias: f32,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TextureLink {
    pub property: String,
    pub texture: u32,
    pub scale: [f32; 2],
    pub offset: [f32; 2],
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SkyMaterialReference {
    pub source: SourceId,
    pub shader: ShaderReference,
    pub source_data: ResourceReference,
    pub floats: BTreeMap<String, f32>,
    pub colors: BTreeMap<String, [f32; 4]>,
    pub textures: Vec<TextureLink>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LightingMaterialLink {
    pub asset: AssetId,
    pub gltf_material: u32,
    pub source: SourceId,
    pub condition_key: Option<String>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MaterialBindingReference {
    pub asset: AssetId,
    pub gltf_material: u32,
    pub source: SourceId,
}

impl LightingTextureRecord {
    /// Validate complete ordered rows before allocation or loading.
    pub fn validate_chunks(&self) -> Result<usize, ReferenceError> {
        require(
            self.width > 0
                && self.height > 0
                && self.mip_count > 0
                && self.mip_count <= 32 - self.width.max(self.height).leading_zeros(),
            "source texture dimensions",
        )?;
        let mut index = 0usize;
        let mut total = 0usize;
        for mip in 0..self.mip_count {
            let width = (self.width >> mip).max(1);
            let height = (self.height >> mip).max(1);
            total = total
                .checked_add(row_bytes(width, height)?)
                .ok_or_else(chunk_overflow)?;
            let mut row = 0u32;
            while row < height {
                let chunk = self
                    .chunks
                    .get(index)
                    .ok_or_else(|| ReferenceError("Missing source texture chunk".into()))?;
                require(
                    chunk.mip == mip
                        && chunk.row == row
                        && chunk.width == width
                        && chunk.rows > 0
                        && chunk.rows <= height - row
                        && row_bytes(width, chunk.rows)? <= 1024 * 1024
                        && chunk.data.source == self.source,
                    "source texture chunk coverage",
                )?;
                row = row.checked_add(chunk.rows).ok_or_else(chunk_overflow)?;
                index = index.checked_add(1).ok_or_else(chunk_overflow)?;
            }
        }
        require(index == self.chunks.len(), "extra source texture chunks")?;
        Ok(total)
    }

    pub fn chunk_range(&self, index: usize) -> Result<std::ops::Range<usize>, ReferenceError> {
        self.validate_chunks()?;
        self.validated_chunk_range(index)
    }

    fn validated_chunk_range(
        &self,
        index: usize,
    ) -> Result<std::ops::Range<usize>, ReferenceError> {
        let chunk = self
            .chunks
            .get(index)
            .ok_or_else(|| ReferenceError("Missing source texture chunk".into()))?;
        let mut start = 0usize;
        for mip in 0..chunk.mip {
            start = start
                .checked_add(row_bytes(
                    (self.width >> mip).max(1),
                    (self.height >> mip).max(1),
                )?)
                .ok_or_else(chunk_overflow)?;
        }
        start = start
            .checked_add(row_bytes(chunk.width, chunk.row)?)
            .ok_or_else(chunk_overflow)?;
        let end = start
            .checked_add(row_bytes(chunk.width, chunk.rows)?)
            .ok_or_else(chunk_overflow)?;
        Ok(start..end)
    }

    pub fn copy_chunk(
        &self,
        index: usize,
        pixels: &[u8],
        output: &mut [u8],
    ) -> Result<(), ReferenceError> {
        let total = self.validate_chunks()?;
        let range = self.validated_chunk_range(index)?;
        require(
            output.len() == total && pixels.len() == range.len(),
            "source texture chunk copy length",
        )?;
        output
            .get_mut(range)
            .ok_or_else(chunk_overflow)?
            .copy_from_slice(pixels);
        Ok(())
    }
}
fn row_bytes(width: u32, rows: u32) -> Result<usize, ReferenceError> {
    usize::try_from(width)
        .ok()
        .and_then(|w| w.checked_mul(usize::try_from(rows).ok()?))
        .and_then(|n| n.checked_mul(4))
        .ok_or_else(chunk_overflow)
}
fn chunk_overflow() -> ReferenceError {
    ReferenceError("Source texture chunk size overflow".into())
}

impl LightingTextureRecord {
    pub fn validate_chunk_receipts(
        &self,
        catalog: &super::FileCatalog,
    ) -> Result<usize, ReferenceError> {
        let pixels = self.validate_chunks()?;
        for (index, chunk) in self.chunks.iter().enumerate() {
            let receipt = catalog.file(&chunk.data.path)?;
            let bytes = self
                .validated_chunk_range(index)?
                .len()
                .checked_add(16)
                .ok_or_else(chunk_overflow)?;
            require(
                receipt.bytes as usize == bytes && receipt.sha256 == chunk.data.sha256,
                "fixed row file receipt",
            )?;
        }
        Ok(pixels)
    }
}
