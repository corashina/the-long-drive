//! World counts and their caps.
/// Name and unit of each `budgets` row.
pub const BUDGET_LABELS: [(&str, &str); 7] = [
    ("Roads", "segments"),
    ("Near tiles", "tiles"),
    ("Distant tiles", "tiles"),
    ("Far tiles", "tiles"),
    ("Objects", "objects"),
    ("Pending work", "requests"),
    ("Jobs", "jobs"),
];

#[derive(Clone, Copy, Debug)]
pub struct ResourceBudget {
    pub name: &'static str,
    pub used: usize,
    pub limit: usize,
    pub unit: &'static str,
}

/// Live counts. Excludes retained simulation and source data.
#[derive(Default)]
pub struct WorldPerformance {
    pub roads: usize,
    pub terrain: usize,
    pub objects: usize,
    pub jobs: usize,
    pub pending: usize,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, serde::Serialize)]
pub struct WorldMetrics {
    pub road_checkpoints: usize,
    pub building_checkpoints: usize,
    pub road_replay_ms: f64,
    pub peak_road_replay_ms: f64,
    pub generation_jobs: usize,
    pub road_segments: usize,
    pub near_tiles: usize,
    pub distant_tiles: [usize; 2],
    pub generated_objects: usize,
    pub pending_work: usize,
    pub changed_objects: usize,
    pub spawn_boundaries: usize,
    pub prepared_roads: u32,
    pub prepared_buildings: u32,
    pub last_road_work: usize,
}
