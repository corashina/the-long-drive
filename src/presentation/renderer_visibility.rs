//! Camera membership is separate from the union used for mesh and shadow extraction.
use super::surface_views::SourceSurfaceView;
use super::view_frame::SourceViewFrame;
use crate::reference::render_lighting::RendererRecord;
use crate::reference::shadow_candidates::Bounds;
use bevy::camera::primitives::Aabb;
use bevy::camera::visibility::{RenderLayers, VisibleEntities};
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
    Option<&'static Aabb>,
    &'static GlobalTransform,
    Option<&'static RenderLayers>,
);

/// Layer bits of the first 32 render layers. No `RenderLayers` is layer 0.
fn layer_bits(layers: Option<&RenderLayers>) -> u32 {
    layers.map_or(1, |layers| {
        layers.bits().first().map_or(0, |bits| *bits as u32)
    })
}

/// World bounds of `aabb` in the source frame of `LayerRanges`.
fn source_bounds(aabb: &Aabb, transform: &GlobalTransform) -> Bounds {
    let affine = transform.affine();
    let center = affine.transform_point3a(aabb.center);
    let m = affine.matrix3;
    let extents = m.x_axis.abs() * aabb.half_extents.x
        + m.y_axis.abs() * aabb.half_extents.y
        + m.z_axis.abs() * aabb.half_extents.z;
    Bounds {
        center: [center.x, center.y, -center.z],
        extents: extents.to_array(),
    }
}

/// Keep the Mesh3d class even when empty so Bevy can remove retained phase items.
/// Retain order and capacity. This system does not change ViewVisibility and shadow masks.
pub fn filter_camera_visibility(
    mut views: Query<(&Camera, &mut VisibleEntities, Option<&SourceSurfaceView>)>,
    meshes: Query<CameraMesh, With<Mesh3d>>,
    frame: Res<SourceViewFrame>,
    mut output: ResMut<CameraVisibility>,
) {
    output.entities.clear();
    output.sort_data.clear();
    let class = TypeId::of::<Mesh3d>();
    for (camera, mut visible, view) in &mut views {
        let entities = visible.get_mut(class);
        if !camera.is_active {
            entities.clear();
            continue;
        }
        let ranges = view
            .and_then(|view| frame.views.get(view.index).copied().flatten())
            .map(|snapshot| snapshot.layer_ranges);
        entities.retain(|entity| {
            let Ok((renderer, mode, sort, warmup, hidden, aabb, transform, layers)) =
                meshes.get(*entity)
            else {
                return false;
            };
            if hidden {
                return false;
            }
            // Bevy tested the six camera planes. An entity without bounds is not culled.
            if let (Some(ranges), Some(aabb)) = (ranges, aabb)
                && !ranges.visible(layer_bits(layers), source_bounds(aabb, transform))
            {
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
