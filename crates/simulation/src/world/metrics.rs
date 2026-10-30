//! Retained world ownership and logical allocation reservations.
//! These charges are not process memory or driver residency measurements.
/// Name and unit of each `presentation_budgets` row.
pub const BUDGET_LABELS: [(&str, &str); 10] = [
    ("World CPU", "bytes"),
    ("World GPU", "bytes"),
    ("Asset CPU", "bytes"),
    ("Asset GPU", "bytes"),
    ("Objects", "objects"),
    ("Pending work", "requests"),
    ("Staging", "bytes"),
    ("Dependency staging", "bytes"),
    ("Generation staging", "bytes"),
    ("Jobs", "jobs"),
];

#[derive(Clone, Copy, Debug)]
pub struct ResourceBudget {
    pub name: &'static str,
    pub used: usize,
    pub limit: usize,
    pub unit: &'static str,
}

/// Live counts and renderer reservations. Excludes retained simulation and source data.
#[derive(Default)]
pub struct WorldPerformance {
    pub roads: usize,
    pub terrain: usize,
    pub objects: usize,
    pub jobs: usize,
    pub pending: usize,
    pub render_cpu_bytes: usize,
    pub render_gpu_bytes: usize,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, serde::Serialize)]
pub struct WorldMetrics {
    /// Actual grass worker linear-memory capacity, separate from logical buffers.
    pub grass_worker_wasm_bytes: usize,
    /// Subset of active CPU: installed grass datasets and shared domains.
    pub grass_worker_cache_bytes: usize,
    /// Subset of active CPU: estimated grass worker capacity high-water charge.
    pub grass_worker_reserved_bytes: usize,
    /// Subset of active CPU: imported item snapshots retained for change capture.
    pub imported_snapshot_bytes: usize,
    pub road_checkpoints: usize,
    pub building_checkpoints: usize,
    pub road_replay_ms: f64,
    pub peak_road_replay_ms: f64,
    /// Actual road worker linear-memory capacity. Separate from logical reservations.
    pub road_worker_wasm_bytes: usize,
    /// Subset of active CPU: estimated road worker capacity high-water charge.
    pub road_worker_reserved_bytes: usize,
    /// Subset of active CPU, retained immutable road worker sources.
    pub road_worker_cache_bytes: usize,
    pub generation_jobs: usize,
    pub road_segments: usize,
    pub near_tiles: usize,
    pub distant_tiles: [usize; 2],
    pub generated_objects: usize,
    pub pending_work: usize,
    pub pending_bytes: usize,
    pub active_cpu_bytes: usize,
    pub active_gpu_bytes: usize,
    pub prototype_cpu_bytes: usize,
    pub prototype_gpu_bytes: usize,
    pub checkpoint_bytes: usize,
    pub changed_record_bytes: usize,
    pub changed_objects: usize,
    pub spawn_parents: usize,
    pub spawn_boundaries: usize,
    pub spawn_boundary_bytes: usize,
    pub prepared_roads: u32,
    pub prepared_buildings: u32,
    pub last_road_work: usize,
}

/// Hash-table allocation bound includes spare buckets and control bytes.
pub fn map_bytes(capacity: usize, entry: usize) -> usize {
    if capacity == 0 {
        0
    } else {
        capacity
            .saturating_mul(2)
            .saturating_mul(entry + 1)
            .saturating_add(16)
    }
}

/// Reserve table growth before inserting a new owner. Include old/new table overlap.
pub fn map_insert_bytes<K, V>(map: &std::collections::HashMap<K, V>) -> usize {
    if map.len() < map.capacity() {
        0
    } else {
        map_bytes(
            map.capacity().saturating_add(1).saturating_mul(2),
            std::mem::size_of::<(K, V)>(),
        )
    }
}
