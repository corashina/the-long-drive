//! Car control reach: source `carscript` moves sent to the player body.
use super::{GarageRuntime, VehicleRuntime};
use crate::player::body::{BodyState, ReachFrame, ReachTarget};
use crate::reference::{ReferenceManifest, WorldPrototypeReference};
use crate::simulation::reach::{GearStick, Limb, ReachMove, ReachNode};
use crate::vehicle::{from_rapier, to_rapier};
use crate::world::ObjectKey;
use rapier3d::prelude::RigidBodyHandle;

/// Reach targets of one car.
#[derive(Clone, Copy)]
pub(super) struct CarFrames<'a> {
    chassis: RigidBodyHandle,
    prototype: &'a WorldPrototypeReference,
    /// Gear stick transmission and kick targets on the engine part body.
    gear: [Option<ReachTarget>; 2],
}

impl CarFrames<'_> {
    pub fn gear_stick(&self) -> GearStick {
        GearStick {
            transmission: self.gear[0].is_some(),
            kick: self.gear[1].is_some(),
        }
    }
    fn target(&self, node: ReachNode) -> Option<ReachTarget> {
        match node {
            // The chassis body frame is the car root node: reference validation requires it.
            ReachNode::Car(n) => Some(ReachTarget {
                frame: ReachFrame::Body(self.chassis),
                pose: crate::world::node_frame(self.prototype, 0, n as usize),
            }),
            ReachNode::GearTransmission => self.gear[0],
            ReachNode::GearKick => self.gear[1],
        }
    }
    /// `MoveNearestHand` or `MoveNearestLeg`.
    pub fn send(&self, body: &mut BodyState, m: ReachMove) {
        let Some(target) = self.target(m.node) else {
            return;
        };
        match m.limb {
            Limb::Hand => body.move_nearest_hand(target, m.delay),
            Limb::Leg => body.move_nearest_leg(target, m.delay),
        }
    }
}

impl GarageRuntime {
    pub(super) fn car_frames<'a>(
        &self,
        reference: &'a ReferenceManifest,
        key: &ObjectKey,
        vehicle: &VehicleRuntime,
    ) -> CarFrames<'a> {
        let engine = vehicle
            .powertrain
            .engine
            .and_then(|item| self.powertrain.engine(item));
        let part = engine.and_then(|e| self.area.items.get(e.item));
        let gear = std::array::from_fn(|i| {
            let frame = engine?.gear_stick[i]?;
            let part = part?;
            Some(ReachTarget {
                frame: ReachFrame::Body(part.body?),
                pose: from_rapier(&(to_rapier(part.local_pose) * to_rapier(frame))),
            })
        });
        CarFrames {
            chassis: self.physics.chassis_handle(key),
            prototype: &reference.world.prototypes[vehicle.prototype],
            gear,
        }
    }

    /// Sends the move of a car control when the player sits in car `key`.
    pub(super) fn reach_seated_car(
        &mut self,
        reference: &ReferenceManifest,
        key: &ObjectKey,
        control: impl FnOnce(&crate::reference::VehicleReach) -> Option<ReachMove>,
    ) {
        if self.area.player.seat_vehicle() != Some(key) {
            return;
        }
        let Some(vehicle) = self.vehicles.get(key) else {
            return;
        };
        let frames = self.car_frames(reference, key, vehicle);
        if let Some(m) = control(&vehicle.record(reference).reach) {
            frames.send(&mut self.area.body, m);
        }
    }
}
