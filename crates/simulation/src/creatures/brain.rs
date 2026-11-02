//! Source `aiscript.Update` and `ProcessStimul`: stimulus sum, flee, roam and steering.
use super::senses::{Sensed, angle};
use crate::reference::BrainReference;
use crate::weather::StormRandom;
use rapier3d::math::Vector;

/// One `Update` result for `movecreaturescript` and `nyulscript.SetAnims`.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Steer {
    /// `move.moveDir`, set only when the rabbit moves.
    pub move_dir: Option<Vector>,
    /// `move.moveRot` in Unity degrees: positive turns right.
    pub rot: Option<f32>,
    pub jump: bool,
    pub move_speed: f32,
    /// `nyul.SeeTarget(angle, distance)`: set when the flight sum is positive and `attack` is on.
    pub see_target: Option<(f32, f32)>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Brain {
    pub fleeing: bool,
    /// The last `Update` had a positive flight sum: the rabbit goes to a stimulus.
    pub targeting: bool,
    pub interest: Vector,
    pub sens_rot: f32,
    /// Simulation seconds.
    pub last_flee_signal: f64,
    pub last_roam: f64,
    pub next_roam: f32,
    pub temp: [Vector; 2],
    /// `goToTarget`, `avoidObstacles` and `enableInvert`. `nyulscript` changes them per rabbit.
    pub go_to_target: bool,
    pub avoid_obstacles: bool,
    pub enable_invert: bool,
}

/// What the brain reads of the rabbit this frame.
pub struct BrainInput<'a> {
    pub now: f64,
    pub position: Vector,
    /// `transform.forward` and `transform.right`.
    pub forward: Vector,
    pub right: Vector,
    pub sensed: &'a [Sensed],
    /// `overRideStimul`, processed after the sensed stimuli.
    pub threat: Option<Sensed>,
    pub ray_sum: f32,
    pub speed: f32,
    pub has_eyes: bool,
    /// `JumpRay`, cast only when the other jump rules pass.
    pub jump_ray: &'a dyn Fn() -> bool,
}

fn inverse_lerp(a: f32, b: f32, value: f32) -> f32 {
    if a == b {
        return 0.;
    }
    ((value - a) / (b - a)).clamp(0., 1.)
}

/// Unity `Vector3.SignedAngle(from, to, up)`: positive when `to` is right of `from`.
/// Converted right is `from x up`.
pub fn signed_angle(from: Vector, to: Vector) -> f32 {
    let a = angle(from, to);
    if to.dot(from.cross(Vector::Y)) < 0. {
        -a
    } else {
        a
    }
}

fn horizontal(v: Vector) -> Vector {
    Vector::new(v.x, 0., v.z)
}

impl Brain {
    /// Source `Start`: the interest is the spawn position.
    pub fn new(r: &BrainReference, position: Vector) -> Self {
        Self {
            fleeing: false,
            targeting: false,
            interest: position,
            sens_rot: 0.,
            last_flee_signal: 0.,
            last_roam: 0.,
            next_roam: 0.,
            temp: [Vector::ZERO; 2],
            go_to_target: r.go_to_target,
            avoid_obstacles: r.avoid_obstacles,
            enable_invert: r.enable_invert,
        }
    }

    pub fn rebase(&mut self, shift: Vector) {
        self.interest -= shift;
        for v in &mut self.temp {
            *v -= shift;
        }
    }

    pub(crate) fn update(
        &mut self,
        r: &BrainReference,
        i: &BrainInput,
        random: &mut StormRandom,
    ) -> Steer {
        // `stopFleeing` checks once per frame while fleeing.
        if self.fleeing && i.now - self.last_flee_signal > f64::from(r.flee_time) {
            self.fleeing = false;
            self.interest = i.position;
        }
        let mut flight = 0.;
        let (mut positive, mut negative) = (0., 0.);
        let (mut positive_at, mut negative_at) = (Vector::ZERO, Vector::ZERO);
        for s in i.sensed.iter().chain(&i.threat) {
            let Some(p) = r.preferences.iter().find(|p| p.kind == s.kind) else {
                continue;
            };
            let value = s.value * p.value;
            if value > positive {
                positive = value;
                positive_at = s.position;
            } else if value < negative {
                negative = value;
                negative_at = s.position;
            }
            flight += value;
        }
        if flight < 0. && flight > r.invert_flight_if_above && self.enable_invert {
            flight = -flight;
            positive_at = negative_at;
        }
        let position = i.position;
        self.targeting = flight > 0.;
        if flight < 0. && r.flee {
            self.fleeing = true;
            self.last_flee_signal = i.now;
            self.interest = if r.pre_calc_flee {
                self.temp[0] = self.temp[1];
                self.temp[1] = negative_at;
                let ahead = self.temp[1]
                    + (self.temp[1] - self.temp[0]).normalize_or_zero()
                        * r.target_pre_calc_distance;
                position
                    + ((position - self.temp[1] + (position - ahead)) * 0.5).normalize_or_zero()
                        * r.flee_distance
            } else {
                position + (position - negative_at).normalize_or_zero() * r.flee_distance
            };
        } else if flight > 0. {
            self.interest = positive_at;
        } else if r.roam && i.now - self.last_roam > f64::from(self.next_roam) {
            self.last_roam = i.now;
            self.next_roam = random.range(r.roam_time[0], r.roam_time[1]);
            let z = random.range(r.roam_z[0], r.roam_z[1]);
            let x = random.range(r.roam_x[0], r.roam_x[1]);
            self.interest = position + i.forward * z + i.right * x;
        }
        let mut steer = Steer::default();
        if !self.go_to_target {
            return steer;
        }
        let target = horizontal(self.interest - position);
        let distance = target.length();
        let signed = signed_angle(i.forward, target);
        let angle = signed.abs();
        if flight > 0. && r.attack {
            steer.see_target = Some((angle, distance));
        }
        if angle > r.target_angle_goal {
            let mut rot = signed;
            if self.avoid_obstacles {
                self.sens_rot = i.ray_sum;
                if self.sens_rot != 0. {
                    rot = self.sens_rot;
                }
            }
            steer.rot = Some(rot);
        }
        if distance > r.target_distance_goal {
            let mut speed = inverse_lerp(r.target_angle_move, r.target_angle_goal, angle)
                * (r.walk_speed_modifier
                    + (1. - r.walk_speed_modifier)
                        * inverse_lerp(r.walk_distance, r.run_distance, distance));
            let attack = r.attack && flight > 0.;
            let direction = target.normalize_or_zero();
            if self.sens_rot > 0. {
                if self.fleeing && speed < r.min_flee_speed {
                    speed = r.min_flee_speed;
                } else if attack && speed < r.attack_speed {
                    speed = r.attack_speed;
                } else if speed < r.walk_speed_modifier {
                    speed = r.walk_speed_modifier;
                }
                steer.move_dir = Some(
                    (i.forward * r.move_forward_modifier
                        + direction * r.move_towards_target_modifier)
                        .normalize_or_zero()
                        * speed,
                );
            } else {
                if self.fleeing && speed < r.min_flee_speed {
                    speed = r.min_flee_speed;
                }
                if attack && speed < r.attack_speed {
                    speed = r.attack_speed;
                }
                steer.move_dir = Some(direction * speed);
            }
            steer.move_speed = speed;
        }
        steer.jump = r.jump && i.has_eyes && i.speed < r.jump_if_speed && (i.jump_ray)();
        steer
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::reference::{SourceId, StimulusKind, StimulusValue};

    fn brain() -> BrainReference {
        BrainReference {
            source: SourceId {
                file: "sharedassets1.assets".into(),
                path_id: 131892,
            },
            acceleration_death: 25.,
            go_to_target: true,
            flee: true,
            pre_calc_flee: true,
            avoid_obstacles: true,
            roam: true,
            attack: true,
            jump: true,
            jump_if_speed: 1.,
            jump_sense_distance: 1.,
            jump_mask: 1,
            enable_invert: false,
            go_to_player_target: false,
            target_pre_calc_distance: 15.,
            target_distance_goal: 1.,
            target_angle_goal: 2.,
            target_angle_move: 50.,
            walk_distance: 5.,
            run_distance: 10.,
            attack_speed: 0.5,
            flee_distance: 100.,
            min_flee_speed: 0.5,
            flee_time: 10.,
            walk_speed_modifier: 0.3,
            move_towards_target_modifier: 1.,
            move_forward_modifier: 2.,
            roam_x: [-10., 10.],
            roam_z: [5., 10.],
            roam_time: [5., 10.],
            invert_flight_if_above: -11.,
            preferences: vec![StimulusValue {
                kind: StimulusKind::Threat,
                value: -1.,
            }],
        }
    }

    fn input<'a>(now: f64, sensed: &'a [Sensed], jump: &'a dyn Fn() -> bool) -> BrainInput<'a> {
        BrainInput {
            now,
            position: Vector::ZERO,
            forward: -Vector::Z,
            right: Vector::X,
            sensed,
            threat: None,
            ray_sum: 0.,
            speed: 5.,
            has_eyes: true,
            jump_ray: jump,
        }
    }

    #[test]
    fn a_threat_sends_the_rabbit_away() {
        let r = brain();
        let mut b = Brain::new(&r, Vector::ZERO);
        let threat = [Sensed {
            position: Vector::new(0., 0., -20.),
            kind: StimulusKind::Threat,
            value: 10.,
        }];
        let no = || false;
        let steer = b.update(&r, &input(1., &threat, &no), &mut StormRandom(1));
        assert!(b.fleeing);
        // Pre-calc flee from a still threat: half way between away and the look-ahead point.
        assert!(b.interest.z > 0. && (b.interest.length() - 100.).abs() < 1e-3);
        // The goal is behind: turn at the full angle, no move until the angle is in range.
        assert_eq!(steer.rot, Some(180.));
        assert_eq!(steer.move_speed, 0.5);
        // The flee ends after fleeTime without a signal.
        b.update(&r, &input(11.5, &[], &no), &mut StormRandom(1));
        assert!(!b.fleeing);
    }

    #[test]
    fn inverted_flight_pulls_toward_a_weak_threat() {
        let r = brain();
        let no = || false;
        let at = |value| {
            [Sensed {
                position: Vector::new(0., 0., -20.),
                kind: StimulusKind::Threat,
                value,
            }]
        };
        let mut b = Brain::new(&r, Vector::ZERO);
        b.enable_invert = true;
        let steer = b.update(&r, &input(1., &at(10.), &no), &mut StormRandom(1));
        assert!(!b.fleeing && b.interest == Vector::new(0., 0., -20.));
        assert!(
            steer
                .see_target
                .is_some_and(|(a, d)| a < 1e-3 && (d - 20.).abs() < 1e-4)
        );
        let mut b = Brain::new(&r, Vector::ZERO);
        b.enable_invert = true;
        b.update(&r, &input(1., &at(12.), &no), &mut StormRandom(1));
        assert!(b.fleeing);
    }

    #[test]
    fn speed_falls_with_angle_and_rises_with_distance() {
        let r = brain();
        let mut b = Brain::new(&r, Vector::ZERO);
        b.last_roam = 100.;
        b.next_roam = 100.;
        let no = || false;
        b.interest = Vector::new(0., 0., -20.);
        let ahead = b.update(&r, &input(1., &[], &no), &mut StormRandom(1));
        assert_eq!(ahead.move_speed, 1.);
        assert_eq!(ahead.rot, None);
        b.interest = Vector::new(3., 0., -3.);
        let side = b.update(&r, &input(1., &[], &no), &mut StormRandom(1));
        // 45 degrees right at 4.24 m: InverseLerp(50, 2, 45) * walkSpeedModifier.
        assert!((side.move_speed - (5. / 48.) * 0.3).abs() < 1e-4);
        assert!(side.rot.unwrap() > 44. && side.rot.unwrap() < 46.);
    }
}
