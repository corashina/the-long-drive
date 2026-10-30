//! Source metalDetectorScript: ray scan, smoothed value, beep pitch and volume.
//! Electronics jam only comes from EMP (F21), so the value is not raised by it.
use crate::reference::{ColliderTag, TorqueCurve};
use crate::simulation::dials::{inverse_lerp, lerp};
// An unweighted Hermite curve with clamped ends.
use crate::simulation::engine::torque as curve;
use crate::vehicle::{PhysicsWorld, collider_tag};
use crate::world::unity_random::UnityRandom;
use rapier3d::math::{Pose, Vector};
use rapier3d::prelude::{QueryFilter, Ray, RigidBodyHandle};

#[derive(Clone, Debug)]
pub struct DetectorRules {
    pub ray_count: u32,
    pub scan_seconds: f32,
    /// `hitsArraySize`: hits read per ray.
    pub hits: usize,
    pub min_range: f32,
    pub max_range: f32,
    pub ray_start_radius: f32,
    pub ray_end_radius: f32,
    pub ray_distance: f32,
    pub wall_max: f32,
    pub wall_min: f32,
    pub rescale: TorqueCurve,
    pub range_curve: TorqueCurve,
    pub tags: Vec<ColliderTag>,
    pub mask: u32,
    pub pitch: [f32; 2],
    pub pitch_values: [f32; 2],
    pub volume: [f32; 2],
    pub volume_values: [f32; 2],
    pub lerp_speed: f32,
}

#[derive(Clone, Debug)]
pub struct MetalDetector {
    /// `onOffButton.currentTurnState != 0`.
    pub on: bool,
    value: f32,
    /// `currentValueSmooth`: the beep and meter value.
    pub smooth: f32,
    scan_in: f32,
}
impl MetalDetector {
    pub fn new(on: bool) -> Self {
        Self {
            on,
            value: 0.,
            smooth: 0.,
            scan_in: 0.,
        }
    }
    /// usablescript Turn, then metalDetectorScript.Turn: the values restart at 0.
    pub fn turn(&mut self) {
        self.on = !self.on;
        self.value = 0.;
        self.smooth = 0.;
    }
    /// Update while on. Returns the beep pitch and volume.
    #[allow(clippy::too_many_arguments)]
    pub fn advance(
        &mut self,
        rules: &DetectorRules,
        dt: f32,
        transform: Pose,
        body: Option<RigidBodyHandle>,
        physics: &PhysicsWorld,
        random: &mut UnityRandom,
        hits: &mut Vec<(f32, u128)>,
    ) -> Option<(f32, f32)> {
        if !self.on {
            return None;
        }
        self.scan_in -= dt;
        if self.scan_in <= 0. {
            self.scan_in = rules.scan_seconds;
            self.value = scan(rules, transform, body, physics, random, hits);
        }
        self.smooth = lerp(self.smooth, self.value, dt * rules.lerp_speed);
        let pitch = lerp(
            rules.pitch[0],
            rules.pitch[1],
            inverse_lerp(rules.pitch_values[0], rules.pitch_values[1], self.smooth),
        );
        let volume = if self.smooth < rules.volume_values[0] {
            0.
        } else {
            lerp(
                rules.volume[0],
                rules.volume[1],
                inverse_lerp(rules.volume_values[0], rules.volume_values[1], self.smooth),
            )
        };
        Some((pitch, volume))
    }
}

/// Scan: rays from a disc at the detector to a wider disc ahead of it. The first tagged
/// hit of a ray adds the range curve value; other hits on the way shorten the ranges.
/// The source hit order is unsorted; this reads the nearest hits first.
fn scan(
    rules: &DetectorRules,
    transform: Pose,
    body: Option<RigidBodyHandle>,
    physics: &PhysicsWorld,
    random: &mut UnityRandom,
    hits: &mut Vec<(f32, u128)>,
) -> f32 {
    let right = transform.rotation * Vector::X;
    let up = transform.rotation * Vector::Y;
    // Source +Z is converted -Z.
    let forward = transform.rotation * -Vector::Z;
    let mut filter = QueryFilter::default().exclude_sensors();
    if let Some(body) = body {
        filter = filter.exclude_rigid_body(body);
    }
    let mask = rules.mask;
    let predicate = |_, c: &rapier3d::prelude::Collider| {
        c.is_enabled() && c.collision_groups().memberships.bits() & mask != 0
    };
    filter.predicate = Some(&predicate);
    let queries = physics.queries(filter);
    let mut total = 0.;
    for _ in 0..rules.ray_count {
        let [x, y] = random
            .inside_unit_circle()
            .map(|v| v * rules.ray_start_radius);
        let start = transform.translation + right * x + up * y;
        let [x, y] = random
            .inside_unit_circle()
            .map(|v| v * rules.ray_end_radius);
        let direction = (forward * rules.ray_distance + right * x + up * y).normalize_or_zero();
        if direction == Vector::ZERO {
            continue;
        }
        hits.clear();
        // Source rays do not hit a collider they start inside.
        hits.extend(
            queries
                .intersect_ray(Ray::new(start, direction), rules.max_range, true)
                .filter(|(_, _, hit)| hit.time_of_impact > 0.)
                .map(|(_, c, hit)| (hit.time_of_impact, c.user_data)),
        );
        hits.sort_unstable_by(|a, b| a.0.total_cmp(&b.0));
        let (mut min, mut max) = (rules.min_range, rules.max_range);
        for &(distance, data) in hits.iter().take(rules.hits) {
            if collider_tag(data).is_some_and(|t| rules.tags.contains(&t)) {
                total += curve(&rules.range_curve, inverse_lerp(min, max, distance));
                break;
            }
            // Triggers are ignored, so no hit is a partslotscript sensor.
            max *= rules.wall_max;
            min *= rules.wall_min;
        }
    }
    curve(&rules.rescale, total / rules.ray_count.max(1) as f32)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::reference::CurveKey;
    use rapier3d::prelude::ColliderBuilder;

    fn line(a: f32, b: f32) -> TorqueCurve {
        let key = |time, value| CurveKey {
            time,
            value,
            in_tangent: b - a,
            out_tangent: b - a,
            in_weight: 1. / 3.,
            out_weight: 1. / 3.,
            weighted_mode: 0,
        };
        TorqueCurve {
            keys: vec![key(0., a), key(1., b)],
            pre_wrap_mode: 2,
            post_wrap_mode: 2,
        }
    }
    fn rules() -> DetectorRules {
        DetectorRules {
            ray_count: 20,
            scan_seconds: 0.05,
            hits: 10,
            min_range: 0.,
            max_range: 10.,
            ray_start_radius: 0.1,
            ray_end_radius: 0.1,
            ray_distance: 2.5,
            wall_max: 0.5,
            wall_min: 0.5,
            rescale: line(0., 1.),
            range_curve: line(1., 0.),
            tags: vec![ColliderTag::Metal],
            mask: 1,
            pitch: [0.9, 1.],
            pitch_values: [0., 1.],
            volume: [0.1, 0.5],
            volume_values: [0.01, 1.],
            lerp_speed: 30.,
        }
    }
    fn plate(physics: &mut PhysicsWorld, z: f32, tag: Option<ColliderTag>) {
        physics.insert_collider(
            ColliderBuilder::cuboid(5., 5., 0.1)
                .translation(Vector::new(0., 0., z))
                .collision_groups(rapier3d::prelude::InteractionGroups::new(
                    rapier3d::prelude::Group::GROUP_1,
                    rapier3d::prelude::Group::ALL,
                    rapier3d::prelude::InteractionTestMode::And,
                ))
                .user_data(crate::vehicle::tag_bits(tag)),
        );
    }
    fn value(physics: &PhysicsWorld) -> f32 {
        let mut random = UnityRandom::new(1);
        scan(
            &rules(),
            Pose::IDENTITY,
            None,
            physics,
            &mut random,
            &mut Vec::new(),
        )
    }

    #[test]
    fn metal_ahead_beeps_and_a_wall_in_front_weakens_it() {
        let mut physics = PhysicsWorld::empty_for_tests();
        plate(&mut physics, -4., Some(ColliderTag::Metal));
        physics.refresh_queries();
        let open = value(&physics);
        assert!((open - 0.6).abs() < 0.02, "{open}");
        plate(&mut physics, -1., None);
        physics.refresh_queries();
        let walled = value(&physics);
        assert!(walled < open, "{walled}");
        let mut detector = MetalDetector::new(false);
        detector.turn();
        let mut random = UnityRandom::new(1);
        let (_, volume) = detector
            .advance(
                &rules(),
                0.1,
                Pose::IDENTITY,
                None,
                &physics,
                &mut random,
                &mut Vec::new(),
            )
            .unwrap();
        assert!(volume > 0.1);
        detector.turn();
        assert_eq!(detector.smooth, 0.);
    }
}
