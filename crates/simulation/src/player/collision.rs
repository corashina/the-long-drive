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
