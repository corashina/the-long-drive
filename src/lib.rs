//! Browser application: renderer and presentation over the simulation crate.
#![recursion_limit = "256"]
use wasm_heap as _;

pub use ::simulation::{
    animation, audio, conditions, damage, exhaust, fluids, flyers, interaction, inventory,
    light_dither, lighting, particles, plates, player, reference, render_readiness, save,
    simulation, sky_traffic, sleep, starting_area, survival, vehicle, weather, wheel_effects,
    world,
};

#[cfg(target_arch = "wasm32")]
pub mod app;

#[cfg(target_arch = "wasm32")]
pub mod browser;

pub mod display_settings;

#[cfg(target_arch = "wasm32")]
mod render_scale;

#[cfg(target_arch = "wasm32")]
pub mod presentation;

pub mod ownership;

#[cfg(test)]
#[path = "presentation/world/preflight/selected.rs"]
#[allow(dead_code)]
mod selected_tests;

#[cfg(all(test, not(target_arch = "wasm32")))]
#[path = "presentation/world/shared_materials.rs"]
#[allow(dead_code)]
mod shared_material_tests;

#[cfg(all(test, not(target_arch = "wasm32")))]
#[path = "presentation/mirror_turns.rs"]
mod mirror_turns_tests;

#[cfg(all(test, not(target_arch = "wasm32")))]
#[path = "presentation/cap_motion.rs"]
#[allow(dead_code)]
mod cap_motion_tests;

#[cfg(all(test, not(target_arch = "wasm32")))]
#[path = "presentation/outfit.rs"]
#[allow(dead_code)]
mod outfit_tests;

#[cfg(all(test, not(target_arch = "wasm32")))]
#[path = "presentation/world/lod.rs"]
#[allow(dead_code)]
mod lod_tests;

#[cfg(all(test, not(target_arch = "wasm32")))]
#[path = "presentation/world/node_plan.rs"]
#[allow(dead_code)]
mod node_plan_tests;

#[cfg(all(test, not(target_arch = "wasm32")))]
#[path = "presentation/world/gpu_props.rs"]
#[allow(dead_code)]
mod gpu_props_tests;

#[cfg(not(target_arch = "wasm32"))]
#[path = "presentation/world/grass_worker.rs"]
pub mod grass_worker;
