//! Character shape casts with contact-manifold normals.
//!
//! Rapier takes a touching cast's normal from GJK closest points. That normal degrades against
//! large boxes and at far coordinates, so a flat floor near 2000 units can read as a wall.
use std::cell::RefCell;

use rapier3d::math::{Pose, Real, Vector};
use rapier3d::parry::query::{
    ClosestPoints, Contact, ContactManifold, DefaultQueryDispatcher, NonlinearRigidMotion,
    PersistentQueryDispatcher, QueryDispatcher, ShapeCastHit, ShapeCastOptions, ShapeDistance,
    ShapeIntersection, Unsupported, details,
};
use rapier3d::parry::shape::Shape;

/// Parry treats a cast shorter than this as starting in contact.
const TOUCHING_TIME_OF_IMPACT: Real = 1.0e-4;
/// Manifold margin beyond the cast target distance.
const MANIFOLD_MARGIN: Real = 1.0e-3;

thread_local! {
    static MANIFOLD: RefCell<ContactManifold<(), ()>> = RefCell::new(ContactManifold::new());
}

pub struct CharacterQueryDispatcher;

impl QueryDispatcher for CharacterQueryDispatcher {
    fn intersection_test(
        &self,
        pos12: &Pose,
        g1: &dyn Shape,
        g2: &dyn Shape,
    ) -> Result<ShapeIntersection, Unsupported> {
        DefaultQueryDispatcher.intersection_test(pos12, g1, g2)
    }
    fn distance(
        &self,
        pos12: &Pose,
        g1: &dyn Shape,
        g2: &dyn Shape,
    ) -> Result<ShapeDistance, Unsupported> {
        DefaultQueryDispatcher.distance(pos12, g1, g2)
    }
    fn contact(
        &self,
        pos12: &Pose,
        g1: &dyn Shape,
        g2: &dyn Shape,
        prediction: Real,
    ) -> Result<Option<Contact>, Unsupported> {
        DefaultQueryDispatcher.contact(pos12, g1, g2, prediction)
    }
    fn closest_points(
        &self,
        pos12: &Pose,
        g1: &dyn Shape,
        g2: &dyn Shape,
        max_dist: Real,
    ) -> Result<ClosestPoints, Unsupported> {
        DefaultQueryDispatcher.closest_points(pos12, g1, g2, max_dist)
    }
    fn cast_shapes_nonlinear(
        &self,
        motion1: &NonlinearRigidMotion,
        g1: &dyn Shape,
        motion2: &NonlinearRigidMotion,
        g2: &dyn Shape,
        start_time: Real,
        end_time: Real,
        stop_at_penetration: bool,
    ) -> Result<Option<ShapeCastHit>, Unsupported> {
        DefaultQueryDispatcher.cast_shapes_nonlinear(
            motion1,
            g1,
            motion2,
            g2,
            start_time,
            end_time,
            stop_at_penetration,
        )
    }
    fn cast_shapes(
        &self,
        pos12: &Pose,
        vel12: Vector,
        g1: &dyn Shape,
        g2: &dyn Shape,
        options: ShapeCastOptions,
    ) -> Result<Option<ShapeCastHit>, Unsupported> {
        // Split composites here so their parts dispatch back through this type.
        if let Some(c1) = g1.as_composite_shape() {
            return Ok(details::cast_shapes_composite_shape_shape(
                self, pos12, vel12, c1, g2, options,
            ));
        }
        if let Some(h2) = g2.as_heightfield() {
            return details::cast_shapes_shape_heightfield(self, pos12, vel12, g1, h2, options);
        }
        if let Some(c2) = g2.as_composite_shape() {
            return Ok(details::cast_shapes_shape_composite_shape(
                self, pos12, vel12, g1, c2, options,
            ));
        }
        // Keep touching hits so the separation test below uses the corrected normal.
        let Some(mut hit) = DefaultQueryDispatcher.cast_shapes(
            pos12,
            vel12,
            g1,
            g2,
            ShapeCastOptions {
                stop_at_penetration: true,
                ..options
            },
        )?
        else {
            return Ok(None);
        };
        let at_impact = Pose::from_translation(-vel12 * hit.time_of_impact) * *pos12;
        MANIFOLD.with_borrow_mut(|manifold| {
            manifold.clear();
            // Composites are split above, so the remaining pairs are convex.
            if DefaultQueryDispatcher
                .contact_manifold_convex_convex(
                    &at_impact,
                    g1,
                    g2,
                    None,
                    None,
                    options.target_distance + MANIFOLD_MARGIN,
                    manifold,
                )
                .is_ok()
                && !manifold.points.is_empty()
            {
                hit.normal1 = manifold.local_n1;
                hit.normal2 = manifold.local_n2;
            }
        });
        let touching = hit.time_of_impact < TOUCHING_TIME_OF_IMPACT;
        if touching && !options.stop_at_penetration && hit.normal1.dot(vel12) >= 0.0 {
            return Ok(None);
        }
        Ok(Some(hit))
    }
}
