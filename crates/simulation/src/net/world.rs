//! World messages: the host clock (`SendTime`), the radio stations (`SRadioChannel`), body sounds (`SSound`),
//! radio controls (`SRadio`), and the port weather cells and lightning strikes.
use super::NetRole;
use super::message::{
    self, HEADER, Message, RADIO_CHANNEL, RadioChannelMessage, RadioMessage, SoundMessage,
    StrikeMessage, TimeMessage, WeatherCellWire, WeatherCellsMessage,
};
use super::objects::{NetKey, ObjectIndex};
use crate::audio::{AudioAction, AudioKind, SurvivalAudioEvent};
use crate::starting_area::GarageRuntime;
use crate::weather::{MAX_CELLS, WeatherCell, WeatherCells, WeatherKind};

/// The wait of source `multiSyncTime` between clock sends.
pub const TIME_SYNC: f32 = 10.;

#[derive(Default)]
pub struct WorldSync {
    time_timer: f32,
    /// A guest joined. The host sends the clock and the stations at once (source `SendStartStuff`).
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
        let strike = runtime.weather.strike.take();
        if role != NetRole::Host {
            return;
        }
        if let Some(position) = strike {
            message::push(slot, &StrikeMessage { position }, out);
        }
        let joined = std::mem::take(&mut self.joined);
        self.time_timer += dt;
        if joined || self.time_timer >= TIME_SYNC {
            self.time_timer = 0.;
            let time = TimeMessage {
                cycle_seconds: runtime.lighting.cycle.cycle_seconds(),
            };
            message::push(slot, &time, out);
            let weather = &runtime.weather;
            let m = weather_message(&weather.cells, weather.drift, weather.wetness);
            message::push(slot, &m, out);
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

    /// Applies one world message of another peer.
    pub fn receive(role: NetRole, packet: &[u8], index: &ObjectIndex, runtime: &mut GarageRuntime) {
        // Only the host sends the clock and the stations.
        let from_host = |slot: u8| slot == 0 && role == NetRole::Guest;
        if let Ok((slot, time)) = TimeMessage::decode(packet) {
            if from_host(slot) && time.cycle_seconds.is_finite() {
                runtime.lighting.cycle.set_cycle_seconds(time.cycle_seconds);
            }
        } else if let Ok((slot, m)) = WeatherCellsMessage::decode(packet) {
            if from_host(slot) {
                if let Some(cells) = read_cells(&m) {
                    let weather = &mut runtime.weather;
                    weather
                        .cells
                        .set(cells.into_iter().flatten(), m.locked != 0);
                    weather.drift = m.drift;
                    weather.wetness = m.wetness;
                }
            }
        } else if let Ok((slot, m)) = StrikeMessage::decode(packet) {
            if from_host(slot) && m.position.iter().all(|v| v.is_finite()) {
                let local = runtime.area.player.body_pose(&runtime.physics).position;
                let player = runtime.stream.origin.absolute(local);
                runtime.weather.lightning.strike(m.position, player);
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
        }
    }
}

fn weather_message(cells: &WeatherCells, drift: [f32; 2], wetness: f32) -> WeatherCellsMessage {
    let mut m = WeatherCellsMessage {
        drift,
        wetness,
        locked: cells.locked().into(),
        ..Default::default()
    };
    for (wire, cell) in m.cells.iter_mut().zip(cells.iter()) {
        *wire = WeatherCellWire {
            center: cell.center,
            radius: cell.radius,
            life: cell.life,
            age: cell.age,
            kind: cell.kind as u32,
        };
        m.count += 1;
    }
    m
}

/// The cells of a host message. `None` for a bad kind or value.
fn read_cells(m: &WeatherCellsMessage) -> Option<[Option<WeatherCell>; MAX_CELLS]> {
    let count = m.count as usize;
    if count > MAX_CELLS
        || m.locked > 1
        || (m.locked == 1 && count != 1)
        || !(0. ..=1.).contains(&m.wetness)
        || !m.drift.iter().all(|v| v.is_finite())
    {
        return None;
    }
    let mut cells = [None; MAX_CELLS];
    for (cell, wire) in cells.iter_mut().zip(&m.cells[..count]) {
        let value = WeatherCell {
            kind: *WeatherKind::ALL.get(wire.kind as usize)?,
            center: wire.center,
            radius: wire.radius,
            life: wire.life,
            age: wire.age,
        };
        if !value.valid() {
            return None;
        }
        *cell = Some(value);
    }
    Some(cells)
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
    // The clip must be one of this sound's clips. The browser plays only known clips.
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn guest_applies_the_host_cells() {
        let mut host = WeatherCells::new(1);
        for _ in 0..10 {
            host.advance(10., [3., 0.], [0.; 2], false, |_| 2);
        }
        assert!(!host.is_empty());
        let mut out = Vec::new();
        weather_message(&host, [3., 0.], 0.5).encode(0, &mut out);
        let (slot, m) = WeatherCellsMessage::decode(&out).unwrap();
        assert_eq!((slot, m), (0, weather_message(&host, [3., 0.], 0.5)));
        let mut bad = m;
        bad.cells[0].kind = 9;
        assert_eq!(read_cells(&bad), None);
        bad = m;
        bad.cells[0].radius = f32::NAN;
        assert_eq!(read_cells(&bad), None);
        let mut guest = WeatherCells::new(2);
        guest.set(read_cells(&m).unwrap().into_iter().flatten(), false);
        assert!(guest.iter().eq(host.iter()));
        // A guest moves the cells but does not age them or spawn new ones.
        guest.advance(10., [3., 0.], [0.; 2], true, |_| 2);
        assert_eq!(guest.len(), host.len());
        assert!(guest.iter().zip(host.iter()).all(|(g, h)| g.age == h.age));
    }
}
