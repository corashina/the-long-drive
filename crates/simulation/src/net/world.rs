//! World messages: the host clock (`SendTime`), the radio stations (`SRadioChannel`), body sounds (`SSound`)
//! and radio controls (`SRadio`).
use super::NetRole;
use super::message::{
    self, HEADER, Message, RADIO_CHANNEL, RadioChannelMessage, RadioMessage, SoundMessage,
    TimeMessage,
};
use super::objects::{NetKey, ObjectIndex};
use crate::audio::{AudioAction, AudioKind, SurvivalAudioEvent};
use crate::starting_area::GarageRuntime;

/// Source `multiSyncTime` wait.
pub const TIME_SYNC: f32 = 10.;

#[derive(Default)]
pub struct WorldSync {
    time_timer: f32,
    /// A guest joined: the host sends the clock and the stations at once (source `SendStartStuff`).
    joined: bool,
}

impl WorldSync {
    pub fn joined(&mut self) {
        self.joined = true;
    }

    /// Sends the local world messages of one frame.
    pub fn send(
        &mut self,
        role: NetRole,
        slot: u8,
        runtime: &mut GarageRuntime,
        dt: f32,
        index: &ObjectIndex,
        out: &mut Vec<u8>,
    ) {
        for sound in runtime.shared_sounds.drain(..) {
            message::push(slot, &sound, out);
        }
        for &item in &runtime.shared_radios {
            if let (Some(key), Some(state)) = (index.key(item), runtime.stream.radio_state(item)) {
                let saved = state.capture_save();
                let m = RadioMessage {
                    key: key.0,
                    tuning: saved.tuning,
                    volume: saved.volume,
                    on: saved.on.into(),
                    am: saved.am.into(),
                };
                message::push(slot, &m, out);
            }
        }
        runtime.shared_radios.clear();
        if role != NetRole::Host {
            return;
        }
        let joined = std::mem::take(&mut self.joined);
        self.time_timer += dt;
        if joined || self.time_timer >= TIME_SYNC {
            self.time_timer = 0.;
            let time = TimeMessage {
                cycle_seconds: runtime.lighting.cycle.cycle_seconds(),
            };
            message::push(slot, &time, out);
        }
        if let Some(audio) = &mut runtime.audio
            && (audio.broadcasts.take_changed() || joined)
        {
            let channels = audio.broadcasts.host_channels(audio.game_seconds);
            message::push_with(true, HEADER + 4 + channels.len() * 8, out, |bytes| {
                RadioChannelMessage::encode(slot, channels, bytes)
            });
        }
    }

    /// Applies one packet. False when it is not a world message.
    pub fn receive(
        role: NetRole,
        packet: &[u8],
        index: &ObjectIndex,
        runtime: &mut GarageRuntime,
    ) -> bool {
        // Only the host sends the clock and the stations.
        let from_host = |slot: u8| slot == 0 && role == NetRole::Guest;
        if let Ok((slot, time)) = TimeMessage::decode(packet) {
            if from_host(slot) && time.cycle_seconds.is_finite() {
                runtime.lighting.cycle.set_cycle_seconds(time.cycle_seconds);
            }
        } else if packet.first() == Some(&RADIO_CHANNEL) {
            if let Ok((slot, channels)) = RadioChannelMessage::decode(packet)
                && from_host(slot)
                && let Some(audio) = &mut runtime.audio
            {
                audio.broadcasts.apply_host(channels, audio.game_seconds);
            }
        } else if let Ok((_, m)) = RadioMessage::decode(packet) {
            if let Some(item) = index.item(NetKey(m.key)) {
                let saved = crate::save::RadioSnapshot {
                    on: m.on != 0,
                    am: m.am != 0,
                    tuning: m.tuning,
                    volume: m.volume,
                };
                runtime
                    .stream
                    .set_radio(item, &saved, &mut runtime.area.items);
            }
        } else if let Ok((slot, sound)) = SoundMessage::decode(packet) {
            play(runtime, slot, &sound);
        } else {
            return false;
        }
        true
    }
}

/// Source `soundMainScript.PlaySound`: a one-shot at the sender position.
fn play(runtime: &mut GarageRuntime, slot: u8, sound: &SoundMessage) {
    let kind = match sound.kind {
        k if k == AudioKind::Burp as u32 => AudioKind::Burp,
        k if k == AudioKind::Fart as u32 => AudioKind::Fart,
        k if k == AudioKind::Shit as u32 => AudioKind::Shit,
        // Source `WeaponShot`.
        k if k == AudioKind::ItemHit as u32 => AudioKind::ItemHit,
        _ => return,
    };
    // The clip must be one of this sound's clips: the browser plays only known clips.
    let known = if kind == AudioKind::ItemHit {
        runtime
            .audio
            .as_ref()
            .is_some_and(|a| a.reference.clips.iter().any(|c| c.id == sound.clip))
    } else {
        runtime
            .audio_gameplay
            .binding(kind as u32)
            .clips
            .contains(&sound.clip)
    };
    if !known
        || !sound.position.iter().all(|v| v.is_finite())
        || !sound.gain.is_finite()
        || !sound.pitch.is_finite()
    {
        return;
    }
    let Some(audio) = &mut runtime.audio else {
        return;
    };
    audio.survival_event(SurvivalAudioEvent {
        session: 0,
        // A non-zero emitter plays at the event position, not at the local player.
        emitter: u32::from(slot) + 1,
        kind,
        action: AudioAction::Start,
        clip: sound.clip,
        gain: sound.gain,
        pitch: sound.pitch,
        position: sound.position,
    });
}
