//! Rain water on the glass of the seated car. `weather_glass.wgsl` draws it procedurally.
//! Stained glass bends the grab with it. Standard transparent glass bends its normal.
//! The flag is the cabin view: a car shelter and the player camera. Only glass near the camera gets water.
use bevy::render::extract_resource::{ExtractResource, ExtractResourcePlugin};
use bevy::render::render_resource::{ShaderType, encase};
use bevy::{ecs as bevy_ecs, prelude::*, render as bevy_render};
use simulation::boundary::{PoseRow, StateBlock};
use simulation::weather::Shelter;

/// Glass farther from the camera gets no water, in meters.
const CABIN_RADIUS: f32 = 2.5;
/// Water speed back along the car per car speed.
const AIR: f32 = 0.06;
/// Water speed down by gravity, in meters per second.
const SLIDE: f32 = 0.03;
/// The flow wraps here, in meters. It keeps the wave phases precise.
const FLOW_PERIOD: f32 = 256.;
/// The clock wraps here, in seconds. The shader waves and rings repeat in it.
const CLOCK_PERIOD: f32 = 64.;
/// Seconds for the water to come or go with the rain.
const FADE_SECONDS: f32 = 4.;
/// At this car speed in m/s, the beads are gone and the runs are longest.
const FULL_SPEED: f32 = 25.;

/// It has the same layout as `GlassWater` in weather_glass.wgsl.
#[derive(Clone, Copy, Debug, Default, PartialEq, ShaderType)]
pub struct GlassWaterUniform {
    /// xyz camera, w cabin radius.
    pub cabin: Vec4,
    /// xyz seated body, the pattern origin. w clock in seconds.
    pub anchor: Vec4,
    /// xyz water flow in world axes, in meters.
    pub flow: Vec4,
    /// x amount (0 is off), y bead and ring share, z run stretch.
    pub drops: Vec4,
}

#[derive(Resource, Clone, Copy, Default, ExtractResource)]
pub struct GlassWater {
    /// Smoothed rain on the car, 0..1.
    amount: f32,
    /// The camera is in the cabin of a car with a roof.
    cabin: bool,
    /// Seated body at the rendered camera.
    body: Vec3,
    rotation: Quat,
    /// Water flow in car axes, in meters, 0..FLOW_PERIOD.
    flow: Vec3,
    /// Seconds, 0..CLOCK_PERIOD.
    clock: f32,
    /// Car speed share of `FULL_SPEED`, 0..1.
    speed: f32,
}

impl GlassWater {
    /// The water uniform of the player view.
    pub fn uniform(&self, camera: Vec3) -> GlassWaterUniform {
        if !self.cabin || self.amount <= 0. {
            return GlassWaterUniform::default();
        }
        GlassWaterUniform {
            cabin: camera.extend(CABIN_RADIUS),
            anchor: self.body.extend(self.clock),
            flow: (self.rotation * self.flow).extend(0.),
            drops: Vec4::new(self.amount, 1. - self.speed, 1. + 1.5 * self.speed, 0.),
        }
    }
}

pub fn install(app: &mut App) {
    app.init_resource::<GlassWater>()
        .add_plugins(ExtractResourcePlugin::<GlassWater>::default())
        .add_systems(PostUpdate, snapshot);
}

fn snapshot(
    sim: Res<super::sim_frame::SimFrameResource>,
    drive: Res<crate::app::Drive>,
    time: Res<Time>,
    mut water: ResMut<GlassWater>,
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
    water.cabin = car && !outside_view;
    let target = if car { frame.weather.rain } else { 0. };
    let step = dt / FADE_SECONDS;
    water.amount += (target - water.amount).clamp(-step, step);
    let pose = drive.rendered_body_pose(state.body_pose.pose());
    water.body = Vec3::from_array(pose.position);
    water.rotation = Quat::from_array(pose.rotation);
    let speed = frame.hud.speed_kmh.abs() / 3.6;
    water.speed = (speed / FULL_SPEED).min(1.);
    // The car forward is -Z, so the air pushes the water to +Z.
    let velocity = Vec3::Z * (AIR * speed) + water.rotation.inverse() * Vec3::NEG_Y * SLIDE;
    water.flow = (water.flow + velocity * dt).rem_euclid(Vec3::splat(FLOW_PERIOD));
    water.clock = (water.clock + dt).rem_euclid(CLOCK_PERIOD);
}
