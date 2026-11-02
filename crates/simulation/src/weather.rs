//! Source sandstorm simulation on the fixed step. Presentation reads this state and does not advance it.
use crate::particles::ParticleState;
use crate::reference::WeatherReference;
use crate::save::{StormSnapshot, WeatherSnapshot};
use crate::vehicle::PhysicsWorld;
use rapier3d::math::{Pose, Rotation, Vector};
use rapier3d::prelude::{Ball, Collider, ColliderHandle, QueryFilter, RigidBodyHandle};

mod tumbleweeds;
pub use tumbleweeds::{ImportantObject, Tumbleweeds};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StormId(pub u32);

#[derive(Clone, Copy, Debug, Default)]
pub struct StormInfluence {
    pub id: Option<StormId>,
    pub strength: f32,
}

/// A live interior volume and the root body that owns it. `None` is a static building.
#[derive(Clone, Copy, Debug)]
pub struct InteriorVolume {
    pub collider: ColliderHandle,
    pub root: Option<RigidBodyHandle>,
}

#[derive(Clone, Copy, Debug)]
struct WindTarget {
    body: RigidBodyHandle,
    extent: f32,
}

#[derive(Debug)]
pub struct Storm {
    pub id: StormId,
    pub absolute_position: [f64; 3],
    pub yaw_degrees: f32,
    pub size: f32,
    pub life: f32,
    pub direction: [f32; 2],
    pub speed: f32,
    /// Source currentDistance, read by Move before this step computes it again.
    pub previous_distance: f32,
    /// Source llerp, sampled before Move changes life.
    pub life_strength: f32,
    /// Source distanceLerp.
    pub strength: f32,
    pub current_force: [f32; 3],
    pub next_force: [f32; 3],
    next_wind_time: f64,
    next_scan_time: f64,
    scan_position: [f64; 3],
    /// Source distanceLerp when Wind runs, before this step computes it again.
    wind_strength: f32,
    targets: Vec<WindTarget>,
    /// Big and small dust systems.
    pub particles: [ParticleState; 2],
    /// Source lastPos: the player position of the previous particle step.
    last_player: Option<[f64; 3]>,
    /// Small dust emitter. It stays in place while the player is outside `radius`.
    small_position: [f64; 3],
}

pub struct WeatherState {
    storms: Vec<Storm>,
    influence: StormInfluence,
    /// Source lastTime.
    last_spawn_time: f64,
    /// Source nextTime: the wait from lastTime to the next spawn.
    wait: f32,
    next_id: u32,
    random: StormRandom,
    origin: [f64; 3],
    body_capacity: usize,
    pub tumbleweeds: Tumbleweeds,
}

impl WeatherState {
    pub fn new(reference: &WeatherReference, seed: u64) -> Self {
        let mut random = StormRandom(seed ^ 0x7374_6f72_6d73_0001);
        let wait = random.range(
            reference.min_interval_seconds,
            reference.max_interval_seconds,
        );
        Self {
            storms: Vec::new(),
            influence: StormInfluence::default(),
            last_spawn_time: 0.,
            wait,
            next_id: 0,
            random,
            origin: [0.; 3],
            body_capacity: 0,
            tumbleweeds: Tumbleweeds::new(seed),
        }
    }
    pub fn storms(&self) -> &[Storm] {
        &self.storms
    }
    pub fn influence(&self) -> StormInfluence {
        self.influence
    }
    pub fn next_spawn_time(&self) -> f64 {
        self.last_spawn_time + self.wait as f64
    }

    /// Source save_sandstorm. `now` is the simulation time that `advance` reads.
    pub fn snapshot(&self, reference: &WeatherReference, now: f64) -> WeatherSnapshot {
        WeatherSnapshot {
            since_spawn: (now - self.last_spawn_time) as f32,
            wait: self.wait,
            storms: self
                .storms
                .iter()
                .map(|storm| StormSnapshot {
                    position: storm.absolute_position,
                    yaw: storm.yaw_degrees,
                    prefab: reference.prefab_id,
                    life: storm.life,
                    size: storm.size,
                    speed: storm.speed,
                    direction: storm.direction,
                })
                .collect(),
        }
    }

    /// Source load: storms start with saved values and do not draw new ones.
    /// Wind targets, gust force and particles start empty and rebuild.
    pub fn restore(&mut self, reference: &WeatherReference, now: f64, saved: &WeatherSnapshot) {
        self.last_spawn_time = now - saved.since_spawn as f64;
        self.wait = saved.wait;
        self.storms.clear();
        self.influence = StormInfluence::default();
        for storm in &saved.storms {
            self.push_storm(reference, storm);
        }
    }

    /// Call when bodies enter the world. Wind scans then do not grow target buffers.
    pub fn reserve_bodies(&mut self, count: usize) {
        if count <= self.body_capacity {
            return;
        }
        self.body_capacity = count;
        for storm in &mut self.storms {
            storm
                .targets
                .reserve(count.saturating_sub(storm.targets.len()));
        }
    }

    /// Storm positions and deadlines are absolute. The origin only maps them to physics space.
    pub fn set_origin(&mut self, origin: [f64; 3]) {
        self.origin = origin;
    }

    pub fn spawn_at(&mut self, reference: &WeatherReference, position: [f64; 3]) -> StormId {
        let r = &reference.storm;
        // SpawnAt draws prefab yaw before Start calls FStart: size, direction, then speed.
        let yaw_degrees = self.random.range(0., 360.);
        let size = self.random.range(r.size_range[0], r.size_range[1]);
        let angle = self.random.range(0., std::f32::consts::TAU);
        let speed = self.random.range(r.speed_range[0], r.speed_range[1]);
        self.push_storm(
            reference,
            &StormSnapshot {
                position,
                yaw: yaw_degrees,
                prefab: reference.prefab_id,
                life: r.life,
                size,
                speed,
                direction: [angle.cos(), angle.sin()],
            },
        )
    }

    /// Source Start with started set: the storm keeps the given values.
    fn push_storm(&mut self, reference: &WeatherReference, saved: &StormSnapshot) -> StormId {
        let r = &reference.storm;
        let &StormSnapshot {
            position,
            yaw: yaw_degrees,
            life,
            size,
            speed,
            direction,
            ..
        } = saved;
        let id = StormId(self.next_id);
        self.next_id = self
            .next_id
            .checked_add(1)
            .expect("storm ID space exhausted");
        self.storms.push(Storm {
            id,
            absolute_position: position,
            yaw_degrees,
            size,
            life,
            direction,
            speed,
            previous_distance: 0.,
            life_strength: 0.,
            strength: r.distance_lerp,
            current_force: r.current_force,
            next_force: r.next_force,
            next_wind_time: 0.,
            next_scan_time: f64::NEG_INFINITY,
            scan_position: position,
            wind_strength: r.distance_lerp,
            targets: Vec::with_capacity(self.body_capacity),
            particles: std::array::from_fn(|i| {
                // Validated order: big, then small.
                let mut p =
                    ParticleState::new(&reference.particles[i], id.0 as u64 * 2 + i as u64 + 1);
                p.rotation = Rotation::from_rotation_y(-yaw_degrees.to_radians());
                p.radius_scale = size;
                // Update sets emission rates from defBRate* and defSRate*, scaled by strength.
                let [time, distance] = if i == 0 {
                    [r.def_b_rate_t, r.def_b_rate_d]
                } else {
                    [r.def_s_rate_t, r.def_s_rate_d]
                };
                p.set_rates(time, distance);
                p
            }),
            last_player: None,
            small_position: position,
        });
        id
    }

    /// `now` is normal simulation time, with the sleep time scale and no extra cycle time.
    /// `terrain` gives the terrain height and color at an absolute position.
    pub fn advance(
        &mut self,
        reference: &WeatherReference,
        now: f64,
        dt: f32,
        player_absolute: [f64; 3],
        mut terrain: impl FnMut([f64; 3]) -> (f32, [f32; 4]),
    ) {
        if !now.is_finite()
            || !dt.is_finite()
            || dt <= 0.
            || !player_absolute.iter().all(|v| v.is_finite())
        {
            return;
        }
        if now >= self.next_spawn_time() {
            self.last_spawn_time = now;
            self.wait = self.random.range(
                reference.min_interval_seconds,
                reference.max_interval_seconds,
            );
            let angle = self.random.range(0., std::f32::consts::TAU);
            let mut position = player_absolute;
            position[0] += (angle.cos() * reference.spawn_radius) as f64;
            position[2] += (angle.sin() * reference.spawn_radius) as f64;
            self.spawn_at(reference, position);
        }
        let r = &reference.storm;
        self.influence = StormInfluence::default();
        for storm in &mut self.storms {
            storm.life_strength = storm.size * storm.life.clamp(0., 1.);
            storm.scan_position = storm.absolute_position;
            let (height, color) = terrain(storm.absolute_position);
            storm.absolute_position[0] += (storm.direction[0] * storm.speed * dt) as f64;
            storm.absolute_position[1] = height as f64;
            storm.absolute_position[2] += (storm.direction[1] * storm.speed * dt) as f64;
            // The terrain color already holds the game mode override, as for footsteps.
            let biome = crate::world::biome(color, None);
            if r.biome != biome || storm.previous_distance >= r.destroy_dist {
                storm.life = (storm.life - dt * r.life_loss_on_other_biome).clamp(0., 1.);
            } else {
                storm.life = (storm.life + dt * r.life_gain).clamp(0., 1.);
            }
            if now > storm.next_wind_time {
                storm.next_wind_time = now
                    + self
                        .random
                        .range(r.wind_time_range[0], r.wind_time_range[1])
                        as f64;
                storm.next_force = self.random.inside_sphere();
            }
            for i in 0..3 {
                storm.current_force[i] +=
                    (storm.next_force[i] - storm.current_force[i]) * dt.clamp(0., 1.);
            }
            // Wind runs before this Update sets distanceLerp.
            storm.wind_strength = storm.strength;
            storm.previous_distance = storm
                .absolute_position
                .iter()
                .zip(player_absolute)
                .map(|(a, b)| (a - b).powi(2))
                .sum::<f64>()
                .sqrt() as f32;
            storm.strength = ((r.radius - storm.previous_distance) / (r.radius * 0.5))
                .clamp(0., 1.)
                * storm.life_strength;
            if storm.strength > self.influence.strength {
                self.influence = StormInfluence {
                    id: Some(storm.id),
                    strength: storm.strength,
                };
            }
        }
        self.storms.retain(|storm| storm.life > 0.);
    }

    /// Source dust after `advance`. `night` is the SetColorLerp weight: 0 day, 1 night.
    /// `interior` selects interior colliders. Particles that touch them are removed.
    pub fn advance_particles(
        &mut self,
        reference: &WeatherReference,
        dt: f32,
        player_absolute: [f64; 3],
        night: f32,
        physics: &PhysicsWorld,
        interior: impl Fn(ColliderHandle) -> bool,
    ) {
        if !dt.is_finite() || dt <= 0. {
            return;
        }
        let r = &reference.storm;
        for storm in &mut self.storms {
            let last = storm.last_player.unwrap_or(player_absolute);
            storm.last_player = Some(player_absolute);
            if storm.previous_distance < r.radius {
                storm.small_position = small_emitter(player_absolute, last, dt, r.yoffset);
            }
            let emitters = [
                (storm.absolute_position, storm.life_strength),
                (storm.small_position, storm.strength),
            ];
            for (p, (position, strength)) in storm.particles.iter_mut().zip(emitters) {
                let first = crate::lighting::gradient(&r.day_color1, p.time());
                let second = crate::lighting::gradient(&r.day_color2, p.time());
                let draw = p.random_draw();
                p.start_color = Some(std::array::from_fn(|j| {
                    let day = first[j] + (second[j] - first[j]) * draw;
                    day + (r.night_color[j] - day) * night
                }));
                p.set_origin(self.origin);
                p.advance(dt, position, strength, physics);
                p.remove_inside(physics, &interior);
            }
        }
    }

    /// Live dust particles of all storms.
    pub fn particle_count(&self) -> usize {
        self.storms
            .iter()
            .flat_map(|s| &s.particles)
            .map(|p| p.particles().len())
            .sum()
    }

    /// Collects wind targets once per `wind_scan_seconds` for each storm.
    /// `root_of` gives a body's root, or `None` for a body that wind does not move.
    pub fn scan_wind(
        &mut self,
        physics: &PhysicsWorld,
        reference: &WeatherReference,
        now: f64,
        root_of: impl Fn(RigidBodyHandle) -> Option<RigidBodyHandle>,
    ) {
        let r = &reference.storm;
        let mask = |_: ColliderHandle, collider: &Collider| {
            collider.is_enabled() && collider.collision_groups().memberships.bits() & r.wind_lm != 0
        };
        let queries = physics.queries(QueryFilter::default().predicate(&mask));
        for storm in &mut self.storms {
            if now < storm.next_scan_time {
                continue;
            }
            let center = if storm.next_scan_time == f64::NEG_INFINITY {
                storm.scan_position
            } else {
                storm.absolute_position
            };
            storm.next_scan_time = now + r.wind_scan_seconds as f64;
            storm.targets.clear();
            let position =
                Vector::from_array(std::array::from_fn(|i| (center[i] - self.origin[i]) as f32));
            for (_, collider) in
                queries.intersect_shape(Pose::from_translation(position), &Ball::new(r.radius))
            {
                let Some(body) = collider.parent().filter(|body| root_of(*body).is_some()) else {
                    continue;
                };
                let bounds = collider.compute_aabb();
                let extent = (bounds.maxs - bounds.mins).element_sum();
                if let Some(target) = storm.targets.iter_mut().find(|t| t.body == body) {
                    target.extent += extent;
                } else {
                    storm.targets.push(WindTarget { body, extent });
                }
            }
        }
    }

    /// Source Wind. Skips a body inside an interior of another root.
    pub fn apply_wind(
        &self,
        physics: &mut PhysicsWorld,
        reference: &WeatherReference,
        interiors: &[InteriorVolume],
        dt: f32,
        root_of: impl Fn(RigidBodyHandle) -> Option<RigidBodyHandle>,
    ) {
        if !dt.is_finite() || dt <= 0. {
            return;
        }
        let r = &reference.storm;
        let integration_dt = physics.rapier().integration_parameters.dt;
        for storm in &self.storms {
            for target in &storm.targets {
                let Some(root) = root_of(target.body) else {
                    continue;
                };
                let world = physics.rapier();
                let Some(body) = world.bodies.get(target.body) else {
                    continue;
                };
                if !body.is_enabled() || !body.is_dynamic() {
                    continue;
                }
                let position = body.translation();
                if interiors.iter().any(|interior| {
                    interior.root != Some(root)
                        && world
                            .colliders
                            .get(interior.collider)
                            .is_some_and(|c| c.shape().contains_point(c.position(), position))
                }) {
                    continue;
                }
                // Unity AddForce uses Force mode. Keep its explicit dt, then integrate one step.
                let scale = r.wind_force
                    * dt
                    * target
                        .extent
                        .clamp(r.wind_bounds_range[0], r.wind_bounds_range[1])
                    * storm.wind_strength
                    * integration_dt;
                let impulse = Vector::from_array(storm.current_force) * scale;
                physics.edit_body(target.body, |body| body.apply_impulse(impulse, true));
            }
        }
    }
}

/// Source small dust position: halfway between lastPos and the player, led by twice its velocity.
fn small_emitter(player: [f64; 3], last: [f64; 3], dt: f32, yoffset: f32) -> [f64; 3] {
    let dt = dt.clamp(0.0001, 1.) as f64;
    std::array::from_fn(|i| {
        let up = if i == 1 { yoffset as f64 } else { 0. };
        (player[i] + (player[i] - last[i]) / dt * 2. + up + last[i]) * 0.5
    })
}

/// A separate deterministic stream. Unity's native random sequence is unrecovered.
#[derive(Debug)]
pub(crate) struct StormRandom(pub(crate) u64);
impl StormRandom {
    pub(crate) fn unit(&mut self) -> f32 {
        self.0 = self.0.wrapping_add(0x9e37_79b9_7f4a_7c15);
        (crate::world::hash(self.0) >> 40) as f32 / 16_777_216.
    }
    pub(crate) fn range(&mut self, min: f32, max: f32) -> f32 {
        min + (max - min) * self.unit()
    }
    /// A uniform direction.
    pub(crate) fn on_unit_sphere(&mut self) -> [f32; 3] {
        let vertical = self.range(-1., 1.);
        let angle = self.range(0., std::f32::consts::TAU);
        let horizontal = (1. - vertical * vertical).sqrt();
        [horizontal * angle.cos(), vertical, horizontal * angle.sin()]
    }
    fn inside_sphere(&mut self) -> [f32; 3] {
        let vertical = self.range(-1., 1.);
        let angle = self.range(0., std::f32::consts::TAU);
        let radius = self.unit().cbrt();
        let horizontal = (1. - vertical * vertical).sqrt();
        [
            horizontal * angle.cos() * radius,
            vertical * radius,
            horizontal * angle.sin() * radius,
        ]
    }
}
