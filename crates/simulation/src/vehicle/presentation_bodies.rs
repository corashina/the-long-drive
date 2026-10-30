use bevy_platform::collections::HashSet;
use rapier3d::math::Pose;
use rapier3d::prelude::{RigidBody, RigidBodyHandle, RigidBodyType};

#[derive(Clone, Copy, PartialEq)]
pub(super) struct BodyPresentationState {
    position: Pose,
    next_position: Pose,
    enabled: bool,
    body_type: RigidBodyType,
    sleeping: bool,
}

impl BodyPresentationState {
    pub(super) fn capture(body: &RigidBody) -> Self {
        Self {
            position: *body.position(),
            next_position: *body.next_position(),
            enabled: body.is_enabled(),
            body_type: body.body_type(),
            sleeping: body.is_sleeping(),
        }
    }
}

#[derive(Default)]
pub(super) struct PresentationBodies {
    handles: Vec<RigidBodyHandle>,
    queued: HashSet<RigidBodyHandle>,
}

impl PresentationBodies {
    pub(super) fn mark(&mut self, handle: RigidBodyHandle) {
        if self.queued.insert(handle) {
            self.handles.push(handle);
        }
    }

    pub(super) fn mark_if_changed(
        &mut self,
        handle: RigidBodyHandle,
        before: BodyPresentationState,
        body: &RigidBody,
    ) {
        if before != BodyPresentationState::capture(body) {
            self.mark(handle);
        }
    }

    fn clear(&mut self) {
        self.handles.clear();
        self.queued.clear();
    }

    fn retained_bytes(&self) -> usize {
        let table = if self.queued.capacity() == 0 {
            0
        } else {
            self.queued.capacity().next_power_of_two()
                * (std::mem::size_of::<RigidBodyHandle>() + 1)
                + 16
        };
        self.handles.capacity() * std::mem::size_of::<RigidBodyHandle>() + table
    }
}

impl super::PhysicsWorld {
    /// Includes direct edits, every step's active bodies, and bodies active without a new step.
    pub fn presentation_bodies(&self) -> impl Iterator<Item = RigidBodyHandle> + '_ {
        self.presentation_bodies.handles.iter().copied().chain(
            self.physics
                .islands
                .active_bodies()
                .filter(|handle| !self.presentation_bodies.queued.contains(handle)),
        )
    }

    /// Clear after presentation sync. Query refreshes use separate queues.
    pub fn clear_presentation_bodies(&mut self) {
        self.presentation_bodies.clear();
    }

    pub fn presentation_retained_bytes(&self) -> usize {
        self.presentation_bodies.retained_bytes()
    }
}

#[cfg(test)]
mod tests;
