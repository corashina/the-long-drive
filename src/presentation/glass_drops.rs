//! Rain drops on the glass of the seated car. `source_glass.wgsl` draws them procedurally.
//! The flag is the cabin view: a car shelter and the player camera. Only glass near the camera gets drops.
use bevy::render::extract_resource::{ExtractResource, ExtractResourcePlugin};
use bevy::{ecs as bevy_ecs, prelude::*, render as bevy_render};
use simulation::boundary::{PoseRow, StateBlock};
use simulation::weather::Shelter;

/// Glass farther from the camera gets no drops, in meters.
const CABIN_RADIUS: f32 = 2.5;
/// Drop speed up the glass per car speed.
const AIR: f32 = 0.06;
/// Drop speed down the glass by gravity, in meters per second.
const SLIDE: f32 = 0.03;
/// The scroll wraps here. It is a whole number of drop cells (5 cm).
const SCROLL_PERIOD: f32 = 51.2;
/// Seconds for the drops to come or go with the rain.
const FADE_SECONDS: f32 = 4.;
/// At this car speed in m/s, the beads are gone and the runs are longest.
const FULL_SPEED: f32 = 25.;

#[derive(Resource, Clone, Copy, Default, ExtractResource)]
pub struct GlassDrops {
    /// Smoothed rain on the car, 0..1.
    amount: f32,
    /// The camera is in the cabin of a car with a roof.
    cabin: bool,
    /// Meters the runs moved up the glass, 0..SCROLL_PERIOD.
    scroll: f32,
    /// Car speed share of `FULL_SPEED`, 0..1.
    speed: f32,
}

impl GlassDrops {
    /// The `cabin` and `drops` values of the glass view uniform.
    pub fn uniform(&self, camera: Vec3) -> (Vec4, Vec4) {
        if !self.cabin || self.amount <= 0. {
            return (Vec4::ZERO, Vec4::ZERO);
        }
        (
            camera.extend(CABIN_RADIUS),
            Vec4::new(
                self.amount,
                self.scroll,
                1. - self.speed,
                1. + 1.5 * self.speed,
            ),
        )
    }
}

pub fn install(app: &mut App) {
    app.init_resource::<GlassDrops>()
        .add_plugins(ExtractResourcePlugin::<GlassDrops>::default())
        .add_systems(PostUpdate, snapshot);
}

fn snapshot(
    sim: Res<super::sim_frame::SimFrameResource>,
    time: Res<Time>,
    mut drops: ResMut<GlassDrops>,
) {
    let frame = &sim.frame;
    let state = &frame.state;
    let dt = if state.flags & StateBlock::PAUSED != 0 {
        0.
    } else {
        time.delta_secs()
    };
    let car = frame.weather.shelter == Shelter::Car;
    let outside_view = (state.sleep_camera.flags | state.view_camera.flags) & PoseRow::ACTIVE != 0;
    drops.cabin = car && !outside_view;
    let target = if car { frame.weather.rain } else { 0. };
    let step = dt / FADE_SECONDS;
    drops.amount += (target - drops.amount).clamp(-step, step);
    let speed = frame.hud.speed_kmh.abs() / 3.6;
    drops.speed = (speed / FULL_SPEED).min(1.);
    drops.scroll = (drops.scroll + (AIR * speed - SLIDE) * dt).rem_euclid(SCROLL_PERIOD);
}
