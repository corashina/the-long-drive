//! Car control reach: source `carscript` moves sent to the player body.
use super::VehicleRuntime;
use crate::interaction::items::ItemStore;
use crate::player::body::{BodyState, ReachFrame, ReachTarget};
use crate::reference::{ReferenceManifest, WorldPrototypeReference};
use crate::simulation::powertrain::PowertrainParts;
use crate::simulation::reach::{GearStick, Limb, ReachMove, ReachNode};
use crate::vehicle::{PhysicsWorld, from_rapier, to_rapier};
use crate::world::ObjectKey;
use rapier3d::prelude::RigidBodyHandle;

/// Reach targets of one car.
#[derive(Clone, Copy)]
pub struct CarFrames<'a> {
    chassis: RigidBodyHandle,
    prototype: &'a WorldPrototypeReference,
    /// Gear stick transmission and kick targets on the engine part body.
    gear: [Option<ReachTarget>; 2],
}

impl<'a> CarFrames<'a> {
    pub fn new(
        reference: &'a ReferenceManifest,
        key: &ObjectKey,
        vehicle: &VehicleRuntime,
        powertrain: &PowertrainParts,
        items: &ItemStore,
        physics: &PhysicsWorld,
    ) -> Self {
        let engine = vehicle
            .powertrain
            .engine
            .and_then(|item| powertrain.engine(item));
        let part = engine.and_then(|e| items.get(e.item));
        let gear = std::array::from_fn(|i| {
            let frame = engine?.gear_stick[i]?;
            let part = part?;
            Some(ReachTarget {
                frame: ReachFrame::Body(part.body?),
                pose: from_rapier(&(to_rapier(part.local_pose) * to_rapier(frame))),
            })
        });
        Self {
            chassis: physics.chassis_handle(key),
            prototype: &reference.world.prototypes[vehicle.prototype],
            gear,
        }
    }
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
