//! Validated installed-build data. Geometry uses converted body-local meters.

pub mod fixed_reservation;
pub mod package;
pub mod prototype_plan;
pub use package::{FileCatalog, FileReceipt, PlanReceipt};

pub mod player;
pub use player::*;

pub mod driving;
pub use driving::*;
pub mod settings;
pub use settings::*;
pub mod language;
pub use language::*;

pub mod audio;
pub use audio::*;

pub mod lighting_scene;
pub mod postprocessing;
pub mod reflections;
pub mod render_lighting;
pub mod world;
pub use world::*;
pub mod lamp_shadows;
pub mod rendering;
pub mod shadow_bounds;
pub mod shadow_cadence;
pub mod shadow_candidates;
pub mod shadow_frame;
pub mod shadow_planes;
pub mod shadow_scene;
pub mod shadow_splits;
pub mod shadows;
pub mod sky;
pub mod sorting;
pub mod temporal;
pub mod texture_batch;
pub mod view_math;
pub use rendering::*;

use serde::{Deserialize, Serialize};
use std::{
    collections::{HashMap, HashSet},
    fmt,
};

pub mod conditions;
pub use conditions::*;
pub mod flashlights;
pub use flashlights::*;
pub mod storage;
pub use storage::{
    PrototypeStorageFlag, PrototypeVolumeReference, StorageFlag, StoragePolicy, StorageRecord,
};
pub mod small_items;
pub use small_items::*;
pub mod computer;
pub use computer::*;
pub mod melee;
pub use melee::*;
pub mod guns;
pub use guns::*;
pub mod explosions;
pub use explosions::*;
pub mod crash;
pub use crash::*;
pub mod breakables;
pub use breakables::*;
pub mod creatures;
pub use creatures::*;
pub mod wheel_effects;
pub use wheel_effects::{EffectGains, SkidReference, WheelEffectsReference, WheelGains};
pub mod plates;
pub use plates::*;
pub mod clothing;
pub use clothing::*;
pub mod item_prototypes;
pub use item_prototypes::*;
pub mod acquisition;
pub use acquisition::*;
pub mod inventory;
pub mod survival;
pub use inventory::*;
pub mod particles;
pub use particles::*;
pub mod sky_traffic;
pub mod weather;
pub use sky_traffic::*;
pub use survival::*;
pub use weather::*;
pub mod instruments;
pub mod ui;
pub use instruments::*;
pub use ui::*;
pub mod lighting;
pub use lighting::*;
pub mod vehicles;
pub use vehicles::*;
pub mod towing;
pub use towing::*;
pub mod followers;
pub use followers::*;

pub const APP_ID: u32 = 1_017_180;
pub const BUILD_ID: u32 = 11_140_890;

macro_rules! id {
    ($name:ident, $inner:ty) => {
        #[derive(
            Clone,
            Copy,
            Debug,
            Default,
            PartialEq,
            Eq,
            PartialOrd,
            Ord,
            Hash,
            Serialize,
            Deserialize,
        )]
        #[serde(transparent)]
        pub struct $name(pub $inner);
    };
}
id!(MechanismId, u32);
id!(ItemId, u32);
id!(ConditionId, u32);
id!(WashId, u32);
id!(AssetId, u32);
id!(NodeId, u32);
id!(BodyId, u32);
id!(ColliderId, u32);
id!(AnchorId, u32);
id!(OpeningId, u32);
id!(PartSlotId, u32);
id!(LightId, u32);
id!(LightControlId, u32);

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SourceId {
    pub file: String,
    pub path_id: i64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SourceHash {
    pub file: String,
    pub sha256: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Transform {
    pub translation: [f32; 3],
    pub rotation: [f32; 4],
    pub scale: [f32; 3],
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AssetUsage {
    UniqueScene,
    RoadLampPrototype,
    SkyPrototype,
    FoodPrototype,
    WorldPrototype,
    WorldMaterial,
    ItemPrototype,
    SkyTraffic,
    PlayerPrototype,
    RagdollPrototype,
    StationPrototype,
    CreaturePrototype,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AssetRecord {
    pub mesh_count: u32,
    pub material_count: u32,
    pub usage: AssetUsage,
    pub id: AssetId,
    pub path: String,
    pub sha256: String,
    pub root: SourceId,
    pub dependencies: Vec<FileReceipt>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NodeRecord {
    pub id: NodeId,
    pub source: SourceId,
    pub name: String,
    pub hierarchy_path: String,
    pub asset: AssetId,
    pub gltf_node: u32,
    pub parent: Option<NodeId>,
    pub active: bool,
    pub transform: Transform,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BodyRecord {
    pub id: BodyId,
    pub source: SourceId,
    pub node: NodeId,
    pub dynamic: bool,
    pub mass_kg: f32,
    pub linear_damping: f32,
    pub angular_damping: f32,
    pub pose: Transform,
    pub parent_body: Option<BodyId>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum ColliderGeometry {
    Box {
        center: [f32; 3],
        half_extents: [f32; 3],
        rotation: [f32; 4],
    },
    Sphere {
        center: [f32; 3],
        radius: f32,
    },
    Capsule {
        a: [f32; 3],
        b: [f32; 3],
        radius: f32,
    },
    Convex {
        points: Vec<[f32; 3]>,
    },
    Trimesh {
        points: Vec<[f32; 3]>,
        triangles: Vec<[u32; 3]>,
    },
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ColliderRecord {
    pub id: ColliderId,
    pub source: SourceId,
    pub node: NodeId,
    pub body: BodyId,
    pub sensor: bool,
    pub enabled: bool,
    pub membership: u32,
    pub filter: u32,
    pub friction: f32,
    pub restitution: f32,
    pub geometry: ColliderGeometry,
    pub step_surface: Option<StepSurface>,
    pub tag: Option<ColliderTag>,
}

/// Source custom tag of a collider GameObject (TagManager order).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ColliderTag {
    Flesh,
    Plant,
    Wood,
    Fire,
    Metal,
    Sand,
    Cobweb,
    Ignore,
    ContainsMetal,
}
/// `ColliderTag` values in declaration order.
pub const COLLIDER_TAGS: [ColliderTag; 9] = [
    ColliderTag::Flesh,
    ColliderTag::Plant,
    ColliderTag::Wood,
    ColliderTag::Fire,
    ColliderTag::Metal,
    ColliderTag::Sand,
    ColliderTag::Cobweb,
    ColliderTag::Ignore,
    ColliderTag::ContainsMetal,
];

/// Source footstep surface of a collider: `otherStepSoundScript.tip`, or tag `Wood` or `Sand`.
/// The value is the collider user data code.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum StepSurface {
    Road = 1,
    Desert = 2,
    Winter = 3,
    Grass = 4,
    Wood = 5,
    Sand = 6,
    /// Tag `Wood`. Sand layers win over it.
    #[serde(rename = "wood_tag")]
    WoodTag = 7,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ActionMasks {
    pub use_mask: u32,
    pub pickup_mask: u32,
    pub attach_mask: u32,
    pub push_mask: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ActionBinding {
    pub collider: ColliderId,
    pub use_owner: Option<ItemId>,
    pub pickup_owner: Option<ItemId>,
    pub removal_owner: Option<ItemId>,
    pub push_owner: Option<ItemId>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AnchorRecord {
    pub id: AnchorId,
    pub source: SourceId,
    pub node: NodeId,
    pub body: BodyId,
    pub transform: Transform,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MechanismReference {
    pub id: MechanismId,
    pub source: SourceId,
    pub item: ItemId,
    pub parent_item: Option<ItemId>,
    pub driven_nodes: Vec<NodeId>,
    pub enabled: bool,
    pub child: Option<ItemId>,
    pub invert_child: bool,
    pub invert_value: bool,
    pub disable_on_craft: bool,
    pub disable_on_uncraft: bool,
    pub drag: MechanismDrag,
    pub rotation: Option<RotationReference>,
    pub reaction: EndpointReactionReference,
    pub slide: Option<SlideReference>,
    pub turn: Option<TurnReference>,
    /// Source `usablescript.handTarget`: the using hand parent. When null, the slide door or the hit serves.
    pub hand_target: Option<NodeId>,
    /// Source `usablescript.rightHand`: the right hand uses it.
    pub right_hand: bool,
    /// Source `usablescript.leftHand`: the left hand uses it.
    pub left_hand: bool,
    /// Source `usablescript.notNearestHand`: the hand farther from the target uses it.
    pub not_nearest_hand: bool,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TurnReference {
    pub action: TurnAction,
    pub max_state: u8,
    pub initial_state: u8,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum EndpointReactionReference {
    None,
    Rotation {
        node: NodeId,
        /// Unity right, up, and forward flags. Binding reflects forward Z once.
        axes: [bool; 3],
        inverted: bool,
        force: f32,
        /// Enter minimum, enter maximum, leave minimum, leave maximum.
        events: [bool; 4],
    },
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum TurnAction {
    ToggleHandbrake,
    CycleHeadlights,
    InteriorLight,
    ToggleHomeLight { source: SourceId },
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MechanismDrag {
    pub mode: DragMode,
    pub node: NodeId,
    pub node2: NodeId,
    pub axis: [f32; 3],
    pub axes2: [[f32; 3]; 2],
}
#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DragMode {
    Direct,
    Relative,
    RelativeTwo,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RotationReference {
    pub targets: Vec<RotationTarget>,
    pub sum_axes: bool,
    pub limits_degrees: [[f32; 2]; 2],
    pub initial_degrees: [f32; 2],
    pub inverted: [bool; 2],
    pub sensitivity: f32,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RotationTarget {
    pub node: NodeId,
    pub mapping: [[bool; 3]; 2],
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SlideReference {
    pub initial_value: f32,
    pub speed: f32,
    pub sensitivity: f32,
    pub axes: [bool; 6],
    pub multiplier: [f32; 3],
    pub scroll_without_use: bool,
    pub animation_source: Option<SourceId>,
    pub paths: Vec<SlidePath>,
    pub rotations: Vec<SlideRotation>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SlidePath {
    pub node: NodeId,
    pub points: Vec<SlidePoint>,
    pub rotate: bool,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SlidePoint {
    pub node: NodeId,
    pub value: f32,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SlideRotation {
    pub node: NodeId,
    pub reference: NodeId,
    pub axis: [f32; 3],
    pub angle_degrees: f32,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ItemKind {
    Food,
    Decoration,
    Door,
    Hood,
    Trunk,
    Container,
    Cap,
    Engine,
    Radiator,
    Tire,
    Rim,
    Wheel,
    Seat,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ItemRecord {
    pub id: ItemId,
    pub source: SourceId,
    pub node: NodeId,
    pub body: BodyId,
    pub name: String,
    pub kind: ItemKind,
    pub condition: f32,
    pub mass_kg: f32,
    pub attached_to: Option<ItemId>,
    pub pickup: Option<PickupReference>,
    pub collision_clip: Option<u32>,
    pub collision_gain: f32,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PickupReference {
    pub source: SourceId,
    pub cant_pickup: bool,
    pub cant_pickup_while_mounted: bool,
    pub cant_pickup_while_seated: bool,
    pub no_rotation: bool,
    pub move_with_rotation: bool,
    pub use_recenter_position: bool,
    pub recenter_position: [f32; 3],
    pub use_recenter_rotation: bool,
    pub recenter_rotation: [f32; 4],
    pub recenter_distance: f32,
    pub can_inventory: bool,
    pub can_hold_right_hand: bool,
    pub disable_rh_fix: bool,
    pub right_hand_position: [f32; 3],
    pub right_hand_rotation: [f32; 4],
    pub right_hand_grip: Option<SourceId>,
    pub left_hand_grip: Option<SourceId>,
    /// Source `pickupable.doubleHand`: both hands hold it, as when mass exceeds `onehandLimit`.
    pub double_hand: bool,
    pub disable_when_stored: Option<SourceId>,
    pub keep_when_stored: Option<SourceId>,
    pub inventory_icon: Option<SourceId>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OpeningRecord {
    pub id: OpeningId,
    pub source: SourceId,
    pub tank: ItemId,
    pub cap: ItemId,
    pub anchor: AnchorId,
    pub flow_liters_per_second: f32,
    pub tilt_degrees: f32,
    pub tilt_axis: [f32; 3],
    pub angle_disabled: bool,
    pub spill_disabled: bool,
    pub tube_connected: bool,
    pub cant_tube: bool,
    pub flow_radius: f32,
    pub particles: FluidParticleReference,
    pub can_toggle: bool,
    pub valve: f32,
    pub can_tune: bool,
    pub tune_rate: f32,
    /// `img`: the GCMMB sprite of a `canTune` cap. None shows `ICap`.
    pub tune_icon: Option<SourceId>,
    pub visuals: CapVisualReference,
    pub presentation: FluidPresentation,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FluidParticleReference {
    pub source: SourceId,
    pub local_pose: Transform,
    pub capacity: u32,
    pub cone_degrees: f32,
    pub cone_radius: f32,
    pub lifetime_seconds: f32,
    pub radius: f32,
    pub gravity_multiplier: f32,
    pub collision_mask: u32,
    pub bounce: f32,
    pub dampen: f32,
    pub max_speed: f32,
    pub min_rate: f32,
    pub max_rate: f32,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CapVisualReference {
    pub open_nodes: Vec<NodeId>,
    pub closed_nodes: Vec<NodeId>,
    pub cap_node: Option<NodeId>,
    pub closed_node: Option<NodeId>,
    pub open_node: Option<NodeId>,
    pub euler_rotation: bool,
    pub closed_euler_degrees: [f32; 3],
    pub open_euler_degrees: [f32; 3],
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TankReference {
    pub item: ItemId,
    pub source: SourceId,
    pub capacity_liters: f32,
    /// Entries retain source list order.
    pub fluids: Vec<crate::fluids::FluidEntry>,
    pub openings: Vec<OpeningId>,
    pub no_drink: bool,
    pub no_fill_this: bool,
    pub no_fill_from: bool,
    pub slow_oil: bool,
    pub role: TankRole,
    pub explosion: crate::world::TankExplosion,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TankRole {
    Portable,
    Supply,
    Fixed,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CurveKey {
    pub time: f32,
    pub value: f32,
    pub in_tangent: f32,
    pub out_tangent: f32,
    pub in_weight: f32,
    pub out_weight: f32,
    pub weighted_mode: u8,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TorqueCurve {
    pub keys: Vec<CurveKey>,
    pub pre_wrap_mode: i32,
    pub post_wrap_mode: i32,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EngineGlobals {
    pub source: SourceId,
    pub fuel_consumption: f32,
    pub oil_consumption: f32,
    pub rpm_change: f32,
    pub car_temperature_bounds: [f32; 2],
    pub coolant_evaporate_rate: f32,
    pub temperature_difference_modifier: f32,
    pub temperature_move_modifier: f32,
    pub max_car_velocity: f32,
    pub outside_temperature_modifier: f32,
    pub min_steam_temperature: f32,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FluidRulesReference {
    pub source: SourceId,
    pub pour_speed: f32,
    pub slow_oil_modifier: f32,
    pub fluid_tune_speed: f32,
    /// Hose end height difference where siphon flow starts.
    pub min_tube_vertical_height: f32,
    /// Hose end height difference where siphon flow is full.
    pub max_tube_vertical_height: f32,
    /// Source `mainscript.fluids[i].burnColor`, indexed by fluid type.
    pub burn_colors: [[f32; 4]; 6],
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
/// Source `pushgripable`. World prototypes use prototype node and collider indices.
pub struct PushGripReference<Node = NodeId, Collider = ColliderId> {
    pub source: SourceId,
    pub node: Node,
    /// Source `HT`. A null `HT` moves the nearest hand to the grip.
    pub hand_node: Option<Node>,
    pub colliders: Vec<Collider>,
    pub local_pose: Transform,
    pub in_use: bool,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PushReference {
    pub source: SourceId,
    pub owner: ItemId,
    pub body: BodyId,
    pub mask: u32,
    pub modifier: f32,
    pub only_projected: bool,
    pub both: bool,
    pub push_even_if_same_parent: bool,
    pub grip: Option<PushGripReference>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PlayerReference {
    pub posture: PlayerPostureReference,
    pub view: PlayerViewReference,
    pub prone_multiplier: f32,
    pub duck_jump_multiplier: f32,
    pub inverse_duck_multiplier: f32,
    pub max_fall_speed: f32,
    /// Unity `Physics.defaultMaxDepenetrationVelocity`.
    pub max_depenetration_velocity: f32,
    /// Source `FairM`: air force from the move keys.
    pub air_force: f32,
    /// Source `FGripM`: speed multiplier while gripping a ledge.
    pub grip_multiplier: f32,
    /// Source `FwallJumpM`: jump force multiplier of the wall jump.
    pub wall_jump_multiplier: f32,
    /// Source `FwallJumpM2`: jump force multiplier of the ledge jump.
    pub ledge_jump_multiplier: f32,
    /// Source `FGripLoose`: slowest fall speed while gripping.
    pub grip_loose: f32,
    /// Source `FmaxGripFallSpeed`: grip only below this speed.
    pub max_grip_fall_speed: f32,
    pub wall_sensors: PlayerWallSensorsReference,
    pub interior_sensor: PlayerInteriorSensorReference,
    pub source: SourceId,
    pub capsule_source: SourceId,
    pub camera_source: SourceId,
    pub walk_speed: f32,
    pub run_multiplier: f32,
    pub ladder_speed: f32,
    pub walk_bob_speed: f32,
    pub walk_bob_speed_rb: f32,
    pub max_walk_bob: f32,
    pub walk_bob_sound_threshold: f32,
    pub jump_force: f32,
    pub mass_kg: f32,
    pub duck_multiplier: f32,
    pub aim_reach: f32,
    pub action_masks: ActionMasks,
    pub get_out_layer_mask: u32,
    pub exit_capsule_half_height: f32,
    pub exit_capsule_radius: f32,
    pub exit_distance: f32,
    pub exit_step_vertical: f32,
    pub exit_steps_vertical: u32,
    pub exit_step_horizontal: f32,
    pub exit_steps_horizontal: u32,
    pub source_force_interval: f32,
    pub throw_minimum: f32,
    pub throw_gain: f32,
    pub throw_multiplier: f32,
    pub push_force: f32,
    pub airborne_push_force: f32,
    pub grip_force: f32,
    pub grip_speed: f32,
    pub grip_distance: f32,
    /// Source `fpscontroller.PedalingNm`: bike pedal torque at full gas.
    pub pedaling_nm: f32,
    /// Source `fpscontroller.PedalingRpm`: bike pedal rpm at full gas.
    pub pedaling_rpm: f32,
    pub carry_min_weight: f32,
    pub carry_max_weight: f32,
    pub carry_anchor_offset: [f32; 3],
    pub carry_offset_radius: f32,
    pub carry_drop_distance: f32,
    pub carry_force_multiplier: f32,
    pub carry_max_force: f32,
    pub carry_min_distance: f32,
    pub carry_max_distance: f32,
    pub car_camera: CarCameraReference,
    pub camera_culling_mask: u32,
    /// Source `fpscontroller.pointRange`: pointing ray length.
    pub point_range: f32,
    /// Source `fpscontroller.pointRange2`: pointing distance without a hit.
    pub point_range2: f32,
    /// Source `fpscontroller.pointingLayer` bits.
    pub pointing_layer: u32,
}

/// Source WheelCollider values.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WheelColliderReference {
    pub radius: f32,
    pub suspension_distance: f32,
    pub center: [f32; 3],
    pub spring: f32,
    pub damper: f32,
    pub target_position: f32,
    pub forward_friction: WheelFrictionCurve,
    pub sideways_friction: WheelFrictionCurve,
    pub wheel_mass_kg: f32,
    pub rotational_damping: f32,
    pub force_application_distance_m: f32,
}

impl WheelColliderReference {
    pub fn valid(&self) -> bool {
        finite(&[
            self.radius,
            self.suspension_distance,
            self.spring,
            self.damper,
            self.target_position,
            self.wheel_mass_kg,
            self.rotational_damping,
            self.force_application_distance_m,
        ]) && finite(&self.center)
            && self.forward_friction.valid()
            && self.sideways_friction.valid()
            && self.radius > 0.0
            && self.suspension_distance > 0.0
            && self.spring >= 0.0
            && self.damper >= 0.0
            && (0.0..=1.0).contains(&self.target_position)
            && self.wheel_mass_kg > 0.0
            && self.rotational_damping >= 0.0
            && self.force_application_distance_m >= 0.0
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PartScanReference {
    pub source: SourceId,
    pub mask: u32,
    pub radius: f32,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PartReference {
    pub scan_radius: f32,
    pub usable: Option<ItemId>,
    pub item: ItemId,
    pub source: SourceId,
    pub node: NodeId,
    pub main_types: Vec<String>,
    pub part_types: Vec<String>,
    pub cover_slot: Option<PartSlotId>,
    pub local_rotation_after_mount: [f32; 4],
    pub colliders: Vec<ColliderId>,
    pub linear_damping: f32,
    pub angular_damping: f32,
    pub enable_if_mounted: Vec<NodeId>,
    pub enable_if_dismounted: Vec<NodeId>,
    pub disable_if_mounted: Vec<NodeId>,
    pub disable_if_dismounted: Vec<NodeId>,
    /// Source `partscript.fallOffCategory`: a `mainscript.defFallOffChance` index.
    pub fall_off_category: u8,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PartSlotReference {
    pub id: PartSlotId,
    pub source: SourceId,
    pub node: NodeId,
    pub owner: Option<ItemId>,
    pub part: Option<ItemId>,
    pub mount_anchor: AnchorId,
    pub detach_anchor: Option<AnchorId>,
    pub fixed_detach: bool,
    pub detach_position: [f32; 3],
    pub detach_rotation: [f32; 4],
    pub main_types: Vec<String>,
    pub part_types: Vec<String>,
    pub cant_craft: bool,
    pub cant_uncraft: bool,
    pub disable_colliders_on_craft: bool,
    pub colliders: Vec<ColliderId>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WheelPartReference {
    pub cover_slot: Option<PartSlotId>,
    pub item: ItemId,
    pub source: SourceId,
    pub radius: f32,
    pub main_types: Vec<String>,
    pub part_types: Vec<String>,
    pub colliders: Vec<ColliderId>,
    pub tire_slot_anchor: Option<AnchorId>,
    pub tire_mount: Option<Transform>,
    pub tire_main_types: Vec<String>,
    pub tire_types: Vec<String>,
    pub local_rotation_after_mount: [f32; 4],
    /// Source `partscript.fallOffCategory`: a `mainscript.defFallOffChance` index.
    pub fall_off_category: u8,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Bounds {
    pub min: [f32; 3],
    pub max: [f32; 3],
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StartingAreaReference {
    pub source: SourceId,
    pub home_root: NodeId,
    pub home_pose: Transform,
    pub home_bounds: Bounds,
    pub player_spawn: Transform,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LibraryProvenance {
    pub schema_version: u32,
    pub manifest_sha256: String,
    pub catalog_sha256: String,
    pub source_manifest_sha256: String,
}

/// Source `PhysicsManager` settings.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PhysicsSettings {
    pub source: SourceId,
    /// `m_SleepThreshold`: the PhysX mass-normalized kinetic energy below which a body sleeps.
    pub sleep_threshold: f32,
}

/// The manifest with all world vehicle prototypes is about 295 MB.
pub const MAX_MANIFEST_BYTES: usize = 384 * 1024 * 1024;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReferenceManifest {
    pub content_base: String,
    pub file_catalog: std::sync::Arc<FileCatalog>,
    pub conditions: ConditionRecord,
    pub driving: DrivingReference,
    pub settings: SettingsReference,
    pub language: LanguageReference,
    pub clothing: ClothingReference,
    pub item_prototypes: Vec<ItemPrototype>,
    pub inventory: InventoryReference,
    pub acquisition: AcquisitionReference,
    pub audio: serde_json::Value,
    pub survival: SurvivalReference,
    pub sleep: SleepReference,
    pub particles: Vec<ParticleReference>,
    pub weather: WeatherReference,
    pub sky: SkyReference,
    pub creatures: CreaturesReference,
    pub ambient: AmbientReference,
    pub world: std::sync::Arc<WorldReference>,
    pub ui: UiReference,
    pub library: LibraryProvenance,
    pub lighting: LightingRecord,
    pub render_lighting: lighting_scene::LightingRecord,
    pub plates: PlateRecord,
    pub flashlights: FlashlightRecord,
    pub storage: StorageRecord,
    pub small_items: SmallItemsReference,
    pub computer: ComputerRecord,
    pub melee: MeleeReference,
    pub guns: GunsReference,
    pub explosions: ExplosionsReference,
    pub crash: CrashReference,
    pub breakables: Vec<BreakableReference>,
    pub gib_constants: GibConstants,
    pub physics: PhysicsSettings,
    pub skid: SkidReference,
    pub app_id: u32,
    pub build_id: u32,
    pub source_hashes: Vec<SourceHash>,
    pub assets: Vec<AssetRecord>,
    pub rendering: RenderingRecord,
    pub nodes: Vec<NodeRecord>,
    pub bodies: Vec<BodyRecord>,
    pub colliders: Vec<ColliderRecord>,
    pub action_bindings: Vec<ActionBinding>,
    pub pushables: Vec<PushReference>,
    pub anchors: Vec<AnchorRecord>,
    pub mechanisms: Vec<MechanismReference>,
    pub wheel_parts: Vec<WheelPartReference>,
    pub part_scan: PartScanReference,
    pub parts: Vec<PartReference>,
    pub part_slots: Vec<PartSlotReference>,
    pub items: Vec<ItemRecord>,
    pub openings: Vec<OpeningRecord>,
    pub tanks: Vec<TankReference>,
    pub fluid_rules: FluidRulesReference,
    pub player: PlayerReference,
    pub area: StartingAreaReference,
}

#[derive(Debug)]
pub struct ReferenceError(pub String);
impl fmt::Display for ReferenceError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}
impl std::error::Error for ReferenceError {}

fn require(ok: bool, field: &str) -> Result<(), ReferenceError> {
    if ok {
        Ok(())
    } else {
        Err(ReferenceError(format!("Invalid reference field: {field}")))
    }
}
fn finite(v: &[f32]) -> bool {
    v.iter().all(|x| x.is_finite())
}
fn transform(t: &Transform) -> bool {
    finite(&t.translation)
        && finite(&t.rotation)
        && finite(&t.scale)
        && t.scale.iter().all(|x| *x != 0.0)
        && (t.rotation.iter().map(|v| v * v).sum::<f32>() - 1.0).abs() < 0.002
}
fn ids<T: Eq + std::hash::Hash>(
    values: impl Iterator<Item = T>,
    field: &str,
) -> Result<HashSet<T>, ReferenceError> {
    let mut result = HashSet::new();
    for value in values {
        require(result.insert(value), field)?;
    }
    Ok(result)
}
fn hash(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(|b| b.is_ascii_hexdigit())
}

fn valid_tune_icon(can_tune: bool, icon: &Option<SourceId>, ui: &UiReference) -> bool {
    icon.as_ref()
        .is_none_or(|icon| can_tune && ui.sprite(icon).is_some())
}
fn valid_tuning(valve: f32, can_tune: bool, tune_rate: f32) -> bool {
    valve.is_finite()
        && (0.0..=1.0).contains(&valve)
        && tune_rate.is_finite()
        && tune_rate >= 0.0
        && (!can_tune || tune_rate > 0.0)
}
fn validate_cap_visuals(
    visuals: &CapVisualReference,
    node: impl Fn(NodeId) -> bool,
) -> Result<(), ReferenceError> {
    require(
        visuals
            .open_nodes
            .iter()
            .chain(&visuals.closed_nodes)
            .all(|id| node(*id))
            && [visuals.cap_node, visuals.closed_node, visuals.open_node]
                .iter()
                .all(|id| id.is_none_or(&node))
            && finite(&visuals.closed_euler_degrees)
            && finite(&visuals.open_euler_degrees),
        "cap visual bindings",
    )?;
    require(
        visuals.closed_node.is_some() == visuals.open_node.is_some()
            && (visuals.cap_node.is_some()
                || (visuals.closed_node.is_none() && !visuals.euler_rotation)),
        "cap visual control",
    )
}
fn valid_flow(
    flow: f32,
    tilt_degrees: f32,
    tilt_axis: &[f32; 3],
    radius: f32,
    p: &FluidParticleReference,
) -> bool {
    finite(&[flow, tilt_degrees])
        && flow > 0.0
        && finite(tilt_axis)
        && radius.is_finite()
        && radius > 0.0
        && p.capacity > 0
        && finite(&[
            p.cone_degrees,
            p.cone_radius,
            p.lifetime_seconds,
            p.radius,
            p.gravity_multiplier,
            p.max_speed,
            p.min_rate,
            p.max_rate,
            p.bounce,
            p.dampen,
        ])
        && transform(&p.local_pose)
        && p.cone_degrees >= 0.
        && p.cone_radius >= 0.
        && p.lifetime_seconds > 0.
        && p.radius > 0.
        && p.gravity_multiplier >= 0.
        && p.max_speed > 0.
        && p.max_rate >= p.min_rate
        && (0. ..=1.).contains(&p.bounce)
        && (0. ..=1.).contains(&p.dampen)
        && p.collision_mask != 0
}

impl ReferenceManifest {
    /// Exported audio clip id of a source clip.
    pub fn clip_id(&self, source: &SourceId) -> Option<u32> {
        self.audio["clips"].as_array()?.iter().find_map(|c| {
            (c["source"]["file"].as_str() == Some(source.file.as_str())
                && c["source"]["path_id"].as_i64() == Some(source.path_id))
            .then(|| c["id"].as_u64())
            .flatten()
            .map(|id| id as u32)
        })
    }
    pub fn expected_instances(&self) -> usize {
        self.assets
            .iter()
            .filter(|a| a.usage == AssetUsage::UniqueScene)
            .count()
    }
    pub fn from_json(bytes: &[u8]) -> Result<Self, ReferenceError> {
        require(bytes.len() <= MAX_MANIFEST_BYTES, "manifest size")?;
        let mut manifest: Self =
            serde_json::from_slice(bytes).map_err(|e| ReferenceError(e.to_string()))?;
        std::sync::Arc::make_mut(&mut manifest.file_catalog).admit()?;
        manifest.validate()?;
        Ok(manifest)
    }

    pub fn from_runtime_json(bytes: &[u8]) -> Result<Self, ReferenceError> {
        require(bytes.len() <= MAX_MANIFEST_BYTES, "manifest size")?;
        let mut manifest: Self =
            serde_json::from_slice(bytes).map_err(|e| ReferenceError(e.to_string()))?;
        std::sync::Arc::make_mut(&mut manifest.file_catalog).admit()?;
        manifest.validate_gameplay()?;
        Ok(manifest)
    }

    pub fn validate(&self) -> Result<(), ReferenceError> {
        AudioReference::from_value(&self.audio, self)?;
        self.validate_gameplay()
    }

    fn validate_gameplay(&self) -> Result<(), ReferenceError> {
        for texture in self
            .lighting
            .textures
            .iter()
            .chain(self.world.material_textures.iter())
            .chain(self.world.terrain.textures.iter())
        {
            texture.validate_batch_receipts(&self.file_catalog)?;
        }
        let package = self
            .content_base
            .strip_prefix("assets/runtime/")
            .and_then(|v| v.strip_suffix('/'));
        require(
            package.is_some_and(|v| {
                v.len() == 64
                    && v.bytes()
                        .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
            }),
            "content base",
        )?;
        for asset in &self.assets {
            require(
                self.file_catalog.file(&asset.path)?.sha256 == asset.sha256,
                "asset receipt hash",
            )?;
            self.file_catalog.plan(&asset.path)?;
            for receipt in &asset.dependencies {
                require(
                    self.file_catalog.file(&receipt.file)? == receipt,
                    "asset dependency receipt",
                )?;
            }
        }

        self.driving.validate()?;
        self.settings.validate()?;
        self.language.validate()?;
        require(
            self.openings.iter().all(|o| {
                !o.presentation.has_name
                    || (o.presentation.word as usize) < self.language.names.len()
            }),
            "cap name word",
        )?;
        require(
            self.assets
                .iter()
                .filter(|a| a.usage == AssetUsage::PlayerPrototype)
                .count()
                == 1,
            "exactly one player asset",
        )?;
        require(
            self.library.schema_version == 1
                && hash(&self.library.manifest_sha256)
                && hash(&self.library.catalog_sha256)
                && hash(&self.library.source_manifest_sha256),
            "library provenance",
        )?;
        require(
            self.app_id == APP_ID && self.build_id == BUILD_ID,
            "app/build",
        )?;
        self.world.validate(self)?;
        self.lighting.validate(self)?;
        self.render_lighting.validate(&self.nodes)?;
        self.survival.validate(self)?;
        for food in &self.survival.foods {
            food.pickup.validate_links(self, food.asset)?;
        }
        self.clothing.validate(self)?;
        self.inventory.validate(self)?;
        self.acquisition.validate(self)?;
        item_prototypes::validate_prototypes(self)?;
        AudioGameplayReference::from_value(&self.audio)?;
        self.sleep.validate(self)?;
        particles::validate_particles(self)?;
        self.weather.validate(self)?;
        self.sky.validate(self)?;
        self.creatures.validate(self)?;
        self.ambient.validate()?;
        self.plates.validate(self)?;
        require(
            self.bodies.iter().all(|b| {
                self.nodes.iter().any(|n| {
                    n.id == b.node
                        && self
                            .assets
                            .iter()
                            .any(|a| a.id == n.asset && a.usage == AssetUsage::UniqueScene)
                })
            }),
            "prototype body",
        )?;
        self.rendering
            .validate(&self.assets, self.lighting.cycle.sky_geometry.asset)?;
        for texture in self
            .rendering
            .materials
            .iter()
            .flat_map(|m| &m.properties.textures)
            .chain(&self.plates.material.textures)
        {
            fixed_reservation::primary_staging(texture, &self.file_catalog)?;
        }
        let asset_ids = ids(self.assets.iter().map(|r| r.id), "duplicate asset ID")?;
        let node_ids = ids(self.nodes.iter().map(|r| r.id), "duplicate node ID")?;
        self.ui.validate()?;
        let body_ids = ids(self.bodies.iter().map(|r| r.id), "duplicate body ID")?;
        let collider_ids = ids(self.colliders.iter().map(|r| r.id), "duplicate collider ID")?;
        let anchor_ids = ids(self.anchors.iter().map(|r| r.id), "duplicate anchor ID")?;
        let item_ids = ids(self.items.iter().map(|r| r.id), "duplicate item ID")?;
        let opening_ids = ids(self.openings.iter().map(|r| r.id), "duplicate opening ID")?;
        let part_ids = ids(self.parts.iter().map(|p| p.item), "duplicate part item")?;
        require(
            self.part_scan.mask != 0
                && self.part_scan.radius.is_finite()
                && self.part_scan.radius > 0.,
            "part scan parameters",
        )?;
        let part_slot_ids = ids(self.part_slots.iter().map(|s| s.id), "duplicate part slot")?;
        ids(
            self.part_slots.iter().filter_map(|s| s.part),
            "duplicate slot occupant",
        )?;
        let mechanism_items: HashSet<_> = self.mechanisms.iter().map(|m| m.item).collect();
        let mut owned_shapes = HashSet::new();
        for p in &self.parts {
            require(
                item_ids.contains(&p.item) && node_ids.contains(&p.node),
                "part item/node",
            )?;
            require(
                p.usable.is_none_or(|id| mechanism_items.contains(&id)),
                "part usable mechanism",
            )?;
            require(
                !p.part_types.is_empty()
                    && p.cover_slot.is_none_or(|s| part_slot_ids.contains(&s))
                    && usize::from(p.fall_off_category) < FALL_OFF_CATEGORIES,
                "part types/cover/fall-off",
            )?;
            require(
                p.linear_damping.is_finite()
                    && p.linear_damping >= 0.
                    && p.angular_damping.is_finite()
                    && p.angular_damping >= 0.,
                "part damping",
            )?;
            require(
                p.scan_radius.is_finite()
                    && p.scan_radius >= 0.
                    && p.local_rotation_after_mount.iter().all(|v| v.is_finite()),
                "part mount rotation",
            )?;
            require(
                p.colliders
                    .iter()
                    .all(|id| collider_ids.contains(id) && owned_shapes.insert(*id)),
                "part collider ownership",
            )?;
            require(
                p.enable_if_mounted
                    .iter()
                    .chain(&p.disable_if_mounted)
                    .chain(&p.enable_if_dismounted)
                    .chain(&p.disable_if_dismounted)
                    .all(|id| node_ids.contains(id)),
                "part visibility node",
            )?;
        }
        for s in &self.part_slots {
            require(
                node_ids.contains(&s.node)
                    && anchor_ids.contains(&s.mount_anchor)
                    && s.detach_anchor.is_none_or(|id| anchor_ids.contains(&id)),
                "part slot anchors",
            )?;
            require(
                s.part.is_none_or(|id| part_ids.contains(&id))
                    && s.owner.is_none_or(|id| item_ids.contains(&id)),
                "part slot owner/occupant",
            )?;
            require(
                s.colliders.iter().all(|id| {
                    self.colliders
                        .iter()
                        .any(|c| c.id == *id && c.node == s.node)
                }),
                "part slot collider",
            )?;
            require(
                s.detach_position
                    .iter()
                    .chain(&s.detach_rotation)
                    .all(|v| v.is_finite()),
                "part detach pose",
            )?;
            let mut owner = s.owner;
            for _ in 0..=self.part_slots.len() {
                let Some(item) = owner else {
                    break;
                };
                require(s.part != Some(item), "cyclic part slots")?;
                owner = self
                    .part_slots
                    .iter()
                    .find(|s| s.part == Some(item))
                    .and_then(|s| s.owner);
            }
            require(owner.is_none(), "cyclic part slots")?;
        }
        for p in &self.wheel_parts {
            require(
                p.colliders.iter().all(|id| owned_shapes.insert(*id)),
                "part collider ownership",
            )?;
            require(
                p.cover_slot.is_none_or(|id| part_slot_ids.contains(&id)),
                "wheel cover slot",
            )?;
        }
        require(
            self.assets
                .iter()
                .enumerate()
                .all(|(i, r)| r.id.0 == i as u32 + 1)
                && self
                    .nodes
                    .iter()
                    .enumerate()
                    .all(|(i, r)| r.id.0 == i as u32 + 1)
                && self
                    .bodies
                    .iter()
                    .enumerate()
                    .all(|(i, r)| r.id.0 == i as u32 + 1)
                && self
                    .colliders
                    .iter()
                    .enumerate()
                    .all(|(i, r)| r.id.0 == i as u32 + 1)
                && self
                    .anchors
                    .iter()
                    .enumerate()
                    .all(|(i, r)| r.id.0 == i as u32 + 1)
                && self
                    .items
                    .iter()
                    .enumerate()
                    .all(|(i, r)| r.id.0 == i as u32 + 1),
            "dense runtime IDs",
        )?;
        ids(
            self.nodes.iter().map(|r| &r.source),
            "duplicate node source",
        )?;
        require(
            !self.assets.is_empty() && !self.source_hashes.is_empty(),
            "assets/source hashes",
        )?;
        for r in &self.source_hashes {
            require(!r.file.is_empty() && hash(&r.sha256), "source hash")?;
        }
        for r in &self.assets {
            require(
                r.path.starts_with("reference/") && !r.path.contains("..") && hash(&r.sha256),
                "asset path/hash",
            )?;
            let bindings = ids(
                self.nodes
                    .iter()
                    .filter(|n| n.asset == r.id)
                    .map(|n| n.gltf_node),
                "duplicate glTF node binding",
            )?;
            require(
                (0..bindings.len() as u32).all(|id| bindings.contains(&id)),
                "glTF node bindings",
            )?;
            ids(
                r.dependencies.iter().map(|dependency| &dependency.file),
                "duplicate asset dependency",
            )?;
            for dependency in &r.dependencies {
                require(
                    dependency.file.starts_with("reference/")
                        && !dependency.file.contains("..")
                        && hash(&dependency.sha256),
                    "asset dependency hash/path",
                )?;
            }
        }
        require(
            self.nodes.len() <= 100_000 && self.colliders.len() <= 100_000,
            "record count",
        )?;
        let node_map: HashMap<_, _> = self.nodes.iter().map(|r| (r.id, r)).collect();
        for r in &self.nodes {
            require(
                asset_ids.contains(&r.asset)
                    && r.parent
                        .is_none_or(|id| node_ids.contains(&id) && id != r.id),
                "node binding",
            )?;
            require(transform(&r.transform), "node transform")?;
            let mut parent = r.parent;
            for _ in 0..64 {
                if parent.is_none() {
                    break;
                }
                require(parent != Some(r.id), "node cycle")?;
                parent = parent
                    .and_then(|id| node_map.get(&id))
                    .and_then(|n| n.parent);
            }
            require(parent.is_none(), "node cycle")?;
        }
        for r in &self.bodies {
            require(
                r.parent_body
                    .is_none_or(|id| body_ids.contains(&id) && id != r.id),
                "body parent binding",
            )?;
            require(
                node_ids.contains(&r.node) && transform(&r.pose),
                "body binding/pose",
            )?;
            require(
                r.mass_kg.is_finite() && r.mass_kg >= 0.0 && (!r.dynamic || r.mass_kg > 0.0),
                "body mass",
            )?;
            require(
                finite(&[r.linear_damping, r.angular_damping])
                    && r.linear_damping >= 0.0
                    && r.angular_damping >= 0.0,
                "body damping",
            )?;
            require(r.pose.scale == [1.0; 3], "body scale")?;
            let mut parent = r.parent_body;
            for _ in 0..64 {
                if parent.is_none() {
                    break;
                }
                require(parent != Some(r.id), "body cycle")?;
                parent = self
                    .bodies
                    .iter()
                    .find(|body| Some(body.id) == parent)
                    .and_then(|body| body.parent_body);
            }
            require(parent.is_none(), "body cycle/depth")?;
        }
        for r in &self.colliders {
            require(
                body_ids.contains(&r.body) && node_ids.contains(&r.node),
                "collider binding",
            )?;
            require(
                r.friction.is_finite()
                    && r.friction >= 0.0
                    && r.restitution.is_finite()
                    && (0.0..=1.0).contains(&r.restitution),
                "collider material",
            )?;
            let valid = match &r.geometry {
                ColliderGeometry::Box {
                    center,
                    half_extents,
                    rotation,
                } => {
                    finite(center)
                        && finite(rotation)
                        && finite(half_extents)
                        && half_extents.iter().all(|x| *x > 0.0)
                }
                ColliderGeometry::Sphere { center, radius } => {
                    finite(center) && radius.is_finite() && *radius > 0.0
                }
                ColliderGeometry::Capsule { a, b, radius } => {
                    finite(a) && finite(b) && radius.is_finite() && *radius > 0.0
                }
                ColliderGeometry::Convex { points } => {
                    points.len() >= 4
                        && points.len() <= 1_000_000
                        && points.iter().all(|p| finite(p))
                }
                ColliderGeometry::Trimesh { points, triangles } => {
                    !points.is_empty()
                        && points.len() <= 1_000_000
                        && !triangles.is_empty()
                        && triangles.len() <= 2_000_000
                        && points.iter().all(|p| finite(p))
                        && triangles
                            .iter()
                            .flatten()
                            .all(|i| (*i as usize) < points.len())
                }
            };
            require(valid, "collider geometry")?;
        }
        let action_ids = ids(
            self.action_bindings.iter().map(|r| r.collider),
            "duplicate action collider",
        )?;
        require(
            action_ids == collider_ids,
            "complete action collider bindings",
        )?;
        for binding in &self.action_bindings {
            require(
                [
                    binding.use_owner,
                    binding.pickup_owner,
                    binding.removal_owner,
                    binding.push_owner,
                ]
                .iter()
                .all(|owner| owner.is_none_or(|id| item_ids.contains(&id))),
                "action owner binding",
            )?;
        }
        for r in &self.anchors {
            require(
                node_ids.contains(&r.node) && body_ids.contains(&r.body) && transform(&r.transform),
                "anchor binding",
            )?;
        }
        for r in &self.items {
            require(
                node_ids.contains(&r.node)
                    && body_ids.contains(&r.body)
                    && r.attached_to
                        .is_none_or(|id| item_ids.contains(&id) && id != r.id),
                "item binding",
            )?;
            require(
                r.condition.is_finite()
                    && (0.0..=1.0).contains(&r.condition)
                    && r.mass_kg.is_finite()
                    && r.mass_kg >= 0.0,
                "item condition/mass",
            )?;
        }
        for item in &self.items {
            if let Some(pickup) = &item.pickup {
                let asset = self
                    .nodes
                    .iter()
                    .find(|n| n.id == item.node)
                    .ok_or_else(|| ReferenceError("pickup node".into()))?
                    .asset;
                pickup.validate_links(self, asset)?;
                require(
                    finite(&pickup.recenter_position)
                        && finite(&pickup.recenter_rotation)
                        && (pickup.recenter_rotation.iter().map(|v| v * v).sum::<f32>() - 1.0)
                            .abs()
                            < 0.001
                        && pickup.recenter_distance.is_finite(),
                    "pickup recenter pose",
                )?;
            }
        }
        ids(
            self.mechanisms.iter().map(|m| m.id),
            "duplicate mechanism ID",
        )?;
        ids(
            self.mechanisms.iter().map(|m| m.item),
            "duplicate mechanism item",
        )?;
        let mechanism_items: HashSet<_> = self.mechanisms.iter().map(|m| m.item).collect();
        for r in &self.mechanisms {
            if let EndpointReactionReference::Rotation { node, force, .. } = &r.reaction {
                require(
                    r.rotation.is_some()
                        && node_ids.contains(node)
                        && force.is_finite()
                        && *force >= 0.,
                    "mechanism endpoint reaction",
                )?;
            }
            require(
                item_ids.contains(&r.item)
                    && r.parent_item.is_none_or(|id| item_ids.contains(&id))
                    && r.child
                        .is_none_or(|id| mechanism_items.contains(&id) && id != r.item)
                    && (!r.driven_nodes.is_empty() || r.turn.is_some())
                    && r.driven_nodes.iter().all(|id| {
                        node_ids.contains(id) && self.bodies.iter().any(|b| b.node == *id)
                    })
                    && node_ids.contains(&r.drag.node)
                    && node_ids.contains(&r.drag.node2)
                    && r.hand_target.is_none_or(|n| node_ids.contains(&n))
                    && finite(&r.drag.axis)
                    && r.drag.axes2.iter().all(|a| finite(a)),
                "mechanism binding",
            )?;
            require(
                r.rotation.is_some() || r.slide.is_some() || r.turn.is_some(),
                "empty mechanism",
            )?;
            if let Some(turn) = &r.turn {
                require(turn.initial_state <= turn.max_state, "switch state")?;
                match &turn.action {
                    TurnAction::ToggleHomeLight { source } => require(
                        self.lighting
                            .controllers
                            .iter()
                            .any(|g| g.source == *source && g.kind == LightControllerKind::Switch),
                        "switch lamp link",
                    )?,
                    TurnAction::InteriorLight => {
                        require(turn.max_state == 2, "cabin switch modes")?
                    }
                    _ => require(turn.max_state == 0, "event switch state")?,
                }
            }
            if let Some(rotation) = &r.rotation {
                require(
                    !rotation.targets.is_empty()
                        && rotation
                            .targets
                            .iter()
                            .all(|t| r.driven_nodes.contains(&t.node))
                        && rotation
                            .limits_degrees
                            .iter()
                            .all(|v| finite(v) && v[0] <= v[1])
                        && finite(&rotation.initial_degrees)
                        && rotation.sensitivity.is_finite()
                        && rotation.sensitivity > 0.0,
                    "mechanism rotation",
                )?;
            }
            if let Some(slide) = &r.slide {
                require(
                    slide.initial_value.is_finite()
                        && (0.0..=1.0).contains(&slide.initial_value)
                        && slide.speed.is_finite()
                        && slide.sensitivity.is_finite()
                        && slide.speed > 0.0
                        && slide.sensitivity > 0.0
                        && finite(&slide.multiplier)
                        && (!slide.paths.is_empty() || !slide.rotations.is_empty()),
                    "mechanism slide",
                )?;
                for path in &slide.paths {
                    require(
                        r.driven_nodes.contains(&path.node)
                            && path.points.len() >= 2
                            && path
                                .points
                                .iter()
                                .all(|p| node_ids.contains(&p.node) && p.value.is_finite())
                            && path.points.windows(2).all(|p| p[0].value < p[1].value)
                            && path.points[0].value == 0.0
                            && path.points.last().unwrap().value == 1.0,
                        "mechanism path",
                    )?;
                }
                for track in &slide.rotations {
                    require(
                        r.driven_nodes.contains(&track.node)
                            && node_ids.contains(&track.reference)
                            && finite(&track.axis)
                            && track.axis.iter().any(|v| *v != 0.0)
                            && track.angle_degrees.is_finite(),
                        "mechanism animation",
                    )?;
                }
            }
            let mut child = r.child;
            for depth in 0..=self.mechanisms.len() {
                let Some(id) = child else {
                    break;
                };
                require(depth < self.mechanisms.len(), "mechanism child cycle")?;
                child = self
                    .mechanisms
                    .iter()
                    .find(|m| m.item == id)
                    .and_then(|m| m.child);
            }
        }
        for r in &self.openings {
            require(
                valid_tuning(r.valve, r.can_tune, r.tune_rate)
                    && valid_tune_icon(r.can_tune, &r.tune_icon, &self.ui),
                "opening valve/tuning",
            )?;
            validate_cap_visuals(&r.visuals, |id| node_ids.contains(&id))?;
            require(
                item_ids.contains(&r.tank)
                    && item_ids.contains(&r.cap)
                    && anchor_ids.contains(&r.anchor),
                "opening binding",
            )?;
            require(
                valid_flow(
                    r.flow_liters_per_second,
                    r.tilt_degrees,
                    &r.tilt_axis,
                    r.flow_radius,
                    &r.particles,
                ),
                "opening flow",
            )?;
        }
        ids(self.tanks.iter().map(|r| r.item), "duplicate tank")?;
        for r in &self.tanks {
            require(
                item_ids.contains(&r.item) && r.openings.iter().all(|id| opening_ids.contains(id)),
                "tank binding",
            )?;
            require(
                r.capacity_liters.is_finite()
                    && r.capacity_liters > 0.0
                    && crate::fluids::validate_fluid_entries(&r.fluids)
                    && r.fluids.iter().all(|v| v.amount >= 0.0)
                    && r.fluids.iter().map(|v| f64::from(v.amount)).sum::<f64>()
                        <= f64::from(r.capacity_liters),
                "tank quantities",
            )?;
        }
        let p = &self.player;
        p.validate_movement()?;
        require(p.camera_culling_mask != 0, "camera culling mask")?;
        require(
            finite(&p.carry_anchor_offset)
                && finite(&[
                    p.carry_min_weight,
                    p.carry_max_weight,
                    p.carry_offset_radius,
                    p.carry_drop_distance,
                    p.carry_force_multiplier,
                    p.carry_max_force,
                    p.carry_min_distance,
                    p.carry_max_distance,
                ])
                && p.carry_min_weight >= 0.0
                && p.carry_max_weight > p.carry_min_weight
                && p.carry_offset_radius > 0.0
                && p.carry_drop_distance > 0.0
                && p.carry_min_distance >= 0.0
                && p.carry_max_distance >= p.carry_min_distance,
            "carry controls",
        )?;
        require(
            finite(&[
                p.exit_distance,
                p.exit_step_vertical,
                p.exit_step_horizontal,
            ]) && p.exit_distance > 0.0
                && p.exit_steps_vertical <= 100
                && p.exit_steps_horizontal <= 100,
            "effective player geometry/exit scan",
        )?;
        require(
            finite(&[
                self.fluid_rules.pour_speed,
                self.fluid_rules.slow_oil_modifier,
                self.fluid_rules.fluid_tune_speed,
                self.fluid_rules.min_tube_vertical_height,
                self.fluid_rules.max_tube_vertical_height,
            ]) && self.fluid_rules.pour_speed > 0.0
                && self.fluid_rules.min_tube_vertical_height
                    < self.fluid_rules.max_tube_vertical_height
                && self.fluid_rules.burn_colors.iter().all(|c| finite(c)),
            "fluid rules",
        )?;
        require(
            finite(&[p.mass_kg, p.duck_multiplier]) && p.mass_kg == 72.0,
            "player movement",
        )?;
        require(
            finite(&[
                p.walk_speed,
                p.run_multiplier,
                p.jump_force,
                p.aim_reach,
                p.exit_capsule_half_height,
                p.exit_capsule_radius,
            ]) && p.walk_speed > 0.0
                && p.aim_reach > 0.0
                && p.action_masks.use_mask != 0
                && p.action_masks.pickup_mask != 0
                && p.action_masks.attach_mask != 0
                && p.action_masks.push_mask != 0
                && p.get_out_layer_mask != 0
                && p.exit_capsule_half_height > 0.0
                && p.exit_capsule_radius > 0.0,
            "player parameters",
        )?;
        require(
            finite(&[p.point_range, p.point_range2])
                && p.point_range > 0.0
                && p.point_range2 >= 0.0
                && p.pointing_layer != 0,
            "player pointing",
        )?;
        let bob = [
            p.walk_bob_speed,
            p.walk_bob_speed_rb,
            p.max_walk_bob,
            p.walk_bob_sound_threshold,
        ];
        require(
            finite(&bob)
                && bob.iter().all(|v| *v >= 0.0)
                && p.ladder_speed.is_finite()
                && p.ladder_speed > 0.0,
            "ladder and head bob parameters",
        )?;
        require(
            finite(&[
                p.source_force_interval,
                p.throw_minimum,
                p.throw_gain,
                p.throw_multiplier,
                p.push_force,
                p.airborne_push_force,
                p.grip_force,
                p.grip_speed,
                p.grip_distance,
            ]) && p.source_force_interval > 0.0
                && p.throw_minimum > 0.0
                && p.throw_minimum < 1.0
                && p.throw_gain > 0.0
                && p.throw_multiplier > 0.0
                && p.push_force > 0.0
                && p.airborne_push_force > 0.0
                && p.grip_force > 0.0
                && p.grip_speed > 0.0
                && p.grip_distance > 0.0,
            "push and throw parameters",
        )?;
        require(
            finite(&[p.pedaling_nm, p.pedaling_rpm])
                && p.pedaling_nm >= 0.0
                && p.pedaling_rpm >= 0.0,
            "pedaling parameters",
        )?;
        let mut push_owners = HashSet::new();
        for push in &self.pushables {
            require(
                item_ids.contains(&push.owner)
                    && self
                        .items
                        .iter()
                        .find(|item| item.id == push.owner)
                        .is_some_and(|item| item.body == push.body)
                    && self
                        .bodies
                        .iter()
                        .find(|body| body.id == push.body)
                        .is_some_and(|body| body.dynamic)
                    && body_ids.contains(&push.body)
                    && push_owners.insert(push.owner)
                    && push.mask == p.action_masks.push_mask
                    && push.modifier.is_finite()
                    && push.modifier >= 0.0,
                "push record",
            )?;
            if let Some(grip) = &push.grip {
                require(
                    node_ids.contains(&grip.node)
                        && grip.hand_node.is_none_or(|node| node_ids.contains(&node))
                        && transform(&grip.local_pose)
                        && !grip.colliders.is_empty()
                        && grip.colliders.iter().all(|id| collider_ids.contains(id)),
                    "push grip",
                )?;
            }
        }
        require(
            self.action_bindings.iter().all(|binding| {
                binding
                    .push_owner
                    .is_none_or(|owner| push_owners.contains(&owner))
            }),
            "push action owner",
        )?;
        ids(
            self.wheel_parts.iter().map(|part| part.item),
            "duplicate wheel part",
        )?;
        for part in &self.wheel_parts {
            require(
                item_ids.contains(&part.item)
                    && !part.colliders.is_empty()
                    && part.colliders.iter().all(|id| collider_ids.contains(id))
                    && part
                        .tire_slot_anchor
                        .is_none_or(|id| anchor_ids.contains(&id)),
                "wheel part bindings",
            )?;
            require(
                part.radius.is_finite()
                    && part.radius > 0.0
                    && finite(&part.local_rotation_after_mount)
                    && part.tire_mount.as_ref().is_none_or(transform)
                    && part.main_types.len() <= 16
                    && part.part_types.len() <= 16
                    && !part.part_types.is_empty()
                    && usize::from(part.fall_off_category) < FALL_OFF_CATEGORIES,
                "wheel part geometry/fit",
            )?;
        }
        let a = &self.area;
        require(
            node_ids.contains(&a.home_root)
                && transform(&a.home_pose)
                && transform(&a.player_spawn),
            "area binding/poses",
        )?;
        let b = &a.home_bounds;
        require(
            finite(&b.min) && finite(&b.max) && (0..3).all(|i| b.min[i] <= b.max[i]),
            "area bounds",
        )?;
        let cable = &self.world.home_cable;
        require(
            !cable.ropes.is_empty()
                && node_ids.contains(&cable.root)
                && cable.ropes.iter().all(|id| node_ids.contains(id)),
            "home cable",
        )?;
        self.flashlights.validate(self)?;
        self.storage.validate(self)?;
        self.small_items.validate(self)?;
        self.computer.validate(self)?;
        self.melee.validate(self)?;
        self.guns.validate(self)?;
        self.explosions.validate(self)?;
        self.crash.validate()?;
        require(
            self.world
                .prototypes
                .iter()
                .filter_map(|p| p.vehicle.as_ref()?.blood.as_ref())
                .all(|b| (b.permission as usize) < self.conditions.clean_tables.len()),
            "car blood clean property",
        )?;
        ids(
            self.breakables.iter().map(|b| &b.source),
            "breakable sources",
        )?;
        for breakable in &self.breakables {
            breakable.validate(self)?;
        }
        self.gib_constants.validate()?;
        require(
            self.physics.sleep_threshold.is_finite() && self.physics.sleep_threshold > 0.,
            "physics sleep threshold",
        )?;
        wheel_effects::validate(self)?;
        self.conditions.validate(self)?;
        Ok(())
    }
}
