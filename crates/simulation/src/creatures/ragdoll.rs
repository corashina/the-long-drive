//! `ragdollactivatescript.SetRagdol(true)`: dynamic bones with gravity, joints and pickups.
//! `InitColliders` ignores contacts between the bones of one rabbit.
use super::rig::RigPose;
use crate::player::ragdoll::joint;
use crate::reference::{ColliderId, ItemId, RabbitReference, ReferenceError};
use crate::vehicle::{PhysicsWorld, collider_builder, step_surface_bits, tag_bits};
use rapier3d::math::{Pose, Vector};
use rapier3d::prelude::{
    ColliderHandle, Group, InteractionGroups, InteractionTestMode, RigidBodyBuilder,
    RigidBodyHandle,
};

/// One bone body, its item and its colliders: handle, id and membership.
pub struct Bone {
    pub body: RigidBodyHandle,
    pub item: ItemId,
    pub colliders: Vec<(ColliderHandle, ColliderId, u32)>,
}

/// World pose of each bone body when the prefab root is at `root` with the sampled `pose`.
pub fn poses(r: &RabbitReference, nodes: &[usize], root: Pose, pose: &RigPose) -> Vec<Pose> {
    debug_assert_eq!(nodes.len(), r.ragdoll.bodies.len());
    nodes
        .iter()
        .map(|i| {
            let f = pose.model[*i];
            root * Pose::from_parts(f.position, f.rotation)
        })
        .collect()
}

/// Inserts the bones at `poses`. `SetMass` gives each bone `mass`. Unity scales the
/// colliders and the joint anchors with the root scale.
#[allow(clippy::too_many_arguments)]
pub fn spawn(
    r: &RabbitReference,
    poses: &[Pose],
    scale: f32,
    mass: f32,
    velocity: Vector,
    items: &[ItemId],
    collider_id: &mut dyn FnMut() -> Result<ColliderId, ReferenceError>,
    physics: &mut PhysicsWorld,
) -> Result<Vec<Bone>, ReferenceError> {
    let rag = &r.ragdoll;
    let mut bones = Vec::with_capacity(rag.bodies.len());
    for ((b, pose), item) in rag.bodies.iter().zip(poses).zip(items) {
        let body = physics.insert_body(
            RigidBodyBuilder::dynamic()
                .pose(*pose)
                .ccd_enabled(true)
                .additional_mass(mass)
                .linear_damping(b.linear_damping)
                .angular_damping(b.angular_damping)
                .linvel(velocity),
        );
        bones.push(Bone {
            body,
            item: *item,
            colliders: Vec::new(),
        });
    }
    for c in &rag.colliders {
        let bone = &mut bones[c.body.0 as usize - 1];
        let id = collider_id()?;
        let geometry = crate::world::scaled_geometry(&c.geometry, [scale; 3]);
        let collider = collider_builder(&geometry)?
            .friction(c.friction)
            .restitution(c.restitution)
            .sensor(c.sensor)
            .density(0.)
            .collision_groups(InteractionGroups::new(
                Group::from_bits_truncate(c.membership),
                Group::from_bits_truncate(c.filter),
                InteractionTestMode::And,
            ))
            .user_data(
                ((id.0 as u128) << 64)
                    | step_surface_bits(c.step_surface)
                    | tag_bits(c.tag)
                    | (bone.item.0 as u128 + 1),
            )
            .build();
        let handle = physics.insert_collider_with_parent(collider, bone.body);
        bone.colliders.push((handle, id, c.membership));
    }
    for j in &rag.joints {
        physics.insert_impulse_joint(
            bones[j.connected_body.0 as usize - 1].body,
            bones[j.body.0 as usize - 1].body,
            joint(&rag.bodies, j, scale),
            true,
        );
    }
    let bodies: Vec<_> = bones.iter().map(|b| b.body).collect();
    physics.ignore_collisions_between(&bodies);
    physics.refresh_queries();
    Ok(bones)
}
