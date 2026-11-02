use rapier3d::math::Pose;
use rapier3d::parry::partitioning::Bvh;
use rapier3d::prelude::{ColliderHandle, PhysicsWorld, RigidBodyHandle};

#[derive(Default)]
pub(super) struct QueryBounds {
    pub bvh: Bvh,
    #[cfg(test)]
    last_work: RefreshWork,
    states: Vec<Option<ColliderState>>,
    pending_bodies: HandleQueue<RigidBodyHandle>,
    pending_colliders: HandleQueue<ColliderHandle>,
    geometry_changed: Vec<bool>,
    removals: HandleQueue<ColliderHandle>,
    step_bodies: HandleQueue<RigidBodyHandle>,
    step_colliders: HandleQueue<ColliderHandle>,
}

#[cfg(test)]
#[derive(Default, Clone, Copy, Debug, PartialEq, Eq)]
// Counts owned refresh-loop visits and explicit AABB calls. Excludes dependency internals.
pub(super) struct RefreshWork {
    pub bodies: usize,
    pub colliders: usize,
    pub aabbs: usize,
}

#[derive(Default)]
struct HandleQueue<H> {
    handles: Vec<H>,
    markers: Vec<Option<(H, usize)>>,
}
impl<H: Copy + Eq> HandleQueue<H> {
    fn mark(&mut self, handle: H, slot: usize) -> usize {
        if self.markers.len() <= slot {
            self.markers.resize_with(slot + 1, || None);
        }
        if let Some((previous, index)) = self.markers[slot] {
            if previous == handle {
                return index;
            }
        }
        let index = self.handles.len();
        self.handles.push(handle);
        self.markers[slot] = Some((handle, index));
        index
    }
    fn clear(&mut self, slot: impl Fn(H) -> usize) {
        for &handle in &self.handles {
            let marker = &mut self.markers[slot(handle)];
            if marker.is_some_and(|(previous, _)| previous == handle) {
                *marker = None;
            }
        }
        self.handles.clear();
    }
    fn retained_bytes(&self) -> usize {
        self.handles.capacity() * std::mem::size_of::<H>()
            + self.markers.capacity() * std::mem::size_of::<Option<(H, usize)>>()
    }
}

struct ColliderState {
    handle: ColliderHandle,
    position: Pose,
    parent: Option<(RigidBodyHandle, Pose)>,
    enabled: bool,
}

impl QueryBounds {
    pub(super) fn mark_body(&mut self, body: RigidBodyHandle) {
        let slot = body.into_raw_parts().0 as usize;
        self.pending_bodies.mark(body, slot);
        self.step_bodies.mark(body, slot);
    }
    pub(super) fn mark_simulated_body(&mut self, body: RigidBodyHandle) {
        self.pending_bodies
            .mark(body, body.into_raw_parts().0 as usize);
    }
    pub(super) fn mark_collider(&mut self, collider: ColliderHandle, geometry_changed: bool) {
        self.step_colliders
            .mark(collider, collider.into_raw_parts().0 as usize);
        self.mark_pending_collider(collider, geometry_changed);
    }
    fn mark_pending_collider(&mut self, collider: ColliderHandle, geometry_changed: bool) {
        let slot = collider.into_raw_parts().0 as usize;
        let index = self.pending_colliders.mark(collider, slot);
        if self.geometry_changed.len() <= index {
            self.geometry_changed.push(geometry_changed);
        } else {
            self.geometry_changed[index] |= geometry_changed;
        }
    }
    pub(super) fn retire_collider(&mut self, collider: ColliderHandle) {
        self.removals
            .mark(collider, collider.into_raw_parts().0 as usize);
    }
    fn clear_pending(&mut self) {
        self.pending_bodies.clear(|h| h.into_raw_parts().0 as usize);
        self.pending_colliders
            .clear(|h| h.into_raw_parts().0 as usize);
        self.removals.clear(|h| h.into_raw_parts().0 as usize);
        self.geometry_changed.clear();
    }
    pub(super) fn requeue_step_mutations(&mut self) {
        for index in 0..self.step_bodies.handles.len() {
            let body = self.step_bodies.handles[index];
            if self.step_bodies.markers[body.into_raw_parts().0 as usize] == Some((body, index)) {
                self.mark_simulated_body(body);
            }
        }
        for index in 0..self.step_colliders.handles.len() {
            // Pending geometry edits retain their flag. Earlier refreshes already applied theirs.
            let collider = self.step_colliders.handles[index];
            if self.step_colliders.markers[collider.into_raw_parts().0 as usize]
                == Some((collider, index))
            {
                self.mark_pending_collider(collider, false);
            }
        }
    }
    pub(super) fn finish_step(&mut self) {
        self.step_bodies.clear(|h| h.into_raw_parts().0 as usize);
        self.step_colliders.clear(|h| h.into_raw_parts().0 as usize);
    }
    #[cfg(test)]
    pub(super) fn pending_counts(&self) -> (usize, usize, usize) {
        (
            self.pending_bodies.handles.len(),
            self.pending_colliders.handles.len(),
            self.removals.handles.len(),
        )
    }
    #[cfg(test)]
    pub(super) fn step_counts(&self) -> (usize, usize) {
        (
            self.step_bodies.handles.len(),
            self.step_colliders.handles.len(),
        )
    }

    #[cfg(test)]
    pub(super) fn last_work(&self) -> RefreshWork {
        self.last_work
    }
    #[cfg(test)]
    pub(super) fn queue_capacities(&self) -> Vec<usize> {
        [&self.pending_bodies, &self.step_bodies]
            .into_iter()
            .flat_map(|q| [q.handles.capacity(), q.markers.capacity()])
            .chain(
                [
                    &self.pending_colliders,
                    &self.step_colliders,
                    &self.removals,
                ]
                .into_iter()
                .flat_map(|q| [q.handles.capacity(), q.markers.capacity()]),
            )
            .chain([self.geometry_changed.capacity(), self.states.capacity()])
            .collect()
    }

    pub fn retained_bytes(&self) -> usize {
        self.pending_bodies.retained_bytes()
            + self.pending_colliders.retained_bytes()
            + self.removals.retained_bytes()
            + self.step_bodies.retained_bytes()
            + self.step_colliders.retained_bytes()
            + self.geometry_changed.capacity() * std::mem::size_of::<bool>()
            + self.bvh.heap_memory_size()
            + self.states.capacity() * std::mem::size_of::<Option<ColliderState>>()
    }

    pub fn refresh(&mut self, physics: &mut PhysicsWorld) -> usize {
        #[cfg(test)]
        {
            self.last_work = RefreshWork::default();
        }
        // Preserve Rapier's body, island, and collision notifications for the next step.
        physics
            .bodies
            .propagate_modified_body_positions_to_colliders(&mut physics.colliders);
        for index in 0..self.pending_bodies.handles.len() {
            #[cfg(test)]
            {
                self.last_work.bodies += 1;
            }
            if let Some(body) = physics.bodies.get(self.pending_bodies.handles[index]) {
                for &handle in body.colliders() {
                    self.mark_pending_collider(handle, false);
                }
            }
        }
        // A retired generation must not remove a replacement in the same leaf slot.
        for &handle in &self.removals.handles {
            #[cfg(test)]
            {
                self.last_work.colliders += 1;
            }
            let slot = handle.into_raw_parts().0 as usize;
            if let Some(state) = self.states.get_mut(slot) {
                if state.as_ref().is_some_and(|state| state.handle == handle) {
                    self.bvh.remove(slot as u32);
                    *state = None;
                }
            }
        }
        let mut updates = 0;
        for index in 0..self.pending_colliders.handles.len() {
            #[cfg(test)]
            {
                self.last_work.colliders += 1;
            }
            let handle = self.pending_colliders.handles[index];
            let Some(collider) = physics.colliders.get(handle) else {
                continue;
            };
            let slot = handle.into_raw_parts().0 as usize;
            let previous = self.states.get(slot).and_then(Option::as_ref);
            let parent = collider
                .parent()
                .zip(collider.position_wrt_parent().copied());
            let enabled = collider.is_enabled()
                && parent.is_none_or(|(handle, _)| physics.bodies[handle].is_enabled());
            // A local attachment edit does not move its body. Synchronize that edit here.
            let position =
                if previous.is_none_or(|state| state.handle != handle || state.parent != parent) {
                    parent.map_or(*collider.position(), |(handle, local)| {
                        *physics.bodies[handle].position() * local
                    })
                } else {
                    *collider.position()
                };
            if !self.geometry_changed[index]
                && previous.is_some_and(|state| {
                    state.handle == handle
                        && state.position == position
                        && state.parent == parent
                        && state.enabled == enabled
                })
            {
                continue;
            }
            if collider.position() != &position {
                physics.colliders[handle].set_position(position);
            }
            if enabled {
                #[cfg(test)]
                {
                    self.last_work.aabbs += 1;
                }
                self.bvh.reinsert_or_update_with_change_detection(
                    physics.colliders[handle].compute_aabb(),
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
            });
        }
        self.clear_pending();
        updates
    }
}

#[cfg(test)]
mod reference;
#[cfg(test)]
mod tests;
