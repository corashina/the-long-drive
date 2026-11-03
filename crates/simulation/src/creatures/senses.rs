//! Source `sensoryscript`: vision rays, the vision cone and hearing.
use super::stimuli::Stimulus;
use crate::reference::{SensesReference, StimulusKind, VisionRay};
use crate::simulation::dials::inverse_lerp;
use crate::vehicle::PhysicsWorld;
use crate::weather::StormRandom;
use rapier3d::math::{Pose, Vector};
use rapier3d::prelude::{Collider, QueryFilter, Ray, RigidBodyHandle};

/// One sensed `stimul`: the position, the kind and the value.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Sensed {
    pub position: Vector,
    pub kind: StimulusKind,
    pub value: f32,
}

/// `visionRay` state.
#[derive(Clone, Copy, Debug, Default)]
pub struct RayState {
    pub value: f32,
    /// `Start` adds a random spread.
    pub max_value: f32,
}

pub struct Senses {
    pub has_eyes: bool,
    pub has_ears: bool,
    pub rays: Vec<RayState>,
    pub sensed: Vec<Sensed>,
    /// Simulation seconds of the next `Scan`.
    pub next_scan: f64,
}

/// The nearest hit in `(0, max]` of a ray with `mask`. Rays that start inside a collider
/// do not hit it, as in Unity.
pub fn cast(
    physics: &PhysicsWorld,
    own: RigidBodyHandle,
    ignore: Option<RigidBodyHandle>,
    ray: Ray,
    max: f32,
    mask: u32,
    sensors: bool,
) -> Option<f32> {
    let predicate = |_, c: &Collider| {
        c.is_enabled()
            && c.collision_groups().memberships.bits() & mask != 0
            && (sensors || !c.is_sensor())
            && (ignore.is_none() || c.parent() != ignore)
    };
    let filter = QueryFilter::default()
        .exclude_rigid_body(own)
        .predicate(&predicate);
    physics
        .queries(filter)
        .intersect_ray(ray, max, true)
        .map(|(_, _, hit)| hit.time_of_impact)
        .filter(|t| *t > 0.)
        .min_by(f32::total_cmp)
}

/// Unity `Vector3.Angle`.
pub fn angle(a: Vector, b: Vector) -> f32 {
    let d = (a.length_squared() * b.length_squared()).sqrt();
    if d < 1e-15 {
        return 0.;
    }
    (a.dot(b) / d).clamp(-1., 1.).acos().to_degrees()
}

fn project_on_plane(v: Vector, normal: Vector) -> Vector {
    v - normal * v.dot(normal)
}

/// Source `sensoryscript.Start` squares these.
pub struct ScanInput<'a> {
    pub physics: &'a PhysicsWorld,
    pub own: RigidBodyHandle,
    /// Root and eye poses in local physics space.
    pub root: Vector,
    pub eye: Pose,
    pub stimuli: &'a [Stimulus],
}

impl Senses {
    pub(crate) fn new(r: &SensesReference, random: &mut StormRandom, now: f64) -> Self {
        Self {
            has_eyes: true,
            has_ears: true,
            rays: r
                .rays
                .iter()
                .map(|ray| RayState {
                    value: 0.,
                    max_value: ray.max_value + random.range(-ray.spread, ray.spread),
                })
                .collect(),
            sensed: Vec::with_capacity(8),
            next_scan: now,
        }
    }

    /// Sum of the ray values. `aiscript` steers by it.
    pub fn ray_sum(&self) -> f32 {
        self.rays.iter().map(|r| r.value).sum()
    }

    /// Source `Scan`. `OverlapSphere` finds the stimulus colliders in range.
    pub fn scan(&mut self, r: &SensesReference, input: &ScanInput) {
        self.sensed.clear();
        let radius = r.hear_distance.max(r.fov[2]);
        let hear = r.hear_distance * r.hear_distance;
        let min_hear = r.min_hear_distance * r.min_hear_distance;
        let fov_z = r.fov[2] * r.fov[2];
        let (half_x, half_y) = (r.fov[0] * 0.5, r.fov[1] * 0.5);
        let near = |s: &&Stimulus| s.position.distance(input.root) < radius + s.radius;
        if self.has_eyes {
            for (ray, state) in r.rays.iter().zip(&mut self.rays) {
                test_ray(ray, state, input);
            }
            let eye = input.eye.translation;
            let forward = input.eye.rotation * -Vector::Z;
            let up = input.eye.rotation * Vector::Y;
            let right = input.eye.rotation * Vector::X;
            for s in input.stimuli.iter().filter(near) {
                let to = s.position - eye;
                if !(to.length_squared() < fov_z
                    && s.visible
                    && angle(forward, project_on_plane(to, up)) < half_y
                    && angle(forward, project_on_plane(to, right)) < half_x)
                {
                    continue;
                }
                // The ray must hit the stimulus collider first.
                let length = to.length();
                let reach = (length - s.radius).max(0.);
                let blocked = reach > 0.
                    && cast(
                        input.physics,
                        input.own,
                        s.body,
                        Ray::new(eye, to / length),
                        reach,
                        r.vision_mask,
                        false,
                    )
                    .is_some();
                if !blocked {
                    self.sensed.push(Sensed {
                        position: s.position,
                        kind: s.kind,
                        value: s.value,
                    });
                }
            }
        }
        if !self.has_ears {
            return;
        }
        for s in input.stimuli.iter().filter(near) {
            let d = s.position.distance_squared(input.root);
            if d < hear && d > min_hear && s.sound && s.value > r.min_hear_value {
                // The source falloff uses squared distances: far sounds count more.
                self.sensed.push(Sensed {
                    position: s.position,
                    kind: s.kind,
                    value: s.value * r.hear_modifier * inverse_lerp(0., hear, d),
                });
            }
        }
    }
}

/// Source `TestRay` with the mirror ray about the eye right axis.
fn test_ray(ray: &VisionRay, state: &mut RayState, input: &ScanInput) {
    if !ray.enabled {
        state.value = 0.;
        return;
    }
    let max = state.max_value;
    let local = Vector::from_array(ray.direction);
    let cast = |direction: Vector| {
        cast(
            input.physics,
            input.own,
            None,
            Ray::new(input.eye.translation, input.eye.rotation * direction),
            ray.length,
            ray.mask,
            true,
        )
    };
    state.value = match cast(local) {
        Some(_) if ray.boolean => {
            if ray.inverted {
                0.
            } else {
                max
            }
        }
        Some(d) if ray.inverted => max * inverse_lerp(0., ray.length, d),
        Some(d) => max * inverse_lerp(ray.length, 0., d),
        None if ray.inverted => max,
        None => 0.,
    };
    if !ray.symmetry {
        return;
    }
    let mirror = Vector::new(-local.x, local.y, local.z);
    match cast(mirror) {
        Some(_) if ray.boolean => {
            if !ray.inverted {
                state.value -= max;
            }
        }
        Some(d) if ray.inverted => state.value -= max * inverse_lerp(0., ray.length, d),
        Some(d) => state.value -= max * inverse_lerp(ray.length, 0., d),
        None if ray.inverted => state.value -= max,
        None => {}
    }
}
