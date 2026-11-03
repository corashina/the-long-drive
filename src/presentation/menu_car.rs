//! The start menu car. A copy of the garage start car turns on a dark stage while the start menu is open.
//! Port choice. The source menu shows a figure. This port shows the start car, as a showroom.
//! The copy draws the authored surfaces with Bevy PBR, as the figure stages do. Source-only
//! effects (the condition shader extension, plate text, mirror images) are not drawn.
use super::figure_stage::StageCamera;
use super::materials::SourceMaterial;
use super::materials::specular::SpecularMaterial;
use super::world::instances::{CarMaterial, CarSurface, CarTarget, SourceInstances};
use super::world::node_plan::matrix_transform;
use crate::app::RuntimeReference;
use bevy::asset::UntypedAssetId;
use bevy::camera::primitives::MeshAabb;
use bevy::camera::visibility::RenderLayers;
use bevy::camera::{CameraOutputMode, ClearColorConfig, Exposure};
use bevy::ecs as bevy_ecs;
use bevy::mesh::skinning::SkinnedMesh;
use bevy::prelude::*;
use bevy::render::view::NoIndirectDrawing;
use bevy::window::PrimaryWindow;
use std::collections::HashMap;
use std::f32::consts::{PI, TAU};

/// Source layers are 0 to 31, and the appearance preview uses 32.
const LAYER: usize = 33;
/// 10 km below the world, away from the appearance preview stage.
const STAGE: Vec3 = Vec3::new(1_000., -10_000., 0.);
/// The stage camera draws after every source camera.
const ORDER: isize = 101;
/// One turn of the car.
const TURN_SECONDS: f32 = 30.;
/// Vertical field of view.
const FOV: f32 = 30. * PI / 180.;
/// The camera looks down at this angle.
const PITCH: f32 = 9. * PI / 180.;
/// Free space around the car, as a factor on its bounding radius.
const MARGIN: f32 = 1.2;
/// The car sits this fraction of the half view height above the middle, clear of the buttons.
const LIFT: f32 = 0.22;
/// Backdrop and floor (sRGB). The floor away from the lights reads as the backdrop.
const BACKDROP: Color = Color::srgb(0.035, 0.033, 0.031);
const FLOOR: Color = Color::srgb(0.16, 0.15, 0.14);
/// With `ev100` 0, Bevy multiplies light by 1 / 1.2. These factors make an intensity of 1 give
/// albedo times 1 at 1 m, as in the figure stages.
const LIGHT_SCALE: f32 = PI * 1.2;

#[derive(Resource, Default)]
pub struct MenuCar {
    stage: Option<Entity>,
    turntable: Option<Entity>,
    camera: Option<Entity>,
    /// Bounding radius and height of the car copy.
    size: Vec2,
    angle: f32,
    surfaces: Vec<CarSurface>,
}

/// Follow the browser start menu. Copy the car once it is ready, turn it, and despawn it on close.
#[allow(clippy::too_many_arguments)]
pub(crate) fn sync(
    mut commands: Commands,
    reference: Res<RuntimeReference>,
    instances: Res<SourceInstances>,
    primary: Res<Assets<SourceMaterial>>,
    specular: Res<Assets<SpecularMaterial>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut meshes: ResMut<Assets<Mesh>>,
    time: Res<Time>,
    windows: Query<&Window, With<PrimaryWindow>>,
    globals: Query<&GlobalTransform>,
    parents: Query<&ChildOf>,
    inherited: Query<&InheritedVisibility>,
    skins: Query<&SkinnedMesh>,
    mut transforms: Query<&mut Transform>,
    mut car: ResMut<MenuCar>,
) {
    if !crate::browser::game_menu_car() {
        if let Some(stage) = car.stage.take() {
            commands.entity(stage).despawn();
        }
        car.turntable = None;
        car.camera = None;
        return;
    }
    if let (Some(turntable), Some(camera)) = (car.turntable, car.camera) {
        car.angle = (car.angle + TAU * time.delta_secs() / TURN_SECONDS) % TAU;
        if let Ok(mut transform) = transforms.get_mut(turntable) {
            transform.rotation = Quat::from_rotation_y(car.angle);
        }
        let Ok(window) = windows.single() else {
            return;
        };
        let aspect = window.width().max(1.) / window.height().max(1.);
        if let Ok(mut transform) = transforms.get_mut(camera) {
            transform.set_if_neq(view(car.size, aspect));
        }
        return;
    }
    let MenuCar { surfaces, .. } = &mut *car;
    let Some(root) = instances.start_car(&reference.0.world.prototypes, surfaces) else {
        return;
    };
    let Ok(root_world) = globals.get(root) else {
        return;
    };
    let inverse = root_world.affine().inverse();
    let mut copies = Vec::with_capacity(surfaces.len());
    let mut stage_materials =
        HashMap::<(UntypedAssetId, Option<[u32; 4]>), Handle<StandardMaterial>>::new();
    let (mut min, mut max) = (Vec3::splat(f32::MAX), Vec3::splat(f32::MIN));
    for surface in surfaces.iter() {
        // A skinned surface draws from its joints. The copy gets joints at the same pose
        // relative to the surface, so it draws the current pose on the stage.
        let mut skin = None;
        let (world, color) = match surface.target {
            CarTarget::Entity(entity) => {
                // LOD hides the renderer itself. The node chain above it says whether the part is on.
                let on = parents
                    .get(entity)
                    .ok()
                    .and_then(|p| inherited.get(p.parent()).ok())
                    .is_some_and(|v| v.get());
                let Ok(global) = globals.get(entity) else {
                    continue;
                };
                if !on {
                    continue;
                }
                if let Ok(skinned) = skins.get(entity) {
                    let inverse = global.affine().inverse();
                    let Some(joints) = skinned
                        .joints
                        .iter()
                        .map(|joint| globals.get(*joint).ok().map(|g| inverse * g.affine()))
                        .collect::<Option<Vec<_>>>()
                    else {
                        continue;
                    };
                    skin = Some((skinned.inverse_bindposes.clone(), joints));
                }
                (global.affine(), None)
            }
            CarTarget::Row { world, color } => {
                (bevy::math::Affine3A::from_mat4(world), Some(color))
            }
        };
        let (id, base) = match &surface.material {
            CarMaterial::Primary(h) => (h.id().untyped(), primary.get(h).map(|m| &m.base)),
            CarMaterial::Specular(h) => (h.id().untyped(), specular.get(h).map(|m| m.base())),
        };
        let Some(base) = base else {
            continue;
        };
        let material = stage_materials
            .entry((id, color.map(|c| c.to_array().map(f32::to_bits))))
            .or_insert_with(|| materials.add(stage_material(base, color)))
            .clone();
        let local = inverse * world;
        if let Some(aabb) = meshes.get(&surface.mesh).and_then(MeshAabb::compute_aabb) {
            let (center, half) = (Vec3::from(aabb.center), Vec3::from(aabb.half_extents));
            for corner in 0..8 {
                let sign = Vec3::new(
                    if corner & 1 == 0 { -1. } else { 1. },
                    if corner & 2 == 0 { -1. } else { 1. },
                    if corner & 4 == 0 { -1. } else { 1. },
                );
                let point = local.transform_point3(center + half * sign);
                min = min.min(point);
                max = max.max(point);
            }
        }
        copies.push((surface.mesh.clone(), local, material, skin));
    }
    if copies.is_empty() || min.x > max.x {
        error!("Menu car: the start car has no drawn surfaces");
        return;
    }
    let layer = RenderLayers::layer(LAYER);
    let stage = commands
        .spawn((
            Transform::from_translation(STAGE),
            Visibility::Inherited,
            layer.clone(),
        ))
        .id();
    let turntable = commands
        .spawn((
            Transform::default(),
            Visibility::Inherited,
            layer.clone(),
            ChildOf(stage),
        ))
        .id();
    // The car stands on the floor with its bounds centered on the turntable axis.
    let center = (min + max) * 0.5;
    let body = commands
        .spawn((
            Transform::from_translation(Vec3::new(-center.x, -min.y, -center.z)),
            Visibility::Inherited,
            layer.clone(),
            ChildOf(turntable),
        ))
        .id();
    for (mesh, local, material, skin) in copies {
        let copy = commands
            .spawn((
                Mesh3d(mesh),
                MeshMaterial3d(material),
                matrix_transform(Mat4::from(local)),
                Visibility::Inherited,
                layer.clone(),
                ChildOf(body),
            ))
            .id();
        if let Some((inverse_bindposes, joints)) = skin {
            let joints = joints
                .into_iter()
                .map(|pose| {
                    commands
                        .spawn((matrix_transform(Mat4::from(pose)), ChildOf(copy)))
                        .id()
                })
                .collect();
            commands.entity(copy).insert(SkinnedMesh {
                inverse_bindposes,
                joints,
            });
        }
    }
    let extent = max - min;
    car.size = Vec2::new(extent.xz().length() * 0.5, extent.y);
    commands.spawn((
        Mesh3d(meshes.add(Plane3d::new(Vec3::Y, Vec2::splat(60.)))),
        MeshMaterial3d(materials.add(StandardMaterial {
            base_color: FLOOR,
            perceptual_roughness: 0.85,
            ..default()
        })),
        Transform::default(),
        layer.clone(),
        ChildOf(stage),
    ));
    lights(&mut commands, &layer, stage, car.size);
    let camera = commands
        .spawn((
            StageCamera,
            Camera3d::default(),
            Camera {
                order: ORDER,
                clear_color: ClearColorConfig::Custom(BACKDROP),
                output_mode: CameraOutputMode::Write {
                    blend_state: None,
                    clear_color: ClearColorConfig::None,
                },
                ..default()
            },
            Projection::Perspective(PerspectiveProjection {
                fov: FOV,
                near: 0.1,
                far: 200.,
                ..default()
            }),
            bevy::core_pipeline::tonemapping::Tonemapping::None,
            Msaa::Off,
            NoIndirectDrawing,
            Exposure { ev100: 0. },
            AmbientLight {
                color: Color::srgb(0.3, 0.3, 0.32),
                brightness: 0.25 * 1.2,
                affects_lightmapped_meshes: false,
            },
            view(car.size, 16. / 9.),
            layer,
            ChildOf(stage),
        ))
        .id();
    car.stage = Some(stage);
    car.turntable = Some(turntable);
    car.camera = Some(camera);
    car.angle = 0.;
}

/// The authored surface with the row color. Strips the maps the figure stages strip.
fn stage_material(base: &StandardMaterial, color: Option<Vec4>) -> StandardMaterial {
    let mut material = base.clone();
    if let Some(color) = color {
        material.base_color = Color::LinearRgba(LinearRgba::from_vec4(color));
    }
    material.emissive_texture = None;
    material.metallic_roughness_texture = None;
    material.occlusion_texture = None;
    material.normal_map_texture = None;
    material.depth_map = None;
    if !matches!(material.alpha_mode, AlphaMode::Opaque | AlphaMode::Mask(_)) {
        // The source schedule removes the transparent sort, so glass draws without blending.
        material.alpha_mode = AlphaMode::Mask(0.5);
    }
    material
}

/// Camera pose that frames a car of bounding radius `size.x` and height `size.y`.
fn view(size: Vec2, aspect: f32) -> Transform {
    let half_v = FOV * 0.5;
    let half_h = (half_v.tan() * aspect).atan();
    let radius = size.x * MARGIN;
    let distance = (radius / half_h.tan()).max(radius / half_v.tan()) + radius;
    let middle = Vec3::Y * size.y * 0.5;
    // Aim below the car so that it sits above the middle of the view.
    let target = middle - Vec3::Y * distance * half_v.tan() * LIFT;
    let position = middle + Vec3::new(0., PITCH.sin(), PITCH.cos()) * distance;
    Transform::from_translation(position).looking_at(target, Vec3::Y)
}

/// A key light, a fill light, a rim light and a pool of light on the floor.
fn lights(commands: &mut Commands, layer: &RenderLayers, stage: Entity, size: Vec2) {
    let r = size.x.max(1.);
    let aim = Vec3::Y * size.y * 0.5;
    // Lumens that give `intensity` at 1 m.
    let lumens = |intensity: f32| intensity * 4. * PI * LIGHT_SCALE;
    let warm = Color::srgb(1., 0.93, 0.82);
    let spots = [
        // Key: front right, above.
        (Vec3::new(1.2 * r, 1.6 * r, 1.6 * r), aim, warm, 1.1, 45.),
        // Rim: behind, above.
        (
            Vec3::new(-0.6 * r, 1.4 * r, -2. * r),
            aim,
            Color::srgb(0.85, 0.9, 1.),
            0.9,
            40.,
        ),
        // Floor pool: straight down.
        (Vec3::new(0., 2.4 * r, 0.), Vec3::ZERO, warm, 0.7, 50.),
    ];
    for (position, target, color, factor, angle) in spots {
        let distance = position.distance(target);
        let up = if (target - position).normalize().y.abs() > 0.99 {
            Vec3::Z
        } else {
            Vec3::Y
        };
        commands.spawn((
            SpotLight {
                color,
                intensity: lumens(factor * distance * distance),
                range: distance * 3.,
                outer_angle: (angle * 0.5_f32).to_radians(),
                inner_angle: (angle * 0.3_f32).to_radians(),
                ..default()
            },
            Transform::from_translation(position).looking_at(target, up),
            layer.clone(),
            ChildOf(stage),
        ));
    }
    // Fill: front left, low, soft.
    let fill = Vec3::new(-1.6 * r, 0.8 * r, 1.2 * r);
    let distance = fill.distance(aim);
    commands.spawn((
        PointLight {
            color: Color::srgb(0.8, 0.85, 1.),
            intensity: lumens(0.35 * distance * distance),
            range: distance * 2.5,
            ..default()
        },
        Transform::from_translation(fill),
        layer.clone(),
        ChildOf(stage),
    ));
}
