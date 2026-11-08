//! On-foot third-person view. The source game has no such view. The camera key toggles it.
use glam::{Quat, Vec3};

/// Lift of the camera line above the head, so the head does not cover the screen center.
const LIFT: f32 = 0.35;
const DISTANCE: f32 = 2.5;
/// Radius of the ball that keeps the camera out of walls.
pub const CLEARANCE_RADIUS: f32 = 0.2;

#[derive(Clone, Copy, Debug, Default)]
pub struct FootCamera {
    outside: bool,
    position: Vec3,
    rotation: Quat,
}

impl FootCamera {
    pub fn outside(&self) -> bool {
        self.outside
    }

    /// The camera pose while the view is on.
    pub fn pose(&self) -> Option<(Vec3, Quat)> {
        self.outside.then_some((self.position, self.rotation))
    }

    pub fn leave(&mut self) {
        self.outside = false;
    }

    /// A press toggles the view. `free` is false when the player sits, is dead or sleeps.
    pub fn toggle(&mut self, pressed: bool, free: bool) {
        self.outside = free && (self.outside != pressed);
    }

    /// The move from the head to the free camera position: up by the lift, back by the distance.
    pub fn line(head: (Vec3, Quat)) -> Vec3 {
        Vec3::Y * LIFT + head.1 * Vec3::new(0., 0., DISTANCE)
    }

    /// Places the camera on the line. `reach` in 0..=1 is the free part of the line.
    pub fn place(&mut self, head: (Vec3, Quat), reach: f32) {
        self.position = head.0 + Self::line(head) * reach.clamp(0., 1.);
        self.rotation = head.1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_tap_toggles_a_seat_ends_and_a_wall_pulls_in() {
        let mut camera = FootCamera::default();
        camera.toggle(true, true);
        assert!(camera.outside());
        camera.toggle(false, true);
        assert!(camera.outside());
        camera.place((Vec3::ZERO, Quat::IDENTITY), 1.);
        let (position, _) = camera.pose().unwrap();
        assert!((position - Vec3::new(0., LIFT, DISTANCE)).length() < 1e-5);
        camera.place((Vec3::ZERO, Quat::IDENTITY), 0.5);
        assert!((camera.pose().unwrap().0 - Vec3::new(0., LIFT, DISTANCE) * 0.5).length() < 1e-5);
        camera.toggle(false, false);
        assert!(camera.pose().is_none());
        camera.toggle(true, true);
        camera.toggle(true, true);
        assert!(!camera.outside());
    }
}
