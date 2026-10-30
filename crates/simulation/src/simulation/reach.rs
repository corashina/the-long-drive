//! Source `carscript` hand and foot reach of the car controls. Callers run it only for the
//! car the player drives and send each move to the player body.
use crate::reference::VehicleReach;

/// Source `carscript.legDelay`: private, so always its default.
const LEG_DELAY: f32 = 0.2;

/// A reach target node.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReachNode {
    /// A car prototype node.
    Car(u32),
    /// `gearstickScript.TransmissionHandPos` of the car engine.
    GearTransmission,
    /// `gearstickScript.KickNitionHandPos` of the car engine.
    GearKick,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Limb {
    /// `MoveNearestHand`.
    Hand,
    /// `MoveNearestLeg`.
    Leg,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ReachMove {
    pub limb: Limb,
    pub node: ReachNode,
    /// None: `handSnapDelay`.
    pub delay: Option<f32>,
}

fn hand(node: ReachNode) -> ReachMove {
    ReachMove {
        limb: Limb::Hand,
        node,
        delay: None,
    }
}

fn limb(leg: bool, node: ReachNode, delay: Option<f32>) -> ReachMove {
    ReachMove {
        limb: if leg { Limb::Leg } else { Limb::Hand },
        node,
        delay,
    }
}

/// The car engine gear stick targets that exist.
#[derive(Clone, Copy, Debug, Default)]
pub struct GearStick {
    pub transmission: bool,
    pub kick: bool,
}

/// `IgnitionHandPos`, else the gear stick `KickNitionHandPos`.
fn ignition_node(reach: &VehicleReach, gear: GearStick) -> Option<ReachNode> {
    reach
        .ignition
        .map(ReachNode::Car)
        .or(gear.kick.then_some(ReachNode::GearKick))
}

/// Ignition switch state before `IgnitionState::step`.
#[derive(Clone, Copy, Debug)]
pub struct IgnitionReach {
    /// `iignition`.
    pub held: bool,
    /// `lastignition`.
    pub last_held: bool,
    /// `starting`.
    pub starting: bool,
    /// `startingValue`.
    pub hold: f32,
    /// `startingTime`.
    pub delay: f32,
}

/// carscript.Update ignition block.
pub fn ignition(
    reach: &VehicleReach,
    gear: GearStick,
    s: IgnitionReach,
    mut out: impl FnMut(ReachMove),
) {
    let current = ignition_node(reach, gear);
    let leg = reach.leg_ignition;
    if s.held {
        if (s.starting || s.hold > s.delay * 0.5) && gear.kick {
            out(limb(true, ReachNode::GearKick, None));
        }
        if let Some(node) = current
            && (!reach.dont_hold_ignition_when_start || !s.last_held)
        {
            out(limb(leg, node, None));
        }
    } else if s.last_held
        && s.hold < s.delay
        && reach.dont_hold_ignition_when_start
        && let Some(node) = current
    {
        out(limb(leg, node, None));
    }
}

/// carscript.Update: the horn sounds.
pub fn horn(reach: &VehicleReach) -> Option<ReachMove> {
    reach.horn.map(|n| hand(ReachNode::Car(n)))
}

/// carscript.InteractSwitchLights.
pub fn lights(reach: &VehicleReach) -> Option<ReachMove> {
    reach.lights.map(|n| hand(ReachNode::Car(n)))
}

/// carscript handbrake change.
pub fn handbrake(reach: &VehicleReach) -> Option<ReachMove> {
    reach
        .handbrake
        .map(|n| limb(reach.leg_handbrake, ReachNode::Car(n), None))
}

/// carscript.Shifted. `seconds` is `TransmissionLerp`.
pub fn shift(
    reach: &VehicleReach,
    gear: GearStick,
    automatic: bool,
    seconds: f32,
) -> Option<ReachMove> {
    reach
        .transmission
        .map(ReachNode::Car)
        .or(gear.transmission.then_some(ReachNode::GearTransmission))
        .filter(|_| !automatic || reach.leg_shift)
        .map(|node| limb(reach.leg_shift, node, Some(seconds)))
}

/// Pedal values of one step. They are zero when nobody drives.
#[derive(Clone, Copy, Debug, Default)]
pub struct Pedals {
    pub throttle: f32,
    pub brake: f32,
    pub clutch: f32,
}

/// carscript.Update pedal block: the poses of `LLDrivePos` and `RLDrivePos`.
#[derive(Clone, Copy, Debug, Default)]
pub struct PedalReach {
    clutch_leg: f32,
    brake_leg: f32,
    left_brake_leg: f32,
    gas_leg: f32,
    /// Left, right drive leg node. None: the authored drive frame.
    pub legs: [Option<u32>; 2],
}

impl PedalReach {
    /// `drive_legs`: `LLDrivePos`, `RLDrivePos` exist. `player`: `isPlayerDriving`.
    pub fn step(
        &mut self,
        reach: &VehicleReach,
        drive_legs: [bool; 2],
        p: Pedals,
        player: bool,
        dt: f32,
        mut out: impl FnMut(ReachMove),
    ) {
        let mut by_hand = |node: u32| {
            if player {
                out(hand(ReachNode::Car(node)));
            }
        };
        if drive_legs[0] {
            if let Some(n) = reach.default_left_leg {
                self.legs[0] = Some(n);
            } else if let Some(n) = reach.clutch.filter(|_| !reach.no_clutch) {
                self.legs[0] = Some(n);
            }
            if p.clutch > 0. {
                self.clutch_leg = LEG_DELAY;
            } else if p.throttle > 0. && p.brake > 0. {
                self.left_brake_leg = LEG_DELAY;
            }
            if self.left_brake_leg > 0.
                && let Some(n) = reach.brake
            {
                self.left_brake_leg -= dt;
                if reach.hand_brake {
                    by_hand(n);
                } else {
                    self.legs[0] = Some(n);
                }
            }
            if self.clutch_leg > 0.
                && let Some(n) = reach.clutch
            {
                self.clutch_leg -= dt;
                if !reach.no_clutch {
                    if reach.hand_clutch {
                        by_hand(n);
                    } else {
                        self.legs[0] = Some(n);
                    }
                }
            }
        }
        if drive_legs[1] {
            if let Some(n) = reach.default_right_leg.or(reach.gas) {
                self.legs[1] = Some(n);
            }
            if p.brake > 0. {
                self.brake_leg = LEG_DELAY;
            }
            if p.throttle > 0. {
                self.gas_leg = LEG_DELAY;
            }
            if self.brake_leg > 0. {
                self.brake_leg -= dt;
                if let Some(n) = reach.brake {
                    if reach.hand_brake {
                        by_hand(n);
                    } else {
                        self.legs[1] = Some(n);
                    }
                }
            }
            if self.gas_leg > 0. {
                self.gas_leg -= dt;
                if let Some(n) = reach.gas {
                    if reach.hand_gas {
                        by_hand(n);
                    } else {
                        self.legs[1] = Some(n);
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ignition_reach_uses_leg_when_leg_ignition() {
        let mut reach: VehicleReach =
            serde_json::from_value(crate::reference::reach_fixture()).unwrap();
        reach.ignition = Some(7);
        let held = IgnitionReach {
            held: true,
            last_held: false,
            starting: false,
            hold: 0.,
            delay: 1.,
        };
        let mut moves = Vec::new();
        ignition(&reach, GearStick::default(), held, |m| moves.push(m));
        assert_eq!(moves, [hand(ReachNode::Car(7))]);
        reach.leg_ignition = true;
        moves.clear();
        ignition(&reach, GearStick::default(), held, |m| moves.push(m));
        assert_eq!(moves, [limb(true, ReachNode::Car(7), None)]);
    }
}
