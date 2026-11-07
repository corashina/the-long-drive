//! Lightning bolt of the world view: one strip draw after the opaque pass.
//! The shader builds the jagged strip from the seed. The CPU writes one uniform per frame.
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
use simulation::boundary::BoltFrame;
use simulation::weather::BOLT_SECONDS;

/// Strip segments. The shader has the same value.
const SEGMENTS: u32 = 32;
/// Walk step across the line, as a share of the bolt length.
const STEP: f32 = 0.03;
/// Emissive color at the strike.
const COLOR: Vec3 = Vec3::new(14., 15., 20.);
/// Share of the far plane where the bolt draws when it is farther.
const FAR_SHARE: f32 = 0.9;

#[derive(Resource, Clone, Copy, Default, ExtractResource)]
struct BoltInput(Option<BoltFrame>);

#[derive(Clone, Copy, ShaderType)]
struct BoltParams {
    clip_from_world: Mat4,
    camera: Vec4,
    start: Vec4,
    end: Vec4,
    color: Vec4,
}

#[derive(Resource)]
pub struct BoltPass {
    pipeline: CachedRenderPipelineId,
    uniforms: super::uniform_slots::UniformSlots<BoltParams>,
    group: BindGroup,
    /// A bolt shows in this frame.
    visible: bool,
}

pub fn install(app: &mut App) {
    app.init_resource::<BoltInput>()
        .add_plugins(ExtractResourcePlugin::<BoltInput>::default())
        .add_systems(PostUpdate, snapshot);
    app.sub_app_mut(RenderApp).add_systems(
        Render,
        prepare
            .after(RenderSystems::PrepareResources)
            .before(RenderSystems::Render),
    );
}

fn snapshot(sim: Res<super::sim_frame::SimFrameResource>, mut input: ResMut<BoltInput>) {
    input.0 = sim.frame.weather.bolt;
}

fn layout() -> BindGroupLayoutDescriptor {
    BindGroupLayoutDescriptor::new(
        "weather_bolt",
        &BindGroupLayoutEntries::single(
            ShaderStages::VERTEX_FRAGMENT,
            uniform_buffer::<BoltParams>(false),
        ),
    )
}

#[allow(clippy::too_many_arguments)]
fn prepare(
    mut commands: Commands,
    pass: Option<ResMut<BoltPass>>,
    assets: Option<Res<super::lighting_assets::LightingRenderAssets>>,
    input: Res<BoltInput>,
    frame: Res<SourceViewFrame>,
    antialiasing: Res<super::antialiasing::Antialiasing>,
    device: Res<RenderDevice>,
    queue: Res<RenderQueue>,
    cache: Res<PipelineCache>,
    status: Res<RendererStateHandle>,
) {
    let Some(mut pass) = pass else {
        if let Some(assets) = assets {
            commands.insert_resource(BoltPass::new(
                &device,
                &cache,
                assets.shaders.bolt.clone(),
                antialiasing.samples(),
            ));
        }
        return;
    };
    if let Some(error) = super::postprocessing::named_pipeline_failure(
        &cache,
        &[pass.pipeline],
        "Weather bolt pipeline",
    ) {
        status.fail(error);
        return;
    }
    pass.visible = false;
    let (Some(view), Some(bolt)) = (frame.views[0], input.0) else {
        return;
    };
    let camera = view.view_from_world.inverse().w_axis.truncate();
    let start = Vec3::from(bolt.start);
    let end = Vec3::from(bolt.end);
    // Two flickers while the bolt fades out.
    let t = (bolt.age / BOLT_SECONDS).clamp(0., 1.);
    let flicker = 0.7 + 0.3 * (t * 4. * std::f32::consts::TAU).cos();
    pass.uniforms.set(
        0,
        &BoltParams {
            clip_from_world: view.raster_projection * view.view_from_world,
            camera: camera.extend(view.far * FAR_SHARE),
            start: start.extend(STEP * start.distance(end)),
            end: end.extend((bolt.seed & 0xff_ffff) as f32),
            color: (COLOR * (1. - t) * flicker).extend(1.),
        },
    );
    pass.uniforms.upload(&queue);
    pass.visible = true;
}

impl BoltPass {
    fn new(
        device: &RenderDevice,
        cache: &PipelineCache,
        shader: Handle<Shader>,
        samples: u32,
    ) -> Self {
        let layout = layout();
        let add = BlendComponent {
            src_factor: BlendFactor::One,
            dst_factor: BlendFactor::One,
            operation: BlendOperation::Add,
        };
        let pipeline = cache.queue_render_pipeline(RenderPipelineDescriptor {
            label: Some("weather_bolt".into()),
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
                        color: add,
                        alpha: BlendComponent::REPLACE,
                    }),
                    write_mask: ColorWrites::COLOR,
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
        let uniforms = super::uniform_slots::UniformSlots::new(device, "weather_bolt", 1);
        let group = device.create_bind_group(
            "weather_bolt",
            &cache.get_bind_group_layout(&layout),
            &BindGroupEntries::single(uniforms.binding(0)),
        );
        Self {
            pipeline,
            uniforms,
            group,
            visible: false,
        }
    }

    /// The pipeline when this frame has something to draw.
    pub fn pipeline<'a>(&self, cache: &'a PipelineCache) -> Option<&'a RenderPipeline> {
        if !self.visible {
            return None;
        }
        cache.get_render_pipeline(self.pipeline)
    }

    /// Records the draw into the shared weather pass.
    pub fn encode(&self, render: &mut wgpu::RenderPass<'_>, pipeline: &RenderPipeline) {
        render.set_pipeline(pipeline);
        render.set_bind_group(0, &self.group, &[]);
        render.draw(0..SEGMENTS * 6, 0..1);
    }
}
