//! Source home `buildingscript.FStart`: connect the home cable to the nearest road pole.
//! Runs once per world session. No pole in range: the root stays hidden.
use super::{GarageRuntime, StartingArea};
use crate::boundary::RopeAim;
use crate::reference::{NodeId, ReferenceError, ReferenceManifest, Transform};
use crate::vehicle::PhysicsWorld;
use glam::{Affine3A, Quat, Vec3};

impl GarageRuntime {
    /// The rope aims once the target query resolves. Empty: no pole, the root stays hidden.
    pub(super) fn home_cable(&mut self, reference: &ReferenceManifest) -> Option<Vec<RopeAim>> {
        let session = self.stream.frame().request.session;
        if self.home_cable_session == Some(session)
            || !self.startup_aligned
            || !self.stream.initial_generation_ready()
        {
            return None;
        }
        let cable = &reference.world.home_cable;
        let pose = |node| body_node_transform(reference, &self.area, &self.physics, node);
        let Some(rope0) = pose(cable.ropes[0]) else {
            self.stream.fail(ReferenceError(
                "Home cable is not below a starting-area body".into(),
            ));
            self.home_cable_session = Some(session);
            return None;
        };
        let rope0 = self.stream.origin.absolute(rope0.translation.to_array());
        let (status, target) = self.stream.home_cable_target(rope0);
        if status == crate::world::QueryStatus::Pending {
            return None;
        }
        self.home_cable_session = Some(session);
        let Some(target) = target else {
            return Some(Vec::new());
        };
        let target = self.stream.origin.local(target).map(f64::from);
        let pose = |node| body_node_transform(reference, &self.area, &self.physics, node);
        let mut aims = Vec::with_capacity(cable.ropes.len());
        for &rope in &cable.ropes {
            let node = reference
                .nodes
                .iter()
                .find(|n| n.id == rope)
                .expect("validated home rope");
            let parent = node.parent.expect("home rope below its root");
            let (Some(parent_world), Some(rope_world)) = (pose(parent), pose(rope)) else {
                continue;
            };
            let (_, parent_rotation, _) = parent_world.to_scale_rotation_translation();
            let (rotation, scale_z) = crate::world::aim(
                rope_world.translation.to_array().map(f64::from),
                rapier3d::math::Rotation::from_array(parent_rotation.to_array()),
                target,
            );
            aims.push(RopeAim {
                node: rope,
                rotation,
                scale_z,
            });
        }
        Some(aims)
    }
}

/// World transform of `node` below its starting-area body. Source BodyVisual scale rule.
fn body_node_transform(
    reference: &ReferenceManifest,
    area: &StartingArea,
    physics: &PhysicsWorld,
    node: NodeId,
) -> Option<Affine3A> {
    let local = |t: &Transform| {
        Affine3A::from_scale_rotation_translation(
            Vec3::from_array(t.scale),
            Quat::from_array(t.rotation),
            Vec3::from_array(t.translation),
        )
    };
    let mut below = Affine3A::IDENTITY;
    let mut current = reference.nodes.iter().find(|n| n.id == node)?;
    let body = loop {
        if let Some(body) = reference.bodies.iter().find(|b| b.node == current.id) {
            break body;
        }
        below = local(&current.transform) * below;
        let parent = current.parent?;
        current = reference.nodes.iter().find(|n| n.id == parent)?;
    };
    // BodyVisual: the physics pose with the scale of the body node chain.
    let mut scale = Vec3::ONE;
    let mut scaled = Some(current);
    while let Some(n) = scaled {
        scale *= Vec3::from_array(n.transform.scale);
        scaled = n
            .parent
            .and_then(|p| reference.nodes.iter().find(|n| n.id == p));
    }
    let pose = physics.rapier().bodies[*area.bodies.get(&body.id)?].position();
    let world = Affine3A::from_scale_rotation_translation(
        scale,
        Quat::from_array(pose.rotation.to_array()),
        Vec3::from_array(pose.translation.to_array()),
    );
    Some(world * below)
}
