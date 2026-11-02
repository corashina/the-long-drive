use super::*;
use crate::reference::{ItemId, WheelColliderReference, WheelHubReference};
use crate::simulation::{DriveInput, FIXED_DT};
use crate::vehicle::body::{VehicleBody, VehicleControls, VehicleWheel};
use crate::vehicle::query_bounds::QueryBounds;
use crate::vehicle::{DriveTorques, PhysicsWorld};
use crate::world::ObjectKey;
use rapier3d::prelude::{ColliderBuilder, RigidBodyBuilder};

// Exported Lada values (Steam build 11140890).
const SLIP: WheelSlip = WheelSlip {
    tire: [1.25, 0.75, 1.25, 0.75],
    rim: [0.4, 0.3],
};
const TIRE: WheelAssembly = WheelAssembly::Tire {
    radius_m: 0.3,
    mass_kg: 20.0,
    quality: 1.0,
};
const RIM: WheelAssembly = WheelAssembly::Rim {
    radius_m: 0.2,
    mass_kg: 10.0,
};

/// One Lada wheel: its collider and drive rules.
struct Slot {
    collider: WheelColliderReference,
    rules: VehicleWheel,
}

fn garage_vehicle_key(_: u8) -> ObjectKey {
    ObjectKey::Imported(ItemId(1))
}

fn lada_slot(steer: bool, drive: bool) -> Slot {
    Slot {
        collider: WheelColliderReference {
            radius: 0.3,
            suspension_distance: 0.3,
            center: [0.0; 3],
            spring: 25000.0,
            damper: 1500.0,
            target_position: 0.45,
            forward_friction: WheelFrictionCurve {
                extremum_slip: 0.4,
                extremum_value: 1.0,
                asymptote_slip: 0.8,
                asymptote_value: 0.5,
                stiffness: 0.985,
            },
            sideways_friction: WheelFrictionCurve {
                extremum_slip: 0.2,
                extremum_value: 1.0,
                asymptote_slip: 0.5,
                asymptote_value: 0.75,
                stiffness: 0.85,
            },
            wheel_mass_kg: 20.0,
            rotational_damping: 0.25,
            force_application_distance_m: 0.0,
        },
        rules: VehicleWheel {
            steer,
            drive,
            brake: true,
            handbrake: steer,
            pedal: false,
            sideways_stiffness_modifier: 0.0,
            hub: WheelHubReference {
                radius_m: 0.04,
                mass_kg: 5.0,
                forward_stiffness: 0.1,
                sideways_stiffness: 1.0,
            },
        },
    }
}

/// A 960 kg box car on flat ground. Slots 1 and 2 drive. Slots 0 and 3 steer.
fn lada(assembly: WheelAssembly) -> PhysicsWorld {
    let mut physics = rapier3d::prelude::PhysicsWorld::new();
    physics.integration_parameters.dt = FIXED_DT;
    let mut world = PhysicsWorld {
        inactive_bodies: Default::default(),
        body_mass_override: None,
        body_cargo: Vec::new(),
        center_of_mass: Vec::new(),
        physics,
        query_bounds: QueryBounds::default(),
        presentation_bodies: Default::default(),
        chassis: Vec::new(),
        vehicles: Default::default(),
        hit_collector: Default::default(),
        hits: Vec::new(),
        trigger_enters: Vec::new(),
        ignored: Default::default(),
        sleep_speed: rapier3d::prelude::RigidBodyActivation::default_normalized_linear_threshold(),
    };
    world.insert_box([0.0, -0.5, 0.0], [500.0, 0.5, 500.0]);
    let (chassis, _) = world.insert(
        RigidBodyBuilder::dynamic().translation(Vector::Y * 0.5),
        ColliderBuilder::cuboid(0.8, 0.25, 2.0).mass(960.0),
    );
    let corners = [(-0.7, -1.2), (-0.7, 1.2), (0.7, 1.2), (0.7, -1.2)];
    let references: [Slot; 4] =
        std::array::from_fn(|i| lada_slot(i == 0 || i == 3, i == 1 || i == 2));
    let mounts: Vec<_> = (0..4)
        .map(|i| {
            (
                Vector::new(corners[i].0, 0.0, corners[i].1),
                &references[i].collider,
                references[i].rules,
            )
        })
        .collect();
    let mut vehicle = VehicleBody::new(
        chassis,
        SLIP,
        &mounts,
        VehicleControls {
            max_steer_degrees: 40.0,
            brake_torque_nm: 600.0,
            handbrake_torque_nm: 600.0,
            handbrake: false,
        },
    );
    for index in 0..4 {
        vehicle.assemblies[index] = assembly;
        vehicle.wheels.set_profile(
            index,
            resolve_profile(&vehicle.rules[index], &SLIP, assembly),
        );
    }
    world.insert_vehicle(garage_vehicle_key(0), vehicle);
    world.refresh_queries();
    world
}

fn drive(world: &mut PhysicsWorld, seconds: f32, input: DriveInput, motor_nm: f32) {
    for _ in 0..(seconds / FIXED_DT) as usize {
        world.apply_vehicle_forces(
            &garage_vehicle_key(0),
            input,
            DriveTorques {
                motor_nm,
                engine_brake_nm: 0.0,
                pedal_nm: None,
            },
        );
        world.step_physics();
    }
}

fn throttle() -> DriveInput {
    DriveInput {
        throttle: 1.0,
        ..Default::default()
    }
}

#[test]
fn friction_curve_follows_source_points() {
    let curve = lada_slot(false, false).collider.forward_friction;
    assert_eq!(friction(&curve, 0.0), 0.0);
    assert_eq!(friction(&curve, 0.4), 1.0);
    assert_eq!(friction(&curve, -0.4), -1.0);
    assert_eq!(friction(&curve, 0.8), 0.5);
    assert_eq!(friction(&curve, 3.0), 0.5);
    assert!(friction(&curve, 0.2) > 0.5 && friction(&curve, 0.6) > 0.5);
}

#[test]
fn profiles_follow_source_wheel_support() {
    let slot = lada_slot(false, true);
    let new = resolve_profile(&slot.rules, &SLIP, TIRE);
    assert_eq!(
        (new.forward_stiffness, new.sideways_stiffness),
        (1.25, 1.25)
    );
    let worn = resolve_profile(
        &slot.rules,
        &SLIP,
        WheelAssembly::Tire {
            radius_m: 0.3,
            mass_kg: 20.0,
            quality: 0.0,
        },
    );
    assert_eq!(
        (worn.forward_stiffness, worn.sideways_stiffness),
        (0.75, 0.75)
    );
    let rim = resolve_profile(&slot.rules, &SLIP, RIM);
    assert_eq!(
        (rim.radius, rim.forward_stiffness, rim.sideways_stiffness),
        (0.2, 0.4, 0.3)
    );
    let hub = resolve_profile(&slot.rules, &SLIP, WheelAssembly::Hub);
    assert_eq!(
        (hub.radius, hub.forward_stiffness, hub.sideways_stiffness),
        (0.04, 0.1, 1.0)
    );
    assert_eq!(hub.inertia, 0.5 * 5.0 * 0.04 * 0.04);
}

#[test]
fn brake_holds_a_wheel_and_motor_overcomes_it() {
    let slot = lada_slot(false, true);
    let mut wheel = WheelState::new(
        Vector::ZERO,
        resolve_profile(&slot.rules, &SLIP, TIRE),
        0.135,
    );
    let frame = WheelFrame {
        up: Vector::Y,
        axle: Vector::X,
        velocity: Vector::ZERO,
        sprung_mass: 240.0,
        gravity: 9.81,
    };
    let held = WheelTorques {
        motor_nm: 300.0,
        brake_nm: 600.0,
    };
    solve(&mut wheel, &slot.collider, frame, held, FIXED_DT);
    assert_eq!(wheel.omega, 0.0);
    let spun = WheelTorques {
        motor_nm: 900.0,
        brake_nm: 600.0,
    };
    solve(&mut wheel, &slot.collider, frame, spun, FIXED_DT);
    assert!(wheel.omega > 0.0);
}

#[test]
fn car_settles_on_source_suspension() {
    let mut world = lada(TIRE);
    drive(&mut world, 3.0, DriveInput::default(), 0.0);
    let snapshot = world.snapshot(&garage_vehicle_key(0));
    assert!(world.wheel_contacts(&garage_vehicle_key(0)).all(|c| c));
    assert!(snapshot.absolute_speed_kmh() < 0.5);
    for wheel in world.vehicles[&garage_vehicle_key(0)].wheels.wheels() {
        assert!(
            (wheel.suspension_length - 0.135).abs() < 0.02,
            "{}",
            wheel.suspension_length
        );
    }
}

#[test]
fn tires_launch_and_wheels_spin_with_the_road() {
    let mut world = lada(TIRE);
    drive(&mut world, 2.0, DriveInput::default(), 0.0);
    // First gear at peak torque: about 100 Nm * 3.75 * 4.3.
    drive(&mut world, 3.0, throttle(), 1600.0);
    let speed = world.snapshot(&garage_vehicle_key(0)).speed_mps;
    assert!(speed > 5.0, "{speed}");
    let road_rpm = speed / 0.3 * 60.0 / std::f32::consts::TAU;
    let wheel_rpm = world.driven_wheel_rpm(&garage_vehicle_key(0));
    assert!(
        wheel_rpm > road_rpm && wheel_rpm < road_rpm * 1.5,
        "{wheel_rpm} {road_rpm}"
    );
}

#[test]
fn excess_torque_spins_the_driven_wheels() {
    let mut world = lada(TIRE);
    drive(&mut world, 2.0, DriveInput::default(), 0.0);
    drive(&mut world, 0.5, throttle(), 12000.0);
    let road_rpm =
        world.snapshot(&garage_vehicle_key(0)).speed_mps / 0.3 * 60.0 / std::f32::consts::TAU;
    assert!(world.driven_wheel_rpm(&garage_vehicle_key(0)) > road_rpm * 2.0);
}

#[test]
fn bare_rims_grip_less_than_tires() {
    let mut tires = lada(TIRE);
    let mut rims = lada(RIM);
    for world in [&mut tires, &mut rims] {
        drive(world, 2.0, DriveInput::default(), 0.0);
        drive(world, 2.0, throttle(), 3000.0);
    }
    assert!(
        rims.snapshot(&garage_vehicle_key(0)).speed_mps
            < tires.snapshot(&garage_vehicle_key(0)).speed_mps * 0.8
    );
}

#[test]
fn service_brake_works_while_throttle_is_held() {
    let mut world = lada(TIRE);
    drive(&mut world, 2.0, DriveInput::default(), 0.0);
    drive(&mut world, 3.0, throttle(), 1600.0);
    let before = world.snapshot(&garage_vehicle_key(0)).speed_mps;
    let both = DriveInput {
        throttle: 1.0,
        brake: 1.0,
        ..Default::default()
    };
    drive(&mut world, 1.0, both, 1600.0);
    // 2400 Nm of brakes beat 1600 Nm of motor torque.
    assert!(world.snapshot(&garage_vehicle_key(0)).speed_mps < before - 2.0);
}

#[test]
fn handbrake_stops_a_rolling_car() {
    let mut world = lada(TIRE);
    drive(&mut world, 2.0, DriveInput::default(), 0.0);
    drive(&mut world, 3.0, throttle(), 1600.0);
    let handbrake = DriveInput {
        handbrake: 1.0,
        ..Default::default()
    };
    drive(&mut world, 6.0, handbrake, 0.0);
    assert!(world.snapshot(&garage_vehicle_key(0)).speed_mps.abs() < 0.3);
}

#[test]
fn sideways_slide_stops_on_tires() {
    let mut world = lada(TIRE);
    drive(&mut world, 2.0, DriveInput::default(), 0.0);
    let chassis = world.chassis_handle(&garage_vehicle_key(0));
    world.physics.bodies[chassis].set_linvel(Vector::X * 5.0, true);
    drive(&mut world, 2.0, DriveInput::default(), 0.0);
    assert!(world.physics.bodies[chassis].linvel().x.abs() < 0.3);
}

#[test]
fn handbrake_holds_a_parked_car_against_a_walking_push() {
    for push in [Vector::Z * 60.0, Vector::X * 60.0] {
        let mut world = lada(TIRE);
        let parked = DriveInput {
            handbrake: 1.0,
            ..Default::default()
        };
        drive(&mut world, 2.0, parked, 0.0);
        let chassis = world.chassis_handle(&garage_vehicle_key(0));
        let start = world.physics.bodies[chassis].translation();
        // The player pushes before the car step, as in the garage frame.
        for _ in 0..(2.0 / FIXED_DT) as usize {
            let body = &mut world.physics.bodies[chassis];
            let point = body.translation();
            body.apply_impulse_at_point(push, point, true);
            drive(&mut world, FIXED_DT, parked, 0.0);
        }
        let moved = (world.physics.bodies[chassis].translation() - start).length();
        assert!(moved < 0.02, "{push:?} moved {moved}");
    }
}
