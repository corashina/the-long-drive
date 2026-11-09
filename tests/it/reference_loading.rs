use thelongdrive::reference::{MAX_MANIFEST_BYTES, ReferenceManifest};

#[test]
#[ignore = "requires the installed source export"]
fn installed_startup_selects_only_garage_lada() {
    let bytes =
        thelongdrive::reference::read_manifest(thelongdrive::reference::INSTALLED_MANIFEST)
            .unwrap();
    let mut reference = ReferenceManifest::from_manifest(&bytes).unwrap();
    assert!(
        reference
            .assets
            .iter()
            .all(|asset| asset.path != "reference/skoda.gltf")
    );
    let packs = &mut reference.acquisition.startup_packs;
    assert_eq!(packs.len(), 1);
    assert_eq!(packs[0].vehicle.path_id, 77839);
    packs.clear();
    assert!(
        reference
            .validate()
            .unwrap_err()
            .to_string()
            .contains("selected packs")
    );
}

#[test]
#[ignore = "requires the installed source export"]
fn installed_manifest_checks_lod_bounds_and_initializes_runtime() {
    let bytes =
        thelongdrive::reference::read_manifest(thelongdrive::reference::INSTALLED_MANIFEST)
            .unwrap();
    let mut reference = ReferenceManifest::from_manifest(&bytes).unwrap();
    let index = reference
        .world
        .records
        .iter()
        .position(|r| r.class_name == "LODGroup")
        .unwrap();
    let fields = reference.world.records[index].fields.clone();
    let mut value = serde_json::to_value(&fields).unwrap();
    value
        .as_object_mut()
        .unwrap()
        .remove("m_LocalReferencePoint");
    let changed = serde_json::from_value(value).unwrap();
    std::sync::Arc::make_mut(&mut reference.world)
        .records
        .edit(|records| records[index].fields = changed);
    assert!(
        reference
            .validate()
            .unwrap_err()
            .to_string()
            .contains("m_LocalReferencePoint")
    );
    std::sync::Arc::make_mut(&mut reference.world)
        .records
        .edit(|records| records[index].fields = fields);
    let root = &reference.world.startup[0].source;
    let index = reference
        .world
        .records
        .iter()
        .position(|r| &r.source == root)
        .unwrap();
    let root = std::sync::Arc::make_mut(&mut reference.world)
        .records
        .edit(|records| records.swap_remove(index));
    assert!(reference.validate().is_err());
    std::sync::Arc::make_mut(&mut reference.world)
        .records
        .edit(|records| records.push(root));
    let evaluator =
        thelongdrive::world::terrain_evaluator(&reference.world, reference.world.terrain.seed);
    thelongdrive::world::StartupTerrain::new(
        &reference.world,
        &thelongdrive::world::DrawDistances::resolve(
            &reference.world.draw_distance,
            reference.world.draw_distance.default,
        ),
        &evaluator,
        &[],
    )
    .expect("installed startup frames initialize");
    let mut runtime = thelongdrive::starting_area::GarageRuntime::new(
        &reference,
        thelongdrive::world::GameMode::default(),
        reference.world.draw_distance.default,
    )
    .expect("installed world initializes");
    runtime.stream.prepare(0.);
    assert_eq!(runtime.stream.metrics().prepared_roads, 256);
    use thelongdrive::world::{ROAD_SEGMENTS, RoadIndex, RoadSequence};
    let seed = reference.world.terrain.seed;
    let mut small = RoadIndex::new(
        RoadSequence::new(&reference.world, seed, None).unwrap(),
        thelongdrive::world::terrain_evaluator(&reference.world, seed),
    );
    let mut large = RoadIndex::new(
        RoadSequence::new(&reference.world, seed, None).unwrap(),
        evaluator,
    );
    let start = std::time::Instant::now();
    while !small.prepare(8) {}
    let mut batches = 0;
    let mut peak = std::time::Duration::ZERO;
    loop {
        let batch_start = std::time::Instant::now();
        let ready = large.prepare(256);
        peak = peak.max(batch_start.elapsed());
        batches += 1;
        if ready {
            break;
        }
    }
    assert_eq!(batches, ROAD_SEGMENTS.div_ceil(256));
    assert_eq!(small.prepared_segments(), ROAD_SEGMENTS);
    assert_eq!(large.prepared_segments(), ROAD_SEGMENTS);
    assert_eq!(small.checkpoint_count(), large.checkpoint_count());
    for index in [0, 255, 256, 1031, ROAD_SEGMENTS - 1] {
        assert_eq!(small.segment(index).unwrap(), large.segment(index).unwrap());
    }
    eprintln!(
        "Both road indexes: {:?}; largest 256-section batch: {peak:?}",
        start.elapsed()
    );
}

#[test]
fn oversized_documents_stop_before_parsing() {
    let bytes = vec![b' '; MAX_MANIFEST_BYTES + 1];
    for result in [
        ReferenceManifest::from_runtime(&bytes),
        ReferenceManifest::from_manifest(&bytes),
    ] {
        assert_eq!(
            result.unwrap_err().to_string(),
            "Invalid reference field: manifest size"
        );
    }
}
