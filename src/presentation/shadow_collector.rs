//! Source sun shadow cascades for each view, written before the view's surface draws.
use super::{
    lighting_assets::LightingRenderAssets,
    readiness::RendererStateHandle,
    render_targets::SourceViewTargets,
    shadow_atlas::ShadowCadence,
    shadow_phase::ShadowDrawState,
    shadow_queue::ShadowQueues,
    shadow_scene::ShadowScene,
    surface_graph::{SourceSurface, SourceSurfaceStage, SurfaceViewTargets},
    surface_views::SourceSurfaceView,
    view_frame::SourceViewFrame,
};
use bevy::render::{
    Render, RenderApp, RenderSystems,
    render_resource::{ShaderType, encase},
    renderer::ViewQuery,
};
use bevy::{ecs as bevy_ecs, prelude::*};
use std::sync::{
    Arc,
    atomic::{AtomicU32, Ordering},
};
/// Matches `SunShadow` in source_view_lighting.wgsl, inside the view lighting uniform.
#[derive(Clone, Copy, Debug, Default, PartialEq, ShaderType)]
pub struct SunShadowUniform {
    world_to_shadow: [Mat4; 4],
    spheres: [Vec4; 4],
    radii_squared: Vec4,
    // x=1-strength, y=inverse atlas extent, z=atlas extent, w=has valid cascades
    params: Vec4,
}
/// Per-view sun shadow preparation. Surface shaders sample the cascades at each fragment.
#[derive(Component)]
pub struct CollectorView {
    targets: Arc<SourceViewTargets>,
    pub value: SunShadowUniform,
    key: super::shadow_preparation::CollectorKey,
    pub prepared_frame: u32,
    encoded_frame: AtomicU32,
}
impl CollectorView {
    pub fn encoded_frame(&self) -> u32 {
        self.encoded_frame.load(Ordering::Acquire)
    }
}
fn prepare(
    mut commands: Commands,
    scene: Res<ShadowScene>,
    frame: Res<SourceViewFrame>,
    assets: Option<Res<LightingRenderAssets>>,
    mut views: Query<(
        Entity,
        &SourceSurfaceView,
        &SurfaceViewTargets,
        &ShadowCadence,
        Option<&mut CollectorView>,
    )>,
    status: Res<RendererStateHandle>,
) {
    if scene.frame == 0
        || scene.frame != frame.frame
        || !frame.required_views_ready()
        || status.failure().is_some()
    {
        return;
    }
    let Some(assets) = assets else {
        return;
    };
    let light = &assets.record.lights[0];
    let rotation = scene.sun;
    let h = |v: Vec3| Vec3::new(v.x, v.y, -v.z);
    let basis = [
        h(rotation * Vec3::X),
        h(rotation * Vec3::Y),
        h(rotation * -Vec3::Z),
    ];
    for (entity, view, targets, cadence, mut previous) in &mut views {
        let Some(snapshot) = frame.views[view.index] else {
            continue;
        };
        let key = super::shadow_preparation::CollectorKey {
            view: scene.view_revision(view.index, &frame),
            cadence: cadence.revision,
            projection: snapshot.raster_projection.to_cols_array().map(f32::to_bits),
            view_from_world: snapshot.view_from_world.to_cols_array().map(f32::to_bits),
            near_far_strength: [snapshot.near, snapshot.far, light.shadows.strength]
                .map(f32::to_bits),
            atlas_size: targets.0.atlas_size,
            primary: view.index == 0,
        };
        if let Some(old) = previous.as_deref_mut() {
            if Arc::ptr_eq(&old.targets, &targets.0) && old.key == key {
                old.prepared_frame = frame.frame;
                continue;
            }
        }
        let stale = previous
            .as_deref()
            .filter(|old| Arc::ptr_eq(&old.targets, &targets.0))
            .map(|old| old.value);
        let mut value = SunShadowUniform {
            params: Vec4::new(
                1. - light.shadows.strength,
                1. / targets.0.atlas_size as f32,
                targets.0.atlas_size as f32,
                0.,
            ),
            world_to_shadow: [Mat4::ZERO; 4],
            ..default()
        };
        if let Some(fit) = scene.fits[view.index] {
            value.params.w = 1.;
            for (index, cascade) in fit.cascades.iter().enumerate() {
                // A stale cascade keeps the transform and sphere of its atlas tile.
                if let Some(old) = stale.filter(|_| !cadence.plan.render[index]) {
                    value.world_to_shadow[index] = old.world_to_shadow[index];
                    value.spheres[index] = old.spheres[index];
                    value.radii_squared[index] = old.radii_squared[index];
                    continue;
                }
                let clip = super::shadow_queue::cascade_projection(cascade, basis).0;
                // Clip to this top-left atlas tile. Projection flip and world conversion
                // are already in clip and must not be applied again.
                let offset = Vec2::new(
                    (index & 1) as f32 * 0.5 + 0.25,
                    (index >> 1) as f32 * 0.5 + 0.25,
                );
                let atlas = Mat4::from_cols(
                    Vec4::new(0.25, 0., 0., 0.),
                    Vec4::new(0., -0.25, 0., 0.),
                    Vec4::Z,
                    Vec4::new(offset.x, offset.y, 0., 1.),
                );
                value.world_to_shadow[index] = atlas * clip;
                value.spheres[index] = Vec4::new(
                    cascade.sphere_center[0],
                    cascade.sphere_center[1],
                    -cascade.sphere_center[2],
                    0.,
                );
                value.radii_squared[index] = cascade.sphere_radius * cascade.sphere_radius;
            }
        }
        if let Some(mut old) = previous.filter(|old| Arc::ptr_eq(&old.targets, &targets.0)) {
            old.value = value;
            old.key = key;
            old.prepared_frame = frame.frame;
            continue;
        }
        commands.entity(entity).insert(CollectorView {
            targets: targets.0.clone(),
            value,
            key,
            prepared_frame: frame.frame,
            encoded_frame: AtomicU32::new(0),
        });
    }
}
/// Marks the frame's sun shadows complete once the atlas is encoded. Surface passes wait for it.
fn collect(
    view: ViewQuery<(&SourceSurfaceView, &CollectorView, &ShadowDrawState)>,
    frame: Res<SourceViewFrame>,
    queues: Res<ShadowQueues>,
    status: Res<RendererStateHandle>,
) {
    let (view, collector, shadow) = view.into_inner();
    if !queues.ready[view.index]
        || queues.frame != frame.frame
        || collector.prepared_frame != frame.frame
        || shadow.encoded_frame() != frame.frame
        || status.failure().is_some()
    {
        return;
    }
    collector
        .encoded_frame
        .store(frame.frame, Ordering::Release);
}
pub fn install(app: &mut App) {
    app.sub_app_mut(RenderApp)
        .add_systems(
            Render,
            prepare
                .in_set(RenderSystems::PrepareResources)
                .after(super::shadow_queue::write_cascades)
                .before(super::surface_views::prepare_views),
        )
        .add_systems(SourceSurface, collect.in_set(SourceSurfaceStage::Collector));
}
