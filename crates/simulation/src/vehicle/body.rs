//! Vehicles keyed by world object. A vehicle is a chassis body with a wheel set.
use rapier3d::math::Vector;
use rapier3d::prelude::{ActiveEvents, ColliderHandle, RigidBodyHandle};

use super::wheel::{self, WheelAssembly, WheelSlip};
use super::{
    PhysicsWorld, VEHICLE_COLLIDER_FLAG, VehiclePose, WheelMount, WheelSet, WheelTorques,
    set_collider_enabled, to_rapier,
};
use crate::reference::{
    ReferenceError, ReferenceManifest, WheelColliderReference, WheelHubReference,
    WorldPrototypeReference,
};
use crate::world::ObjectKey;

/// Drive rules of one wheel.
#[derive(Clone, Copy, Debug)]
pub(super) struct VehicleWheel {
    pub hub: WheelHubReference,
    pub sideways_stiffness_modifier: f32,
    pub steer: bool,
    pub drive: bool,
    pub brake: bool,
    pub handbrake: bool,
    /// Source `bikepedalscript.WHDriven`: the wheel takes pedal torque.
    pub pedal: bool,
}

/// Steering and brake values of one vehicle.
pub(super) struct VehicleControls {
    pub max_steer_degrees: f32,
    pub brake_torque_nm: f32,
    pub handbrake_torque_nm: f32,
    pub handbrake: bool,
}

/// One chassis body with its wheel set. Wheel indices are local to the vehicle.
pub(super) struct VehicleBody {
    pub chassis: RigidBodyHandle,
    pub slip: WheelSlip,
    pub wheels: WheelSet,
    pub rules: Vec<VehicleWheel>,
    pub assemblies: Vec<WheelAssembly>,
    pub max_steer: f32,
    pub brake_torque: f32,
    pub handbrake_torque: f32,
    pub last_handbrake: f32,
    /// Mass of items locked to the chassis. `set_vehicle_mass` adds it.
    pub cargo_mass: f32,
    /// Per-step scratch values, one per wheel.
    pub torques: Vec<WheelTorques>,
    pub steer: Vec<f32>,
}

impl VehicleBody {
    /// Each mount is a chassis-space wheel connection, its collider and its rules.
    pub(super) fn new(
        chassis: RigidBodyHandle,
        slip: WheelSlip,
        mounts: &[(Vector, &WheelColliderReference, VehicleWheel)],
        controls: VehicleControls,
    ) -> Self {
        // Wheel assembly sync replaces this before the first drive step.
        let assemblies: Vec<_> = mounts
            .iter()
            .map(|(_, collider, _)| WheelAssembly::Tire {
                radius_m: collider.radius,
                mass_kg: collider.wheel_mass_kg,
                quality: 1.0,
            })
            .collect();
        let wheel_mounts: Vec<_> = mounts
            .iter()
            .zip(&assemblies)
            .map(|((connection, collider, rules), assembly)| WheelMount {
                connection: *connection,
                collider,
                profile: wheel::resolve_profile(rules, &slip, *assembly),
            })
            .collect();
        Self {
            chassis,
            slip,
            wheels: WheelSet::new(chassis, &wheel_mounts),
            rules: mounts.iter().map(|(_, _, rules)| *rules).collect(),
            assemblies,
            max_steer: controls.max_steer_degrees.to_radians(),
            brake_torque: controls.brake_torque_nm,
            handbrake_torque: controls.handbrake_torque_nm,
            last_handbrake: controls.handbrake as u8 as f32,
            cargo_mass: 0.0,
            torques: vec![WheelTorques::default(); mounts.len()],
            steer: vec![0.0; mounts.len()],
        }
    }

    /// Source carscript.DrivenWheelsNum2: driven slots with a mounted rim.
    pub(super) fn driven_wheel_count(&self) -> usize {
        self.assemblies
            .iter()
            .zip(&self.rules)
            .filter(|(assembly, rules)| rules.drive && assembly.has_rim())
            .count()
    }
}

impl PhysicsWorld {
    /// An empty world with capacity for the reference bodies. World cars add vehicles later.
    pub fn new(reference: &ReferenceManifest) -> Self {
        let mut physics = Self::empty(
            0,
            reference.bodies.len() + reference.parts.len() + reference.wheel_parts.len(),
        );
        // PhysX sleeps a body below a mass-normalized kinetic energy, 0.5 * v * v.
        physics.sleep_speed = (2. * reference.physics.sleep_threshold).sqrt();
        physics.refresh_queries();
        physics
    }

    /// Makes an existing world instance root body a vehicle. `colliders` are the live
    /// handles of the record's chassis colliders. The record sets the chassis mass.
    pub fn add_vehicle(
        &mut self,
        key: ObjectKey,
        chassis: RigidBodyHandle,
        colliders: &[ColliderHandle],
        prototype: &WorldPrototypeReference,
    ) -> Result<(), ReferenceError> {
        let record = prototype
            .vehicle
            .as_ref()
            .ok_or_else(|| ReferenceError("Prototype has no vehicle record".into()))?;
        let source_body = prototype
            .bodies
            .get(record.body as usize)
            .ok_or_else(|| ReferenceError("Missing chassis body".into()))?;
        if self.vehicles.contains_key(&key) || self.chassis.contains(&chassis) {
            return Err(ReferenceError("Vehicle already exists".into()));
        }
        for &handle in colliders {
            self.edit_collider(handle, |collider| {
                collider.user_data |= VEHICLE_COLLIDER_FLAG;
                collider
                    .set_active_events(collider.active_events() | ActiveEvents::COLLISION_EVENTS);
                let enabled = collider.is_enabled();
                set_collider_enabled(collider, enabled);
            });
        }
        let to_chassis = to_rapier(VehiclePose {
            position: source_body.pose.translation,
            rotation: source_body.pose.rotation,
        })
        .inverse();
        let mounts: Vec<_> = record
            .wheels
            .iter()
            .enumerate()
            .map(|(index, wheel)| {
                let (node, _) = prototype.root_relative(wheel.node as usize);
                let connection = (to_chassis * node) * Vector::from_array(wheel.collider.center);
                let rules = VehicleWheel {
                    hub: record.hub,
                    sideways_stiffness_modifier: wheel.sideways_stiffness_modifier,
                    steer: wheel.steer,
                    drive: wheel.drive,
                    brake: wheel.brake,
                    handbrake: wheel.handbrake,
                    pedal: record
                        .pedals
                        .as_ref()
                        .is_some_and(|p| p.driven_wheels.contains(&(index as u32))),
                };
                (connection, &wheel.collider, rules)
            })
            .collect();
        let vehicle = VehicleBody::new(
            chassis,
            WheelSlip {
                tire: record.wheel_slip,
                rim: record.rim_slip,
            },
            &mounts,
            VehicleControls {
                max_steer_degrees: record.max_steer_degrees,
                brake_torque_nm: record.brake_torque_nm,
                handbrake_torque_nm: record.handbrake_torque_nm,
                handbrake: record.handbrake,
            },
        );
        self.edit_body(chassis, |body| body.enable_ccd(true));
        self.apply_vehicle_mass(chassis, record.mass_kg);
        self.insert_vehicle(key, vehicle);
        Ok(())
    }

    /// Removes the vehicle data. The chassis body and its colliders stay.
    pub fn remove_vehicle(&mut self, key: &ObjectKey) -> bool {
        let Some(vehicle) = self.vehicles.remove(key) else {
            return false;
        };
        if let Some(index) = self.chassis.iter().position(|c| *c == vehicle.chassis) {
            self.chassis.swap_remove(index);
        }
        true
    }

    pub(super) fn insert_vehicle(&mut self, key: ObjectKey, vehicle: VehicleBody) {
        self.chassis.push(vehicle.chassis);
        self.vehicles.insert(key, vehicle);
    }
}

#[cfg(test)]
pub mod tests {
    use super::*;
    use crate::reference::ItemId;
    use crate::simulation::{DriveInput, FIXED_DT};
    use crate::vehicle::DriveTorques;
    use rapier3d::prelude::{ColliderBuilder, RigidBodyBuilder};
    use serde_json::json;

    /// A 820 kg box car prototype with four wheels. The front wheels steer, the rear wheels drive.
    pub fn car() -> WorldPrototypeReference {
        let src = |id: u32| json!({"file": "a", "path_id": id});
        let pose = |x: f32, z: f32| json!({"translation": [x, 0, z], "rotation": [0, 0, 0, 1], "scale": [1, 1, 1]});
        let node = |id: u32, x: f32, z: f32| json!({"source": src(id), "name": "n", "parent": if id == 1 { None } else { Some(0) }, "active": true, "renderer_enabled": true, "layer": 0, "transform": pose(x, z), "gltf_node": id - 1, "biome_materials": false});
        let friction = json!({"extremum_slip": 0.4, "extremum_value": 1.0, "asymptote_slip": 0.8, "asymptote_value": 0.5, "stiffness": 1.0});
        let corners = [(-0.7, -1.2), (0.7, -1.2), (-0.7, 1.2), (0.7, 1.2)];
        let wheels: Vec<_> = (0..4).map(|i| json!({"source": src(20 + i), "node": i + 1, "slot": i + 1, "mount": i + 1,
            "steer": i < 2, "drive": i >= 2, "brake": true, "handbrake": i >= 2, "sideways_stiffness_modifier": 1.0,
            "collider": {"radius": 0.3, "suspension_distance": 0.3, "center": [0, 0, 0], "spring": 25000.0, "damper": 1500.0, "target_position": 0.45, "forward_friction": friction, "sideways_friction": friction, "wheel_mass_kg": 20.0, "rotational_damping": 0.25, "force_application_distance_m": 0.0}})).collect();
        let effects: Vec<_> = (0..4).map(|i| json!({"source": src(30 + i), "pose": pose(corners[i as usize].0, corners[i as usize].1),
            "rate": 1.0, "ray_mask": 1, "own_gains": null, "dust": null, "grass": []})).collect();
        let mut nodes = vec![node(1, 0.0, 0.0)];
        nodes.extend((0..4).map(|i| node(i + 2, corners[i as usize].0, corners[i as usize].1)));
        serde_json::from_value(json!({
            "source": src(1), "role": "structure", "asset": 5, "nodes": nodes,
            "controls": [], "excluded_actions": [], "deferred_materials": [], "skins": [],
            "bodies": [{"source": src(10), "node": 0, "mass_kg": 900.0, "linear_damping": 0.0, "angular_damping": 0.05, "kinematic": false, "use_gravity": true, "constraints": 0, "pose": pose(0.0, 0.0)}],
            "colliders": [], "prefab_pose": pose(0.0, 0.0),
            "vehicle": {
                "source": src(11), "body": 0, "car_type": 1, "colliders": [0], "mass_kg": 820.0, "serialized_mass_kg": 900.0,
                "fuel_tank": null, "fuel_tank_slot": null, "electronics": null, "starter_delay_seconds": 1.0, "balance": null, "pedals": null, "no_gas_while_pedaling": false,
                "driving": {"initial_gear": 1, "force_automatic": false, "play_shift_sounds_in_automatic": false},
                "handbrake": false, "hub": {"radius_m": 0.3, "mass_kg": 10.0, "forward_stiffness": 1.0, "sideways_stiffness": 1.0},
                "wheels": wheels,
                "gear_ratios": [-3.0, 0.0, 3.0], "gear_free_run": [false, true, false], "final_drive_ratio": 4.0,
                "max_steer_degrees": 35.0, "brake_torque_nm": 1000.0, "handbrake_torque_nm": 2000.0,
                "wheel_slip": [1.25, 0.75, 1.25, 0.75], "rim_slip": [0.4, 0.3], "auto_shift_stop_speed": 1.0,
                "auto_shift_min_rpm": 1500.0, "auto_shift_max_rpm": 4000.0, "dont_ignite_with_throttle_in_automatic": false,
                "auto_move_min": 0.3, "collision_gain": 1.0, "exhaust": [], "seats": [],
                "crash": {"multiplier": 1.0, "blood": 1.0, "horn": 1.0, "fall_off": 1.0}, "blood": null, "blood_clip": null, "condition": null,
                "drive_frames": {"left_hand": null, "right_hand": null, "left_leg": null, "right_leg": null}, "reach": crate::reference::reach_fixture(),
                "wheel_effects": effects, "effect_gains": {"ground": 1.0, "spring": 1.0, "spring_pitch": 1.0, "slide_angle": 20.0, "max_slide_volume_speed": 10.0},
                "lever": null, "key_steer_multiplier": 0.0, "oil_led_two_stroke": false,
                "lights": [], "light_switch": null, "instruments": null, "high_beams": [], "plates": [], "mirrors": [],
                "gauge_limits": crate::reference::gauge_limits_fixture(), "kmh_meters": []
            }
        }))
        .unwrap()
    }

    /// Two copies of [`car`] on a ground box, at x = -5 and x = 5. Returns their keys and chassis.
    pub fn two_cars(
        prototype: &WorldPrototypeReference,
    ) -> (PhysicsWorld, [ObjectKey; 2], [RigidBodyHandle; 2]) {
        let mut physics = PhysicsWorld::empty(2, 2);
        physics.insert_box([0.0, -0.5, 0.0], [500.0, 0.5, 500.0]);
        let keys = [ItemId(1), ItemId(2)].map(ObjectKey::Imported);
        let chassis = [-5.0, 5.0].map(|x| {
            // A world instance root body: zero-density colliders and authored mass.
            physics.insert(
                RigidBodyBuilder::dynamic()
                    .translation(Vector::new(x, 0.5, 0.0))
                    .additional_mass(900.0),
                ColliderBuilder::cuboid(0.8, 0.25, 2.0).density(0.0),
            )
        });
        for (key, (body, collider)) in keys.iter().zip(chassis) {
            physics
                .add_vehicle(key.clone(), body, &[collider], prototype)
                .unwrap();
        }
        physics.refresh_queries();
        (physics, keys, chassis.map(|(body, _)| body))
    }

    #[test]
    fn removing_one_vehicle_keeps_the_other_driving() {
        let prototype = car();
        let (mut physics, keys, chassis) = two_cars(&prototype);
        for body in chassis {
            assert_eq!(physics.rapier().bodies[body].mass(), 820.0);
        }
        // Mass changes keep the centre of mass override.
        let com = Vector::new(0.0, -0.1, 0.2);
        let second = physics.chassis_handle(&keys[1]);
        physics.set_center_of_mass(second, com).unwrap();
        physics.set_vehicle_mass(&keys[1], 820.0);
        let local = physics.rapier().bodies[second]
            .mass_properties()
            .local_mprops;
        assert!((local.local_com - com).length() < 1e-4, "{local:?}");
        assert!((local.mass() - 820.0).abs() < 1e-3);

        let throttle = DriveInput {
            throttle: 1.0,
            ..Default::default()
        };
        let torques = DriveTorques {
            motor_nm: 1600.0,
            engine_brake_nm: 0.0,
            pedal_nm: None,
        };
        let drive = |physics: &mut PhysicsWorld, keys: &[ObjectKey], seconds: f32| {
            for _ in 0..(seconds / FIXED_DT) as usize {
                for key in keys {
                    physics.apply_vehicle_forces(key, throttle, torques);
                }
                physics.step_physics();
            }
        };
        drive(&mut physics, &keys, 1.0);
        for key in &keys {
            assert!(physics.wheel_contacts(key).all(|contact| contact));
            assert!(physics.snapshot(key).speed_mps > 1.0);
        }
        let first = physics.chassis_handle(&keys[0]);
        assert!(physics.remove_vehicle(&keys[0]));
        assert!(!physics.remove_vehicle(&keys[0]));
        assert_eq!(physics.vehicle_of(first), None);
        assert_eq!(physics.vehicle_count(), 1);
        let before = physics.snapshot(&keys[1]).speed_mps;
        drive(&mut physics, &keys[1..], 1.0);
        assert!(physics.snapshot(&keys[1]).speed_mps > before + 1.0);
    }

    #[test]
    fn pedal_torque_drives_pedal_wheels_until_the_engine_gives_torque() {
        let mut prototype = car();
        prototype.vehicle.as_mut().unwrap().pedals = Some(
            serde_json::from_value(json!({"source": {"file": "a", "path_id": 12},
                "driven_wheels": [2, 3], "driven_slots": [3, 4], "seat": 0, "pedals": [], "pedals_inverse": [],
                "feet_holders": [], "axes": [], "axis_index": [], "ratio": 1.0, "speed_modifier": 1.0,
                "input_debounce": 0.5, "gas_tap": 1.0, "player_use": true}))
            .unwrap(),
        );
        let (mut physics, keys, chassis) = two_cars(&prototype);
        let throttle = DriveInput {
            throttle: 1.0,
            ..Default::default()
        };
        let pedal = DriveTorques {
            pedal_nm: Some(400.0),
            ..Default::default()
        };
        let engine = DriveTorques {
            motor_nm: -1600.0,
            ..pedal
        };
        for _ in 0..(1.0 / FIXED_DT) as usize {
            physics.apply_vehicle_forces(&keys[0], throttle, pedal);
            physics.apply_vehicle_forces(&keys[1], throttle, engine);
            physics.step_physics();
        }
        let [pedaled, driven] = chassis.map(|body| physics.rapier().bodies[body].linvel().z);
        assert!(pedaled.abs() > 0.5, "{pedaled}");
        assert!(
            driven.abs() > 0.5 && driven.signum() != pedaled.signum(),
            "{driven}"
        );
    }
}
