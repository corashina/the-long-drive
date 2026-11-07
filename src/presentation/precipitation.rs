//! Rain streaks of the world view: one instanced draw after the opaque pass.
//! The shader hashes each instance id to a point in a box around the camera.
//! The CPU does no work per drop. It writes one uniform per frame.
//! Drops take the ambient sky light, so they show in dark weather. A thunderstorm gives longer,
//! faster streaks.
use super::{
    readiness::RendererStateHandle,
    render_targets::{DEPTH_FORMAT, HDR_FORMAT},
    view_frame::SourceViewFrame,
};
use bevy::render::{
    Render, RenderApp, RenderSystems,
    extract_resource::{ExtractResource, ExtractResourcePlugin},
    render_resource::{binding_types::uniform_buffer, *},
    renderer::{RenderDevice, RenderQueue},
};
use bevy::{ecs as bevy_ecs, prelude::*, render as bevy_render};
use simulation::boundary::StateBlock;

/// Side of the drop box in meters. The fade ends at half the side, where drops are sub-pixel.
const BOX: f32 = 32.;
/// Fall speed in meters per second.
const FALL_SPEED: f32 = 9.;
/// Share of `FALL_SPEED` that a full thunderstorm adds.
const HEAVY_FALL: f32 = 0.4;
/// Streak length in meters. A full thunderstorm doubles it.
const STREAK: f32 = 0.5;
/// Drops inside this distance fade out when the player is sheltered.
const SHELTER_RADIUS: f32 = 3.;
/// Seconds for the shelter fade.
const SHELTER_SECONDS: f32 = 0.5;
/// Drop opacity at full rain.
const OPACITY: f32 = 0.5;
/// Drop color: the ambient sky light times this value.
const AMBIENT_GAIN: f32 = 1.4;

#[derive(Resource, Clone, Copy, Default, ExtractResource)]
struct RainInput {
    count: u32,
    rain: f32,
    /// Streak length in meters.
    streak: f32,
    /// Drop offset in meters by fall and wind, each axis 0..BOX.
    offset: Vec3,
    /// Drop velocity in meters per second: fall plus wind.
    velocity: Vec3,
    /// 0 outside, 1 sheltered.
    shelter: f32,
}

#[derive(Clone, Copy, ShaderType)]
struct RainParams {
    clip_from_world: Mat4,
    camera: Vec4,
    color: Vec4,
    controls: Vec4,
    offset: Vec4,
    axis: Vec4,
}

#[derive(Resource)]
pub struct PrecipitationPass {
    pipeline: CachedRenderPipelineId,
    uniforms: super::uniform_slots::UniformSlots<RainParams>,
    group: BindGroup,
    /// Drops of this frame. 0 skips the draw.
    count: u32,
}

pub fn install(app: &mut App) {
    app.init_resource::<RainInput>()
        .add_plugins(ExtractResourcePlugin::<RainInput>::default())
        .add_systems(PostUpdate, snapshot);
    app.sub_app_mut(RenderApp).add_systems(
        Render,
        prepare
            .after(RenderSystems::PrepareResources)
            .before(RenderSystems::Render),
    );
}

fn snapshot(
    sim: Res<super::sim_frame::SimFrameResource>,
    display: Res<crate::display_settings::DisplaySettings>,
    time: Res<Time>,
    mut input: ResMut<RainInput>,
) {
    let weather = sim.frame.weather;
    let dt = if sim.frame.state.flags & StateBlock::PAUSED != 0 {
        0.
    } else {
        time.delta_secs()
    };
    input.rain = weather.rain;
    input.count = (display.rain_drops() as f32 * weather.rain).round() as u32;
    let [x, z] = weather.wind.map(|v| v * weather.gust);
    let heavy = weather.heavy.clamp(0., 1.);
    input.streak = STREAK * (1. + heavy);
    input.velocity = Vec3::new(x, -FALL_SPEED * (1. + HEAVY_FALL * heavy), z);
    input.offset = (input.offset + input.velocity * dt).rem_euclid(Vec3::splat(BOX));
    let target = if weather.shelter == simulation::weather::Shelter::None {
        0.
    } else {
        1.
    };
    let step = dt / SHELTER_SECONDS;
    input.shelter += (target - input.shelter).clamp(-step, step);
}

fn layout() -> BindGroupLayoutDescriptor {
    BindGroupLayoutDescriptor::new(
        "weather_rain",
        &BindGroupLayoutEntries::single(
            ShaderStages::VERTEX_FRAGMENT,
            uniform_buffer::<RainParams>(false),
        ),
    )
}

#[allow(clippy::too_many_arguments)]
fn prepare(
    mut commands: Commands,
    pass: Option<ResMut<PrecipitationPass>>,
    assets: Option<Res<super::lighting_assets::LightingRenderAssets>>,
    input: Res<RainInput>,
    frame: Res<SourceViewFrame>,
    scene: Res<super::lighting_scene::SourceSceneLighting>,
    antialiasing: Res<super::antialiasing::Antialiasing>,
    device: Res<RenderDevice>,
    queue: Res<RenderQueue>,
    cache: Res<PipelineCache>,
    status: Res<RendererStateHandle>,
) {
    let Some(mut pass) = pass else {
        if let Some(assets) = assets {
            commands.insert_resource(PrecipitationPass::new(
                &device,
                &cache,
                assets.shaders.rain.clone(),
                antialiasing.samples(),
            ));
        }
        return;
    };
    if let Some(error) = super::postprocessing::named_pipeline_failure(
        &cache,
        &[pass.pipeline],
        "Weather rain pipeline",
    ) {
        status.fail(error);
        return;
    }
    pass.count = 0;
    let (Some(view), Some(scene)) = (frame.views[0], scene.0) else {
        return;
    };
    if input.count == 0 {
        return;
    }
    let camera = view.view_from_world.inverse().w_axis.truncate();
    // Rows 0, 1 and 2 of the flat SH hold the linear ambient RGB in w.
    let [r, g, b] = [0, 1, 2].map(|i| scene.ambient_sh[i].w);
    let color = Vec3::new(r, g, b) * AMBIENT_GAIN;
    pass.uniforms.set(
        0,
        &RainParams {
            clip_from_world: view.raster_projection * view.view_from_world,
            camera: camera.extend(BOX),
            color: color.extend(OPACITY * input.rain),
            controls: Vec4::new(input.shelter, SHELTER_RADIUS, input.streak, 0.),
            offset: input.offset.extend(0.),
            axis: input.velocity.normalize().extend(0.),
        },
    );
    pass.uniforms.upload(&queue);
    pass.count = input.count;
}

impl PrecipitationPass {
    fn new(
        device: &RenderDevice,
        cache: &PipelineCache,
        shader: Handle<Shader>,
        samples: u32,
    ) -> Self {
        let layout = layout();
        let blend = BlendComponent {
            src_factor: BlendFactor::SrcAlpha,
            dst_factor: BlendFactor::OneMinusSrcAlpha,
            operation: BlendOperation::Add,
        };
        let pipeline = cache.queue_render_pipeline(RenderPipelineDescriptor {
            label: Some("weather_rain".into()),
            layout: vec![layout.clone()],
            vertex: VertexState {
                shader: shader.clone(),
                entry_point: Some("vertex".into()),
                ..default()
            },
            fragment: Some(FragmentState {
                shader,
                entry_point: Some("fragment".into()),
                targets: vec![Some(ColorTargetState {
                    format: HDR_FORMAT,
                    blend: Some(BlendState {
                        color: blend,
                        alpha: blend,
                    }),
                    write_mask: ColorWrites::ALL,
                })],
                ..default()
            }),
            primitive: PrimitiveState {
                topology: PrimitiveTopology::TriangleStrip,
                cull_mode: None,
                ..default()
            },
            // Reverse depth: nearer is greater.
            depth_stencil: Some(DepthStencilState {
                format: DEPTH_FORMAT,
                depth_write_enabled: Some(false),
                depth_compare: Some(CompareFunction::GreaterEqual),
                stencil: default(),
                bias: default(),
            }),
            multisample: MultisampleState {
                count: samples,
                ..default()
            },
            ..default()
        });
        let uniforms = super::uniform_slots::UniformSlots::new(device, "weather_rain", 1);
        let group = device.create_bind_group(
            "weather_rain",
            &cache.get_bind_group_layout(&layout),
            &BindGroupEntries::single(uniforms.binding(0)),
        );
        Self {
            pipeline,
            uniforms,
            group,
            count: 0,
        }
    }

    /// The pipeline when this frame has something to draw.
    pub fn pipeline<'a>(&self, cache: &'a PipelineCache) -> Option<&'a RenderPipeline> {
        if self.count == 0 {
            return None;
        }
        cache.get_render_pipeline(self.pipeline)
    }

    /// Records the draw into the shared weather pass.
    pub fn encode(&self, render: &mut wgpu::RenderPass<'_>, pipeline: &RenderPipeline) {
        render.set_pipeline(pipeline);
        render.set_bind_group(0, &self.group, &[]);
        render.draw(0..4, 0..self.count);
    }
}
