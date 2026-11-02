//! Source `fpscontroller` pointing (`input.point`, 18258-18288): `pointingValue` and `pointAt`.
use rapier3d::math::{Matrix, Pose, Rotation, Vector};

#[derive(Clone, Copy, Debug)]
pub struct PointState {
    /// `pointingValue`.
    pub value: f32,
    /// `pointRange3`.
    range: f32,
    /// `pointAt`. Its parent is the Player, so it is player-local.
    pub at: Pose,
}

impl Default for PointState {
    fn default() -> Self {
        Self {
            value: 0.,
            range: 0.,
            at: Pose::IDENTITY,
        }
    }
}

fn lerp(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t.clamp(0., 1.)
}

/// Unity `Quaternion.LookRotation(forward, up)`: -Z looks along `forward`.
fn look_rotation(forward: Vector, up: Vector) -> Option<Rotation> {
    let forward = forward.normalize_or_zero();
    let right = forward.cross(up).normalize_or_zero();
    if right == Vector::ZERO {
        return None;
    }
    let up = right.cross(forward);
    Some(Rotation::from_mat3(&Matrix::from_cols(right, up, -forward)))
}

impl PointState {
    /// One frame. `camera` and the hit (point, normal) are player-local.
    /// `range` is `pointRange`, `range2` is `pointRange2`.
    pub fn step(
        &mut self,
        held: bool,
        camera: Pose,
        hit: Option<(Vector, Vector)>,
        range: f32,
        range2: f32,
        dt: f32,
    ) {
        if !held {
            self.value = (self.value - dt * 10.).clamp(0., 1.);
            return;
        }
        self.value = (self.value + dt * 15.).clamp(0., 1.);
        let forward = camera.rotation * -Vector::Z;
        let target = match hit {
            Some((point, normal)) => {
                self.range = lerp(self.range, range, dt * 15.);
                // Mathf.InverseLerp: 0 for an empty range.
                let t = if range == range2 {
                    0.
                } else {
                    (self.range - range2) / (range - range2)
                };
                self.at.translation = self
                    .at
                    .translation
                    .lerp(point + normal * 0.025, t.clamp(0., 1.));
                let up = camera.rotation * Vector::Y;
                let along = forward + up;
                look_rotation(along - normal * along.dot(normal), normal)
            }
            None => {
                self.range = lerp(self.range, range2, dt * 15.);
                self.at.translation = camera.translation + forward * self.range;
                // `pointAt.parent.rotation`: the Player.
                Some(Rotation::IDENTITY)
            }
        };
        if let Some(target) = target {
            self.at.rotation = self.at.rotation.lerp(target, (dt * 8.5).clamp(0., 1.));
        }
    }
}
