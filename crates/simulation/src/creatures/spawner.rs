//! Source `aispawn` on the Player: `SpawnRoutine`, `TrySpawn`, `RandomPlace` and `CanSpawn`.
use super::senses::angle;
use crate::reference::SpawnerReference;
use crate::vehicle::PhysicsWorld;
use crate::weather::StormRandom;
use rapier3d::math::{Pose, Rotation, Vector};
use rapier3d::prelude::{Ball, Collider, QueryFilter, Ray};

/// `CanSpawn` casts from this height above the place.
const CAST_HEIGHT: f32 = 10_000.;
/// `TrySpawn` tries a place this many more times.
const RETRIES: usize = 10;

/// The player transform of `aispawn`.
#[derive(Clone, Copy, Debug)]
pub struct Player {
    pub position: Vector,
    pub rotation: Rotation,
}

impl Player {
    fn forward(&self) -> Vector {
        self.rotation * -Vector::Z
    }
}

/// Source `RandomPlace`. `onUnitSphere` projected on the ground plane is shorter than 1.
pub(crate) fn random_place(
    r: &SpawnerReference,
    player: &Player,
    random: &mut StormRandom,
) -> Vector {
    let [x, _, z] = random.on_unit_sphere();
    let distance = random.range(r.distance_range[0], r.distance_range[1]);
    player.position
        + player.rotation.inverse() * Vector::from_array(r.offset)
        + Vector::new(x, 0., z) * distance
}

/// Source cone rule: `Angle(forward, place - forward) > halfFov`.
pub fn outside_cone(r: &SpawnerReference, player: &Player, place: Vector) -> bool {
    let forward = player.forward();
    angle(forward, place - forward) > r.half_fov
}

/// Source `CanSpawn`: the ground point under `place`, off the ground by the radius, outside the
/// cone, with a free sphere.
pub fn can_spawn(
    r: &SpawnerReference,
    player: &Player,
    physics: &PhysicsWorld,
    place: Vector,
) -> Option<Vector> {
    let surface = |_, c: &Collider| {
        c.is_enabled()
            && !c.is_sensor()
            && c.collision_groups().memberships.bits() & r.surface_mask != 0
    };
    let ray = Ray::new(place + Vector::Y * CAST_HEIGHT, -Vector::Y);
    let (_, hit) = physics
        .queries(QueryFilter::default().predicate(&surface))
        .cast_ray_and_get_normal(&ray, f32::MAX, true)?;
    let point = ray.point_at(hit.time_of_impact) + hit.normal * r.radius * 1.01;
    if !outside_cone(r, player, point) {
        return None;
    }
    let sphere = |_, c: &Collider| {
        c.is_enabled()
            && !c.is_sensor()
            && c.collision_groups().memberships.bits() & r.sphere_mask != 0
    };
    let free = physics
        .queries(QueryFilter::default().predicate(&sphere))
        .intersect_shape(Pose::from_translation(point), &Ball::new(r.radius))
        .next()
        .is_none();
    free.then_some(point)
}

/// Source `SpawnRoutine` timer.
#[derive(Clone, Debug, PartialEq)]
pub struct Spawner {
    /// Simulation seconds of the next `TrySpawn`. The routine starts at load.
    pub next: f64,
}

impl Spawner {
    pub fn new(now: f64) -> Self {
        Self { next: now }
    }

    pub fn due(&self, now: f64) -> bool {
        now >= self.next
    }

    /// The places of one due `TrySpawn`, or none before the time. `count` is the live AI count.
    pub(crate) fn step(
        &mut self,
        r: &SpawnerReference,
        now: f64,
        count: usize,
        player: &Player,
        physics: &PhysicsWorld,
        random: &mut StormRandom,
        mut spawn: impl FnMut(Vector, &mut StormRandom),
    ) {
        if !self.due(now) {
            return;
        }
        if count < r.max_count as usize && random.unit() <= r.chance {
            // The loop bound is drawn again each step.
            let mut i = 0.;
            while i < random.range(r.count_range[0], r.count_range[1] + 1.) {
                let place = (0..=RETRIES)
                    .find_map(|_| can_spawn(r, player, physics, random_place(r, player, random)));
                if let Some(place) = place {
                    spawn(place, random);
                }
                i += 1.;
            }
        }
        self.next = now + f64::from(random.range(r.time_range[0], r.time_range[1]));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spawner() -> SpawnerReference {
        SpawnerReference {
            source: crate::reference::SourceId {
                file: "level1".into(),
                path_id: 74613,
            },
            chance: 1.,
            time_range: [50., 100.],
            count_range: [1., 2.],
            max_count: 10,
            distance_range: [30., 100.],
            offset: [0., 0., -100.],
            radius: 2.,
            half_fov: 20.,
            surface_mask: 1,
            sphere_mask: 1,
        }
    }

    #[test]
    fn cone_rule_uses_the_source_offset_vector() {
        let r = spawner();
        let player = Player {
            position: Vector::ZERO,
            rotation: Rotation::IDENTITY,
        };
        // Straight ahead is inside the cone; behind is outside.
        assert!(!outside_cone(&r, &player, Vector::new(0., 0., -50.)));
        assert!(outside_cone(&r, &player, Vector::new(0., 0., 50.)));
        // The rule subtracts the forward vector from the place, not the player position.
        assert!(outside_cone(&r, &player, Vector::new(0., 0., -0.5)));
    }

    #[test]
    fn random_place_lies_around_the_offset_center() {
        let r = spawner();
        let player = Player {
            position: Vector::ZERO,
            rotation: Rotation::IDENTITY,
        };
        let mut random = StormRandom(7);
        for _ in 0..100 {
            let p = random_place(&r, &player, &mut random);
            let ring = (p - Vector::new(0., 0., -100.)).length();
            assert!(ring <= 100. + 1e-3 && p.y == 0.);
        }
    }
}
