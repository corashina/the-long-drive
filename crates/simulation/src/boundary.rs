//! The data that crosses between the simulation and the renderer.
//! `SimFrame` goes out once per tick. `SimCommand` comes in once per rendered frame.
//! Poses and state are Pod rows; rare changes are postcard encoded.
use crate::interaction::items::ItemLocation;
use crate::lighting::{
    ControlPoseOutput, CycleOutput, LampMaterialOutput, LightOutput, NodeVisibilityOutput,
};
use crate::reference::DrivingAssists;
use crate::reference::{
    BodyId, ConditionId, ConditionStartup, ItemId, NodeId, OpeningId, ReferenceManifest, SourceId,
};
use crate::simulation::FrameInput;
use crate::simulation::input::DrivingSettings;
use crate::starting_area::StartingArea;
use crate::starting_area::spawned::SpawnedItem;
use crate::vehicle::{PhysicsWorld, VehiclePose};
use crate::world::{GeneratedCapVisual, GeneratedId};
use bevy_platform::collections::HashMap;
use bytemuck::{Pod, Zeroable};
use glam::{Quat, Vec3};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use std::hash::Hash;

/// Index of one row in the frame pose arrays.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Handle(pub u32);

/// Gives dense handles to keys. A retired handle is reused only after `release`.
#[derive(Debug)]
pub struct Handles<K> {
    by_key: HashMap<K, Handle>,
    keys: Vec<Option<K>>,
    free: Vec<Handle>,
}

impl<K: Copy + Eq + Hash> Default for Handles<K> {
    fn default() -> Self {
        Self {
            by_key: HashMap::default(),
            keys: Vec::new(),
            free: Vec::new(),
        }
    }
}

impl<K: Copy + Eq + Hash> Handles<K> {
    /// The handle of `key`, and true if the key is new.
    pub fn assign(&mut self, key: K) -> (Handle, bool) {
        if let Some(&handle) = self.by_key.get(&key) {
            return (handle, false);
        }
        let handle = self.free.pop().unwrap_or_else(|| {
            self.keys.push(None);
            Handle(self.keys.len() as u32 - 1)
        });
        self.keys[handle.0 as usize] = Some(key);
        self.by_key.insert(key, handle);
        (handle, true)
    }
    /// Removes the key. Its handle stays taken until `release`, after the renderer saw the retire.
    pub fn retire(&mut self, handle: Handle) {
        if let Some(key) = self.keys[handle.0 as usize].take() {
            self.by_key.remove(&key);
        }
    }
    /// The handle can go to a new key.
    pub fn release(&mut self, handle: Handle) {
        debug_assert!(
            self.keys[handle.0 as usize].is_none(),
            "release of a live handle"
        );
        self.free.push(handle);
    }
    /// Number of rows the handles index.
    pub fn len(&self) -> usize {
        self.keys.len()
    }
    pub fn is_empty(&self) -> bool {
        self.keys.is_empty()
    }
    pub fn key(&self, handle: Handle) -> Option<K> {
        self.keys[handle.0 as usize]
    }
}

/// One posed object at one tick.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Pod, Zeroable)]
pub struct PoseRow {
    pub position: [f32; 3],
    pub flags: u32,
    pub rotation: [f32; 4],
}

impl PoseRow {
    /// The renderer shows the object.
    pub const VISIBLE: u32 = 1;
    /// The object sits in an inventory slot.
    pub const STORED: u32 = 1 << 1;
    /// The body or item takes part in physics.
    pub const ACTIVE: u32 = 1 << 2;
    pub fn pose(&self) -> VehiclePose {
        VehiclePose {
            position: self.position,
            rotation: self.rotation,
        }
    }
    pub fn new(pose: VehiclePose, flags: u32) -> Self {
        Self {
            position: pose.position,
            flags,
            rotation: pose.rotation,
        }
    }
}

/// One remote player figure. Its joints follow in `SimFrame::remote_joints`.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Pod, Zeroable)]
pub struct RemoteRow {
    /// Peer slot, 1 to 7 on guests and the host, 0 for the host on guests.
    pub slot: u32,
    pub flags: u32,
    /// Player Transform in stream space, like `StateBlock::body_pose`.
    pub body_pose: PoseRow,
    /// `CharacterRoot` relative to the Player.
    pub body_root: PoseRow,
}

impl RemoteRow {
    pub const VISIBLE: u32 = 1;
}

/// Simulation values the renderer reads each frame. New fields come with their readers.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Pod, Zeroable)]
pub struct StateBlock {
    pub simulation_seconds: f64,
    /// Starting area origin in world space.
    pub origin: [f64; 3],
    /// World stream origin: stream space is world space minus this offset.
    pub stream_origin: [f64; 3],
    /// Day cycle active seconds.
    pub cycle_elapsed_seconds: f64,
    /// World stream session.
    pub session: u64,
    /// Required content selection revision.
    pub selection_revision: u64,
    /// Interpolation from `SimFrame::previous` to `SimFrame::current`.
    pub alpha: f32,
    /// Sleep clock factor on frame time.
    pub time_scale: f32,
    pub flags: u32,
    pub fov_degrees: f32,
    /// Items stored in the inventory slots, as `ItemId + 1`. 0 is an empty slot.
    pub stored_items: [u32; 4],
    /// Player body at the current pose; the renderer moves it with the camera interpolation.
    pub body_pose: PoseRow,
    /// `CharacterRoot` relative to the Player.
    pub body_root: PoseRow,
    /// Uniform scale of every joint relative to `CharacterRoot`.
    pub joint_scale: f32,
    /// Sky traffic anchor relative to the stream origin.
    pub sky_anchor: [f32; 3],
    /// Cigarette that owns the ember light, as `ItemId + 1`. 0 is none.
    pub ember_light: u32,
    /// Item that owns the `lightdither` light, as `ItemId + 1`. 0 is none.
    pub dither_light: u32,
    /// Keeps the block free of padding.
    pub pad: u32,
    /// `DeadV` weight: death seconds over the survival death seconds, 0 to 1. 0 while alive.
    pub dead_weight: f32,
    /// Bumps when the audio motion history restarts.
    pub motion_revision: u32,
    /// Source `Cam2FarPlane` of the draw distance.
    pub far_plane: f32,
    /// Sleep camera in stream space. `ACTIVE` while the sleep view shows.
    pub sleep_camera: PoseRow,
    /// Camera of the seated outside view, drop cam or map view. `ACTIVE` while one shows.
    pub view_camera: PoseRow,
    /// Startup roads with prepared meshes.
    pub prepared_roads: u32,
    /// Lens of the worn glasses, as `GlassesState::encode`. Read it with `StateBlock::glasses`.
    pub glasses: u32,
    /// Applied `DOF` volume focus distance in meters.
    pub focus_distance: f32,
    /// `tavcso` focus distance while `BINOCULARS` is set.
    pub binocular_focus: f32,
    /// Player camera. The renderer adds the mouse of commands after `SimFrame::seq_consumed`.
    pub look: LookBlock,
    pub perf: PerfBlock,
}

/// F3 counters of the last frame.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Pod, Zeroable)]
pub struct PerfBlock {
    /// Milliseconds of the fixed steps.
    pub physics_ms: f64,
    /// Milliseconds of the world step.
    pub world_ms: f64,
    pub render_cpu_bytes: f64,
    pub render_gpu_bytes: f64,
    pub frame_steps: u32,
    /// Wheels of the current car that touch the ground.
    pub contacts: u32,
    pub bodies: u32,
    pub colliders: u32,
    pub roads: u32,
    pub terrain: u32,
    pub objects: u32,
    pub jobs: u32,
    pub pending: u32,
    pub storms: u32,
    pub storm_strength: f32,
    pub weather_particles: u32,
}

impl PerfBlock {
    /// Simulated seconds of the frame.
    pub fn frame_seconds(&self) -> f32 {
        self.frame_steps as f32 * crate::simulation::FIXED_DT
    }
}

/// Values that start a simulation.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SimInit {
    pub mode: crate::world::GameMode,
    pub options: crate::world::GameOptions,
    pub draw_distance: f32,
    /// SHA-256 of the manifest bytes. The saves carry it.
    pub manifest_sha256: String,
    /// JSON of the save that the simulation starts from. Empty: the garage of a new game.
    pub save: Vec<u8>,
}

/// Renderer setup values of a new simulation.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct SimStartup {
    /// Player camera.
    pub view: VehiclePose,
    pub fov_degrees: f32,
    /// Source `Cam2FarPlane` of the draw distance.
    pub far_plane: f32,
    /// World stream session.
    pub session: u64,
}

/// Player camera of the last two steps: `parent * yaw * pitch * camera`.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Pod, Zeroable)]
pub struct LookBlock {
    /// Camera position at the previous and the current step, in stream space.
    pub position: [[f32; 3]; 2],
    /// Rotation that carries the look at the previous and the current step. On foot it is near identity.
    pub parent: [[f32; 4]; 2],
    /// Lean and look around after the look.
    pub camera: [f32; 4],
    /// Degrees after the current step.
    pub yaw: f32,
    pub pitch: f32,
    /// Degrees per command mouse unit at the last step with mouse. 0 while the mouse does not look.
    pub gain: f32,
    pub yaw_limits: [f32; 2],
    pub pitch_limits: [f32; 2],
    /// Keeps the block free of padding.
    pub pad: u32,
}

impl LookBlock {
    /// Yaw and pitch after the command `mouse` that no step consumed yet.
    pub fn predict(&self, mouse: [f32; 2]) -> [f32; 2] {
        let [yaw_min, yaw_max] = self.yaw_limits;
        let [pitch_min, pitch_max] = self.pitch_limits;
        [
            (self.yaw + mouse[0] * self.gain).clamp(yaw_min, yaw_max),
            (self.pitch + mouse[1] * self.gain)
                .clamp(-89., 89.)
                .clamp(pitch_min, pitch_max),
        ]
    }

    /// Camera at `alpha` between the steps, with the look after `mouse`.
    pub fn pose(&self, alpha: f32, mouse: [f32; 2]) -> VehiclePose {
        let [yaw, pitch] = self.predict(mouse);
        let [previous, current] = self.parent.map(Quat::from_array);
        VehiclePose {
            position: Vec3::from_array(self.position[0])
                .lerp(Vec3::from_array(self.position[1]), alpha)
                .to_array(),
            rotation: (previous.slerp(current, alpha)
                * Quat::from_rotation_y(-yaw.to_radians())
                * Quat::from_rotation_x(-pitch.to_radians())
                * Quat::from_array(self.camera))
            .to_array(),
        }
    }
}

/// Mouse of sent commands that no step consumed yet. The renderer keeps it.
#[derive(Debug, Default)]
pub struct MouseLook {
    sent: std::collections::VecDeque<(u64, [f32; 2])>,
}

impl MouseLook {
    pub fn send(&mut self, seq: u64, mouse: [f32; 2]) {
        if mouse != [0.; 2] && mouse.iter().all(|v| v.is_finite()) {
            self.sent.push_back((seq, mouse));
        }
    }

    /// Drops the consumed mouse and returns the camera of `frame`.
    pub fn camera(&mut self, frame: &SimFrame) -> VehiclePose {
        while self
            .sent
            .front()
            .is_some_and(|&(seq, _)| seq <= frame.seq_consumed)
        {
            self.sent.pop_front();
        }
        let mouse = self
            .sent
            .iter()
            .fold([0.; 2], |sum, (_, m)| [sum[0] + m[0], sum[1] + m[1]]);
        frame.state.look.pose(frame.state.alpha, mouse)
    }
}

/// Source `szbglasscript`: the active lens of the worn glasses.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GlassesState {
    /// Index in `clothing.glasses_profiles`.
    pub profile: u32,
    /// Source `disableLensFlare`: the sun flare is hidden.
    pub hide_flare: bool,
}

impl GlassesState {
    const HIDE_FLARE: u32 = 1 << 31;
    /// 0 is no glasses. Else bits 0-30 are `profile + 1` and bit 31 is `hide_flare`.
    pub fn encode(state: Option<Self>) -> u32 {
        state.map_or(0, |s| {
            (s.profile + 1) | if s.hide_flare { Self::HIDE_FLARE } else { 0 }
        })
    }
    pub fn decode(value: u32) -> Option<Self> {
        let profile = value & !Self::HIDE_FLARE;
        (profile != 0).then(|| Self {
            profile: profile - 1,
            hide_flare: value & Self::HIDE_FLARE != 0,
        })
    }
}

impl StateBlock {
    /// The lens of the worn glasses.
    pub fn glasses(&self) -> Option<GlassesState> {
        GlassesState::decode(self.glasses)
    }
    pub const STARTED: u32 = 1;
    pub const PAUSED: u32 = 1 << 1;
    pub const SEATED: u32 = 1 << 2;
    pub const SLEEPING: u32 = 1 << 3;
    pub const DEAD: u32 = 1 << 4;
    pub const RECOVERING: u32 = 1 << 5;
    /// The world stream failed.
    pub const WORLD_FAILED: u32 = 1 << 6;
    /// The initial world generation finished.
    pub const GENERATION_READY: u32 = 1 << 7;
    /// The world supports the player and the required content is ready.
    pub const WORLD_READY: u32 = 1 << 8;
    /// Source `otherView()`: the outside view, the drop cam or the map view shows.
    pub const OTHER_VIEW: u32 = 1 << 9;
    /// The storage startup frames are done.
    pub const STORAGE_READY: u32 = 1 << 10;
    /// The world has no generation jobs and no pending work.
    pub const WORLD_IDLE: u32 = 1 << 12;
    /// The other view is the drop cam.
    pub const DROP_CAM: u32 = 1 << 14;
    /// The other view is the map view.
    pub const MAP_VIEW: u32 = 1 << 15;
    /// fpscontroller `Bbinoculars`: the view camera is the binocular camera.
    pub const BINOCULARS: u32 = 1 << 16;
}

/// Valve of one opening: 0 closed, 1 open.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Pod, Zeroable)]
pub struct ValveRow {
    pub opening: u32,
    pub valve: f32,
}

/// One browser audio call. The renderer plays the list of a frame in order.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub enum AudioCall {
    Begin {
        session: u32,
        started: bool,
        paused: bool,
        seconds: f64,
    },
    SurvivalMaster {
        gain: f32,
        revision: u32,
        seconds: f64,
    },
    /// Mixer `PlayerEar` in dB over the game mixers, and the 2D ear ring loop outside it. A
    /// `ring` of 0 stops the loop. `pan` is the stereo pan, -1 left to 1 right.
    PlayerEar {
        db: f32,
        clip: u32,
        ring: f32,
        pan: f32,
    },
    OriginShift([f32; 3]),
    /// The renderer sends its camera as the listener here.
    Listener,
    SurvivalEmitter {
        kind: u32,
        position: [f32; 3],
    },
    Survival {
        kind: u32,
        emitter: u32,
        action: u32,
        clip: u32,
        gain: f32,
        pitch: f32,
        position: [f32; 3],
    },
    /// A reference emitter at the transform of its node.
    NodeEmitter {
        id: u32,
        node: NodeId,
        alive: bool,
    },
    Emitter {
        id: u32,
        position: [f32; 3],
        rotation: [f32; 4],
        alive: bool,
    },
    Mixer {
        id: u32,
        db: f32,
    },
    Route {
        id: u32,
        mixer: u32,
    },
    Voice {
        id: u32,
        playing: bool,
        gain: f32,
        pitch: f32,
        looping: bool,
        clip: u32,
    },
    Event {
        session: u32,
        sequence: u32,
        binding: u32,
        emitter: u32,
        position: [f32; 3],
        gain: f32,
        pitch: f32,
        seconds: f64,
    },
    Radio {
        emitter: u32,
        frame: crate::simulation::radio::RadioFrame,
    },
    End,
}

/// Renderer values of one rendered frame.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct RenderInput {
    /// Simulated seconds of the frame.
    pub dt: f32,
    /// Real seconds of the frame.
    pub real_dt: f32,
    /// Camera position of the last rendered frame.
    pub listener: Option<[f32; 3]>,
}

/// HUD values of one tick.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Pod, Zeroable)]
pub struct HudBlock {
    pub cooldown_remaining: f64,
    /// Player focus in world space.
    pub absolute: [f64; 3],
    pub origin: [f64; 3],
    pub trip_m: f64,
    pub hp: f32,
    pub food: f32,
    pub water: f32,
    pub bowel: f32,
    pub bladder: f32,
    pub opacity: f32,
    pub look_scale: f32,
    pub sleep_phase: u32,
    pub sleep_charge: f32,
    pub sleep_progress: f32,
    pub flags: u32,
    pub speed_kmh: f32,
    /// Player focus in stream space.
    pub focus: [f32; 3],
    /// Wheel contact bits.
    pub contacts: u32,
    pub surfaces: u32,
    pub colliders: u32,
    pub rebases: u32,
    pub fuel: f32,
    pub radio_volume: f32,
    pub radio_frequency: f32,
    /// Inventory slot items, as `ItemId`. 0 is empty.
    pub slots: [u32; 4],
    pub selected: u32,
    pub swap_progress: f32,
    /// `weaponCrossHairIcon` of the gun in the hand as a `UiReference::sprites` index + 1.
    /// 0 shows no weapon crosshair.
    pub weapon_icon: u32,
    /// `weaponCrossHairColor` or `weaponCrossHairColorEmpty` as RGBA8, R in the low byte.
    pub weapon_color: u32,
    /// Gear of the current car: -1 reverse, 0 neutral. 0 without a car.
    pub gear: i32,
    /// Driving assists: bit 0 automatic clutch, bit 1 automatic gears.
    pub assists: u32,
    /// Steer, throttle, brake, clutch, handbrake.
    pub controls: [f32; 5],
    /// Source `postprocesscontrol.h.root` body speed: the seat owner, else the player.
    pub view_speed_kmh: f32,
}

impl HudBlock {
    pub const DEAD: u32 = 1;
    pub const MENU_DUE: u32 = 1 << 1;
    pub const SLEEP_ELIGIBLE: u32 = 1 << 2;
    pub const SLEEP_HINT: u32 = 1 << 3;
    /// Safe for autosave when the grab key is up.
    pub const AUTOSAVE_SAFE: u32 = 1 << 4;
    /// The player aims at a radio knob.
    pub const RADIO: u32 = 1 << 6;
    pub const RADIO_AM: u32 = 1 << 7;
    /// The player drives a car.
    pub const DRIVING: u32 = 1 << 8;
    /// A computer takes the typed text.
    pub const TYPING: u32 = 1 << 9;
}

/// Interaction feedback text and values. Sent when they change.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Feedback {
    pub actions: u32,
    pub context: u32,
    pub name: String,
    pub notice: String,
    pub carrying: bool,
    pub amount: f32,
    pub capacity: f32,
    pub has_name: bool,
    pub no_text: bool,
    /// The `ls.w` index of the tank name. Valid when `has_name` is set.
    pub fluid_word: u32,
    pub held_name: String,
    pub throw_charge: f32,
    pub throw_active: bool,
    pub rpm: f32,
    pub ignition: bool,
    pub running: bool,
    pub starter: bool,
    pub seated: bool,
    pub handbrake: bool,
    pub quantities: [f32; 6],
    pub order: u32,
    pub light_control: u32,
    pub lmb_use: bool,
    pub prompts: u8,
    pub computer: u8,
    /// `interaction::crosshair::Crosshair` layer bits.
    pub crosshair: u32,
    /// Source `weightM` of the carried item. It fades the pickup layer children.
    pub weight: f32,
    /// `interaction::help_texts` bits of the help box under the use ray.
    pub help: u8,
}

/// Use and limit of one world budget.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Pod, Zeroable)]
pub struct BudgetRow {
    pub used: f64,
    pub limit: f64,
}

/// Presentation view of one world structure slot. The renderer keeps the last view of each slot.
/// Ranges index `SimFrame::structure_poses`, `hidden_nodes`, `node_materials` and `node_colors`.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Pod, Zeroable)]
pub struct StructureRow {
    pub slot: u32,
    pub flags: u32,
    /// Emission color of a lamp item. Valid with `LAMP`.
    pub lamp: [f32; 4],
    /// Computer item of the display output, as `ItemId + 1`. 0 is none.
    pub display_map: u32,
    pub poses: [u32; 2],
    pub hidden: [u32; 2],
    pub materials: [u32; 2],
    pub colors: [u32; 2],
}

impl StructureRow {
    /// The content bindings are ready. Without it the renderer hides the structure.
    pub const READY: u32 = 1;
    /// `lamp` is the emission of the emissive renderers.
    pub const LAMP: u32 = 1 << 1;
    /// The structure is a computer display.
    pub const DISPLAY: u32 = 1 << 2;
    /// The display is on.
    pub const DISPLAY_ON: u32 = 1 << 3;
    /// The player wears the item. The camera does not draw its wearable renderers.
    pub const WORN: u32 = 1 << 4;
    /// The camera does not draw any renderer of the structure.
    pub const CAMERA_HIDDEN: u32 = 1 << 5;
}

/// One node write of a structure view. `node` indexes the prototype nodes,
/// or the prototype bodies for `BODY`.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Pod, Zeroable)]
pub struct NodePoseRow {
    pub node: u32,
    pub kind: u32,
    pub translation: [f32; 3],
    pub rotation: [f32; 4],
    pub scale: [f32; 3],
}

impl NodePoseRow {
    /// A pose relative to the glTF parent.
    pub const LOCAL: u32 = 0;
    /// Only the local rotation of a spawned node.
    pub const ROTATION: u32 = 1;
    /// A body pose in stream space. Its nodes follow.
    pub const BODY: u32 = 2;
    /// A content node pose in stream space.
    pub const CONTENT: u32 = 3;
}

/// The condition color of the renderers of one prototype node.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Pod, Zeroable)]
pub struct NodeColorRow {
    pub node: u32,
    pub color: [f32; 4],
}

/// The material that the renderers of one prototype node show.
/// `source` indexes the `Change::MaterialSource` list.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Pod, Zeroable)]
pub struct MaterialRow {
    pub node: u32,
    pub source: u32,
}

/// State of one condition owner. Frames carry changed owners; the first frame carries all.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Pod, Zeroable)]
pub struct ConditionRow {
    pub id: u32,
    pub state: u32,
    pub state2: u32,
    pub state3: u32,
    pub repair_progress: f32,
    pub color: [f32; 4],
}

impl ConditionRow {
    pub fn new(id: ConditionId, s: &ConditionStartup) -> Self {
        Self {
            id: id.0,
            state: u32::from(s.state),
            state2: s.state2,
            state3: s.state3,
            repair_progress: s.repair_progress,
            color: s.color,
        }
    }
    pub fn startup(&self) -> ConditionStartup {
        ConditionStartup {
            state: self.state as u8,
            state2: self.state2,
            state3: self.state3,
            repair_progress: self.repair_progress,
            color: self.color,
        }
    }
}

/// Per-handle item state. Body rows stay zero.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Pod, Zeroable)]
pub struct ItemRow {
    /// Gib shrink scale; 1 for other items.
    pub scale: f32,
    /// Food bite index.
    pub bite: u32,
}

/// One cigarette: part blend and ember output.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Pod, Zeroable)]
pub struct CigaretteRow {
    pub item: u32,
    pub blend: f32,
    pub color: [f32; 4],
    pub light_intensity: f32,
}

/// One sky traffic plane relative to the sky anchor.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Pod, Zeroable)]
pub struct PlaneRow {
    pub id: u32,
    pub prefab: u32,
    pub offset: [f32; 3],
    pub heading: [f32; 4],
}

/// One rabbit in physics space. The renderer samples the blend tree at `phase` and `value`,
/// the action state, and the override layer at `twitch`. A dead rabbit takes its bone rows
/// instead.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Pod, Zeroable)]
pub struct CreatureRow {
    pub id: u32,
    /// `PoseRow::VISIBLE`, `DEAD`, `NO_EYES`, `EVIL_EYES` and `GREEN_EYES`.
    pub flags: u32,
    pub position: [f32; 3],
    pub rotation: [f32; 4],
    pub scale: f32,
    pub phase: f32,
    pub value: f32,
    pub twitch: f32,
    /// `NO_ACTION`, `ROAR` or `ATTACK`, with its normalized time and weight.
    pub action: u32,
    pub action_time: f32,
    pub action_weight: f32,
    /// First index and count of the bone rows in `SimFrame::creature_bones`.
    pub bones: [u32; 2],
}
impl CreatureRow {
    pub const DEAD: u32 = 1 << 8;
    /// `LooseEyes` hid the eye objects.
    pub const NO_EYES: u32 = 1 << 9;
    /// `GetEvil(natural: true)` set `EyeEvil`.
    pub const EVIL_EYES: u32 = 1 << 10;
    /// `GetEvil(natural: false)` set `EyeGreen`.
    pub const GREEN_EYES: u32 = 1 << 11;
    pub const NO_ACTION: u32 = 0;
    pub const ROAR: u32 = 1;
    pub const ATTACK: u32 = 2;
}

/// One bone body of a dead rabbit, relative to the unscaled creature row pose.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Pod, Zeroable)]
pub struct CreatureBoneRow {
    /// Rig node index.
    pub node: u32,
    pub position: [f32; 3],
    pub rotation: [f32; 4],
}

/// One bullet hole in stream space. `kind` indexes `GunsReference::decals`.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Pod, Zeroable)]
pub struct DecalRow {
    pub previous: PoseRow,
    pub current: PoseRow,
    pub kind: u32,
    pub scale: f32,
}

/// One live particle. Positions are relative to the stream origin (contrails: the sky anchor).
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Pod, Zeroable)]
pub struct ParticleRow {
    pub position: [f32; 3],
    pub size: f32,
    pub color: [f32; 4],
    pub rotation: f32,
    pub age: f32,
    pub lifetime: f32,
    pub uv_draw: f32,
}

/// One particle system and its rows in `SimFrame::particles`.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Pod, Zeroable)]
pub struct ParticleGroupRow {
    /// `ParticleGroupRow::URINE` and the other kinds.
    pub kind: u32,
    /// Kind id: tool, wash, opening, storm, plane, item, effect or car system.
    pub a: u32,
    /// Car prototype.
    pub b: u32,
    /// Car chassis body index.
    pub c: u32,
    /// System index inside the owner.
    pub source: u32,
    pub start: u32,
    pub count: u32,
    /// Rows come from a fluid contact: no age or sheet.
    pub fluid: u32,
}

impl ParticleGroupRow {
    pub const URINE: u32 = 0;
    pub const BRUSH: u32 = 1;
    pub const SPRAY: u32 = 2;
    pub const WASH: u32 = 3;
    pub const OUTLET: u32 = 4;
    pub const SPRAY_BOTTLE: u32 = 5;
    pub const CAR: u32 = 6;
    pub const STORM: u32 = 7;
    pub const MELEE: u32 = 8;
    pub const SKY: u32 = 9;
    pub const EMBER: u32 = 10;
    pub const EXHALE: u32 = 11;
    pub const COBWEB: u32 = 12;
    /// `mainscript.ParticleShootStart` (0) and `ParticleShootEnd` (1).
    pub const TRACER: u32 = 13;
    /// `explosionscript.particle`; source is the explosion type.
    pub const EXPLOSION: u32 = 14;
    /// `mainscript.FoodParticles`; source is the food particle type.
    pub const FOOD: u32 = 15;
    /// A loot tool: `a` is the item, `b` the `conditions.prototype_tools` index.
    pub const LOOT_TOOL: u32 = 16;
    /// fpscontroller `heatHaze`: the old particle heat haze.
    pub const HEAT_HAZE: u32 = 17;
    /// Car systems in `a`.
    pub const CAR_EXHAUST: u32 = 0;
    /// Wheel effects: source is wheel * 8 + system (0 dust, 1.. grass).
    pub const CAR_WHEEL: u32 = 1;
    /// Skid bursts: `tiresmoke` (0) or `tiresand` (1).
    pub const CAR_SKID: u32 = 2;
    /// `carscript.CoolantSteam`; source 0.
    pub const CAR_STEAM: u32 = 3;
}

/// The `(car system, source)` pairs of every particle system of a car prototype.
pub fn car_particle_sources(
    reference: &ReferenceManifest,
    prototype: usize,
) -> impl Iterator<Item = (u32, usize)> + '_ {
    let record = crate::starting_area::world_vehicle(reference, prototype);
    let exhaust = (0..record.exhaust.len()).map(|i| (ParticleGroupRow::CAR_EXHAUST, i));
    let wheels = record
        .wheel_effects
        .iter()
        .enumerate()
        .flat_map(|(wheel, w)| {
            let dust = w.dust.iter().map(|_| 0);
            let grass = 1..=w.grass.len();
            dust.chain(grass)
                .map(move |system| (ParticleGroupRow::CAR_WHEEL, wheel * 8 + system))
        });
    let skid = (0..2).map(|i| (ParticleGroupRow::CAR_SKID, i));
    let steam = record
        .coolant_steam
        .iter()
        .map(|_| (ParticleGroupRow::CAR_STEAM, 0));
    exhaust.chain(wheels).chain(skid).chain(steam)
}

/// Day cycle and lamp outputs.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct FrameLighting {
    pub cycle: CycleOutput,
    pub road_lamp: LightOutput,
}

/// What the renderer needs to build a new handle's visual.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub enum SpawnData {
    /// The visual already exists in the reference scene.
    Bound,
    /// A spawned item prototype body.
    Spawned(SpawnedItem),
    /// A food item that is not world content.
    Food { prototype: u32 },
}

/// What a pose handle stands for.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum PoseKey {
    Body(BodyId),
    Item(ItemId),
}

/// A rare frame event.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum Change {
    /// The handle row now poses `key`.
    Spawn {
        handle: Handle,
        key: PoseKey,
        data: SpawnData,
    },
    /// Skin joint nodes of the player body, in `SimFrame::joints` order.
    PlayerRig(Vec<NodeId>),
    /// The renderer drops the handle, then releases it.
    Retire(Handle),
    /// Home cable rope aims. Empty: no pole in range, the root stays hidden.
    HomeCable(Vec<RopeAim>),
    Feedback(Box<Feedback>),
    /// A world structure takes the slot.
    StructureAdd {
        slot: u32,
        id: GeneratedId,
    },
    /// The structure of the slot is gone. The slot can take a new structure.
    StructureRemove(u32),
    /// Cap visuals of the structure in the slot.
    StructureCaps {
        slot: u32,
        caps: Vec<GeneratedCapVisual>,
    },
    /// The next material source index names this source.
    MaterialSource(SourceId),
    /// A computer screen changed or is new.
    Screen(Box<crate::interaction::computer::screen::ScreenView>),
    /// The computers that have a screen. Screens of other items drop.
    Screens(Vec<ItemId>),
    /// Plate texts of a world car, in `WorldVehicleReference::plates` order.
    Plates {
        id: GeneratedId,
        texts: Vec<String>,
    },
    /// The simulation part of a world measurement, as JSON.
    Measurements(String),
    /// The JSON answer to one sandbox command.
    Sandbox(String),
    Failure(String),
    /// Renderer credit of the world step for the renderer report of the command.
    Grant(crate::world::PresentationGrant),
    /// The answer to a browser save request.
    SaveResult {
        id: u32,
        kind: String,
        payload: String,
        error: String,
    },
}

/// Local rotation and z scale of one home cable rope.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct RopeAim {
    pub node: NodeId,
    pub rotation: [f32; 4],
    pub scale_z: f32,
}

/// The simulation output of one tick.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SimFrame {
    /// The last command sequence the simulation consumed.
    pub seq_consumed: u64,
    pub state: StateBlock,
    pub hud: HudBlock,
    /// Poses at the step before `current`, indexed by handle.
    pub previous: Vec<PoseRow>,
    pub current: Vec<PoseRow>,
    pub items: Vec<ItemRow>,
    pub valves: Vec<ValveRow>,
    /// Player body joints relative to `CharacterRoot`.
    pub joints: Vec<PoseRow>,
    /// Remote players of a multiplayer session.
    pub remotes: Vec<RemoteRow>,
    /// Joints of each remote in `remotes` order, `joints` rows per remote.
    pub remote_joints: Vec<PoseRow>,
    /// World poses of the ragdoll bodies in `RagdollReference::bodies` order. Empty while the ragdoll is inactive.
    pub ragdoll: Vec<PoseRow>,
    pub cigarettes: Vec<CigaretteRow>,
    pub particle_groups: Vec<ParticleGroupRow>,
    pub particles: Vec<ParticleRow>,
    /// Live bullet holes, oldest first.
    pub decals: Vec<DecalRow>,
    pub planes: Vec<PlaneRow>,
    pub creatures: Vec<CreatureRow>,
    pub creature_bones: Vec<CreatureBoneRow>,
    pub conditions: Vec<ConditionRow>,
    /// 1 when the wash shows, in `ConditionRecord::washes` order.
    pub washes: Vec<u8>,
    /// Brush model poses, in `ConditionRecord::brushes` order.
    pub brush_poses: Vec<PoseRow>,
    /// Spray colors, in `ConditionRecord::sprays` order.
    pub spray_colors: Vec<[f32; 4]>,
    /// Browser audio calls. Empty without audio.
    pub audio: Vec<AudioCall>,
    /// World budgets in `BUDGET_LABELS` order. Empty after the start.
    pub budgets: Vec<BudgetRow>,
    pub lighting: FrameLighting,
    /// Lights in `LightingRecord::lights` order.
    pub lights: Vec<LightOutput>,
    pub lamp_materials: Vec<LampMaterialOutput>,
    pub control_poses: Vec<ControlPoseOutput>,
    pub node_visibility: Vec<NodeVisibilityOutput>,
    /// Structures whose view changed.
    pub structures: Vec<StructureRow>,
    pub structure_poses: Vec<NodePoseRow>,
    /// Prototype nodes that the structure hides.
    pub hidden_nodes: Vec<u32>,
    pub node_materials: Vec<MaterialRow>,
    pub node_colors: Vec<NodeColorRow>,
    pub changes: Vec<Change>,
    /// Changes of the renderer world copy.
    pub world: crate::world::WorldDelta,
}

impl SimFrame {
    /// Removes the events: the renderer applies each frame once. The state stays.
    pub fn clear_events(&mut self) {
        self.conditions.clear();
        self.audio.clear();
        self.structures.clear();
        self.changes.clear();
        self.world = Default::default();
    }
    pub fn valve(&self, opening: OpeningId) -> f32 {
        self.valves
            .iter()
            .find(|v| v.opening == opening.0)
            .map_or(0., |v| v.valve)
    }
    pub fn item_stored(&self, item: ItemId) -> bool {
        self.state.stored_items.contains(&(item.0 + 1))
    }
}

/// The renderer input of one rendered frame.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct SimCommand {
    pub seq: u64,
    /// Real seconds of the frame.
    pub delta_seconds: f64,
    pub input: FrameInput,
    /// Menu actions: `PAUSE`, `START`, `RECOVER`.
    pub actions: u32,
    /// The renderer shows the garage.
    pub ready: bool,
    /// A save request is pending. The simulation drops the input and the actions.
    pub hold: bool,
    pub assists: Option<DrivingAssists>,
    /// Stored settings from the browser. The simulation validates them.
    pub settings: Option<DrivingSettings>,
    /// Changed game options. They apply at once, also while paused.
    pub options: Option<crate::world::GameOptions>,
    pub player_settings: Option<crate::reference::PlayerSettings>,
    /// Source `BOldHeatHaze`.
    pub old_heat_haze: bool,
    /// Local month and day.
    pub calendar: [u8; 2],
    /// The browser asks for a world measurement.
    pub measure: bool,
    pub world: WorldCommand,
    /// Save requests, in order.
    pub save: Vec<SaveCommand>,
    /// Camera position of the last rendered frame.
    pub listener: Option<[f32; 3]>,
    /// Test sandbox commands of the `?sandbox` page, as one JSON array.
    pub sandbox: Option<String>,
}

/// One save input of the renderer.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum SaveCommand {
    /// A browser request: JSON with id, kind and payload.
    Request {
        json: String,
        /// Game mode of a new world, from the browser menu.
        mode: crate::world::GameMode,
        /// Renderer scene epoch. A fresh session commits only in its epoch.
        epoch: u32,
    },
    /// Drops the prepared session of request `id`.
    Cancel(u32),
}

/// Renderer input of the world step.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct WorldCommand {
    /// Facts of the renderer steps since the last command, in order.
    pub facts: crate::world::RendererFacts,
    /// Renderer budget use. `None`: no renderer step since the last command.
    pub report: Option<crate::world::PresentationReport>,
    /// Camera views that select required content. Empty: no complete camera view.
    pub views: Vec<crate::world::RequiredView>,
    pub lamps: LampAdmission,
    /// A renderer failure. The world stops.
    pub failure: Option<String>,
}

/// Lamps the renderer admitted in its last step.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub enum LampAdmission {
    /// No change since the last command.
    #[default]
    Same,
    /// The renderer did not finish lamp preparation.
    Pending,
    Ready(Vec<GeneratedId>),
}

impl WorldCommand {
    /// Adds a later command.
    pub fn merge(&mut self, next: Self) {
        self.facts.extend(next.facts);
        self.report = match (self.report.take(), next.report) {
            (Some(report), Some(next)) => Some(report.merge(next)),
            (report, next) => next.or(report),
        };
        self.views = next.views;
        if next.lamps != LampAdmission::Same {
            self.lamps = next.lamps;
        }
        self.failure = self.failure.take().or(next.failure);
    }
}

impl SimCommand {
    pub const PAUSE: u32 = 1 << 1;
    pub const START: u32 = 1 << 2;
    pub const RECOVER: u32 = 1 << 4;
    pub const ACTIONS: u32 = Self::PAUSE | Self::START | Self::RECOVER;

    /// Adds a later command. A held command drops the input before it.
    pub fn merge(&mut self, next: Self) {
        self.seq = next.seq;
        self.delta_seconds += next.delta_seconds;
        if next.hold {
            self.input.clear();
        } else {
            self.input.merge(next.input);
        }
        self.actions |= next.actions;
        self.ready = next.ready;
        self.hold = next.hold;
        self.assists = next.assists.or(self.assists);
        self.settings = next.settings.or(self.settings);
        self.options = next.options.or(self.options);
        self.player_settings = next.player_settings.or(self.player_settings);
        self.calendar = next.calendar;
        self.measure |= next.measure;
        self.world.merge(next.world);
        self.save.extend(next.save);
        self.listener = next.listener.or(self.listener);
    }
}

#[derive(Debug, PartialEq)]
pub struct DecodeError(pub String);

/// Writes the whole frame into `out`. `out` keeps its capacity between frames.
pub fn encode(frame: &SimFrame, out: &mut Vec<u8>) {
    out.clear();
    write(frame, out, None);
}

/// Encodes the frames of one stream. Pose rows that did not change since the last frame are left out.
/// The decoder must read every frame of the stream, in order, into the same `SimFrame`.
#[derive(Default)]
pub struct FrameEncoder {
    previous: Vec<PoseRow>,
    current: Vec<PoseRow>,
    items: Vec<ItemRow>,
    changed: Vec<u32>,
    started: bool,
}

impl FrameEncoder {
    pub fn encode(&mut self, frame: &SimFrame, out: &mut Vec<u8>) {
        out.clear();
        write(frame, out, Some(self));
    }
}

/// Marks a whole pose section. Otherwise the count of changed slots follows the row count.
const ALL_POSES: u32 = u32::MAX;

/// Writes `previous`, `current` and `items`. They have one row per handle slot.
fn put_poses(out: &mut Vec<u8>, frame: &SimFrame, encoder: Option<&mut FrameEncoder>) {
    let rows = frame.current.len();
    debug_assert!(frame.previous.len() == rows && frame.items.len() == rows);
    out.extend_from_slice(&(rows as u32).to_le_bytes());
    let sent = match encoder {
        Some(sent) if sent.started => sent,
        encoder => {
            out.extend_from_slice(&ALL_POSES.to_le_bytes());
            out.extend_from_slice(bytemuck::cast_slice(&frame.previous));
            out.extend_from_slice(bytemuck::cast_slice(&frame.current));
            out.extend_from_slice(bytemuck::cast_slice(&frame.items));
            if let Some(encoder) = encoder {
                encoder.started = true;
                encoder.previous.clone_from(&frame.previous);
                encoder.current.clone_from(&frame.current);
                encoder.items.clone_from(&frame.items);
            }
            return;
        }
    };
    // Bytes compare exactly, so a -0.0 or NaN change is sent too.
    fn same<T: Pod>(a: &T, b: &T) -> bool {
        bytemuck::bytes_of(a) == bytemuck::bytes_of(b)
    }
    sent.previous.resize(rows, PoseRow::default());
    sent.current.resize(rows, PoseRow::default());
    sent.items.resize(rows, ItemRow::default());
    sent.changed.clear();
    for slot in 0..rows {
        if !same(&frame.previous[slot], &sent.previous[slot])
            || !same(&frame.current[slot], &sent.current[slot])
            || !same(&frame.items[slot], &sent.items[slot])
        {
            sent.previous[slot] = frame.previous[slot];
            sent.current[slot] = frame.current[slot];
            sent.items[slot] = frame.items[slot];
            sent.changed.push(slot as u32);
        }
    }
    out.extend_from_slice(&(sent.changed.len() as u32).to_le_bytes());
    out.extend_from_slice(bytemuck::cast_slice(&sent.changed));
    for &slot in &sent.changed {
        out.extend_from_slice(bytemuck::bytes_of(&frame.previous[slot as usize]));
    }
    for &slot in &sent.changed {
        out.extend_from_slice(bytemuck::bytes_of(&frame.current[slot as usize]));
    }
    for &slot in &sent.changed {
        out.extend_from_slice(bytemuck::bytes_of(&frame.items[slot as usize]));
    }
}

fn take_poses(rest: &mut &[u8], frame: &mut SimFrame) -> Result<(), DecodeError> {
    let count = |rest: &mut &[u8]| -> Result<usize, DecodeError> {
        Ok(u32::from_le_bytes(take(rest, 4)?.try_into().expect("4 bytes")) as usize)
    };
    let rows = count(rest)?;
    let changed = count(rest)?;
    if changed == ALL_POSES as usize {
        for target in [&mut frame.previous, &mut frame.current] {
            let bytes = take(rest, rows * size_of::<PoseRow>())?;
            target.clear();
            target.extend(
                bytes
                    .chunks_exact(size_of::<PoseRow>())
                    .map(bytemuck::pod_read_unaligned::<PoseRow>),
            );
        }
        let bytes = take(rest, rows * size_of::<ItemRow>())?;
        frame.items.clear();
        frame.items.extend(
            bytes
                .chunks_exact(size_of::<ItemRow>())
                .map(bytemuck::pod_read_unaligned::<ItemRow>),
        );
        return Ok(());
    }
    frame.previous.resize(rows, PoseRow::default());
    frame.current.resize(rows, PoseRow::default());
    frame.items.resize(rows, ItemRow::default());
    let slots = take(rest, changed * 4)?;
    let previous = take(rest, changed * size_of::<PoseRow>())?;
    let current = take(rest, changed * size_of::<PoseRow>())?;
    let items = take(rest, changed * size_of::<ItemRow>())?;
    fn rows_of(bytes: &[u8], size: usize) -> std::slice::ChunksExact<'_, u8> {
        bytes.chunks_exact(size)
    }
    for (((slot, previous), current), item) in rows_of(slots, 4)
        .zip(rows_of(previous, size_of::<PoseRow>()))
        .zip(rows_of(current, size_of::<PoseRow>()))
        .zip(rows_of(items, size_of::<ItemRow>()))
    {
        let slot = u32::from_le_bytes(slot.try_into().expect("4 bytes")) as usize;
        if slot >= rows {
            return Err(DecodeError("pose slot out of range".into()));
        }
        frame.previous[slot] = bytemuck::pod_read_unaligned(previous);
        frame.current[slot] = bytemuck::pod_read_unaligned(current);
        frame.items[slot] = bytemuck::pod_read_unaligned(item);
    }
    Ok(())
}

fn write(frame: &SimFrame, out: &mut Vec<u8>, encoder: Option<&mut FrameEncoder>) {
    out.extend_from_slice(&frame.seq_consumed.to_le_bytes());
    out.extend_from_slice(bytemuck::bytes_of(&frame.state));
    out.extend_from_slice(bytemuck::bytes_of(&frame.hud));
    put_poses(out, frame, encoder);
    put_rows(out, &frame.valves);
    put_rows(out, &frame.joints);
    put_rows(out, &frame.remotes);
    put_rows(out, &frame.remote_joints);
    put_rows(out, &frame.ragdoll);
    put_rows(out, &frame.cigarettes);
    put_rows(out, &frame.particle_groups);
    put_rows(out, &frame.particles);
    put_rows(out, &frame.decals);
    put_rows(out, &frame.planes);
    put_rows(out, &frame.creatures);
    put_rows(out, &frame.creature_bones);
    put_rows(out, &frame.conditions);
    put_rows(out, &frame.washes);
    put_rows(out, &frame.brush_poses);
    put_rows(out, &frame.spray_colors);
    put_rows(out, &frame.budgets);
    put_rows(out, &frame.structures);
    put_rows(out, &frame.structure_poses);
    put_rows(out, &frame.hidden_nodes);
    put_rows(out, &frame.node_materials);
    put_rows(out, &frame.node_colors);
    put(out, &frame.lighting);
    put_list(out, &frame.lights);
    put_list(out, &frame.lamp_materials);
    put_list(out, &frame.control_poses);
    put_list(out, &frame.node_visibility);
    put_list(out, &frame.audio);
    put(out, &frame.changes);
    put(out, &frame.world);
}

/// Reads `bytes` into `frame`, reusing its arrays.
pub fn decode(bytes: &[u8], frame: &mut SimFrame) -> Result<(), DecodeError> {
    let mut rest = bytes;
    read(&mut rest, frame)?;
    if !rest.is_empty() {
        return Err(DecodeError("long frame".into()));
    }
    Ok(())
}

fn read(rest: &mut &[u8], frame: &mut SimFrame) -> Result<(), DecodeError> {
    frame.seq_consumed = u64::from_le_bytes(take(rest, 8)?.try_into().expect("8 bytes"));
    frame.state = bytemuck::pod_read_unaligned(take(rest, size_of::<StateBlock>())?);
    frame.hud = bytemuck::pod_read_unaligned(take(rest, size_of::<HudBlock>())?);
    take_poses(rest, frame)?;
    take_rows(rest, &mut frame.valves)?;
    take_rows(rest, &mut frame.joints)?;
    take_rows(rest, &mut frame.remotes)?;
    take_rows(rest, &mut frame.remote_joints)?;
    take_rows(rest, &mut frame.ragdoll)?;
    take_rows(rest, &mut frame.cigarettes)?;
    take_rows(rest, &mut frame.particle_groups)?;
    take_rows(rest, &mut frame.particles)?;
    take_rows(rest, &mut frame.decals)?;
    take_rows(rest, &mut frame.planes)?;
    take_rows(rest, &mut frame.creatures)?;
    take_rows(rest, &mut frame.creature_bones)?;
    take_rows(rest, &mut frame.conditions)?;
    take_rows(rest, &mut frame.washes)?;
    take_rows(rest, &mut frame.brush_poses)?;
    take_rows(rest, &mut frame.spray_colors)?;
    take_rows(rest, &mut frame.budgets)?;
    take_rows(rest, &mut frame.structures)?;
    take_rows(rest, &mut frame.structure_poses)?;
    take_rows(rest, &mut frame.hidden_nodes)?;
    take_rows(rest, &mut frame.node_materials)?;
    take_rows(rest, &mut frame.node_colors)?;
    frame.lighting = take_value(rest)?;
    take_list(rest, &mut frame.lights)?;
    take_list(rest, &mut frame.lamp_materials)?;
    take_list(rest, &mut frame.control_poses)?;
    take_list(rest, &mut frame.node_visibility)?;
    take_list(rest, &mut frame.audio)?;
    frame.changes = take_value(rest)?;
    frame.world = take_value(rest)?;
    Ok(())
}

fn put_rows<T: Pod>(out: &mut Vec<u8>, rows: &[T]) {
    out.extend_from_slice(&(rows.len() as u32).to_le_bytes());
    out.extend_from_slice(bytemuck::cast_slice(rows));
}

fn put<T: Serialize>(out: &mut Vec<u8>, value: &T) {
    *out = postcard::to_extend(value, std::mem::take(out)).expect("frame value encode");
}

/// Serde rows, one postcard value each. Decode reuses the vector.
fn put_list<T: Serialize>(out: &mut Vec<u8>, rows: &[T]) {
    out.extend_from_slice(&(rows.len() as u32).to_le_bytes());
    for row in rows {
        put(out, row);
    }
}

fn take_value<T: DeserializeOwned>(rest: &mut &[u8]) -> Result<T, DecodeError> {
    let (value, tail) = postcard::take_from_bytes(rest).map_err(|e| DecodeError(e.to_string()))?;
    *rest = tail;
    Ok(value)
}

fn take_list<T: DeserializeOwned>(rest: &mut &[u8], rows: &mut Vec<T>) -> Result<(), DecodeError> {
    let n = u32::from_le_bytes(take(rest, 4)?.try_into().expect("4 bytes")) as usize;
    rows.clear();
    for _ in 0..n {
        rows.push(take_value(rest)?);
    }
    Ok(())
}

fn take<'a>(rest: &mut &'a [u8], n: usize) -> Result<&'a [u8], DecodeError> {
    if rest.len() < n {
        return Err(DecodeError("short frame".into()));
    }
    let (head, tail) = rest.split_at(n);
    *rest = tail;
    Ok(head)
}

fn take_rows<T: Pod>(rest: &mut &[u8], rows: &mut Vec<T>) -> Result<(), DecodeError> {
    let n = u32::from_le_bytes(take(rest, 4)?.try_into().expect("4 bytes")) as usize;
    let bytes = take(rest, n * size_of::<T>())?;
    rows.clear();
    rows.extend(
        bytes
            .chunks_exact(size_of::<T>())
            .map(bytemuck::pod_read_unaligned::<T>),
    );
    Ok(())
}

pub fn encode_command(command: &SimCommand, out: &mut Vec<u8>) {
    out.clear();
    *out = postcard::to_extend(command, std::mem::take(out)).expect("command encode");
}

pub fn encode_startup(startup: &SimStartup) -> Vec<u8> {
    postcard::to_stdvec(startup).expect("startup encode")
}

pub fn decode_startup(bytes: &[u8]) -> Result<SimStartup, DecodeError> {
    postcard::from_bytes(bytes).map_err(|e| DecodeError(e.to_string()))
}

pub fn decode_command(bytes: &[u8]) -> Result<SimCommand, DecodeError> {
    postcard::from_bytes(bytes).map_err(|e| DecodeError(e.to_string()))
}

/// Pose handles of the starting area bodies and items.
#[derive(Debug, Default)]
pub struct FramePoses {
    handles: Handles<PoseKey>,
    /// Tick stamp per handle. A live handle without the current stamp left the area.
    seen: Vec<u32>,
    stamp: u32,
    /// Joint count of the last `PlayerRig` change.
    rig_joints: Option<usize>,
    /// Handles that the last frame retired. The renderer applies every frame in order,
    /// so the next frame can give them to new keys.
    retired: Vec<Handle>,
}

impl FramePoses {
    /// Writes the area poses into `frame` and adds `Spawn` and `Retire` changes.
    pub fn fill(
        &mut self,
        reference: &ReferenceManifest,
        area: &StartingArea,
        physics: &PhysicsWorld,
        frame: &mut SimFrame,
    ) {
        self.stamp = self.stamp.wrapping_add(1);
        for handle in self.retired.drain(..) {
            self.handles.release(handle);
        }
        let rows = self.seen.len();
        frame.previous.resize(rows, PoseRow::default());
        frame.current.resize(rows, PoseRow::default());
        frame.items.resize(rows, ItemRow::default());
        let stored = area.inventory.slots.map(|id| {
            id.and_then(|id| area.items.get(id))
                .filter(|i| matches!(i.location, ItemLocation::Stored(_)))
                .and_then(|i| i.body)
        });
        for (id, previous, current) in &area.render_poses {
            let body = area.bodies[id];
            let stored = stored.contains(&Some(body));
            let active = physics.body_active(body);
            let mut flags = 0;
            if active || stored {
                flags |= PoseRow::VISIBLE;
            }
            if active {
                flags |= PoseRow::ACTIVE;
            }
            if stored {
                flags |= PoseRow::STORED;
            }
            let slot = self.write(PoseKey::Body(*id), || SpawnData::Bound, frame);
            frame.previous[slot] = PoseRow::new(*previous, flags);
            frame.current[slot] = PoseRow::new(*current, flags);
        }
        let starter = reference.survival.starter_food;
        for (id, previous, current) in &area.item_render_poses {
            let item = area.items.get(*id);
            let mut flags = 0;
            if item.is_some_and(|item| item.inactive_pose.is_none()) {
                flags |= PoseRow::VISIBLE | PoseRow::ACTIVE;
            }
            if item.is_some_and(|item| matches!(item.location, ItemLocation::Stored(_))) {
                flags |= PoseRow::STORED;
            }
            let data = || {
                if let Some(spawned) = area.spawned.get(*id) {
                    SpawnData::Spawned(*spawned)
                } else if let Some(food) = item.and_then(|i| i.food)
                    && (*id == starter || !area.items.is_generated(*id))
                {
                    SpawnData::Food {
                        prototype: food.prototype_index as u32,
                    }
                } else {
                    SpawnData::Bound
                }
            };
            let slot = self.write(PoseKey::Item(*id), data, frame);
            frame.previous[slot] = PoseRow::new(*previous, flags);
            frame.current[slot] = PoseRow::new(*current, flags);
            frame.items[slot] = ItemRow {
                scale: area.gibs.scale(*id).unwrap_or(1.),
                bite: item.and_then(|i| i.food).map_or(0, |f| u32::from(f.bite)),
            };
        }
        for (slot, seen) in self.seen.iter().enumerate() {
            let handle = Handle(slot as u32);
            if *seen != self.stamp && self.handles.key(handle).is_some() {
                self.handles.retire(handle);
                frame.previous[slot] = PoseRow::default();
                frame.current[slot] = PoseRow::default();
                frame.items[slot] = ItemRow::default();
                frame.changes.push(Change::Retire(handle));
                self.retired.push(handle);
            }
        }
        let body = &area.body;
        if self.rig_joints != Some(body.joint_count()) {
            self.rig_joints = Some(body.joint_count());
            frame.changes.push(Change::PlayerRig(
                (0..body.joint_count())
                    .map(|i| body.joint_node(i))
                    .collect(),
            ));
        }
        frame.joints.clear();
        frame
            .joints
            .extend((0..body.joint_count()).map(|i| PoseRow::new(body.joint_local(i), 0)));
    }
    /// The row of `key`, with a `Spawn` change when the key is new.
    fn write(
        &mut self,
        key: PoseKey,
        data: impl FnOnce() -> SpawnData,
        frame: &mut SimFrame,
    ) -> usize {
        let (handle, new) = self.handles.assign(key);
        let slot = handle.0 as usize;
        if slot >= self.seen.len() {
            self.seen.resize(slot + 1, 0);
            frame.previous.resize(slot + 1, PoseRow::default());
            frame.current.resize(slot + 1, PoseRow::default());
            frame.items.resize(slot + 1, ItemRow::default());
        }
        if new {
            frame.changes.push(Change::Spawn {
                handle,
                key,
                data: data(),
            });
        }
        self.seen[slot] = self.stamp;
        slot
    }
}

#[cfg(test)]
mod tests;
