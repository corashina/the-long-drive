//! Shared car state of source `carscript` (`UpdMulti`, `CarSyncValues`, `UpdMulti*`).
//! The car step reads `remote`, writes `sent`, and takes the received changes.
use super::items::Out;
use super::message::{
    CarFloatsMessage, CarGearMessage, CarHandbrakeMessage, CarIgnitionMessage, CarLampsMessage,
    Message, PoseMessage,
};
use crate::car::VehicleRuntime;
use crate::reference::ReferenceManifest;
use crate::starting_area::GarageRuntime;
use crate::vehicle::VehiclePose;
use crate::world::ObjectKey;
use glam::{DQuat, DVec3, Quat, Vec3};

/// Source `carscript.CarSyncValue` values. Steer is the port steer. Positive steers left.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct CarFloats {
    pub steer: f32,
    pub horn: f32,
    pub gas: f32,
    pub brake: f32,
    pub clutch: f32,
    pub bike_gas: f32,
}

impl CarFloats {
    pub fn of(m: &CarFloatsMessage) -> Self {
        Self {
            steer: m.steer,
            horn: m.horn,
            gas: m.gas,
            brake: m.brake,
            clutch: m.clutch,
            bike_gas: m.bike_gas,
        }
    }

    /// Source `UpdMulti` merge: the largest steer and the largest pedals.
    pub fn merge(self, other: Self) -> Self {
        Self {
            steer: if other.steer.abs() > self.steer.abs() {
                other.steer
            } else {
                self.steer
            },
            horn: self.horn.max(other.horn),
            gas: self.gas.max(other.gas),
            brake: self.brake.max(other.brake),
            clutch: self.clutch.max(other.clutch),
            bike_gas: self.bike_gas.max(other.bike_gas),
        }
    }
}

/// Handbrake, headlights, gear and ignition that the peers know.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct CarShared {
    pub handbrake: bool,
    pub headlights: u8,
    pub gear: usize,
    pub ignition: bool,
    pub starter: bool,
}

/// The multiplayer state of one car copy. It is not saved.
#[derive(Clone, Debug, Default)]
pub struct CarNet {
    /// `ObjectKey::seed` of the car: its name on the wire.
    pub key: u64,
    /// Source `multiControlling` values: the merge of the peers that drive this car.
    pub remote: Option<CarFloats>,
    /// The controls of the local driver in the last step. `None`: the local player does not drive.
    pub sent: Option<CarFloats>,
    /// The state that the last message sent or received. `None` until the session reads it once.
    pub shared: Option<CarShared>,
    /// Source `UpdMultiGear`: a received gear for the next step.
    pub gear: Option<usize>,
    /// Source `UpdMultiIgnition`: a received engine rpm for the next step.
    pub rpm: Option<f32>,
}

impl CarNet {
    pub fn new(key: u64) -> Self {
        Self {
            key,
            ..Self::default()
        }
    }

    /// Ends the session. No peer drives the car.
    pub fn clear(&mut self) {
        *self = Self::new(self.key);
    }
}

/// Source `UpdMulti` receiver. A remote value replaces a smaller local one. Without a local
/// driver, the remote steer replaces the local steer.
pub fn with_remote(
    local: crate::simulation::input::DrivingControls,
    remote: CarFloats,
    driving: bool,
) -> crate::simulation::input::DrivingControls {
    crate::simulation::input::DrivingControls {
        steer: if remote.steer.abs() > local.steer.abs() || !driving {
            remote.steer
        } else {
            local.steer
        },
        throttle: local.throttle.max(remote.gas),
        brake: local.brake.max(remote.brake),
        clutch: local.clutch.max(remote.clutch),
        handbrake: local.handbrake,
    }
}

/// The state to compare with `CarNet::shared`. A received gear counts before the step takes it.
pub fn shared(vehicle: &VehicleRuntime) -> CarShared {
    CarShared {
        handbrake: vehicle.handbrake,
        headlights: vehicle.requested_headlights,
        gear: vehicle.net.gear.unwrap_or(vehicle.drivetrain.gear()),
        ignition: vehicle.ignition.ignition,
        starter: vehicle.ignition.starter,
    }
}

/// Source `carscript.UpdMulti`. Driver values older than 1 s drop.
const VALUES_SECONDS: f64 = 1.;

/// Source `carscript.CarSyncValues` entry of one peer and one car.
struct Values {
    slot: u8,
    car: u64,
    floats: CarFloats,
    time: f64,
}

/// Car messages of all peers: driver controls, handbrake, gear and ignition.
#[derive(Default)]
pub struct CarSync {
    values: Vec<Values>,
    /// The cars carry session state that must clear when the session ends.
    touched: bool,
}

impl CarSync {
    /// The session ended. No peer drives a car.
    pub fn end(&mut self, runtime: &mut GarageRuntime) {
        self.values.clear();
        if std::mem::take(&mut self.touched) {
            for vehicle in runtime.vehicles.values_mut() {
                vehicle.net.clear();
            }
        }
    }

    /// A peer left. Its driver values drop.
    pub fn leave(&mut self, slot: u8) {
        self.values.retain(|c| c.slot != slot);
    }

    /// A car message of peer `slot`. `now`: the session clock.
    pub fn receive(
        &mut self,
        reference: &ReferenceManifest,
        runtime: &mut GarageRuntime,
        now: f64,
        packet: &[u8],
    ) {
        if let Ok((slot, m)) = CarFloatsMessage::decode(packet) {
            // Source `UpdMultiValues`.
            let floats = CarFloats::of(&m);
            match self
                .values
                .iter_mut()
                .find(|c| c.slot == slot && c.car == m.car)
            {
                Some(c) => {
                    c.floats = floats;
                    c.time = now;
                }
                None => self.values.push(Values {
                    slot,
                    car: m.car,
                    floats,
                    time: now,
                }),
            }
        } else if let Ok((_, m)) = CarHandbrakeMessage::decode(packet) {
            got_handbrake(runtime, m.car, m.handbrake);
        } else if let Ok((_, m)) = CarLampsMessage::decode(packet) {
            got_headlights(runtime, m.car, m.headlights);
        } else if let Ok((_, m)) = CarGearMessage::decode(packet) {
            got_gear(reference, runtime, m.car, m.gear as usize);
        } else if let Ok((_, m)) = CarIgnitionMessage::decode(packet) {
            got_ignition(runtime, &m);
        }
    }

    /// Source `carscript.UpdMulti` and the `SCar*` senders on change. The receiver half drops old
    /// driver values and merges the rest into each car.
    pub(super) fn send(&mut self, runtime: &mut GarageRuntime, now: f64, out: &mut Out) {
        self.touched = true;
        self.values.retain(|c| now - c.time < VALUES_SECONDS);
        for vehicle in runtime.vehicles.values_mut() {
            vehicle.net.remote = self
                .values
                .iter()
                .filter(|c| c.car == vehicle.net.key)
                .map(|c| c.floats)
                .reduce(CarFloats::merge);
        }
        for (key, vehicle) in &runtime.vehicles {
            let net = &vehicle.net;
            if let Some(f) = net.sent {
                out.push(&CarFloatsMessage {
                    car: net.key,
                    steer: f.steer,
                    horn: f.horn,
                    gas: f.gas,
                    brake: f.brake,
                    clutch: f.clutch,
                    bike_gas: f.bike_gas,
                });
            }
            let Some(was) = net.shared else {
                continue;
            };
            let now = shared(vehicle);
            if now.handbrake != was.handbrake {
                out.push(&CarHandbrakeMessage {
                    car: net.key,
                    handbrake: now.handbrake as u8 as f32,
                    pad: 0,
                });
            }
            if now.headlights != was.headlights {
                out.push(&CarLampsMessage {
                    car: net.key,
                    headlights: now.headlights.into(),
                    pad: 0,
                });
            }
            if now.gear != was.gear {
                out.push(&CarGearMessage {
                    car: net.key,
                    gear: now.gear as u32,
                    pad: 0,
                });
            }
            // The source sends on a toggle and when the starter begins.
            if now.ignition != was.ignition || (now.starter && !was.starter) {
                let mut flags = 0;
                if now.ignition {
                    flags |= CarIgnitionMessage::IGNITION;
                }
                if now.starter {
                    flags |= CarIgnitionMessage::STARTING;
                }
                out.push(&CarIgnitionMessage {
                    car: net.key,
                    flags,
                    rpm: runtime.engine_rpm(key),
                });
            }
        }
        for vehicle in runtime.vehicles.values_mut() {
            vehicle.net.shared = Some(shared(vehicle));
        }
    }
}

fn find(runtime: &mut GarageRuntime, car: u64) -> Option<&mut VehicleRuntime> {
    runtime.vehicles.values_mut().find(|v| v.net.key == car)
}

/// Source `UpdMultiHandbrake`.
fn got_handbrake(runtime: &mut GarageRuntime, car: u64, handbrake: f32) {
    if let Some(v) = find(runtime, car) {
        v.handbrake = handbrake > 0.5;
        if let Some(shared) = &mut v.net.shared {
            shared.handbrake = v.handbrake;
        }
    }
}

/// Port addition: the headlight mode of source `lampak`, 0 to 2.
fn got_headlights(runtime: &mut GarageRuntime, car: u64, headlights: u32) {
    if let Some(v) = find(runtime, car)
        && headlights < 3
    {
        v.requested_headlights = headlights as u8;
        if let Some(shared) = &mut v.net.shared {
            shared.headlights = v.requested_headlights;
        }
    }
}

/// Source `UpdMultiGear`. The next step shifts, so the lever moves.
fn got_gear(reference: &ReferenceManifest, runtime: &mut GarageRuntime, car: u64, gear: usize) {
    if let Some(v) = find(runtime, car)
        && gear < v.record(reference).gear_ratios.len()
    {
        v.net.gear = Some(gear);
        if let Some(shared) = &mut v.net.shared {
            shared.gear = gear;
        }
    }
}

/// Source `UpdMultiIgnition`. The port does not pulse the starter. Source `mstarting` lasts one
/// frame, and the rpm carries the start.
fn got_ignition(runtime: &mut GarageRuntime, m: &CarIgnitionMessage) {
    if let Some(v) = find(runtime, m.car) {
        v.ignition.ignition = m.flags & CarIgnitionMessage::IGNITION != 0;
        v.net.rpm = Some(m.rpm);
        if let Some(shared) = &mut v.net.shared {
            shared.ignition = v.ignition.ignition;
        }
    }
}

/// The car pose that the renderer shows: the car root item between the last two steps.
fn shown_car(runtime: &GarageRuntime, key: &ObjectKey, alpha: f32) -> VehiclePose {
    let area = &runtime.area;
    if let ObjectKey::Generated(id) = key
        && let Some(item) = area.items.generated_item_id(id)
        && let Some((_, previous, current)) =
            area.item_render_poses.iter().find(|(i, ..)| *i == item)
    {
        let p = Vec3::from_array(previous.position).lerp(Vec3::from_array(current.position), alpha);
        let r =
            Quat::from_array(previous.rotation).slerp(Quat::from_array(current.rotation), alpha);
        return VehiclePose {
            position: p.to_array(),
            rotation: r.to_array(),
        };
    }
    let chassis = runtime.physics.chassis_handle(key);
    crate::vehicle::from_rapier(runtime.physics.rapier().bodies[chassis].position())
}

/// Source `snTempPlayerSync.UpdPos` sitting: the world pose of car `car` for seat `seat`.
pub fn seat_pose(
    reference: &ReferenceManifest,
    runtime: &GarageRuntime,
    alpha: f32,
    car: u64,
    seat: u32,
) -> Option<(DVec3, DQuat)> {
    let (key, vehicle) = runtime.vehicles.iter().find(|(_, v)| v.net.key == car)?;
    if seat as usize >= vehicle.record(reference).seats.len() {
        return None;
    }
    let pose = shown_car(runtime, key, alpha);
    let position = DVec3::from_array(runtime.stream.origin.absolute(pose.position));
    let rotation = Quat::from_array(pose.rotation).as_dquat().normalize();
    Some((position, rotation))
}

/// Source `SPlayerPosUpd` in a car seat: the pose relative to the car, the car and the seat.
/// `pose`: the player Transform in stream space.
pub fn seat_message(
    reference: &ReferenceManifest,
    runtime: &GarageRuntime,
    pose: VehiclePose,
    message: &mut PoseMessage,
) {
    let player = &runtime.area.player;
    let Some(key) = player.seat_vehicle() else {
        return;
    };
    let Some(vehicle) = runtime.vehicles.get(key) else {
        return;
    };
    let source = player.seat_source();
    let Some(seat) = vehicle
        .record(reference)
        .seats
        .iter()
        .position(|s| Some(&s.source) == source)
    else {
        return;
    };
    let chassis = runtime.physics.chassis_handle(key);
    let car = crate::vehicle::from_rapier(runtime.physics.rapier().bodies[chassis].position());
    let inverse = Quat::from_array(car.rotation).inverse();
    let local = inverse * (Vec3::from_array(pose.position) - Vec3::from_array(car.position));
    message.position = local.as_dvec3().to_array();
    message.rotation = (inverse * Quat::from_array(pose.rotation)).to_array();
    message.flags |= PoseMessage::IN_CAR;
    message.car = vehicle.net.key;
    message.seat = seat as u32;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn merge_keeps_the_largest_input() {
        let a = CarFloats {
            steer: -0.6,
            gas: 0.2,
            brake: 1.,
            ..CarFloats::default()
        };
        let b = CarFloats {
            steer: 0.3,
            gas: 0.9,
            horn: 1.,
            ..CarFloats::default()
        };
        let m = a.merge(b);
        assert_eq!(m.steer, -0.6);
        assert_eq!((m.gas, m.brake, m.horn), (0.9, 1., 1.));
    }
}
