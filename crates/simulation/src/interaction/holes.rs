//! `weaponscript.Shot` hithole: leak openings of shot tanks. Holes are not saved.
use super::items::{ItemLocation, ItemStore};
use crate::reference::{ItemId, ReferenceManifest};
use crate::vehicle::to_rapier;
use rapier3d::math::{Pose, Vector};

/// Holes per tank. A new hole replaces the oldest (deviation for the frame budget).
pub const HOLES_PER_TANK: usize = 16;

/// The shot overrides of the `tankcaphole` prefab: radius 1.5, valve 1, defPourAngle 5.
const RADIUS: f32 = 1.5;
const POUR_ANGLE: f32 = 5.;

#[derive(Clone, Copy, Debug, PartialEq)]
struct Hole {
    tank: ItemId,
    /// In tank item space. Local -Z is the hole forward (the hit normal).
    pose: Pose,
}

pub struct Holes {
    holes: Vec<Hole>,
    /// Liters per second at valve 1.
    flow: f32,
    angle_disabled: bool,
    spill_disabled: bool,
}

impl Holes {
    pub fn new(r: &ReferenceManifest) -> Self {
        let hole = &r.explosions.hole;
        Self {
            holes: Vec::with_capacity(HOLES_PER_TANK),
            flow: RADIUS * RADIUS * std::f32::consts::PI * 0.025 * r.fluid_rules.pour_speed,
            angle_disabled: hole.no_angle,
            spill_disabled: hole.no_pour,
        }
    }
    /// A hole at `point + normal * 0.01`, facing `normal`, in the space of `tank_pose`.
    pub fn add(&mut self, tank: ItemId, tank_pose: Pose, point: Vector, normal: Vector) {
        let world = Pose::from_parts(point + normal * 0.01, super::melee::look_rotation(normal));
        let hole = Hole {
            tank,
            pose: tank_pose.inverse() * world,
        };
        if self.count(tank) >= HOLES_PER_TANK {
            let oldest = self.holes.iter().position(|h| h.tank == tank).unwrap();
            self.holes.remove(oldest);
        }
        self.holes.push(hole);
    }
    pub fn count(&self, tank: ItemId) -> usize {
        self.holes.iter().filter(|h| h.tank == tank).count()
    }
    pub(super) fn forget(&mut self, tank: ItemId) {
        self.holes.retain(|h| h.tank != tank);
    }
    /// `tankcapscript.Update` of each hole: spill by the tilt of the hole. Holes of a removed tank
    /// are dropped.
    pub(super) fn step(&mut self, items: &mut ItemStore, dt: f32, fluid_scale: f32) {
        self.holes.retain(|h| live(items, h.tank));
        for hole in &self.holes {
            let Some(state) = items.get_mut(hole.tank) else {
                continue;
            };
            if state.inactive_pose.is_some() {
                continue;
            }
            let forward = (to_rapier(state.pose) * hole.pose).rotation * -Vector::Z;
            let angle = forward.dot(Vector::Y).clamp(-1., 1.).acos().to_degrees();
            let Some(fluid) = state.fluid.as_mut() else {
                continue;
            };
            if self.spill_disabled || fluid.total() <= 0. {
                continue;
            }
            let fill = (fluid.total() / fluid.capacity_liters).clamp(0., 1.);
            if let Some(factor) = super::pour_factor(POUR_ANGLE, self.angle_disabled, fill, angle) {
                crate::fluids::spill(fluid, factor * self.flow * dt * fluid_scale);
            }
        }
    }
}

/// The tank and the item it is attached to are not removed.
fn live(items: &ItemStore, tank: ItemId) -> bool {
    let Some(state) = items.get(tank) else {
        return false;
    };
    let owner = match state.location {
        ItemLocation::Attached(owner) => items.get(owner),
        _ => Some(state),
    };
    !state.removed && owner.is_some_and(|o| !o.removed)
}
