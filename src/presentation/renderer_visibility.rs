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

/// Source `makeinvisible`: the camera does not draw a worn item renderer. Its shadow stays.
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
);

/// Keep the Mesh3d class even when empty so Bevy can remove retained phase items.
/// Retain order and capacity. ViewVisibility and shadow masks are not changed here.
pub fn filter_camera_visibility(
    mut views: Query<(&Camera, &mut VisibleEntities)>,
    meshes: Query<CameraMesh, With<Mesh3d>>,
    mut output: ResMut<CameraVisibility>,
) {
    output.entities.clear();
    output.sort_data.clear();
    let class = TypeId::of::<Mesh3d>();
    for (camera, mut visible) in &mut views {
        let entities = visible.get_mut(class);
        if !camera.is_active {
            entities.clear();
            continue;
        }
        entities.retain(|entity| {
            let Ok((renderer, mode, sort, warmup, hidden)) = meshes.get(*entity) else {
                return false;
            };
            if hidden {
                return false;
            }
            // ShadowsOnly: the shadow scene draws it, the camera does not.
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
