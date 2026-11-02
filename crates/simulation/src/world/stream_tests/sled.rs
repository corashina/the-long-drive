//! V04: the source sled slides on a slope, steers, stops its use on stand up and survives save and load.
use super::*;
use crate::player::SeatOwner;
use crate::simulation::input::{DrivingInput, DrivingSettings};
use crate::simulation::{FIXED_DT, player_keys};
use rapier3d::math::{Pose, Rotation, Vector};
use rapier3d::prelude::{ColliderBuilder, ColliderHandle, RigidBodyHandle};

/// Flat forward of the body.
fn flat_forward(run: &GarageRuntime, body: RigidBodyHandle) -> Vector {
    let forward = run.physics.rapier().bodies[body].rotation() * Vector::NEG_Z;
    Vector::new(forward.x, 0., forward.z).normalize()
}

/// One game step of the sled: keyboard input, `step_sleds`, physics.
fn step_sled(run: &mut GarageRuntime, driving: &mut DrivingInput, player_held: u16) {
    let controls = driving
        .step(
            run.area.player.item_seat_body(),
            &crate::simulation::FrameInput {
                player_held,
                ..Default::default()
            },
            None,
            false,
        )
        .controls;
    let seat = match (
        run.area.player.seat_owner(),
        run.area.player.item_seat_source(),
    ) {
        (Some(SeatOwner::Item(owner)), Some(source)) => Some((owner, source)),
        _ => None,
    };
    run.stream.step_sleds(&mut run.physics, seat, controls);
    run.physics.step_physics();
    run.physics.refresh_queries();
}

fn sled_in_use(run: &GarageRuntime) -> bool {
    let mut count = 0;
    run.stream
        .sled_sounds(&run.physics, |_, _, _, _| count += 1);
    count == 1
}

#[test]
#[ignore = "requires the installed source export"]
fn sled_slides_steers_and_keeps_its_seat() {
    let reference = reference();
    let mut run = GarageRuntime::new_default(&reference).unwrap();
    let prototype = reference
        .world
        .prototypes
        .iter()
        .find(|p| p.nodes[0].name == "Szanko")
        .expect("sled prototype");
    let record = prototype.sled.as_ref().expect("sled record");
    assert_eq!(record.wheels.len(), 6);
    assert_eq!(record.seats.len(), 3);
    assert!(!record.colliders.is_empty());
    assert!(record.clip > 0);
    let first = super::super::content_seats::prototype_seats(
        &reference.world,
        &run.stream.contents.rules,
        prototype,
    )
    .unwrap()
    .remove(0);
    assert!(record.seats.contains(&first.source));
    let mut placement = replay_placement(&prototype.source, 801, [0., 200., 0.]);
    placement.scale = prototype.prefab_pose.scale;
    publish_decoration_fixture(&mut run, placement.clone());
    sync_decoration_fixture(&mut run, &reference);
    sync_decoration_fixture(&mut run, &reference);
    assert_eq!(run.stream.sled_bindings.len(), 1, "bound sled");
    let owner = crate::world::ObjectKey::Generated(placement.id.clone());
    let item = run.area.items.resolve_key(&owner).expect("sled item");
    let body = run.area.items.get(item).unwrap().body.unwrap();
    let cols: Vec<ColliderHandle> = run
        .stream
        .structures
        .iter()
        .find(|s| s.placement.id == placement.id)
        .expect("sled structure")
        .colliders
        .iter()
        .filter(|(source, _)| record.colliders.contains(source))
        .map(|(_, handle)| *handle)
        .collect();
    assert!(!cols.is_empty(), "published cols");

    // A 15 degree slope that falls along the sled forward.
    let forward = flat_forward(&run, body);
    let lowest = run.physics.rapier().bodies[body]
        .colliders()
        .iter()
        .map(|&c| run.physics.rapier().colliders[c].compute_aabb().mins.y)
        .fold(f32::INFINITY, f32::min);
    let angle = 15f32.to_radians();
    let normal = Vector::Y * angle.cos() + forward * angle.sin();
    let mut contact = run.physics.rapier().bodies[body].translation();
    contact.y = lowest - 0.5;
    run.physics.insert_collider(
        ColliderBuilder::cuboid(20., 0.5, 60.)
            .position(Pose::from_parts(
                contact - normal * 0.5,
                Rotation::from_rotation_arc(Vector::Y, normal)
                    * Rotation::from_rotation_arc(Vector::NEG_Z, forward),
            ))
            .friction(0.8),
    );
    run.physics.refresh_queries();
    let mut driving = DrivingInput::new(
        reference.settings.sliders.key_steer.value(),
        reference.driving.pedal_rate,
        DrivingSettings::defaults(&reference.driving),
    );
    for _ in 0..(0.5 / FIXED_DT) as usize {
        step_sled(&mut run, &mut driving, 0);
    }
    assert!(!sled_in_use(&run), "empty light sled is not in use");

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
    assert!(run.area.player.enter_item_seat(&seat, &mut run.physics));

    // Slide and push forward for 2 s.
    let start = run.physics.rapier().bodies[body].translation();
    let forward = flat_forward(&run, body);
    for _ in 0..(2.0 / FIXED_DT) as usize {
        step_sled(&mut run, &mut driving, player_keys::GAS);
    }
    assert!(sled_in_use(&run), "seated sled is in use");
    for &c in &cols {
        assert!(
            run.physics.rapier().colliders[c].is_sensor(),
            "cols are sensors"
        );
    }
    let moved = (run.physics.rapier().bodies[body].translation() - start).dot(forward);
    assert!(moved > 1., "sled moved {moved} m forward");

    // LEFT for 1 s turns the forward toward the left.
    let before = flat_forward(&run, body);
    for _ in 0..(1.0 / FIXED_DT) as usize {
        step_sled(
            &mut run,
            &mut driving,
            player_keys::GAS | player_keys::STEER_LEFT,
        );
    }
    let after = flat_forward(&run, body);
    let yaw = before.cross(after).y.asin();
    assert!(yaw > 5f32.to_radians(), "left steer yaw {yaw} rad");

    // Standing up ends the use.
    assert!(run.area.player.try_exit(&mut run.physics));
    step_sled(&mut run, &mut driving, 0);
    assert!(!sled_in_use(&run), "empty sled stops its use");
    for &c in &cols {
        assert!(
            !run.physics.rapier().colliders[c].is_sensor(),
            "cols are solid"
        );
    }

    // Sit again, save and load.
    assert!(run.area.player.enter_item_seat(&seat, &mut run.physics));
    step_sled(&mut run, &mut driving, 0);
    assert!(sled_in_use(&run));
    let saved = save(&run, &reference);
    let mut loaded = open(&reference, &saved);
    republish_fixture(&mut loaded, &reference, &placement);
    restore(&mut loaded, &reference, |_| true);
    assert_eq!(
        loaded.area.player.seat_owner(),
        Some(&SeatOwner::Item(owner.clone()))
    );
    assert_eq!(loaded.area.player.item_seat_source(), Some(&first.source));
    let item = loaded.area.items.resolve_key(&owner).expect("loaded sled");
    let body = loaded.area.items.get(item).unwrap().body.unwrap();
    assert_eq!(loaded.area.player.item_seat_body(), Some(body));
    assert_eq!(loaded.stream.sled_bindings.len(), 1, "loaded sled binding");
    step_sled(&mut loaded, &mut driving, 0);
    assert!(sled_in_use(&loaded), "loaded seated sled is in use");
}
