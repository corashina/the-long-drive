//! Camera membership is separate from the union used for mesh and shadow extraction.
use crate::reference::render_lighting::RendererRecord;
use bevy::camera::visibility::VisibleEntities;
use bevy::ecs::{
    self as bevy_ecs,
    entity::{EntityHashMap, EntityHashSet},
};
use bevy::prelude::*;
use std::any::TypeId;

#[derive(Component, Clone)]
pub struct SourceRenderer(pub RendererRecord);

#[derive(Component)]
pub struct SourceMaterialWarmup;

/// Port choice: a near terrain tile draws past `Cam1FarPlane`. The distant tier skips the pixels
/// of every drawn near tile (`terrain_coverage`), so a cut tile would leave a hole.
#[derive(Component)]
pub struct NearTerrain;

/// Source `makeinvisible`. The camera does not draw a worn item renderer. Its shadow stays.
#[derive(Component)]
pub struct CameraHidden;

#[derive(Component, Clone, Copy)]
pub struct SourceSortData {
    pub material_key: u32,
    pub material_identity: u32,
    pub glass_source: i64,
    pub shader_identity: i32,
    pub source_material_index: u16,
    pub layer_and_order: u32,
    pub queue: i16,
    pub additive: bool,
    pub glass: bool,
    /// Source named `_GrabTexture` user. Only distortion particles set it.
    pub grab: bool,
}

#[derive(Resource, Default)]
pub struct CameraVisibility {
    pub entities: EntityHashSet,
    pub sort_data: EntityHashMap<SourceSortData>,
}

type CameraMesh = (
    Option<&'static SourceRenderer>,
    Option<&'static super::materials::SourceShadowMode>,
    Option<&'static SourceSortData>,
    Has<SourceMaterialWarmup>,
    Has<CameraHidden>,
    Has<NearTerrain>,
    (
        Option<&'static bevy::camera::visibility::RenderLayers>,
        Option<&'static bevy::camera::primitives::Aabb>,
        &'static GlobalTransform,
    ),
);

/// Source camera depth range of the entity layer. An entity without bounds stays.
fn layer_visible(
    ranges: &super::camera::LayerRanges,
    layers: Option<&bevy::camera::visibility::RenderLayers>,
    aabb: Option<&bevy::camera::primitives::Aabb>,
    transform: &GlobalTransform,
) -> bool {
    let layer = layers.and_then(|l| l.iter().next()).unwrap_or(0);
    let Some(aabb) = aabb else {
        return true;
    };
    let affine = transform.affine();
    let center = affine.transform_point3a(aabb.center);
    let m = affine.matrix3;
    let extents = m.x_axis.abs() * aabb.half_extents.x
        + m.y_axis.abs() * aabb.half_extents.y
        + m.z_axis.abs() * aabb.half_extents.z;
    // The source frame negates z.
    ranges.visible(
        layer as u8,
        [center.x, center.y, -center.z],
        extents.to_array(),
    )
}

/// Keep the Mesh3d class even when empty so Bevy can remove retained phase items.
/// Retain order and capacity. This system does not change ViewVisibility and shadow masks.
pub fn filter_camera_visibility(
    mut views: Query<(
        &Camera,
        &mut VisibleEntities,
        Option<&super::surface_views::SourceSurfaceView>,
    )>,
    meshes: Query<CameraMesh, With<Mesh3d>>,
    frame: Res<super::view_frame::SourceViewFrame>,
    mut output: ResMut<CameraVisibility>,
) {
    output.entities.clear();
    output.sort_data.clear();
    let class = TypeId::of::<Mesh3d>();
    for (camera, mut visible, view) in &mut views {
        let ranges = view
            .and_then(|view| frame.views.get(view.index).copied().flatten())
            .map_or(super::camera::LayerRanges::ALL, |s| s.layer_ranges);
        let entities = visible.get_mut(class);
        if !camera.is_active {
            entities.clear();
            continue;
        }
        entities.retain(|entity| {
            let Ok((renderer, mode, sort, warmup, hidden, near, (layers, aabb, transform))) =
                meshes.get(*entity)
            else {
                return false;
            };
            if hidden || !(warmup || near || layer_visible(&ranges, layers, aabb, transform)) {
                return false;
            }
            // With ShadowsOnly, the shadow scene draws it and the camera does not.
            // The glTF mode arrives before the renderer record.
            if !warmup
                && (renderer.is_some_and(|renderer| renderer.0.cast_shadows == 3)
                    || mode.is_some_and(|mode| mode.0 == 3))
            {
                return false;
            }
            output.entities.insert(*entity);
            if let Some(sort) = sort {
                output.sort_data.insert(*entity, *sort);
            }
            true
        });
    }
}
