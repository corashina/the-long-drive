use bevy_platform::collections::{HashMap, HashSet};
use rapier3d::math::{Pose, Vector};
use rapier3d::prelude::{RigidBody, RigidBodyHandle, RigidBodySet, RigidBodyType};

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
    /// Pose before and after the last step of each body that the step moved.
    steps: HashMap<RigidBodyHandle, (Pose, Pose)>,
    /// Bodies that follow the player after the step. They blend while asleep or disabled too.
    followers: HashSet<RigidBodyHandle>,
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

    /// Keeps the pose of each active body before the step.
    pub(super) fn begin_step(
        &mut self,
        bodies: &RigidBodySet,
        active: impl Iterator<Item = RigidBodyHandle>,
    ) {
        self.steps.clear();
        self.followers.clear();
        for handle in active {
            let pose = *bodies[handle].position();
            self.steps.insert(handle, (pose, pose));
        }
    }

    /// Keeps the pose of each body after the step.
    pub(super) fn end_step(&mut self, bodies: &RigidBodySet) {
        for (handle, (_, after)) in &mut self.steps {
            if let Some(body) = bodies.get(*handle) {
                *after = *body.position();
            }
        }
    }

    /// Keeps `after` as the step result of `handle`. The step start stays the first pose of this step.
    pub(super) fn follow(&mut self, handle: RigidBodyHandle, before: Pose, after: Pose) {
        self.steps.entry(handle).or_insert((before, before)).1 = after;
        self.followers.insert(handle);
    }

    pub(super) fn rebase(&mut self, shift: Vector) {
        for (before, after) in self.steps.values_mut() {
            before.translation -= shift;
            after.translation -= shift;
        }
    }

    /// The pose of `body` at `alpha` between the last two steps.
    /// A sleeping body, or a body moved after the step, shows its current pose.
    fn presented(&self, handle: RigidBodyHandle, body: &RigidBody, alpha: f32) -> Pose {
        let current = *body.position();
        match self.steps.get(&handle) {
            Some((before, after))
                if *after == current
                    && (!body.is_sleeping() || self.followers.contains(&handle)) =>
            {
                Pose::from_parts(
                    before.translation.lerp(current.translation, alpha),
                    before.rotation.slerp(current.rotation, alpha),
                )
            }
            _ => current,
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
        self.handles.capacity() * std::mem::size_of::<RigidBodyHandle>()
            + table
            + self.steps.capacity() * (std::mem::size_of::<(RigidBodyHandle, (Pose, Pose))>() + 1)
            + self.followers.capacity() * (std::mem::size_of::<RigidBodyHandle>() + 1)
    }
}

impl super::PhysicsWorld {
    /// Includes direct edits, every step's active bodies, and bodies active or following without a new step.
    pub fn presentation_bodies(&self) -> impl Iterator<Item = RigidBodyHandle> + '_ {
        let queued = &self.presentation_bodies.queued;
        self.presentation_bodies.handles.iter().copied().chain(
            self.physics
                .islands
                .active_bodies()
                .chain(self.presentation_bodies.followers.iter().copied())
                .filter(|handle| !queued.contains(handle)),
        )
    }

    /// Edits `handle` after the step as a move with the player.
    /// The presented pose blends from the step start to the new pose. A direct edit shows at once.
    pub fn follow_body<R>(
        &mut self,
        handle: RigidBodyHandle,
        edit: impl FnOnce(&mut rapier3d::prelude::RigidBody) -> R,
    ) -> R {
        let before = *self.physics.bodies[handle].position();
        let result = self.edit_body(handle, edit);
        let after = *self.physics.bodies[handle].position();
        self.presentation_bodies.follow(handle, before, after);
        result
    }

    /// The pose of `body` at `alpha` between the last two steps, as the seat camera sees it.
    pub fn presented_pose(&self, handle: RigidBodyHandle, alpha: f32) -> Pose {
        self.presentation_bodies
            .presented(handle, &self.physics.bodies[handle], alpha)
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
mod tests {
    use crate::vehicle::PhysicsWorld;
    use rapier3d::math::Vector;
    use rapier3d::prelude::RigidBodyBuilder;

    #[test]
    fn a_moving_body_shows_between_the_last_two_steps() {
        let mut physics = PhysicsWorld::empty_for_tests();
        let body = physics.insert_body(
            RigidBodyBuilder::dynamic()
                .linvel(Vector::new(10., 0., 0.))
                .additional_mass(1.),
        );
        // The first step puts the new body in an island.
        physics.step_physics();
        let before = physics.rapier().bodies[body].translation().x;
        physics.step_physics();
        let x = physics.rapier().bodies[body].translation().x;
        assert!(x > before);
        let half = physics.presented_pose(body, 0.5).translation.x;
        assert!(
            (half - (before + x) * 0.5).abs() < 1e-5,
            "{half} {before} {x}"
        );
        assert_eq!(physics.presented_pose(body, 1.).translation.x, x);
        // A body moved after the step shows its new pose at once.
        physics.edit_body(body, |b| b.set_translation(Vector::new(5., 0., 0.), true));
        assert_eq!(physics.presented_pose(body, 0.5).translation.x, 5.);
    }

    #[test]
    fn a_body_that_follows_after_the_step_shows_between_the_steps() {
        let mut physics = PhysicsWorld::empty_for_tests();
        let body = physics.insert_body(RigidBodyBuilder::kinematic_position_based());
        for x in [1., 2.] {
            physics.step_physics();
            physics.follow_body(body, |b| b.set_translation(Vector::new(x, 0., 0.), false));
        }
        assert_eq!(physics.presented_pose(body, 0.5).translation.x, 1.5);
        assert!(physics.presentation_bodies().any(|h| h == body));
        physics.clear_presentation_bodies();
        // A frame without a step still selects the follower.
        assert!(physics.presentation_bodies().any(|h| h == body));
    }
}
