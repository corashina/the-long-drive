//! Checked displacement-camera producer and its selected source renderers.
use super::*;
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorldGrassDisplacement {
    pub camera: SourceId,
    pub follow: SourceId,
    pub displacement: SourceId,
    pub transform: SourceId,
    pub follow_target: SourceId,
    pub target: SourceId,
    pub follow_height: f32,
    pub border: f32,
    pub writers: Vec<WorldGrassWriter>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorldGrassWriter {
    pub transform: SourceId,
    pub owner: SourceId,
    pub renderer: SourceId,
    pub filter: SourceId,
    pub mesh: SourceId,
    pub materials: Vec<SourceId>,
    pub layer: u8,
    pub enabled: bool,
}
impl WorldGrassDisplacement {
    pub(super) fn validate(
        &self,
        records: &HashMap<&SourceId, &WorldSourceRecord>,
        receipts: &[WorldSourceReceipt],
    ) -> Result<(), ReferenceError> {
        let camera = checked_record(records, &self.camera, &["Camera"])?;
        let follow = checked_record(records, &self.follow, &["CameraFollow"])?;
        let displacement =
            checked_record(records, &self.displacement, &["RenderTextureDisplacement"])?;
        let transform = checked_record(records, &self.transform, &["Transform"])?;
        checked_record(records, &self.follow_target, &["Transform"])?;
        checked_record(records, &self.target, &["RenderTexture"])?;
        require(
            record_link(camera, "m_GameObject") == record_link(transform, "m_GameObject")
                && record_link(camera, "m_GameObject") == record_link(follow, "m_GameObject")
                && record_link(camera, "m_GameObject") == record_link(displacement, "m_GameObject")
                && record_link(camera, "m_TargetTexture") == Some(&self.target)
                && record_link(follow, "follow") == Some(&self.follow_target)
                && number(follow, "followHeight") == Some(self.follow_height)
                && number(displacement, "borderSmoothingArea") == Some(self.border)
                && self.follow_height.is_finite()
                && self.border.is_finite(),
            "Grass displacement producer links",
        )?;
        let mut seen = HashSet::new();
        for writer in &self.writers {
            let node = checked_record(records, &writer.transform, &["Transform"])?;
            let owner = checked_record(records, &writer.owner, &["GameObject"])?;
            let renderer = checked_record(records, &writer.renderer, &["MeshRenderer"])?;
            let filter = checked_record(records, &writer.filter, &["MeshFilter"])?;
            require(
                seen.insert(&writer.transform)
                    && writer.layer < 32
                    && record_link(node, "m_GameObject") == Some(&writer.owner)
                    && record_link(renderer, "m_GameObject") == Some(&writer.owner)
                    && record_link(filter, "m_GameObject") == Some(&writer.owner)
                    && record_link(filter, "m_Mesh") == Some(&writer.mesh)
                    && matches!(owner.fields.get("m_Layer"), Some(WorldValue::Integer(n)) if *n == writer.layer as i64)
                    && matches!(renderer.fields.get("m_Enabled"), Some(WorldValue::Bool(enabled)) if *enabled == writer.enabled)
                    && receipts.iter().any(|receipt| receipt.source == writer.mesh),
                "Grass writer ownership and layer",
            )?;
            let Some(WorldValue::Array(materials)) = renderer.fields.get("m_Materials") else {
                return Err(ReferenceError("Missing Grass writer materials".into()));
            };
            require(
                materials.len() == writer.materials.len()
                    && materials.iter().zip(&writer.materials).all(
                        |(value, source)| matches!(value, WorldValue::Link(link) if link == source),
                    ),
                "Grass writer material identities",
            )?;
            for source in &writer.materials {
                checked_record(records, source, &["Material"])?;
            }
        }
        Ok(())
    }
}
fn number(record: &WorldSourceRecord, name: &str) -> Option<f32> {
    match record.fields.get(name) {
        Some(WorldValue::Number(value)) => Some(*value as f32),
        Some(WorldValue::Integer(value)) => Some(*value as f32),
        _ => None,
    }
}
