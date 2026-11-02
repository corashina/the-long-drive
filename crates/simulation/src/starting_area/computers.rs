//! Computer and TV actions of the player (fpscontroller look-at and held-item code).
use super::GarageRuntime;
use crate::audio::AudioKind;
use crate::interaction::AimTarget;
use crate::interaction::computer::{TypingFrame, note_pitch};
use crate::interaction::feedback::ComputerPrompt;
use crate::reference::{ComputerReference, ItemId};
use crate::simulation::{FIXED_DT, FrameInput, keys};

impl GarageRuntime {
    /// Changed and new computer screens, and the computer set when it changed.
    pub(super) fn screen_changes(&mut self, frame: &mut crate::boundary::SimFrame) {
        use crate::boundary::Change;
        use crate::interaction::computer::screen::ScreenView;
        let sent = self.screens_sent.as_deref().unwrap_or(&[]);
        let items = &mut self.screens_scratch;
        items.clear();
        self.stream
            .computers
            .take_screens(|item, computer, changed| {
                items.push(item);
                if changed || !sent.contains(&item) {
                    frame
                        .changes
                        .push(Change::Screen(Box::new(ScreenView::of(item, computer))));
                }
            });
        if self.screens_sent.as_ref() != Some(&self.screens_scratch) {
            frame
                .changes
                .push(Change::Screens(self.screens_scratch.clone()));
            self.screens_sent = Some(self.screens_scratch.clone());
        }
    }
    /// inputscript while usingComputer: the game reads no other input, and LMB ends typing.
    pub(super) fn step_typing(&mut self, input: &mut FrameInput) {
        let blocked = self.needs_output().dead || self.sleeping();
        let computers = &mut self.stream.computers;
        let mut frame = None;
        let was_typing = computers.typing;
        if was_typing {
            if blocked || input.pressed & keys::PRIMARY != 0 {
                computers.typing = false;
            } else {
                let keys = input.typing.keys;
                frame = Some(TypingFrame {
                    text: input.typing.text(),
                    any_key: keys & 1 << 24 != 0,
                    arrows: (keys >> 17) as u8 & 15,
                    piano: keys & 0x1ffff,
                    pressed: (keys >> 21) as u8 & 7,
                });
            }
        }
        computers.advance(FIXED_DT, frame);
        if was_typing {
            *input = FrameInput::default();
        }
    }
    /// The aimed item within reach.
    fn looked_at(&self) -> Option<ItemId> {
        let reach = self.area.interaction.reach();
        self.aim
            .focus_target
            .filter(|t: &AimTarget| t.distance.is_finite() && (0. ..=reach).contains(&t.distance))
            .map(|t| t.item)
    }
    /// R turns the aimed computer on or off and LMB uses it. E connects or disconnects a
    /// held computer.
    pub(super) fn computer_actions(&mut self, input: FrameInput) {
        let looked = self.looked_at();
        let computers = &mut self.stream.computers;
        if let Some(item) = looked
            && let Some(b) = computers.computer(item)
        {
            if b.computer.on && input.pressed & keys::PRIMARY != 0 {
                computers.use_computer(item);
            }
            if input.pressed & keys::RELOAD != 0 {
                computers.turn(item);
            }
        }
        if let Some(held) = self.area.interaction.held()
            && let Some(b) = computers.computer(held)
            && input.pressed & keys::USE != 0
        {
            if b.display.is_some() {
                computers.disconnect(held);
            } else if let Some(display) = looked.filter(|d| computers.is_display(*d)) {
                computers.connect(held, Some(display));
            }
        }
        self.stream.publish_display_changes();
    }
    /// `ComputerPrompt` bits of the aimed and held computers.
    pub fn computer_prompt(&self) -> u8 {
        let computers = &self.stream.computers;
        let looked = self.looked_at();
        let mut bits = 0;
        if computers.typing {
            bits |= ComputerPrompt::TYPING;
        }
        if let Some(b) = looked.and_then(|item| computers.computer(item)) {
            bits |= if b.computer.on {
                ComputerPrompt::TURN_OFF | ComputerPrompt::USE
            } else {
                ComputerPrompt::TURN_ON
            };
        }
        if let Some(b) = self
            .area
            .interaction
            .held()
            .and_then(|h| computers.computer(h))
        {
            if b.display.is_some() {
                bits |= ComputerPrompt::DISCONNECT;
            } else if looked.is_some_and(|d| computers.is_display(d)) {
                bits |= ComputerPrompt::CONNECT;
            }
        }
        bits
    }
    /// Music mode notes: one loop per held note. Orphaned notes of an earlier music
    /// command keep their sound and get no pitch change.
    pub(super) fn play_computer_notes(&mut self, reference: &ComputerReference) {
        let Some(audio) = &mut self.audio else {
            return;
        };
        audio.stop_survival_loops(AudioKind::ComputerNote);
        let detune = (
            reference.pitch_difference,
            reference.pitch_difference_chance,
        );
        for b in self.stream.computers.computers() {
            let c = &b.computer;
            let mut bits = c.notes | c.orphans;
            if bits == 0 {
                continue;
            }
            let Some(item) = self.area.items.get(b.item) else {
                continue;
            };
            let position = self.stream.origin.absolute(item.pose.position);
            while bits != 0 {
                let semitone = bits.trailing_zeros() as u8;
                bits &= bits - 1;
                let held = c.notes & 1 << semitone != 0;
                // The 24-bit loop key keeps 17 bits of the item id and 7 bits of the note.
                let emitter = (b.item.0 << 7 | u32::from(semitone)) & 0xff_ffff;
                audio.computer_note(
                    emitter,
                    reference.volume,
                    note_pitch(semitone),
                    if held { detune } else { (0., 0.) },
                    position,
                );
            }
        }
    }
    /// mainscript.PlayClipAtPoint with a random pitch for the display switch.
    pub(super) fn play_computer_sounds(&mut self) {
        for (clip, gain, position) in self.stream.computers.sounds.drain(..) {
            if let Some(audio) = &mut self.audio {
                let pitch = audio.random_pitch();
                audio.survival_event(crate::audio::SurvivalAudioEvent {
                    session: audio.session,
                    emitter: 0,
                    kind: crate::audio::AudioKind::ItemHit,
                    action: crate::audio::AudioAction::Start,
                    clip,
                    gain,
                    pitch,
                    position,
                });
            }
        }
    }
}
