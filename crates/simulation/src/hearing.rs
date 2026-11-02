//! Source `fpscontroller` hearing and ear ringing, and the `explosionscript` hearing loss.
use crate::reference::{ExplosionPlayer, ReferenceError, ReferenceManifest};
use crate::save::HearingSnapshot;

pub struct HearingState {
    pub hearing: f32,
    pub ringing: f32,
    pub target: f32,
    /// `earRingDir`: the stereo pan of the ear ring, -1 left to 1 right.
    pub dir: f32,
    /// The ear ring clip.
    pub clip: u32,
    min_force_to_die: f32,
    loss: f32,
    regen: f32,
    gain: f32,
    decay: f32,
}

impl HearingState {
    pub fn new(r: &ReferenceManifest) -> Result<Self, ReferenceError> {
        let p: &ExplosionPlayer = &r.explosions.player;
        let clip = crate::interaction::melee::clip_table(r)
            .iter()
            .find(|(s, _, _)| *s == p.ear_ring.clip)
            .ok_or_else(|| ReferenceError("Missing ear ring clip".into()))?
            .1;
        Ok(Self {
            hearing: 1.,
            ringing: 0.,
            target: 0.,
            dir: 0.,
            clip,
            min_force_to_die: p.min_force_to_die,
            loss: p.hear_loss_modifier,
            regen: p.hear_regen,
            gain: p.ear_ringing_gain,
            decay: p.ear_ringing_regen,
        })
    }
    /// A blast of `power` at `angle` degrees right of the camera forward. `draw` is in [0, 1).
    /// Returns true when the blast kills the player.
    pub fn blast(&mut self, power: f32, angle: f32, draw: f32) -> bool {
        let loss = power * self.loss;
        self.hearing -= loss;
        self.target += loss * (0.5 + 0.5 * draw);
        self.dir = (angle / 90.).clamp(-1., 1.);
        power >= self.min_force_to_die
    }
    /// One frame with the cursor locked.
    pub fn advance(&mut self, dt: f32) {
        if self.hearing < 1. {
            self.hearing = (self.hearing + dt * self.regen).clamp(self.hearing, 1.);
        }
        if self.ringing < self.target {
            self.ringing += dt * self.gain;
        } else if self.target > 0. {
            self.target -= dt * self.decay;
        }
        if self.ringing > self.target {
            self.ringing = self.target;
        }
    }
    /// Mixer `PlayerEar`.
    pub fn ear_db(&self) -> f32 {
        crate::audio::source_gain_db(self.hearing)
    }
    /// `EarRingSource.volume`; 0 stops it.
    pub fn ring_gain(&self) -> f32 {
        self.ringing.clamp(0., 1.)
    }
    pub fn capture_save(&self) -> HearingSnapshot {
        HearingSnapshot {
            hearing: self.hearing,
            ringing: self.ringing,
            target: self.target,
            dir: self.dir,
        }
    }
    pub fn restore_save(&mut self, s: &HearingSnapshot) {
        self.hearing = s.hearing;
        self.ringing = s.ringing;
        self.target = s.target;
        self.dir = s.dir;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Source `explosionscript.Explode` (hearing loss, ring target, `earRingDir`, death) and the
    /// `fpscontroller` hearing frame. The save keeps all four fields, as `savedatascript` does.
    #[test]
    fn tp_wd18_a_blast_dulls_rings_from_its_side_fades_saves_and_kills_up_close() {
        let mut h = HearingState {
            hearing: 1.,
            ringing: 0.,
            target: 0.,
            dir: 0.,
            clip: 0,
            min_force_to_die: 50.,
            loss: 0.01,
            regen: 0.1,
            gain: 1.,
            decay: 0.05,
        };
        assert!(!h.blast(20., 45., 1.));
        // Dull sound: the ear mixer drops.
        assert!((h.hearing - 0.8).abs() < 1e-6);
        assert!(h.ear_db() < -1.9);
        // The ring comes from the blast side: 45 degrees right pans half right.
        assert!((h.dir - 0.5).abs() < 1e-6);
        assert!((h.target - 0.2).abs() < 1e-6);
        h.advance(0.1);
        assert!(h.ring_gain() > 0.);
        let saved = h.capture_save();
        let mut loaded = HearingState { ..h };
        loaded
            .restore_save(&serde_json::from_value(serde_json::to_value(&saved).unwrap()).unwrap());
        assert_eq!(loaded.capture_save(), saved);
        // Both fade.
        for _ in 0..600 {
            loaded.advance(1. / 60.);
        }
        assert!(loaded.ring_gain() == 0. && loaded.target <= 0.);
        assert!(loaded.hearing == 1.);
        // Left side, behind: the pan clamps to full left.
        h.blast(1., -170., 0.);
        assert_eq!(h.dir, -1.);
        // Close: power at `minForceToDieFromExplosion` kills.
        assert!(h.blast(50., 0., 0.));
    }
}
