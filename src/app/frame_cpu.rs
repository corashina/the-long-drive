//! The F3 main thread CPU time of one frame. It covers the main world update and the render app.
//! On WASM both run on the main thread, one after the other.
//! Render covers extract, prepare, queue, command encode and submit.
use bevy::prelude::*;
use bevy::render::{Render, RenderApp, RenderSystems};
use simulation::host::game_now;
use std::sync::atomic::{AtomicU64, Ordering};

static UPDATE_START: AtomicU64 = AtomicU64::new(0);
static UPDATE_END: AtomicU64 = AtomicU64::new(0);
static UPDATE_MS: AtomicU64 = AtomicU64::new(0);
static RENDER_MS: AtomicU64 = AtomicU64::new(0);

fn store(cell: &AtomicU64, value: f64) {
    cell.store(value.to_bits(), Ordering::Relaxed);
}

fn load(cell: &AtomicU64) -> f64 {
    f64::from_bits(cell.load(Ordering::Relaxed))
}

/// Main update and render milliseconds of the last complete frame.
pub(super) fn last_ms() -> (f64, f64) {
    (load(&UPDATE_MS), load(&RENDER_MS))
}

fn update_start() {
    store(&UPDATE_START, game_now());
}

fn update_end() {
    let now = game_now();
    store(&UPDATE_END, now);
    store(&UPDATE_MS, now - load(&UPDATE_START));
}

fn render_end() {
    store(&RENDER_MS, game_now() - load(&UPDATE_END));
}

pub(super) fn install(app: &mut App) {
    app.add_systems(First, update_start)
        .add_systems(Last, update_end.after(super::performance::publish));
    app.sub_app_mut(RenderApp)
        .add_systems(Render, render_end.in_set(RenderSystems::PostCleanup));
}
