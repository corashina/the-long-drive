//! Pure source terrain math. It has no engine, physics, or browser state.
mod job;
mod noise;
mod random;
mod surface;

pub use job::{TerrainJob, TerrainTask, evaluate};
pub use noise::{TerrainEvaluator, TerrainSettings, source_perlin};
pub use random::{RandomState, SourceRandom};
pub use surface::{
    TerrainContext, TerrainFoundation, TerrainMesh, TerrainRoadPoint, TerrainSurfaceJob,
    TerrainTile, biome, forced_color, regular_grid, surface, surface_origin,
};
