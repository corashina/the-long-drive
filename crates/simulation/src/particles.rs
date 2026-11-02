//! Selected Unity particle modules. Absolute positions keep custom space stable across rebases.
use crate::reference::{ParticleColor, ParticleCurve, ParticleReference};
use crate::vehicle::PhysicsWorld;
use rapier3d::math::{Pose, Rotation, Vector};
use rapier3d::parry::query::ShapeCastOptions;
use rapier3d::prelude::{Ball, ColliderHandle, QueryFilter};

#[derive(Clone, Copy, Debug)]
pub struct ParticleOutput {
    pub absolute_position: [f64; 3],
    pub size: f32,
    pub color: [f32; 4],
    pub rotation: f32,
    pub age: f32,
    pub lifetime: f32,
    velocity: Vector,
    force: Vector,
    start_size: f32,
    start_color: [f32; 4],
    rotation_speed: f32,
    size_draw: f32,
    velocity_draw: [f32; 3],
    inherit_draw: f32,
    collision_draw: f32,
    uv_draw: f32,
}

/// Module fields of one reference, read once. The per-particle loops do no map lookups.
#[derive(Debug)]
struct Modules {
    capacity: usize,
    rate_over_time: f32,
    rate_over_distance: f32,
    duration: f32,
    local_rotation: Rotation,
    local_translation: Vector,
    /// `None` when the game object is inactive.
    emission: Option<Emission>,
    /// Trigger module with the Kill inside action: the ball radius scale.
    kill_inside: Option<f32>,
}
/// Source `ShapeModule.type`: 0 sphere, 2 hemisphere, 4 cone.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Shape {
    Sphere,
    Hemisphere,
    Cone,
}
#[derive(Debug)]
struct Emission {
    shape: Shape,
    cos_angle: f32,
    radius_thickness: f32,
    shape_radius: f32,
    random_position_amount: f32,
    shape_scale: Vector,
    shape_rotation: Rotation,
    start_speed: ParticleCurve,
    start_lifetime: ParticleCurve,
    start_size: ParticleCurve,
    start_color: ParticleColor,
    start_rotation: ParticleCurve,
    gravity: ParticleCurve,
    rotation: Option<ParticleCurve>,
    force: Option<Force>,
    velocity: Option<VelocityModule>,
    inherit: Option<Inherit>,
    collision: Option<Collision>,
    noise: Option<Noise>,
    uv: bool,
    size: Option<ParticleCurve>,
    color: Option<ParticleColor>,
    burst: Option<usize>,
    ring: bool,
}
#[derive(Debug)]
struct Force {
    curves: [ParticleCurve; 3],
    world: bool,
}
#[derive(Debug)]
struct VelocityModule {
    curves: [ParticleCurve; 3],
    speed_modifier: ParticleCurve,
    world: bool,
}
#[derive(Debug)]
struct Inherit {
    /// Source `InheritVelocityModule.m_Mode`: 0 initial velocity, 1 current velocity.
    mode: f32,
    curve: ParticleCurve,
}
/// Source `NoiseModule` position noise. The noise field is an approximation: Unity's native
/// noise is not ported. Strength, frequency, octaves, scroll and damping follow the source fields.
#[derive(Debug)]
struct Noise {
    strength: [ParticleCurve; 3],
    position_amount: ParticleCurve,
    scroll_speed: ParticleCurve,
    frequency: f32,
    octaves: u32,
    octave_multiplier: f32,
    octave_scale: f32,
    /// Source `damping`: the strength is divided by the frequency.
    damping: f32,
}
/// Per-axis offsets into the 2D noise field, so that the axes do not move together.
const NOISE_OFFSETS: [[f32; 2]; 3] = [[17.3, 61.9], [113.7, 29.1], [71.5, 157.3]];
impl Noise {
    /// Noise in [-1, 1] per axis at an absolute position, before strength.
    fn sample(&self, absolute: [f64; 3], scroll: f32) -> Vector {
        let mut out = [0f32; 3];
        let (mut frequency, mut amplitude) = (f64::from(self.frequency), 1.);
        for _ in 0..self.octaves {
            // The 2D field repeats every 256 units. Wrap in f64 to keep precision far out.
            let q: [f32; 3] =
                std::array::from_fn(|i| (absolute[i] * frequency).rem_euclid(256.) as f32);
            for (i, value) in out.iter_mut().enumerate() {
                let [a, b] = NOISE_OFFSETS[i];
                let first = terrain::source_perlin(q[(i + 1) % 3] + a, q[(i + 2) % 3] + scroll + b);
                let second = terrain::source_perlin(q[i] + b, q[(i + 1) % 3] + scroll + a);
                *value += (first + second - 1.) * amplitude;
            }
            frequency *= f64::from(self.octave_scale);
            amplitude *= self.octave_multiplier;
        }
        Vector::from_array(out)
    }
}
#[derive(Debug)]
struct Collision {
    mask: u32,
    dynamic: bool,
    radius_scale: f32,
    energy_loss: ParticleCurve,
    dampen: ParticleCurve,
    bounce: ParticleCurve,
    min_kill_speed: f32,
    max_kill_speed: f32,
}
impl Modules {
    fn new(r: &ParticleReference) -> Self {
        let on = |module: &str| r.module_flags.get(module) == Some(&true);
        let curve = |field: &str| r.curves[field].clone();
        let xyz = |module: &str| {
            [
                curve(&format!("{module}.x")),
                curve(&format!("{module}.y")),
                curve(&format!("{module}.z")),
            ]
        };
        // A disabled shape emits from one point along +Z: a cone with zero angle and radius.
        let shape = on("ShapeModule");
        let shape_scalar = |field: &str, off: f32| if shape { r.scalars[field] } else { off };
        let shape_vector = |field: &str, off: [f32; 3]| if shape { r.vectors[field] } else { off };
        let emission = (r.flags.get("gameObject.active") == Some(&true)).then(|| Emission {
            shape: match shape_scalar("ShapeModule.type", 4.) {
                0. => Shape::Sphere,
                2. => Shape::Hemisphere,
                _ => Shape::Cone,
            },
            cos_angle: shape_scalar("ShapeModule.angle", 0.).to_radians().cos(),
            radius_thickness: shape_scalar("ShapeModule.radiusThickness", 1.),
            shape_radius: shape_scalar("ShapeModule.radius.value", 0.),
            random_position_amount: shape_scalar("ShapeModule.randomPositionAmount", 0.),
            shape_scale: Vector::from_array(shape_vector("ShapeModule.m_Scale", [1.; 3])),
            shape_rotation: shape_rotation(shape_vector("ShapeModule.m_Rotation", [0.; 3])),
            start_speed: curve("InitialModule.startSpeed"),
            start_lifetime: curve("InitialModule.startLifetime"),
            start_size: curve("InitialModule.startSize"),
            start_color: r.colors["InitialModule.startColor"].clone(),
            start_rotation: curve("InitialModule.startRotation"),
            gravity: curve("InitialModule.gravityModifier"),
            rotation: r.curves.get("RotationModule.curve").cloned(),
            force: on("ForceModule").then(|| Force {
                curves: xyz("ForceModule"),
                world: r.flags["ForceModule.inWorldSpace"],
            }),
            velocity: on("VelocityModule").then(|| VelocityModule {
                curves: xyz("VelocityModule"),
                speed_modifier: curve("VelocityModule.speedModifier"),
                world: r.flags["VelocityModule.inWorldSpace"],
            }),
            inherit: on("InheritVelocityModule").then(|| Inherit {
                mode: r.scalars["InheritVelocityModule.m_Mode"],
                curve: curve("InheritVelocityModule.m_Curve"),
            }),
            collision: on("CollisionModule").then(|| Collision {
                mask: r.scalars["CollisionModule.collidesWith.m_Bits"] as u32,
                dynamic: r.flags["CollisionModule.collidesWithDynamic"],
                radius_scale: r.scalars["CollisionModule.radiusScale"],
                energy_loss: curve("CollisionModule.m_EnergyLossOnCollision"),
                dampen: curve("CollisionModule.m_Dampen"),
                bounce: curve("CollisionModule.m_Bounce"),
                min_kill_speed: r.scalars["CollisionModule.minKillSpeed"],
                max_kill_speed: r.scalars["CollisionModule.maxKillSpeed"],
            }),
            noise: on("NoiseModule").then(|| {
                let frequency = r.scalars["NoiseModule.frequency"];
                let strength = if r.flags["NoiseModule.separateAxes"] {
                    [
                        curve("NoiseModule.strength"),
                        curve("NoiseModule.strengthY"),
                        curve("NoiseModule.strengthZ"),
                    ]
                } else {
                    std::array::from_fn(|_| curve("NoiseModule.strength"))
                };
                Noise {
                    strength,
                    position_amount: curve("NoiseModule.positionAmount"),
                    scroll_speed: curve("NoiseModule.scrollSpeed"),
                    frequency,
                    octaves: r.scalars["NoiseModule.octaves"] as u32,
                    octave_multiplier: r.scalars["NoiseModule.octaveMultiplier"],
                    octave_scale: r.scalars["NoiseModule.octaveScale"],
                    damping: if r.flags["NoiseModule.damping"] {
                        1. / frequency
                    } else {
                        1.
                    },
                }
            }),
            uv: on("UVModule"),
            size: r.curves.get("SizeModule.curve").cloned(),
            color: r.colors.get("ColorModule.gradient").cloned(),
            burst: (r.scalars.get("EmissionModule.m_BurstCount") == Some(&1.))
                .then(|| r.curves["EmissionModule.m_Bursts.0.countCurve"].scalar as usize),
            ring: r.scalars["ringBufferMode"] == 1.,
        });
        Self {
            capacity: r.capacity as usize,
            rate_over_time: r.rate_over_time,
            rate_over_distance: r.rate_over_distance,
            duration: r.duration,
            local_rotation: Rotation::from_array(r.local_pose.rotation),
            local_translation: Vector::from_array(r.local_pose.translation),
            emission,
            kill_inside: (on("TriggerModule") && r.scalars["TriggerModule.inside"] == 1.)
                .then(|| 0.5 * r.scalars["TriggerModule.radiusScale"]),
        }
    }
}
/// Source `ShapeModule.rotation` in source degrees. Unity's ZXY order, with +Z converted to -Z.
fn shape_rotation(e: [f32; 3]) -> Rotation {
    Rotation::from_rotation_y(-e[1].to_radians())
        * Rotation::from_rotation_x(-e[0].to_radians())
        * Rotation::from_rotation_z(e[2].to_radians())
}

#[derive(Debug)]
pub struct ParticleState {
    modules: Modules,
    particles: Vec<ParticleOutput>,
    random: ParticleRandom,
    remainder: f32,
    elapsed: f32,
    last_emitter: Option<[f64; 3]>,
    /// Source `NoiseModule.scrollSpeed` integrated over time.
    noise_scroll: f32,
    pub rotation: Rotation,
    pub emission_rotation: Option<Rotation>,
    /// Scales the shape radius. Storms set it to their size.
    pub radius_scale: f32,
    pub start_color: Option<[f32; 4]>,
    pub origin: [f64; 3],
    burst_started: bool,
}
impl ParticleOutput {
    pub fn uv_draw(&self) -> f32 {
        self.uv_draw
    }
}
impl ParticleState {
    pub fn new(reference: &ParticleReference, seed: u64) -> Self {
        Self {
            modules: Modules::new(reference),
            particles: Vec::with_capacity(reference.capacity as usize),
            random: ParticleRandom(seed),
            remainder: 0.,
            elapsed: 0.,
            last_emitter: None,
            noise_scroll: 0.,
            rotation: Rotation::IDENTITY,
            emission_rotation: None,
            radius_scale: 1.,
            start_color: None,
            origin: [0.; 3],
            burst_started: false,
        }
    }
    /// A system that only runs `Emit`: no authored burst starts it.
    pub fn emit_only(reference: &ParticleReference, seed: u64) -> Self {
        Self {
            burst_started: true,
            ..Self::new(reference, seed)
        }
    }
    /// `mainscript.Particle`: the system moves to the point and runs `Emit(count)`.
    pub fn emit(&mut self, count: usize, emitter_absolute: [f64; 3], rotation: Rotation) {
        let m = &self.modules;
        if m.emission.is_none() || count == 0 {
            return;
        }
        let offset = rotation * m.local_translation;
        let emission = self
            .emission_rotation
            .unwrap_or(rotation * m.local_rotation);
        self.rotation = rotation;
        self.reset_emitter();
        let emitter = std::array::from_fn(|i| emitter_absolute[i] + offset[i] as f64);
        self.spawn(count, emitter, emission, Vector::ZERO);
    }
    pub fn particles(&self) -> &[ParticleOutput] {
        &self.particles
    }
    /// Source `ParticleSystem.time`: seconds into the current duration.
    pub fn time(&self) -> f32 {
        self.elapsed
    }
    /// One draw from this system's stream, for a start color set by its owner.
    pub fn random_draw(&mut self) -> f32 {
        self.random.unit()
    }
    /// Base emission rates. `advance` scales them by its strength.
    pub fn set_rates(&mut self, over_time: f32, over_distance: f32) {
        self.modules.rate_over_time = over_time;
        self.modules.rate_over_distance = over_distance;
    }
    /// Source `ShapeModule.rotation`: Euler angles in source degrees.
    pub fn set_shape_rotation(&mut self, degrees: [f32; 3]) {
        if let Some(e) = &mut self.modules.emission {
            e.shape_rotation = shape_rotation(degrees);
        }
    }
    /// Source trigger module with the Kill inside action: removes particles that touch an
    /// interior collider. `interior` selects those colliders. Call after `advance`.
    pub fn remove_inside(
        &mut self,
        physics: &PhysicsWorld,
        interior: impl Fn(ColliderHandle) -> bool,
    ) {
        let Some(scale) = self.modules.kill_inside else {
            return;
        };
        let predicate = |handle: ColliderHandle, _: &rapier3d::prelude::Collider| interior(handle);
        let queries = physics.queries(QueryFilter::default().predicate(&predicate));
        let origin = self.origin;
        self.particles.retain(|p| {
            let position = Vector::from_array(std::array::from_fn(|i| {
                (p.absolute_position[i] - origin[i]) as f32
            }));
            let ball = Ball::new((p.size * scale).max(f32::EPSILON));
            queries
                .intersect_shape(Pose::from_translation(position), &ball)
                .next()
                .is_none()
        });
    }
    /// A new Instantiate of the system: its burst and duration start again.
    pub fn restart(&mut self) {
        self.burst_started = false;
        self.elapsed = 0.;
        self.remainder = 0.;
    }
    /// Removes every live particle.
    pub fn clear(&mut self) {
        self.particles.clear();
    }
    pub fn reset_emitter(&mut self) {
        self.last_emitter = None;
    }
    pub fn set_origin(&mut self, origin: [f64; 3]) {
        if self.origin != origin {
            self.reset_emitter();
            self.origin = origin;
        }
    }
    /// Spawns `count` particles at the emitter, as one emission step.
    fn spawn(
        &mut self,
        count: usize,
        emitter_absolute: [f64; 3],
        rotation: Rotation,
        emitter_velocity: Vector,
    ) {
        let m = &self.modules;
        let Some(e) = &m.emission else {
            return;
        };
        let random = &mut self.random;
        let phase = self.elapsed / m.duration;
        for _ in 0..count.min(m.capacity - self.particles.len()) {
            let a = random.unit() * std::f32::consts::TAU;
            let z = match e.shape {
                Shape::Sphere => 2. * random.unit() - 1.,
                Shape::Hemisphere => random.unit(),
                Shape::Cone => e.cos_angle + (1. - e.cos_angle) * random.unit(),
            };
            let radial = (1. - z * z).sqrt();
            // Source +Z is converted to -Z.
            let direction = Vector::new(a.cos() * radial, a.sin() * radial, -z);
            let inner = (1. - e.radius_thickness).powi(3);
            let shape_radius = e.shape_radius * self.radius_scale;
            let radius = shape_radius * (inner + (1. - inner) * random.unit()).cbrt();
            let mut position = if e.shape != Shape::Cone {
                direction * radius
            } else {
                Vector::new(a.cos(), a.sin(), 0.) * shape_radius * random.unit().sqrt()
            };
            // Native EmitterStoreData adds a random unit vector times randomPositionAmount.
            if e.random_position_amount > 0. {
                position += Vector::from_array(random.inside_sphere()).normalize_or_zero()
                    * e.random_position_amount;
            }
            position *= e.shape_scale;
            position = rotation * (e.shape_rotation * position);
            let speed = particle_curve(&e.start_speed, phase, random.unit());
            let lifetime = particle_curve(&e.start_lifetime, phase, random.unit());
            let size = particle_curve(&e.start_size, phase, random.unit());
            let color = self
                .start_color
                .unwrap_or_else(|| particle_color(&e.start_color, 0., random.unit()));
            let mut force = Vector::ZERO;
            if let Some(f) = &e.force {
                force = Vector::new(
                    particle_curve(&f.curves[0], 0., random.unit()),
                    particle_curve(&f.curves[1], 0., random.unit()),
                    -particle_curve(&f.curves[2], 0., random.unit()),
                );
                if !f.world {
                    force = rotation * force;
                }
            }
            force.y -= 9.81 * particle_curve(&e.gravity, 0., random.unit());
            let velocity_draw = if e.velocity.is_some() {
                std::array::from_fn(|_| random.unit())
            } else {
                [0.; 3]
            };
            let inherit_draw = if e.inherit.is_some() {
                random.unit()
            } else {
                0.
            };
            let inherited = match &e.inherit {
                Some(i) if i.mode == 0. => {
                    emitter_velocity * particle_curve(&i.curve, 0., inherit_draw)
                }
                _ => Vector::ZERO,
            };
            let collision_draw = if e.collision.is_some() {
                random.unit()
            } else {
                0.
            };
            let uv_draw = if e.uv { random.unit() } else { 0. };
            self.particles.push(ParticleOutput {
                absolute_position: std::array::from_fn(|i| {
                    emitter_absolute[i] + position[i] as f64
                }),
                size,
                color,
                rotation: particle_curve(&e.start_rotation, 0., random.unit()),
                age: 0.,
                lifetime,
                velocity: rotation
                    * (e.shape_rotation * (direction * e.shape_scale).normalize_or_zero())
                    * speed
                    + inherited,
                force,
                start_size: size,
                start_color: color,
                rotation_speed: e
                    .rotation
                    .as_ref()
                    .map_or(0., |c| particle_curve(c, 0., random.unit())),
                size_draw: random.unit(),
                velocity_draw,
                inherit_draw,
                collision_draw,
                uv_draw,
            });
        }
    }
    pub fn advance(
        &mut self,
        dt: f32,
        emitter_absolute: [f64; 3],
        strength: f32,
        physics: &PhysicsWorld,
    ) {
        if self.modules.emission.is_none() || !dt.is_finite() || dt <= 0. {
            self.reset_emitter();
            return;
        }
        let m = &self.modules;
        let rotation = self
            .emission_rotation
            .unwrap_or(self.rotation * m.local_rotation);
        let offset = self.rotation * m.local_translation;
        let emitter_absolute = std::array::from_fn(|i| emitter_absolute[i] + offset[i] as f64);
        let emitter_velocity = self.last_emitter.map_or(Vector::ZERO, |p| {
            Vector::from_array(std::array::from_fn(|i| {
                ((emitter_absolute[i] - p[i]) / dt as f64) as f32
            }))
        });
        let distance = self.last_emitter.map_or(0., |p| {
            (0..3)
                .map(|i| (p[i] - emitter_absolute[i]).powi(2))
                .sum::<f64>()
                .sqrt() as f32
        });
        self.last_emitter = Some(emitter_absolute);
        self.elapsed = (self.elapsed + dt) % m.duration;
        self.remainder +=
            (m.rate_over_time * dt + m.rate_over_distance * distance) * strength.max(0.);
        let mut count = self.remainder.floor() as usize;
        self.remainder -= count as f32;
        // Authored splash starts with one burst. No managed collision-to-splash link exists.
        if !self.burst_started {
            count += m.emission.as_ref().and_then(|e| e.burst).unwrap_or(0);
        }
        self.burst_started = true;
        self.spawn(count, emitter_absolute, rotation, emitter_velocity);
        let e = self.modules.emission.as_ref().expect("active system");
        if let Some(n) = &e.noise {
            let phase = self.elapsed / self.modules.duration;
            self.noise_scroll =
                (self.noise_scroll + particle_curve(&n.scroll_speed, phase, 0.5) * dt) % 256.;
        }
        let scroll = self.noise_scroll;
        let origin = self.origin;
        // Disabled modules do not access their payload or issue physics queries.
        let surfaces = |_: ColliderHandle, collider: &rapier3d::prelude::Collider| {
            e.collision.as_ref().is_some_and(|c| {
                collider.is_enabled()
                    && !collider.is_sensor()
                    && collider.collision_groups().memberships.bits() & c.mask != 0
                    && (c.dynamic
                        || collider
                            .parent()
                            .is_none_or(|body| !physics.rapier().bodies[body].is_dynamic()))
            })
        };
        let queries = physics.queries(QueryFilter::default().predicate(&surfaces));
        let size_at = |p: &ParticleOutput, t: f32| {
            p.start_size
                * e.size
                    .as_ref()
                    .map_or(1., |c| particle_curve(c, t, p.size_draw))
        };
        self.particles.retain_mut(|p| {
            if p.age >= p.lifetime {
                return e.ring;
            }
            p.age = (p.age + dt).min(p.lifetime);
            if p.age >= p.lifetime && !e.ring {
                return false;
            }
            p.velocity += p.force * dt;
            let t = p.age / p.lifetime;
            let mut module_velocity = Vector::ZERO;
            let mut speed_modifier = 1.;
            if let Some(v) = &e.velocity {
                module_velocity = Vector::new(
                    particle_curve(&v.curves[0], t, p.velocity_draw[0]),
                    particle_curve(&v.curves[1], t, p.velocity_draw[1]),
                    -particle_curve(&v.curves[2], t, p.velocity_draw[2]),
                );
                if !v.world {
                    module_velocity = rotation * module_velocity;
                }
                speed_modifier = particle_curve(&v.speed_modifier, t, p.velocity_draw[0]);
            }
            if let Some(i) = e.inherit.as_ref().filter(|i| i.mode == 1.) {
                module_velocity += emitter_velocity * particle_curve(&i.curve, t, p.inherit_draw);
            }
            let velocity = (p.velocity + module_velocity) * speed_modifier;
            let mut travel = velocity * dt;
            // Noise moves the position only. It does not change the particle velocity.
            if let Some(n) = &e.noise {
                let amount = particle_curve(&n.position_amount, t, p.size_draw) * n.damping * dt;
                let strength = Vector::new(
                    particle_curve(&n.strength[0], t, p.size_draw),
                    particle_curve(&n.strength[1], t, p.size_draw),
                    particle_curve(&n.strength[2], t, p.size_draw),
                );
                travel += n.sample(p.absolute_position, scroll) * strength * amount;
            }
            let mut position = Vector::from_array(std::array::from_fn(|i| {
                (p.absolute_position[i] - origin[i]) as f32
            }));
            let mut collided = false;
            if let Some(c) = &e.collision
                && let Some((_, hit)) = queries.cast_shape(
                    &Pose::from_parts(position, Rotation::IDENTITY),
                    travel,
                    &Ball::new(size_at(p, t) * 0.5 * c.radius_scale),
                    ShapeCastOptions {
                        max_time_of_impact: 1.,
                        stop_at_penetration: true,
                        ..Default::default()
                    },
                )
            {
                // Match the existing fluid swept-sphere route for initial penetration.
                if hit.time_of_impact <= 0. {
                    return false;
                }
                p.age += p.lifetime * particle_curve(&c.energy_loss, t, p.collision_draw);
                if p.age >= p.lifetime {
                    return false;
                }
                let normal = hit.normal1.normalize_or_zero();
                let inward = velocity.dot(normal).min(0.);
                let bounced = (velocity - normal * inward)
                    * (1. - particle_curve(&c.dampen, t, p.collision_draw))
                    - normal * inward * particle_curve(&c.bounce, t, p.collision_draw);
                let speed = bounced.length();
                if speed < c.min_kill_speed || speed > c.max_kill_speed {
                    return false;
                }
                position += travel * hit.time_of_impact + normal * 0.0001;
                // Preserve the response when lifetime velocity is applied next frame.
                p.velocity = if speed_modifier > 0. {
                    bounced / speed_modifier - module_velocity
                } else {
                    Vector::ZERO
                };
                collided = true;
            }
            if collided {
                p.absolute_position = std::array::from_fn(|i| origin[i] + position[i] as f64);
            } else {
                for i in 0..3 {
                    p.absolute_position[i] += travel[i] as f64;
                }
            }
            p.rotation += p.rotation_speed * dt;
            let t = p.age / p.lifetime;
            p.size = size_at(p, t);
            let c = e
                .color
                .as_ref()
                .map_or([1.; 4], |c| particle_color(c, t, p.size_draw));
            p.color = std::array::from_fn(|i| p.start_color[i] * c[i]);
            true
        });
    }
}

/// Unity texture sheet animation over lifetime on a whole grid sheet.
#[derive(Clone, Debug)]
pub struct TextureSheet {
    pub tiles: [u32; 2],
    frame_over_time: ParticleCurve,
    start_frame: ParticleCurve,
    cycles: f32,
}
impl TextureSheet {
    /// `None` when the UV module is off.
    pub fn new(r: &ParticleReference) -> Option<Self> {
        (r.module_flags.get("UVModule") == Some(&true)).then(|| Self {
            tiles: [
                r.scalars["UVModule.tilesX"] as u32,
                r.scalars["UVModule.tilesY"] as u32,
            ],
            frame_over_time: r.curves["UVModule.frameOverTime"].clone(),
            start_frame: r.curves["UVModule.startFrame"].clone(),
            cycles: r.scalars["UVModule.cycles"],
        })
    }
    pub fn frame(&self, p: &ParticleOutput) -> u32 {
        self.frame_at(p.age, p.lifetime, p.uv_draw)
    }
    /// Sheet frame of a particle with this age, lifetime and UV random draw.
    pub fn frame_at(&self, age: f32, lifetime: f32, uv_draw: f32) -> u32 {
        let t = age / lifetime;
        sheet_frame(
            particle_curve(&self.frame_over_time, t, uv_draw),
            particle_curve(&self.start_frame, t, uv_draw),
            self.cycles,
            self.tiles[0] * self.tiles[1],
        )
    }
}
/// Frame from normalized curve values. Frame 0 is the top-left tile, row by row.
fn sheet_frame(over_time: f32, start: f32, cycles: f32, frames: u32) -> u32 {
    let phase = (over_time * cycles + start).rem_euclid(1.);
    ((phase * frames as f32) as u32).min(frames - 1)
}

pub fn particle_curve(c: &ParticleCurve, t: f32, draw: f32) -> f32 {
    use crate::simulation::engine::torque;
    match c.mode {
        0 => c.scalar,
        1 => c.scalar * torque(&c.max_curve, t),
        2 => {
            c.scalar
                * (torque(&c.min_curve, t)
                    + (torque(&c.max_curve, t) - torque(&c.min_curve, t)) * draw)
        }
        3 => c.min_scalar + (c.scalar - c.min_scalar) * draw,
        _ => unreachable!("validated particle curve"),
    }
}
pub fn particle_color(c: &ParticleColor, t: f32, draw: f32) -> [f32; 4] {
    let mix = |a: [f32; 4], b: [f32; 4]| std::array::from_fn(|i| a[i] + (b[i] - a[i]) * draw);
    match c.mode {
        0 => c.max_color,
        1 => crate::lighting::gradient(&c.max_gradient, t),
        2 => mix(c.min_color, c.max_color),
        3 => mix(
            crate::lighting::gradient(&c.min_gradient, t),
            crate::lighting::gradient(&c.max_gradient, t),
        ),
        4 => crate::lighting::gradient(&c.max_gradient, draw),
        _ => unreachable!("validated particle color"),
    }
}

/// A separate deterministic stream. Unity's native random sequence is unrecovered.
#[derive(Debug)]
struct ParticleRandom(u64);
impl ParticleRandom {
    fn unit(&mut self) -> f32 {
        self.0 = self.0.wrapping_add(0x9e37_79b9_7f4a_7c15);
        (crate::world::hash(self.0) >> 40) as f32 / 16_777_216.
    }
    fn range(&mut self, min: f32, max: f32) -> f32 {
        min + (max - min) * self.unit()
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

/// The same collision owner serves the player outlet and container outlets.
pub fn fluid_reference(
    p: &crate::reference::ParticleReference,
) -> crate::reference::FluidParticleReference {
    crate::reference::FluidParticleReference {
        source: p.source.clone(),
        local_pose: p.local_pose.clone(),
        capacity: p.capacity,
        cone_degrees: p.scalars["ShapeModule.angle"],
        cone_radius: p.scalars["ShapeModule.radius.value"],
        lifetime_seconds: p.curves["InitialModule.startLifetime"].scalar,
        radius: p.curves["InitialModule.startSize"].scalar
            * 0.5
            * p.scalars["CollisionModule.radiusScale"],
        gravity_multiplier: p.curves["InitialModule.gravityModifier"].scalar,
        collision_mask: p.scalars["CollisionModule.collidesWith.m_Bits"] as u32,
        bounce: p.curves["CollisionModule.m_Bounce"].scalar,
        dampen: p.curves["CollisionModule.m_Dampen"].scalar,
        max_speed: p.scalars["outlet.maxSpeed"],
        min_rate: p.scalars["outlet.minRate"],
        max_rate: p.scalars["outlet.maxRate"],
    }
}
