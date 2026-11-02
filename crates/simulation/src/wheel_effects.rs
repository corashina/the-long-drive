//! Source wheel ground effects: `talajnezo.FixedUpdate` per wheel.
//! Settings `BSmoke` and `BGrass` are taken as on.
use crate::interaction::melee::look_rotation;
use crate::particles::{ParticleOutput, ParticleState};
use crate::reference::{EffectGains, SkidReference, WheelEffectsReference, WheelGains};
use crate::simulation::dials::{inverse_lerp, lerp};
use crate::vehicle::{PhysicsWorld, VEHICLE_COLLIDER_FLAG};
use rapier3d::math::{Rotation, Vector};
use rapier3d::prelude::{Collider, ColliderHandle, QueryFilter, Ray, RigidBodyHandle};

/// Source `v`: steps that `ground` holds after a ray hit.
const GROUND_HOLD: u8 = 5;
/// Source `wrpmSmooth`.
const RPM_SAMPLES: usize = 5;

/// One wheel input. Values come from the wheel model.
#[derive(Clone, Copy, Debug)]
pub struct WheelInput {
    /// Source `W.rpm`. Positive rolls the car forward.
    pub rpm: f32,
    /// Source `W.radius`.
    pub radius: f32,
    /// Source `W.transform.forward`, world. Unity does not turn this node with steer.
    pub forward: Vector,
    /// Source `W.isGrounded`.
    pub grounded: bool,
}

pub struct WheelEffectsInput {
    /// Absolute position of the car root node.
    pub root_absolute: [f64; 3],
    /// Rotation of the car root node.
    pub rotation: Rotation,
    /// Absolute position of the physics origin.
    pub origin: [f64; 3],
    /// The car body. The ground ray ignores its colliders.
    pub chassis: RigidBodyHandle,
    /// Source `RB.velocity`.
    pub linear_velocity: Vector,
    /// Source `carscript.speed`: forward speed magnitude in km/h.
    pub car_kmh: f32,
}

/// Source AudioSource values of one wheel, for the audio voices.
#[derive(Clone, Copy, Debug)]
pub struct WheelSound {
    pub ground_playing: bool,
    pub ground_gain: f32,
    pub spring_playing: bool,
    pub spring_gain: f32,
    pub spring_pitch: f32,
    pub slide_playing: bool,
    pub slide_gain: f32,
    pub slide_pitch: f32,
    /// The slide voice plays the skid road clip, else the skid dirt clip.
    pub slide_road: bool,
}

/// One source `talajnezo`.
#[derive(Debug)]
pub struct WheelEffect {
    dust: Option<ParticleState>,
    grass: Vec<ParticleState>,
    /// Source `v`.
    hold: u8,
    ground: bool,
    road: bool,
    /// Source `lastDist`.
    last_distance: f32,
    /// Source `wrpmarray`.
    rpm: [f32; RPM_SAMPLES],
    sound: WheelSound,
}

/// The wheels of one car and the mainscript `tiresmoke` and `tiresand` bursts.
#[derive(Debug)]
pub struct WheelEffects {
    wheels: Vec<WheelEffect>,
    bursts: [ParticleState; 2],
}

impl WheelEffects {
    pub fn new(wheels: &[WheelEffectsReference], skid: &SkidReference, seed: u64) -> Self {
        Self {
            wheels: wheels
                .iter()
                .enumerate()
                .map(|(w, r)| {
                    let seed = seed.wrapping_add(8 * w as u64);
                    WheelEffect {
                        dust: r.dust.as_ref().map(|d| ParticleState::new(d, seed)),
                        grass: r
                            .grass
                            .iter()
                            .enumerate()
                            .map(|(i, g)| ParticleState::new(g, seed + 1 + i as u64))
                            .collect(),
                        hold: 0,
                        ground: false,
                        road: false,
                        last_distance: 0.,
                        rpm: [0.; RPM_SAMPLES],
                        sound: WheelSound {
                            ground_playing: false,
                            ground_gain: 0.,
                            spring_playing: false,
                            spring_gain: 0.,
                            spring_pitch: 1.,
                            slide_playing: false,
                            slide_gain: 0.,
                            slide_pitch: 1.,
                            slide_road: false,
                        },
                    }
                })
                .collect(),
            bursts: std::array::from_fn(|i| {
                ParticleState::emit_only(&skid.particles[i], seed.wrapping_add(32 + i as u64))
            }),
        }
    }

    /// One fixed step of every wheel, in source order. `wheel` reads one wheel input.
    #[allow(clippy::too_many_arguments)]
    pub fn advance(
        &mut self,
        dt: f32,
        input: &WheelEffectsInput,
        wheel: impl Fn(usize) -> WheelInput,
        references: &[WheelEffectsReference],
        car_gains: &EffectGains,
        skid: &SkidReference,
        sand_layers: &[u32],
        physics: &PhysicsWorld,
    ) {
        for burst in &mut self.bursts {
            burst.set_origin(input.origin);
            burst.advance(dt, input.origin, 0., physics);
        }
        let root = Vector::from_array(std::array::from_fn(|i| {
            (input.root_absolute[i] - input.origin[i]) as f32
        }));
        let velocity = input.linear_velocity;
        let speed = velocity.length();
        let own = input.chassis;
        for (w, e) in self.wheels.iter_mut().enumerate() {
            let r = &references[w];
            let wheel = wheel(w);
            let gains = r.own_gains.unwrap_or(WheelGains {
                ground: car_gains.ground,
                spring: car_gains.spring,
                spring_pitch: car_gains.spring_pitch,
            });
            if e.hold > 0 {
                e.ground = true;
                e.hold -= 1;
            } else {
                e.ground = wheel.grounded;
                e.road = false;
            }
            let pose_rotation = input.rotation * quat(r.pose.rotation);
            let position = root + input.rotation * Vector::from_array(r.pose.translation);
            let mask = r.ray_mask;
            // Same own-car rule as the wheel ray, plus the source layer mask.
            let surfaces = |_: ColliderHandle, c: &Collider| {
                let parent = c.parent();
                parent != Some(own)
                    && (c.user_data & VEHICLE_COLLIDER_FLAG == 0
                        || parent.is_some_and(|p| physics.vehicle_of(p).is_some()))
                    && c.collision_groups().memberships.bits() & mask != 0
            };
            let hit = physics
                .queries(
                    QueryFilter::default()
                        .exclude_sensors()
                        .exclude_rigid_body(own)
                        .predicate(&surfaces),
                )
                .cast_ray(
                    &Ray::new(position, pose_rotation * -Vector::Y),
                    wheel.radius + 0.05,
                    true,
                );
            // Source em.enabled: on for a sand hit only.
            let mut dust = 0.;
            if let Some((handle, _)) = hit {
                let membership = physics.rapier().colliders[handle]
                    .collision_groups()
                    .memberships
                    .bits();
                if sand_layers.iter().any(|l| membership & (1 << l) != 0) {
                    e.road = false;
                    dust = 1.;
                    if let (Some(system), Some(reference)) = (&mut e.dust, &r.dust) {
                        system.set_rates(speed * r.rate, reference.rate_over_distance);
                        // Source sets the shape Euler angles to -RB.velocity. Converted Z is mirrored.
                        system.set_shape_rotation([-velocity.x, -velocity.y, velocity.z]);
                    }
                } else {
                    e.road = true;
                }
                e.hold = GROUND_HOLD;
            }
            if let Some(system) = &mut e.dust {
                advance_system(system, dt, input, dust, physics);
            }
            let grass = if e.ground { 1. } else { 0. };
            for system in &mut e.grass {
                advance_system(system, dt, input, grass, physics);
            }
            let s = &mut e.sound;
            match ground_gain(speed, e.road, gains.ground).filter(|_| e.ground) {
                Some(gain) => {
                    s.ground_playing = true;
                    s.ground_gain = gain;
                }
                None => s.ground_playing = false,
            }
            match hit.filter(|_| e.ground) {
                Some((_, distance)) => {
                    (s.spring_pitch, s.spring_gain) = spring(
                        distance - e.last_distance,
                        &gains,
                        dt,
                        s.spring_pitch,
                        s.spring_gain,
                    );
                    s.spring_playing = true;
                    e.last_distance = distance;
                }
                None => s.spring_playing = false,
            }
            if !(e.ground && wheel.grounded) {
                s.slide_gain = 0.;
                s.slide_playing = false;
                continue;
            }
            // A wheel without a Car link has no slide sound.
            if r.own_gains.is_some() {
                s.slide_playing = false;
                continue;
            }
            let forward = pose_rotation * -Vector::Z;
            let angle = vector_angle(velocity, forward).min(vector_angle(velocity, -forward));
            // Source shift copies the oldest-but-one sample into every older slot.
            for j in 0..RPM_SAMPLES - 1 {
                e.rpm[j + 1] = e.rpm[j];
            }
            e.rpm[0] = wheel.rpm;
            let rpm = e.rpm.iter().sum::<f32>() / RPM_SAMPLES as f32;
            let wheel_kmh = rpm.abs() * wheel.radius * std::f32::consts::TAU / 60. * 3.6;
            let level = slide(angle, speed, wheel_kmh, input.car_kmh, car_gains, skid);
            if level == 0. {
                s.slide_playing = false;
                continue;
            }
            let (volume, pitch, burst) = if e.road {
                (skid.road_volume, skid.road_pitch, 0)
            } else {
                (skid.dirt_volume, skid.dirt_pitch, 1)
            };
            s.slide_playing = true;
            s.slide_road = e.road;
            s.slide_gain = level * volume;
            s.slide_pitch = lerp(pitch[0], pitch[1], level);
            if let Some((_, distance)) = hit {
                let point = position + pose_rotation * -Vector::Y * distance;
                // Unity Mathf.Sign gives 1 for 0.
                let direction = wheel.forward * if -rpm >= 0. { 1. } else { -1. };
                let system = &mut self.bursts[burst];
                let rotation = if direction == Vector::ZERO {
                    system.rotation
                } else {
                    look_rotation(direction)
                };
                system.emit(
                    (skid.smoke_rate * level) as usize,
                    std::array::from_fn(|i| input.origin[i] + f64::from(point[i])),
                    rotation,
                );
            }
        }
    }

    /// System 0 is the dust, then the grass systems in reference order.
    pub fn particles(&self, wheel: usize, system: usize) -> &[ParticleOutput] {
        let e = &self.wheels[wheel];
        match system {
            0 => e.dust.as_ref().map_or(&[][..], |d| d.particles()),
            _ => e.grass[system - 1].particles(),
        }
    }

    /// Burst 0 is `tiresmoke` on road, 1 is `tiresand` on dirt.
    pub fn burst_particles(&self, burst: usize) -> &[ParticleOutput] {
        self.bursts[burst].particles()
    }

    pub fn sound(&self, wheel: usize) -> WheelSound {
        self.wheels[wheel].sound
    }
}

fn advance_system(
    system: &mut ParticleState,
    dt: f32,
    input: &WheelEffectsInput,
    strength: f32,
    physics: &PhysicsWorld,
) {
    system.rotation = input.rotation;
    system.set_origin(input.origin);
    system.advance(dt, input.root_absolute, strength, physics);
}

fn quat(r: [f32; 4]) -> Rotation {
    Rotation::from_xyzw(r[0], r[1], r[2], r[3])
}

/// Unity `Vector3.Angle` in degrees. A zero vector gives 0.
fn vector_angle(a: Vector, b: Vector) -> f32 {
    let denominator = (a.length_squared() * b.length_squared()).sqrt();
    if denominator < 1e-15 {
        return 0.;
    }
    (a.dot(b) / denominator).clamp(-1., 1.).acos().to_degrees()
}

/// Source rolling volume. None stops the ground voice.
pub fn ground_gain(speed: f32, road: bool, gain: f32) -> Option<f32> {
    (speed > 1.).then(|| inverse_lerp(0., 5., speed) * if road { 0.5 } else { 1. } * gain)
}

/// Source spring voice: new pitch and volume from the change of the hit distance.
pub fn spring(delta: f32, gains: &WheelGains, dt: f32, pitch: f32, volume: f32) -> (f32, f32) {
    let target = (1. + delta * gains.spring_pitch).clamp(0.95, 1.05);
    let springv = delta.abs() * gains.spring * dt;
    (lerp(pitch, target, dt * 5.), lerp(volume, springv, dt * 3.))
}

/// Source slide level: the angle part plus the wheel speed difference part.
pub fn slide(
    angle: f32,
    speed: f32,
    wheel_kmh: f32,
    car_kmh: f32,
    gains: &EffectGains,
    skid: &SkidReference,
) -> f32 {
    let mut level = 0.;
    if angle > gains.slide_angle {
        level += inverse_lerp(gains.slide_angle, 90., angle)
            * inverse_lerp(0., gains.max_slide_volume_speed, speed);
    }
    let [min, max] = skid.speed_difference;
    let difference = (car_kmh - wheel_kmh).abs();
    if difference > min {
        level += inverse_lerp(min, max, difference);
    }
    level
}

#[cfg(test)]
pub mod tests {
    use super::*;
    use crate::exhaust::tests::smoke;
    use crate::reference::SourceId;

    fn source() -> SourceId {
        SourceId {
            file: "level1".into(),
            path_id: 1,
        }
    }

    pub fn skid() -> SkidReference {
        SkidReference {
            source: source(),
            road_clip: 0,
            dirt_clip: 1,
            road_volume: 0.5,
            road_pitch: [0.9, 1.1],
            dirt_volume: 0.4,
            dirt_pitch: [0.8, 1.2],
            speed_difference: [5., 25.],
            smoke_rate: 10.,
            particles: [smoke(2000, 2.), smoke(2000, 2.)],
        }
    }
}
