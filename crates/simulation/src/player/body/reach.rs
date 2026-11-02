//! Source `fpscontroller` reach: `MovedUsingHandObj`, `MoveNearestHand`, `MoveNearestLeg`,
//! `MoveNearestLegHand`. Callers set targets during the frame. `BodyState::step` resolves them.
use crate::reference::NodeId;
use crate::vehicle::VehiclePose;
use rapier3d::math::{Rotation, Vector};
use rapier3d::prelude::RigidBodyHandle;

/// The frame of a reach pose. A target in a moving frame follows it, as a source child Transform.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ReachFrame {
    /// Runtime world coordinates.
    World,
    /// Local to a rigid body.
    Body(RigidBodyHandle),
    /// Local to a mechanism node.
    Node(NodeId),
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ReachTarget {
    pub frame: ReachFrame,
    pub pose: VehiclePose,
}

/// The hand that takes a using pose. Source `usablescript` flags.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HandRule {
    /// No usable (`selectedU` null): the right hand when both hands are free.
    Free,
    /// The nearest hand.
    Nearest,
    /// `rightHand`.
    Right,
    /// `leftHand`.
    Left,
    /// `notNearestHand`.
    NotNearest,
}

impl HandRule {
    /// Source flag order: `rightHand`, `leftHand`, `notNearestHand`.
    pub fn from_flags(right: bool, left: bool, not_nearest: bool) -> Self {
        if right {
            Self::Right
        } else if left {
            Self::Left
        } else if not_nearest {
            Self::NotNearest
        } else {
            Self::Nearest
        }
    }
}

pub(super) const USING: usize = 0;
pub(super) const HAND: usize = 1;
pub(super) const LEG: usize = 2;

/// The call that last set a distance pair (`distLH`/`distRH` or `distLL`/`distRL`).
#[derive(Clone, Copy, Debug, PartialEq)]
enum DistFrom {
    Slot(usize),
    LegHand,
}

#[derive(Clone, Copy, Debug)]
struct Slot {
    target: ReachTarget,
    /// `v`, `VmovedNearestHand`, `VmovedNearestLeg`.
    timer: f32,
    /// Player-local pose of `target` in this step.
    local: (Vector, Rotation),
}

pub(super) struct Reach {
    /// `handSnapDelay`.
    snap_delay: f32,
    /// Using hand, nearest hand, nearest leg.
    slots: [Slot; 3],
    /// `selectedU` flags.
    pub(super) rule: HandRule,
    /// A `MoveNearestLegHand` call that waits for the limb choice.
    leg_hand: Option<ReachTarget>,
    /// A later call in the frame took the hand or leg slot after `leg_hand`.
    later: [bool; 2],
    hands_from: Option<DistFrom>,
    legs_from: Option<DistFrom>,
    /// `distLH`, `distRH`, `distLL`, `distRL`.
    pub(super) dist: [f32; 4],
}

impl Reach {
    pub(super) fn new(snap_delay: f32) -> Self {
        let slot = Slot {
            target: ReachTarget {
                frame: ReachFrame::World,
                pose: VehiclePose {
                    position: [0.; 3],
                    rotation: [0., 0., 0., 1.],
                },
            },
            timer: 0.,
            local: (Vector::ZERO, Rotation::IDENTITY),
        };
        Self {
            snap_delay,
            slots: [slot; 3],
            rule: HandRule::Free,
            leg_hand: None,
            later: [false; 2],
            hands_from: None,
            legs_from: None,
            dist: [0.; 4],
        }
    }

    fn set(&mut self, slot: usize, target: ReachTarget, delay: f32) {
        self.slots[slot].target = target;
        self.slots[slot].timer = delay;
    }

    pub(super) fn move_using_hand(&mut self, target: ReachTarget, rule: HandRule) {
        self.set(USING, target, self.snap_delay);
        self.rule = rule;
        self.hands_from = Some(DistFrom::Slot(USING));
    }

    /// `MovedUsingHandObj()`: a held usable keeps its target.
    pub(super) fn refresh_using_hand(&mut self) {
        self.slots[USING].timer = self.snap_delay;
    }

    pub(super) fn move_nearest_hand(&mut self, target: ReachTarget, delay: Option<f32>) {
        self.set(HAND, target, delay.unwrap_or(self.snap_delay));
        self.hands_from = Some(DistFrom::Slot(HAND));
        self.later[0] = true;
    }

    pub(super) fn move_nearest_leg(&mut self, target: ReachTarget, delay: Option<f32>) {
        self.set(LEG, target, delay.unwrap_or(self.snap_delay));
        self.legs_from = Some(DistFrom::Slot(LEG));
        self.later[1] = true;
    }

    pub(super) fn move_nearest_leg_hand(&mut self, target: ReachTarget) {
        self.leg_hand = Some(target);
        self.later = [false; 2];
        self.hands_from = Some(DistFrom::LegHand);
        self.legs_from = Some(DistFrom::LegHand);
    }

    /// Resolves targets to player-local poses and takes the call-time distances.
    /// `limbs`: `LHTarget`, `RHTarget`, `LLTarget`, `RLTarget` positions. They change only after this.
    pub(super) fn resolve(
        &mut self,
        limbs: [Vector; 4],
        mut resolve: impl FnMut(ReachTarget) -> Option<VehiclePose>,
    ) {
        let dist = |p: Vector, i: usize| (limbs[i] - p).length_squared();
        let mut point = None;
        if let Some(target) = self.leg_hand.take()
            && let Some(p) = resolve(target)
        {
            let local = (
                Vector::from_array(p.position),
                Rotation::from_array(p.rotation),
            );
            let d = [0, 1, 2, 3].map(|i| dist(local.0, i));
            let hand = (d[0] < d[2] && d[0] < d[3]) || (d[1] < d[2] && d[1] < d[3]);
            let slot = if hand { HAND } else { LEG };
            if !self.later[slot - HAND] {
                self.set(slot, target, self.snap_delay);
            }
            point = Some(local.0);
        }
        for slot in &mut self.slots {
            if slot.timer <= 0. {
                continue;
            }
            match resolve(slot.target) {
                Some(p) => {
                    slot.local = (
                        Vector::from_array(p.position),
                        Rotation::from_array(p.rotation),
                    )
                }
                // The frame is gone: the source Transform would be destroyed.
                None => slot.timer = 0.,
            }
        }
        let from = |from: Option<DistFrom>, slots: &[Slot; 3]| match from? {
            DistFrom::Slot(i) => (slots[i].timer > 0.).then_some(slots[i].local.0),
            DistFrom::LegHand => point,
        };
        if let Some(p) = from(self.hands_from.take(), &self.slots) {
            self.dist[0] = dist(p, 0);
            self.dist[1] = dist(p, 1);
        }
        if let Some(p) = from(self.legs_from.take(), &self.slots) {
            self.dist[2] = dist(p, 2);
            self.dist[3] = dist(p, 3);
        }
        self.later = [false; 2];
    }

    /// Counts the slot timer down. Some while it was running, as source `if (v > 0) { v -= dt; ... }`.
    pub(super) fn tick(&mut self, slot: usize, dt: f32) -> Option<(Vector, Rotation)> {
        let s = &mut self.slots[slot];
        (s.timer > 0.).then(|| {
            s.timer -= dt;
            s.local
        })
    }

    pub(super) fn target(&self, slot: usize) -> ReachTarget {
        self.slots[slot].target
    }

    /// UpdHandPos using hand: the hand that takes the pose (0 left, 1 right),
    /// and the hand whose `VCurrent*T` becomes the using hand. Source quirks kept.
    pub(super) fn using_hands(&self, hands_free: bool) -> (usize, usize) {
        let near_left = self.dist[0] < self.dist[1];
        match self.rule {
            HandRule::Free | HandRule::Right if hands_free => (1, 1),
            HandRule::Free | HandRule::Right | HandRule::Left => (0, 0),
            HandRule::NotNearest if near_left && hands_free => (1, 0),
            HandRule::NotNearest => (0, 0),
            HandRule::Nearest if near_left => (0, 0),
            HandRule::Nearest if hands_free => (1, 1),
            HandRule::Nearest => (0, 1),
        }
    }
}
