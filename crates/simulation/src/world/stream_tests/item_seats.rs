//! V04: a world item seat holds the player on the item body and survives save and load.
use super::*;
use crate::player::SeatOwner;
use crate::vehicle::to_rapier;
use rapier3d::math::Vector;

#[test]
#[ignore = "requires the installed source export"]
fn item_seat_follows_item_body() {
    let reference = reference();
    let mut run = GarageRuntime::new_default(&reference).unwrap();
    let prototype = reference
        .world
        .prototypes
        .iter()
        .find(|p| p.nodes[0].name == "Szanko")
        .expect("sled prototype");
    let first = super::super::content_seats::prototype_seats(
        &reference.world,
        &run.stream.contents.rules,
        prototype,
    )
    .unwrap()
    .remove(0);
    let mut placement = replay_placement(&prototype.source, 800, [0., 10., 0.]);
    placement.scale = prototype.prefab_pose.scale;
    publish_decoration_fixture(&mut run, placement.clone());
    sync_decoration_fixture(&mut run, &reference);
    sync_decoration_fixture(&mut run, &reference);
    let owner = crate::world::ObjectKey::Generated(placement.id.clone());
    let item = run.area.items.resolve_key(&owner).expect("sled item");
    let body = run.area.items.get(item).unwrap().body.unwrap();
    let seat = run
        .physics
        .rapier()
        .colliders
        .iter()
        .find_map(|(handle, _)| {
            run.area
                .interaction
                .item_seat(handle)
                .filter(|seat| seat.seat.source == first.source)
        })
        .expect("bound first sled seat");
    assert_eq!(
        (seat.item, seat.body, &seat.owner),
        (item, body, &SeatOwner::Item(owner.clone()))
    );
    assert!(run.area.player.enter_item_seat(&seat, &mut run.physics));
    let pose = seat.pose;
    assert_eq!(
        run.area.player.seat_owner(),
        Some(&SeatOwner::Item(owner.clone()))
    );
    let start = run.physics.rapier().bodies[body].translation();
    run.physics.edit_body(body, |b| {
        b.set_linvel(Vector::new(1., 0., 0.), true);
    });
    for _ in 0..30 {
        run.physics.step_physics();
        let expected = *run.physics.rapier().bodies[body].position() * to_rapier(pose);
        let seated = to_rapier(run.area.player.body_pose(&run.physics));
        assert!((seated.translation - expected.translation).length() < 1e-4);
        assert!(seated.rotation.angle_between(expected.rotation) < 1e-4);
    }
    let moved = (run.physics.rapier().bodies[body].translation() - start).length();
    assert!(moved > 0.01, "sled moved {moved} m");
    let point = run.physics.rapier().bodies[body].translation().to_array();
    assert!(
        run.area
            .interaction
            .pickup_item(
                item,
                point,
                &run.area.player,
                &mut run.area.items,
                &mut run.physics
            )
            .is_err(),
        "occupied item refuses pickup"
    );
    let stamp = roadside_stamp(&reference);
    let saved = run
        .capture_save(&reference, &stamp, "2026-10-26T12:00:00Z")
        .unwrap();
    let saved: crate::save::SaveSnapshot =
        serde_json::from_slice(&serde_json::to_vec(&saved).unwrap()).unwrap();
    let seat_save = saved.player.seat.as_ref().expect("saved item seat");
    assert_eq!(
        seat_save.owner,
        crate::save::SeatOwnerSnapshot::Item {
            item: owner.clone()
        }
    );
    assert!(
        saved.world.changes.get(&owner).is_some(),
        "seat item change"
    );
    let mut loaded = GarageRuntime::from_save(
        &reference,
        &stamp,
        &saved,
        crate::world::GameMode::default(),
        reference.world.draw_distance.default,
    )
    .unwrap();
    assert!(loaded.restore_pending());
    assert!(
        !loaded.area.player.seated(),
        "seat waits for the item binding"
    );
    for _ in 0..2000 {
        prepare_ready(&mut loaded, &reference);
        if loaded.advance_saved_restore(&reference).unwrap() && !loaded.restore_pending() {
            break;
        }
    }
    assert!(!loaded.restore_pending());
    assert_eq!(
        loaded.area.player.seat_owner(),
        Some(&SeatOwner::Item(owner.clone()))
    );
    let item = loaded.area.items.resolve_key(&owner).expect("loaded sled");
    let body = loaded.area.items.get(item).unwrap().body.unwrap();
    assert_eq!(loaded.area.player.item_seat_body(), Some(body));
    let expected = *loaded.physics.rapier().bodies[body].position() * to_rapier(pose);
    let seated = to_rapier(loaded.area.player.body_pose(&loaded.physics));
    assert!((seated.translation - expected.translation).length() < 1e-3);
    assert!(loaded.area.player.try_exit(&mut loaded.physics));
    assert!(!loaded.area.player.seated());
}
