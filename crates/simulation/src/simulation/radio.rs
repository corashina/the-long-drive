//! Source controls and broadcasts. Playback belongs to the browser.
use crate::reference::{AudioClipReference, RadioAudioReference, RadioStationReference};

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum RadioControl {
    VolumeUp,
    VolumeDown,
    TuneUp,
    TuneDown,
    Band,
}
#[derive(Clone, Copy, Debug, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct RadioPlayback {
    pub station: Option<u32>,
    pub clip: Option<u32>,
    pub offset_seconds: f64,
    pub gain: f32,
    pub next_clip: Option<u32>,
}
#[derive(Clone, Copy, Debug, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct RadioFrame {
    pub power: bool,
    pub am: bool,
    pub frequency: f32,
    pub volume: f32,
    pub static_gain: f32,
    pub primary: RadioPlayback,
    pub secondary: RadioPlayback,
}
#[derive(Clone, Copy)]
struct Recording {
    clip: u32,
    duration: f64,
    gain: f32,
    holiday: u8,
}
#[derive(Clone, Copy)]
struct Selected {
    recording: Recording,
    music: bool,
}
struct Station {
    source: RadioStationReference,
    music: Vec<Recording>,
    talk: Vec<Recording>,
    music_bag: Vec<usize>,
    talk_bag: Vec<usize>,
    current: Option<Selected>,
    next: Option<Selected>,
    start: f64,
    random: u32,
}
/// One radioscript's controls: the Lada radio or a portable radio.
#[derive(Clone, Copy, Debug)]
pub struct RadioState {
    pub on: bool,
    pub am: bool,
    pub tuning: f32,
    pub volume: f32,
    /// AMmin, AMmax and AMStep.
    am_range: [f32; 3],
    /// FMmin, FMmax and FMStep.
    fm_range: [f32; 3],
    volume_step: f32,
    band: bool,
}
/// mainscript radioChs and radioChsAM, shared by every radio.
pub struct RadioBroadcasts {
    stations: Vec<Station>,
    fm_tolerance: f32,
    am_tolerance: f32,
    custom_gain: f32,
    holiday_ranges: [[u8; 4]; 2],
    calendar: [u8; 2],
    /// Multiplayer guest: the host selects the clips (source `RADIO2`).
    remote: bool,
    /// A station took a new clip since the last `take_changed`.
    changed: bool,
}
impl Station {
    fn select(
        &mut self,
        was_music: bool,
        calendar: [u8; 2],
        ranges: &[[u8; 4]; 2],
        reset: bool,
    ) -> Option<Selected> {
        let music = if was_music {
            self.talk.is_empty()
        } else {
            !self.music.is_empty()
        };
        let (recordings, bag) = if music {
            (&self.music, &mut self.music_bag)
        } else {
            (&self.talk, &mut self.talk_bag)
        };
        if bag.is_empty() {
            if !reset {
                return None;
            }
            for (i, r) in recordings.iter().enumerate() {
                if holiday(r.holiday, calendar, ranges) {
                    bag.push(i);
                }
            }
        }
        if bag.is_empty() {
            return None;
        }
        let index = (u64::from(xorshift(&mut self.random)) * bag.len() as u64 >> 32) as usize;
        let recording = recordings[bag.remove(index)];
        Some(Selected { recording, music })
    }
    /// True when the station takes a new clip.
    fn advance(&mut self, seconds: f64, calendar: [u8; 2], ranges: &[[u8; 4]; 2]) -> bool {
        if self.source.custom {
            return false;
        }
        let (current, changed) = match self.current {
            None => {
                let current = self.select(self.source.initial_music, calendar, ranges, true);
                (current, current.is_some())
            }
            Some(s) if seconds - self.start >= s.recording.duration => {
                let current = self
                    .next
                    .or_else(|| self.select(s.music, calendar, ranges, true));
                (current, true)
            }
            Some(_) => return false,
        };
        self.current = current;
        self.start = seconds;
        self.next = current.and_then(|s| self.select(s.music, calendar, ranges, false));
        changed
    }
    /// Source `radioCH.GetCurrentIndex`: talk index, or talk count plus music index. -1 for none.
    fn current_index(&self) -> i32 {
        let Some(s) = self.current else {
            return -1;
        };
        let (list, base) = if s.music {
            (&self.music, self.talk.len())
        } else {
            (&self.talk, 0)
        };
        list.iter()
            .position(|r| r.clip == s.recording.clip)
            .map_or(-1, |i| (base + i) as i32)
    }
    /// Source `radioCH.UpdFromMulti`.
    fn apply_host(&mut self, index: i32, offset: f32, seconds: f64) {
        let Ok(index) = usize::try_from(index) else {
            return;
        };
        if self.source.custom || !offset.is_finite() {
            return;
        }
        let selected = match index.checked_sub(self.talk.len()) {
            None => Selected {
                recording: self.talk[index],
                music: false,
            },
            Some(i) if i < self.music.len() => Selected {
                recording: self.music[i],
                music: true,
            },
            Some(_) => return,
        };
        self.current = Some(selected);
        self.next = None;
        self.start = seconds - f64::from(offset);
    }
    fn playback(&self, seconds: f64, gain: f32, primary: bool, custom_gain: f32) -> RadioPlayback {
        let clip_gain = if self.source.custom {
            custom_gain
        } else if primary {
            self.current.map_or(0., |s| s.recording.gain)
        } else {
            1.
        };
        RadioPlayback {
            station: Some(self.source.id),
            clip: self.current.map(|s| s.recording.clip),
            offset_seconds: self.current.map_or(0., |_| (seconds - self.start).max(0.)),
            gain: gain * clip_gain,
            next_clip: self.next.map(|s| s.recording.clip),
        }
    }
}
/// One xorshift step of `random`.
fn xorshift(random: &mut u32) -> u32 {
    *random ^= *random << 13;
    *random ^= *random >> 17;
    *random ^= *random << 5;
    *random
}
fn holiday(value: u8, calendar: [u8; 2], ranges: &[[u8; 4]; 2]) -> bool {
    if value == 0 {
        return true;
    }
    ranges
        .get(usize::from(value - 1))
        .is_some_and(|&range| in_holiday(calendar, range))
}
/// Source `mainscript.CheckHoliday` for one start and end date, both inclusive.
pub fn in_holiday([month, day]: [u8; 2], [sm, sd, em, ed]: [u8; 4]) -> bool {
    let date = u16::from(month) * 100 + u16::from(day);
    let start = u16::from(sm) * 100 + u16::from(sd);
    let end = u16::from(em) * 100 + u16::from(ed);
    if start <= end {
        (start..=end).contains(&date)
    } else {
        date >= start || date <= end
    }
}
impl RadioState {
    pub fn new(
        am_range: [f32; 3],
        fm_range: [f32; 3],
        volume_step: f32,
        band: bool,
        initial: crate::save::RadioSnapshot,
    ) -> Self {
        Self {
            on: initial.on,
            am: initial.am,
            tuning: initial.tuning,
            volume: initial.volume,
            am_range,
            fm_range,
            volume_step,
            band,
        }
    }
    fn range(&self) -> [f32; 3] {
        if self.am {
            self.am_range
        } else {
            self.fm_range
        }
    }
    /// radioscript.UpdValues FR.
    pub fn frequency(&self) -> f32 {
        let [min, max, step] = self.range();
        ((min + (max - min) * self.tuning.clamp(0., 1.)) / step).round_ties_even() * step
    }
    pub fn control(&mut self, control: RadioControl) {
        match control {
            RadioControl::VolumeUp => {
                self.volume = (self.volume + self.volume_step).clamp(0., 1.);
                if self.volume > 0. {
                    self.on = true;
                }
            }
            RadioControl::VolumeDown => {
                self.volume = (self.volume - self.volume_step).clamp(0., 1.);
                if self.volume == 0. {
                    self.on = false;
                }
            }
            RadioControl::Band => {
                if self.band {
                    self.am = !self.am;
                }
            }
            RadioControl::TuneUp | RadioControl::TuneDown => {
                let [min, max, step] = self.range();
                let delta = 1. / ((max - min) / step);
                let sign = if control == RadioControl::TuneUp {
                    1.
                } else {
                    -1.
                };
                self.tuning =
                    ((self.tuning + delta * sign).clamp(0., 1.) / delta).round_ties_even() * delta;
            }
        }
    }
    pub fn capture_save(&self) -> crate::save::RadioSnapshot {
        crate::save::RadioSnapshot {
            on: self.on,
            am: self.am,
            tuning: self.tuning,
            volume: self.volume,
        }
    }
    /// save_radioscript restores controls only; stations pick fresh recordings.
    pub fn restore_save(
        &mut self,
        saved: &crate::save::RadioSnapshot,
    ) -> Result<(), crate::save::SaveError> {
        if saved.am && !self.band {
            return Err(crate::save::SaveError::CorruptData(
                "Radio band without band control".into(),
            ));
        }
        self.on = saved.on;
        self.am = saved.am;
        self.tuning = saved.tuning;
        self.volume = saved.volume;
        Ok(())
    }
}

impl RadioBroadcasts {
    pub fn new(
        reference: &RadioAudioReference,
        clips: &[AudioClipReference],
        calendar: [u8; 2],
    ) -> Self {
        let stations = reference
            .stations
            .iter()
            .map(|s| {
                let recordings = |ids: &[u32], gains: &[f32], holidays: &[u8]| {
                    ids.iter()
                        .enumerate()
                        .map(|(i, &clip)| Recording {
                            clip,
                            duration: clips
                                .iter()
                                .find(|c| c.id == clip)
                                .expect("validated radio clip")
                                .source_duration_seconds,
                            gain: gains.get(i).copied().filter(|&v| v != 0.).unwrap_or(1.),
                            holiday: holidays[i],
                        })
                        .collect::<Vec<_>>()
                };
                let music = recordings(&s.music, &s.music_gains, &s.music_holidays);
                let talk = recordings(&s.talk, &s.talk_gains, &s.talk_holidays);
                Station {
                    source: s.clone(),
                    music_bag: Vec::with_capacity(music.len()),
                    talk_bag: Vec::with_capacity(talk.len()),
                    music,
                    talk,
                    current: None,
                    next: None,
                    start: 0.,
                    random: s.id.wrapping_mul(0x9e3779b9),
                }
            })
            .collect();
        Self {
            stations,
            fm_tolerance: reference.fm_tolerance,
            am_tolerance: reference.am_tolerance,
            custom_gain: reference.custom_gain,
            holiday_ranges: reference.holiday_ranges,
            calendar,
            remote: false,
            changed: false,
        }
    }
    /// A multiplayer guest takes its clips from the host. Leaving the session selects again.
    pub fn set_remote(&mut self, remote: bool) {
        self.remote = remote;
    }
    /// True once after a station took a new clip.
    pub fn take_changed(&mut self) -> bool {
        std::mem::take(&mut self.changed)
    }
    /// Source `RadioChannelMultiplayerUpd`: the clip index and clip time of each station.
    pub fn host_channels(&self, seconds: f64) -> impl ExactSizeIterator<Item = (i32, f32)> + '_ {
        self.stations.iter().map(move |s| {
            let index = s.current_index();
            let offset = if index < 0 {
                0.
            } else {
                (seconds - s.start) as f32
            };
            (index, offset)
        })
    }
    /// Source `RadioChannelFromMultiplayerUpd`. A list for another station count is skipped.
    pub fn apply_host(
        &mut self,
        channels: impl ExactSizeIterator<Item = (i32, f32)>,
        seconds: f64,
    ) {
        if channels.len() != self.stations.len() {
            return;
        }
        for (station, (index, offset)) in self.stations.iter_mut().zip(channels) {
            station.apply_host(index, offset, seconds);
        }
    }
    pub fn calendar(&mut self, month: u8, day: u8) {
        self.calendar = [month, day];
    }
    /// mainscript.RADIO: every station advances once per step, heard or not.
    pub fn advance(&mut self, game_seconds: f64) {
        if self.remote {
            return;
        }
        for s in &mut self.stations {
            self.changed |= s.advance(game_seconds, self.calendar, &self.holiday_ranges);
        }
    }
    /// radioscript.SetRadio gains for one radio.
    pub fn frame(
        &self,
        radio: &RadioState,
        game_seconds: f64,
        electronics_output: f32,
        electronics_cutoff: f32,
    ) -> RadioFrame {
        let frequency = radio.frequency();
        let power = radio.on && electronics_output > electronics_cutoff;
        let mut nearest = f32::MAX;
        let mut secondary_distance = f32::MAX;
        let mut primary = None;
        let mut secondary = None;
        for (i, s) in self.stations.iter().enumerate() {
            if (s.source.band == "am") != radio.am {
                continue;
            }
            let d = (s.source.frequency - frequency).abs();
            if d < nearest {
                if primary.is_some() {
                    secondary = primary;
                    secondary_distance = nearest;
                }
                primary = Some(i);
                nearest = d;
            }
        }
        let tolerance = if radio.am {
            self.am_tolerance
        } else {
            self.fm_tolerance
        };
        let signal = |d: f32| (1. - d / tolerance).clamp(0., 1.) * electronics_output;
        let gain = if power { radio.volume } else { 0. };
        RadioFrame {
            power,
            am: radio.am,
            frequency,
            volume: radio.volume,
            static_gain: (1. - signal(nearest)) * gain,
            primary: primary.map_or(RadioPlayback::default(), |i| {
                self.stations[i].playback(
                    game_seconds,
                    signal(nearest) * gain,
                    true,
                    self.custom_gain,
                )
            }),
            secondary: secondary.map_or(RadioPlayback::default(), |i| {
                self.stations[i].playback(
                    game_seconds,
                    signal(secondary_distance) * gain,
                    false,
                    self.custom_gain,
                )
            }),
        }
    }
    /// mainscript.Update advances FM stations while the sleep loop gain exceeds 0.5.
    pub fn advance_sleep(&mut self, seconds: f64) {
        if self.remote {
            return;
        }
        self.changed = true;
        let mut iteration = 0;
        loop {
            // Unity redraws Random.Range(1, 6) at each loop condition.
            let Some(first) = self.stations.first_mut() else {
                return;
            };
            let limit = 1 + ((u64::from(xorshift(&mut first.random)) * 5) >> 32) as usize;
            if iteration >= limit {
                break;
            }
            for station in &mut self.stations {
                if station.source.band != "fm" || station.source.custom {
                    continue;
                }
                let music = station
                    .current
                    .map_or(station.source.initial_music, |s| s.music);
                station.current = station
                    .next
                    .or_else(|| station.select(music, self.calendar, &self.holiday_ranges, true));
                station.start = seconds;
                station.next = station.current.and_then(|s| {
                    station.select(s.music, self.calendar, &self.holiday_ranges, false)
                });
            }
            iteration += 1;
        }
    }
}
