//! Source `newAiScript.Update`, `AvoidWalls`, `Use`, `GetNewPatrolPos` and `NewPatrolTime`:
//! sight, chase, patrol, move, wall avoidance, jump, duck, ladders and doors.
//! Positions and directions use the converted space: forward is -Z.
use super::brain::signed_angle;
use super::senses::angle;
use crate::reference::{HumanAiReference, ItemId};
use crate::simulation::dials::{inverse_lerp, lerp};
use crate::weather::StormRandom;
use rapier3d::math::{Pose, Rotation, Vector};

/// Source `sens.v`: a `sens` stays sensed for this many fixed steps after its last touch.
pub const SENS_HOLD: u8 = 3;
/// `ladderscript.OnTriggerStay` sets `ladderV` to this.
pub const LADDER_HOLD: u8 = 5;

/// A `usablescript` that the AI can push.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Usable {
    pub item: ItemId,
    pub position: Vector,
    pub rotate: bool,
    pub slide: bool,
}

/// A `fpscontroller`: its transform and head (`Th`) positions.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PlayerView {
    pub position: Vector,
    pub head: Vector,
}

/// What a `sens` touched last: source `T` and its `GetComponentInParent` results.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Touch {
    Player(PlayerView),
    Usable(Usable),
    Other,
}

/// Source `sens`: `OnTriggerStay` sets `T` and holds `sensed` for `SENS_HOLD` fixed steps.
#[derive(Clone, Copy, Debug, Default)]
pub struct Sens {
    pub touch: Option<Touch>,
    hold: u8,
}

impl Sens {
    pub fn sensed(&self) -> bool {
        self.touch.is_some()
    }

    /// `OnTriggerStay`.
    pub fn stay(&mut self, touch: Touch) {
        self.touch = Some(touch);
        self.hold = SENS_HOLD;
    }

    /// `FixedUpdate`. Call it before the touches of this step.
    pub fn fixed_update(&mut self) {
        if self.hold > 0 {
            self.hold -= 1;
        } else {
            self.touch = None;
        }
    }

    fn player(&self) -> Option<PlayerView> {
        match self.touch {
            Some(Touch::Player(p)) => Some(p),
            _ => None,
        }
    }

    fn usable(&self) -> Option<Usable> {
        match self.touch {
            Some(Touch::Usable(u)) => Some(u),
            _ => None,
        }
    }
}

/// `sensU`, `sensH`, `sensD`, `sensG` and `sensCanGetUp`.
#[derive(Clone, Copy, Debug, Default)]
pub struct Sensors {
    pub up: Sens,
    pub head: Sens,
    pub down: Sens,
    pub ground: Sens,
    pub can_get_up: Sens,
}

impl Sensors {
    pub fn fixed_update(&mut self) {
        for s in [
            &mut self.up,
            &mut self.head,
            &mut self.down,
            &mut self.ground,
            &mut self.can_get_up,
        ] {
            s.fixed_update();
        }
    }
}

/// One hit of a `lm` ray.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Hit {
    pub distance: f32,
    pub point: Vector,
    pub normal: Vector,
    /// The hit collider's `fpscontroller`.
    pub player: Option<PlayerView>,
    /// A `ragdollColliders` or `animColliders` entry of this AI.
    pub own: bool,
    /// Layer 23. Sight passes it.
    pub glass: bool,
    /// The hit collider's `usablescript`.
    pub usable: Option<Usable>,
}

/// The rays of `newAiScript` with its `lm` mask. Directions need not be unit length.
pub trait World {
    /// `Physics.RaycastAll`, sorted by distance.
    fn ray_all(&mut self, origin: Vector, direction: Vector, range: f32, hits: &mut Vec<Hit>);
    /// `Physics.Raycast`: the nearest hit.
    fn ray(&mut self, origin: Vector, direction: Vector, range: f32) -> Option<Hit>;
}

/// The `newAiScript` fields that X03e owns: saved or set in `Start`.
pub struct Owned<'a> {
    pub sleeping: &'a mut bool,
    pub first: &'a mut bool,
    /// Source `target.position`.
    pub target: &'a mut Vector,
    /// Source `patrolCurrentTime`.
    pub patrol_time: &'a mut f32,
    /// `startPatrolTimeMin` and `startPatrolTimeMax`.
    pub patrol_range: [f32; 2],
}

/// What `Update` reads this frame.
pub struct Frame {
    pub now: f64,
    pub dt: f32,
    /// Root pose and Rigidbody velocity.
    pub pose: Pose,
    pub velocity: Vector,
    /// `head` pose. X03g animates it; until then the body pose with a fixed offset.
    pub head: Pose,
    /// `mainscript.M.player`.
    pub player: Option<PlayerView>,
    /// `GSettings.peacefulEnemies == 2`.
    pub peaceful: bool,
    /// `rizseshusi` with `player.currentTarget`: the target follows it.
    pub debug_target: Option<Vector>,
}

/// What `Update` writes this frame.
#[derive(Clone, Copy, Debug)]
pub struct Output {
    pub velocity: Vector,
    pub rotation: Rotation,
    /// `rb.useGravity`.
    pub gravity: bool,
    /// `Use`: push this usable by `doorSpeed * dt` on each axis.
    pub door: Option<Usable>,
    /// `Attack`: `survival.DamageInstant(damage * dt, show: true)` on the player.
    pub damage: f32,
}

/// `newAiAnim` inputs. `forward` and `turn` keep their value while the AI stands.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct AnimInput {
    pub forward: f32,
    pub turn: f32,
    pub on_ground: bool,
    pub duck: bool,
    /// `anim.Hold` points of the left (0) and right (1) hand. `Use` holds the left hand, `Attack` both.
    pub hold: [Option<Vector>; 2],
}

#[derive(Debug, Default)]
pub struct HumanAi {
    pub sensors: Sensors,
    pub is_chasing: bool,
    pub is_seeing: bool,
    pub is_attacking: bool,
    /// True on the frame of source `SoundNotice`.
    pub noticed: bool,
    pub stuck: bool,
    pub jump: bool,
    pub duck: bool,
    pub ladder: bool,
    /// Source `ladderV`.
    pub ladder_v: u8,
    pub head_sensed: bool,
    /// `col` lerps from `colStand` to `colDuck` by this.
    pub duck_value: f32,
    pub anim: AnimInput,
    chase_start: f64,
    chase_time: f32,
    duck_debounce: f32,
    last_jumped: f64,
    head_sensed_time: f64,
    reset_on_ground: bool,
    avoid_dir: Vector,
    current_player: Option<PlayerView>,
    /// `Attack` health damage this frame.
    damage: f32,
    attack_hits: Vec<Hit>,
    patrol_charge: f32,
    last_patrol_pos: Vector,
    hits: Vec<Hit>,
    points: Vec<Vector>,
}

fn flat(v: Vector) -> Vector {
    Vector::new(v.x, 0., v.z)
}

/// Unity `normalized`: zero below 1e-5.
fn normalized(v: Vector) -> Vector {
    let length = v.length();
    if length > 1e-5 {
        v / length
    } else {
        Vector::ZERO
    }
}

fn forward(rotation: Rotation) -> Vector {
    rotation * -Vector::Z
}

/// Unity `Quaternion.Euler(0, degrees, 0)`.
fn yaw(degrees: f32) -> Rotation {
    Rotation::from_rotation_y(-degrees.to_radians())
}

/// Unity `Quaternion.LookRotation(direction, Vector3.up)` of a horizontal direction.
fn look(direction: Vector) -> Rotation {
    Rotation::from_rotation_y((-direction.x).atan2(-direction.z))
}

impl HumanAi {
    pub fn new() -> Self {
        Self::default()
    }

    /// Source `TargetReset`.
    pub fn target_reset(pose: Pose) -> Vector {
        pose.translation + pose.rotation * Vector::Y * 0.75 + forward(pose.rotation) * 0.05
    }

    /// Source `NewPatrolTime`.
    fn new_patrol_time(
        &mut self,
        range: [f32; 2],
        random: &mut StormRandom,
        patrol_time: &mut f32,
    ) {
        self.patrol_charge = 0.;
        *patrol_time = random.range(range[0], range[1]);
    }

    /// `ladderscript.OnTriggerStay` on this AI.
    pub fn on_ladder(&mut self) {
        self.ladder_v = LADDER_HOLD;
    }

    pub fn rebase(&mut self, shift: Vector) {
        self.last_patrol_pos -= shift;
    }

    /// Source `Update` after the voice code. The caller skips dead AIs.
    pub(crate) fn update(
        &mut self,
        r: &HumanAiReference,
        owned: Owned,
        frame: &Frame,
        world: &mut dyn World,
        random: &mut StormRandom,
    ) -> Output {
        let Owned {
            sleeping,
            first,
            target,
            patrol_time,
            patrol_range,
        } = owned;
        if *sleeping && *first {
            *sleeping = false;
        }
        let now = frame.now;
        let dt = frame.dt;
        let position = frame.pose.translation;
        let rotation = frame.pose.rotation;
        let fwd = forward(rotation);
        self.noticed = false;
        self.anim.hold = [None; 2];
        self.damage = 0.;
        self.ladder = self.ladder_v > 0;
        self.avoid_dir = Vector::ZERO;
        if self.ladder {
            self.ladder_v -= 1;
        }
        // Source checks the chase end inside the voice code, which runs only awake.
        if !*sleeping && self.is_chasing && self.chase_start + (self.chase_time as f64) < now {
            self.is_chasing = false;
        }
        self.is_seeing = false;
        if !*first && !self.reset_on_ground && self.sensors.ground.sensed() {
            self.reset_on_ground = true;
            *target = Self::target_reset(frame.pose);
        }
        if let Some(debug) = frame.debug_target {
            *target = debug;
        } else if !frame.peaceful
            && let Some(player) = frame.player
        {
            let to_player = player.position - position;
            if angle(forward(frame.head.rotation), to_player) < r.fov {
                let mut hits = std::mem::take(&mut self.hits);
                hits.clear();
                world.ray_all(frame.head.translation, to_player, r.range, &mut hits);
                for hit in &hits {
                    if let Some(seen) = hit.player {
                        *target = seen.head;
                        *first = true;
                        if !self.is_chasing {
                            self.chase_time = random.range(r.chase_time[0], r.chase_time[1]);
                            self.noticed = true;
                        }
                        self.is_seeing = true;
                        self.chase_start = now;
                        self.is_chasing = true;
                    }
                    if !hit.glass {
                        break;
                    }
                }
                self.hits = hits;
            }
        }
        let mut usable = None;
        self.current_player = None;
        let rb_speed = frame.velocity.length();
        self.stuck = false;
        self.jump = false;
        self.is_attacking = false;
        let mut vertical = frame.velocity.y;
        let to_target = *target - frame.head.translation;
        let target_dir = normalized(to_target);
        let mut move_dir = if self.ladder {
            target_dir
        } else {
            normalized(flat(target_dir))
        };
        let horizontal = flat(frame.velocity);
        let mut speed = lerp(
            r.min_speed,
            r.max_speed,
            inverse_lerp(r.walk_dist, r.run_dist, to_target.length()),
        );
        if to_target.length() < r.target_dist * 0.5 || *sleeping {
            speed = 0.;
        }
        if !self.is_chasing {
            speed = speed.clamp(0., r.min_speed * 0.7);
        }
        if speed > 0. {
            self.anim.forward =
                inverse_lerp(0., r.max_velocity, rb_speed) * fwd.dot(normalized(frame.velocity));
            self.anim.turn = lerp(
                -1.,
                1.,
                inverse_lerp(-45., 45., signed_angle(fwd, normalized(move_dir))),
            );
        }
        self.anim.on_ground = self.sensors.ground.sensed();
        let hdist = flat(to_target).length();
        if (hdist > r.target_dist || to_target.y > r.target_dist_v) && rb_speed < speed {
            if self.duck_debounce >= 1. {
                self.duck_debounce = 1.;
                self.stuck = true;
            } else {
                self.duck_debounce += dt * r.duck_debounce_speed_up;
            }
        } else if self.duck_debounce > 0. {
            self.duck_debounce -= dt * r.duck_debounce_speed_down;
        } else {
            self.duck_debounce = 0.;
        }
        self.current_player = self
            .sensors
            .up
            .player()
            .or_else(|| self.sensors.down.player());
        if !frame.peaceful
            && let Some(player) = self.current_player
        {
            self.attack(r, frame, world, target, player);
        }
        if self.stuck {
            let ray_usable = self.avoid_walls(r, frame, target, world);
            let current = self.sensors.up.usable().filter(|u| u.rotate || u.slide);
            let current = current.or_else(|| self.sensors.down.usable());
            usable = current.or(ray_usable);
            if let Some(u) = usable {
                // `Use`: the caller pushes the door. The hand holds the midpoint of the usable
                // and one meter ahead.
                self.anim.hold[0] = Some((u.position + position + fwd) * 0.5);
            }
            if current.is_none() && self.current_player.is_none() {
                let (down, up) = (self.sensors.down.sensed(), self.sensors.up.sensed());
                if down && !up {
                    self.jump = true;
                }
                if !down && up {
                    self.duck = true;
                }
                if self.duck {
                    self.jump = true;
                }
            }
        }
        if self.duck
            && !self.sensors.can_get_up.sensed()
            && (self.sensors.down.sensed() || !self.sensors.up.sensed())
        {
            self.duck = false;
        }
        if self.sensors.head.sensed() {
            self.head_sensed_time = now;
        }
        self.head_sensed = now - self.head_sensed_time < 0.5;
        if (self.stuck || !self.sensors.ground.sensed()) && !self.is_attacking && self.head_sensed {
            self.duck = true;
            self.jump = true;
        }
        if self.sensors.ground.sensed() && self.jump && now - self.last_jumped > 0.5 {
            vertical += if self.duck {
                r.jump_velocity_duck
            } else {
                r.jump_velocity
            };
            self.last_jumped = now;
        }
        let duck_goal = if self.duck { 1. } else { 0. };
        self.duck_value += (duck_goal - self.duck_value).clamp(-dt * 5., dt * 5.);
        self.anim.duck = self.duck;
        let t = (dt * r.velocity_change).clamp(0., 1.);
        let mut velocity = if self.ladder {
            let v = horizontal.lerp(flat(move_dir) * r.max_velocity * speed, t);
            let limit = move_dir.length() * r.max_velocity * speed;
            vertical = (vertical + move_dir.y * r.max_velocity * speed).clamp(-limit, limit);
            v
        } else {
            if self.avoid_dir != Vector::ZERO {
                move_dir += flat(self.avoid_dir);
            }
            if self.sensors.ground.sensed() && vertical < r.grounded_vertical_velocity {
                vertical = r.grounded_vertical_velocity;
            }
            horizontal.lerp(move_dir * r.max_velocity * speed, t)
        };
        let mut rotation = rotation;
        let facing = normalized(flat(move_dir));
        if move_dir != Vector::ZERO && speed > 0. && facing != Vector::ZERO {
            let rate = r.rot_speed * dt * if self.is_chasing { 1. } else { 0.4 };
            rotation = rotation.lerp(look(facing), rate.clamp(0., 1.)).normalize();
        }
        velocity.y = vertical;
        let output = Output {
            velocity,
            rotation,
            gravity: !self.ladder,
            door: usable,
            damage: self.damage,
        };
        if self.is_attacking || *sleeping {
            return output;
        }
        if (self.last_patrol_pos - position).length() < r.start_patrol_dist {
            self.patrol_charge += dt;
            if self.patrol_charge > *patrol_time {
                self.new_patrol_time(patrol_range, random, patrol_time);
                *target = self.new_patrol_pos(r, frame, world, random);
            }
        } else {
            self.patrol_charge = 0.;
            self.last_patrol_pos = position;
        }
        output
    }

    /// Source `Attack`: the first hit that is not this AI must be the player.
    fn attack(
        &mut self,
        r: &HumanAiReference,
        frame: &Frame,
        world: &mut dyn World,
        target: &mut Vector,
        player: PlayerView,
    ) {
        let position = frame.pose.translation;
        let mut hits = std::mem::take(&mut self.attack_hits);
        hits.clear();
        world.ray_all(
            position,
            player.position - position,
            r.ray_attack_dist_range * 2.,
            &mut hits,
        );
        let can_attack = hits
            .iter()
            .find(|hit| !hit.own)
            .is_some_and(|hit| hit.player.is_some());
        self.attack_hits = hits;
        *target = player.head;
        if can_attack {
            self.is_attacking = true;
            self.damage += r.damage * frame.dt;
            let hold = (player.head + position + forward(frame.pose.rotation)) * 0.5;
            self.anim.hold = [Some(hold); 2];
        }
    }

    /// Source `AvoidWalls`: sets `avoid_dir` and returns `rayUsable`.
    fn avoid_walls(
        &mut self,
        r: &HumanAiReference,
        frame: &Frame,
        target: &mut Vector,
        world: &mut dyn World,
    ) -> Option<Usable> {
        let mut ray_usable: Option<Usable> = None;
        let mut count = 0;
        let position = frame.pose.translation;
        let fwd = forward(frame.pose.rotation);
        let half = Vector::Y * r.avoid_ray_vertical * 0.5;
        let mut hits = std::mem::take(&mut self.hits);
        for i in 0..r.avoid_ray_y {
            let origin = (position + half).lerp(
                position - half,
                inverse_lerp(0., r.avoid_ray_y as f32 - 1., i as f32),
            );
            for j in 0..r.avoid_ray_x {
                let degrees = lerp(
                    -r.avoid_ray_angle * 0.5,
                    r.avoid_ray_angle * 0.5,
                    inverse_lerp(0., r.avoid_ray_x as f32 - 1., j as f32),
                );
                let direction = yaw(degrees) * fwd;
                hits.clear();
                world.ray_all(origin, direction, r.avoid_ray_range, &mut hits);
                for hit in &hits {
                    if hit.player.is_some() || hit.own {
                        if !frame.peaceful
                            && self.current_player.is_none()
                            && hit.distance < r.ray_attack_dist_range
                            && let Some(player) = hit.player
                        {
                            self.current_player = Some(player);
                            self.attack(r, frame, world, target, player);
                        }
                        continue;
                    }
                    if ray_usable.is_none_or(|u| !u.rotate && !u.slide) {
                        ray_usable = hit.usable;
                    }
                    count += 1;
                    self.avoid_dir +=
                        -direction * inverse_lerp(r.avoid_ray_range, 0., hit.distance);
                    break;
                }
            }
        }
        self.hits = hits;
        if count > 0 {
            self.avoid_dir /= count as f32;
            let along = self.avoid_dir - fwd * self.avoid_dir.dot(fwd);
            self.avoid_dir = self.avoid_dir.lerp(along, r.avoid_lerp.clamp(0., 1.));
        }
        self.avoid_dir *= r.avoid_force;
        ray_usable
    }

    /// Source `GetNewPatrolPos`. Source sorts the points by their distance to the world origin;
    /// the port sorts by the distance to the floating origin.
    fn new_patrol_pos(
        &mut self,
        r: &HumanAiReference,
        frame: &Frame,
        world: &mut dyn World,
        random: &mut StormRandom,
    ) -> Vector {
        let fwd = forward(frame.pose.rotation);
        let points = &mut self.points;
        points.clear();
        points.push(frame.head.translation);
        let count = r.patrol_ray_count as usize;
        let (mut start, mut till) = (0, 1);
        for _ in 0..r.patrol_iteration {
            for j in start..till {
                for k in 0..count {
                    let direction = yaw(inverse_lerp(0., count as f32, k as f32) * 360.) * fwd;
                    let range = random.range(r.patrol_ray_range[0], r.patrol_ray_range[1]);
                    let lead = direction * r.patrol_ray_start_dist;
                    let origin = points[j] + if j == 0 { lead } else { Vector::ZERO };
                    let point = match world.ray(origin, direction, range) {
                        Some(hit) => hit.point - hit.normal * 0.05,
                        None => points[j] + lead + direction * range,
                    };
                    points.push(point);
                }
            }
            start = till;
            till += count;
        }
        points.sort_by(|a, b| a.length().total_cmp(&b.length()));
        let index = (points.len() as f32 * random.range(0.25, 0.75)) as usize;
        points[index.min(points.len() - 1)]
    }
}
