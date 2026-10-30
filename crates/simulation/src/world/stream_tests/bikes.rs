//! B12: the Bike01 copy kick starts at 10 m/s, then rides with balance on flat ground.
//! The Bike03 copy pedals from rest, then kick starts its engine.
use super::drive_out::{frame, frame_with, settle};
use super::*;
use crate::player::SeatOwner;
use crate::reference::{DrivingAssists, WorldVehicleReference};
use crate::simulation::{FIXED_DT, FrameInput, player_keys};
use crate::world::ObjectKey;
use crate::world::stream::content_seats::WorldSeatReference;
use rapier3d::math::Vector;

/// The reference with a `category` startup entry 30 m above `StartCar02`.
/// The entry is the prototype that passes `pick` and has the most engines.
/// Returns the startup ordinal and the vehicle record.
fn bike_reference(
    category: &str,
    pick: fn(&WorldVehicleReference) -> bool,
) -> (ReferenceManifest, u32, WorldVehicleReference) {
    let mut reference = reference();
    let world = std::sync::Arc::make_mut(&mut reference.world);
    let category = world
        .catalog
        .categories
        .iter()
        .find(|c| c.name == category)
        .expect("bike category");
    let prototype = category
        .items
        .iter()
        .flatten()
        .filter_map(|&i| world.catalog.items[i as usize].prototype.as_ref())
        .filter_map(|source| world.prototypes.iter().find(|p| &p.source == source))
        .filter(|p| p.vehicle.as_ref().is_some_and(pick))
        .max_by_key(|p| p.engines.len())
        .expect("bike prototype");
    let (source, vehicle) = (prototype.source.clone(), prototype.vehicle.clone().unwrap());
    let start = world
        .startup
        .iter()
        .position(|s| s.group == "StartCar02" && s.prototype.is_some())
        .expect("StartCar02 placement");
    let mut copy = world.startup[start].clone();
    copy.pose.translation[1] += 30.;
    copy.prototype = Some(source);
    world.startup.push(copy);
    let ordinal = world.startup.len() as u32 - 1;
    (reference, ordinal, vehicle)
}

/// Spawns the startup copy `ordinal` on a flat deck and seats the player in the seat that passes `pick`.
fn seated_bike(
    reference: &ReferenceManifest,
    ordinal: u32,
    pick: impl Fn(&WorldSeatReference) -> bool,
) -> (GarageRuntime, ObjectKey) {
    let mut run = GarageRuntime::new_default(reference).unwrap();
    prepare_ready(&mut run, reference);
    run.started = true;
    run.paused = false;
    run.driving_assists = DrivingAssists {
        automatic_clutch: true,
        automatic_gears: true,
    };
    let key = (0..2000)
        .find_map(|_| {
            let key = run.vehicles.keys().find(|key| {
                matches!(key, ObjectKey::Generated(id)
                    if id.layer == WorldLayer::Startup && id.ordinal == ordinal)
            });
            let key = key.cloned();
            if key.is_none() {
                frame(&mut run, reference);
            }
            key
        })
        .expect("bike copy");

    // A flat deck under the wheels, then the seat.
    let pose = run.physics.snapshot(&key).pose;
    let lowest = (0..run.physics.wheel_contacts(&key).count())
        .map(|i| run.physics.wheel_pose(&key, i).position[1])
        .fold(f32::INFINITY, f32::min);
    run.physics.insert_box(
        [pose.position[0], lowest - 1.5, pose.position[2]],
        [40., 0.5, 400.],
    );
    run.physics.reset_preserving_wheels(&key, pose);
    run.physics.refresh_queries();
    let seat = run
        .physics
        .rapier()
        .colliders
        .iter()
        .find_map(|(handle, _)| {
            run.area
                .interaction
                .item_seat(handle)
                .filter(|s| s.owner == SeatOwner::Vehicle(key.clone()) && pick(&s.seat))
        })
        .expect("bike seat");
    assert!(run.area.player.enter_item_seat(&seat, &mut run.physics));
    settle(&mut run, reference, 2.);
    assert!(run.physics.wheel_contacts(&key).all(|c| c), "wheels down");
    assert!(!running(&run, &key));
    (run, key)
}

fn distance(a: [f32; 3], b: [f32; 3]) -> f32 {
    ((b[0] - a[0]).powi(2) + (b[2] - a[2]).powi(2)).sqrt()
}

/// Source roll in degrees: the chassis up around its forward.
fn roll(run: &GarageRuntime, key: &ObjectKey) -> f32 {
    let rotation = *run.physics.rapier().bodies[run.physics.chassis_handle(key)].rotation();
    let (up, right) = (rotation * Vector::Y, rotation * Vector::X);
    right.y.atan2(up.y).to_degrees()
}

fn running(run: &GarageRuntime, key: &ObjectKey) -> bool {
    run.engine_output(key).running
}

/// One game frame with `player_held` keys.
fn step(run: &mut GarageRuntime, reference: &ReferenceManifest, player_held: u16) {
    let input = FrameInput {
        player_held,
        ..Default::default()
    };
    frame_with(run, reference, input);
}

#[test]
#[ignore = "requires the installed source export"]
fn bike01_kick_starts_and_rides_upright() {
    let (reference, ordinal, vehicle) = bike_reference("bike01", |v| v.balance.is_some());
    let balance = vehicle.balance.unwrap();
    let (mut run, key) = seated_bike(&reference, ordinal, |s| s.driver);

    // Ignition on and 10 m/s forward: the kick start runs the engine.
    run.vehicles.get_mut(&key).unwrap().ignition.ignition = true;
    let chassis = run.physics.chassis_handle(&key);
    run.physics.edit_body(chassis, |body| {
        let forward = *body.rotation() * Vector::NEG_Z;
        body.set_linvel(forward * 10., true);
    });
    let started = (0..(3. / FIXED_DT) as usize).any(|_| {
        step(&mut run, &reference, player_keys::GAS);
        running(&run, &key)
    });
    assert!(started, "kick start runs the engine");

    // Ride 10 s with throttle and a steer change. The roll stays inside the balance limits.
    let start = run.physics.snapshot(&key).pose.position;
    for i in 0..(10. / FIXED_DT) as usize {
        let steer = if i * 2 < (10. / FIXED_DT) as usize {
            player_keys::STEER_LEFT
        } else {
            player_keys::STEER_RIGHT
        };
        step(&mut run, &reference, player_keys::GAS | steer);
        let roll = roll(&run, &key);
        assert!(
            roll > balance.min_angle && roll < balance.max_angle,
            "step {i}: roll {roll} outside {}..{}",
            balance.min_angle,
            balance.max_angle
        );
    }
    let moved = distance(start, run.physics.snapshot(&key).pose.position);
    assert!(moved > 20., "rode {moved} m");
    assert!(running(&run, &key));
}

#[test]
#[ignore = "requires the installed source export"]
fn bike03_pedals_then_kick_starts() {
    let (reference, ordinal, vehicle) = bike_reference("bike03", |v| v.pedals.is_some());
    let pedal_seat = vehicle.seats[vehicle.pedals.as_ref().unwrap().seat as usize]
        .source
        .clone();
    let (mut run, key) = seated_bike(&reference, ordinal, |s| s.source == pedal_seat);
    run.vehicles.get_mut(&key).unwrap().ignition.ignition = true;

    // Pedal 3 s from rest: pedal torque speeds the bike up with the engine off.
    let start = run.physics.snapshot(&key).pose.position;
    let mut torque = false;
    for _ in 0..(3. / FIXED_DT) as usize {
        step(&mut run, &reference, player_keys::GAS);
        torque |= run.vehicles[&key].pedals.output > 0.;
        if running(&run, &key) {
            break;
        }
    }
    assert!(torque, "pedal torque");
    let moved = distance(start, run.physics.snapshot(&key).pose.position);
    assert!(moved > 1., "pedaled {moved} m");

    // Keep pedaling: the wheel rpm passes shutdown rpm and the kick start runs the engine.
    let started = (0..(30. / FIXED_DT) as usize).any(|_| {
        step(&mut run, &reference, player_keys::GAS);
        running(&run, &key)
    });
    let moved = distance(start, run.physics.snapshot(&key).pose.position);
    assert!(started, "pedaling kick starts the engine after {moved} m");

    // The pedals give no torque while the engine runs.
    for i in 0..(2. / FIXED_DT) as usize {
        step(&mut run, &reference, player_keys::GAS);
        let pedals = run.vehicles[&key].pedals;
        assert_eq!(pedals.output, 0., "step {i}: pedal torque");
    }
    assert!(running(&run, &key));
    assert_eq!(run.vehicles[&key].pedals.gas, 0.);
}
