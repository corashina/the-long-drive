use the_long_slop::reference::{MAX_MANIFEST_BYTES, ReferenceManifest};

#[test]
#[ignore = "requires the installed source export"]
fn installed_startup_selects_only_garage_lada() {
    let bytes = std::fs::read("web/assets/reference/manifest.json").unwrap();
    ReferenceManifest::from_runtime_json(&bytes).unwrap();
    let mut manifest: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert!(
        manifest["assets"]
            .as_array()
            .unwrap()
            .iter()
            .all(|asset| { asset["path"] != "reference/skoda.gltf" })
    );
    let packs = manifest["acquisition"]["startup_packs"]
        .as_array_mut()
        .unwrap();
    assert_eq!(packs.len(), 1);
    assert_eq!(packs[0]["vehicle"]["path_id"], 77839);
    packs.clear();
    let bytes = serde_json::to_vec(&manifest).unwrap();
    assert!(
        ReferenceManifest::from_runtime_json(&bytes)
            .unwrap_err()
            .to_string()
            .contains("selected packs")
    );
}

#[test]
#[ignore = "requires the installed source export"]
fn installed_manifest_checks_lod_bounds_and_initializes_runtime() {
    let bytes = std::fs::read("web/assets/reference/manifest.json").unwrap();
    let mut reference = ReferenceManifest::from_runtime_json(&bytes).unwrap();
    let index = reference
        .world
        .records
        .iter()
        .position(|r| r.class_name == "LODGroup")
        .unwrap();
    let fields = reference.world.records[index].fields.clone();
    let mut value = serde_json::to_value(&fields).unwrap();
    value
        .as_array_mut()
        .unwrap()
        .retain(|f| f["name"] != "m_LocalReferencePoint");
    std::sync::Arc::make_mut(&mut reference.world).records[index].fields =
        serde_json::from_value(value).unwrap();
    assert!(
        reference
            .validate()
            .unwrap_err()
            .to_string()
            .contains("m_LocalReferencePoint")
    );
    std::sync::Arc::make_mut(&mut reference.world).records[index].fields = fields;
    let root = &reference.world.startup[0].source;
    let index = reference
        .world
        .records
        .iter()
        .position(|r| &r.source == root)
        .unwrap();
    let root = std::sync::Arc::make_mut(&mut reference.world)
        .records
        .swap_remove(index);
    assert!(reference.validate().is_err());
    std::sync::Arc::make_mut(&mut reference.world)
        .records
        .push(root);
    let evaluator =
        the_long_slop::world::terrain_evaluator(&reference.world, reference.world.terrain.seed);
    the_long_slop::world::StartupTerrain::new(
        &reference.world,
        &the_long_slop::world::DrawDistances::resolve(
            &reference.world.draw_distance,
            reference.world.draw_distance.default,
        ),
        &evaluator,
        &[],
    )
    .expect("installed startup frames initialize");
    let mut runtime = the_long_slop::starting_area::GarageRuntime::new(
        &reference,
        the_long_slop::world::GameMode::default(),
        reference.world.draw_distance.default,
    )
    .expect("installed world initializes");
    use the_long_slop::world::PresentationAllocation;
    let allocation = runtime.stream.presentation_credit();
    let mib = 1024 * 1024;
    for (name, next) in [
        (
            "World CPU",
            PresentationAllocation {
                active_cpu: 128 * mib + 1,
                ..allocation
            },
        ),
        (
            "World GPU",
            PresentationAllocation {
                active_gpu: 128 * mib + 1,
                ..allocation
            },
        ),
        (
            "Asset CPU",
            PresentationAllocation {
                prototype_cpu: 256 * mib + 1,
                ..allocation
            },
        ),
        (
            "Asset GPU",
            PresentationAllocation {
                prototype_gpu: 512 * mib + 1,
                ..allocation
            },
        ),
        (
            "Objects",
            PresentationAllocation {
                objects: 8193,
                ..allocation
            },
        ),
        (
            "Pending work",
            PresentationAllocation {
                pending: 33,
                ..allocation
            },
        ),
        (
            "Staging",
            PresentationAllocation {
                staging_bytes: 32 * mib + 1,
                ..allocation
            },
        ),
        (
            "Dependency staging",
            PresentationAllocation {
                dependency_staging_bytes: 1,
                ..allocation
            },
        ),
        (
            "Jobs",
            PresentationAllocation {
                jobs: 3,
                ..allocation
            },
        ),
    ] {
        // The stream covers renderer use only inside these limits.
        let budgets = runtime.stream.presentation_budgets(next);
        assert!(
            budgets.iter().any(|b| b.name == name && b.used > b.limit),
            "{name}"
        );
    }
    runtime.stream.prepare(0.);
    assert_eq!(runtime.stream.metrics().prepared_roads, 256);
    use the_long_slop::world::{ROAD_SEGMENTS, RoadIndex, RoadSequence};
    let seed = reference.world.terrain.seed;
    let mut small = RoadIndex::new(
        RoadSequence::new(&reference.world, seed, None).unwrap(),
        the_long_slop::world::terrain_evaluator(&reference.world, seed),
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
        ReferenceManifest::from_runtime_json(&bytes),
        ReferenceManifest::from_json(&bytes),
    ] {
        assert_eq!(
            result.unwrap_err().to_string(),
            "Invalid reference field: manifest size"
        );
    }
}
