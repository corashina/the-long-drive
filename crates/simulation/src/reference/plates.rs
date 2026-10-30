//! Checked source plate font and material contracts. Car prototype plates use them.
use super::*;
use std::collections::BTreeMap;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PlateRecord {
    pub font: PlateFontReference,
    pub material: PlateMaterialReference,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PlateLayoutReference {
    pub bounds: [f32; 2],
    pub pivot: [f32; 2],
    pub anchor_min: [f32; 2],
    pub anchor_max: [f32; 2],
    pub font_size: f32,
    pub font_size_base: f32,
    pub font_size_min: f32,
    pub font_size_max: f32,
    pub font_style: u32,
    pub font_weight: u32,
    pub horizontal_alignment: u32,
    pub vertical_alignment: u32,
    pub auto_sizing: bool,
    pub kerning: bool,
    pub word_wrapping: bool,
    pub extra_padding: bool,
    pub orthographic: bool,
    pub rich_text: bool,
    pub overflow_mode: u32,
    pub max_visible_descender: bool,
    pub spacing: [f32; 4],
    pub margin: [f32; 4],
    pub color: [f32; 4],
    pub transform: Transform,
}
impl PlateLayoutReference {
    /// Finite TextMeshPro values with the source alignment: Center=2, Middle=512, Bold.
    pub(super) fn valid(&self) -> bool {
        let l = self;
        finite(&l.bounds)
            && l.bounds.iter().all(|v| *v > 0.)
            && finite(&l.pivot)
            && finite(&l.anchor_min)
            && finite(&l.anchor_max)
            && finite(&[
                l.font_size,
                l.font_size_base,
                l.font_size_min,
                l.font_size_max,
            ])
            && l.font_size > 0.
            && finite(&l.spacing)
            && finite(&l.margin)
            && finite(&l.color)
            && transform(&l.transform)
            && matches!(l.horizontal_alignment, 1 | 2)
            && matches!(l.vertical_alignment, 256 | 512)
            && l.font_style == 1
            && l.font_weight == 400
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PlateFontReference {
    pub source: SourceId,
    pub name: String,
    pub version: String,
    pub family: String,
    pub style: String,
    /// Source FaceInfo numeric fields. Names omit the source m_ prefix.
    pub face: BTreeMap<String, f32>,
    pub glyphs: Vec<PlateGlyphReference>,
    pub characters: Vec<PlateCharacterReference>,
    pub pair_adjustments: Vec<PlatePairAdjustment>,
    pub atlas: PlateResourceReference,
    pub atlas_channel: String,
    pub atlas_extent: [u32; 2],
    pub padding: u32,
    pub atlas_render_mode: u32,
    pub normal_style: f32,
    pub normal_spacing: f32,
    pub bold_style: f32,
    pub bold_spacing: f32,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PlateGlyphReference {
    pub index: u32,
    pub atlas_index: u32,
    pub scale: f32,
    /// Width, height, bearing X, bearing Y, and advance in source font units.
    pub metrics: [f32; 5],
    /// X, Y, width, and height in source atlas texels.
    pub rect: [u32; 4],
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PlateCharacterReference {
    pub unicode: u32,
    pub glyph: u32,
    pub scale: f32,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PlateGlyphAdjustment {
    pub glyph: u32,
    /// Placement X/Y and advance X/Y in source font units.
    pub values: [f32; 4],
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PlatePairAdjustment {
    pub first: PlateGlyphAdjustment,
    pub second: PlateGlyphAdjustment,
    pub flags: u32,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PlateResourceReference {
    pub source: SourceId,
    pub path: String,
    pub sha256: String,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PlateMaterialReference {
    pub source: SourceId,
    pub shader: PlateResourceReference,
    pub compiled_shader: PlateResourceReference,
    pub shader_name: String,
    pub keywords: Vec<String>,
    pub disabled_passes: Vec<String>,
    pub state: MaterialState,
    pub floats: BTreeMap<String, f32>,
    pub colors: BTreeMap<String, [f32; 4]>,
    pub textures: Vec<TextureRecord>,
    pub resources: Vec<PlateResourceReference>,
    pub passes: Vec<String>,
    pub pass_states: Vec<PlateShaderPassReference>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PlateShaderPassReference {
    pub name: String,
    pub light_mode: String,
    pub color_mask: u8,
    pub state: MaterialState,
}
impl PlateRecord {
    pub fn validate(&self, manifest: &ReferenceManifest) -> Result<(), ReferenceError> {
        let valid_source = |s: &SourceId| {
            s.path_id != 0 && manifest.source_hashes.iter().any(|h| h.file == s.file)
        };
        let resource = |r: &PlateResourceReference| {
            valid_source(&r.source)
                && r.path.starts_with("reference/plate-")
                && !r.path.contains(['\\', ':', '?', '#', '\0'])
                && !r
                    .path
                    .split('/')
                    .any(|p| p.is_empty() || p == ".." || p == ".")
                && hash(&r.sha256)
                && manifest
                    .assets
                    .iter()
                    .flat_map(|a| &a.dependencies)
                    .any(|d| d.file == r.path && d.sha256 == r.sha256)
        };
        let f = &self.font;
        require(
            valid_source(&f.source)
                && f.source.file == "sharedassets0.assets"
                && f.source.path_id == 1366
                && f.atlas.source.file == "sharedassets0.assets"
                && f.atlas.source.path_id == 366
                && resource(&f.atlas)
                && f.atlas_channel == "alpha"
                && f.atlas_extent == [1024, 1024]
                && f.padding == 9
                && f.version == "1.1.0"
                && f.face.values().all(|v| v.is_finite())
                && f.face.get("PointSize").is_some_and(|v| *v > 0.)
                && f.face.get("Scale").is_some_and(|v| *v > 0.)
                && f.face.contains_key("Baseline")
                && finite(&[
                    f.normal_style,
                    f.normal_spacing,
                    f.bold_style,
                    f.bold_spacing,
                ]),
            "sharedassets0.assets:1366 font/atlas",
        )?;
        let glyph_ids = ids(f.glyphs.iter().map(|g| g.index), "duplicate plate glyph")?;
        let characters = ids(
            f.characters.iter().map(|c| c.unicode),
            "duplicate plate character",
        )?;
        require(
            (65..=90)
                .chain(48..=57)
                .chain([45])
                .all(|c| characters.contains(&c)),
            "sharedassets0.assets:1366 generated glyph coverage",
        )?;
        for g in &f.glyphs {
            require(
                g.atlas_index == 0
                    && g.scale.is_finite()
                    && g.scale > 0.
                    && finite(&g.metrics)
                    && g.metrics[0] >= 0.
                    && g.metrics[1] >= 0.
                    && u64::from(g.rect[0]) + u64::from(g.rect[2]) <= u64::from(f.atlas_extent[0])
                    && u64::from(g.rect[1]) + u64::from(g.rect[3]) <= u64::from(f.atlas_extent[1]),
                "plate glyph metrics/atlas bounds",
            )?;
        }
        require(
            f.characters
                .iter()
                .all(|c| glyph_ids.contains(&c.glyph) && c.scale.is_finite() && c.scale > 0.),
            "plate character mapping",
        )?;
        require(
            f.pair_adjustments.iter().all(|p| {
                [&p.first, &p.second]
                    .into_iter()
                    .all(|a| glyph_ids.contains(&a.glyph) && finite(&a.values))
            }),
            "plate pair adjustments",
        )?;
        let m = &self.material;
        require(
            valid_source(&m.source)
                && m.source.file == "sharedassets0.assets"
                && m.source.path_id == 18
                && m.shader.source.file == "sharedassets0.assets"
                && m.shader.source.path_id == 730
                && m.compiled_shader.source == m.shader.source
                && resource(&m.shader)
                && resource(&m.compiled_shader)
                && m.shader_name == "TextMeshPro/Distance Field (Surface)"
                && m.keywords.is_empty()
                && m.disabled_passes.is_empty()
                && m.passes == ["FORWARDBASE", "FORWARDADD", "SHADOWCASTER"]
                && m.pass_states.len() == 3
                && m.pass_states.iter().zip(&m.passes).all(|(p, mode)| {
                    p.light_mode == *mode
                        && p.color_mask <= 15
                        && p.state.cull <= 2
                        && p.state.z_test <= 8
                })
                && m.floats.values().all(|v| v.is_finite())
                && m.colors.values().all(|v| finite(v))
                && m.state.alpha_mode == AlphaMode::Blend
                && m.state.cull <= 2
                && m.state.z_test <= 8
                && m.state.src_blend == 5
                && m.state.dst_blend == 10
                && !m.state.z_write,
            "sharedassets0.assets:18 plate material/shader",
        )?;
        ids(
            m.resources.iter().map(|r| &r.path),
            "duplicate plate resource",
        )?;
        require(
            m.resources.iter().all(resource),
            "plate material dependency receipt",
        )?;
        let texture_slots = ids(
            m.textures.iter().map(|t| &t.slot),
            "duplicate plate texture slot",
        )?;
        require(
            ["_MainTex", "_FaceTex", "_OutlineTex", "_BumpMap"]
                .iter()
                .all(|slot| texture_slots.contains(&slot.to_string())),
            "plate named textures",
        )?;
        for t in &m.textures {
            let super::TextureStorage::PngChain { mips } = &t.storage else {
                return Err(ReferenceError("Plate requires PNG storage".into()));
            };
            require(
                valid_source(&t.source)
                    && !mips.is_empty()
                    && mips[0].file == t.file()
                    && finite(&t.scale)
                    && finite(&t.offset)
                    && t.sampler.filter <= 2
                    && t.sampler.wrap_u <= 3
                    && t.sampler.wrap_v <= 3
                    && t.sampler.mip_bias.is_finite()
                    && t.sampler.mip_count as usize == mips.len()
                    && mips.iter().all(|mip| {
                        mip.width > 0
                            && mip.height > 0
                            && m.resources
                                .iter()
                                .any(|r| r.path == mip.file && r.source == t.source)
                    }),
                "plate texture/sampler/mips",
            )?;
        }
        require(
            m.textures.iter().any(|t| {
                t.slot == "_MainTex" && t.source == f.atlas.source && t.file() == f.atlas.path
            }),
            "plate material atlas binding",
        )?;
        Ok(())
    }
}
