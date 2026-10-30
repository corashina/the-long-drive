use super::*;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InventoryReference {
    pub source: SourceId,
    pub slot_count: u8,
    pub initial_slots: [Option<SourceId>; 4],
    pub selected_slot: u8,
    pub swap_gain: f32,
    pub starter_groups: [bool; 3],
    pub equip_hold_seconds: f32,
    pub hand_interpolation_factor: f32,
    pub hand_interpolation_seconds: f32,
    pub right_hand_anchor: SourceId,
    pub inputs: Vec<SourceInput>,
    pub evidence: ManagedEvidence,
}

impl InventoryReference {
    pub fn validate(&self, _manifest: &ReferenceManifest) -> Result<(), ReferenceError> {
        require(
            self.source.file == "level1"
                && self.source.path_id == 80125
                && self.slot_count == 4
                && self.initial_slots.iter().all(Option::is_none)
                && self.selected_slot == 0
                && self.swap_gain == 5.
                && self.starter_groups == [false; 3]
                && self.equip_hold_seconds == 0.25
                && self.hand_interpolation_factor == 20.
                && self.hand_interpolation_seconds == 1.
                && self.right_hand_anchor.file == "level1"
                && self.right_hand_anchor.path_id == 23118,
            "inventory startup",
        )?;
        require(
            self.inputs
                .iter()
                .map(|input| (input.action.as_str(), input.index, input.key.as_str()))
                .eq([
                    ("equiprecenter", 19, "Tab"),
                    ("removeSunglasses", 30, "X"),
                    ("inventory1", 31, "Alpha1"),
                    ("inventory2", 32, "Alpha2"),
                    ("inventory3", 33, "Alpha3"),
                    ("inventory4", 34, "Alpha4"),
                    ("smoke", 29, "CapsLock"),
                ]),
            "inventory inputs",
        )?;
        require(
            self.evidence.valid()
                && self.evidence.methods.iter().map(String::as_str).eq([
                    "private float rightHandLerp = 20f;",
                    "private float rightHandTime = 1f;",
                    "public static float holdButtonTime = 0.25f;",
                    "inventorySwapValue += inventorySwapGain * dt;",
                ]),
            "inventory source evidence",
        )
    }
}
