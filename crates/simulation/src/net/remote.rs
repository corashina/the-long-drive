//! One remote player: source `snTempPlayerSync.player` smoothing and the `mpplayerscript` body.
use super::SYNC_TIME;
use super::message::{BodyMessage, PoseMessage};
use crate::player::body::BodyState;
use crate::vehicle::VehiclePose;
use crate::world::WorldOrigin;
use glam::{DQuat, DVec3};

/// Source `UpdPos`: the figure starts to move 1 s after it shows.
const READY_DELAY: f64 = 1.;

#[derive(Clone, Copy, Default)]
struct Sample {
    position: DVec3,
    rotation: DQuat,
}

impl Sample {
    fn of(m: &PoseMessage) -> Self {
        let r = m.rotation.map(f64::from);
        Self {
            position: DVec3::from_array(m.position),
            rotation: DQuat::from_xyzw(r[0], r[1], r[2], r[3]).normalize(),
        }
    }
}

pub struct Remote {
    pub slot: u8,
    pub body: BodyState,
    pub smoothing: Smoothing,
    targets: Option<[[f32; 7]; 6]>,
}

impl Remote {
    pub fn new(slot: u8, body: BodyState) -> Self {
        Self {
            slot,
            body,
            smoothing: Smoothing::default(),
            targets: None,
        }
    }

    /// Source `SPlayerAnim` receiver.
    pub fn got_body(&mut self, m: &BodyMessage) {
        self.targets = Some(m.targets);
    }

    /// Source `UpdPos`, then the IK. Returns the figure pose in stream space and `CharacterRoot`.
    pub fn step(
        &mut self,
        now: f64,
        dt: f32,
        origin: &WorldOrigin,
    ) -> Option<(VehiclePose, VehiclePose)> {
        let targets = self.targets?;
        let shown = self.smoothing.update(now, dt)?;
        self.body.pose_remote(self.smoothing.duck, &targets);
        let pose = VehiclePose {
            position: origin.local(shown.position.to_array()),
            rotation: shown.rotation.as_quat().to_array(),
        };
        Some((pose, self.body.root_local()))
    }
}

/// Source `snTempPlayerSync.player` pose state.
#[derive(Default)]
pub struct Smoothing {
    /// Source `oldPos`, `lastPos`, `nextPos`, `previousNextPos` with rotations.
    old: Sample,
    last: Sample,
    next: Sample,
    previous_next: Sample,
    old_time: f64,
    last_time: f64,
    next_time: f64,
    /// Figure Transform `T`.
    shown: Sample,
    seated: bool,
    pub duck: f32,
    start: Option<f64>,
}

impl Smoothing {
    /// Source `GotPos`. The first pose places the figure.
    pub fn got_pose(&mut self, m: &PoseMessage, now: f64) {
        let sample = Sample::of(m);
        let seated = m.flags & PoseMessage::SEATED != 0;
        if self.start.is_none() {
            *self = Self {
                old: sample,
                last: sample,
                next: sample,
                previous_next: sample,
                old_time: now,
                last_time: now,
                next_time: now,
                shown: sample,
                seated,
                duck: m.duck,
                start: Some(now),
            };
        }
        let was_seated = self.seated;
        self.seated = seated;
        self.duck = m.duck;
        self.old = self.last;
        self.old_time = self.last_time;
        self.last = sample;
        if !seated {
            if was_seated {
                self.old = sample;
                self.next = sample;
                self.previous_next = sample;
                self.shown = sample;
            } else {
                self.previous_next = self.next;
                self.next = Sample {
                    position: self.old.position.lerp(self.last.position, 2.),
                    rotation: lerp_unclamped(self.old.rotation, self.last.rotation, 2.),
                };
            }
        }
        self.last_time = now;
        self.next_time = now + (self.last_time - self.old_time);
    }

    /// Source `UpdPos`. `None` before the first pose.
    fn update(&mut self, now: f64, dt: f32) -> Option<Sample> {
        let start = self.start?;
        if now - start <= READY_DELAY {
            return Some(self.shown);
        }
        if self.seated {
            // Seats are not shared yet: the figure takes the sent world pose.
            self.shown = self.last;
            return Some(self.shown);
        }
        let span = self.next_time - self.last_time;
        let t = if span > 0. {
            ((now - self.last_time) / span).clamp(0., 1.)
        } else {
            1.
        };
        let middle = (self.previous_next.position + self.last.position) * 0.5;
        let b = middle.lerp(self.next.position, t);
        let b2 = nlerp(
            nlerp(self.previous_next.rotation, self.last.rotation, 0.5),
            self.next.rotation,
            t,
        );
        let k = (SYNC_TIME as f64 * 100. * dt as f64).clamp(0., 1.);
        self.shown.position = self.shown.position.lerp(b, k);
        self.shown.rotation = nlerp(self.shown.rotation, b2, k);
        Some(self.shown)
    }
}

/// Unity `Quaternion.Lerp`: normalized, shortest path, `t` clamped.
fn nlerp(a: DQuat, b: DQuat, t: f64) -> DQuat {
    lerp_unclamped(a, b, t.clamp(0., 1.))
}

/// Unity `Quaternion.LerpUnclamped`.
fn lerp_unclamped(a: DQuat, b: DQuat, t: f64) -> DQuat {
    let b = if a.dot(b) < 0. { -b } else { b };
    (a * (1. - t) + b * t).normalize()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pose(x: f64) -> PoseMessage {
        PoseMessage {
            position: [x, 0., 0.],
            rotation: [0., 0., 0., 1.],
            duck: 1.,
            flags: 0,
        }
    }

    #[test]
    fn got_pose_extrapolates_one_interval() {
        let mut s = Smoothing::default();
        for (x, now) in [(0., 0.), (1., 0.15), (2., 0.3)] {
            s.got_pose(&pose(x), now);
        }
        // `nextPos = LerpUnclamped(oldPos, lastPos, 2)`, `nextTime = lastTime + (lastTime - oldTime)`.
        assert_eq!(s.next.position.x, 3.);
        assert!((s.next_time - 0.45).abs() < 1e-12);
        // Before `readyToMove` the figure stays at the first pose.
        assert_eq!(s.update(0.5, 1. / 60.).unwrap().position.x, 0.);
        // Later it moves toward the blend at `nextTime`: (2 + 2) / 2 lerps to 3.
        let mut s2 = Smoothing::default();
        s2.got_pose(&pose(0.), 0.);
        s2.got_pose(&pose(1.), 1.2);
        s2.got_pose(&pose(2.), 1.35);
        let shown = s2.update(1.5, 1.).unwrap();
        assert!((shown.position.x - 3.).abs() < 1e-9);
    }

    #[test]
    fn standing_up_snaps_the_figure() {
        let mut s = Smoothing::default();
        s.got_pose(&pose(0.), 0.);
        s.got_pose(
            &PoseMessage {
                flags: PoseMessage::SEATED,
                ..pose(5.)
            },
            2.,
        );
        assert_eq!(s.update(2.1, 0.01).unwrap().position.x, 5.);
        s.got_pose(&pose(9.), 2.15);
        assert_eq!(s.update(2.2, 0.01).unwrap().position.x, 9.);
    }

    #[test]
    fn lerp_unclamped_takes_the_short_path() {
        let a = DQuat::IDENTITY;
        let b = -DQuat::from_rotation_y(0.2);
        let q = lerp_unclamped(a, b, 1.);
        assert!(q.abs_diff_eq(DQuat::from_rotation_y(0.2), 1e-9));
    }
}
