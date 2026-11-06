//! Distant rain shafts: one instanced draw of soft billboards under the wet cells.
//! The CPU writes one uniform per frame. It also gives the sky the direction of the strongest wet cell.
use super::{
    readiness::RendererStateHandle,
    render_targets::{DEPTH_FORMAT, HDR_FORMAT},
    surface_graph::{SourceSurface, SourceSurfaceStage, SurfaceViewTargets},
    surface_views::{SourceSurfaceView, WORLD_VIEWS},
    view_frame::SourceViewFrame,
};
use bevy::render::{
    Render, RenderApp, RenderSystems,
    extract_resource::{ExtractResource, ExtractResourcePlugin},
    render_resource::{binding_types::uniform_buffer, *},
    renderer::{RenderContext, RenderDevice, RenderQueue, ViewQuery},
};
use bevy::{ecs as bevy_ecs, prelude::*, render as bevy_render};
use simulation::{boundary::StateBlock, boundary::WetCell, weather::MAX_CELLS};

/// Shaft top above the camera in meters: the cloud base.
const CLOUD_BASE: f32 = 1500.;
/// Shaft bottom below the camera in meters. The terrain hides the part under the ground.
const GROUND_DEPTH: f32 = 300.;
/// Shaft half width as a share of the cell radius. The cell weight is full inside this share.
const WIDTH: f32 = 0.7;
/// The shaft and the sky darkening are 0 when the camera is nearer than the first radius share,
/// and full beyond the second. The overcast covers the cell around the camera.
const NEAR_FADE: [f32; 2] = [0.8, 1.3];
/// Meters of the far fade. Cells retire at 12 km.
const FAR_FADE: [f32; 2] = [9000., 12_000.];
/// Shafts farther than this share of the far plane move nearer at the same angular size.
const FAR_SHARE: f32 = 0.8;
/// Streak fall speed in shaft heights per second.
const SCROLL_SPEED: f32 = 0.02;
/// Shaft opacity at full rain.
const OPACITY: f32 = 0.55;
/// The sky darkens toward wet cells within this distance in meters.
const SKY_RANGE: f32 = 8000.;
/// Meters of the sky range fade.
const SKY_FADE: f32 = 2000.;

#[derive(Resource, Clone, Copy, Default, ExtractResource)]
struct ShaftInput {
    /// xy: center `x`, `z`. z: radius. w: wet strength.
    cells: [Vec4; MAX_CELLS],
    count: u32,
    /// Streak offset in shaft heights, 0..1.
    scroll: f32,
}

#[derive(Clone, Copy, ShaderType)]
struct ShaftParams {
    clip_from_world: Mat4,
    camera: Vec4,
    color: Vec4,
    shape: Vec4,
    fade: Vec4,
    cells: [Vec4; MAX_CELLS],
}

#[derive(Resource)]
struct ShaftPass {
    pipeline: CachedRenderPipelineId,
    uniforms: super::uniform_slots::UniformSlots<ShaftParams>,
    group: BindGroup,
    /// Shafts of this frame. 0 skips the draw.
    count: u32,
}

pub fn install(app: &mut App) {
    app.init_resource::<ShaftInput>()
        .add_plugins(ExtractResourcePlugin::<ShaftInput>::default())
        .add_systems(PostUpdate, snapshot);
    app.sub_app_mut(RenderApp)
        .add_systems(
            Render,
            prepare
                .after(RenderSystems::PrepareResources)
                .before(RenderSystems::Render),
        )
        .add_systems(
            SourceSurface,
            draw.in_set(SourceSurfaceStage::AfterOpaque)
                .after(super::depth_resolve::resolve)
                .before(super::source_effects::encode_ao)
                .run_if(super::frame_graph::surface_ready)
                .run_if(super::shadow_queue::collector_encoded),
        );
}

/// 0 near the cell, 1 far from it. `distance` is from the camera to the cell center.
fn near_fade(distance: f32, radius: f32) -> f32 {
    let t = ((distance / radius - NEAR_FADE[0]) / (NEAR_FADE[1] - NEAR_FADE[0])).clamp(0., 1.);
    t * t * (3. - 2. * t)
}

/// The sky darkening. xz: world direction from `player` to the wet cells. w: strength 0..1.
/// The direction is a sum weighted by the squared strength, so it does not jump between cells.
pub(crate) fn sky_storm(cells: &[WetCell], player: Vec2) -> Vec4 {
    let mut sum = Vec2::ZERO;
    let mut strength = 0f32;
    for cell in cells.iter().filter(|c| c.wet > 0.) {
        let offset = Vec2::from_array(cell.center) - player;
        let distance = offset.length();
        let range = ((SKY_RANGE - distance) / SKY_FADE).clamp(0., 1.);
        let s = cell.wet * near_fade(distance, cell.radius) * range;
        if s > 0. {
            sum += offset / distance * s * s;
            strength = strength.max(s);
        }
    }
    let direction = sum.normalize_or_zero();
    Vec4::new(direction.x, 0., direction.y, strength)
}

fn snapshot(
    sim: Res<super::sim_frame::SimFrameResource>,
    time: Res<Time>,
    mut input: ResMut<ShaftInput>,
) {
    let dt = if sim.frame.state.flags & StateBlock::PAUSED != 0 {
        0.
    } else {
        time.delta_secs()
    };
    let mut count = 0;
    for cell in sim.frame.weather.cells.iter().filter(|c| c.wet > 0.) {
        input.cells[count] = Vec4::new(cell.center[0], cell.center[1], cell.radius, cell.wet);
        count += 1;
    }
    input.count = count as u32;
    input.scroll = (input.scroll + SCROLL_SPEED * dt).fract();
}

fn layout() -> BindGroupLayoutDescriptor {
    BindGroupLayoutDescriptor::new(
        "weather_shafts",
        &BindGroupLayoutEntries::single(
            ShaderStages::VERTEX_FRAGMENT,
            uniform_buffer::<ShaftParams>(false),
        ),
    )
}

#[allow(clippy::too_many_arguments)]
fn prepare(
    mut commands: Commands,
    pass: Option<ResMut<ShaftPass>>,
    assets: Option<Res<super::lighting_assets::LightingRenderAssets>>,
    input: Res<ShaftInput>,
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
            commands.insert_resource(ShaftPass::new(
                &device,
                &cache,
                assets.shaders.shafts.clone(),
                antialiasing.samples(),
            ));
        }
        return;
    };
    if let Some(error) = super::postprocessing::named_pipeline_failure(
        &cache,
        &[pass.pipeline],
        "Weather shafts pipeline",
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
    pass.uniforms.set(
        0,
        &ShaftParams {
            clip_from_world: view.raster_projection * view.view_from_world,
            camera: camera.extend(FAR_SHARE * view.far),
            color: scene.fog_color_density,
            shape: Vec4::new(CLOUD_BASE, GROUND_DEPTH, WIDTH, input.scroll),
            fade: Vec4::new(NEAR_FADE[0], NEAR_FADE[1], FAR_FADE[0], FAR_FADE[1]),
            cells: input.cells.map(|c| c * Vec4::new(1., 1., 1., OPACITY)),
        },
    );
    pass.uniforms.upload(&queue);
    pass.count = input.count;
}

impl ShaftPass {
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
            label: Some("weather_shafts".into()),
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
        let uniforms = super::uniform_slots::UniformSlots::new(device, "weather_shafts", 1);
        let group = device.create_bind_group(
            "weather_shafts",
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
}

/// Draws into the world view color before AO, so before the rain streaks too.
fn draw(
    view: ViewQuery<(&SourceSurfaceView, &SurfaceViewTargets)>,
    pass: Option<Res<ShaftPass>>,
    cache: Res<PipelineCache>,
    mut context: RenderContext,
) {
    let (source, targets) = view.into_inner();
    let Some(shafts) = pass.filter(|p| p.count > 0) else {
        return;
    };
    if source.index >= WORLD_VIEWS {
        return;
    }
    let Some(pipeline) = cache.get_render_pipeline(shafts.pipeline) else {
        return;
    };
    let targets = &targets.0;
    let depth = targets.multisample_depth.as_ref().unwrap_or(&targets.depth);
    let mut render = context
        .command_encoder()
        .begin_render_pass(&RenderPassDescriptor {
            label: Some("weather_shafts"),
            color_attachments: &[Some(RenderPassColorAttachment {
                view: targets.colors.attachment(),
                depth_slice: None,
                resolve_target: None,
                ops: Operations {
                    load: LoadOp::Load,
                    store: StoreOp::Store,
                },
            })],
            depth_stencil_attachment: Some(RenderPassDepthStencilAttachment {
                view: &depth.attachment,
                depth_ops: Some(Operations {
                    load: LoadOp::Load,
                    store: StoreOp::Store,
                }),
                stencil_ops: None,
            }),
            ..default()
        });
    render.set_pipeline(pipeline);
    render.set_bind_group(0, &shafts.group, &[]);
    render.draw(0..6, 0..shafts.count);
}
