//! Source `fpscontroller` hum on the player. See `crate::hum`.
use crate::audio::AudioKind;
use crate::hum::HumInput;
use crate::reference::ReferenceManifest;
use crate::simulation::{FIXED_DT, FrameInput, keys, player_keys};
use crate::vehicle::to_rapier;
use rapier3d::math::Vector;

impl super::GarageRuntime {
    /// One source hum `Update`. While the player hums, the mouse does not look unless LMB is held.
    pub(super) fn step_hum(
        &mut self,
        reference: &ReferenceManifest,
        input: &mut FrameInput,
        smoking: bool,
    ) {
        let r = &reference.survival.hum;
        let lmb = input.held & keys::PRIMARY != 0;
        let hum = HumInput {
            held: input.player_held & player_keys::HUM != 0 && !smoking,
            lmb,
            // Browser +Y is down. Source `lookY` is positive up.
            look: [input.mouse[0], -input.mouse[1]].map(|v| if v.is_finite() { v } else { 0. }),
            scroll: if input.wheel.is_finite() {
                input.wheel
            } else {
                0.
            },
        };
        self.hum.step(r, hum, self.simulation_seconds, FIXED_DT);
        if self.hum.humming() && !lmb {
            input.mouse = [0.; 2];
        }
        let Some(audio) = &mut self.audio else {
            return;
        };
        let Some(sound) = self.hum.sound() else {
            audio.stop_survival_loop(AudioKind::Hum, 0);
            return;
        };
        let offset = audio
            .reference
            .survival
            .iter()
            .find(|b| b.kind == AudioKind::Hum as u32)
            .expect("validated survival playback")
            .offset;
        let local =
            to_rapier(self.area.player.body_pose(&self.physics)) * Vector::from_array(offset);
        let position = self.stream.origin.absolute(local.to_array());
        audio.clip_loop(
            AudioKind::Hum,
            0,
            sound.clip,
            sound.volume,
            Some(sound.pitch),
            position,
        );
    }
}
