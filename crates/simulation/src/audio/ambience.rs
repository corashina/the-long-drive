//! Source rock heat sounds (`soundplayscript`) and ambience zones (`ambienceZone`).
use super::{AudioAction, AudioKind, AudioState, SurvivalAudioEvent, xorshift};
use crate::lighting::{CycleOutput, CyclePhase};
use crate::reference::{AmbienceReference, AmbienceZoneReference};
use crate::world::{GeneratedId, StructureInstance, place_point};
use std::collections::HashMap;

const CAPACITY: usize = 256;

struct RockTimer {
    ambience: usize,
    position: [f64; 3],
    last: f64,
    wait: f64,
    seen: bool,
}

/// One source timer per published placement of an ambience prototype.
pub struct RockAmbience {
    timers: HashMap<GeneratedId, RockTimer>,
    revision: Option<u64>,
}

impl Default for RockAmbience {
    fn default() -> Self {
        Self {
            timers: HashMap::with_capacity(CAPACITY),
            revision: None,
        }
    }
}

/// True when `lerp` is inside the source window of the current phase.
fn window(a: &AmbienceReference, phase: CyclePhase, lerp: f32) -> bool {
    let w = match phase {
        CyclePhase::Dawn => a.phases.morning,
        CyclePhase::Day => a.phases.day,
        CyclePhase::Dusk => a.phases.dusk,
        CyclePhase::Night => a.phases.night,
    };
    w.is_some_and(|[min, max]| lerp > min && lerp < max)
}

impl RockAmbience {
    pub fn reset(&mut self) {
        self.timers.clear();
        self.revision = None;
    }
    /// A new timer tries on its first step inside a window, as the source timer starts at zero.
    fn add(&mut self, id: &GeneratedId, ambience: usize, position: [f64; 3], now: f64) {
        if let Some(timer) = self.timers.get_mut(id) {
            timer.seen = true;
        } else if self.timers.len() < CAPACITY {
            let timer = RockTimer {
                ambience,
                position,
                last: now,
                wait: 0.,
                seen: true,
            };
            self.timers.insert(id.clone(), timer);
        }
    }
    /// Matches the timers to the published structures when the stream revision changes.
    pub fn sync(
        &mut self,
        revision: u64,
        structures: &[StructureInstance],
        ambience: &[AmbienceReference],
        now: f64,
    ) {
        if self.revision == Some(revision) {
            return;
        }
        self.revision = Some(revision);
        for timer in self.timers.values_mut() {
            timer.seen = false;
        }
        for s in structures {
            let p = &s.placement;
            if let Some(i) = ambience.iter().position(|a| a.prototype == p.prototype) {
                self.add(&p.id, i, p.pose.position, now);
            }
        }
        self.timers.retain(|_, t| t.seen);
    }
    #[allow(clippy::too_many_arguments)]
    fn step(
        &mut self,
        ambience: &[AmbienceReference],
        phase: CyclePhase,
        lerp: f32,
        now: f64,
        listener: [f64; 3],
        range: f32,
        random: &mut u32,
        mut emit: impl FnMut([f64; 3], u32, f32),
    ) {
        let range = f64::from(range);
        for t in self.timers.values_mut() {
            let d: f64 = (0..3).map(|i| (t.position[i] - listener[i]).powi(2)).sum();
            let a = &ambience[t.ambience];
            // The source tests the phase before it resets the timer.
            if d > range * range || now - t.last <= t.wait || !window(a, phase, lerp) {
                continue;
            }
            t.last = now;
            t.wait = f64::from(a.interval[0] + (a.interval[1] - a.interval[0]) * xorshift(random));
            if xorshift(random) > a.chance {
                continue;
            }
            let index = ((xorshift(random) * a.clips.len() as f32) as usize).min(a.clips.len() - 1);
            let gain = a.gain[0] + (a.gain[1] - a.gain[0]) * xorshift(random);
            emit(t.position, a.clips[index], gain);
        }
    }
}

const ZONE_CAPACITY: usize = 16;

struct Zone {
    id: GeneratedId,
    zone: usize,
    position: [f64; 3],
    playing: bool,
    seen: bool,
}

/// One source zone per published placement. The slot index is the loop emitter.
pub struct ZoneAmbience {
    slots: [Option<Zone>; ZONE_CAPACITY],
    revision: Option<u64>,
}

impl Default for ZoneAmbience {
    fn default() -> Self {
        Self {
            slots: std::array::from_fn(|_| None),
            revision: None,
        }
    }
}

/// The result of one source `FixedUpdate` for one zone slot.
#[derive(Debug, PartialEq)]
enum ZoneStep {
    Idle,
    Play {
        gain: f32,
        lerp: f32,
        position: [f64; 3],
    },
    Stop,
}

impl ZoneAmbience {
    pub fn reset(&mut self) {
        self.slots = std::array::from_fn(|_| None);
        self.revision = None;
    }
    /// Matches the zones to the published structures. Calls `stop` for each removed playing zone.
    fn sync(
        &mut self,
        revision: u64,
        structures: &[StructureInstance],
        zones: &[AmbienceZoneReference],
        mut stop: impl FnMut(u32),
    ) {
        if self.revision == Some(revision) {
            return;
        }
        self.revision = Some(revision);
        for z in self.slots.iter_mut().flatten() {
            z.seen = false;
        }
        for s in structures {
            let p = &s.placement;
            for (i, zone) in zones.iter().enumerate() {
                if zone.prototype != p.prototype {
                    continue;
                }
                let slots = &mut self.slots;
                if let Some(z) = slots
                    .iter_mut()
                    .flatten()
                    .find(|z| z.zone == i && z.id == p.id)
                {
                    z.seen = true;
                } else if let Some(slot) = slots.iter_mut().find(|z| z.is_none()) {
                    *slot = Some(Zone {
                        id: p.id.clone(),
                        zone: i,
                        position: place_point(p.pose, p.scale, zone.pose.translation),
                        playing: false,
                        seen: true,
                    });
                }
            }
        }
        for (emitter, slot) in self.slots.iter_mut().enumerate() {
            if slot.as_ref().is_some_and(|z| !z.seen) && slot.take().is_some_and(|z| z.playing) {
                stop(emitter as u32);
            }
        }
    }
    /// Source order: a stopped zone starts on this step and sets its gain on the next step.
    fn step(
        &mut self,
        slot: usize,
        zones: &[AmbienceZoneReference],
        listener: [f64; 3],
    ) -> ZoneStep {
        let Some(z) = &mut self.slots[slot] else {
            return ZoneStep::Idle;
        };
        let zone = &zones[z.zone];
        let d = (0..3)
            .map(|i| (z.position[i] - listener[i]).powi(2))
            .sum::<f64>()
            .sqrt() as f32;
        // Mathf.InverseLerp(maxDist, minDist, distance).
        let lerp =
            ((d - zone.max_distance) / (zone.min_distance - zone.max_distance)).clamp(0., 1.);
        if z.playing {
            if lerp == 0. {
                z.playing = false;
                return ZoneStep::Stop;
            }
            return ZoneStep::Play {
                gain: zone.volume * lerp,
                lerp,
                position: z.position,
            };
        }
        z.playing = lerp > 0.;
        ZoneStep::Idle
    }
}

impl AudioState {
    pub fn sync_zones(&mut self, revision: u64, structures: &[StructureInstance]) {
        let loops = &mut self.survival_loops;
        self.zones.sync(
            revision,
            structures,
            &self.reference.ambience_zones,
            |emitter| loops.retain(|v| v.kind != AudioKind::AmbienceZone || v.emitter != emitter),
        );
    }
    /// Plays the zones near `listener`. Returns the largest playing lerp for the ambient duck.
    pub fn zone_step(&mut self, listener: [f64; 3]) -> f32 {
        let mut duck = 0f32;
        for slot in 0..ZONE_CAPACITY {
            match self
                .zones
                .step(slot, &self.reference.ambience_zones, listener)
            {
                ZoneStep::Idle => {}
                ZoneStep::Play {
                    gain,
                    lerp,
                    position,
                } => {
                    duck = duck.max(lerp);
                    self.source_loop(AudioKind::AmbienceZone, slot as u32, gain, position);
                }
                ZoneStep::Stop => self.stop_survival_loop(AudioKind::AmbienceZone, slot as u32),
            }
        }
        duck
    }
}

impl AudioState {
    pub fn sync_rocks(&mut self, revision: u64, structures: &[StructureInstance]) {
        let now = self.game_seconds;
        self.rocks
            .sync(revision, structures, &self.reference.ambience, now);
    }
    /// Emits rock heat sounds for timers within the shared hearing range of `listener`.
    pub fn rock_step(&mut self, cycle: &CycleOutput, listener: [f64; 3]) {
        let session = self.session;
        let events = &mut self.survival_events;
        self.rocks.step(
            &self.reference.ambience,
            cycle.phase,
            cycle.progress,
            self.game_seconds,
            listener,
            self.reference.shared.settings.max_distance,
            &mut self.random,
            |position, clip, gain| {
                if events.len() < events.capacity() {
                    events.push(SurvivalAudioEvent {
                        session,
                        emitter: 0,
                        kind: AudioKind::RockHeat,
                        action: AudioAction::Start,
                        clip,
                        gain,
                        pitch: 1.,
                        position,
                    });
                }
            },
        );
    }
}
