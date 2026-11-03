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
        let forward = edge.dot(dir) - reach * 0.5;
        // A side or overhead contact gives no forward step. A lift alone drops back next step.
        if lift <= 0.0 || forward <= 0.0 {
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
        // The edge must hold the stepped feet inside the 45 degree climb limit.
        // A wall corner has no top, so the player would drop back on the next step.
        let stepped = Pose::from_parts(lifted.translation + dir * forward, pose.rotation);
        cast(&stepped, -Vector::Y, lift)
            .filter(|(_, hit)| {
                hit.time_of_impact < lift * 0.5
                    && hit.normal1.y.abs() > std::f32::consts::FRAC_1_SQRT_2
            })
            .map(|_| Vector::Y * lift + dir * forward)
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
    use crate::vehicle::PhysicsWorld;
    use rapier3d::prelude::{ColliderBuilder, QueryFilter};

    fn test_shape() -> PlayerSweepShape {
        let clearance = (1..=10)
            .map(|i| serde_json::json!({"posture": i as f32 * 0.1, "sphere": {"source": {"file": "level1", "path_id": 1}, "center": [0.0, 0.0, 0.0], "radius": 0.15, "membership": 1, "filter": 1}}))
            .collect::<Vec<_>>();
        let posture: PlayerPostureReference = serde_json::from_value(serde_json::json!({
            "capsule_membership": 1, "capsule_filter": 1,
            "standing_head": {"translation": [0.0, 0.65, 0.0], "rotation": [0.0, 0.0, 0.0, 1.0], "scale": [1.0, 1.0, 1.0]},
            "prone_head": {"translation": [0.0, -0.685, 0.0], "rotation": [0.0, 0.0, 0.0, 1.0], "scale": [1.0, 1.0, 1.0]},
            "minimum_radius": 0.1, "maximum_radius": 0.3, "ctrl_value": 0.3, "scroll_step": 0.05, "rise_rate": 5.0,
            "feet": {"source": {"file": "level1", "path_id": 1}, "center": [0.0, -0.573, 0.0], "radius": 0.3, "membership": 1, "filter": 1},
            "clearance": clearance, "sensor_hold_steps": 3
        }))
        .unwrap();
        PlayerSweepShape::from_posture(&posture)
    }

    /// The garage door jamb next to the light switch touched the feet sphere at its side.
    /// The step lifted the player 0.29 m in one step. That is 14 m/s, over the 13.5 m/s death
    /// change.
    #[test]
    fn wall_corner_is_no_step() {
        let shape = test_shape();
        let offset = 0.01 * shape.compute_local_aabb().extents().y;
        let feet_y = -0.573;
        let floor = feet_y - 0.3 - offset;
        let mut physics = PhysicsWorld::empty_for_tests();
        physics.insert_collider(
            ColliderBuilder::cuboid(5.0, 0.1, 5.0).translation(Vector::new(0.0, floor - 0.1, 0.0)),
        );
        // A tall jamb whose face is one offset from the feet sphere side.
        let side = 0.3 + offset + 1.0e-3;
        physics.insert_collider(
            ColliderBuilder::cuboid(1.0, 1.5, 0.1).translation(Vector::new(
                0.0,
                floor + 1.5,
                side + 0.1,
            )),
        );
        // A 0.15 m step ahead, so the stairs case still climbs.
        let rise = floor + 0.15;
        let face = (0.3f32 * 0.3 - (rise - feet_y).powi(2)).sqrt() + offset;
        physics.insert_collider(
            ColliderBuilder::cuboid(0.5, 0.075, 0.5).translation(Vector::new(
                -face - 0.5,
                rise - 0.075,
                -1.0,
            )),
        );
        physics.refresh_queries();
        let queries = physics.character_queries(QueryFilter::default());
        let walk = Vector::new(-0.866, 0.0, 0.5).normalize();
        let corner = Vector::new(0.0, feet_y - 1.0e-4, side);
        assert_eq!(
            shape.edge_step(&queries, &Pose::IDENTITY, walk, corner, offset),
            None
        );
        let stairs = Pose::from_translation(Vector::new(0.0, 0.0, -1.0));
        let edge = Vector::new(-face, rise, -1.0);
        assert!(
            shape
                .edge_step(&queries, &stairs, -Vector::X, edge, offset)
                .is_some()
        );
    }

    /// Many garage walls are resting kinematic bodies. A player on the floor who touched one
    /// could not walk away from it, and stopped dead while sliding along the next wall.
    #[test]
    fn resting_kinematic_wall_lets_go() {
        use rapier3d::control::KinematicCharacterController;
        use rapier3d::prelude::RigidBodyBuilder;
        let shape = test_shape();
        let controller = KinematicCharacterController::default();
        let offset = 0.01 * shape.compute_local_aabb().extents().y;
        let floor = -0.573 - 0.3 - offset;
        let mut physics = PhysicsWorld::empty_for_tests();
        physics.insert_collider(
            ColliderBuilder::cuboid(5.0, 0.1, 5.0).translation(Vector::new(0.0, floor - 0.1, 0.0)),
        );
        // A kinematic wall behind the player, one offset from the capsule.
        physics.insert(
            RigidBodyBuilder::kinematic_position_based().translation(Vector::new(
                0.0,
                floor + 1.5,
                -(0.3 + offset + 0.1),
            )),
            ColliderBuilder::cuboid(2.0, 1.5, 0.1),
        );
        physics.refresh_queries();
        let queries = physics.character_queries(QueryFilter::default());
        // Walk away from the wall with gravity, so the floor hit runs the kinematic body pass.
        let desired = Vector::new(0.0, -0.004, 0.035);
        let movement =
            controller.move_shape(0.02, &queries, &shape, &Pose::IDENTITY, desired, |_| {});
        assert!(movement.translation.z > 0.9 * desired.z, "{movement:?}");
    }
}
