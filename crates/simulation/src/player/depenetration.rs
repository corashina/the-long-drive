//! Source PhysX pushes the player body out of overlaps, such as a door closed on it.
//! Unity `Physics.defaultMaxDepenetrationVelocity` caps the push speed.
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
