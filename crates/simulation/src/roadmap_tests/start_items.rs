//! Headless scenario tests: start house items (SI rows).
use crate::world::stream::tests::kit::*;
use std::collections::HashMap;

/// SI1: each StartSpawnItems row spawns one item at its scene pose. After 5 s each item rests
/// within 0.5 m of its start. None falls through a shelf or the floor.
#[test]
#[ignore = "requires the installed source export"]
fn tp_si1_start_house_items_spawn_and_stay_in_place() {
    let (reference, mut run) = load_garage();
    settle(&mut run, &reference);
    let rows: Vec<_> = reference
        .world
        .startup
        .iter()
        .filter(|s| s.group == "StartSpawnItems")
        .map(|s| &s.source)
        .collect();
    assert!(!rows.is_empty(), "the export has StartSpawnItems rows");
    let roots = |run: &crate::starting_area::GarageRuntime| -> HashMap<_, _> {
        run.area
            .items
            .generated_bindings()
            .filter(|(id, _)| id.children.is_empty() && rows.contains(&&id.generator))
            .map(|(id, item)| (id.generator.clone(), item.pose.position))
            .collect()
    };
    let start = roots(&run);
    let missing: Vec<_> = rows.iter().filter(|s| !start.contains_key(**s)).collect();
    assert!(missing.is_empty(), "rows without an item: {missing:?}");
    seconds(&mut run, &reference, 5.);
    for (source, end) in roots(&run) {
        let a = start[&source];
        let moved = (0..3).map(|i| (end[i] - a[i]).powi(2)).sum::<f32>().sqrt();
        assert!(moved < 0.5, "{source:?} moved {moved} m");
    }
}
