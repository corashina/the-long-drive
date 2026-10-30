//! Source PhysX pushes the player body out of overlaps, such as a door closed on it.
//! The push speed is capped by Unity `Physics.defaultMaxDepenetrationVelocity`.
use rapier3d::math::{Pose, Vector};
use rapier3d::parry::bounding_volume::BoundingVolume;
use rapier3d::parry::shape::Shape;
use rapier3d::pipeline::QueryPipeline;

/// Passes per step, so a push out of one collider does not leave the shape in another.
const PASSES: usize = 4;

/// Translation that moves `shape` out of overlaps. Its length is at most `budget`.
pub fn depenetrate(
    queries: &QueryPipeline,
    shape: &dyn Shape,
    pose: &Pose,
    offset: f32,
    budget: f32,
) -> Vector {
    let mut translation = Vector::ZERO;
    let mut applied = 0.0;
    for _ in 0..PASSES {
        let aabb = shape
            .compute_aabb(&(Pose::from_translation(translation) * *pose))
            .loosened(offset);
        let mut corrected = false;
        for (_, collider) in queries.intersect_aabb_conservative(aabb) {
            let current = Pose::from_translation(translation) * *pose;
            let pos12 = current.inv_mul(collider.position());
            let Ok(Some(contact)) =
                queries
                    .dispatcher
                    .contact(&pos12, shape, collider.shape(), 0.0)
            else {
                continue;
            };
            if contact.dist >= -1.0e-5 {
                continue;
            }
            let push = (offset - contact.dist).min(budget - applied);
            if push <= 0.0 {
                return translation;
            }
            // `normal1` points from the shape to the obstacle.
            translation -= (current.rotation * contact.normal1) * push;
            applied += push;
            corrected = true;
        }
        if !corrected {
            break;
        }
    }
    translation
}

#[cfg(test)]
mod tests {
    use super::*;
    use rapier3d::prelude::*;

    /// A door closed on a standing player pushes it out below the source death speed.
    #[test]
    fn door_overlap_push_is_capped() {
        let mut bodies = RigidBodySet::new();
        let mut colliders = ColliderSet::new();
        let door = bodies.insert(RigidBodyBuilder::fixed());
        colliders.insert_with_parent(ColliderBuilder::cuboid(0.45, 1.0, 0.025), door, &mut bodies);
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
            &rapier3d::parry::query::DefaultQueryDispatcher,
            &bodies,
            &colliders,
            QueryFilter::default(),
        );
        let shape = Capsule::new_y(0.6, 0.3);
        let mut pose = Pose::from_translation(Vector::new(0.0, 0.9, 0.2));
        let (dt, death_velocity_change) = (1.0 / 60.0, 13.5);
        for _ in 0..10 {
            let push = depenetrate(&queries, &shape, &pose, 0.01, 10.0 * dt);
            assert!(push.length() / dt <= 10.0 + 1.0e-3);
            assert!(push.length() / dt < death_velocity_change);
            pose.translation += push;
        }
        assert!(pose.translation.z >= 0.025 + 0.3);
    }
}
