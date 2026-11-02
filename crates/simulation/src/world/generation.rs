//! Completeness describes an explicit prefix, never unknown future geometry.
use super::{PlacementIndexSnapshot, RoadIndexSnapshot, WorldBounds};
use crate::reference::ReferenceError;
use serde::{Deserialize, Serialize};
pub const GENERATOR_CONTRACT: u32 = 1;
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct GenerationCoverage {
    pub road_end: u32,
    pub building_end: u32,
    pub revision: u64,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[must_use]
pub enum QueryStatus {
    Pending,
    Complete,
}
impl QueryStatus {
    pub fn for_prefix(available: u32, required: u32) -> Self {
        if available >= required {
            Self::Complete
        } else {
            Self::Pending
        }
    }
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GenerationSnapshot {
    pub generator_contract: u32,
    pub package_id: String,
    pub roads: RoadIndexSnapshot,
    pub placements: PlacementIndexSnapshot,
}
impl GenerationSnapshot {
    pub fn validate_identity(&self, package: &str) -> Result<(), ReferenceError> {
        if self.generator_contract != GENERATOR_CONTRACT || self.package_id != package {
            Err(invalid())
        } else {
            Ok(())
        }
    }
}
pub fn invalid() -> ReferenceError {
    ReferenceError("Invalid saved generation frontier".into())
}
pub fn valid_bounds(b: WorldBounds) -> bool {
    (0..3).all(|a| b.min[a].is_finite() && b.max[a].is_finite() && b.min[a] <= b.max[a])
}
