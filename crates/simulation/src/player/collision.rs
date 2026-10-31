//! Fixed query union. Physical shapes remain separate, uniquely owned colliders.
use crate::reference::{PlayerPostureReference, PlayerReference};
use rapier3d::{
    math::{Pose, Vector},
    parry::{
        bounding_volume::{Aabb, BoundingSphere, BoundingVolume},
        mass_properties::MassProperties,
        partitioning::{Bvh, BvhBuildStrategy},
        query::{
            PointProjection, PointQuery, Ray, RayCast, RayIntersection, ShapeCastOptions,
            details::NormalConstraints,
        },
        shape::{
            Ball, Capsule, CompositeShape, CompositeShapeRef, FeatureId, Shape, ShapeType,
            TypedCompositeShape, TypedShape,
        },
    },
    pipeline::QueryPipeline,
};

#[derive(Clone)]
pub struct PlayerSweepShape {
    capsule: Capsule,
    capsule_pose: Pose,
    feet: Ball,
    feet_pose: Pose,
    feet_enabled: bool,
    bvh: Bvh,
}
impl PlayerSweepShape {
    pub fn new(reference: &PlayerReference) -> Self {
        Self::from_posture(&reference.posture)
    }
    fn from_posture(reference: &PlayerPostureReference) -> Self {
        let mut shape = Self {
            capsule: Capsule::new_y(0.0, reference.minimum_radius),
            capsule_pose: Pose::IDENTITY,
            feet: Ball::new(reference.minimum_radius),
            feet_pose: Pose::from_translation(Vector::from_array(reference.feet.center)),
            feet_enabled: true,
            bvh: Bvh::default(),
        };
        let lower = Vector::from_array(reference.prone_head.translation);
        let upper = Vector::from_array(reference.standing_head.translation);
        let radius = Vector::splat(reference.maximum_radius);
        let capsule_bounds = Aabb::new(lower.min(upper) - radius, lower.max(upper) + radius);
        shape.update(reference, 1.0);
        let feet_bounds = shape.feet.compute_aabb(&shape.feet_pose);
        shape.bvh = Bvh::from_iter(
            BvhBuildStrategy::Binned,
            [(0, capsule_bounds), (1, feet_bounds)],
        );
        shape
    }
    pub fn set_posture(&mut self, reference: &PlayerReference, value: f32) {
        self.update(&reference.posture, value);
    }
    fn update(&mut self, reference: &PlayerPostureReference, value: f32) {
        let lower = Vector::from_array(reference.prone_head.translation);
        let head = lower.lerp(
            Vector::from_array(reference.standing_head.translation),
            value,
        );
        let radius = reference.minimum_radius
            + (reference.maximum_radius - reference.minimum_radius) * value;
        // Unity clamps capsule height to at least its diameter.
        self.capsule = Capsule::new_y(((head - lower).length() * 0.5 - radius).max(0.0), radius);
        self.capsule_pose = Pose::from_translation((head + lower) * 0.5);
        self.feet.radius = radius;
        self.feet_enabled = value > reference.ctrl_value;
    }
    pub fn feet_enabled(&self) -> bool {
        self.feet_enabled
    }
    pub fn radius(&self) -> f32 {
        self.feet.radius
    }
    /// The source feet sphere rolls over an edge below its center. Prone has no feet sphere.
    /// `contact` is the world edge point that blocked a move along the horizontal `dir`.
    /// The step puts the edge 30 degrees off the sphere bottom, inside the 45 degree climb limit.
    /// It probes from the real edge, so the next step of steep stairs does not block it.
    pub fn edge_step(
        &self,
        queries: &QueryPipeline,
        pose: &Pose,
        dir: Vector,
        contact: Vector,
        offset: f32,
    ) -> Option<Vector> {
        if !self.feet_enabled {
            return None;
        }
        let feet = *pose * self.feet_pose.translation;
        let edge = contact - feet;
        if edge.y >= 0.0 || edge.dot(dir) <= 0.0 {
            return None;
        }
        let reach = self.feet.radius + offset;
        let lift = edge.y + reach * 30f32.to_radians().cos() + offset;
        let forward = (edge.dot(dir) - reach * 0.5).max(0.0);
        if lift <= 0.0 {
            return None;
        }
        let cast = |from: &Pose, along: Vector, distance: f32| {
            queries.cast_shape(
                from,
                along,
                self,
                ShapeCastOptions {
                    target_distance: offset,
                    stop_at_penetration: false,
                    max_time_of_impact: distance,
                    compute_impact_geometry_on_penetration: true,
                },
            )
        };
        if cast(pose, Vector::Y, lift).is_some() {
            return None;
        }
        let lifted = Pose::from_parts(pose.translation + Vector::Y * lift, pose.rotation);
        if cast(&lifted, dir, forward).is_some() {
            return None;
        }
        Some(Vector::Y * lift + dir * forward)
    }
    pub fn capsule(&self) -> &Capsule {
        &self.capsule
    }
    pub fn capsule_pose(&self) -> Pose {
        self.capsule_pose
    }
    pub(super) fn feet(&self) -> Ball {
        self.feet
    }
    pub(super) fn feet_pose(&self) -> Pose {
        self.feet_pose
    }
}
impl CompositeShape for PlayerSweepShape {
    fn map_part_at(
        &self,
        id: u32,
        f: &mut dyn FnMut(Option<&Pose>, &dyn Shape, Option<&dyn NormalConstraints>),
    ) {
        self.map_untyped_part_at(id, f);
    }
    fn bvh(&self) -> &Bvh {
        &self.bvh
    }
}
impl TypedCompositeShape for PlayerSweepShape {
    type PartShape = dyn Shape;
    type PartNormalConstraints = dyn NormalConstraints;
    fn map_typed_part_at<T>(
        &self,
        id: u32,
        f: impl FnMut(Option<&Pose>, &Self::PartShape, Option<&Self::PartNormalConstraints>) -> T,
    ) -> Option<T> {
        self.map_untyped_part_at(id, f)
    }
    fn map_untyped_part_at<T>(
        &self,
        id: u32,
        mut f: impl FnMut(Option<&Pose>, &dyn Shape, Option<&dyn NormalConstraints>) -> T,
    ) -> Option<T> {
        match id {
            0 => Some(f(Some(&self.capsule_pose), &self.capsule, None)),
            1 if self.feet_enabled => Some(f(Some(&self.feet_pose), &self.feet, None)),
            _ => None,
        }
    }
}
impl RayCast for PlayerSweepShape {
    fn cast_local_ray(&self, ray: &Ray, max: f32, solid: bool) -> Option<f32> {
        CompositeShapeRef(self)
            .cast_local_ray(ray, max, solid)
            .map(|hit| hit.1)
    }
    fn cast_local_ray_and_get_normal(
        &self,
        ray: &Ray,
        max: f32,
        solid: bool,
    ) -> Option<RayIntersection> {
        CompositeShapeRef(self)
            .cast_local_ray_and_get_normal(ray, max, solid)
            .map(|(id, hit)| hit.with_subshape(id))
    }
}
impl PointQuery for PlayerSweepShape {
    fn project_local_point(&self, point: Vector, solid: bool) -> PointProjection {
        CompositeShapeRef(self)
            .project_local_point(point, f32::MAX, solid)
            .map(|(id, projection)| projection.with_subshape(id))
            .unwrap_or(PointProjection::new(false, point))
    }
    fn project_local_point_and_get_feature(&self, point: Vector) -> (PointProjection, FeatureId) {
        CompositeShapeRef(self)
            .project_local_point_and_get_feature(point, f32::MAX)
            .map(|(id, projection, feature)| (projection.with_subshape(id), feature))
            .unwrap_or((PointProjection::new(false, point), FeatureId::Unknown))
    }
}
impl Shape for PlayerSweepShape {
    fn compute_local_aabb(&self) -> Aabb {
        let capsule = self.capsule.compute_aabb(&self.capsule_pose);
        if self.feet_enabled {
            capsule.merged(&self.feet.compute_aabb(&self.feet_pose))
        } else {
            capsule
        }
    }
    fn compute_local_bounding_sphere(&self) -> BoundingSphere {
        self.compute_local_aabb().bounding_sphere()
    }
    fn clone_dyn(&self) -> Box<dyn Shape> {
        Box::new(self.clone())
    }
    fn scale_dyn(&self, _: Vector, _: u32) -> Option<Box<dyn Shape>> {
        None
    }
    fn mass_properties(&self, _: f32) -> MassProperties {
        MassProperties::default()
    }
    fn shape_type(&self) -> ShapeType {
        ShapeType::Custom
    }
    fn as_typed_shape(&self) -> TypedShape<'_> {
        TypedShape::Custom(self)
    }
    fn ccd_thickness(&self) -> f32 {
        self.radius()
    }
    fn ccd_angular_thickness(&self) -> f32 {
        std::f32::consts::PI
    }
    fn as_composite_shape(&self) -> Option<&(dyn CompositeShape + Sync)> {
        Some(self)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rapier3d::parry::shape::{Ball, Capsule};
    use rapier3d::{control::KinematicCharacterController, prelude::*};

    /// Walks the standing player 3 s toward -z up 5 steps. Returns the final pose.
    fn walk_up_stairs(rise: f32, tread: f32) -> Pose {
        let (radius, feet_y, head_y) = (0.3, -0.573, 0.65);
        let feet_pose = Pose::from_translation(Vector::new(0.0, feet_y, 0.0));
        let feet = Ball::new(radius);
        let capsule = Capsule::new_y((head_y - feet_y) * 0.5 - radius, radius);
        let capsule_pose = Pose::from_translation(Vector::new(0.0, (head_y + feet_y) * 0.5, 0.0));
        let bvh = Bvh::from_iter(
            BvhBuildStrategy::Binned,
            [
                (0, capsule.compute_aabb(&capsule_pose)),
                (1, feet.compute_aabb(&feet_pose)),
            ],
        );
        let shape = PlayerSweepShape {
            capsule,
            capsule_pose,
            feet,
            feet_pose,
            feet_enabled: true,
            bvh,
        };
        let mut bodies = RigidBodySet::new();
        let mut colliders = ColliderSet::new();
        let ground = bodies.insert(RigidBodyBuilder::fixed());
        colliders.insert_with_parent(
            ColliderBuilder::cuboid(10.0, 0.5, 10.0).translation(Vector::new(0.0, -0.5, 0.0)),
            ground,
            &mut bodies,
        );
        // Each step runs from its riser to a landing at z = -10.
        for step in 1..=5 {
            let (height, riser) = (rise * step as f32, -1.0 - tread * (step - 1) as f32);
            let depth = (10.0 + riser) * 0.5;
            colliders.insert_with_parent(
                ColliderBuilder::cuboid(1.0, height * 0.5, depth).translation(Vector::new(
                    0.0,
                    height * 0.5,
                    riser - depth,
                )),
                ground,
                &mut bodies,
            );
        }
        let mut broad_phase = BroadPhaseBvh::new();
        PhysicsPipeline::new().step(
            Vector::ZERO,
            &IntegrationParameters::default(),
            &mut IslandManager::new(),
            &mut broad_phase,
            &mut NarrowPhase::new(),
            &mut bodies,
            &mut colliders,
            &mut ImpulseJointSet::new(),
            &mut MultibodyJointSet::new(),
            &mut SoftBodySet::new(),
            &mut CCDSolver::new(),
            &(),
            &(),
        );
        let queries = broad_phase.as_query_pipeline(
            &crate::player::normals::CharacterQueryDispatcher,
            &bodies,
            &colliders,
            QueryFilter::default(),
        );
        let controller = KinematicCharacterController::default();
        let (dt, speed, offset) = (1.0 / 50.0, 1.5, 0.01);
        let dir = -Vector::Z;
        let mut pose = Pose::from_translation(Vector::new(0.0, radius - feet_y, 0.0));
        let mut vertical = 0.0;
        let mut contacts = vec![];
        for _ in 0..150 {
            vertical -= 9.81 * dt;
            contacts.clear();
            let desired = Vector::new(0.0, vertical * dt, 0.0) + dir * speed * dt;
            let mut movement =
                controller.move_shape(dt, &queries, &shape, &pose, desired, |collision| {
                    contacts.push(collision.hit.witness1)
                });
            if movement.translation.dot(dir) < 0.5 * speed * dt {
                let moved = Pose::from_translation(pose.translation + movement.translation);
                if let Some(step) = contacts
                    .iter()
                    .find_map(|&contact| shape.edge_step(&queries, &moved, dir, contact, offset))
                {
                    movement.translation += step;
                }
            }
            pose.translation += movement.translation;
            if movement.grounded {
                vertical = 0.0;
            }
        }
        pose
    }

    /// The standing player walks up stairs without a jump. Steep pit stairs included.
    #[test]
    fn standing_player_walks_up_stairs() {
        for (rise, tread) in [(0.18, 0.28), (0.2, 0.22), (0.25, 0.2), (0.3, 0.25)] {
            let pose = walk_up_stairs(rise, tread);
            assert!(
                pose.translation.z < -1.0 - tread * 4.0,
                "{rise} {tread} {pose:?}"
            );
            assert!(
                pose.translation.y - 0.573 - 0.3 > rise * 5.0 - 0.05,
                "{rise} {tread} {pose:?}"
            );
        }
    }
}
