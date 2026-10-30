//! Source TMP single-line glyph geometry in text-local coordinates.
use crate::reference::{
    PlateFontReference, PlateLayoutReference, PlateMaterialReference, ReferenceError,
};

#[derive(Clone, Copy, Debug)]
pub struct GlyphVertex {
    pub position: [f32; 3],
    pub normal: [f32; 3],
    pub uv0: [f32; 2],
    pub uv1: [f32; 2],
    pub color: [f32; 4],
}
#[derive(Debug)]
pub struct GlyphMesh {
    pub vertices: Vec<GlyphVertex>,
    pub indices: Vec<u32>,
}
fn error() -> ReferenceError {
    ReferenceError("invalid plate text layout".into())
}
pub fn layout_text(
    font: &PlateFontReference,
    layout: &PlateLayoutReference,
    material: &PlateMaterialReference,
    text: &str,
) -> Result<GlyphMesh, ReferenceError> {
    let face = |key: &str| font.face.get(key).copied().ok_or_else(error);
    let prop = |key: &str| material.floats.get(key).copied().ok_or_else(error);
    if font
        .face
        .values()
        .chain(material.floats.values())
        .any(|x| !x.is_finite())
        || layout
            .bounds
            .iter()
            .chain(&layout.pivot)
            .chain(&layout.anchor_min)
            .chain(&layout.anchor_max)
            .chain(&layout.margin)
            .chain(&layout.spacing)
            .chain(&layout.color)
            .chain(&layout.transform.translation)
            .chain(&layout.transform.rotation)
            .chain(&layout.transform.scale)
            .chain([
                &layout.font_size,
                &layout.font_size_base,
                &layout.font_size_min,
                &layout.font_size_max,
                &font.bold_style,
                &font.bold_spacing,
                &font.normal_style,
                &font.normal_spacing,
            ])
            .any(|x| !x.is_finite())
    {
        return Err(error());
    }
    if face("PointSize")? <= 0.0
        || face("Scale")? <= 0.0
        || layout.font_size <= 0.0
        || font.atlas_extent.contains(&0)
        || layout.bounds.iter().any(|v| *v <= 0.0)
        || layout.transform.scale.contains(&0.0)
    {
        return Err(error());
    }
    if layout.auto_sizing
        || layout.word_wrapping
        || layout.font_style & !1 != 0
        || !matches!(layout.horizontal_alignment, 1 | 2)
        || !matches!(layout.vertical_alignment, 256 | 512)
        || layout.spacing[2] != 0.0
        || layout.spacing[3] != 0.0
        || text.contains(['\n', '\r', '<', '>'])
    {
        return Err(error());
    }
    let scale = layout.font_size / face("PointSize")?
        * face("Scale")?
        * if layout.orthographic { 1.0 } else { 0.1 };
    let bold = layout.font_style & 1 != 0;
    let gradient = prop("_GradientScale")?;
    if gradient <= 0.0 {
        return Err(error());
    }
    // ShaderUtilities.UpdateShaderRatios recomputes A unless RATIOS_OFF is active.
    let ratio = if material.keywords.iter().any(|k| k == "RATIOS_OFF") {
        1.0
    } else {
        (gradient - 1.0)
            / (gradient
                * (prop("_WeightNormal")?.max(prop("_WeightBold")?) / 4.0
                    + prop("_FaceDilate")?
                    + prop("_OutlineWidth")?
                    + prop("_OutlineSoftness")?)
                .max(1.0))
    };
    let extent =
        (prop("_FaceDilate")? + prop("_OutlineWidth")? + prop("_OutlineSoftness")?) * ratio;
    let padding =
        (extent + if layout.extra_padding { 4.0 } else { 0.0 }).min(1.0) * gradient + 1.25;
    let style = if bold {
        font.bold_style
    } else {
        font.normal_style
    } / 4.0
        * gradient
        * ratio;
    let padding = padding.min(gradient - style);
    let expand = padding + style;
    let mut mesh = GlyphMesh {
        vertices: Vec::with_capacity(text.chars().count() * 4),
        indices: Vec::with_capacity(text.chars().count() * 6),
    };
    let glyphs: Vec<_> = text
        .chars()
        .map(|ch| {
            let c = font
                .characters
                .iter()
                .find(|c| c.unicode == ch as u32)
                .ok_or_else(error)?;
            let g = font
                .glyphs
                .iter()
                .find(|g| g.index == c.glyph)
                .ok_or_else(error)?;
            if !c.scale.is_finite()
                || c.scale <= 0.0
                || !g.scale.is_finite()
                || g.scale <= 0.0
                || g.metrics.iter().any(|v| !v.is_finite())
                || g.metrics[0] < 0.0
                || g.metrics[1] < 0.0
                || g.atlas_index != 0
                || u64::from(g.rect[0]) + u64::from(g.rect[2]) > u64::from(font.atlas_extent[0])
                || u64::from(g.rect[1]) + u64::from(g.rect[3]) > u64::from(font.atlas_extent[1])
            {
                return Err(error());
            }
            Ok((c, g))
        })
        .collect::<Result<_, _>>()?;
    let mut advance = 0.0;
    let mut last_spacing = 0.0;
    let mut ascender = f32::NEG_INFINITY;
    let mut descender = f32::INFINITY;
    for (i, (character, glyph)) in glyphs.iter().enumerate() {
        let s = scale * character.scale * glyph.scale;
        let mut adjust = [0.0; 4];
        let mut char_spacing = layout.spacing[0];
        if layout.kerning {
            if let Some((_, next)) = glyphs.get(i + 1) {
                if let Some(p) = font
                    .pair_adjustments
                    .iter()
                    .find(|p| p.first.glyph == glyph.index && p.second.glyph == next.index)
                {
                    for (a, b) in adjust.iter_mut().zip(p.first.values) {
                        *a += b;
                    }
                    if p.flags & 0x100 != 0 {
                        char_spacing = 0.0;
                    }
                }
            }
            if i > 0 {
                let previous = glyphs[i - 1].1;
                if let Some(p) = font
                    .pair_adjustments
                    .iter()
                    .find(|p| p.first.glyph == previous.index && p.second.glyph == glyph.index)
                {
                    for (a, b) in adjust.iter_mut().zip(p.second.values) {
                        *a += b;
                    }
                    if p.flags & 0x100 != 0 {
                        char_spacing = 0.0;
                    }
                }
            }
        }
        if adjust.iter().any(|v| !v.is_finite()) {
            return Err(error());
        }
        let [width, height, bearing_x, bearing_y, horizontal_advance] = glyph.metrics;
        let left = advance + (bearing_x - expand + adjust[0]) * s;
        let top =
            face("Baseline")? * scale * face("Scale")? + (bearing_y + padding + adjust[1]) * s;
        let bottom = top - (height + 2.0 * padding) * s;
        let right = left + (width + 2.0 * expand) * s;
        ascender = ascender.max(face("AscentLine")? * s);
        descender = descender.min(face("DescentLine")? * s);
        let [x, y, w, h] = glyph.rect.map(|v| v as f32);
        let [aw, ah] = font.atlas_extent.map(|v| v as f32);
        let uv_left = (x - expand) / aw;
        let uv_right = (x + w + expand) / aw;
        let uv_bottom = (y - expand) / ah;
        let uv_top = (y + h + expand) / ah;
        let base = mesh.vertices.len() as u32;
        for (corner, (position, uv0)) in [
            ([left, bottom, 0.0], [uv_left, uv_bottom]),
            ([right, bottom, 0.0], [uv_right, uv_bottom]),
            ([right, top, 0.0], [uv_right, uv_top]),
            ([left, top, 0.0], [uv_left, uv_top]),
        ]
        .into_iter()
        .enumerate()
        {
            // TMP PackUV stores two 9-bit coordinates.
            let packed = [0.0, 511.0 * 4096.0, 511.0 * 4096.0 + 511.0, 511.0][corner];
            mesh.vertices.push(GlyphVertex {
                position,
                normal: [0.0, 0.0, 1.0],
                uv0,
                uv1: [packed, if bold { -s } else { s }],
                color: layout.color,
            });
        }
        mesh.indices
            .extend([base, base + 1, base + 2, base, base + 2, base + 3]);
        last_spacing =
            (font.normal_spacing + char_spacing + if bold { font.bold_spacing } else { 0.0 })
                * layout.font_size
                * 0.01
                * if layout.orthographic { 1.0 } else { 0.1 };
        advance += (horizontal_advance + adjust[2]) * s + last_spacing;
        if character.unicode == 32 {
            advance += layout.spacing[1] * scale;
        }
    }
    if !mesh.vertices.is_empty() {
        let max_advance = advance - last_spacing;
        // TMP alignment: 1 left, 2 center; 256 top, 512 middle. Margins are left, top, right, bottom.
        let [w, h] = layout.bounds;
        let [left, top, right, bottom] = layout.margin;
        let dx = if layout.horizontal_alignment == 1 {
            -layout.pivot[0] * w + left
        } else {
            (0.5 - layout.pivot[0]) * w + (left - right - max_advance) * 0.5
        };
        let dy = if layout.vertical_alignment == 256 {
            (1.0 - layout.pivot[1]) * h - top - ascender
        } else {
            (0.5 - layout.pivot[1]) * h - (ascender + descender + top - bottom) * 0.5
        };
        for v in &mut mesh.vertices {
            v.position[0] += dx;
            v.position[1] += dy;
        }
    }
    if mesh.vertices.iter().any(|v| {
        v.position
            .iter()
            .chain(&v.normal)
            .chain(&v.uv0)
            .chain(&v.uv1)
            .any(|x| !x.is_finite())
    }) {
        return Err(error());
    }
    Ok(mesh)
}

#[derive(Clone, Copy, Debug)]
pub struct SdfParameters {
    pub face_color: [f32; 4],
    pub outline_color: [f32; 4],
    pub face_dilate: f32,
    pub outline_width: f32,
    pub outline_softness: f32,
    pub gradient_scale: f32,
    pub perspective_filter: f32,
    pub scale_ratio_a: f32,
    pub weight: f32,
}
pub fn sdf_parameters(
    material: &PlateMaterialReference,
    layout: &PlateLayoutReference,
) -> SdfParameters {
    let p = &material.floats;
    SdfParameters {
        face_color: material.colors["_FaceColor"],
        outline_color: material.colors["_OutlineColor"],
        face_dilate: p["_FaceDilate"],
        outline_width: p["_OutlineWidth"],
        outline_softness: p["_OutlineSoftness"],
        gradient_scale: p["_GradientScale"],
        perspective_filter: p["_PerspectiveFilter"],
        scale_ratio_a: p["_ScaleRatioA"],
        weight: p[if layout.font_style & 1 != 0 {
            "_WeightBold"
        } else {
            "_WeightNormal"
        }],
    }
}
