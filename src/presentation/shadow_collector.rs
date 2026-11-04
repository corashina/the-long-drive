//! Source sun shadow cascades of the world view, written before any view's surface draws.
use super::{
    lighting_assets::LightingRenderAssets,
    readiness::RendererStateHandle,
    render_targets::SunAtlas,
    shadow_atlas::ShadowCadence,
    shadow_phase::ShadowDrawState,
    shadow_queue::ShadowQueues,
    shadow_scene::ShadowScene,
    surface_graph::{SourceSurface, SourceSurfaceStage},
    surface_views::{SourceSurfaceView, SurfaceResources},
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
    // x=1-strength, y=inverse atlas extent, z=atlas extent,
    // w=0 no valid cascades, 1 PCF 7x7, 2 PCF 3x3
    params: Vec4,
}
/// Sun shadow preparation of the world view. Surface shaders sample the cascades at each fragment.
/// Probe faces and mirrors render after the world view, so they sample the same atlas and cascades.
#[derive(Resource, Default)]
pub struct SunShadow {
    atlas: Option<Arc<SunAtlas>>,
    pub value: SunShadowUniform,
    /// Fade of the world view fit. The fade center is a sphere center (w=1), so every view can use it.
    pub fade: Vec4,
    pub fade_center: Vec4,
    key: Option<super::shadow_preparation::CollectorKey>,
    pub prepared_frame: u32,
    encoded_frame: AtomicU32,
}
impl SunShadow {
    pub fn encoded_frame(&self) -> u32 {
        self.encoded_frame.load(Ordering::Acquire)
    }
}
fn prepare(
    scene: Res<ShadowScene>,
    frame: Res<SourceViewFrame>,
    assets: Option<Res<LightingRenderAssets>>,
    resources: Option<Res<SurfaceResources>>,
    views: Query<(&SourceSurfaceView, &ShadowCadence)>,
    mut sun: ResMut<SunShadow>,
    status: Res<RendererStateHandle>,
) {
    if scene.frame == 0
        || scene.frame != frame.frame
        || !frame.required_views_ready()
        || status.failure().is_some()
    {
        return;
    }
    let (Some(assets), Some(resources)) = (assets, resources) else {
        return;
    };
    let light = &assets.record.lights[0];
    let basis = super::shadow_queue::light_basis(scene.sun);
    // Medium and Low use the cheaper PCF 3x3 filter.
    let filter = if frame.shadow_quality.0 > 0 { 2. } else { 1. };
    let atlas = &resources.targets.sun;
    for (view, cadence) in &views {
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
            atlas_size: atlas.size,
            filter: filter as u32,
        };
        let same_atlas = sun
            .atlas
            .as_ref()
            .is_some_and(|old| Arc::ptr_eq(old, atlas));
        if same_atlas && sun.key == Some(key) {
            sun.prepared_frame = frame.frame;
            continue;
        }
        let stale = same_atlas.then_some(sun.value);
        let mut value = SunShadowUniform {
            params: Vec4::new(
                1. - light.shadows.strength,
                1. / atlas.size as f32,
                atlas.size as f32,
                0.,
            ),
            world_to_shadow: [Mat4::ZERO; 4],
            ..default()
        };
        let (mut fade, mut fade_center) = (Vec4::ZERO, Vec4::ZERO);
        if let Some(fit) = scene.fits[view.index] {
            value.params.w = filter;
            fade = Vec4::from_array(fit.fade);
            fade_center = Vec4::new(
                fit.fade_center[0],
                fit.fade_center[1],
                -fit.fade_center[2],
                fit.fade_center[3],
            );
            for (index, cascade) in fit.cascades.iter().enumerate() {
                // A stale cascade keeps the transform and sphere of its atlas tile.
                if let Some(old) = stale.filter(|_| !cadence.plan.render[index]) {
                    value.world_to_shadow[index] = old.world_to_shadow[index];
                    value.spheres[index] = old.spheres[index];
                    value.radii_squared[index] = old.radii_squared[index];
                    continue;
                }
                let clip = super::shadow_queue::cascade_projection(cascade, basis).0;
                // Clip to this top-left atlas tile. `clip` already holds the projection flip
                // and the world conversion. Do not apply them again.
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
        sun.atlas = Some(atlas.clone());
        sun.value = value;
        sun.fade = fade;
        sun.fade_center = fade_center;
        sun.key = Some(key);
        sun.prepared_frame = frame.frame;
    }
}
/// Marks the frame's sun shadows complete once the world atlas is encoded.
/// The surface passes of every view wait for it.
fn collect(
    view: ViewQuery<(&SourceSurfaceView, &ShadowDrawState)>,
    frame: Res<SourceViewFrame>,
    queues: Res<ShadowQueues>,
    sun: Res<SunShadow>,
    status: Res<RendererStateHandle>,
) {
    let (view, shadow) = view.into_inner();
    if !queues.ready[view.index]
        || queues.frame != frame.frame
        || sun.prepared_frame != frame.frame
        || shadow.encoded_frame() != frame.frame
        || status.failure().is_some()
    {
        return;
    }
    sun.encoded_frame.store(frame.frame, Ordering::Release);
}
pub fn install(app: &mut App) {
    app.sub_app_mut(RenderApp)
        .init_resource::<SunShadow>()
        .add_systems(
            Render,
            prepare
                .in_set(RenderSystems::PrepareResources)
                .after(super::shadow_queue::write_cascades)
                .before(super::surface_views::prepare_views),
        )
        .add_systems(SourceSurface, collect.in_set(SourceSurfaceStage::Collector));
}
