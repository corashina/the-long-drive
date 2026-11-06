//! Web Worker entry for terrain, road and grass mesh jobs.
//! Each worker runs its own instance with its own WASM memory and uses one job module.
use wasm_heap as _;

mod grass_job;
mod road_job;
mod terrain_job;
