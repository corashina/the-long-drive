//! Start menu figure (`mainmenuscript.menuCharacter`). While the start menu is open, a figure stage
//! shows the menu camera view of the figure over the whole canvas. `menu_ik` turns it toward the mouse.
//! Port choice: the menu scene skybox and scenery are not ported. The camera clears to its background color.
use super::figure_stage::{FigureStage, StageCamera, StageOwner, StageSpec, player_gltf};
use super::materials::SourceMaterials;
use super::menu_ik::{MenuIk, MenuMouse, MenuView};
use super::outfit::Outfit;
use super::reference::ReferenceVisuals;
use super::sky::source_pose;
use crate::app::RuntimeReference;
use crate::reference::{PreviewFigure, ReferenceManifest};
use bevy::ecs as bevy_ecs;
use bevy::prelude::*;
use bevy::window::PrimaryWindow;

/// Source layers are 0 to 31, and the appearance preview uses 32.
const MENU_LAYER: usize = 33;
/// 10 km below the world, away from the appearance preview stage.
const STAGE: Vec3 = Vec3::new(1_000., -10_000., 0.);
/// After every source camera.
const ORDER: isize = 101;
/// The camera viewport covers the whole canvas.
const FULL: [f32; 4] = [0., 0., 1., 1.];

#[derive(Resource, Default)]
pub struct MenuFigure {
    stage: FigureStage,
    ik: Option<MenuIk>,
}
impl StageOwner for MenuFigure {
    fn stage(&mut self) -> &mut FigureStage {
        &mut self.stage
    }
    fn figure(manifest: &ReferenceManifest) -> &PreviewFigure {
        &manifest.clothing.menu_figure.figure
    }
}

/// Follow the browser start menu: spawn on open, solve IK each frame, dress on change, despawn on close.
pub(crate) fn sync(
    mut commands: Commands,
    reference: Res<RuntimeReference>,
    visuals: Res<ReferenceVisuals>,
    gltfs: Res<Assets<bevy::gltf::Gltf>>,
    source: Res<SourceMaterials>,
    outfit: Res<Outfit>,
    time: Res<Time>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut figure: ResMut<MenuFigure>,
    windows: Query<&Window, With<PrimaryWindow>>,
    mut cameras: Query<&mut Camera, With<StageCamera>>,
    mut visibility: Query<&mut Visibility>,
    mut transforms: Query<&mut Transform>,
) {
    let mut input = [0f32; 4];
    if !crate::browser::game_menu_figure(&mut input) {
        figure.stage.despawn(&mut commands);
        figure.ik = None;
        return;
    }
    let record = &reference.0.clothing.menu_figure;
    if figure.stage.stage.is_none() {
        let Some(gltf) = player_gltf(&reference.0, &visuals, &source, &gltfs) else {
            return;
        };
        let [r, g, b] = record.background;
        let [ar, ag, ab] = record.ambient;
        figure.stage.spawn::<MenuFigure>(
            &mut commands,
            gltf,
            StageSpec {
                origin: STAGE,
                layer: MENU_LAYER,
                order: ORDER,
                camera: &record.camera,
                clear: Color::srgb(r, g, b),
                ambient: (Color::srgb(ar, ag, ab), 1.),
                lights: &record.lights,
                figure: &record.figure,
            },
        );
        return;
    }
    let Ok(window) = windows.single() else {
        return;
    };
    figure.stage.place(FULL, window, &mut cameras);
    figure.stage.dress(&outfit, &mut materials, &mut visibility);
    if !figure.stage.bound {
        return;
    }
    let MenuFigure { stage, ik } = &mut *figure;
    if ik.is_none() {
        *ik = MenuIk::new(record, &stage.parents);
        if ik.is_none() {
            error!("Menu figure: the IK bones do not form limb chains");
            stage.bound = false;
            return;
        }
    }
    let Some(ik) = ik else {
        return;
    };
    let height = window.physical_height().max(1) as f32;
    let view = MenuView {
        pose: source_pose(&record.camera.pose),
        fov: record.camera.fov.to_radians(),
        aspect: window.physical_width() as f32 / height,
    };
    let mouse = MenuMouse {
        position: Vec2::new(input[0], input[1]),
        buttons: input[2] as u32,
        pressed: input[3] as u32,
    };
    ik.update(time.delta_secs(), mouse, &view);
    for skeleton in &stage.skeletons {
        for (entity, pose) in skeleton.iter().zip(ik.poses()) {
            if let Ok(mut transform) = transforms.get_mut(*entity) {
                *transform = *pose;
            }
        }
    }
}
