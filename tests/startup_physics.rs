use the_long_slop::{
    reference::ReferenceManifest,
    simulation::{FIXED_DT, FrameInput},
    starting_area::GarageRuntime,
    vehicle::from_rapier,
};

#[test]
#[ignore = "requires the installed source export"]
fn installed_spawn_alignment_and_settling() {
    let bytes = std::fs::read("web/assets/reference/manifest.json").unwrap();
    let reference = ReferenceManifest::from_runtime_json(&bytes).unwrap();
    let mut run = GarageRuntime::new(
        &reference,
        the_long_slop::world::GameMode::default(),
        reference.world.draw_distance.default,
    )
    .unwrap();
    use the_long_slop::world::{RequiredView, WorldStream};
    let camera = [
        [1., 0., 0., 1e8],
        [-1., 0., 0., 1e8],
        [0., 1., 0., 1e8],
        [0., -1., 0., 1e8],
        [0., 0., 1., 1e8],
        [0., 0., -1., 1e8],
    ];
    let views = [RequiredView {
        camera,
        shadow: the_long_slop::reference::shadow_candidates::DirectionalHull::new(
            camera,
            [1., 0., 0.],
        )
        .unwrap(),
        mask: u32::MAX,
        light_mask: u32::MAX,
        origin: [0.; 3],
    }];
    let mut replay = WorldStream::new(
        &reference,
        42,
        reference.world.spawn_context.date,
        the_long_slop::world::GameMode::default(),
        the_long_slop::world::DrawDistances::resolve(
            &reference.world.draw_distance,
            reference.world.draw_distance.default,
        ),
    )
    .unwrap();
    replay.request_at([0.; 3], [0.; 3]);
    for _ in 0..300 {
        replay.advance_generation();
        replay.prepare(0.);
        replay.select_required(&views);
        if replay.frame().startup.is_some() && !replay.required_content().roads.is_empty() {
            break;
        }
        assert!(
            replay.frame().failure.is_none(),
            "{:?}",
            replay.frame().failure
        );
    }
    assert!(
        replay.frame().startup.is_some(),
        "ordinary generation must not reset startup replay"
    );
    assert!(
        !replay.required_content().roads.is_empty(),
        "selected replay must advance under ordinary generation"
    );
    assert!(replay.coverage().road_end < 400);

    assert!(
        !run.stream.required_ready(),
        "empty unresolved startup is not ready"
    );
    assert!(run.stream.recovery_target([0.; 3], true).unwrap().is_none());
    assert!(run.stream.frame().failure.is_none());
    assert!(!run.stream.supports(the_long_slop::world::WorldBounds {
        min: [-8.; 3],
        max: [8.; 3]
    }));
    for _ in 0..10000 {
        run.stream.prepare(100.);
        run.stream.select_required(&views);
        if run.stream.frame().startup.is_some() {
            break;
        }
        assert!(run.stream.frame().failure.is_none());
    }
    assert_eq!(run.stream.coverage().road_end, 100);
    assert!(!run.stream.required_ready());
    let before = run.stream.coverage();
    for _ in 0..8 {
        run.stream.prepare(100.);
        run.stream.select_required(&views);
    }
    assert_eq!(
        run.stream.coverage(),
        before,
        "prepare must not advance generation"
    );
    let revision = run.stream.frame().request.revision;
    run.stream.advance_generation();
    assert_eq!(run.stream.coverage().road_end, 101);
    assert_eq!(
        run.stream.frame().request.revision,
        revision,
        "irrelevant growth must preserve selection"
    );
    let offset = run.stream.frame().startup.unwrap().player_offset;
    assert!(run.align_startup(&reference, [1, 1]).unwrap());
    let player = run.area.player.body_handle();
    let physical = from_rapier(run.physics.rapier().bodies[player].position());
    assert_eq!(run.area.player.body_pose(&run.physics), physical);
    assert!(run.align_startup(&reference, [1, 1]).unwrap());
    assert_eq!(run.area.player.body_pose(&run.physics), physical);

    let settle_steps = (2.0_f32 / FIXED_DT).round() as usize;
    for _ in 0..settle_steps {
        run.area
            .player
            .step_view(FrameInput::default(), Default::default(), FIXED_DT);
        run.area.player.step_movement(
            FrameInput::default(),
            Default::default(),
            &mut run.physics,
            FIXED_DT,
        );
        run.area.sync_attachments(&mut run.physics, true);
        run.physics.step_physics();
        run.area.sync_attachments(&mut run.physics, false);
        run.physics.refresh_queries();
    }
    assert!(run.area.player.grounded());
    run.sync_world_activity(&reference).unwrap();
    let mut roads = the_long_slop::world::RoadIndex::new(
        the_long_slop::world::RoadSequence::new(&reference.world, 42, None).unwrap(),
        the_long_slop::world::terrain_evaluator(&reference.world, 42),
    );
    roads.prepare(102);
    let frontier = roads.segment(101).unwrap().position();
    run.stream.request_at(frontier, frontier);
    let revision = run.stream.frame().request.revision;
    run.stream.advance_generation();
    assert_eq!(run.stream.coverage().road_end, 102);
    assert!(
        run.stream.frame().request.revision > revision,
        "stationary growth must select new local identities"
    );
    assert!(!run.stream.required_ready());
    for _ in 0..200 {
        run.stream.prepare(100.);
        run.stream.select_required(&views);
        if run.stream.required_content().roads.contains(&101) {
            break;
        }
        assert!(
            run.stream.frame().failure.is_none(),
            "{:?}",
            run.stream.frame().failure
        );
    }
    assert!(run.stream.required_content().roads.contains(&101));
    assert_eq!(run.stream.coverage().road_end, 102);
    eprintln!("Spawn offset {offset} m.");
}
