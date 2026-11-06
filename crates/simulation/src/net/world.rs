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
use crate::weather::{MAX_CELLS, WeatherCell, WeatherKind, WeatherState, Wind};

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
            message::push(slot, &weather_message(&runtime.weather), out);
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
                apply_weather(&m, &mut runtime.weather);
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

fn weather_message(weather: &WeatherState) -> WeatherCellsMessage {
    let mut m = WeatherCellsMessage {
        wind_dir: weather.wind.dir,
        wind_speed: weather.wind.speed,
        wetness: weather.wetness,
        snow_cover: weather.snow_cover,
        locked: weather.cells.locked().into(),
        ..Default::default()
    };
    for (wire, cell) in m.cells.iter_mut().zip(weather.cells.iter()) {
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

/// Applies a host message on a guest. Drops a message with a bad kind or value.
fn apply_weather(m: &WeatherCellsMessage, weather: &mut WeatherState) {
    if let Some(cells) = read_cells(m) {
        weather
            .cells
            .set(cells.into_iter().flatten(), m.locked != 0);
        weather.wind.follow(m.wind_dir, m.wind_speed);
        weather.wetness = m.wetness;
        weather.snow_cover = m.snow_cover;
    }
}

/// The cells of a host message. `None` for a bad kind or value.
fn read_cells(m: &WeatherCellsMessage) -> Option<[Option<WeatherCell>; MAX_CELLS]> {
    let count = m.count as usize;
    if count > MAX_CELLS
        || m.locked > 1
        || (m.locked == 1 && count != 1)
        || !(0. ..=1.).contains(&m.wetness)
        || !(0. ..=1.).contains(&m.snow_cover)
        || !Wind::valid(m.wind_dir, m.wind_speed)
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
    fn guest_applies_the_host_cells_and_wind() {
        let mut host = WeatherState::test(1);
        host.snow_cover = 0.3;
        host.wetness = 0.5;
        for _ in 0..10 {
            host.advance_cells(10., [0.; 3], false, -5., |_| 2, |_| 0.);
        }
        assert!(!host.cells.is_empty());
        let mut out = Vec::new();
        weather_message(&host).encode(0, &mut out);
        let (slot, m) = WeatherCellsMessage::decode(&out).unwrap();
        assert_eq!((slot, m), (0, weather_message(&host)));
        let mut bad = m;
        bad.cells[0].kind = 9;
        assert_eq!(read_cells(&bad), None);
        bad = m;
        bad.cells[0].radius = f32::NAN;
        assert_eq!(read_cells(&bad), None);
        bad = m;
        bad.wind_speed = f32::INFINITY;
        assert_eq!(read_cells(&bad), None);
        bad = m;
        bad.snow_cover = 2.;
        assert_eq!(read_cells(&bad), None);
        let mut guest = WeatherState::test(2);
        apply_weather(&m, &mut guest);
        assert!(guest.cells.iter().eq(host.cells.iter()));
        assert_eq!(guest.wind.dir, host.wind.dir);
        assert_eq!(
            (guest.wetness, guest.snow_cover),
            (host.wetness, host.snow_cover)
        );
        // A guest moves the cells with the host wind. It does not age them, spawn new ones or change the surface.
        let start: Vec<_> = guest.cells.iter().map(|c| c.center).collect();
        for _ in 0..100 {
            guest.advance_cells(1., [0.; 3], true, 20., |_| 2, |_| 0.);
        }
        assert_eq!(guest.cells.len(), host.cells.len());
        assert!(
            guest
                .cells
                .iter()
                .zip(host.cells.iter())
                .all(|(g, h)| g.age == h.age)
        );
        assert_eq!(guest.wind.dir, host.wind.dir);
        assert!((guest.wind.speed - host.wind.speed).abs() < 0.05);
        assert_eq!(
            (guest.wetness, guest.snow_cover),
            (host.wetness, host.snow_cover)
        );
        let [dx, dz] = host.wind.dir;
        for (cell, from) in guest.cells.iter().zip(start) {
            let moved = [
                (cell.center[0] - from[0]) as f32,
                (cell.center[1] - from[1]) as f32,
            ];
            assert!(moved[0] * dx + moved[1] * dz > 0.);
        }
    }
}
