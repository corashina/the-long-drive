use super::*;
use crate::reference::WheelFrictionCurve;
use crate::vehicle::{PhysicsWorld, collider_profile};
use rapier3d::prelude::{ColliderBuilder, RigidBodyBuilder};

fn sled_collider() -> WheelColliderReference {
    let curve = WheelFrictionCurve {
        extremum_slip: 0.4,
        extremum_value: 1.0,
        asymptote_slip: 0.8,
        asymptote_value: 0.5,
        stiffness: 1.0,
    };
    WheelColliderReference {
        radius: 0.1,
        suspension_distance: 0.2,
        center: [0.0; 3],
        spring: 2000.0,
        damper: 200.0,
        target_position: 0.5,
        forward_friction: curve,
        sideways_friction: curve,
        wheel_mass_kg: 1.0,
        rotational_damping: 0.25,
        force_application_distance_m: 0.0,
    }
}

#[test]
fn six_wheels_carry_a_box_and_a_disabled_set_does_not() {
    let mut world = PhysicsWorld::empty(0, 0);
    world.insert_box([0.0, -0.5, 0.0], [50.0, 0.5, 50.0]);
    // The box bottom is 0.1 m below the wheel connections.
    let (body, _) = world.insert(
        RigidBodyBuilder::dynamic().translation(Vector::Y * 0.4),
        ColliderBuilder::cuboid(0.3, 0.1, 0.6).mass(60.0),
    );
    world.refresh_queries();
    let collider = sled_collider();
    let mounts: [WheelMount; 6] = std::array::from_fn(|i| WheelMount {
        connection: Vector::new(
            if i < 3 { -0.25 } else { 0.25 },
            0.0,
            (i % 3) as f32 * 0.5 - 0.5,
        ),
        collider: &collider,
        profile: collider_profile(&collider),
    });
    let mut set = WheelSet::new(body, &mounts);
    let torques = [WheelTorques {
        motor_nm: 1.0,
        brake_nm: 0.0,
    }; 6];
    let steer = [0.0; 6];
    for _ in 0..(3.0 / FIXED_DT) as usize {
        world.step_wheel_set(&mut set, &torques, &steer);
        world.step_physics();
    }
    for wheel in set.wheels() {
        assert!(wheel.contact.is_some());
        assert!(
            (wheel.suspension_length - 0.1).abs() < 0.02,
            "{}",
            wheel.suspension_length
        );
    }
    let carried = *world.rapier().bodies[body].position();
    assert!((carried.rotation * Vector::Y).y > 0.999);
    assert!((carried.translation.y - 0.2).abs() < 0.02, "{carried:?}");

    set.enabled = false;
    for _ in 0..(2.0 / FIXED_DT) as usize {
        world.step_wheel_set(&mut set, &torques, &steer);
        world.step_physics();
    }
    // The box falls onto its own collider.
    let rested = world.rapier().bodies[body].translation().y;
    assert!((rested - 0.1).abs() < 0.02, "{rested}");
}
