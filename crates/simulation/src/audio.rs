//! Source audio signals and bounded completed-action events.
use crate::interaction::{InteractionAction, WheelAction};
use crate::reference::{AudioReference, ColliderId, ItemId, NodeId, ReferenceManifest};
use crate::starting_area::StartingArea;
use crate::vehicle::{PhysicsWorld, to_rapier};
use crate::world::WorldStream;
use rapier3d::prelude::{Ball, QueryFilter};
use std::collections::{BTreeMap, HashMap};

pub mod ambience;
pub mod cars;
pub mod footsteps;
pub mod impacts;
pub mod interiors;

#[derive(Clone, Copy, Debug)]
pub struct AudioEvent {
    pub session: u32,
    pub sequence: u32,
    pub binding: u32,
    pub emitter: u32,
    pub position: [f32; 3],
    pub gain: f32,
    pub pitch: f32,
    pub time_seconds: f64,
    pub priority: u8,
}
#[derive(Clone, Debug)]
pub struct SensorLatch {
    pub sensed: bool,
    pub count: u32,
    pub hits: Vec<Option<usize>>,
}
impl SensorLatch {
    pub fn new(sensed: bool, count: u32, capacity: usize) -> Self {
        Self {
            sensed,
            count,
            hits: Vec::with_capacity(capacity),
        }
    }
    pub fn begin_step(&mut self) {
        if self.count > 0 {
            self.count -= 1;
        } else {
            self.sensed = false;
            self.hits.clear();
        }
    }
    pub fn hit(&mut self) {
        self.sensed = true;
        self.count = 3;
    }
}
/// Browser emitter ids of world radio voice slots: base + group * slots + slot.
pub const PORTABLE_RADIO_EMITTER: u32 = 0x5000_0000;
/// Voice slots per world radio voice group.
pub const PORTABLE_RADIO_SLOTS: usize = 4;
/// Browser playback kind of voice 0 as an instance. Voice `id` plays as kind `VOICE_KIND + id`.
/// Each instance is a separate loop, so several copies of one prefab voice play at once.
pub const VOICE_KIND: u32 = 0x1_0000;
pub fn source_gain_db(gain: f32) -> f32 {
    gain.clamp(0.001, 1.).ln() * 20.
}
pub fn interior_transmission(total: f32, doors: usize, minimum: u32, maximum: f32) -> f32 {
    let value = if minimum != 0 && total >= minimum as f32 {
        1.
    } else {
        (total / (doors as f32).clamp(1., 100.)).clamp(0., 1.)
    };
    if maximum != 0. {
        value.min(maximum)
    } else {
        value
    }
}
#[derive(Clone, Copy, Debug)]
pub struct AudioSignal {
    pub id: u32,
    pub emitter: u32,
    pub playing: bool,
    pub gain: f32,
    pub pitch: f32,
    pub looping: bool,
    /// Clip id that the voice plays. 0 has no clip.
    pub clip: u32,
}
fn xorshift(state: &mut u32) -> f32 {
    *state ^= *state << 13;
    *state ^= *state >> 17;
    *state ^= *state << 5;
    *state as f32 / u32::MAX as f32
}
/// A full queue drops its last event with the highest priority number for a lower one.
fn insert_event(queue: &mut [Option<AudioEvent>; 64], length: &mut usize, event: AudioEvent) {
    if *length == 64 {
        let worst = (0..64).max_by_key(|&i| queue[i].unwrap().priority).unwrap();
        if event.priority >= queue[worst].unwrap().priority {
            return;
        }
        queue.copy_within(worst + 1.., worst);
        *length -= 1;
    }
    queue[*length] = Some(event);
    *length += 1;
}
fn inverse_lerp(a: f32, b: f32, value: f32) -> f32 {
    ((value - a) / (b - a)).clamp(0., 1.)
}
struct MechanismSound {
    item: ItemId,
    index: usize,
    previous: [f32; 2],
    turn: u8,
    limits: [[f32; 2]; 2],
    slide: bool,
    remaining: f32,
    last_slide_time: f64,
}
/// One AudioSource of a sky prefab.
struct SkyVoice {
    prefab: usize,
    voice: u32,
    clip: u32,
    gain: f32,
    pitch: f32,
    /// Emitter position relative to the prefab root.
    local: rapier3d::math::Vector,
}
/// A playing flyer squeak. It follows its bat until the clip ends.
#[derive(Clone, Copy, Debug)]
struct FlyerSqueak {
    item: ItemId,
    clip: u32,
    gain: f32,
    pitch: f32,
    end: f64,
}
struct FluidSound {
    opening: crate::reference::OpeningId,
    index: usize,
    latch: u32,
    gain: f32,
}
pub struct AudioState {
    pub survival_events: Vec<SurvivalAudioEvent>,
    pub survival_loops: Vec<SurvivalAudioEvent>,
    pub sleep_radio_advanced: bool,
    pub signals: Vec<AudioSignal>,
    /// Car and engine part sounds.
    pub cars: cars::CarSounds,
    /// Largest car voice distance. Cars farther from the listener are quiet.
    car_range: f32,
    crafted: HashMap<ItemId, u32>,
    lamp_controls: HashMap<crate::reference::LightControlId, (usize, bool)>,
    lamp_voices: Vec<(crate::reference::LightControlId, u32, bool)>,
    mechanism_sounds: Vec<MechanismSound>,
    mechanism_items: HashMap<ItemId, usize>,
    fluid_sounds: Vec<FluidSound>,
    fluid_openings: HashMap<crate::reference::OpeningId, usize>,
    positions: HashMap<u32, [f32; 3]>,
    removed: std::collections::HashSet<u32>,
    random: u32,
    origin_shift: [f32; 3],
    voice_indices: HashMap<u32, usize>,
    emitter_anchors: HashMap<u32, (Option<NodeId>, rapier3d::math::Pose)>,
    sky_voices: Vec<SkyVoice>,
    squeaks: Vec<FlyerSqueak>,
    /// Voice instance loops and flyer squeak moves of this step. A loop that is missing stops.
    pub voice_loops: Vec<cars::CarSignal>,
    /// Loot tool loops of this step. A loop that is missing stops.
    pub tool_loops: Vec<cars::CarSignal>,
    pub reference: AudioReference,
    pub session: u32,
    pub game_seconds: f64,
    pub outside_gain: f32,
    pub current_interior: Option<usize>,
    pub sensor: SensorLatch,
    pub route_mixers: Vec<u32>,
    pub broadcasts: crate::simulation::radio::RadioBroadcasts,
    /// World radios holding the browser's voice slots, per voice group.
    pub portable_radio_slots: Vec<[Option<ItemId>; PORTABLE_RADIO_SLOTS]>,
    queue: [Option<AudioEvent>; 64],
    length: usize,
    sequence: u32,
    bindings: BTreeMap<u32, usize>,
    colliders: HashMap<ColliderId, usize>,
    collider_nodes: HashMap<ColliderId, NodeId>,
    control_items: HashMap<crate::reference::MechanismId, ItemId>,
    part_usables: HashMap<ItemId, ItemId>,
    route_sensors: Vec<(Option<crate::vehicle::ColliderHandle>, SensorLatch)>,
    route_nodes: Vec<Option<NodeId>>,
    outside_mixer: u32,
    car_mixer: u32,
    sphere: Option<Ball>,
    impacts: impacts::ImpactState,
    rocks: ambience::RockAmbience,
    zones: ambience::ZoneAmbience,
    world_interiors: interiors::WorldInteriors,
    footsteps: footsteps::FootstepState,
}
impl AudioState {
    pub fn new(reference: AudioReference, session: u32) -> Self {
        let outside = &reference.mixer_controls.outside;
        let sphere = outside.sensor.radius.map(Ball::new);
        let sensor = SensorLatch::new(
            outside.sensor.initial_sensed,
            outside.sensor.initial_count,
            reference.interiors.len() + 256,
        );
        Self {
            survival_events: Vec::with_capacity(512),
            survival_loops: Vec::with_capacity(64),
            sleep_radio_advanced: false,
            crafted: HashMap::new(),
            lamp_controls: HashMap::new(),
            lamp_voices: Vec::new(),
            mechanism_sounds: Vec::new(),
            mechanism_items: HashMap::new(),
            fluid_sounds: Vec::new(),
            fluid_openings: HashMap::new(),
            positions: reference.emitters.iter().map(|e| (e.id, [0.; 3])).collect(),
            removed: std::collections::HashSet::with_capacity(reference.emitters.len()),
            random: session.wrapping_add(0x9e3779b9),
            origin_shift: [0.; 3],
            cars: cars::CarSounds::default(),
            car_range: reference
                .car_voices
                .iter()
                .fold(0., |m: f32, v| m.max(v.settings.max_distance)),
            signals: reference
                .voices
                .iter()
                .map(|v| AudioSignal {
                    id: v.id,
                    emitter: v.emitter,
                    playing: false,
                    gain: v.gain,
                    pitch: v.pitch,
                    looping: v.r#loop,
                    clip: v.clip.unwrap_or(0),
                })
                .collect(),
            voice_indices: reference
                .voices
                .iter()
                .enumerate()
                .map(|(i, v)| (v.id, i))
                .collect(),
            emitter_anchors: HashMap::new(),
            sky_voices: Vec::new(),
            squeaks: Vec::with_capacity(32),
            voice_loops: Vec::with_capacity(48),
            tool_loops: Vec::with_capacity(16),
            session,
            game_seconds: 0.,
            outside_gain: outside.initial_gain,
            current_interior: None,
            sensor,
            queue: [None; 64],
            length: 0,
            sequence: 0,
            bindings: reference
                .bindings
                .iter()
                .enumerate()
                .map(|(i, b)| (b.id, i))
                .collect(),
            colliders: reference
                .interiors
                .iter()
                .enumerate()
                .map(|(i, v)| (v.collider, i))
                .collect(),
            route_mixers: reference.routes.iter().map(|r| r.main_mixer).collect(),
            outside_mixer: reference
                .mixers
                .iter()
                .find(|m| m.name == "OutSideCar")
                .map_or(0, |m| m.id),
            car_mixer: reference
                .mixers
                .iter()
                .find(|m| m.name == "CarSounds")
                .map_or(0, |m| m.id),
            broadcasts: crate::simulation::radio::RadioBroadcasts::new(
                &reference.radio,
                &reference.clips,
                [0, 0],
            ),
            portable_radio_slots: vec![
                [None; PORTABLE_RADIO_SLOTS];
                reference.portable_radios.len()
            ],
            collider_nodes: HashMap::new(),
            route_sensors: reference
                .routes
                .iter()
                .map(|r| {
                    (
                        None,
                        SensorLatch::new(r.sensor.initial_sensed, r.sensor.initial_count, 1),
                    )
                })
                .collect(),
            route_nodes: vec![None; reference.routes.len()],
            part_usables: HashMap::new(),
            control_items: HashMap::new(),
            sphere,
            impacts: impacts::ImpactState::default(),
            rocks: ambience::RockAmbience::default(),
            zones: ambience::ZoneAmbience::default(),
            world_interiors: interiors::WorldInteriors::new(reference.interiors.len()),
            footsteps: footsteps::FootstepState::default(),
            reference,
        }
    }
    pub fn bind_game(
        &mut self,
        game: &ReferenceManifest,
        area: &StartingArea,
        physics: &PhysicsWorld,
    ) {
        self.emitter_anchors.clear();
        for emitter in &self.reference.emitters {
            let world = to_rapier(
                crate::interaction::items::node_pose(game, emitter.node)
                    .expect("validated emitter node"),
            );
            let mut node = Some(emitter.node);
            while let Some(id) = node {
                if area.interaction.mechanisms.node_pose(id).is_some() {
                    break;
                }
                node = game
                    .nodes
                    .iter()
                    .find(|n| n.id == id)
                    .and_then(|n| n.parent);
            }
            let local = node.map_or(world, |id| {
                to_rapier(crate::interaction::items::node_pose(game, id).unwrap()).inverse() * world
            });
            self.emitter_anchors.insert(emitter.id, (node, local));
        }
        self.sky_voices.clear();
        for (prefab, p) in game.sky.prefabs.iter().enumerate() {
            for &id in &p.voices {
                let voice = &self.reference.voices[self.voice_indices[&id]];
                let Some(clip) = voice.clip else {
                    continue;
                };
                let emitter = self
                    .reference
                    .emitters
                    .iter()
                    .find(|e| e.id == voice.emitter)
                    .expect("validated sky emitter");
                // The prefab root has an identity pose, so the node pose is relative to the root.
                let pose = crate::interaction::items::node_pose(game, emitter.node)
                    .expect("validated sky emitter node");
                self.sky_voices.push(SkyVoice {
                    prefab,
                    voice: id,
                    clip,
                    gain: voice.gain,
                    pitch: voice.pitch,
                    local: rapier3d::math::Vector::from_array(pose.position),
                });
            }
        }
        self.lamp_controls = self
            .reference
            .lamps
            .iter()
            .enumerate()
            .filter_map(|(index, l)| {
                game.lighting
                    .controllers
                    .iter()
                    .find(|c| c.id == l.control)
                    .map(|c| (l.control, (index, c.startup_on)))
            })
            .collect();
        self.lamp_voices.clear();
        for lamp in &self.reference.lamps {
            let c = game
                .lighting
                .controllers
                .iter()
                .find(|c| c.id == lamp.control)
                .unwrap();
            for &id in &lamp.voices {
                let voice = &self.reference.voices[self.voice_indices[&id]];
                let node = self
                    .reference
                    .emitters
                    .iter()
                    .find(|e| e.id == voice.emitter)
                    .unwrap()
                    .node;
                let state = c.states.iter().find(|s| {
                    s.renderers.iter().any(|&renderer| {
                        let mut cursor = Some(renderer);
                        while let Some(n) = cursor {
                            if n == node {
                                return true;
                            }
                            cursor = game.nodes.iter().find(|v| v.id == n).and_then(|v| v.parent);
                        }
                        false
                    })
                });
                let enabled = state.is_some_and(|s| !s.broken)
                    && (c.electronics_source.is_none()
                        || c.electronics_output > c.electronics_threshold);
                self.lamp_voices.push((lamp.control, id, enabled));
                self.signals[self.voice_indices[&id]].playing = c.startup_on && enabled;
            }
        }
        self.crafted = self
            .reference
            .bindings
            .iter()
            .filter(|b| b.trigger == "crafted")
            .filter_map(|b| {
                game.items
                    .iter()
                    .find(|i| i.pickup.as_ref().is_some_and(|p| p.source == b.source))
                    .map(|i| (i.id, b.id))
            })
            .collect();
        self.mechanism_sounds = self
            .reference
            .mechanisms
            .iter()
            .enumerate()
            .filter_map(|(index, a)| {
                let m = game.mechanisms.iter().find(|m| m.source == a.source)?;
                let (previous, turn) = area.interaction.mechanisms.audio_values(m.item)?;
                Some(MechanismSound {
                    item: m.item,
                    index,
                    previous,
                    turn,
                    limits: m
                        .rotation
                        .as_ref()
                        .map_or([[0., 1.], [0., 0.]], |r| r.limits_degrees),
                    slide: m.slide.is_some(),
                    remaining: 0.,
                    last_slide_time: 0.,
                })
            })
            .collect();
        for a in &self.reference.mechanisms {
            if let Some(id) = a.movement_voice {
                self.signals[self.voice_indices[&id]].gain = 0.;
            }
        }
        self.mechanism_items = self
            .mechanism_sounds
            .iter()
            .enumerate()
            .map(|(i, m)| (m.item, i))
            .collect();
        self.fluid_sounds = self
            .reference
            .fluids
            .iter()
            .enumerate()
            .filter_map(|(index, a)| {
                let o = game.openings.iter().find(|o| o.source == a.source)?;
                Some(FluidSound {
                    opening: o.id,
                    index,
                    latch: 0,
                    gain: 0.,
                })
            })
            .collect();
        self.fluid_openings = self
            .fluid_sounds
            .iter()
            .enumerate()
            .map(|(i, f)| (f.opening, i))
            .collect();
        self.part_usables = game
            .parts
            .iter()
            .filter_map(|p| p.usable.map(|u| (p.item, u)))
            .collect();
        self.control_items = game.mechanisms.iter().map(|m| (m.id, m.item)).collect();
        self.collider_nodes = game.colliders.iter().map(|c| (c.id, c.node)).collect();
        self.impacts.bind();
        self.sensor.hits.reserve(game.colliders.len());
        for (i, r) in self.reference.routes.iter().enumerate() {
            self.route_sensors[i].0 = game
                .colliders
                .iter()
                .find(|c| Some(c.node) == r.sensor.node && c.sensor)
                .and_then(|c| {
                    physics
                        .rapier()
                        .colliders
                        .iter()
                        .find(|(_, collider)| ColliderId((collider.user_data >> 64) as u32) == c.id)
                        .map(|(handle, _)| handle)
                });
        }
    }
    pub fn sync_lamps(
        &mut self,
        reference: &crate::reference::LightingRecord,
        lighting: &crate::lighting::LightingState,
    ) {
        for (control, (_, on)) in &mut self.lamp_controls {
            let index = reference
                .controllers
                .iter()
                .position(|c| c.id == *control)
                .expect("validated lamp controller");
            *on = lighting.controllers[index].on;
        }
        for &(control, id, enabled) in &self.lamp_voices {
            let signal = &mut self.signals[self.voice_indices[&id]];
            signal.playing = self.lamp_controls[&control].1
                && enabled
                && !self.removed.contains(&signal.emitter);
        }
    }
    pub fn voice(&mut self, id: u32, playing: bool, gain: f32, pitch: f32) {
        if let Some(&index) = self.voice_indices.get(&id) {
            let signal = &mut self.signals[index];
            signal.playing = playing && !self.removed.contains(&signal.emitter);
            signal.gain = gain;
            signal.pitch = pitch;
        }
    }
    /// One loop of a loot tool car voice template. `position` is absolute.
    pub fn tool_loop(&mut self, item: ItemId, voice: u32, gain: f32, position: [f64; 3]) {
        if self.tool_loops.len() < self.tool_loops.capacity() {
            self.tool_loops.push(cars::CarSignal {
                kind: cars::CAR_KIND + voice,
                instance: item.0,
                action: AudioAction::Update,
                clip: cars::template_clip(&self.reference.car_voices, voice),
                gain,
                pitch: 1.,
                position,
            });
        }
    }
    /// Loops of the sky prefab voices, one instance per plane. `anchor` is absolute.
    pub fn sky(&mut self, planes: &[crate::sky_traffic::Plane], anchor: [f64; 3]) {
        self.voice_loops.clear();
        for plane in planes {
            for v in self.sky_voices.iter().filter(|v| v.prefab == plane.prefab) {
                if self.voice_loops.len() == self.voice_loops.capacity() {
                    return;
                }
                let p = plane.offset + plane.heading * v.local;
                self.voice_loops.push(cars::CarSignal {
                    kind: VOICE_KIND + v.voice,
                    instance: plane.id,
                    action: AudioAction::Update,
                    clip: v.clip,
                    gain: v.gain,
                    pitch: v.pitch,
                    position: std::array::from_fn(|i| anchor[i] + f64::from(p[i])),
                });
            }
        }
    }
    /// randomFlyer.Sound through mainscript.PlayClipAtPoint, parented to the bat.
    /// New squeaks start. Playing squeaks move with their bat until the clip ends.
    /// `position` gives the absolute bat position, or `None` when the bat is gone.
    /// Runs after `sky`, which clears `voice_loops`.
    pub fn flyers(
        &mut self,
        voice: u32,
        started: &[crate::flyers::Squeak],
        now: f64,
        position: impl Fn(ItemId) -> Option<[f64; 3]>,
    ) {
        let kind = cars::CAR_KIND + voice;
        let signal = |s: &FlyerSqueak, action, position| cars::CarSignal {
            kind,
            instance: s.item.0,
            action,
            clip: s.clip,
            gain: s.gain,
            pitch: s.pitch,
            position,
        };
        self.squeaks.retain(|s| s.end > now);
        for s in &self.squeaks {
            if let Some(p) = position(s.item)
                && self.voice_loops.len() < self.voice_loops.capacity()
            {
                self.voice_loops.push(signal(s, AudioAction::Update, p));
            }
        }
        for q in started {
            let Some(p) = position(q.item) else {
                continue;
            };
            // The source destroys the source after the clip length.
            let seconds = self
                .reference
                .clips
                .iter()
                .find(|c| c.id == q.clip)
                .map_or(0., |c| c.source_duration_seconds);
            let s = FlyerSqueak {
                item: q.item,
                clip: q.clip,
                gain: q.gain,
                pitch: q.pitch,
                end: now + seconds,
            };
            if self.cars.events.len() < self.cars.events.capacity() {
                self.cars.events.push(signal(&s, AudioAction::Start, p));
            }
            if self.squeaks.len() < self.squeaks.capacity() {
                self.squeaks.push(s);
            }
        }
    }
    fn sample(&mut self) -> f32 {
        xorshift(&mut self.random)
    }
    /// mainscript.PlayClipAtPoint randomPitch: Random.Range(0.85, 1.15).
    pub fn random_pitch(&mut self) -> f32 {
        0.85 + 0.3 * self.sample()
    }
    fn emit(&mut self, id: u32, gain: f32, simulation_seconds: f64) {
        self.emit_at(id, gain, simulation_seconds, None);
    }
    fn emit_at(&mut self, id: u32, gain: f32, simulation_seconds: f64, at: Option<[f32; 3]>) {
        let Some(&index) = self.bindings.get(&id) else {
            return;
        };
        let range = self.reference.bindings[index].pitch_range;
        let pitch = range[0] + (range[1] - range[0]) * self.sample();
        let b = &self.reference.bindings[index];
        if self.removed.contains(&b.emitter) {
            return;
        }
        self.sequence = self.sequence.wrapping_add(1);
        let position = if let Some(at) = at {
            at
        } else {
            self.positions.get(&b.emitter).copied().unwrap_or([0.; 3])
        };
        self.push_event(AudioEvent {
            session: self.session,
            sequence: self.sequence,
            binding: id,
            emitter: b.emitter,
            position,
            gain,
            pitch,
            time_seconds: simulation_seconds,
            priority: b.priority,
        });
    }
    /// Submit only completed operations. Detachment does not remove an owner.
    pub fn action(&mut self, action: InteractionAction, simulation_seconds: f64) {
        let item = match action {
            InteractionAction::LightControl(control) => {
                if let Some((index, on)) = self.lamp_controls.get_mut(&control) {
                    *on = !*on;
                    let lamp = &self.reference.lamps[*index];
                    for &(owner, id, enabled) in &self.lamp_voices {
                        if owner == control {
                            self.signals[self.voice_indices[&id]].playing = *on
                                && enabled
                                && !self
                                    .removed
                                    .contains(&self.signals[self.voice_indices[&id]].emitter);
                        }
                    }
                    let binding = if *on {
                        lamp.on_binding
                    } else {
                        lamp.off_binding
                    };
                    if let Some(id) = binding {
                        self.emit(id, 1., simulation_seconds);
                    }
                }
                None
            }
            InteractionAction::PartInstall(item)
            | InteractionAction::PartRemove(item)
            | InteractionAction::Wheel(WheelAction::Remove(item))
            | InteractionAction::Wheel(WheelAction::Install { item, .. }) => Some(item),
            InteractionAction::Wheel(WheelAction::Join { tire, .. }) => Some(tire),
            _ => None,
        };
        if let Some(id) = item.and_then(|item| self.crafted.get(&item)).copied() {
            self.emit(id, 1., simulation_seconds);
        }
    }
    pub fn cap(
        &mut self,
        id: crate::reference::OpeningId,
        previous: f32,
        current: f32,
        tune: Option<f32>,
    ) {
        let Some(&i) = self.fluid_openings.get(&id) else {
            return;
        };
        let a = &self.reference.fluids[self.fluid_sounds[i].index];
        let (binding, gain) = if let Some(delta) = tune {
            if previous <= 0. || previous >= 1. || delta == 0. {
                return;
            }
            (a.tune_binding, delta.abs())
        } else if previous != current {
            (
                if current > 0. {
                    a.open_binding
                } else {
                    a.close_binding
                },
                1.,
            )
        } else {
            return;
        };
        if let Some(b) = binding {
            self.emit(b, gain, self.game_seconds);
        }
    }
    pub fn mechanism(&mut self, item: ItemId, previous: f32, current: f32, dt: f32) {
        self.mechanism_channels(
            item,
            [previous, 0.],
            [current, 0.],
            [(current - previous).abs(), 0.],
            dt,
        );
    }
    fn mechanism_channels(
        &mut self,
        item: ItemId,
        previous: [f32; 2],
        current: [f32; 2],
        requested: [f32; 2],
        dt: f32,
    ) {
        if !dt.is_finite() || dt <= 0. {
            return;
        }
        let Some(&i) = self.mechanism_items.get(&item) else {
            return;
        };
        let index = self.mechanism_sounds[i].index;
        let slide = self.mechanism_sounds[i].slide;
        let limits = self.mechanism_sounds[i].limits;
        let delta = [
            (current[0] - previous[0]).abs(),
            (current[1] - previous[1]).abs(),
        ];
        for axis in 0..if slide { 1 } else { 2 } {
            if delta[axis] == 0. {
                continue;
            }
            let [min, max] = limits[axis];
            for kind in 0..4 {
                let trigger = ["startPosEnd", "endPosEnd", "startPosStart", "endPosStart"][kind];
                let edge = [
                    current[axis] == min && previous[axis] != min,
                    current[axis] == max && previous[axis] != max,
                    current[axis] != min && previous[axis] == min,
                    current[axis] != max && previous[axis] == max,
                ][kind];
                if !edge {
                    continue;
                }
                for n in 0..self.reference.mechanisms[index].endpoints.len() {
                    let id = self.reference.mechanisms[index].endpoints[n];
                    if self.binding(id).is_some_and(|b| b.trigger == trigger) {
                        self.emit(id, requested[axis].abs() / dt * 0.01, self.game_seconds);
                    }
                }
            }
        }
        let a = &self.reference.mechanisms[index];
        let Some(id) = a.movement_voice else {
            return;
        };
        let difference = delta[0] + delta[1];
        if slide {
            self.mechanism_sounds[i].last_slide_time = self.game_seconds;
        }
        let target = a.audio_multiplier
            * difference
            * if slide {
                a.slide_multiplier * a.slide_gain
            } else {
                a.rotation_gain
            };
        let smoothing = (dt * a.gain_smoothing).clamp(0., 1.);
        let chance = a.squeak_chance;
        let pitch_range = a.pitch_range;
        let voice = self.voice_indices[&id];
        let gain = self.signals[voice].gain + (target - self.signals[voice].gain) * smoothing;
        self.mechanism_sounds[i].remaining = if difference == 0. {
            0.
        } else {
            (self.mechanism_sounds[i].remaining - dt).max(0.)
        };
        let mut playing = difference > 0. && self.mechanism_sounds[i].remaining > 0.;
        let mut pitch = self.signals[voice].pitch;
        if difference > 0. && !playing && ((!slide && delta[0] > 0.) || self.sample() > chance) {
            playing = true;
            pitch = pitch_range[0] + (pitch_range[1] - pitch_range[0]) * self.sample();
            let clip = self.reference.voices[voice].clip.unwrap();
            let duration = self
                .reference
                .clips
                .iter()
                .find(|c| c.id == clip)
                .unwrap()
                .source_duration_seconds as f32;
            self.mechanism_sounds[i].remaining = duration / pitch;
        }
        self.voice(id, playing, gain.clamp(0., 1.), pitch);
    }
    fn fluid(&mut self, i: usize, flow: Option<(bool, f32)>, fill_called: bool, empty_pour: bool) {
        let a = &self.reference.fluids[self.fluid_sounds[i].index];
        let Some(id) = a.fill_voice else {
            return;
        };
        if fill_called {
            self.signals[self.voice_indices[&id]].looping = a.fill_loop;
        }
        if let Some((fill, value)) = flow {
            if fill {
                self.signals[self.voice_indices[&id]].looping = a.fill_loop;
            }
            self.fluid_sounds[i].gain = if fill {
                a.max_gain * value
            } else {
                inverse_lerp(a.pour_speed_min, a.pour_speed_max, value)
            };
            if fill_called || fill {
                self.fluid_sounds[i].latch = a.fill_latch_steps;
            }
        }
        if empty_pour {
            self.fluid_sounds[i].latch = 0;
        }
        let playing = !empty_pour && (flow.is_some() || self.fluid_sounds[i].latch > 0);
        if !fill_called && self.fluid_sounds[i].latch > 0 {
            self.fluid_sounds[i].latch -= 1;
        }
        self.voice(id, playing, self.fluid_sounds[i].gain, 1.);
    }
    pub fn owner_alive(&self, emitter: u32) -> bool {
        !self.removed.contains(&emitter)
    }
    pub fn push_event(&mut self, event: AudioEvent) {
        if event.session != self.session || self.removed.contains(&event.emitter) {
            return;
        }
        insert_event(&mut self.queue, &mut self.length, event);
    }
    pub fn pop_event(&mut self) -> Option<AudioEvent> {
        if self.length == 0 {
            return None;
        }
        let event = self.queue[0];
        self.length -= 1;
        for i in 0..self.length {
            self.queue[i] = self.queue[i + 1];
        }
        self.queue[self.length] = None;
        event
    }
    pub fn reset(&mut self, session: u32) {
        self.survival_events.clear();
        self.survival_loops.clear();
        self.voice_loops.clear();
        self.tool_loops.clear();
        self.squeaks.clear();
        self.sleep_radio_advanced = false;
        // Controls belong to the committed session (fresh or restored); playlists restart.
        self.broadcasts = crate::simulation::radio::RadioBroadcasts::new(
            &self.reference.radio,
            &self.reference.clips,
            [0, 0],
        );
        self.portable_radio_slots.fill([None; PORTABLE_RADIO_SLOTS]);
        self.session = session;
        self.queue.fill(None);
        self.length = 0;
        self.sequence = 0;
        self.game_seconds = 0.;
        self.origin_shift = [0.; 3];
        self.random = session.wrapping_add(0x9e3779b9);
        self.removed.clear();
        self.impacts.reset();
        self.rocks.reset();
        self.zones.reset();
        self.world_interiors.reset();
        self.footsteps = footsteps::FootstepState::default();
        for m in &mut self.mechanism_sounds {
            m.remaining = 0.;
        }
        for f in &mut self.fluid_sounds {
            f.latch = 0;
            f.gain = 0.;
        }
        self.cars.reset();
        self.outside_gain = self.reference.mixer_controls.outside.initial_gain;
        self.sensor.sensed = false;
        self.sensor.count = 0;
        self.sensor.hits.clear();
        self.current_interior = None;
        for (signal, voice) in self.signals.iter_mut().zip(&self.reference.voices) {
            signal.playing = false;
            signal.gain = voice.gain;
            signal.pitch = voice.pitch;
            signal.looping = voice.r#loop;
            signal.clip = voice.clip.unwrap_or(0);
        }
        for a in &self.reference.mechanisms {
            if let Some(id) = a.movement_voice {
                self.signals[self.voice_indices[&id]].gain = 0.;
            }
        }
        for (_, latch) in &mut self.route_sensors {
            latch.sensed = false;
            latch.count = 0;
            latch.hits.clear();
        }
    }
    pub fn rebase(&mut self, shift: [f32; 3]) {
        for (total, delta) in self.origin_shift.iter_mut().zip(shift) {
            *total += delta;
        }
        for position in self.positions.values_mut() {
            for (p, d) in position.iter_mut().zip(shift) {
                *p -= d;
            }
        }
        for event in self.queue.iter_mut().flatten() {
            for (p, d) in event.position.iter_mut().zip(shift) {
                *p -= d;
            }
        }
    }
    pub fn take_origin_shift(&mut self) -> [f32; 3] {
        std::mem::take(&mut self.origin_shift)
    }
    pub fn binding(&self, id: u32) -> Option<&crate::reference::AudioEventBinding> {
        self.bindings.get(&id).map(|&i| &self.reference.bindings[i])
    }
    pub fn fixed_step(
        &mut self,
        area: &StartingArea,
        physics: &PhysicsWorld,
        stream: &WorldStream,
        dt: f32,
    ) {
        self.game_seconds += f64::from(dt);
        self.broadcasts.advance(self.game_seconds);
        for (&id, &(anchor, local)) in &self.emitter_anchors {
            let mut world = anchor
                .and_then(|node| area.interaction.mechanisms.node_pose(node))
                .map_or(local, |pose| to_rapier(pose) * local);
            if anchor.is_none() {
                world.translation -=
                    rapier3d::math::Vector::from_array(area.origin.map(|v| v as f32));
            }
            *self.positions.get_mut(&id).unwrap() = crate::vehicle::from_rapier(&world).position;
        }
        for i in 0..self.mechanism_sounds.len() {
            let item = self.mechanism_sounds[i].item;
            let Some((current, turn)) = area.interaction.mechanisms.audio_values(item) else {
                continue;
            };
            let (active, requested) = area.interaction.mechanisms.audio_input(item);
            if active {
                self.mechanism_channels(
                    item,
                    self.mechanism_sounds[i].previous,
                    current,
                    requested,
                    dt,
                );
            } else if let Some(id) =
                self.reference.mechanisms[self.mechanism_sounds[i].index].movement_voice
            {
                self.mechanism_sounds[i].remaining =
                    (self.mechanism_sounds[i].remaining - dt).max(0.);
                let a = &self.reference.mechanisms[self.mechanism_sounds[i].index];
                let signal = &mut self.signals[self.voice_indices[&id]];
                if self.game_seconds - self.mechanism_sounds[i].last_slide_time
                    > f64::from(a.slide_stop_seconds)
                {
                    signal.gain *= 1. - (dt * 5.).clamp(0., 1.);
                }
                signal.playing &= self.mechanism_sounds[i].remaining > 0. && signal.gain > 0.;
            }
            if turn != self.mechanism_sounds[i].turn {
                if let Some(id) =
                    self.reference.mechanisms[self.mechanism_sounds[i].index].turn_binding
                {
                    self.emit(id, 1., self.game_seconds);
                }
            }
            self.mechanism_sounds[i].previous = current;
            self.mechanism_sounds[i].turn = turn;
        }
        for i in 0..self.fluid_sounds.len() {
            let flags = area
                .interaction
                .opening_audio_flags(self.fluid_sounds[i].opening);
            self.fluid(
                i,
                area.interaction.opening_flow(self.fluid_sounds[i].opening),
                flags.0,
                flags.1,
            );
        }
        if self.world_interiors.sync(
            stream.structure_revision(),
            stream.frame().structures,
            &stream.world_reference().prototypes,
        ) {
            let world = &self.world_interiors;
            for hits in std::iter::once(&mut self.sensor.hits).chain(
                self.route_sensors
                    .iter_mut()
                    .map(|(_, latch)| &mut latch.hits),
            ) {
                for hit in hits.iter_mut() {
                    *hit = hit.and_then(|i| world.remap(i));
                }
            }
        }
        self.sensor.begin_step();
        for (index, (id, latch)) in self.route_sensors.iter_mut().enumerate() {
            latch.begin_step();
            if !latch.sensed {
                self.route_nodes[index] = None;
            }
            if let Some(handle) = *id {
                if let Some(sensor) = physics.rapier().colliders.get(handle) {
                    for (other_handle, other) in physics
                        .queries(QueryFilter {
                            exclude_collider: Some(handle),
                            exclude_rigid_body: sensor.parent(),
                            groups: Some(sensor.collision_groups()),
                            ..QueryFilter::default()
                        })
                        .intersect_shape(*sensor.position(), sensor.shape())
                    {
                        let id = ColliderId((other.user_data >> 64) as u32);
                        self.route_nodes[index] = self.collider_nodes.get(&id).copied();
                        latch.hits.clear();
                        latch.hits.push(
                            self.world_interiors
                                .entry(other_handle)
                                .or_else(|| self.colliders.get(&id).copied()),
                        );
                        latch.hit();
                    }
                }
            }
        }
        let Some(sphere) = &self.sphere else {
            return;
        };
        let settings = &self.reference.mixer_controls.outside.sensor;
        let mut pose = to_rapier(area.player.view(physics).head_pose);
        pose.translation += pose.rotation
            * rapier3d::math::Vector::from_array(settings.local_position.unwrap_or([0.; 3]));
        let Some(mask) = settings.mask else {
            return;
        };
        let mut first = true;
        for (handle, collider) in physics
            .queries(QueryFilter::default())
            .intersect_shape(pose, sphere)
        {
            if collider.collision_groups().memberships.bits() & mask == 0 {
                continue;
            }
            if first {
                self.sensor.hits.clear();
                first = false;
            }
            self.sensor
                .hits
                .push(self.world_interiors.entry(handle).or_else(|| {
                    self.colliders
                        .get(&ColliderId((collider.user_data >> 64) as u32))
                        .copied()
                }));
        }
        if !first {
            self.sensor.hit();
        }
    }
    fn control(
        &self,
        control: &crate::reference::AudioControlReference,
        area: &StartingArea,
    ) -> f32 {
        control
            .mechanism
            .and_then(|id| self.control_items.get(&id))
            .and_then(|&id| area.interaction.mechanisms.value(id, true))
            .unwrap_or(1.)
    }
    /// Keeps each playing world radio in its slot and gives free slots of its voice group to
    /// the nearest playing radios within the group's maximum voice distance.
    pub fn assign_portable_radios<'a>(
        &mut self,
        listener: [f32; 3],
        radios: impl Iterator<Item = (ItemId, &'a crate::reference::SourceId, [f32; 3], bool)> + Clone,
    ) {
        for (group, record) in self.reference.portable_radios.iter().enumerate() {
            let slots = &mut self.portable_radio_slots[group];
            let range = record
                .voices
                .iter()
                .fold(0_f32, |m, v| m.max(v.max_distance));
            let audible = |position: [f32; 3]| {
                let d: f32 = (0..3).map(|i| (position[i] - listener[i]).powi(2)).sum();
                (d <= range * range).then_some(d)
            };
            let members = radios
                .clone()
                .filter(|(_, script, _, _)| record.sources.contains(script));
            for slot in slots.iter_mut() {
                if let Some(id) = *slot
                    && !members
                        .clone()
                        .any(|(item, _, p, on)| item == id && on && audible(p).is_some())
                {
                    *slot = None;
                }
            }
            for i in 0..PORTABLE_RADIO_SLOTS {
                if slots[i].is_some() {
                    continue;
                }
                let taken = *slots;
                slots[i] = members
                    .clone()
                    .filter(|(item, _, _, on)| *on && !taken.contains(&Some(*item)))
                    .filter_map(|(item, _, p, _)| audible(p).map(|d| (item, d)))
                    .min_by(|a, b| a.1.total_cmp(&b.1))
                    .map(|(item, _)| item);
            }
        }
    }
    /// `mainscript.PlayClipAtPoint(clip, position, 1f, pitch)` through the one-shot `voice` template.
    pub fn clip_at(
        &mut self,
        voice: u32,
        instance: u32,
        clip: u32,
        position: [f64; 3],
        pitch: f32,
    ) {
        if self.cars.events.len() < self.cars.events.capacity() {
            self.cars.events.push(cars::CarSignal {
                kind: cars::CAR_KIND + voice,
                instance,
                action: AudioAction::Start,
                clip,
                gain: 1.,
                pitch,
                position,
            });
        }
    }
    /// breakablescript.Break: mainscript.PlayClipAtPoint(clip, position, 1f, AudioPriorities[6]).
    pub fn break_clip(&mut self, clip: &crate::reference::SourceId, position: [f64; 3]) {
        let clip = self
            .reference
            .clips
            .iter()
            .find(|c| &c.source == clip)
            .expect("validated breakable clip")
            .id;
        self.survival_event(SurvivalAudioEvent {
            session: self.session,
            emitter: 0,
            kind: AudioKind::Break,
            action: AudioAction::Start,
            clip,
            gain: 1.,
            pitch: 1.,
            position,
        });
    }
    /// carscript.DamageStuff: PlayClipAtPoint(SBlood01, position, 1f, randomPitch, AudioPriorities[6]).
    /// It has the group, priority and settings of the Break row, so it plays through it.
    pub fn point_clip(&mut self, clip: u32, position: [f64; 3]) {
        let pitch = self.random_pitch();
        self.survival_event(SurvivalAudioEvent {
            session: self.session,
            emitter: 0,
            kind: AudioKind::Break,
            action: AudioAction::Start,
            clip,
            gain: 1.,
            pitch,
            position,
        });
    }
    /// radioscript.TurnRadio and the band switch share Cturned, played at the radio.
    pub fn radio_click_at(&mut self, position: [f64; 3]) {
        let b = self
            .reference
            .survival
            .iter()
            .find(|b| b.kind == AudioKind::RadioClick as u32)
            .expect("validated survival playback");
        let (clip, gain, [low, high]) = (b.clips[0], b.gain[0], b.pitch);
        let pitch = low + (high - low) * self.sample();
        self.survival_event(SurvivalAudioEvent {
            session: self.session,
            emitter: 0,
            kind: AudioKind::RadioClick,
            action: AudioAction::Start,
            clip,
            gain,
            pitch,
            position,
        });
    }
    fn slot(&self, slot: crate::reference::PartSlotId, area: &StartingArea) -> f32 {
        area.interaction
            .parts
            .slot_item(slot)
            .and_then(|id| self.part_usables.get(&id))
            .and_then(|&id| area.interaction.mechanisms.value(id, true))
            .unwrap_or(1.)
    }
    /// The parent list index of a live interior.
    fn interior_parent(&self, index: usize) -> Option<usize> {
        match self.reference.interiors.get(index) {
            Some(interior) => interior
                .parent
                .and_then(|id| self.reference.interiors.iter().position(|i| i.id == id)),
            None => self.world_interiors.parent(index),
        }
    }
    /// `invert`, `useValue` and `value` of a live interior.
    fn interior_flags(&self, index: usize, stream: &WorldStream) -> (bool, bool, f32) {
        if let Some(i) = self.reference.interiors.get(index) {
            return (i.invert, i.use_value, i.value);
        }
        self.world_interiors
            .get(index, &stream.world_reference().prototypes)
            .map_or((false, false, 0.), |i| (i.invert, i.use_value, i.value))
    }
    pub fn interior_value(
        &self,
        index: usize,
        area: &StartingArea,
        stream: &WorldStream,
        caller: Option<usize>,
        depth: usize,
    ) -> f32 {
        let Some(interior) = self.reference.interiors.get(index) else {
            let door = |id: &_, prototype: &_, door: &_| {
                stream
                    .door_item(id, prototype, door)
                    .and_then(|item| area.interaction.mechanisms.value(item, true))
            };
            return self.world_interiors.value(
                index,
                &stream.world_reference().prototypes,
                &door,
                caller,
                depth,
            );
        };
        if depth > self.reference.interiors.len() {
            return 0.;
        }
        let mut total = 0.;
        let mut count = interior.doors.len();
        if interior.removable_doors {
            count += interior.door_slots.len();
            for (i, &slot) in interior.door_slots.iter().enumerate() {
                let value = self.slot(slot, area);
                total += (2.
                    * if interior.invert_slots[i] {
                        1. - value
                    } else {
                        value
                    })
                .clamp(0., 1.);
            }
        }
        for (i, door) in interior.doors.iter().enumerate() {
            let value = self.control(door, area);
            total += (2.
                * if interior.invert_doors[i] {
                    1. - value
                } else {
                    value
                })
            .clamp(0., 1.);
        }
        if let Some(parent) = interior
            .parent
            .and_then(|id| self.reference.interiors.iter().position(|i| i.id == id))
        {
            if Some(parent) != caller {
                let value = if interior.removable_doors {
                    interior
                        .parent_slot
                        .map_or(1., |slot| self.slot(slot, area))
                } else {
                    interior
                        .parent_door
                        .as_ref()
                        .map_or(1., |door| self.control(door, area))
                };
                total += self.interior_value(parent, area, stream, Some(index), depth + 1)
                    * (2. * value).clamp(0., 1.);
            }
        }
        interior_transmission(
            total,
            count,
            interior.minimum_doors_to_max,
            interior.maximum_transmission,
        )
    }
    pub fn update_mixers(&mut self, area: &StartingArea, stream: &WorldStream, dt: f32) {
        let mut current = if self.sensor.sensed {
            self.sensor.hits.first().copied().flatten()
        } else {
            None
        };
        for (ri, route) in self.reference.routes.iter().enumerate() {
            let inside = self
                .sensor
                .sensed
                .then(|| {
                    self.sensor.hits.iter().flatten().copied().find(|&i| {
                        self.reference
                            .interiors
                            .get(i)
                            .is_some_and(|interior| route.interiors.contains(&interior.id))
                    })
                })
                .flatten();
            if let Some(interior) = inside {
                current = Some(interior);
                self.route_mixers[ri] = route.car_mixer.unwrap_or(route.main_mixer);
                continue;
            }
            let latch = &self.route_sensors[ri].1;
            let sensed = if latch.sensed {
                latch.hits.first().copied().flatten()
            } else {
                None
            };
            let same = latch.sensed
                && current.is_none_or(|p| {
                    self.reference
                        .interiors
                        .get(p)
                        .is_some_and(|interior| self.route_nodes[ri] == Some(interior.node))
                        || sensed.is_some_and(|i| {
                            let parent = self.interior_parent(i);
                            i == p
                                || parent == Some(p)
                                || self.interior_parent(p) == Some(i)
                                || parent.is_some() && parent == self.interior_parent(p)
                        })
                });
            self.route_mixers[ri] = if same {
                route.main_mixer
            } else {
                route.outside_mixer
            };
        }
        self.current_interior = current;
        let settings = &self.reference.mixer_controls.outside;
        let target = if let Some(index) = current {
            let doors = self
                .sensor
                .hits
                .iter()
                .map(|hit| {
                    hit.map_or(0., |i| {
                        let v = self.interior_value(i, area, stream, None, 0);
                        if self.interior_flags(i, stream).0 {
                            1. - v
                        } else {
                            v
                        }
                    })
                })
                .fold(f32::INFINITY, f32::min);
            let (_, use_value, value) = self.interior_flags(index, stream);
            interiors::outside_target(settings.default_interior_gain, use_value, value, doors)
        } else {
            1.
        };
        self.outside_gain +=
            (target - self.outside_gain) * (settings.gain_smoothing * dt).clamp(0., 1.);
    }
    /// Starts the car sound step at absolute physics origin `origin`.
    pub fn begin_cars(&mut self, origin: [f64; 3]) {
        self.cars.begin(origin);
    }
    /// A car or engine part at `distance` meters from the listener can sound.
    pub fn car_audible(&self, distance: f32) -> bool {
        distance <= self.car_range
    }
    pub fn engine_sound(&mut self, input: cars::EngineSoundInput) {
        let clips = &self.reference.clips;
        let seconds = |id: u32| {
            clips
                .iter()
                .find(|c| c.id == id)
                .map_or(0., |c| c.source_duration_seconds as f32)
        };
        self.cars.engine(
            input,
            &self.reference.car_voices,
            seconds,
            self.game_seconds,
        );
    }
    pub fn car_sound(
        &mut self,
        key: &crate::world::ObjectKey,
        input: cars::CarSoundInput,
        dt: f32,
    ) {
        self.cars.car(key, input, &self.reference.car_voices, dt);
    }
    pub fn car_shot(
        &mut self,
        key: &crate::world::ObjectKey,
        shot: &crate::reference::CarShotReference,
        pose: rapier3d::math::Pose,
    ) {
        self.cars.car_shot(key, shot, pose);
    }
    /// Whether a player sensor hit is an interior that `inside` accepts.
    fn player_inside(&self, inside: impl Fn(usize) -> bool) -> bool {
        self.sensor.sensed && self.sensor.hits.iter().flatten().any(|&i| inside(i))
    }
    /// Source `audioBasedOnInteriorsensscript` of a world car: the car mixer while the player
    /// is in one of its interiors, else the outside mixer.
    pub fn world_car_mixer(
        &self,
        route: &crate::reference::CarRouteReference,
        placement: &crate::world::GeneratedId,
    ) -> u32 {
        let inside = self.player_inside(|i| {
            self.world_interiors.owner(i).is_some_and(|(p, interior)| {
                p == placement && route.interiors.contains(&(interior as u32))
            })
        });
        if inside {
            route.car_mixer.unwrap_or(route.main_mixer)
        } else {
            route.outside_mixer
        }
    }
    pub fn mixer_outputs(&self) -> [(u32, f32); 2] {
        [
            (self.outside_mixer, source_gain_db(self.outside_gain)),
            (
                self.car_mixer,
                source_gain_db(
                    self.outside_gain * self.reference.mixer_controls.outside.car_sound_multiplier,
                ),
            ),
        ]
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u32)]
pub enum AudioKind {
    Eat = 0,
    Burp = 1,
    Fart = 2,
    Urinate = 3,
    Sleep = 4,
    Death = 5,
    Storm = 6,
    AmbientDay = 7,
    AmbientNight = 8,
    Shit = 9,
    EatCrunchy = 10,
    ItemHit = 11,
    CarHit = 12,
    RockHeat = 13,
    MusicBox = 14,
    Footstep = 15,
    StormInside = 16,
    MeleeHit = 17,
    MetalDetector = 18,
    AmbienceZone = 19,
    Sled = 20,
    RadioClick = 21,
    Break = 22,
    ComputerNote = 23,
    Horror = 24,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u32)]
pub enum AudioAction {
    Start = 0,
    Update = 1,
    Stop = 2,
}
#[derive(Clone, Copy, Debug)]
pub struct SurvivalAudioEvent {
    pub session: u32,
    pub emitter: u32,
    pub kind: AudioKind,
    pub action: AudioAction,
    pub clip: u32,
    pub gain: f32,
    pub pitch: f32,
    pub position: [f64; 3],
}

impl AudioState {
    pub fn survival_event(&mut self, mut event: SurvivalAudioEvent) {
        event.session = self.session;
        if self.survival_events.len() < self.survival_events.capacity() {
            self.survival_events.push(event);
        }
    }
    pub fn source_loop(&mut self, kind: AudioKind, emitter: u32, gain: f32, position: [f64; 3]) {
        self.pitched_loop(kind, emitter, gain, None, position);
    }
    /// A loop whose pitch the source sets each frame. `None` keeps the source pitch.
    pub fn pitched_loop(
        &mut self,
        kind: AudioKind,
        emitter: u32,
        gain: f32,
        pitch: Option<f32>,
        position: [f64; 3],
    ) {
        let b = self
            .reference
            .survival
            .iter()
            .find(|b| b.kind == kind as u32)
            .expect("validated survival playback");
        let event = SurvivalAudioEvent {
            session: self.session,
            emitter,
            kind,
            action: AudioAction::Update,
            clip: b.clips[0],
            gain,
            pitch: pitch.unwrap_or(b.pitch[0]),
            position,
        };
        if let Some(old) = self
            .survival_loops
            .iter_mut()
            .find(|v| v.kind == kind && v.emitter == emitter)
        {
            *old = event;
        } else if self.survival_loops.len() < self.survival_loops.capacity() {
            self.survival_loops.push(event);
        }
    }
    /// computerscript.PlayNote: a looped note. Each step the pitch moves by the random
    /// difference with probability `chance`.
    pub fn computer_note(
        &mut self,
        emitter: u32,
        gain: f32,
        pitch: f32,
        (detune, chance): (f32, f32),
        position: [f64; 3],
    ) {
        let pitch = if self.sample() < chance {
            pitch + (self.sample() - 0.5) * detune
        } else {
            pitch
        };
        self.pitched_loop(
            AudioKind::ComputerNote,
            emitter,
            gain,
            Some(pitch),
            position,
        );
    }
    /// musicboxscript.PlayNote: one note with the source random pitch difference.
    pub fn music_note(&mut self, position: [f64; 3], pitch: f32, detune: f32, chance: f32) {
        let b = self
            .reference
            .survival
            .iter()
            .find(|b| b.kind == AudioKind::MusicBox as u32)
            .expect("validated survival playback");
        let (clip, gain) = (b.clips[0], b.gain[0]);
        let pitch = if self.sample() < chance {
            pitch + (self.sample() - 0.5) * detune
        } else {
            pitch
        };
        self.survival_event(SurvivalAudioEvent {
            session: self.session,
            emitter: 0,
            kind: AudioKind::MusicBox,
            action: AudioAction::Start,
            clip,
            gain,
            pitch,
            position,
        });
    }
    pub fn stop_survival_loop(&mut self, kind: AudioKind, emitter: u32) {
        self.survival_loops
            .retain(|v| v.kind != kind || v.emitter != emitter);
    }
    /// Stops the loops of removed storms.
    pub fn remove_storm_loops(&mut self, storms: &[crate::weather::Storm]) {
        self.survival_loops
            .retain(|v| v.kind != AudioKind::Storm || storms.iter().any(|s| s.id.0 == v.emitter));
    }
    /// mainscript.RandomHorrorSound: the player horror source, or stopped.
    pub fn horror(&mut self, playing: bool, position: [f64; 3]) {
        if playing {
            self.source_loop(AudioKind::Horror, 0, 1., position);
        } else {
            self.stop_survival_loop(AudioKind::Horror, 0);
        }
    }
    /// Stops the loops of every emitter of a kind.
    pub fn stop_survival_loops(&mut self, kind: AudioKind) {
        self.survival_loops.retain(|v| v.kind != kind);
    }
}

/// Day and night ambient channels, independent of particle effects.
pub struct AmbientState {
    modifier: f32,
    only_modifier: f32,
}

impl AmbientState {
    pub fn new(reference: &crate::reference::AmbientReference) -> Self {
        Self {
            modifier: reference.modifier,
            only_modifier: reference.only_modifier,
        }
    }

    /// Source `ambienceZone`: a playing zone lowers the day and night ambient.
    pub fn duck(&mut self, lerp: f32) {
        self.only_modifier = self.only_modifier.min(1. - lerp);
    }

    /// `storm` is the source `sandStormAmbientDecrease * inSandStorm` of the previous step.
    pub fn gains(
        &mut self,
        reference: &crate::reference::AmbientReference,
        cycle: &crate::lighting::CycleOutput,
        storm: f32,
        dt: f32,
    ) -> [f32; 2] {
        let night = cycle.night();
        let modifier =
            self.modifier * self.only_modifier * (cycle.biome_color[0] - cycle.biome_color[2]);
        let gains = [reference.day_volume, reference.night_volume]
            .map(|v| ((v[0] + (v[1] - v[0]) * night) * modifier - storm).clamp(0., 1.));
        self.modifier = (self.modifier + dt * reference.recovery_per_second).clamp(0., 1.);
        self.only_modifier =
            (self.only_modifier + dt * reference.recovery_per_second).clamp(0., 1.);
        gains
    }
}
