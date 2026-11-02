mod required;
pub use required::{REQUIRED_VIEW_CAPACITY, RequiredContent, RequiredView, RequiredViews};
mod generation;
pub use generation::*;
pub mod changes;
pub mod types;
pub use changes::{
    AttachmentSnapshot, ChangeRecords, MechanismSnapshot, ObjectChange, ObjectSnapshot,
};
pub use types::*;

mod draw_distance;
pub use draw_distance::DrawDistances;
mod cables;
pub use cables::aim;
mod lamps;
mod source_lights;
pub use source_lights::{WorldLight, required_world_lights};
mod placement;
mod prop_layers;
mod props;
pub use props::{PropCandidate, PropGenerator};
pub mod spawn_state;
pub mod unity_random;
pub use placement::{
    BUILDING_CHECKPOINT_INTERVAL, BuildingCheckpoint, BuildingGroupState, BuildingSequence,
    FoundationPoint, PlacementIndex, PlacementIndexSnapshot, SupplyDefinition, SupplyFluid,
    SupplyPoint,
};
pub use placement::{node_frame, place_point};
pub use spawn_state::{SourceSpawnState, SpawnBoundary, SpawnContext, SpawnPhase};
pub use unity_random::{UnityRandom, UnityRandomState};
mod road;
pub mod road_worker;
pub use road::{
    ROAD_CHECKPOINT_INTERVAL, ROAD_CONTROLS, ROAD_SEGMENTS, RoadBlock, RoadCheckpoint, RoadCurve,
    RoadIndex, RoadIndexSnapshot, RoadSegment, RoadSequence, RoadState,
};
pub use road_worker::{RoadJobId, RoadPoll, RoadWorker};
mod instances;
pub use instances::{StructureInstance, StructureRecipe, TriggerRoles, scaled_geometry};
pub mod metrics;
pub use metrics::WorldMetrics;
pub mod renderer_facts;
pub use renderer_facts::{RendererFact, RendererFacts};
pub mod stream;
mod stream_limits;
pub use stream::content_boxes::BoxRelease;
pub use stream::content_explosives::{Explosive, TankExplosion};
pub use stream::content_index::ContentIndex;
pub use stream::renderer_world::{
    RendererFrame, RendererStructure, RendererSupply, RendererSurface, RendererWorld, WorldDelta,
};
pub use stream::{
    PresentationAllocation, PresentationGrant, PresentationReport, PublishedSupply,
    PublishedSurface, Reclaim, SurfaceKey, WorldFrame, WorldRequest, WorldStream,
};
pub use stream_limits::StreamLimits;
mod start_car;
pub use start_car::{StartCar, StartCarChoice, menu_random, start_fields};
mod startup;
pub use startup::{StartupTerrain, source_world_pose};
mod terrain;
pub use ::terrain::{
    RandomState, SourceRandom, TerrainContext, TerrainEvaluator, TerrainFoundation, TerrainJob,
    TerrainRoadPoint, TerrainTile, biome, forced_color, source_perlin,
};
pub use terrain::{terrain_evaluator, terrain_output_bound, terrain_settings, terrain_tile};

/// Game mode of a new world. Biome: 0 Mixed, 1 Desert, 2 Winter, 3 Grass.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct GameMode {
    pub biome: u8,
    pub start_car: StartCarChoice,
    /// World seed. The browser menu sends the seed field value.
    pub seed: i32,
}

impl Default for GameMode {
    fn default() -> Self {
        Self {
            biome: 0,
            start_car: StartCarChoice::default(),
            seed: DEFAULT_SEED as i32,
        }
    }
}

impl GameMode {
    /// The mode of the browser menu values. -1 or an unknown car is Random.
    pub fn from_menu(
        world: &crate::reference::WorldReference,
        biome: u32,
        start_car: i32,
        seed: i32,
    ) -> Self {
        Self {
            biome: if biome > 3 { 0 } else { biome as u8 },
            start_car: StartCarChoice::from_index(world, start_car),
            seed,
        }
    }
    /// Biome index for `biome`: 0 desert, 1 winter, 2 grass. Mixed uses the terrain color.
    pub fn biome_override(self) -> Option<u8> {
        self.biome.checked_sub(1)
    }
}

/// Game options. They apply at once and are not part of a save, as in the original.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct GameOptions {
    /// `peacefulEnemies == 2`.
    pub peaceful: bool,
    /// `survival == 2`.
    pub survival: bool,
}

impl Default for GameOptions {
    /// `gamemodesettingsscript.defS`.
    fn default() -> Self {
        Self {
            peaceful: false,
            survival: true,
        }
    }
}

impl GameOptions {
    /// Browser bits: 1 peaceful, 2 survival.
    pub fn from_bits(bits: u32) -> Self {
        Self {
            peaceful: bits & 1 != 0,
            survival: bits & 2 != 0,
        }
    }
    /// Source `peacefulEnemies` value: 1 off, 2 on.
    pub fn peaceful_enemies(self) -> i32 {
        if self.peaceful { 2 } else { 1 }
    }
}

pub const MAX_ABSOLUTE_COORDINATE: f64 = (1_u64 << 45) as f64;
pub const REBASE_LIMIT: f32 = 2048.0;
pub const DEFAULT_SEED: u64 = 42;

#[derive(Clone, Copy, Debug, Default, serde::Serialize, serde::Deserialize)]
pub struct WorldOrigin {
    pub offset: [f64; 3],
}

impl WorldOrigin {
    pub fn absolute(&self, local: [f32; 3]) -> [f64; 3] {
        std::array::from_fn(|i| self.offset[i] + local[i] as f64)
    }

    pub fn local(&self, absolute: [f64; 3]) -> [f32; 3] {
        std::array::from_fn(|i| (absolute[i] - self.offset[i]) as f32)
    }

    pub fn shift(&mut self, shift: [f32; 3]) {
        for (offset, shift) in self.offset.iter_mut().zip(shift) {
            *offset += shift as f64;
        }
    }
}

mod spawn_contents;
mod spawn_initialization;
mod spawn_rules;
mod spawn_selection;

mod biome_materials;
mod building_extras;
mod spawn_appearance;
mod spawn_building;
mod spawn_conditions;
mod spawn_lamps;
pub use spawn_appearance::NodeAppearance;
mod spawn_slots;
mod spawn_starts;
mod structure_helpers;

mod contents;

mod content_containers;

mod content_parts;

mod content_slots;
pub mod content_vehicles;
mod content_wheels;

mod content_mechanisms;

pub use contents::GeneratedCapVisual;

pub fn hash(mut value: u64) -> u64 {
    value = value.wrapping_add(0x9e3779b97f4a7c15);
    value = (value ^ (value >> 30)).wrapping_mul(0xbf58476d1ce4e5b9);
    value = (value ^ (value >> 27)).wrapping_mul(0x94d049bb133111eb);
    value ^ (value >> 31)
}

pub mod stream_profile;
