//! Source `wheelgraphicsscript.Graphics`: it moves the `T` node to the wheel collider pose.
//! The hub and drum meshes under `T` turn with steer and roll with the wheel.
use crate::reference::{Transform, WorldPrototypeReference};
use crate::vehicle::{WheelState, to_rapier};
use crate::world::placement::node_frame;
use rapier3d::math::{Pose, Rotation, Vector};

/// The `T` node of one wheel and the frames that pose it.
pub struct WheelGraphic {
    /// The `T` node.
    pub node: usize,
    /// Root frame of the body, scaled.
    body_pose: Pose,
    /// Unscaled root frame of the `T` parent, inverted.
    parent_inverse: Pose,
    /// Rotation of the wheel collider node in the body frame.
    collider_rotation: Rotation,
    scale: [f32; 3],
    /// Rest scale of `T`.
    rest_scale: [f32; 3],
}

impl WheelGraphic {
    /// `body_pose`: the scaled root frame of the wheel body. `scale`: the placement scale.
    pub fn new(
        prototype: &WorldPrototypeReference,
        node: usize,
        collider_node: usize,
        body_pose: Pose,
        scale: [f32; 3],
    ) -> Self {
        let frame = |i: usize| to_rapier(node_frame(prototype, 0, i));
        let parent = prototype.nodes[node].parent.unwrap_or(0) as usize;
        Self {
            node,
            body_pose,
            parent_inverse: frame(parent).inverse(),
            collider_rotation: body_pose.rotation.inverse() * frame(collider_node).rotation,
            scale,
            rest_scale: prototype.nodes[node].transform.scale,
        }
    }

    /// WheelCollider.GetWorldPose in the parent frame of `T`: the wheel center below its
    /// connection, turned by steer, then by the roll angle. Also gives the `T` position in the
    /// unscaled root frame.
    pub fn pose(&self, wheel: &WheelState) -> (Transform, Vector) {
        let center = wheel.connection - Vector::Y * wheel.suspension_length;
        let rotation = self.collider_rotation
            * Rotation::from_rotation_y(wheel.steering)
            * Rotation::from_rotation_x(-wheel.angle);
        let root = self.body_pose * Pose::from_parts(center, rotation);
        // Prototype node frames are unscaled.
        let unscaled =
            Vector::from_array(std::array::from_fn(|i| root.translation[i] / self.scale[i]));
        let local = self.parent_inverse * Pose::from_parts(unscaled, root.rotation);
        let pose = Transform {
            translation: local.translation.to_array(),
            rotation: local.rotation.to_array(),
            scale: self.rest_scale,
        };
        (pose, unscaled)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::vehicle::{WheelMount, WheelSet, car_prototype_fixture, collider_profile};

    /// The hub node under `T` turns with steer, so the steered tire stays on its hub.
    #[test]
    fn hub_follows_the_steered_wheel() {
        let prototype = car_prototype_fixture();
        let wheel = &prototype.vehicle.as_ref().unwrap().wheels[0];
        let connection = Vector::new(-0.7, 0.2, -1.2);
        let set = WheelSet::new(
            rapier3d::prelude::RigidBodyHandle::invalid(),
            &[WheelMount {
                connection,
                collider: &wheel.collider,
                profile: collider_profile(&wheel.collider),
            }],
        );
        let mut state = set.wheels()[0];
        state.steering = 0.4;
        let g = WheelGraphic::new(
            &prototype,
            wheel.graphic as usize,
            wheel.node as usize,
            Pose::IDENTITY,
            [1.; 3],
        );
        let (pose, root) = g.pose(&state);
        let center = connection - Vector::Y * state.suspension_length;
        assert!((root - center).length() < 1e-5);
        let rotation = Rotation::from_array(pose.rotation);
        assert!(rotation.angle_between(Rotation::from_rotation_y(0.4)) < 1e-5);
    }
}
