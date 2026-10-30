use the_long_slop::reference::WorldReference;
use the_long_slop::world::*;
fn reference() -> &'static WorldReference {
    static REFERENCE: std::sync::OnceLock<WorldReference> = std::sync::OnceLock::new();
    REFERENCE.get_or_init(|| {
        let path = std::env::var("FRONTIER_MANIFEST")
            .unwrap_or("web/assets/reference/manifest.json".into());
        let bytes = std::fs::read(path).unwrap();
        #[derive(serde::Deserialize)]
        struct Manifest {
            world: WorldReference,
        }
        serde_json::from_slice::<Manifest>(&bytes).unwrap().world
    })
}

fn road(r: &WorldReference) -> RoadIndex {
    RoadIndex::new(
        RoadSequence::new(r, 42, None).unwrap(),
        terrain_evaluator(r, 42),
    )
}
#[test]
#[ignore = "requires the installed source export"]
fn incremental_matches_sequential() {
    let r = reference();
    let mut index = road(&r);
    let mut sequence = RoadSequence::new(&r, 42, None).unwrap();
    let terrain = terrain_evaluator(&r, 42);
    let mut buildings = PlacementIndex::new(&r, 42);
    let mut sequential = BuildingSequence::new(&r, 42);
    let mut expected_buildings = Vec::new();
    let mut actual_buildings = Vec::new();
    for end in [100, 255, 256, 257, 1025] {
        while index.coverage() < end {
            let expected = sequence.advance(&terrain).unwrap();
            index.prepare(1);
            assert_eq!(index.segment(expected.index).unwrap(), expected);
        }
        assert_eq!(index.capture_index().state, sequence.checkpoint().state());
        while buildings.coverage() < end {
            let Some(expected) = sequential.next() else {
                assert!(buildings.prepare(1));
                break;
            };
            expected_buildings.push(expected);
            buildings.prepare(1);
        }
        assert_eq!(buildings.capture_index().state, sequential.checkpoint());
        assert_eq!(
            buildings
                .update_buildings_in(
                    WorldBounds {
                        min: [-1e12; 3],
                        max: [1e12; 3]
                    },
                    buildings.coverage(),
                    &mut actual_buildings,
                    |_| true
                )
                .unwrap(),
            QueryStatus::Complete
        );
        assert_eq!(actual_buildings, expected_buildings);
    }
}
#[test]
#[ignore = "requires the installed source export"]
fn empty_query_requires_coverage() {
    let r = reference();
    let mut index = road(&r);
    let bounds = WorldBounds {
        min: [1e12; 3],
        max: [1e12 + 1.; 3],
    };
    let mut out = vec![];
    assert_eq!(
        index.segments_in(bounds, 100, &mut out),
        QueryStatus::Pending
    );
    index.prepare(100);
    assert_eq!(
        index.segments_in(bounds, 100, &mut out),
        QueryStatus::Complete
    );
    assert!(out.is_empty());
}
#[test]
#[ignore = "requires the installed source export"]
fn partial_checkpoint_round_trip() {
    let r = reference();
    let mut a = road(&r);
    a.prepare(257);
    let mut b = road(&r);
    b.restore_index(&a.capture_index()).unwrap();
    a.prepare(43);
    b.prepare(43);
    assert_eq!(a.capture_index(), b.capture_index());
    let mut a = PlacementIndex::new(&r, 42);
    a.prepare(33);
    let mut b = PlacementIndex::new(&r, 42);
    b.restore_index(&a.capture_index()).unwrap();
    a.prepare(45);
    b.prepare(45);
    assert_eq!(a.capture_index(), b.capture_index());
}
#[test]
#[ignore = "requires the installed source export"]
fn distant_restore_is_bounded() {
    let r = reference();
    let mut a = road(&r);
    a.prepare(1025);
    let mut b = road(&r);
    b.restore_index(&a.capture_index()).unwrap();
    assert_eq!(b.coverage(), 1025);
    assert_eq!(b.replay_work(), 0);
    assert_eq!(b.checkpoint_count(), 5);
}
#[test]
#[ignore = "requires the installed source export"]
fn invalid_frontier_is_rejected() {
    let r = reference();
    let mut a = road(&r);
    a.prepare(257);
    let mut saved = a.capture_index();
    saved.blocks[0].bounds.min[0] = f64::NAN;
    assert!(a.restore_index(&saved).is_err());
    let mut saved = a.capture_index();
    saved.checkpoints.pop();
    assert!(a.restore_index(&saved).is_err());
}

#[test]
fn schema_five_is_rejected_before_payload_decode() {
    let bytes = br#"{"schema_version":5}"#;
    assert!(matches!(
        the_long_slop::save::SaveSnapshot::from_json(bytes),
        Err(the_long_slop::save::SaveError::IncompatibleSources(_))
    ));
    assert_eq!(bytes, br#"{"schema_version":5}"#);
}
#[test]
#[ignore = "requires the installed source export"]
fn contract_and_package_are_checked() {
    let r = reference();
    let mut saved = GenerationSnapshot {
        generator_contract: GENERATOR_CONTRACT,
        package_id: "package".into(),
        roads: road(&r).capture_index(),
        placements: PlacementIndex::new(&r, 42).capture_index(),
    };
    assert!(saved.validate_identity("other").is_err());
    saved.generator_contract += 1;
    assert!(saved.validate_identity("package").is_err());
}
#[test]
#[ignore = "requires the installed source export"]
fn partial_placement_queries_are_explicit() {
    let r = reference();
    let mut index = PlacementIndex::new(&r, 42);
    let bounds = WorldBounds {
        min: [1e12; 3],
        max: [1e12 + 1.; 3],
    };
    let mut out = vec![];
    assert_eq!(
        index.foundations_in(bounds, 33, &mut out).unwrap(),
        QueryStatus::Pending
    );
    index.prepare(33);
    assert_eq!(
        index.foundations_in(bounds, 33, &mut out).unwrap(),
        QueryStatus::Complete
    );
    assert!(out.is_empty());
}
#[test]
#[ignore = "requires the installed source export"]
fn old_road_block_is_checked_on_replay() {
    let r = reference();
    let mut index = road(&r);
    index.prepare(513);
    let mut saved = index.capture_index();
    saved.blocks[0].bounds.max[0] += 1.;
    let mut restored = road(&r);
    restored.restore_index(&saved).unwrap();
    assert!(restored.prepare_segment(255, 256).is_err());
}
#[test]
#[ignore = "requires the installed source export"]
fn invalid_building_checkpoint_is_rejected() {
    let r = reference();
    let mut index = PlacementIndex::new(&r, 42);
    index.prepare(33);
    let mut saved = index.capture_index();
    saved.state.random.next = 56;
    assert!(index.restore_index(&saved).is_err());
    let mut saved = index.capture_index();
    saved.state.groups[0].available.push(i32::MAX);
    assert!(index.restore_index(&saved).is_err());
}

#[test]
#[ignore = "requires the installed source export"]
fn saved_json_preserves_exact_continuation() {
    let r = reference();
    let mut a = PlacementIndex::new(r, 42);
    a.prepare(257);
    let bytes = serde_json::to_vec(&a.capture_index()).unwrap();
    let saved = serde_json::from_slice(&bytes).unwrap();
    let mut b = PlacementIndex::new(r, 42);
    b.restore_index(&saved).unwrap();
    a.prepare(33);
    b.prepare(33);
    assert_eq!(a.capture_index(), b.capture_index());
}

#[test]
#[ignore = "requires the installed source export"]
fn checkpoint_order_and_helper_coverage_are_checked() {
    let r = reference();
    let mut roads = road(r);
    roads.prepare(513);
    let mut saved = roads.capture_index();
    saved.checkpoints.swap(1, 2);
    assert!(roads.restore_index(&saved).is_err());
    let mut placements = PlacementIndex::new(r, 42);
    placements.prepare(65);
    let saved = placements.capture_index();
    let mut value = serde_json::to_value(&saved).unwrap();
    value["blocks"][1]["helpers"]["start"] = serde_json::json!(999999);
    let broken = serde_json::from_value(value).unwrap();
    assert!(placements.restore_index(&broken).is_err());
    let mut value = serde_json::to_value(&saved).unwrap();
    value["blocks"][2]["checkpoint"]["next_distance"] = serde_json::json!(5_000_000.);
    let broken = serde_json::from_value(value).unwrap();
    assert!(placements.restore_index(&broken).is_err());
    let mut value = serde_json::to_value(&saved).unwrap();
    let last = value["foundations"].as_array().unwrap().len() - 1;
    value["foundations"][last]["position"][0] = serde_json::json!(999999.);
    let broken = serde_json::from_value(value).unwrap();
    assert!(placements.restore_index(&broken).is_err());
    let mut value = serde_json::to_value(&saved).unwrap();
    value["foundations"][0]["position"][0] = serde_json::json!(999999.);
    let broken = serde_json::from_value(value).unwrap();
    let mut restored = PlacementIndex::new(r, 42);
    restored.restore_index(&broken).unwrap();
    assert!(
        restored
            .foundations_in(
                WorldBounds {
                    min: [-1e12; 3],
                    max: [1e12; 3]
                },
                65,
                &mut Vec::new()
            )
            .is_err()
    );
}

#[test]
fn required_views_include_casters_and_use_their_own_origin() {
    use the_long_slop::reference::shadow_candidates::DirectionalHull;
    let camera = [
        [1., 0., 0., 1.],
        [-1., 0., 0., 1.],
        [0., 1., 0., 1.],
        [0., -1., 0., 1.],
        [0., 0., 1., 1.],
        [0., 0., -1., 1.],
    ];
    let live = RequiredView {
        camera,
        shadow: DirectionalHull::new(camera, [1., 0., 0.]).unwrap(),
        mask: 1,
        light_mask: 1,
        origin: [0.; 3],
    };
    let candidate = RequiredView {
        origin: [1000., 0., 0.],
        ..live
    };
    let bounds = |x| WorldBounds {
        min: [x - 0.1, -0.1, -0.1],
        max: [x + 0.1, 0.1, 0.1],
    };
    assert!(live.includes(bounds(0.), 1));
    assert!(
        live.includes(bounds(-5.), 1),
        "off-camera caster remains required"
    );
    assert!(
        !live.includes(bounds(5.), 1),
        "distant non-caster leaves startup gate"
    );
    assert!(
        !live.includes(bounds(0.), 2),
        "source layer mask remains binding"
    );
    assert!(candidate.includes(bounds(1000.), 1));
    assert!(!live.includes(bounds(1000.), 1));
    assert_eq!(
        live.origin, [0.; 3],
        "candidate metadata cannot mutate live views"
    );
}

#[test]
fn required_source_planes_reflect_z_after_nonzero_rebase() {
    use the_long_slop::reference::{
        shadow_candidates::DirectionalHull, shadow_planes::implicit_cull_planes,
        view_math::source_culling_view,
    };
    // Runtime camera at (2,3,4), looking down -Z. This is the live snapshot adapter path.
    let target_view = [
        [1., 0., 0., 0.],
        [0., 1., 0., 0.],
        [0., 0., 1., 0.],
        [-2., -3., -4., 1.],
    ];
    let source_view = source_culling_view(target_view);
    let projection = [
        [1., 0., 0., 0.],
        [0., 1., 0., 0.],
        [0., 0., -101. / 99., -1.],
        [0., 0., -200. / 99., 0.],
    ];
    let clip: [[f32; 4]; 4] = std::array::from_fn(|c| {
        std::array::from_fn(|r| (0..4).map(|k| projection[k][r] * source_view[c][k]).sum())
    });
    let inverse = [
        1., 0., 0., 0., 0., 1., 0., 0., 0., 0., -1., 0., 2., 3., -4., 1.,
    ];
    let planes = implicit_cull_planes(
        clip.into_iter()
            .flatten()
            .collect::<Vec<_>>()
            .try_into()
            .unwrap(),
        inverse,
        1.,
        100.,
        30.,
        0.,
    );
    let view = RequiredView {
        camera: planes.camera,
        shadow: DirectionalHull::new(planes.shadow, [1., 0., 0.]).unwrap(),
        mask: 1,
        light_mask: 0,
        origin: [1000., 2000., 3000.],
    };
    let bounds = |z| WorldBounds {
        min: [1001.9, 2002.9, z - 0.1],
        max: [1002.1, 2003.1, z + 0.1],
    };
    assert!(
        view.includes(bounds(2994.), 1),
        "runtime -Z is source +Z after rebase"
    );
    assert!(
        !view.includes(bounds(3014.), 1),
        "point behind runtime camera must be excluded"
    );
    assert!(
        !view.includes(bounds(-2994.), 1),
        "absolute Z must not be reflected before subtracting origin"
    );
}
