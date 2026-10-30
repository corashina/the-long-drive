//! Appearance editor preview (`settingsplayercustomscript.camStuff`).
//! While the screen is open, a figure stage stands at the `camStuff` place, 10 km below the world.
//! The preview camera draws into the canvas area next to the panel.
use super::figure_stage::{FigureStage, StageCamera, StageOwner, StageSpec, player_gltf};
use super::materials::SourceMaterials;
use super::outfit::Outfit;
use super::reference::ReferenceVisuals;
use crate::app::RuntimeReference;
use crate::reference::{PreviewFigure, ReferenceManifest};
use bevy::ecs as bevy_ecs;
use bevy::prelude::*;
use bevy::window::PrimaryWindow;

/// Source layers are 0 to 31. No source view draws this layer.
const PREVIEW_LAYER: usize = 32;
/// `camStuff` place: 10 km below the world origin.
const STAGE: Vec3 = Vec3::new(0., -10_000., 0.);
/// After every source camera.
const ORDER: isize = 100;
const CLEAR: Color = Color::srgb(0.12, 0.12, 0.12);
/// Port choice: the source preview takes the scene ambient light. The preview uses a fixed one.
const AMBIENT: f32 = 0.25;

#[derive(Resource, Default)]
pub struct OutfitPreview(FigureStage);
impl StageOwner for OutfitPreview {
    fn stage(&mut self) -> &mut FigureStage {
        &mut self.0
    }
    fn figure(manifest: &ReferenceManifest) -> &PreviewFigure {
        &manifest.clothing.preview.figure
    }
}

/// Follow the browser appearance screen: spawn on open, place the viewport, dress on change, despawn on close.
pub(crate) fn sync(
    mut commands: Commands,
    reference: Res<RuntimeReference>,
    visuals: Res<ReferenceVisuals>,
    gltfs: Res<Assets<bevy::gltf::Gltf>>,
    source: Res<SourceMaterials>,
    outfit: Res<Outfit>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut preview: ResMut<OutfitPreview>,
    windows: Query<&Window, With<PrimaryWindow>>,
    mut cameras: Query<&mut Camera, With<StageCamera>>,
    mut visibility: Query<&mut Visibility>,
) {
    let mut rect = [0f32; 4];
    if !crate::browser::game_appearance_preview(&mut rect) {
        preview.0.despawn(&mut commands);
        return;
    }
    if preview.0.stage.is_none() {
        let Some(gltf) = player_gltf(&reference.0, &visuals, &source, &gltfs) else {
            return;
        };
        let record = &reference.0.clothing.preview;
        preview.0.spawn::<OutfitPreview>(
            &mut commands,
            gltf,
            StageSpec {
                origin: STAGE,
                layer: PREVIEW_LAYER,
                order: ORDER,
                camera: &record.camera,
                clear: CLEAR,
                ambient: (Color::WHITE, AMBIENT),
                lights: &record.lights,
                figure: &record.figure,
            },
        );
        return;
    }
    if let Ok(window) = windows.single() {
        preview.0.place(rect, window, &mut cameras);
    }
    preview.0.dress(&outfit, &mut materials, &mut visibility);
}
