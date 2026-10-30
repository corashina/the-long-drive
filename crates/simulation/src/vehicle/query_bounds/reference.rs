use rapier3d::math::Pose;
use rapier3d::parry::partitioning::Bvh;
use rapier3d::prelude::{ColliderHandle, PhysicsWorld, RigidBodyHandle, SharedShape};
use std::sync::Arc;

#[derive(Default)]
pub(super) struct ScanBounds {
    pub bvh: Bvh,
    pub(super) states: Vec<Option<ColliderState>>,
    changed: Vec<(ColliderHandle, Pose)>,
}

pub(super) struct ColliderState {
    pub(super) handle: ColliderHandle,
    position: Pose,
    parent: Option<(RigidBodyHandle, Pose)>,
    enabled: bool,
    // Owning the prior shape forces shape_mut to copy before changing its geometry.
    shape: SharedShape,
}

impl ScanBounds {
    pub fn refresh(&mut self, physics: &mut PhysicsWorld) -> usize {
        // Keep body, island and collision notifications for PhysicsPipeline::step.
        physics
            .bodies
            .propagate_modified_body_positions_to_colliders(&mut physics.colliders);
        self.changed.clear();
        // Remove old generations before a new collider can reuse the same tree leaf.
        for state in &mut self.states {
            if state
                .as_ref()
                .is_some_and(|state| !physics.colliders.contains(state.handle))
            {
                self.bvh
                    .remove(state.as_ref().unwrap().handle.into_raw_parts().0);
                *state = None;
            }
        }
        for (handle, collider) in physics.colliders.iter() {
            let slot = handle.into_raw_parts().0 as usize;
            let previous = self.states.get(slot).and_then(Option::as_ref);
            let parent = collider
                .parent()
                .zip(collider.position_wrt_parent().copied());
            let enabled = collider.is_enabled()
                && parent.is_none_or(|(handle, _)| physics.bodies[handle].is_enabled());
            // A local attachment edit does not move its body. Synchronize that edit here.
            let position = if previous.is_none_or(|state| state.parent != parent) {
                parent.map_or(*collider.position(), |(handle, local)| {
                    *physics.bodies[handle].position() * local
                })
            } else {
                *collider.position()
            };
            if previous.is_none_or(|state| {
                state.handle != handle
                    || state.position != position
                    || state.parent != parent
                    || state.enabled != enabled
                    || !Arc::ptr_eq(&state.shape.0, &collider.shared_shape().0)
            }) {
                self.changed.push((handle, position));
            }
        }
        let mut updates = 0;
        for &(handle, position) in &self.changed {
            if physics.colliders[handle].position() != &position {
                physics.colliders[handle].set_position(position);
            }
            let collider = &physics.colliders[handle];
            let parent = collider
                .parent()
                .zip(collider.position_wrt_parent().copied());
            let enabled = collider.is_enabled()
                && parent.is_none_or(|(handle, _)| physics.bodies[handle].is_enabled());
            let slot = handle.into_raw_parts().0 as usize;
            if enabled {
                self.bvh.reinsert_or_update_with_change_detection(
                    collider.compute_aabb(),
                    slot as u32,
                    0.0,
                );
                updates += 1;
            } else {
                self.bvh.remove(slot as u32);
            }
            if self.states.len() <= slot {
                self.states.resize_with(slot + 1, || None);
            }
            self.states[slot] = Some(ColliderState {
                handle,
                position,
                parent,
                enabled,
                shape: collider.shared_shape().clone(),
            });
        }
        updates
    }
}
