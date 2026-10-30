//! Source LOD quality and native crossfade producer inputs.
use super::*;
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorldLodReference {
    pub quality: SourceId,
    pub profile: u32,
    pub lod_bias: f32,
    pub maximum_lod: u32,
    pub dither: WorldDitherReference,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorldDitherReference {
    pub engine_sha256: String,
    pub creator_rva: u32,
    pub creator_sha256: String,
    pub binding_rva: u32,
    pub binding_sha256: String,
    pub property_slot: u32,
    pub property_name: String,
    pub alpha: [u8; 16],
}
impl WorldLodReference {
    pub(super) fn validate(
        &self,
        records: &HashMap<&SourceId, &WorldSourceRecord>,
    ) -> Result<(), ReferenceError> {
        let record = checked_record(records, &self.quality, &["QualitySettings"])?;
        require(
            matches!(record.fields.get("m_CurrentQuality"),Some(WorldValue::Integer(index))if *index==self.profile as i64),
            "Source LOD quality profile",
        )?;
        let Some(WorldValue::Array(profiles)) = record.fields.get("m_QualitySettings") else {
            return Err(ReferenceError("Missing source quality profiles".into()));
        };
        let Some(WorldValue::Record(profile)) = profiles.get(self.profile as usize) else {
            return Err(ReferenceError("Missing selected quality profile".into()));
        };
        require(
            matches!(profile.get("lodBias"),Some(WorldValue::Number(value))if *value as f32==self.lod_bias)
                && matches!(profile.get("maximumLODLevel"),Some(WorldValue::Integer(value))if *value==self.maximum_lod as i64)
                && self.lod_bias == 1.0
                && self.maximum_lod == 0,
            "Checked source LOD quality values",
        )?;
        let d = &self.dither;
        require(
            d.engine_sha256 == "553c1e7560e1b42bd4638c53d0168dcd1b4b9eb5b9df324b0a44f14b3f18bc86"
                && d.creator_rva == 0x372b80
                && d.creator_sha256
                    == "397f40191129798e57cb4beaa1ba9d881f25c140987f656a6b24c7614dfbbd1d"
                && d.binding_rva == 0x374580
                && d.binding_sha256
                    == "13b5c5a6eb2c0427b7a3181e1e4cce6678312f6e7523b04d14ad5b1fa30df7be"
                && d.property_slot == 18
                && d.property_name == "unity_DitherMask"
                && d.alpha
                    == [
                        0, 192, 48, 240, 128, 64, 176, 112, 32, 224, 16, 208, 160, 96, 144, 80,
                    ],
            "Native LOD dither provenance",
        )
    }
}
