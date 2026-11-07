//! Browser application: renderer and presentation over the simulation crate.
#![recursion_limit = "256"]
use wasm_heap as _;

pub use ::simulation::{
    animation, audio, car, conditions, damage, exhaust, fluids, flyers, interaction, inventory,
    light_dither, lighting, particles, plates, player, reference, render_readiness, save,
    simulation, sky_traffic, sleep, starting_area, survival, vehicle, weather, wheel_effects,
    world,
};

#[cfg(target_arch = "wasm32")]
pub mod app;

#[cfg(target_arch = "wasm32")]
pub mod browser;

pub mod display_settings;

pub mod manifest_shell;

#[cfg(target_arch = "wasm32")]
mod render_scale;

#[cfg(target_arch = "wasm32")]
pub mod presentation;

pub mod ownership;

/// Presentation logic that native tests also compile.
#[cfg(any(test, target_arch = "wasm32"))]
pub mod presentation_logic;
