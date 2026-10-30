//! Checked little-endian immutable datasets and captured views.
use super::{GrassPatch, GrassSettings, GrassView};
use glam::{Vec2, Vec3, Vec4};

pub fn dataset_encoded_len(patches: &[GrassPatch]) -> Result<usize, String> {
    u32::try_from(patches.len()).map_err(|_| "Grass patch count exceeds its address range")?;
    patches
        .len()
        .checked_mul(60)
        .and_then(|bytes| bytes.checked_add(36))
        .ok_or_else(|| "Grass dataset size overflow".into())
}
pub fn encode_dataset(patches: &[GrassPatch], settings: GrassSettings, out: &mut Vec<u8>) {
    out.clear();
    out.extend(settings.edge_length.to_le_bytes());
    out.extend(settings.max_tessellation.to_le_bytes());
    out.extend(settings.max_height.to_le_bytes());
    out.extend(settings.fade_end.to_le_bytes());
    out.extend(settings.lod_start.to_le_bytes());
    out.extend(settings.lod_end.to_le_bytes());
    out.extend(settings.lod_max.to_le_bytes());
    out.extend(u32::from(settings.smooth_domain).to_le_bytes());
    out.extend(
        u32::try_from(patches.len())
            .expect("admitted Grass patch count")
            .to_le_bytes(),
    );
    for patch in patches {
        for position in patch.positions {
            for value in position.to_array() {
                out.extend(value.to_le_bytes());
            }
        }
        for uv in patch.uvs {
            for value in uv.to_array() {
                out.extend(value.to_le_bytes());
            }
        }
    }
}
pub fn encode_view(view: GrassView, out: &mut Vec<u8>) {
    out.clear();
    for value in view.position.to_array() {
        out.extend(value.to_le_bytes());
    }
    out.extend(view.screen_height.to_le_bytes());
    for plane in view.clip_planes {
        for value in plane.to_array() {
            out.extend(value.to_le_bytes());
        }
    }
}
#[derive(Clone, Copy)]
struct Reader<'a>(&'a [u8]);
impl Reader<'_> {
    fn u32(&mut self) -> Result<u32, String> {
        let (head, tail) = self
            .0
            .split_first_chunk::<4>()
            .ok_or("Grass input is truncated")?;
        self.0 = tail;
        Ok(u32::from_le_bytes(*head))
    }
    fn f32(&mut self) -> Result<f32, String> {
        let value = f32::from_bits(self.u32()?);
        if !value.is_finite() {
            return Err("Grass input contains a nonfinite value".into());
        }
        Ok(value)
    }
    fn floats<const N: usize>(&mut self) -> Result<[f32; N], String> {
        let mut values = [0.0; N];
        for value in &mut values {
            *value = self.f32()?;
        }
        Ok(values)
    }
    fn settings(&mut self) -> Result<GrassSettings, String> {
        let edge_length = self.f32()?;
        let max_tessellation = self.u32()?;
        let max_height = self.f32()?;
        let fade_end = self.f32()?;
        let lod_start = self.f32()?;
        let lod_end = self.f32()?;
        let lod_max = self.u32()?;
        let smooth = self.u32()?;
        // Preserve the checked source material constraints.
        if !matches!(max_tessellation, 2 | 4) || !(1..=6).contains(&lod_max) || smooth > 1 {
            return Err("Unsupported checked Grass settings".into());
        }
        Ok(GrassSettings {
            edge_length,
            max_tessellation,
            max_height,
            fade_end,
            lod_start,
            lod_end,
            lod_max,
            smooth_domain: smooth != 0,
        })
    }
    fn patch(&mut self) -> Result<GrassPatch, String> {
        Ok(GrassPatch {
            positions: [
                Vec3::from_array(self.floats()?),
                Vec3::from_array(self.floats()?),
                Vec3::from_array(self.floats()?),
            ],
            uvs: [
                Vec2::from_array(self.floats()?),
                Vec2::from_array(self.floats()?),
                Vec2::from_array(self.floats()?),
            ],
        })
    }
    fn done(self) -> Result<(), String> {
        if !self.0.is_empty() {
            return Err("Grass input has trailing bytes".into());
        }
        Ok(())
    }
}
/// Validate all values and counts before allocating decoded patches.
pub fn decode_dataset(input: &[u8]) -> Result<(Vec<GrassPatch>, GrassSettings), String> {
    let mut reader = Reader(input);
    let settings = reader.settings()?;
    let count = reader.u32()? as usize;
    let bytes = count
        .checked_mul(60)
        .ok_or("Grass patch byte count overflow")?;
    count
        .checked_mul(std::mem::size_of::<GrassPatch>())
        .filter(|bytes| *bytes <= isize::MAX as usize)
        .ok_or("Grass patch allocation overflow")?;
    super::GrassExpansion::planning_bytes(count)?;
    if bytes != reader.0.len() {
        return Err("Grass patch count does not match its input".into());
    }
    let mut validate = reader;
    for _ in 0..count {
        validate.patch()?;
    }
    validate.done()?;
    let mut patches = Vec::with_capacity(count);
    for _ in 0..count {
        patches.push(reader.patch()?);
    }
    reader.done()?;
    Ok((patches, settings))
}
pub fn decode_view(input: &[u8]) -> Result<GrassView, String> {
    let mut reader = Reader(input);
    let view = GrassView {
        position: Vec3::from_array(reader.floats()?),
        screen_height: reader.f32()?,
        clip_planes: [
            Vec4::from_array(reader.floats()?),
            Vec4::from_array(reader.floats()?),
            Vec4::from_array(reader.floats()?),
            Vec4::from_array(reader.floats()?),
        ],
    };
    reader.done()?;
    Ok(view)
}
