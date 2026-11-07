//! Wheel spray of the world view: one instanced draw after the opaque pass.
//! The emitters go into a ring of slots at a fixed rate. Each slot write is one queue write.
//! The shader moves each particle by its slot age, so particles keep no state.
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
    render_resource::{binding_types::uniform_buffer_sized, *},
    renderer::{RenderContext, RenderDevice, RenderQueue, ViewQuery},
};
use bevy::{ecs as bevy_ecs, prelude::*, render as bevy_render};
use simulation::boundary::{StateBlock, WheelSprayRow};
use simulation::weather::MAX_SPRAY;

/// Ring slots. The shader has the same value.
const SLOTS: usize = 16;
/// Seconds between slot writes. `SLOTS` slots cover more than `LIFE`.
const SLOT_SECONDS: f32 = 0.05;
/// Longest particle life in seconds.
const LIFE: f32 = 0.7;
/// The clock wraps after this many seconds.
const PERIOD: f32 = 1000.;
/// The birth of an empty slot. Its age is always more than `LIFE`.
const EMPTY: f32 = -1e9;
/// Particles per emitter in each slot and the opacity of one particle at full amount,
/// by the "Rain" display level. High has 6 particles at 0.22 * 8 / 6: the same cover as 8 at 0.22.
const LEVELS: [(u32, f32); 3] = [(6, 0.29), (4, 0.22), (2, 0.22)];

#[derive(Resource, Clone, Copy, ExtractResource)]
struct SprayInput {
    rows: [WheelSprayRow; MAX_SPRAY],
    count: usize,
    /// Frame seconds. 0 while paused.
    dt: f32,
    /// Changes when the stream origin moves. Old slots are then invalid.
    epoch: u32,
    origin: [f64; 3],
    particles: u32,
    opacity: f32,
}

impl Default for SprayInput {
    fn default() -> Self {
        Self {
            rows: [WheelSprayRow::default(); MAX_SPRAY],
            count: 0,
            dt: 0.,
            epoch: 0,
            origin: [0.; 3],
            particles: LEVELS[0].0,
            opacity: LEVELS[0].1,
        }
    }
}

#[derive(Clone, Copy, ShaderType)]
struct SprayParams {
    clip_from_world: Mat4,
    camera: Vec4,
    color: Vec4,
    controls: Vec4,
    births: [Vec4; SLOTS / 4],
}

#[derive(Resource)]
struct SprayPass {
    pipeline: CachedRenderPipelineId,
    uniforms: super::uniform_slots::UniformSlots<SprayParams>,
    ring: Buffer,
    group: BindGroup,
    /// The clock in seconds, 0..PERIOD.
    clock: f32,
    /// Seconds since the last slot write.
    since: f32,
    /// The next slot to write.
    slot: usize,
    births: [f32; SLOTS],
    epoch: u32,
    /// Instances of this frame. 0 skips the draw.
    instances: u32,
}

pub fn install(app: &mut App) {
    app.init_resource::<SprayInput>()
        .add_plugins(ExtractResourcePlugin::<SprayInput>::default())
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
            draw.in_set(SourceSurfaceStage::Weather)
                .run_if(super::frame_graph::surface_ready)
                .run_if(super::shadow_queue::collector_encoded),
        );
}

fn snapshot(
    sim: Res<super::sim_frame::SimFrameResource>,
    display: Res<crate::display_settings::DisplaySettings>,
    time: Res<Time>,
    mut input: ResMut<SprayInput>,
) {
    let frame = &sim.frame;
    input.dt = if frame.state.flags & StateBlock::PAUSED != 0 {
        0.
    } else {
        time.delta_secs()
    };
    let count = frame.wheel_spray.len().min(MAX_SPRAY);
    input.count = count;
    // Rows are at the newest step. Cars draw between the last two steps, so move the rows back.
    let lag = (1. - frame.state.alpha) * simulation::simulation::FIXED_DT;
    for (row, from) in input.rows.iter_mut().zip(&frame.wheel_spray[..count]) {
        *row = *from;
        for (p, v) in row.position.iter_mut().zip(from.velocity) {
            *p -= v * lag;
        }
    }
    if input.origin != frame.state.stream_origin {
        input.origin = frame.state.stream_origin;
        input.epoch = input.epoch.wrapping_add(1);
    }
    (input.particles, input.opacity) = LEVELS[(display.rain as usize).min(LEVELS.len() - 1)];
}

fn layout() -> BindGroupLayoutDescriptor {
    BindGroupLayoutDescriptor::new(
        "weather_spray",
        &BindGroupLayoutEntries::sequential(
            ShaderStages::VERTEX_FRAGMENT,
            (
                uniform_buffer_sized(false, Some(SprayParams::min_size())),
                uniform_buffer_sized(false, ring_size().try_into().ok()),
            ),
        ),
    )
}

fn ring_size() -> u64 {
    (SLOTS * MAX_SPRAY * size_of::<WheelSprayRow>()) as u64
}

#[allow(clippy::too_many_arguments)]
fn prepare(
    mut commands: Commands,
    pass: Option<ResMut<SprayPass>>,
    assets: Option<Res<super::lighting_assets::LightingRenderAssets>>,
    input: Res<SprayInput>,
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
            commands.insert_resource(SprayPass::new(
                &device,
                &cache,
                assets.shaders.spray.clone(),
                antialiasing.samples(),
            ));
        }
        return;
    };
    if let Some(error) = super::postprocessing::named_pipeline_failure(
        &cache,
        &[pass.pipeline],
        "Weather spray pipeline",
    ) {
        status.fail(error);
        return;
    }
    let pass = &mut *pass;
    pass.instances = 0;
    if pass.epoch != input.epoch {
        pass.epoch = input.epoch;
        pass.births = [EMPTY; SLOTS];
    }
    pass.clock = (pass.clock + input.dt).rem_euclid(PERIOD);
    pass.since += input.dt;
    if pass.since >= SLOT_SECONDS {
        // A long frame skips slots. The particles of one slot cover the gap.
        pass.since = (pass.since - SLOT_SECONDS).min(SLOT_SECONDS);
        let slot = pass.slot;
        pass.slot = (slot + 1) % SLOTS;
        pass.births[slot] = EMPTY;
        if input.count > 0 {
            let mut rows = [WheelSprayRow::default(); MAX_SPRAY];
            rows[..input.count].copy_from_slice(&input.rows[..input.count]);
            let offset = (slot * MAX_SPRAY * size_of::<WheelSprayRow>()) as u64;
            queue.write_buffer(&pass.ring, offset, bytemuck::cast_slice(&rows));
            pass.births[slot] = pass.clock;
        }
    }
    let live = pass.births.iter().any(|&birth| {
        let age = (pass.clock - birth).rem_euclid(PERIOD);
        birth != EMPTY && age < LIFE + SLOT_SECONDS
    });
    let (Some(view), Some(scene)) = (frame.views[0], scene.0) else {
        return;
    };
    if !live {
        return;
    }
    let camera = view.view_from_world.inverse().w_axis.truncate();
    let fog = scene.fog_color_density.truncate();
    pass.uniforms.set(
        0,
        &SprayParams {
            clip_from_world: view.raster_projection * view.view_from_world,
            camera: camera.extend(pass.clock),
            color: fog.extend(input.opacity),
            controls: Vec4::new(input.particles as f32, LIFE, SLOT_SECONDS, PERIOD),
            births: std::array::from_fn(|i| Vec4::from_slice(&pass.births[i * 4..i * 4 + 4])),
        },
    );
    pass.uniforms.upload(&queue);
    pass.instances = (SLOTS * MAX_SPRAY) as u32 * input.particles;
}

impl SprayPass {
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
            label: Some("weather_spray".into()),
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
        let uniforms = super::uniform_slots::UniformSlots::new(device, "weather_spray", 1);
        let ring = device.create_buffer(&BufferDescriptor {
            label: Some("weather_spray_ring"),
            size: ring_size(),
            usage: BufferUsages::UNIFORM | BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let group = device.create_bind_group(
            "weather_spray",
            &cache.get_bind_group_layout(&layout),
            &BindGroupEntries::sequential((uniforms.binding(0), ring.as_entire_binding())),
        );
        Self {
            pipeline,
            uniforms,
            ring,
            group,
            clock: 0.,
            since: 0.,
            slot: 0,
            births: [EMPTY; SLOTS],
            epoch: 0,
            instances: 0,
        }
    }
}

/// Draws into the world view color after AO. The last transparent pass resolves it.
fn draw(
    view: ViewQuery<(&SourceSurfaceView, &SurfaceViewTargets)>,
    pass: Option<Res<SprayPass>>,
    cache: Res<PipelineCache>,
    mut context: RenderContext,
) {
    let (source, targets) = view.into_inner();
    let Some(spray) = pass.filter(|p| p.instances > 0) else {
        return;
    };
    if source.index >= WORLD_VIEWS {
        return;
    }
    let Some(pipeline) = cache.get_render_pipeline(spray.pipeline) else {
        return;
    };
    let targets = &targets.0;
    let depth = targets.multisample_depth.as_ref().unwrap_or(&targets.depth);
    let mut render = context
        .command_encoder()
        .begin_render_pass(&RenderPassDescriptor {
            label: Some("weather_spray"),
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
    render.set_bind_group(0, &spray.group, &[]);
    render.draw(0..6, 0..spray.instances);
}
