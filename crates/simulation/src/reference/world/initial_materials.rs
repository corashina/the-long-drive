//! Checked Start/RefreshLights material initialization before generated publication.
use super::*;
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorldInitialMaterial {
    pub controller: SourceId,
    pub renderer: SourceId,
    pub node: SourceId,
    pub material: SourceId,
}
impl WorldInitialMaterial {
    pub(super) fn validate(
        &self,
        records: &HashMap<&SourceId, &WorldSourceRecord>,
    ) -> Result<(), ReferenceError> {
        let controller = checked_record(records, &self.controller, &["headlightscript"])?;
        let renderer = checked_record(records, &self.renderer, &["MeshRenderer"])?;
        let node = checked_record(records, &self.node, &["Transform"])?;
        checked_record(records, &self.material, &["Material"])?;
        require(
            record_link(controller, "MOFF") == Some(&self.material)
                && record_link(renderer, "m_GameObject") == record_link(node, "m_GameObject"),
            "Initial material ownership",
        )?;
        let Some(WorldValue::Array(states)) = controller.fields.get("states") else {
            return Err(ReferenceError("Missing initial light states".into()));
        };
        require(
            matches!(
                controller.fields.get("currentState"),
                Some(WorldValue::Integer(0))
            ) && states.len() == 3,
            "Checked initial light state",
        )?;
        for (index, state) in states.iter().enumerate() {
            let WorldValue::Record(state) = state else {
                return Err(ReferenceError("Invalid initial light state".into()));
            };
            require(
                matches!(state.get("R"),Some(WorldValue::Link(id)) if id==&self.renderer)
                    && if index == 0 {
                        matches!(state.get("L"), Some(WorldValue::Empty))
                    } else {
                        matches!(state.get("L"), Some(WorldValue::Link(_)))
                    },
                "Checked RefreshLights assignment",
            )?;
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorldPaintingMaterial {
    pub material: SourceId,
    pub texture: SourceId,
    pub asset: AssetId,
}
