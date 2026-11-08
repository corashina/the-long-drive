//! Headless scenario tests: driving and the streamed world.
use crate::world::stream::tests::kit::*;


/// The start car drives away from the start along the road at 25 m/s. Each 100 m the
/// road plan rereads about 256 candidates. After about 340 m the terrain under the cars parked
/// at the start unloads, and those cars freeze. The step never holds for 1 s, so the car does
/// not stop in place while the world loads.
#[test]
#[ignore = "requires the installed source export"]
fn driving_away_from_the_start_never_stops_the_car() {
    use crate::simulation::FIXED_DT;
    const SPEED: f64 = 25.;
    let (reference, mut run) = load_garage();
    settle(&mut run, &reference);
    let key = run.vehicle.clone().unwrap();
    let car = run.physics.snapshot(&key).pose;
    stand_near(&mut run, &reference, car.position, 2.);
    act(&mut run, &reference, r#"{"action":"seat","seat":"driver"}"#);
    let start = run.stream.origin.absolute(car.position)[2] - 40.;
    let (mut travelled, mut held) = (0., 0);
    while travelled < 2000. {
        if run.world_supported() {
            held = 0;
            travelled += SPEED * f64::from(FIXED_DT);
            let point = run
                .stream
                .road_surface_points()
                .find(|p| p[2] <= start - travelled)
                .expect("the road reaches the car");
            let mut position = run.stream.origin.local(point);
            position[1] += 1.;
            run.physics.reset_preserving_wheels(
                &key,
                crate::vehicle::VehiclePose {
                    position,
                    rotation: car.rotation,
                },
            );
            run.physics.refresh_queries();
        } else {
            held += 1;
            let absolute = run
                .stream
                .origin
                .absolute(run.physics.snapshot(&key).pose.position);
            let bounds = crate::world::WorldBounds {
                min: absolute.map(|v| v - 8.),
                max: absolute.map(|v| v + 8.),
            };
            assert!(
                held < 50,
                "the step holds at {travelled:.0} m: {:?}",
                run.stream.ground_gap(bounds)
            );
        }
        frame(&mut run, &reference);
    }
}

/// The player drives a spawned car away along the road at 25 m/s. The start car and the
/// cars near it stay parked. After about 340 m the terrain under them unloads, and they freeze.
/// The step never waits for that ground, so the driven car does not stop in place.
#[test]
#[ignore = "requires the installed source export"]
fn cars_parked_at_the_start_never_stop_a_far_car() {
    drive_spawned_car(25., 1000., 50);
}

/// A spawned car drives along the road at 40 m/s. Near tiles and roadside structures load
/// behind it. The step waits only for ground, and ground patches carry the car over a tile that
/// is not loaded. So the step holds for at most two frames at a time.
#[test]
#[ignore = "requires the installed source export"]
fn a_fast_car_never_waits_for_the_world() {
    drive_spawned_car(40., 2000., 3);
}

/// Spawns a car, seats the player and moves the car along the road at `speed` for `distance`.
/// Fails when the step holds for `limit` frames in a row.
fn drive_spawned_car(speed: f64, distance: f64, limit: u32) {
    use crate::simulation::FIXED_DT;
    let (reference, mut run) = outdoors();
    let parked: Vec<_> = run.vehicles.keys().cloned().collect();
    act(
        &mut run,
        &reference,
        r#"{"action":"spawn","name":"Car01Full","forward":8}"#,
    );
    let mut key = None;
    for _ in 0..500 {
        key = run.vehicles.keys().find(|k| !parked.contains(k)).cloned();
        if key.is_some() {
            break;
        }
        frame(&mut run, &reference);
    }
    let key = key.expect("the spawned car loads");
    let car = run.physics.snapshot(&key).pose;
    stand_near(&mut run, &reference, car.position, 2.);
    act(&mut run, &reference, r#"{"action":"seat","seat":"driver"}"#);
    frame(&mut run, &reference);
    assert_eq!(
        run.vehicle.as_ref(),
        Some(&key),
        "the player sits in the spawned car"
    );
    let start = reference.area.player_spawn.translation[2] as f64 - 40.;
    let (mut travelled, mut held) = (0., 0);
    while travelled < distance {
        if run.world_supported() {
            held = 0;
            travelled += speed * f64::from(FIXED_DT);
            let point = run
                .stream
                .road_surface_points()
                .find(|p| p[2] <= start - travelled)
                .expect("the road reaches the car");
            let mut position = run.stream.origin.local(point);
            position[1] += 1.;
            run.physics.reset_preserving_wheels(
                &key,
                crate::vehicle::VehiclePose {
                    position,
                    rotation: car.rotation,
                },
            );
            run.physics.refresh_queries();
        } else {
            held += 1;
            assert!(
                held < limit,
                "the step holds at {travelled:.0} m: {}",
                run.hold_report()
            );
        }
        frame(&mut run, &reference);
    }
}

/// The start car rolls forward. Its dashboard is a mounted gauge part with content poses.
/// The speed needle turns, and the last pose row of each needle and digit is its gauge rotation.
#[test]
#[ignore = "requires the installed source export"]
fn the_start_car_gauges_turn_over_the_part_poses() {
    use crate::boundary::NodePoseRow;
    let (reference, mut run) = load_garage();
    settle(&mut run, &reference);
    let key = run.vehicle.clone().unwrap();
    let car = run.physics.snapshot(&key).pose;
    stand_near(&mut run, &reference, car.position, 2.);
    act(&mut run, &reference, r#"{"action":"seat","seat":"driver"}"#);
    let handle = run.physics.chassis_handle(&key);
    let mut out = crate::boundary::SimFrame::default();
    let mut posed = std::collections::HashMap::new();
    for _ in 0..100 {
        run.physics.edit_body(handle, |b| {
            let forward = b.rotation() * rapier3d::math::Vector::Z;
            b.set_linvel(forward * 10., true);
        });
        frame(&mut run, &reference);
        run.fill_frame(&reference, 1., &mut out);
        let gauge = run.powertrain.gauges().next().unwrap();
        let slot = run.structure_views.slots[&gauge.parent];
        for row in out.structures.iter().filter(|r| r.slot == slot) {
            for pose in &out.structure_poses[row.poses[0] as usize..row.poses[1] as usize] {
                posed.insert(pose.node, pose.kind);
            }
        }
    }
    let gauge = run.powertrain.gauges().next().unwrap();
    let speed = gauge
        .record
        .needles
        .iter()
        .position(|n| n.signal == "speed")
        .unwrap();
    let rest = crate::simulation::instruments::needle_angle(&gauge.record.needles[speed], 0.);
    assert!(
        gauge.state.needle_angles[speed] != rest,
        "the speed needle turns"
    );
    let digits = gauge
        .record
        .counters
        .iter()
        .flat_map(|c| &c.digits)
        .map(|d| d.node);
    for node in gauge.record.needles.iter().map(|n| n.node).chain(digits) {
        assert_eq!(
            posed.get(&node.0),
            Some(&NodePoseRow::ROTATION),
            "node {}",
            node.0
        );
    }
}

/// The start car drives. Each frame between two steps shows its mounted wheels at their rest
/// offset on the car body, so the wheels do not trail or lead the body.
#[test]
#[ignore = "requires the installed source export"]
fn the_start_car_wheels_stay_on_the_driving_body() {
    use crate::boundary::NodePoseRow;
    use crate::world::ObjectKey;
    use rapier3d::math::{Pose, Rotation, Vector};
    let (reference, mut run) = load_garage();
    settle(&mut run, &reference);
    let key = run.vehicle.clone().unwrap();
    let ObjectKey::Generated(car_id) = key.clone() else {
        panic!("the start car is a world car");
    };
    let car = run.physics.snapshot(&key).pose;
    stand_near(&mut run, &reference, car.position, 2.);
    act(&mut run, &reference, r#"{"action":"seat","seat":"driver"}"#);
    let handle = run.physics.chassis_handle(&key);
    let mut out = crate::boundary::SimFrame::default();
    for _ in 0..50 {
        run.physics.edit_body(handle, |b| {
            let forward = b.rotation() * Vector::Z;
            b.set_linvel(forward * 20., true);
        });
        frame(&mut run, &reference);
    }
    run.fill_frame(&reference, 0.5, &mut out);
    let body_row = |slot: u32| {
        let row = out.structures.iter().find(|r| r.slot == slot)?;
        let pose = out.structure_poses[row.poses[0] as usize..row.poses[1] as usize]
            .iter()
            .find(|p| p.kind == NodePoseRow::BODY && p.node == 0)?;
        Some(Pose::from_parts(
            Vector::from_array(pose.translation),
            Rotation::from_array(pose.rotation),
        ))
    };
    let shown_car = body_row(run.structure_views.slots[&car_id]).expect("the car is shown");
    let chassis = *run.physics.rapier().bodies[handle].position();
    let mut wheels = 0;
    for item in run.area.items.iter().filter(|i| i.body == Some(handle)) {
        let ObjectKey::Generated(id) = run.area.items.object_key(item.id) else {
            continue;
        };
        let Some(&slot) = run.structure_views.slots.get(&id) else {
            continue;
        };
        let structure = run
            .stream
            .frame()
            .structures
            .iter()
            .find(|s| s.placement.id == id)
            .unwrap();
        if structure.bodies.contains(&handle) {
            continue;
        }
        let own = *run.physics.rapier().bodies[structure.bodies[0]].position();
        let rest = chassis.inverse() * own;
        let shown = body_row(slot).expect("a mounted part is shown with its car");
        let error = (shown_car.inverse() * shown).translation - rest.translation;
        assert!(
            error.length() < 1e-3,
            "part {id:?} is {error} m off its car"
        );
        wheels += 1;
    }
    assert!(wheels >= 4, "the start car has its wheels");
}

/// A spawned car drives at 10 m/s. Each frame between two steps shows every wheel mesh at its
/// rest offset from the wheel center, so the wheels do not trail behind their arches.
#[test]
#[ignore = "requires the installed source export"]
fn the_wheel_meshes_stay_on_the_driving_wheels() {
    use crate::boundary::NodePoseRow;
    use crate::world::ObjectKey;
    use rapier3d::math::{Pose, Rotation, Vector};
    let (reference, mut run) = outdoors();
    let parked: Vec<_> = run.vehicles.keys().cloned().collect();
    act(
        &mut run,
        &reference,
        r#"{"action":"spawn","name":"Car01Full","forward":8}"#,
    );
    let mut key = None;
    for _ in 0..500 {
        key = run.vehicles.keys().find(|k| !parked.contains(k)).cloned();
        if key.is_some() {
            break;
        }
        frame(&mut run, &reference);
    }
    let key = key.expect("the spawned car loads");
    seconds(&mut run, &reference, 2.);
    let ObjectKey::Generated(car_id) = key.clone() else {
        panic!("the spawned car is a world car");
    };
    let handle = run.physics.chassis_handle(&key);
    let mut out = crate::boundary::SimFrame::default();
    let mut checked = 0;
    let mut plates = 0;
    for _ in 0..30 {
        run.physics.edit_body(handle, |b| {
            let forward = b.rotation() * Vector::Z;
            b.set_linvel(forward * 10. + Vector::Y * b.linvel().y, true);
        });
        frame(&mut run, &reference);
        run.fill_frame(&reference, 0.5, &mut out);
        let structure = run
            .stream
            .frame()
            .structures
            .iter()
            .find(|s| s.placement.id == car_id)
            .unwrap();
        let prototype = reference
            .world
            .prototypes
            .find(&structure.placement.prototype)
            .unwrap();
        let slot = run.structure_views.slots[&car_id];
        let row = out.structures.iter().find(|r| r.slot == slot).unwrap();
        let rows = &out.structure_poses[row.poses[0] as usize..row.poses[1] as usize];
        let pose = |p: &NodePoseRow| {
            Pose::from_parts(
                Vector::from_array(p.translation),
                Rotation::from_array(p.rotation),
            )
        };
        let car = rows
            .iter()
            .find(|p| p.kind == NodePoseRow::BODY && p.node == 0)
            .map(pose)
            .unwrap();
        let chassis = *run.physics.rapier().bodies[handle].position();
        // The plate meshes stay on the car. They lag one mechanism update, about 0.2 m here.
        let on_plate = |mut node: Option<u32>| {
            while let Some(n) = node {
                if prototype.nodes[n as usize].name == "plate" {
                    return true;
                }
                node = prototype.nodes[n as usize].parent;
            }
            false
        };
        for p in rows
            .iter()
            .filter(|p| p.kind == NodePoseRow::CONTENT && on_plate(Some(p.node)))
        {
            let rest = prototype.root_relative(p.node as usize).0.translation;
            let error = ((car.inverse() * pose(p)).translation - rest).length();
            assert!(error < 0.3, "a plate is {error} m off its car");
            plates += 1;
        }
        let record = prototype.vehicle.as_ref().unwrap();
        for (index, wheel) in record.wheels.iter().enumerate() {
            let center = (chassis.inverse()
                * crate::vehicle::to_rapier(run.physics.wheel_pose(&key, index)))
            .translation;
            let rest = prototype.root_relative(wheel.mount as usize).0.translation;
            for p in rows.iter().filter(|p| p.kind == NodePoseRow::CONTENT) {
                // Only nodes on a body under the wheel mount, such as the rim and the tire.
                let mut node = Some(p.node);
                let mut on_wheel = false;
                while let Some(n) = node.filter(|&n| n != wheel.mount) {
                    on_wheel |= prototype.bodies.iter().any(|b| b.node == n);
                    node = prototype.nodes[n as usize].parent;
                }
                if node.is_none() || !on_wheel {
                    continue;
                }
                let offset =
                    (prototype.root_relative(p.node as usize).0.translation - rest).length();
                let shown = (car.inverse() * pose(p)).translation;
                let error = (shown - center).length() - offset;
                assert!(
                    error < 0.01,
                    "node {} of wheel {index} is {error} m off its wheel",
                    prototype.nodes[p.node as usize].name
                );
                checked += 1;
            }
        }
    }
    assert!(checked > 0, "the wheels have content poses");
    assert!(plates > 0, "the plate has content poses");
}
