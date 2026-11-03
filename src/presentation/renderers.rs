//! Source renderer records of startup primitives.
use super::materials::{RenderingState, SourceMaterials};
use super::reference::ReferenceVisuals;
use super::renderer_visibility::SourceRenderer;
use crate::reference::render_lighting::RendererRecord;
use bevy::ecs as bevy_ecs;
use bevy::gltf::{GltfExtras, GltfMaterialExtras};
use bevy::prelude::*;

#[derive(Resource, Default)]
pub struct RendererMeshes {
    pub bound: bool,
}
/// Renderer record from the manifest. The player glTF has no renderer extras.
#[derive(Component)]
pub struct AuthoredRenderer(pub RendererRecord);
#[derive(serde::Deserialize)]
struct RendererExtras {
    source_renderer: RendererRecord,
}

pub fn bind_renderer_meshes(
    mut commands: Commands,
    mut state: ResMut<RendererMeshes>,
    mut materials: ResMut<SourceMaterials>,
    visuals: Res<ReferenceVisuals>,
    meshes: Res<Assets<Mesh>>,
    primitives: Query<
        (Entity, &ChildOf, &Mesh3d, Option<&AuthoredRenderer>),
        (
            With<GltfMaterialExtras>,
            Without<SourceRenderer>,
            Without<super::renderer_visibility::SourceMaterialWarmup>,
        ),
    >,
    extras: Query<&GltfExtras>,
) {
    if !visuals.bound || matches!(materials.state, RenderingState::Failed(_)) {
        return;
    }
    if primitives.is_empty() {
        return;
    }
    // Materials bind after every primitive mesh loads. All records bind in one startup pass.
    if primitives
        .iter()
        .any(|(_, _, mesh, _)| !meshes.contains(mesh.0.id()))
    {
        return;
    }
    for (entity, parent, _, authored) in &primitives {
        let record = match authored {
            Some(authored) => Ok(authored.0.clone()),
            None => extras
                .get(parent.parent())
                .map_err(|error| error.to_string())
                .and_then(|extras| {
                    serde_json::from_str::<RendererExtras>(&extras.value)
                        .map(|value| value.source_renderer)
                        .map_err(|error| error.to_string())
                }),
        };
        let record = match record {
            Ok(record) => record,
            Err(error) => {
                materials.state =
                    RenderingState::Failed(format!("Source renderer metadata: {error}"));
                return;
            }
        };
        if let Err(error) = record.validate() {
            materials.state = RenderingState::Failed(format!("Source renderer metadata: {error}"));
            return;
        }
        commands.entity(entity).insert((
            bevy::camera::primitives::Aabb {
                center: Vec3::from_array(record.bounds_center).into(),
                half_extents: Vec3::from_array(record.bounds_extents).into(),
            },
            SourceRenderer(record),
        ));
    }
    state.bound = true;
}
