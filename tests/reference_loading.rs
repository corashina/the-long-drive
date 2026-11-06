use the_long_slop::reference::{MAX_MANIFEST_BYTES, ReferenceManifest};

#[test]
#[ignore = "requires the installed source export"]
fn installed_startup_selects_only_garage_lada() {
    let bytes =
        the_long_slop::reference::read_manifest(the_long_slop::reference::INSTALLED_MANIFEST)
            .unwrap();
    ReferenceManifest::from_runtime(&bytes).unwrap();
    let (json, tables) = the_long_slop::reference::split_manifest(&bytes).unwrap();
    let mut manifest: serde_json::Value = serde_json::from_slice(json).unwrap();
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
    let bytes =
        the_long_slop::reference::join_manifest(&serde_json::to_vec(&manifest).unwrap(), tables);
    assert!(
        ReferenceManifest::from_runtime(&bytes)
            .unwrap_err()
            .to_string()
            .contains("selected packs")
    );
}

#[test]
#[ignore = "requires the installed source export"]
fn installed_manifest_checks_lod_bounds_and_initializes_runtime() {
    let bytes =
        the_long_slop::reference::read_manifest(the_long_slop::reference::INSTALLED_MANIFEST)
            .unwrap();
    let mut reference = ReferenceManifest::from_runtime(&bytes).unwrap();
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
        ReferenceManifest::from_runtime(&bytes),
        ReferenceManifest::from_manifest(&bytes),
    ] {
        assert_eq!(
            result.unwrap_err().to_string(),
            "Invalid reference field: manifest size"
        );
    }
}

/// The shell from the parsed manifest matches the manifest text, so the interface reads the same values.
#[test]
#[ignore = "requires the installed source export"]
fn installed_manifest_shell_matches_the_manifest_text() {
    use serde_json::Value;
    fn same(a: &Value, b: &Value, path: &str) {
        match (a, b) {
            (Value::Number(a), Value::Number(b)) => {
                let (a, b) = (a.as_f64().unwrap(), b.as_f64().unwrap());
                assert!((a - b).abs() <= 1e-6 * a.abs().max(1.0), "{path}: {a} {b}");
            }
            (Value::Array(a), Value::Array(b)) => {
                assert_eq!(a.len(), b.len(), "{path}");
                for (i, (a, b)) in a.iter().zip(b).enumerate() {
                    same(a, b, &format!("{path}/{i}"));
                }
            }
            (Value::Object(a), Value::Object(b)) => {
                let keys =
                    |o: &serde_json::Map<String, Value>| o.keys().cloned().collect::<Vec<_>>();
                assert_eq!(keys(a), keys(b), "{path}");
                for (key, a) in a {
                    same(a, &b[key], &format!("{path}/{key}"));
                }
            }
            _ => assert_eq!(a, b, "{path}"),
        }
    }
    let bytes =
        the_long_slop::reference::read_manifest(the_long_slop::reference::INSTALLED_MANIFEST)
            .unwrap();
    let reference = ReferenceManifest::from_checked_runtime(&bytes).unwrap();
    let mut shell: Value =
        serde_json::from_str(&the_long_slop::manifest_shell::shell(&reference).unwrap()).unwrap();
    assert!(
        !shell["world"]["start_cars"].as_array().unwrap().is_empty(),
        "start cars"
    );
    shell["world"].as_object_mut().unwrap().remove("start_cars");
    let (json, _) = the_long_slop::reference::split_manifest(&bytes).unwrap();
    let text: Value = serde_json::from_slice(json).unwrap();
    for (key, value) in shell.as_object().unwrap() {
        let expected = match key.as_str() {
            "world" => serde_json::json!({
                "draw_distance": text["world"]["draw_distance"],
                "mirrors": text["world"]["mirrors"],
            }),
            "clothing" => serde_json::json!({
                "characters": text["clothing"]["characters"].as_array().unwrap().iter()
                    .map(|c| serde_json::json!({"segments": c["segments"]})).collect::<Vec<_>>(),
            }),
            _ => text[key].clone(),
        };
        same(value, &expected, key);
    }
}
