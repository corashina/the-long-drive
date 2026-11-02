//! Source `fpscontroller.Move` horizontal velocity: ground and air speed limits and the interior clamp.
use rapier3d::math::Vector;

/// Horizontal world velocity that carries over between steps.
#[derive(Clone, Copy, Debug, Default)]
pub(super) struct Momentum {
    velocity: Vector,
    /// Source `FmaxSpeed`. The air step clamps it to `takeoff`.
    max_speed: f32,
    /// Source `speedWhenJumped`: the full speed of the last grounded step.
    takeoff: f32,
}

fn horizontal(v: Vector) -> Vector {
    Vector::new(v.x, 0.0, v.z)
}

/// Source horizontal clamp. Inside an interior with body velocity `r`, the limit grows by
/// `|r| * InverseLerp(90, 0, Angle(v, r))`. Unity `Vector3.Angle` of a zero vector is 0.
fn clamp(v: Vector, limit: f32, interior: Option<Vector>) -> Vector {
    let extra = interior.map_or(0.0, |r| {
        let r = horizontal(r);
        let angle = if v.length_squared() < 1.0e-15 || r.length_squared() < 1.0e-15 {
            0.0
        } else {
            v.angle_between(r).to_degrees()
        };
        r.length() * ((90.0 - angle) / 90.0).clamp(0.0, 1.0)
    });
    v.clamp_length_max((limit + extra).max(0.0))
}

impl Momentum {
    pub(super) fn velocity(&self) -> Vector {
        self.velocity
    }
    pub(super) fn reset(&mut self) {
        *self = Self::default();
    }
    pub(super) fn restore(&mut self, velocity: Vector) {
        *self = Self {
            velocity: horizontal(velocity),
            ..Self::default()
        };
        self.takeoff = self.velocity.length();
        self.max_speed = self.takeoff;
    }
    /// Grounded or gripping. `walk` is the key velocity at `max_speed`. `floor` is the
    /// velocity of a dynamic floor body, which friction drags the player toward.
    pub(super) fn ground(
        &mut self,
        walk: Vector,
        floor: Vector,
        max_speed: f32,
        interior: Option<Vector>,
    ) {
        self.max_speed = max_speed;
        self.velocity = clamp(horizontal(floor + walk), max_speed, interior);
    }
    /// In the air. `push` is the velocity change of the source air force this step.
    pub(super) fn air(&mut self, push: Vector, interior: Option<Vector>) {
        self.max_speed = self.max_speed.min(self.takeoff);
        self.velocity = clamp(self.velocity + horizontal(push), self.max_speed, interior);
    }
    /// Source `speedWhenJumped`, set on each grounded step.
    pub(super) fn grounded(&mut self, speed: f32) {
        self.takeoff = speed;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn running(speed: f32) -> Momentum {
        let mut m = Momentum::default();
        m.ground(Vector::new(0.0, 0.0, -speed), Vector::ZERO, speed, None);
        m.grounded(speed);
        m
    }

    #[test]
    fn air_keeps_momentum_without_keys() {
        let mut m = running(3.5);
        for _ in 0..25 {
            m.air(Vector::ZERO, None);
        }
        assert!((m.velocity() - Vector::new(0.0, 0.0, -3.5)).length() < 1.0e-6);
    }

    #[test]
    fn air_speed_stays_below_takeoff() {
        let mut m = running(1.75);
        for _ in 0..50 {
            m.air(Vector::new(0.0, 0.0, -0.11), None);
        }
        assert!(m.velocity().length() <= 1.75 + 1.0e-5);
        let mut standing = running(0.0);
        standing.air(Vector::new(0.11, 0.0, 0.0), None);
        assert_eq!(standing.velocity(), Vector::ZERO);
    }

    #[test]
    fn air_force_turns_slowly() {
        let mut m = running(3.5);
        // 20000 N * 0.02 s / 72 kg * 0.02 s per step, pushing sideways.
        m.air(Vector::new(20000.0 * 0.02 / 72.0 * 0.02, 0.0, 0.0), None);
        let v = m.velocity();
        assert!(v.z < -3.4 && v.x > 0.0 && v.x < 0.2);
    }

    #[test]
    fn interior_carries_the_floor() {
        let floor = Vector::new(15.0, 0.0, 0.0);
        let mut inside = Momentum::default();
        inside.ground(Vector::ZERO, floor, 1.75, Some(floor));
        assert!((inside.velocity() - floor).length() < 1.0e-4);
        let mut outside = Momentum::default();
        outside.ground(Vector::ZERO, floor, 1.75, None);
        assert!((outside.velocity().length() - 1.75).abs() < 1.0e-5);
        let mut slow = Momentum::default();
        slow.ground(Vector::ZERO, Vector::new(1.0, 0.0, 0.0), 1.75, None);
        assert!((slow.velocity().x - 1.0).abs() < 1.0e-6);
    }
}
