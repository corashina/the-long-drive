//! B01: the loader spawns any number of car copies. Two Lada copies drive, save and load.
use super::drive_out::ground;
use super::kit::*;
use super::*;
use crate::simulation::{DriveInput, FIXED_DT};
use crate::vehicle::DriveTorques;
use crate::world::ObjectKey;

/// Height of the second copy above the start copy. A test deck holds it there.
const DECK_HEIGHT: f32 = 30.;
/// Half size of the test deck.
const DECK: [f32; 3] = [40., 0.5, 40.];

/// The reference with a second `StartCar02` startup entry, `DECK_HEIGHT` above the first.
/// Returns the startup ordinals of the start copy and the second copy.
fn two_lada_reference() -> (ReferenceManifest, u32, u32) {
    let mut reference = reference();
    assert_eq!(reference.build_id, 11140890);
    let world = std::sync::Arc::make_mut(&mut reference.world);
    let start = world
        .startup
        .iter()
        .position(|s| s.group == "StartCar02" && s.prototype.is_some())
        .expect("StartCar02 placement");
    let mut copy = world.startup[start].clone();
    copy.pose.translation[1] += DECK_HEIGHT;
    world.startup.push(copy);
    let second = world.startup.len() - 1;
    (reference, start as u32, second as u32)
}

fn startup_key(run: &GarageRuntime, ordinal: u32) -> Option<ObjectKey> {
    run.vehicles
        .keys()
        .find(|key| {
            matches!(key, ObjectKey::Generated(id)
                if id.layer == WorldLayer::Startup && id.ordinal == ordinal)
        })
        .cloned()
}

/// Steps frames until the startup copies `ordinals` have car runtimes.
fn wait_cars(run: &mut GarageRuntime, reference: &ReferenceManifest, ordinals: [u32; 2]) {
    for _ in 0..2000 {
        if ordinals.iter().all(|&o| startup_key(run, o).is_some()) {
            return;
        }
        frame(run, reference);
    }
    panic!("startup car copies did not spawn");
}

/// A fixed deck under car `key`, at the ground clearance of the start copy `start`.
/// Returns the absolute deck center.
fn insert_deck(run: &mut GarageRuntime, start: &ObjectKey, key: &ObjectKey) -> [f64; 3] {
    let start = run.physics.snapshot(start).pose.position;
    let clearance = start[1] - ground(run, start).expect("ground under the start copy");
    let car = run.physics.snapshot(key).pose;
    let center = [
        car.position[0],
        car.position[1] - clearance - 0.5,
        car.position[2],
    ];
    run.physics.insert_box(center, DECK);
    run.physics.reset_preserving_wheels(key, car);
    run.physics.refresh_queries();
    run.stream.origin.absolute(center)
}

/// Steps physics only. Each car gets `input` with `motor_nm` of drive torque.
fn drive(
    run: &mut GarageRuntime,
    keys: &[&ObjectKey],
    seconds: f32,
    input: DriveInput,
    motor_nm: f32,
) {
    let torques = DriveTorques {
        motor_nm,
        engine_brake_nm: 0.,
        pedal_nm: None,
    };
    for _ in 0..(seconds / FIXED_DT) as usize {
        for key in keys {
            run.physics.apply_vehicle_forces(key, input, torques);
        }
        run.physics.step_physics();
    }
    run.physics.refresh_queries();
}

fn absolute(run: &GarageRuntime, key: &ObjectKey) -> ([f64; 3], [f32; 4]) {
    let pose = run.physics.snapshot(key).pose;
    (run.stream.origin.absolute(pose.position), pose.rotation)
}

fn fuel(run: &GarageRuntime, key: &ObjectKey) -> f32 {
    let tank = run.vehicles[key].fuel_tank.expect("Lada fuel tank");
    run.area
        .items
        .get(tank)
        .unwrap()
        .fluid
        .as_ref()
        .unwrap()
        .total()
}

#[test]
#[ignore = "requires the installed source export"]
fn two_ladas_drive_save_and_load() {
    let (reference, a, b) = two_lada_reference();
    let mut run = ready(&reference);
    run.started = true;
    run.paused = false;
    wait_cars(&mut run, &reference, [a, b]);
    let (a, b) = (startup_key(&run, a).unwrap(), startup_key(&run, b).unwrap());
    assert_eq!(run.vehicles[&a].prototype, run.vehicles[&b].prototype);
    let start_a = absolute(&run, &a).0;
    let start_b = absolute(&run, &b).0;
    assert!(
        (start_b[1] - start_a[1] - f64::from(DECK_HEIGHT)).abs() < 0.01,
        "{start_a:?} {start_b:?}"
    );
    let deck = insert_deck(&mut run, &a, &b);
    drive(&mut run, &[&a, &b], 2., DriveInput::default(), 0.);
    let (start_a, start_b) = (absolute(&run, &a).0, absolute(&run, &b).0);

    // Both copies drive.
    let throttle = DriveInput {
        throttle: 1.,
        ..Default::default()
    };
    drive(&mut run, &[&a, &b], 2., throttle, 1600.);
    for (key, start) in [(&a, start_a), (&b, start_b)] {
        let now = absolute(&run, key).0;
        let moved = ((now[0] - start[0]).powi(2) + (now[2] - start[2]).powi(2)).sqrt();
        assert!(moved > 0.5, "{key:?} moved {moved} m");
        assert!(run.physics.driven_wheel_rpm(key) > 0.);
    }
    let brake = DriveInput {
        brake: 1.,
        ..Default::default()
    };
    drive(&mut run, &[&a, &b], 4., brake, 0.);
    seconds(&mut run, &reference, 1.);

    // Different gear and fuel per copy.
    // The start Lada has only a few liters. Spill a different share per copy.
    for (key, gear, share) in [(&a, 2, 0.25), (&b, 3, 0.5)] {
        let spilled = fuel(&run, key) * share;
        assert!(spilled > 0., "{key:?} has fuel");
        run.vehicles
            .get_mut(key)
            .unwrap()
            .drivetrain
            .restore_gear(gear);
        let tank = run.vehicles[key].fuel_tank.unwrap();
        let fluid = run
            .area
            .items
            .get_mut(tank)
            .unwrap()
            .fluid
            .as_mut()
            .unwrap();
        assert!((crate::fluids::spill(fluid, spilled) - spilled).abs() < 1e-4);
        run.area.items.mark_dirty(tank);
    }
    let saved: Vec<_> = [&a, &b]
        .map(|key| {
            (
                absolute(&run, key),
                run.vehicles[key].drivetrain.gear(),
                fuel(&run, key),
            )
        })
        .into();
    assert_ne!(saved[0].2, saved[1].2);
    // X05: the parked start copy keeps its wheels turned. The sleep cooldown runs.
    run.vehicles.get_mut(&a).unwrap().steer = 0.4;
    run.sleep.restore(0.25, run.simulation_seconds);

    let snapshot = save(&run, &reference);
    for key in [&a, &b] {
        assert!(
            snapshot.world.changes.vehicle(key).is_some(),
            "save keeps {key:?}"
        );
    }
    assert_eq!(snapshot.world.changes.vehicle(&a).unwrap().steer, 0.4);
    assert!((snapshot.player.since_wake - 0.25).abs() < 1e-9);

    // Load into a fresh runtime with the same deck.
    let mut loaded = open(&reference, &snapshot);
    let deck = loaded.stream.origin.local(deck);
    loaded.physics.insert_box(deck, DECK);
    restore(&mut loaded, &reference, |l| {
        l.vehicles.contains_key(&a) && l.vehicles.contains_key(&b)
    });
    for (key, ((position, rotation), gear, fuel_liters)) in [&a, &b].into_iter().zip(&saved) {
        assert!(loaded.vehicles.contains_key(key), "{key:?} returns");
        let (now, now_rotation) = absolute(&loaded, key);
        for i in 0..3 {
            assert!(
                (now[i] - position[i]).abs() < 0.01,
                "{key:?} {position:?} -> {now:?}"
            );
        }
        let dot: f32 = (0..4).map(|i| now_rotation[i] * rotation[i]).sum();
        assert!(
            dot.abs() > 0.9999,
            "{key:?} {rotation:?} -> {now_rotation:?}"
        );
        assert_eq!(loaded.vehicles[key].drivetrain.gear(), *gear, "{key:?}");
        assert!((fuel(&loaded, key) - fuel_liters).abs() < 1e-4, "{key:?}");
    }
    assert_eq!(loaded.vehicles[&a].steer, 0.4);
    let recaptured = save(&loaded, &reference);
    for key in [&a, &b] {
        assert_eq!(
            recaptured.world.changes.vehicle(key).unwrap().steer,
            snapshot.world.changes.vehicle(key).unwrap().steer,
            "{key:?}"
        );
    }
    assert_eq!(recaptured.player.since_wake, snapshot.player.since_wake);
}

/// The old garage Lada test on a loader copy, with its numbers.
#[test]
#[ignore = "requires the installed source export"]
fn installed_lada_launches_and_brakes() {
    let (reference, a, b) = two_lada_reference();
    let mut run = ready(&reference);
    run.started = true;
    run.paused = false;
    wait_cars(&mut run, &reference, [a, b]);
    let (a, key) = (startup_key(&run, a).unwrap(), startup_key(&run, b).unwrap());
    insert_deck(&mut run, &a, &key);
    let key = &key;
    drive(&mut run, &[key], 2., DriveInput::default(), 0.);
    assert!(run.physics.wheel_contacts(key).all(|c| c));
    // First gear near peak engine torque.
    let throttle = DriveInput {
        throttle: 1.,
        ..Default::default()
    };
    drive(&mut run, &[key], 3., throttle, 1600.);
    let speed = run.physics.snapshot(key).speed_mps;
    assert!(speed > 4., "launch speed {speed}");
    assert!(run.physics.driven_wheel_rpm(key) > 0.);
    let brake = DriveInput {
        brake: 1.,
        ..Default::default()
    };
    drive(&mut run, &[key], 4., brake, 0.);
    assert!(run.physics.snapshot(key).speed_mps.abs() < 0.5);
    // A parked car holds against a walking player's push.
    let parked = DriveInput {
        handbrake: 1.,
        ..Default::default()
    };
    drive(&mut run, &[key], 2., parked, 0.);
    let start = run.physics.snapshot(key).pose.position;
    let chassis = run.physics.chassis_handle(key);
    for _ in 0..(2. / FIXED_DT) as usize {
        run.physics.edit_body(chassis, |body| {
            let point = body.translation();
            body.apply_impulse_at_point(rapier3d::math::Vector::X * 60., point, true);
        });
        drive(&mut run, &[key], FIXED_DT, parked, 0.);
    }
    let end = run.physics.snapshot(key).pose.position;
    let moved = ((end[0] - start[0]).powi(2) + (end[2] - start[2]).powi(2)).sqrt();
    assert!(moved < 0.05, "parked car moved {moved}");
}
