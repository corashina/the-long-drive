//! The F3 main thread CPU time of one frame. It covers the main world update and the render app.
//! On WASM both run on the main thread, one after the other.
//! A mark at each Bevy schedule or set boundary splits the frame into stages for the CPU profile.
//! Render covers extract, specialize, queue, prepare, encode, submit and cleanup.
use super::cpu_profile::{self, Stage};
use bevy::app::{MainScheduleOrder, RunFixedMainLoop, SpawnScene};
use bevy::camera::visibility::VisibilitySystems;
use bevy::ecs::{self as bevy_ecs, schedule::ScheduleLabel};
use bevy::prelude::*;
use bevy::render::renderer::{RenderGraph, RenderGraphSystems};
use bevy::render::{Render, RenderApp, RenderSystems};
use simulation::host::game_now;
use std::sync::atomic::{AtomicU64, Ordering};

/// Empty schedules between the main schedules. Their only system is a mark.
#[derive(ScheduleLabel, Clone, Debug, PartialEq, Eq, Hash)]
struct PreUpdateEnd;
#[derive(ScheduleLabel, Clone, Debug, PartialEq, Eq, Hash)]
struct UpdateEnd;

static MARK: AtomicU64 = AtomicU64::new(0);
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

/// Ends `stage` at this mark and returns the time.
fn lap(stage: Stage) -> f64 {
    let now = game_now();
    if cpu_profile::enabled() {
        cpu_profile::record(stage, now - load(&MARK));
    }
    store(&MARK, now);
    now
}

static UPDATE_START: AtomicU64 = AtomicU64::new(0);

fn update_start() {
    let now = game_now();
    store(&MARK, now);
    store(&UPDATE_START, now);
}

fn pre_update_end() {
    lap(Stage::MainPreUpdate);
}

fn main_update_end() {
    lap(Stage::MainUpdate);
}

fn visibility_end() {
    lap(Stage::MainVisibility);
}

fn update_end() {
    let now = lap(Stage::MainPostUpdate);
    store(&UPDATE_END, now);
    store(&UPDATE_MS, now - load(&UPDATE_START));
}

fn extract_end() {
    lap(Stage::Extract);
}

fn specialize_end() {
    lap(Stage::Specialize);
}

fn queue_end() {
    lap(Stage::Queue);
}

fn prepare_end() {
    lap(Stage::Prepare);
}

fn encode_end() {
    lap(Stage::Encode);
}

fn submit_end() {
    lap(Stage::Submit);
}

fn render_end() {
    let now = lap(Stage::Cleanup);
    store(&RENDER_MS, now - load(&UPDATE_END));
}

pub(super) fn install(app: &mut App) {
    let mut order = app.world_mut().resource_mut::<MainScheduleOrder>();
    order.insert_after(RunFixedMainLoop, PreUpdateEnd);
    order.insert_after(SpawnScene, UpdateEnd);
    app.add_systems(First, update_start)
        .add_systems(PreUpdateEnd, pre_update_end)
        .add_systems(UpdateEnd, main_update_end)
        .add_systems(
            PostUpdate,
            visibility_end.after(VisibilitySystems::MarkNewlyHiddenEntitiesInvisible),
        )
        .add_systems(Last, update_end.after(super::performance::publish));
    app.sub_app_mut(RenderApp)
        .add_systems(
            Render,
            (
                extract_end.before(RenderSystems::ExtractCommands),
                specialize_end
                    .after(RenderSystems::PrepareViews)
                    .before(RenderSystems::Queue),
                queue_end
                    .after(RenderSystems::PhaseSort)
                    .before(RenderSystems::Prepare),
                submit_end
                    .after(RenderSystems::Render)
                    .before(RenderSystems::Cleanup),
                render_end.in_set(RenderSystems::PostCleanup),
            ),
        )
        .add_systems(
            RenderGraph,
            (
                prepare_end.before(RenderGraphSystems::Begin),
                encode_end
                    .after(RenderGraphSystems::Render)
                    .before(RenderGraphSystems::Submit),
            ),
        );
}
