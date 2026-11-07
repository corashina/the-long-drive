//! Source `fpscontroller` hum. `IN.hum` plays the start clip of the tune, then loops its hum
//! clip. Scroll selects the tune. The mouse sets the volume and the pitch.
//! `humStimul.SetAudioValue` lets rabbits hear it.
use crate::reference::{AudioReference, HumReference};

/// Source `humSource.clip.length * 0.75f`: the start clip part before the hum clip.
const START_FRACTION: f64 = 0.75;
/// Source `humVolume < 0.01f` stops the source.
const STOP_VOLUME: f32 = 0.01;

/// One source `Update` input.
#[derive(Clone, Copy, Debug, Default)]
pub struct HumInput {
    /// `input.hum && !isSmoking`.
    pub held: bool,
    /// `input.lmb` holds the volume and the pitch.
    pub lmb: bool,
    /// Source `input.lookX` and `input.lookY`. Y is up.
    pub look: [f32; 2],
    /// Source `input.scroll`.
    pub scroll: f32,
}

/// The `humSource` state.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct HumSound {
    /// Index in the hum audio row: tune `i` start clip is `i`, its hum clip is `tunes + i`.
    pub clip: usize,
    pub volume: f32,
    pub pitch: f32,
}

pub struct Hum {
    /// Start clip seconds, one for each tune.
    start_seconds: Vec<f64>,
    selected: usize,
    humming: bool,
    start: bool,
    switched: bool,
    start_time: f64,
    note: f32,
    /// Source serialized `humPitch` is 0.
    pitch: f32,
    volume: f32,
    input_volume: f32,
    clip: usize,
    playing: bool,
}

impl Hum {
    pub fn new(reference: &HumReference, audio: &AudioReference) -> Self {
        let clips = &audio
            .survival_row(crate::audio::AudioKind::Hum as u32)
            .clips;
        let start_seconds = clips[..reference.stimulus_values.len()]
            .iter()
            .map(|id| {
                audio
                    .clips
                    .iter()
                    .find(|c| c.id == *id)
                    .expect("validated hum clip")
                    .duration_seconds
            })
            .collect();
        Self::with_start_seconds(reference, start_seconds)
    }

    fn with_start_seconds(reference: &HumReference, start_seconds: Vec<f64>) -> Self {
        Self {
            start_seconds,
            selected: 0,
            humming: false,
            start: false,
            switched: false,
            start_time: 0.,
            note: 0.,
            pitch: 0.,
            volume: 0.,
            // Source `Start`.
            input_volume: reference.max_volume * 0.5,
            clip: 0,
            playing: false,
        }
    }

    /// Source `humming`. Look and duck do not run.
    pub fn humming(&self) -> bool {
        self.humming
    }

    /// The playing source, if any.
    pub fn sound(&self) -> Option<HumSound> {
        self.playing.then_some(HumSound {
            clip: self.clip,
            volume: self.volume,
            pitch: self.pitch,
        })
    }

    /// Source `SetAudioValue(humSource.volume * humStimulValues[selectedHum])`.
    pub fn stimulus(&self, reference: &HumReference) -> f32 {
        self.volume * reference.stimulus_values[self.selected]
    }

    pub fn step(&mut self, r: &HumReference, input: HumInput, now: f64, dt: f32) {
        let t = (dt * r.lerp).clamp(0., 1.);
        let tunes = self.start_seconds.len();
        if input.held {
            if !self.humming || self.switched {
                self.switched = false;
                self.clip = self.selected;
                self.start = true;
                self.start_time = now + self.start_seconds[self.selected] * START_FRACTION;
            } else if self.start && now >= self.start_time {
                self.start = false;
                self.clip = tunes + self.selected;
            }
            self.humming = true;
            self.playing = true;
            if input.scroll > 0. && self.selected < tunes - 1 {
                self.selected += 1;
                self.switched = true;
            }
            if input.scroll < 0. && self.selected > 0 {
                self.selected -= 1;
                self.switched = true;
            }
            if !input.lmb {
                self.input_volume = (self.input_volume + input.look[0] * r.volume_sensitivity)
                    .clamp(0., r.max_volume);
                let target =
                    (self.note + input.look[1] * r.pitch_sensitivity).clamp(r.note[0], r.note[1]);
                self.note += (target - self.note) * t;
                self.pitch = 2f32.powf(self.note / 12.);
            }
            self.volume += (self.input_volume - self.volume) * t;
        } else {
            self.volume -= self.volume * t;
            self.humming = false;
            if self.volume < STOP_VOLUME {
                self.playing = false;
            }
        }
    }
}
