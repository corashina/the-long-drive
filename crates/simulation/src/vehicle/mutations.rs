use super::{PhysicsWorld, presentation_bodies::BodyPresentationState};
use rapier3d::control::{CharacterCollision, KinematicCharacterController};
#[cfg(test)]
use rapier3d::math::Vector;
use rapier3d::parry::shape::Shape;
use rapier3d::prelude::{
    ActiveHooks, Collider, ColliderHandle, ColliderSet, GenericJoint, ImpulseJoint,
    ImpulseJointHandle, PairFilterContext, PhysicsHooks, QueryFilter, QueryPipelineMut, RigidBody,
    RigidBodyHandle, SolverFlags,
};
use std::collections::HashMap;

/// Bodies of one group do not collide with one another (source `Physics.IgnoreCollision`).
/// Each body maps to its group key.
#[derive(Default)]
pub(super) struct IgnoredCollisions(HashMap<RigidBodyHandle, RigidBodyHandle>);
impl PhysicsHooks for IgnoredCollisions {
    fn filter_contact_pair(&self, context: &PairFilterContext) -> Option<SolverFlags> {
        let group = |body: Option<RigidBodyHandle>| body.and_then(|body| self.0.get(&body));
        match (group(context.rigid_body1), group(context.rigid_body2)) {
            (Some(a), Some(b)) if a == b => None,
            _ => Some(SolverFlags::default()),
        }
    }
}

impl PhysicsWorld {
    pub fn edit_body<R>(
        &mut self,
        handle: RigidBodyHandle,
        edit: impl FnOnce(&mut RigidBody) -> R,
    ) -> R {
        let body = self
            .physics
            .bodies
            .get_mut(handle)
            .expect("Invalid rigid-body handle");
        self.query_bounds.mark_body(handle);
        let before = BodyPresentationState::capture(body);
        let result = edit(body);
        self.presentation_bodies
            .mark_if_changed(handle, before, body);
        result
    }
    pub fn try_edit_body<R>(
        &mut self,
        handle: RigidBodyHandle,
        edit: impl FnOnce(&mut RigidBody) -> R,
    ) -> Option<R> {
        let body = self.physics.bodies.get_mut(handle)?;
        self.query_bounds.mark_body(handle);
        let before = BodyPresentationState::capture(body);
        let result = edit(body);
        self.presentation_bodies
            .mark_if_changed(handle, before, body);
        Some(result)
    }
    pub fn edit_collider<R>(
        &mut self,
        handle: ColliderHandle,
        edit: impl FnOnce(&mut Collider) -> R,
    ) -> R {
        let collider = self
            .physics
            .colliders
            .get_mut(handle)
            .expect("Invalid collider handle");
        self.query_bounds.mark_collider(handle, true);
        edit(collider)
    }
    pub fn try_edit_collider<R>(
        &mut self,
        handle: ColliderHandle,
        edit: impl FnOnce(&mut Collider) -> R,
    ) -> Option<R> {
        let collider = self.physics.colliders.get_mut(handle)?;
        self.query_bounds.mark_collider(handle, true);
        Some(edit(collider))
    }
    pub fn insert_body(&mut self, body: impl Into<RigidBody>) -> RigidBodyHandle {
        let mut body = body.into();
        // A negative threshold is a body that never sleeps.
        if body.activation().normalized_linear_threshold >= 0. {
            body.activation_mut().normalized_linear_threshold = self.sleep_speed;
        }
        let handle = self.physics.insert_body(body);
        self.query_bounds.mark_body(handle);
        self.presentation_bodies.mark(handle);
        handle
    }
    pub fn insert(
        &mut self,
        body: impl Into<RigidBody>,
        collider: impl Into<Collider>,
    ) -> (RigidBodyHandle, ColliderHandle) {
        let body = self.insert_body(body);
        let collider = self.insert_collider_with_parent(collider, body);
        (body, collider)
    }
    pub fn insert_collider(&mut self, collider: impl Into<Collider>) -> ColliderHandle {
        let handle = self.physics.colliders.insert(collider);
        self.query_bounds.mark_collider(handle, true);
        handle
    }
    pub fn insert_collider_with_parent(
        &mut self,
        collider: impl Into<Collider>,
        parent: RigidBodyHandle,
    ) -> ColliderHandle {
        let handle =
            self.physics
                .colliders
                .insert_with_parent(collider, parent, &mut self.physics.bodies);
        self.query_bounds.mark_body(parent);
        self.query_bounds.mark_collider(handle, true);
        handle
    }
    pub fn set_collider_parent(
        &mut self,
        collider: ColliderHandle,
        parent: Option<RigidBodyHandle>,
    ) {
        if let Some(state) = self.physics.colliders.get(collider) {
            if let Some(old) = state
                .parent()
                .filter(|parent| self.physics.bodies.contains(*parent))
            {
                self.query_bounds.mark_body(old);
            }
            if let Some(parent) = parent.filter(|parent| self.physics.bodies.contains(*parent)) {
                self.query_bounds.mark_body(parent);
            }
            self.query_bounds.mark_collider(collider, false);
        }
        self.physics
            .colliders
            .set_parent(collider, parent, &mut self.physics.bodies);
    }
    /// The colliders of these bodies stop colliding with one another.
    pub fn ignore_collisions_between(&mut self, bodies: &[RigidBodyHandle]) {
        let Some(&group) = bodies.first() else {
            return;
        };
        for &body in bodies {
            self.ignored.0.insert(body, group);
            for &collider in self.physics.bodies[body].colliders() {
                let collider = &mut self.physics.colliders[collider];
                collider
                    .set_active_hooks(collider.active_hooks() | ActiveHooks::FILTER_CONTACT_PAIRS);
            }
        }
    }
    pub fn remove_body(&mut self, body: RigidBodyHandle) -> Option<RigidBody> {
        self.ignored.0.remove(&body);
        let state = self.physics.bodies.get(body)?;
        for &collider in state.colliders() {
            self.query_bounds.retire_collider(collider);
        }
        self.query_bounds.mark_body(body);
        self.presentation_bodies.mark(body);
        if self
            .body_mass_override
            .as_ref()
            .is_some_and(|state| state.body == body)
        {
            self.body_mass_override = None;
        }
        self.forget_body_cargo(body);
        self.physics.remove_body(body)
    }
    pub fn recompute_body_mass(&mut self, body: RigidBodyHandle) {
        let state = self
            .physics
            .bodies
            .get_mut(body)
            .expect("Invalid rigid-body handle");
        self.query_bounds.mark_body(body);
        state.recompute_mass_properties_from_colliders(&self.physics.colliders);
    }
    pub fn edit_bodies(
        &mut self,
        mut edit: impl FnMut(RigidBodyHandle, &mut RigidBody, &ColliderSet),
    ) {
        for (handle, body) in self.physics.bodies.iter_mut() {
            self.query_bounds.mark_body(handle);
            let before = BodyPresentationState::capture(body);
            edit(handle, body, &self.physics.colliders);
            self.presentation_bodies
                .mark_if_changed(handle, before, body);
        }
    }
    pub fn insert_impulse_joint(
        &mut self,
        first: RigidBodyHandle,
        second: RigidBodyHandle,
        joint: impl Into<GenericJoint>,
        wake_up: bool,
    ) -> ImpulseJointHandle {
        for body in [first, second] {
            if self.physics.bodies.contains(body) {
                self.query_bounds.mark_body(body);
            }
        }
        self.physics
            .impulse_joints
            .insert(first, second, joint, wake_up)
    }
    pub fn remove_impulse_joint(
        &mut self,
        joint: ImpulseJointHandle,
        wake_up: bool,
    ) -> Option<ImpulseJoint> {
        self.physics.impulse_joints.remove(joint, wake_up)
    }
    #[cfg(test)]
    pub(super) fn set_gravity(&mut self, gravity: Vector) {
        self.physics.gravity = gravity;
    }
    pub fn solve_character_collision_impulses(
        &mut self,
        controller: &KinematicCharacterController,
        dt: f32,
        filter: QueryFilter<'_>,
        shape: &dyn Shape,
        mass: f32,
        collisions: &[CharacterCollision],
    ) {
        controller.solve_character_collision_impulses(
            dt,
            &mut QueryPipelineMut {
                dispatcher: &crate::player::normals::CharacterQueryDispatcher,
                bvh: &self.query_bounds.bvh,
                bodies: &mut self.physics.bodies,
                colliders: &mut self.physics.colliders,
                filter,
            },
            shape,
            mass,
            collisions,
        );
    }
}
