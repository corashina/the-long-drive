//! Persistent targets and lighting bindings for the world and probe views.
use super::{
    lighting_assets::LightingRenderAssets,
    lighting_scene::{PROBE_COUNT, SourceSceneLighting},
    materials::MaterialLoading,
    readiness::RendererStateHandle,
    render_targets::{HDR_FORMAT, ProbeTargets, PublishedProbes, SurfaceTargets},
    source_lighting::SourceViewLighting,
    stained_glass::{GlassViewUniform, SourceGlassView},
    surface_graph::SurfaceViewTargets,
    transparent::SourceTransparentPasses,
};
use bevy::render::{
    extract_component::ExtractComponent,
    render_asset::RenderAssets,
    render_resource::*,
    renderer::{RenderDevice, RenderQueue},
    texture::GpuImage,
    view::ExtractedView,
};
use bevy::{ecs as bevy_ecs, prelude::*};

pub const WORLD_VIEWS: usize = 1;
pub const PROBE_START: usize = WORLD_VIEWS;
pub const MIRROR_START: usize = PROBE_START + 6;
/// `MirrorsUpdate` renders the 5 nearest mirrors.
pub const MIRROR_COUNT: usize = 5;
pub const VIEW_COUNT: usize = MIRROR_START + MIRROR_COUNT;
/// Side of the square `defMirrorTexture`.
pub const MIRROR_EXTENT: u32 = 256;

pub fn probe_view(index: usize) -> bool {
    (PROBE_START..MIRROR_START).contains(&index)
}
pub fn mirror_view(index: usize) -> bool {
    (MIRROR_START..VIEW_COUNT).contains(&index)
}

/// Source `playermodeloutfitscript.SetDistance`: `player_model` `_Dist` is 0.35 while the player camera draws.
/// Other views, such as the probe faces and mirrors, draw it with 0.
pub fn player_dist(index: usize) -> f32 {
    if index < WORLD_VIEWS { 0.35 } else { 0. }
}

/// The world view precedes six probe faces and the mirrors.
/// Shadow values come from each view's source fit and sun shadow preparation.
#[derive(Component, Clone, Copy)]
pub struct SourceSurfaceView {
    pub index: usize,
    pub near: f32,
    pub far: f32,
    pub clip_from_world: Mat4,
    pub shadow_fade: Vec4,
    pub shadow_fade_center: Vec4,
}
impl bevy::render::sync_component::SyncComponent for SourceSurfaceView {
    type Target = Self;
}
impl ExtractComponent for SourceSurfaceView {
    type QueryData = &'static Self;
    type QueryFilter = ();
    type Out = Self;
    fn extract_component(
        item: bevy::ecs::query::QueryItem<'_, '_, Self::QueryData>,
    ) -> Option<Self> {
        Some(*item)
    }
}
#[derive(Resource)]
pub struct SurfaceResources {
    pub targets: SurfaceTargets,
    pub probes: [ProbeTargets; PROBE_COUNT],
    published: PublishedProbes,
    probe_sampler: Sampler,
    shadow_compare: Sampler,
}
impl SurfaceResources {
    pub fn new(
        device: &RenderDevice,
        queue: &RenderQueue,
        width: u32,
        height: u32,
        samples: u32,
        atlas: u32,
    ) -> Result<Self, String> {
        let published = PublishedProbes::new(device, queue);
        Ok(Self {
            targets: SurfaceTargets::new(device, width, height, samples, atlas)?,
            probes: std::array::from_fn(|slot| ProbeTargets::new(device, &published, slot)),
            published,
            probe_sampler: device.create_sampler(&SamplerDescriptor {
                label: Some("source_probe_sampler"),
                mag_filter: FilterMode::Linear,
                min_filter: FilterMode::Linear,
                mipmap_filter: MipmapFilterMode::Linear,
                ..default()
            }),
            shadow_compare: device.create_sampler(&SamplerDescriptor {
                label: Some("source_shadow_linear_compare"),
                compare: Some(CompareFunction::GreaterEqual),
                mag_filter: FilterMode::Linear,
                min_filter: FilterMode::Linear,
                ..default()
            }),
        })
    }
}

pub(crate) fn prepare_views(
    mut commands: Commands,
    frame: Res<super::view_frame::SourceViewFrame>,
    assets: Option<Res<LightingRenderAssets>>,
    scene: Res<SourceSceneLighting>,
    loading: Option<Res<MaterialLoading>>,
    mut existing: Option<ResMut<SurfaceResources>>,
    images: Res<RenderAssets<GpuImage>>,
    mut views: Query<(
        Entity,
        &SourceSurfaceView,
        &ExtractedView,
        &Msaa,
        Option<&SurfaceViewTargets>,
        Option<&mut SourceViewLighting>,
        Option<&mut SourceGlassView>,
        Option<&super::shadow_collector::CollectorView>,
    )>,
    device: Res<RenderDevice>,
    queue: Res<RenderQueue>,
    cache: Res<PipelineCache>,
    status: Res<RendererStateHandle>,
    antialiasing: Res<super::antialiasing::Antialiasing>,
    lamps: Res<super::lamp_shadows::LampShadows>,
) {
    if status.failure().is_some() {
        return;
    }
    let samples = antialiasing.samples();
    let (Some(assets), Some(scene), Some(capacity)) = (
        assets,
        scene.0,
        loading
            .as_ref()
            .and_then(|value| value.transparent_pass_capacity()),
    ) else {
        return;
    };
    let mut extents = [None; VIEW_COUNT];
    for (_, source, view, msaa, _, _, _, _) in &mut views {
        if source.index >= VIEW_COUNT
            || extents[source.index].is_some()
            || view.target_format != HDR_FORMAT
            || msaa.samples()
                != if source.index < WORLD_VIEWS {
                    samples
                } else {
                    1
                }
        {
            status.fail("Source surface view identity, format, or sample count is invalid".into());
            return;
        }
        if view.viewport.z == 0 || view.viewport.w == 0 {
            return;
        }
        extents[source.index] = Some(view.viewport.zw());
    }
    let Some(main) = extents[0] else { return };
    if extents[PROBE_START..MIRROR_START]
        .iter()
        .flatten()
        .any(|size| *size != UVec2::splat(64))
        || extents[MIRROR_START..]
            .iter()
            .flatten()
            .any(|size| *size != UVec2::splat(MIRROR_EXTENT))
    {
        status.fail("Source surface view extents disagree".into());
        return;
    }
    if (PROBE_START..VIEW_COUNT)
        .any(|index| !frame.skip_preparation(index) && extents[index].is_none())
    {
        return;
    }
    if frame.views[0].is_none() {
        return;
    }
    let Some(atlas) = frame.atlas_size(0) else {
        status.fail("Source world atlas extent is invalid".into());
        return;
    };
    let needs_targets = existing.as_ref().is_none_or(|value| {
        value.targets.world.width != main.x
            || value.targets.world.height != main.y
            || value.targets.views[0].atlas_size != atlas
    });
    if needs_targets {
        if let Some(existing) = existing.as_mut() {
            // Window resize does not invalidate the fixed-resolution published probe.
            match existing
                .targets
                .resize_world(&device, main.x, main.y, samples, atlas)
            {
                Ok(()) => {}
                Err(error) => {
                    status.fail(error);
                    return;
                }
            }
        } else {
            match SurfaceResources::new(&device, &queue, main.x, main.y, samples, atlas) {
                Ok(value) => {
                    commands.insert_resource(value);
                }
                Err(error) => {
                    status.fail(error);
                }
            }
            return;
        }
    }
    let resources = existing.unwrap();
    let cookie_id = assets.record.lights[0]
        .cookie
        .as_ref()
        .expect("validated sun cookie");
    let Some(cookie) = assets
        .textures
        .get(cookie_id)
        .and_then(|handle| images.get(handle.id()))
    else {
        return;
    };
    for (entity, source, _, _, bound, binding, glass, sun_shadow) in &mut views {
        if frame.skip_preparation(source.index) {
            continue;
        }
        let target = &resources.targets.views[source.index];
        let uniform = scene.for_view(
            source.shadow_fade,
            source.shadow_fade_center,
            source.clip_from_world,
            Vec4::new(source.near, source.far, player_dist(source.index), 0.),
            sun_shadow.map_or_else(Default::default, |collector| collector.value),
            if source.index < WORLD_VIEWS {
                lamps.uniforms
            } else {
                Default::default()
            },
        );
        let glass_value = match GlassViewUniform::new(
            source.clip_from_world,
            source.near,
            source.far,
            UVec2::new(target.colors.width, target.colors.height),
        ) {
            Ok(value) => value,
            Err(error) => {
                status.fail(error.into());
                return;
            }
        };
        if bound.is_some_and(|bound| std::sync::Arc::ptr_eq(&bound.0, target)) {
            let Some(mut binding) = binding else {
                status.fail("Source view lighting binding is missing".into());
                return;
            };
            let Some(mut glass) = glass else {
                status.fail("Source glass view binding is missing".into());
                return;
            };
            if let Err(error) = glass.write(glass_value, &device, &queue) {
                status.fail(error.into());
                return;
            }
            binding.write(uniform, &device, &queue);
            continue;
        }
        let cubes = if !probe_view(source.index) {
            &resources.published.view
        } else {
            &resources.published.capture_view
        };
        // Only MSAA views keep the resolved camera depth free of the transparent attachment.
        let camera_depth = target
            .multisample_depth
            .is_some()
            .then(|| target.depth.sampled.as_ref())
            .flatten();
        let glass = match SourceGlassView::new(&device, &queue, &cache, glass_value, camera_depth) {
            Ok(value) => value,
            Err(error) => {
                status.fail(error.into());
                return;
            }
        };
        commands.entity(entity).insert((
            glass,
            SurfaceViewTargets(target.clone()),
            SourceViewLighting::new(
                &device,
                &queue,
                &cache,
                uniform,
                (&cookie.texture_view, &cookie.sampler),
                (cubes, &resources.probe_sampler),
                (
                    target.atlas.sampled.as_ref().unwrap(),
                    &resources.shadow_compare,
                ),
                resources.targets.lamp_atlas.sampled.as_ref().unwrap(),
            ),
            SourceTransparentPasses::new(capacity),
        ));
        if let Some(resolve) = super::depth_resolve::DepthResolveView::new(&device, &cache, target)
        {
            commands.entity(entity).insert(resolve);
        }
    }
}
