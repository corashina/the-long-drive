//! Selected source audio. Validation has a separate gameplay failure boundary.
use super::{
    CarAudioReference, CarShotReference, CarSoundReference, ColliderId, CurveKey,
    EngineAudioReference, ItemId, LightControlId, MechanismId, NodeId, PartSlotId, ReferenceError,
    ReferenceManifest, SourceId, Transform, hash, ids, require,
};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashSet};

macro_rules! record {
    ($name:ident { $($field:ident: $ty:ty),* $(,)? }) => {
        #[derive(Clone, Debug, Serialize, Deserialize)]
        #[serde(deny_unknown_fields)]
        pub struct $name { $(pub $field: $ty),* }
    };
}
record!(AudioMethod {
    class: String,
    method: String,
    file: String,
    line_start: u32,
    line_end: u32,
    sha256: String
});
record!(AudioClipReference {
    id: u32, source: SourceId, usages: Vec<String>, path: String, sha256: String, bytes: u64,
    format: String, format_tag: u16, channels: u16, frequency: u32, bits_per_sample: u16,
    block_align: u16, data_bytes: u64, frames: u64, duration_seconds: f64,
    source_duration_seconds: f64, duration_delta_seconds: f64, duration_delta_frames: f64,
    duration_tolerance_frames: f64, duration_status: String
});
record!(AudioEmitterReference { id: u32, source: SourceId, node: NodeId, item: Option<ItemId> });
record!(AudioCurve { keys: Vec<CurveKey>, pre_wrap_mode: i32, post_wrap_mode: i32, rotation_order: i32 });
record!(AudioVoiceReference {
    id: u32, source: SourceId, emitter: u32, clip: Option<u32>,
    authored_clip: Option<u32>, authored_priority: u8, authored_loop: bool, mixer: u32, priority: u8,
    r#loop: bool, gain: f32, pitch: f32, spatial_blend: f32, pan: f32,
    min_distance: f32, max_distance: f32, rolloff_mode: u8, doppler: f32,
    play_on_awake: bool, mute: bool, enabled: bool,
    curves: BTreeMap<String, AudioCurve>, methods: Vec<AudioMethod>
});
record!(AudioMixerEffect {
    r#type: i32, name: String, bypass: bool, parameter_indices: Vec<u32>, parameters: Vec<f32>,
    previous_effect: u32, send_target: u32, wet_mix_index: u32
});
record!(AudioMixerReference {
    id: u32, source: SourceId, controller: SourceId, snapshot: SourceId, name: String,
    parent: Option<u32>, gain_db: f32, pitch: f32, mute: bool, solo: bool,
    bypass_effects: bool, effects: Vec<AudioMixerEffect>
});
record!(AudioEventBinding {
    id: u32, source: SourceId, trigger: String, emitter: u32, clips: Vec<u32>, gain: f32,
    pitch_range: [f32; 2], priority: u8, mixer: u32, voice: Option<u32>, method: AudioMethod
});
record!(AudioControlReference { source: SourceId, node: NodeId, mechanism: Option<MechanismId> });
record!(RadioStationReference {
    id: u32, band: String, frequency: f32, name: String, custom: bool, content: String,
    initial_music: bool, start_time: f32, initial_time: f32, initial_max_time: f32, initial_gain: f32,
    music: Vec<u32>, talk: Vec<u32>, music_gains: Vec<f32>, talk_gains: Vec<f32>,
    music_holidays: Vec<u8>, talk_holidays: Vec<u8>
});
record!(RadioClockReference {
    kind: String,
    selected: AudioMethod,
    music_start: AudioMethod,
    talk_start: AudioMethod,
    multiplayer: AudioMethod
});
// radioscript settings and stations shared by all world radios.
record!(RadioAudioReference {
    constants: BTreeMap<String, f32>, fm_tolerance: f32, am_tolerance: f32,
    electronics_cutoff: f32, time_correction_seconds: f32, late_seek_margin_seconds: f32,
    custom_gain: f32, clock: RadioClockReference, stations: Vec<RadioStationReference>, methods: Vec<AudioMethod>,
    tuning_repeat_seconds: f32, holiday_ranges: [[u8;4];2]
});
// radioscript SS, SC and SC2 settings shared by the world radios in `sources` (portable and car radios).
record!(PortableRadioAudioReference { sources: Vec<SourceId>, voices: [AudioVoiceReference; 3] });
// The stimulscript of one radioscript voice (0 SS, 1 SC, 2 SC2). `offset` is in the radio item frame.
record!(RadioStimulusReference {
    radio: SourceId,
    voice: u8,
    offset: [f32; 3],
    max_sound_value: f32
});
record!(MechanismAudioReference {
    source: SourceId, emitter: u32, movement_voice: Option<u32>, audio_multiplier: f32,
    slide_multiplier: f32, rotation_gain: f32, slide_gain: f32, gain_smoothing: f32,
    slide_stop_seconds: f32, squeak_chance: f32, pitch_range: [f32; 2],
    endpoints: Vec<u32>, turn_binding: Option<u32>
});
record!(FluidAudioReference {
    source: SourceId, emitter: u32, fill_voice: Option<u32>, max_gain: f32, tune_speed: f32, fluid_tune_speed: f32,
    pour_speed_min: f32, pour_speed_max: f32, fill_loop: bool, fill_latch_steps: u32,
    open_binding: Option<u32>, close_binding: Option<u32>, tune_binding: Option<u32>, method: AudioMethod
});
record!(LampAudioReference { source: SourceId, control: LightControlId, voices: Vec<u32>, on_binding: Option<u32>, off_binding: Option<u32>, method: AudioMethod });
record!(AudioManagedEvidence {
    file: String,
    sha256: String,
    decompiled_sha256: String
});
record!(AudioNativeEvidence {
    file: String, sha256: String, effect_type: i32, registration_va: String,
    type_write_va: String, coefficient_va: String, parameter_names: Vec<String>,
    browser_q_rule: String, browser_gain_db_rule: String
});
record!(AudioManagerReference {
    source: SourceId,
    real_voices: u32,
    virtual_voices: u32,
    virtualize_effects: bool,
    doppler_factor: f32,
    rolloff_scale: f32
});
record!(AudioSourceSettings {
    priority: u8,
    r#loop: bool,
    gain: f32,
    pitch: f32,
    min_distance: f32,
    max_distance: f32,
    rolloff_mode: u8,
    spatial_blend: f32,
    spatial_blend_curve: AudioCurve,
    rolloff_curve: AudioCurve
});
record!(AudioSensorReference { source: SourceId, transform_source: SourceId, node: Option<NodeId>,
    player: bool, initial_sensed: bool, initial_count: u32, radius: Option<f32>, mask: Option<u32>,
    method: AudioMethod, local_position: Option<[f32; 3]>, parent_source: Option<SourceId> });
record!(AudioInteriorReference { id: u32, source: SourceId, node: NodeId, collider: ColliderId,
    parent: Option<u32>, invert: bool, minimum_doors_to_max: u32, maximum_transmission: f32,
    removable_doors: bool, use_value: bool, value: f32, doors: Vec<AudioControlReference>,
    door_slots: Vec<PartSlotId>, invert_doors: Vec<bool>, invert_slots: Vec<bool>,
    parent_door: Option<AudioControlReference>, parent_slot: Option<PartSlotId>, method: AudioMethod });
record!(AudioRouteReference { source: SourceId, voices: Vec<u32>, interiors: Vec<u32>,
    sensor: AudioSensorReference, main_mixer: u32, outside_mixer: u32, car_mixer: Option<u32>,
    initial_outside: bool, initial_player_inside: bool, methods: Vec<AudioMethod> });
record!(AudioSharedReference {
    audio_manager: AudioManagerReference,
    priorities: Vec<u8>, random_pitch: [f32; 2], declared_max_audio_count: u32,
    enforced_voice_limit: Option<u32>, gain_above_one: String, destroy_after_source_duration: bool,
    method: AudioMethod, source: SourceId, settings: AudioSourceSettings, mixer: u32
});
record!(AudioOutsideReference { default_interior_gain: f32, gain_smoothing: f32, car_sound_multiplier: f32, initial_gain: f32, sensor: AudioSensorReference, methods: Vec<AudioMethod> });
record!(AudioMixerControls { outside: AudioOutsideReference, source: SourceId, parameter_indices: Vec<u32>, parameter_name_hashes: Vec<u32>, parameter_names: Vec<String>, methods: Vec<AudioMethod> });
record!(AudioReference {
    schema_version: u32, clips: Vec<AudioClipReference>, emitters: Vec<AudioEmitterReference>,
    voices: Vec<AudioVoiceReference>, car_voices: Vec<CarVoiceReference>, mixers: Vec<AudioMixerReference>,
    bindings: Vec<AudioEventBinding>, radio: RadioAudioReference,
    portable_radios: Vec<PortableRadioAudioReference>,
    radio_stimuli: Vec<RadioStimulusReference>,
    mechanisms: Vec<MechanismAudioReference>, fluids: Vec<FluidAudioReference>,
    interiors: Vec<AudioInteriorReference>, routes: Vec<AudioRouteReference>, lamps: Vec<LampAudioReference>,
    silent_actions: Vec<String>, managed: AudioManagedEvidence, native: AudioNativeEvidence,
    shared: AudioSharedReference, mixer_controls: AudioMixerControls,
    survival: Vec<SurvivalAudioReference>, collisions: CollisionAudioReference,
    ambience: Vec<AmbienceReference>, ambience_zones: Vec<AmbienceZoneReference>,
    footsteps: FootstepAudioReference
});
// A voice template of car and engine part sounds. Car records name it by id.
record!(CarVoiceReference {
    id: u32, source: SourceId, one_shot: bool, clip: Option<u32>, settings: AudioSourceSettings,
    mixer: u32, doppler: f32, pan: f32
});
record!(CarTierReference { threshold: f32, clips: Vec<u32>, fixed_gain: Option<f32> });
record!(CollisionAudioReference {
    car_tiers: [CarTierReference; 4], interior_check_seconds: f32, interior_radius: f32,
    loose_multiplier: f32, item_minimum: f32, car_multiplier: f32, method: Vec<AudioMethod>
});
record!(FootstepSurfaceReference { name: String, volume: f32, clips: Vec<u32> });
// Source `stepSoundScript`. Surfaces are in source order: road, desert, winter, grass, wood.
record!(FootstepAudioReference {
    surfaces: [FootstepSurfaceReference; 5], step_gain: f32, ray_distance: f32, ray_mask: u32,
    local_position: [f32; 3], sand_layers: Vec<u32>, method: Vec<AudioMethod>
});
/// Lerp windows per cycle phase. A missing phase never plays.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct AmbiencePhases {
    pub dusk: Option<[f32; 2]>,
    pub night: Option<[f32; 2]>,
    pub morning: Option<[f32; 2]>,
    pub day: Option<[f32; 2]>,
}
record!(AmbienceReference {
    prototype: SourceId, clips: Vec<u32>, phases: AmbiencePhases, interval: [f32; 2],
    chance: f32, gain: [f32; 2], pitch: [f32; 2], priority: u8, method: AudioMethod
});
// Source `ambienceZone`: a 2D loop with gain volume * InverseLerp(max_distance, min_distance, distance).
// The pose is in the prototype root frame.
record!(AmbienceZoneReference {
    prototype: SourceId,
    pose: Transform,
    min_distance: f32,
    max_distance: f32,
    volume: f32,
    clip: u32,
    method: AudioMethod
});
/// Zones play through the one looped 2D survival row of kind 19.
fn validate_zones(
    zones: &[AmbienceZoneReference],
    survival: &[SurvivalAudioReference],
) -> Result<(), ReferenceError> {
    let row = survival.iter().find(|b| b.kind == 19);
    require(
        zones.is_empty() == row.is_none()
            && row.is_none_or(|b| {
                b.role == "ambience_zone"
                    && !b.one_shot
                    && b.settings.r#loop
                    && b.settings.spatial_blend == 0.
            }),
        "ambience zone playback",
    )?;
    for z in zones {
        require(
            values([z.min_distance, z.max_distance, z.volume])
                && values(
                    z.pose
                        .translation
                        .into_iter()
                        .chain(z.pose.rotation)
                        .chain(z.pose.scale),
                )
                && 0. <= z.min_distance
                && z.min_distance < z.max_distance
                && (0. ..=1.).contains(&z.volume)
                && row.is_some_and(|b| b.clips.contains(&z.clip))
                && method(&z.method),
            "ambience zone",
        )?;
    }
    Ok(())
}

fn values(v: impl IntoIterator<Item = f32>) -> bool {
    v.into_iter().all(f32::is_finite)
}
pub(super) fn method(m: &AudioMethod) -> bool {
    !m.class.is_empty()
        && !m.method.is_empty()
        && m.file == "managed/Assembly-CSharp.cs"
        && m.line_start > 0
        && m.line_end >= m.line_start
        && hash(&m.sha256)
}
impl AudioReference {
    pub fn from_value(
        raw: &serde_json::Value,
        game: &ReferenceManifest,
    ) -> Result<Self, ReferenceError> {
        let audio: Self = serde_json::from_value(raw.clone())
            .map_err(|e| ReferenceError(format!("Audio: {e}")))?;
        audio.validate(game)?;
        Ok(audio)
    }
    fn validate(&self, game: &ReferenceManifest) -> Result<(), ReferenceError> {
        require(
            self.schema_version == 1 && !self.clips.is_empty(),
            "audio schema",
        )?;
        let clips = ids(self.clips.iter().map(|v| v.id), "audio clip IDs")?;
        let emitters = ids(self.emitters.iter().map(|v| v.id), "audio emitter IDs")?;
        let voices = ids(self.voices.iter().map(|v| v.id), "audio voice IDs")?;
        let mixers = ids(self.mixers.iter().map(|v| v.id), "audio mixer IDs")?;
        let bindings = ids(self.bindings.iter().map(|v| v.id), "audio binding IDs")?;
        for set in [&clips, &emitters, &voices, &mixers, &bindings] {
            require(!set.contains(&0), "audio zero ID")?;
        }
        for c in &self.clips {
            let block_align = c.channels.checked_mul(c.bits_per_sample / 8);
            let data_bytes = c.frames.checked_mul(u64::from(c.block_align));
            require(
                !c.usages.is_empty() && c.usages.iter().all(|u| u == "effect" || u == "radio"),
                "audio clip usage",
            )?;
            require(
                hash(&c.sha256)
                    && c.source.path_id != 0
                    && c.path
                        == format!("reference/audio-{}-{}.wav", c.source.file, c.source.path_id)
                    && !c.source.file.contains(['/', '\\', ':'])
                    && c.channels > 0
                    && c.frequency > 0
                    && c.frames > 0
                    && c.bits_per_sample > 0
                    && c.bits_per_sample % 8 == 0
                    && block_align == Some(c.block_align)
                    && data_bytes == Some(c.data_bytes)
                    && c.bytes > c.data_bytes
                    && [
                        c.duration_seconds,
                        c.source_duration_seconds,
                        c.duration_delta_seconds,
                        c.duration_delta_frames,
                        c.duration_tolerance_frames,
                    ]
                    .iter()
                    .all(|v| v.is_finite())
                    && c.duration_seconds > 0.0
                    && c.source_duration_seconds > 0.0
                    && (c.duration_seconds - c.frames as f64 / f64::from(c.frequency)).abs() < 1e-9
                    && c.duration_delta_frames.abs() <= c.duration_tolerance_frames,
                "audio clip samples",
            )?;
        }
        for e in &self.emitters {
            require(
                game.nodes.iter().any(|n| n.id == e.node)
                    && e.item.is_none_or(|i| game.items.iter().any(|v| v.id == i)),
                "audio emitter owner",
            )?;
        }
        let valid_curve = |curve: &AudioCurve| {
            !curve.keys.is_empty()
                && curve.keys.iter().all(|k| {
                    values([
                        k.time,
                        k.value,
                        k.in_tangent,
                        k.out_tangent,
                        k.in_weight,
                        k.out_weight,
                    ]) && k.weighted_mode <= 3
                })
                && curve.keys.windows(2).all(|ks| ks[0].time < ks[1].time)
        };
        let mut radio_sources = HashSet::new();
        require(
            !self.portable_radios.is_empty()
                && self.portable_radios.iter().all(|g| {
                    !g.sources.is_empty() && g.sources.iter().all(|s| radio_sources.insert(s))
                }),
            "world radio voice groups",
        )?;
        require(
            self.radio_stimuli.iter().all(|r| {
                radio_sources.contains(&r.radio)
                    && r.voice < 3
                    && r.offset.iter().all(|v| v.is_finite())
                    && r.max_sound_value >= 0.
            }),
            "radio stimuli",
        )?;
        let portable = self
            .portable_radios
            .iter()
            .flat_map(|g| &g.voices)
            .map(|v| (true, v));
        for (portable, v) in self.voices.iter().map(|v| (false, v)).chain(portable) {
            require(
                (if portable {
                    v.id == 0 && v.emitter == 0
                } else {
                    emitters.contains(&v.emitter)
                }) && mixers.contains(&v.mixer)
                    && v.clip.is_none_or(|c| clips.contains(&c))
                    && values([
                        v.gain,
                        v.pitch,
                        v.spatial_blend,
                        v.min_distance,
                        v.max_distance,
                        v.pan,
                        v.doppler,
                    ])
                    && v.gain >= 0.0
                    && v.pitch > 0.0
                    && (0.0..=1.0).contains(&v.spatial_blend)
                    // Linear rolloff is defined from distance 0. The airplane source starts there.
                    && (v.min_distance > 0.0 || (v.min_distance == 0.0 && v.rolloff_mode == 1))
                    && v.max_distance >= v.min_distance
                    && v.max_distance > 0.0
                    && v.rolloff_mode <= 2
                    && !v.methods.is_empty()
                    && v.methods.iter().all(method),
                "audio voice",
            )?;
            for name in ["rolloff", "spatial_blend", "spread", "reverb_mix"] {
                let curve = v
                    .curves
                    .get(name)
                    .ok_or_else(|| ReferenceError("Missing audio curve".into()))?;
                require(valid_curve(curve), "audio curve")?;
            }
        }
        for m in &self.mixers {
            require(
                m.parent.is_none_or(|p| mixers.contains(&p) && p != m.id)
                    && values([m.gain_db, m.pitch])
                    && m.pitch > 0.0,
                "audio mixer",
            )?;
            let mut current = m;
            let mut count = 0;
            while let Some(id) = current.parent {
                count += 1;
                require(count <= self.mixers.len(), "audio mixer cycle")?;
                current = self
                    .mixers
                    .iter()
                    .find(|m| m.id == id)
                    .ok_or_else(|| ReferenceError("Missing audio mixer ancestor".into()))?;
            }
            for effect in &m.effects {
                require(
                    effect.r#type == -2 || effect.r#type == 10,
                    "unsupported audio mixer effect",
                )?;
                require(
                    values(effect.parameters.iter().copied())
                        && effect.parameter_indices.len() == effect.parameters.len(),
                    "audio effect parameters",
                )?;
                if effect.r#type == 10 {
                    require(
                        effect.parameters.len() == 3 && effect.parameters.iter().all(|v| *v > 0.0),
                        "audio ParamEQ",
                    )?;
                }
            }
        }
        AudioGameplayReference::from_value(&game.audio)?;
        for b in &self.survival {
            let s = &b.settings;
            require(
                mixers.contains(&b.mixer)
                    && b.one_shot == matches!(b.kind, 0 | 10..=15 | 17 | 21 | 22)
                    && b.source.path_id != 0
                    && b.audio_source.path_id != 0
                    && method(&b.method)
                    && values(b.offset)
                    && values(b.gain)
                    && b.gain[0] >= 0.
                    && b.gain[1] >= b.gain[0]
                    && values([
                        s.gain,
                        s.pitch,
                        s.min_distance,
                        s.max_distance,
                        s.spatial_blend,
                        b.doppler,
                        b.pan,
                    ])
                    && s.pitch > 0.
                    && s.min_distance > 0.
                    && s.max_distance >= s.min_distance
                    && s.rolloff_mode <= 2
                    && valid_curve(&s.rolloff_curve)
                    && valid_curve(&s.spatial_blend_curve),
                "survival audio playback",
            )?;
        }
        for b in &self.bindings {
            require(
                emitters.contains(&b.emitter)
                    && mixers.contains(&b.mixer)
                    && b.voice.is_none_or(|v| voices.contains(&v))
                    && !b.clips.is_empty()
                    && b.clips.iter().all(|c| clips.contains(c))
                    && method(&b.method)
                    && values([b.gain, b.pitch_range[0], b.pitch_range[1]])
                    && b.gain >= 0.0
                    && b.pitch_range[0] > 0.0
                    && b.pitch_range[1] >= b.pitch_range[0],
                "audio event binding",
            )?;
        }
        for v in &self.car_voices {
            let s = &v.settings;
            require(
                mixers.contains(&v.mixer)
                    && v.clip.is_none_or(|c| clips.contains(&c))
                    && values([
                        s.gain,
                        s.pitch,
                        s.min_distance,
                        s.max_distance,
                        s.spatial_blend,
                        v.doppler,
                        v.pan,
                    ])
                    && s.pitch > 0.
                    && s.min_distance > 0.
                    && s.max_distance >= s.min_distance
                    && s.rolloff_mode <= 2
                    && valid_curve(&s.rolloff_curve)
                    && valid_curve(&s.spatial_blend_curve),
                "car voice",
            )?;
        }
        let loop_voice = |s: &CarSoundReference| {
            values(s.position)
                && self
                    .car_voices
                    .iter()
                    .any(|v| v.id == s.voice && !v.one_shot && v.clip.is_some())
        };
        let shot = |s: &CarShotReference| {
            values(s.position)
                && s.gain.is_finite()
                && s.gain >= 0.
                && clips.contains(&s.clip)
                && self
                    .car_voices
                    .iter()
                    .any(|v| v.id == s.voice && v.one_shot)
        };
        let rising = |v: [f32; 2]| values(v) && v[1] > v[0];
        let engine = |a: &EngineAudioReference| {
            loop_voice(&a.idle)
                && loop_voice(&a.motor)
                && a.starter.as_ref().is_none_or(shot)
                && a.stop.as_ref().is_none_or(shot)
                && rising(a.idle_rpm)
                && rising(a.motor_rpm)
                && values(a.idle_pitch)
                && values(a.motor_pitch)
                && values([a.idle_gain, a.motor_gain])
        };
        let car = |a: &CarAudioReference, interiors: usize| {
            a.horn.as_ref().is_none_or(|h| {
                loop_voice(&h.sound)
                    && values(h.pitch)
                    && h.pitch[0] > 0.
                    && rising(h.temperature)
                    && h.attack_release_rate > 0.
            }) && a.switch.as_ref().is_none_or(shot)
                && a.boil.as_ref().is_none_or(|b| {
                    loop_voice(&b.sound) && b.max_gain.is_finite() && b.max_gain >= 0.
                })
                && a.doors.iter().all(|d| d.sounds.iter().all(shot))
                && a.wheels
                    .iter()
                    .all(|w| [w.ground, w.spring, w.slide].iter().all(loop_voice))
                && {
                    let r = &a.route;
                    r.interiors.iter().all(|&i| (i as usize) < interiors)
                        && [r.main_mixer, r.outside_mixer]
                            .iter()
                            .chain(&r.car_mixer)
                            .all(|m| mixers.contains(m))
                }
        };
        for p in &game.world.prototypes {
            require(
                p.engines
                    .iter()
                    .all(|e| e.audio.as_ref().is_none_or(engine))
                    && p.vehicle.as_ref().is_none_or(|v| {
                        v.audio.as_ref().is_none_or(|a| {
                            a.wheels.len() == v.wheel_effects.len()
                                && a.boil.is_some() == v.coolant_steam.is_some()
                                && car(a, p.interiors.len())
                        }) && v.blood_clip.is_none_or(|c| clips.contains(&c))
                    }),
                "world car audio",
            )?;
        }
        // Voice ids run 1..n in table order: the runtime indexes templates by id.
        require(
            self.car_voices
                .iter()
                .enumerate()
                .all(|(i, v)| v.id as usize == i + 1),
            "car voice IDs",
        )?;
        let r = &self.radio;
        require(
            r.clock.kind == "scaled"
                && r.clock.selected.class == "mainscript"
                && r.clock.selected.method == "RADIO"
                && r.clock.music_start.class == "radioCH"
                && r.clock.music_start.method == "SelectRandomMusic"
                && r.clock.talk_start.class == "radioCH"
                && r.clock.talk_start.method == "SelectRandomTalk"
                && r.clock.multiplayer.class == "mainscript"
                && r.clock.multiplayer.method == "RADIO2"
                && [
                    &r.clock.selected,
                    &r.clock.music_start,
                    &r.clock.talk_start,
                    &r.clock.multiplayer,
                ]
                .iter()
                .all(|m| method(m)),
            "audio radio clock",
        )?;
        require(
            values([
                r.fm_tolerance,
                r.am_tolerance,
                r.electronics_cutoff,
                r.time_correction_seconds,
                r.late_seek_margin_seconds,
                r.custom_gain,
            ]) && r.fm_tolerance > 0.0
                && r.am_tolerance > 0.0
                && r.methods.iter().all(method),
            "audio radio",
        )?;
        for key in [
            "AMmin",
            "AMmax",
            "FMmin",
            "FMmax",
            "AMStep",
            "FMStep",
            "vSpeed",
            "turnVolume",
        ] {
            require(
                r.constants
                    .get(key)
                    .is_some_and(|v| v.is_finite() && *v > 0.0),
                "audio radio constants",
            )?;
        }
        require(
            r.constants["AMmax"] > r.constants["AMmin"]
                && r.constants["FMmax"] > r.constants["FMmin"],
            "audio tuning ranges",
        )?;
        ids(r.stations.iter().map(|s| s.id), "audio station IDs")?;
        for s in &r.stations {
            require(
                (s.band == "am" || s.band == "fm")
                    && values([
                        s.frequency,
                        s.start_time,
                        s.initial_time,
                        s.initial_max_time,
                        s.initial_gain,
                    ])
                    && s.frequency > 0.0
                    && s.music
                        .iter()
                        .chain(s.talk.iter())
                        .all(|c| clips.contains(c))
                    && values(s.music_gains.iter().chain(s.talk_gains.iter()).copied())
                    && s.music_holidays.len() == s.music.len()
                    && s.talk_holidays.len() == s.talk.len()
                    && if s.custom {
                        s.content == "external" && s.music.is_empty() && s.talk.is_empty()
                    } else {
                        s.content == "embedded" && (!s.music.is_empty() || !s.talk.is_empty())
                    },
                "audio station",
            )?;
        }
        for m in &self.mechanisms {
            require(
                emitters.contains(&m.emitter)
                    && m.movement_voice.is_none_or(|v| {
                        self.voices
                            .iter()
                            .find(|voice| voice.id == v)
                            .and_then(|voice| voice.clip)
                            .is_some_and(|clip| {
                                self.clips.iter().any(|c| {
                                    c.id == clip && c.usages.iter().any(|usage| usage == "effect")
                                })
                            })
                    })
                    && m.endpoints.iter().all(|b| bindings.contains(b))
                    && m.turn_binding.is_none_or(|b| bindings.contains(&b))
                    && values([
                        m.audio_multiplier,
                        m.slide_multiplier,
                        m.rotation_gain,
                        m.slide_gain,
                        m.gain_smoothing,
                        m.slide_stop_seconds,
                        m.squeak_chance,
                        m.pitch_range[0],
                        m.pitch_range[1],
                    ]),
                "audio mechanism",
            )?;
        }
        for f in &self.fluids {
            require(
                emitters.contains(&f.emitter)
                    && f.fill_voice.is_none_or(|v| voices.contains(&v))
                    && [f.open_binding, f.close_binding, f.tune_binding]
                        .iter()
                        .flatten()
                        .all(|b| bindings.contains(b))
                    && f.max_gain.is_finite()
                    && f.max_gain >= 0.0
                    && method(&f.method),
                "audio fluid",
            )?;
        }
        let interiors = ids(self.interiors.iter().map(|i| i.id), "audio interior IDs")?;
        let control = |c: &AudioControlReference| {
            game.nodes.iter().any(|n| n.id == c.node)
                && c.mechanism
                    .is_none_or(|id| game.mechanisms.iter().any(|m| m.id == id))
        };
        let sensor = |s: &AudioSensorReference| {
            s.node.is_none_or(|n| game.nodes.iter().any(|v| v.id == n))
                && method(&s.method)
                && s.radius.is_none_or(|r| r.is_finite() && r > 0.0)
                && s.local_position.is_none_or(|p| values(p))
        };
        require(
            r.tuning_repeat_seconds.is_finite()
                && r.tuning_repeat_seconds > 0.
                && r.holiday_ranges.iter().all(|&[sm, sd, em, ed]| {
                    (1..=12).contains(&sm)
                        && (1..=31).contains(&sd)
                        && (1..=12).contains(&em)
                        && (1..=31).contains(&ed)
                }),
            "audio radio source controls",
        )?;
        for i in &self.interiors {
            require(
                game.nodes.iter().any(|n| n.id == i.node)
                    && game.colliders.iter().any(|c| c.id == i.collider)
                    && i.parent.is_none_or(|p| interiors.contains(&p))
                    && i.doors.iter().all(control)
                    && i.door_slots
                        .iter()
                        .all(|s| game.part_slots.iter().any(|p| p.id == *s))
                    && i.parent_door.as_ref().is_none_or(control)
                    && i.parent_slot
                        .is_none_or(|s| game.part_slots.iter().any(|p| p.id == s))
                    && i.invert_doors.len() == i.doors.len()
                    && i.invert_slots.len() == i.door_slots.len()
                    && values([i.maximum_transmission, i.value])
                    && method(&i.method),
                "audio interior",
            )?;
        }
        for r in &self.routes {
            require(
                r.voices.iter().all(|v| voices.contains(v))
                    && r.interiors.iter().all(|i| interiors.contains(i))
                    && mixers.contains(&r.main_mixer)
                    && mixers.contains(&r.outside_mixer)
                    && r.car_mixer.is_none_or(|m| mixers.contains(&m))
                    && sensor(&r.sensor)
                    && r.methods.iter().all(method),
                "audio mixer route",
            )?;
        }
        for l in &self.lamps {
            require(
                game.lighting.controllers.iter().any(|c| c.id == l.control)
                    && l.voices.iter().all(|v| voices.contains(v))
                    && [l.on_binding, l.off_binding]
                        .iter()
                        .flatten()
                        .all(|b| bindings.contains(b))
                    && method(&l.method),
                "audio lamp",
            )?;
        }
        for p in &game.sky.prefabs {
            require(
                p.voices.iter().all(|id| {
                    self.voices.iter().any(|v| {
                        v.id == *id
                            && self.emitters.iter().any(|e| {
                                e.id == v.emitter
                                    && game
                                        .nodes
                                        .iter()
                                        .any(|n| n.id == e.node && n.asset == p.asset)
                            })
                    })
                }),
                "audio sky voice",
            )?;
        }
        let outside = &self.mixer_controls.outside;
        require(
            sensor(&outside.sensor)
                && values([
                    outside.default_interior_gain,
                    outside.gain_smoothing,
                    outside.car_sound_multiplier,
                    outside.initial_gain,
                ])
                && outside.methods.iter().all(method),
            "audio outside mixer",
        )?;
        let range = |v: [f32; 2]| values(v) && v[0] >= 0. && v[1] >= v[0];
        let c = &self.collisions;
        require(
            c.car_tiers
                .windows(2)
                .all(|t| t[0].threshold < t[1].threshold)
                && c.car_tiers.iter().all(|t| {
                    t.threshold.is_finite()
                        && t.threshold >= 0.
                        && !t.clips.is_empty()
                        && t.clips.iter().all(|id| clips.contains(id))
                        && t.fixed_gain.is_none_or(|g| g.is_finite() && g >= 0.)
                })
                && values([
                    c.interior_check_seconds,
                    c.interior_radius,
                    c.loose_multiplier,
                    c.item_minimum,
                    c.car_multiplier,
                ])
                && c.interior_check_seconds > 0.
                && c.interior_radius > 0.
                && c.loose_multiplier >= 0.
                && c.item_minimum >= 0.
                && c.car_multiplier >= 0.
                && !c.method.is_empty()
                && c.method.iter().all(method),
            "audio collisions",
        )?;
        require(
            game.items.iter().all(|i| {
                i.collision_clip.is_none_or(|id| clips.contains(&id))
                    && i.collision_gain.is_finite()
                    && i.collision_gain >= 0.
            }) && game.world.pickup_hits.iter().all(|h| {
                clips.contains(&h.collision_clip)
                    && h.collision_gain.is_finite()
                    && h.collision_gain >= 0.
            }),
            "audio collision gains",
        )?;
        require(
            clips.contains(&game.skid.road_clip) && clips.contains(&game.skid.dirt_clip),
            "audio skid clips",
        )?;
        for a in &self.ambience {
            require(
                game.world
                    .prototypes
                    .iter()
                    .any(|p| p.source == a.prototype)
                    && !a.clips.is_empty()
                    && a.clips.iter().all(|id| clips.contains(id))
                    && [
                        a.phases.dusk,
                        a.phases.night,
                        a.phases.morning,
                        a.phases.day,
                    ]
                    .into_iter()
                    .flatten()
                    .all(range)
                    && range(a.interval)
                    && range(a.gain)
                    && range(a.pitch)
                    && a.pitch[0] > 0.
                    && (0. ..=1.).contains(&a.chance)
                    && method(&a.method),
                "audio ambience",
            )?;
        }
        for z in &self.ambience_zones {
            require(
                game.world
                    .prototypes
                    .iter()
                    .any(|p| p.source == z.prototype)
                    && clips.contains(&z.clip),
                "ambience zone source",
            )?;
        }
        validate_zones(&self.ambience_zones, &self.survival)?;
        // Sleds play through the one looped survival row of kind 20.
        let sled = self.survival.iter().find(|b| b.kind == 20);
        require(
            game.world.prototypes.iter().all(|p| p.sled.is_none())
                || sled.is_some_and(|b| b.role == "sled" && !b.one_shot && !b.clips.is_empty()),
            "sled playback",
        )?;
        let f = &self.footsteps;
        require(
            f.surfaces
                .iter()
                .zip(["road", "desert", "winter", "grass", "wood"])
                .all(|(s, name)| {
                    s.name == name
                        && s.volume.is_finite()
                        && s.volume >= 0.
                        && !s.clips.is_empty()
                        && s.clips.iter().all(|id| clips.contains(id))
                })
                && values(f.local_position)
                && values([f.step_gain, f.ray_distance])
                && f.step_gain >= 0.
                && f.ray_distance > 0.
                && f.sand_layers.iter().all(|l| *l < 32)
                && !f.method.is_empty()
                && f.method.iter().all(method),
            "audio footsteps",
        )?;
        let settings = &self.shared.settings;
        require(
            values([
                settings.gain,
                settings.pitch,
                settings.spatial_blend,
                settings.min_distance,
                settings.max_distance,
            ]) && settings.gain >= 0.0
                && settings.pitch > 0.0
                && (0.0..=1.0).contains(&settings.spatial_blend)
                && settings.min_distance > 0.0
                && settings.max_distance >= settings.min_distance
                && settings.rolloff_mode <= 2
                && valid_curve(&settings.spatial_blend_curve)
                && valid_curve(&settings.rolloff_curve),
            "audio shared source settings",
        )?;
        require(
            hash(&self.managed.sha256)
                && hash(&self.managed.decompiled_sha256)
                && hash(&self.native.sha256)
                && self.native.effect_type == 10
                && self.shared.priorities == [10, 25, 40, 55, 70, 80, 85, 100]
                && self.shared.enforced_voice_limit.is_none()
                && mixers.contains(&self.shared.mixer)
                && method(&self.shared.method)
                && self.mixer_controls.methods.iter().all(method),
            "audio evidence",
        )?;
        Ok(())
    }
}

record!(SurvivalAudioReference {
    role: String, kind: u32, one_shot: bool, source: SourceId, audio_source: SourceId,
    clips: Vec<u32>, gain: [f32; 2], pitch: [f32; 2], offset: [f32; 3],
    settings: AudioSourceSettings, mixer: u32, doppler: f32, pan: f32, method: AudioMethod
});
#[derive(Clone, Debug, Deserialize)]
pub struct AudioGateBinding {
    pub role: String,
    pub kind: u32,
    pub clips: Vec<u32>,
    pub pitch: [f32; 2],
}
#[derive(Clone, Debug, Deserialize)]
pub struct AudioGateClip {
    pub id: u32,
    pub source: SourceId,
    pub frames: u64,
    pub frequency: u32,
    pub duration_seconds: f64,
}
#[derive(Clone, Debug, Deserialize)]
pub struct AudioGameplayReference {
    #[serde(rename = "survival")]
    pub bindings: Vec<AudioGateBinding>,
    pub clips: Vec<AudioGateClip>,
}
impl AudioGameplayReference {
    pub fn from_value(raw: &serde_json::Value) -> Result<Self, ReferenceError> {
        let r: Self = serde_json::from_value(raw.clone())
            .map_err(|e| ReferenceError(format!("Required audio timing: {e}")))?;
        let clips = ids(r.clips.iter().map(|c| c.id), "required audio clip IDs")?;
        for c in &r.clips {
            require(
                c.id > 0
                    && c.source.path_id != 0
                    && c.frames > 0
                    && c.frequency > 0
                    && c.duration_seconds.is_finite()
                    && c.duration_seconds > 0.
                    && (c.duration_seconds - c.frames as f64 / f64::from(c.frequency)).abs() < 1e-6,
                "required source audio duration",
            )?;
        }
        let required = [
            (0, "eat"),
            (1, "burp"),
            (2, "fart"),
            (4, "sleep"),
            (5, "death"),
            (6, "storm"),
            (7, "ambient_day"),
            (8, "ambient_night"),
            (9, "shit"),
            (10, "eat_crunchy"),
            (11, "item_hit"),
            (12, "car_hit"),
            (13, "rock_heat"),
            (14, "music_box"),
            (15, "footstep"),
            (16, "storm_inside"),
            (17, "melee_hit"),
            (18, "metal_detector"),
            (21, "radio_click"),
            (22, "break"),
            (23, "computer_note"),
            (24, "horror"),
        ];
        for (kind, role) in required {
            require(
                r.bindings
                    .iter()
                    .filter(|b| b.kind == kind && b.role == role)
                    .count()
                    == 1,
                "required audio role",
            )?;
            let b = r.binding(kind);
            // Hit and ambience records take clips from their own sources.
            require(
                (matches!(kind, 11..=13) || !b.clips.is_empty())
                    && b.clips.iter().all(|id| clips.contains(id))
                    && values(b.pitch)
                    && b.pitch[0] > 0.
                    && b.pitch[1] >= b.pitch[0],
                "required audio clips and pitch",
            )?;
        }
        // The ambience zone and sled rows exist only when the world has zones or sleds.
        let optional = |kind: u32, role: &str| {
            r.bindings
                .iter()
                .filter(|b| b.kind == kind && b.role == role && !b.clips.is_empty())
                .count()
        };
        let (zones, sleds) = (optional(19, "ambience_zone"), optional(20, "sled"));
        require(
            zones <= 1 && sleds <= 1 && r.bindings.len() == required.len() + zones + sleds,
            "required audio roles",
        )?;
        Ok(r)
    }
    pub fn binding(&self, kind: u32) -> &AudioGateBinding {
        self.bindings
            .iter()
            .find(|b| b.kind == kind)
            .expect("validated audio timing")
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AmbientReference {
    pub source: SourceId,
    pub day_volume: [f32; 2],
    pub night_volume: [f32; 2],
    pub modifier: f32,
    pub only_modifier: f32,
    pub recovery_per_second: f32,
}
impl AmbientReference {
    pub(super) fn validate(&self) -> Result<(), ReferenceError> {
        let ambient = self;
        require(
            ambient.source.file == "level1"
                && ambient.source.path_id == 64920
                && ambient
                    .day_volume
                    .iter()
                    .chain(&ambient.night_volume)
                    .chain([&ambient.modifier, &ambient.only_modifier])
                    .all(|v| v.is_finite() && (0. ..=1.).contains(v))
                && ambient.recovery_per_second.is_finite()
                && ambient.recovery_per_second > 0.,
            "ambient",
        )?;
        Ok(())
    }
}
