//! Source `aiscript.Update` and `ProcessStimul`: stimulus sum, flee, roam and steering.
use super::senses::{Sensed, angle};
use crate::reference::BrainReference;
use crate::simulation::dials::inverse_lerp;
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
