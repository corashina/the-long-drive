use the_long_slop::save::fresh_permit::FreshSessionPermit;

#[test]
fn fresh_permit_is_bound_to_request_session_and_generation() {
    let permit = FreshSessionPermit {
        request_id: 7,
        session: 11,
        generation: 3,
    };
    assert!(permit.matches(7, 11, 3));
    assert!(!permit.matches(8, 11, 3));
    assert!(!permit.matches(7, 12, 3));
    assert!(!permit.matches(7, 11, 4));
}

#[test]
#[ignore = "requires the installed source export"]
fn first_permit_retains_runtime_and_commits_once_paused() {
    use the_long_slop::{reference::ReferenceManifest, starting_area::GarageRuntime};
    let bytes = std::fs::read("web/assets/reference/manifest.json").unwrap();
    let reference = ReferenceManifest::from_runtime_json(&bytes).unwrap();
    let mut runtime = GarageRuntime::new(
        &reference,
        the_long_slop::world::GameMode::default(),
        reference.world.draw_distance.default,
    )
    .unwrap();
    let session = runtime.stream.frame().request.session;
    let pose = runtime.area.player.body_pose(&runtime.physics);
    let bodies = runtime.physics.rapier().bodies.len();
    let discarded = FreshSessionPermit::prepare(7, &runtime, 3).unwrap();
    let retry = FreshSessionPermit::prepare(8, &runtime, 3).unwrap();
    assert!(discarded.commit(8, &mut runtime, 3).is_err());
    assert!(retry.commit(8, &mut runtime, 4).is_err());
    let mut changed_session = GarageRuntime::new(
        &reference,
        the_long_slop::world::GameMode::default(),
        reference.world.draw_distance.default,
    )
    .unwrap();
    assert!(retry.commit(8, &mut changed_session, 3).is_err());
    assert!(!changed_session.started);
    assert!(!runtime.started);
    assert_eq!(runtime.stream.frame().request.session, session);
    assert_eq!(runtime.area.player.body_pose(&runtime.physics), pose);
    assert_eq!(runtime.physics.rapier().bodies.len(), bodies);
    retry.commit(8, &mut runtime, 3).unwrap();
    assert!(runtime.started && runtime.paused);
    assert_eq!(runtime.simulation_seconds, 0.);
    assert_eq!(runtime.stream.frame().request.session, session);
    assert_eq!(runtime.area.player.body_pose(&runtime.physics), pose);
    assert_eq!(runtime.physics.rapier().bodies.len(), bodies);
    assert!(retry.commit(8, &mut runtime, 3).is_err());
    assert!(FreshSessionPermit::prepare(9, &runtime, 3).is_err());
}
