//! Source tank cap marker motion.
use crate::reference::OpeningId;
use bevy::ecs as bevy_ecs;
use bevy::prelude::{Component, Quat, Transform, Vec3};

#[derive(Component)]
pub struct CapVisual {
    pub opening: OpeningId,
    pub closed: Transform,
    pub opened: Transform,
    pub euler: bool,
    pub closed_euler: Vec3,
    pub open_euler: Vec3,
    /// Source `capLerp` while `DoMove` runs.
    pub motion: Option<f32>,
    /// Current `TCap` pose. Car part node poses also write this node, so the cap keeps its own pose.
    pub pose: Transform,
}
impl CapVisual {
    /// Advances the cap toward the pose of `valve` and returns the new local pose.
    pub(crate) fn step(&mut self, valve: f32, dt: f32) -> Transform {
        let mut pose = self.closed;
        if self.euler {
            let angles =
                self.closed_euler.lerp(self.open_euler, valve) * std::f32::consts::PI / 180.0;
            // Unity applies Z, then X, then Y.
            pose.rotation = Quat::from_rotation_y(angles.y)
                * Quat::from_rotation_x(angles.x)
                * Quat::from_rotation_z(angles.z);
        } else {
            pose.translation = self.closed.translation.lerp(self.opened.translation, valve);
            pose.rotation = self
                .closed
                .rotation
                .lerp(self.opened.rotation, valve)
                .normalize();
            pose = cap_motion(self.pose, pose, &mut self.motion, dt);
        }
        self.pose = pose;
        pose
    }
}

/// Source `tankcapscript.DoMove`. Each frame moves `TCap` by `Lerp(current, target, capLerp)`.
/// `capLerp` starts at 0 and gains `deltaTime * 10`. At 1 the cap snaps to its target.
fn cap_motion(
    current: Transform,
    target: Transform,
    motion: &mut Option<f32>,
    dt: f32,
) -> Transform {
    let Some(lerp) = motion else {
        if current.translation != target.translation || current.rotation != target.rotation {
            *motion = Some(0.0);
        }
        return current;
    };
    *lerp += dt * 10.0;
    if *lerp >= 1.0 {
        *motion = None;
        return Transform {
            scale: current.scale,
            ..target
        };
    }
    Transform {
        translation: current.translation.lerp(target.translation, *lerp),
        rotation: current.rotation.lerp(target.rotation, *lerp).normalize(),
        scale: current.scale,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// World car part poses reset `TCap` to closed each frame. The cap must still open.
    #[test]
    fn an_open_cap_reaches_its_open_pose() {
        let closed = Transform::default();
        let opened = Transform::from_xyz(0.0, 0.1, 0.0);
        let mut cap = CapVisual {
            opening: OpeningId(0),
            closed,
            opened,
            euler: false,
            closed_euler: Vec3::ZERO,
            open_euler: Vec3::ZERO,
            motion: None,
            pose: closed,
        };
        let pose = (0..30).fold(closed, |_, _| cap.step(1.0, 1.0 / 60.0));
        assert_eq!(pose.translation, opened.translation);
        let pose = (0..30).fold(opened, |_, _| cap.step(0.0, 1.0 / 60.0));
        assert_eq!(pose.translation, closed.translation);
    }
}
