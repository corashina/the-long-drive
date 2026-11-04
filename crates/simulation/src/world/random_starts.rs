//! Source `TerrainGenerationSettings.randomStarts`. A new game activates one random option per group:
//! the bike kit in the garage (Bike01 with its basket, or Bike03 with its bags), its tank and lamp
//! pair, and a toy.
use super::UnityRandom;
use crate::reference::{WorldReference, WorldStartupReference};

/// Startup group of the random start item copies.
pub const GROUP: &str = "randomStarts";

/// The picked option of each group, in source group order.
/// The picks come from the world seed, so a loaded game picks the same options.
pub fn pick(world: &WorldReference, seed: i32) -> Vec<u32> {
    let mut counts = Vec::new();
    for [group, option] in world.startup.iter().flat_map(|row| &row.random_options) {
        let group = *group as usize;
        if counts.len() <= group {
            counts.resize(group + 1, 0);
        }
        counts[group] = counts[group].max(option + 1);
    }
    let mut random = UnityRandom::new(seed);
    // Source: `objs[Random.Range(0, objs.Length)].SetActive(true)` for each group in order.
    counts
        .iter()
        .map(|count| random.range_int(0, *count as i32) as u32)
        .collect()
}

/// The row spawns: each of its options is picked.
pub fn picked(row: &WorldStartupReference, picks: &[u32]) -> bool {
    row.random_options
        .iter()
        .all(|[group, option]| picks.get(*group as usize) == Some(option))
}
