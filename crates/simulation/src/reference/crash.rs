//! Checked crash rule: mainscript crash speeds, fall-off counts and the fall-off power table.
use super::*;

/// Source `mainscript.defFallOffChance` has 18 members.
pub const FALL_OFF_CATEGORIES: usize = 18;

/// Source `mainscript` crash fields. Speeds are km/h.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CrashReference {
    pub source: SourceId,
    /// `crashSpeedMinBlood`, `crashSpeedMaxBlood`.
    pub blood: [f32; 2],
    /// `crashSpeedMinHorn`, `crashSpeedMaxHorn`.
    pub horn: [f32; 2],
    /// `crashSpeedMinFallOff`, `crashSpeedMaxFallOff`.
    pub fall_off: [f32; 2],
    /// `crashSpeedMaxFallOffMax`.
    pub fall_off_max: f32,
    /// `crashHornChance`.
    pub horn_chance: f32,
    /// `minNumFallOff`..`maxNumFallOff` and `minNumFallOff2`..`maxNumFallOff2`.
    pub count: [[i32; 2]; 2],
    /// `fallOffPowerNeeded` by category. -1: the category never falls off.
    pub fall_off_power: [f32; FALL_OFF_CATEGORIES],
}

/// Source `carscript` crash multipliers.
#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CarCrashReference {
    pub multiplier: f32,
    pub blood: f32,
    pub horn: f32,
    pub fall_off: f32,
}

/// Source `carscript.blood`: the car `washtriggerscript`. Ids are prototype node indices.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CarBloodReference {
    pub node: u32,
    /// Source `objects`: `GotDirty` and `GotClean` set them active with the trigger node.
    pub objects: Vec<u32>,
    /// Source `CleanProperty`: the clean table row.
    pub permission: u8,
    pub never_repair: bool,
    /// Source `ON` at spawn.
    pub on: bool,
}

impl CrashReference {
    pub fn validate(&self) -> Result<(), ReferenceError> {
        let ranges = [self.blood, self.horn, self.fall_off];
        require(
            ranges.iter().all(|r| finite(r) && r[0] <= r[1])
                && finite(&[self.fall_off_max, self.horn_chance])
                && finite(&self.fall_off_power)
                && self.count.iter().all(|c| 0 <= c[0] && c[0] <= c[1]),
            "crash",
        )
    }
}

impl CarCrashReference {
    pub fn valid(&self) -> bool {
        finite(&[self.multiplier, self.blood, self.horn, self.fall_off])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn record(blood: &str, power: usize) -> String {
        let power = vec!["0.2"; power].join(",");
        format!(
            r#"{{"source":{{"file":"level1","path_id":1}},"blood":{blood},"horn":[10,20],
            "fall_off":[30,60],"fall_off_max":90,"horn_chance":0.5,"count":[[1,2],[2,4]],
            "fall_off_power":[{power}]}}"#
        )
    }

    #[test]
    fn crash_reference_reads_and_validates() {
        let ok: CrashReference = serde_json::from_str(&record("[40,80]", 18)).unwrap();
        ok.validate().unwrap();
        let reversed: CrashReference = serde_json::from_str(&record("[80,40]", 18)).unwrap();
        assert!(reversed.validate().is_err());
        assert!(serde_json::from_str::<CrashReference>(&record("[40,80]", 17)).is_err());
    }
}
