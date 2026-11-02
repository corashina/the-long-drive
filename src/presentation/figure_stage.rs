//! Figure stages: the appearance editor preview and the start menu figure.
//! A stage holds a second player asset instance on its own render layer, 10 km below the world.
//! The stage camera draws only that layer into a canvas area.
//! The source views draw only source layers 0 to 31, and their shadow casters need a source renderer. So they do not draw the figure.
//! The figure takes the exported joint poses and the current outfit. The owner despawns the stage when its screen closes.
use super::materials::{
    MaterialIdentity, SourceMaterial, SourceMaterials, authored_caster_material,
};
use super::outfit::Outfit;
use super::player_body::{Identity, character_renderer, named_joints, outfit_slot};
use super::renderers::AuthoredRenderer;
use super::sky::source_pose;
use crate::app::RuntimeReference;
use crate::reference::{
    NodeId, PreviewCamera, PreviewFigure, PreviewLight, PreviewLightKind, ReferenceManifest,
};
use bevy::camera::visibility::{NoFrustumCulling, RenderLayers};
use bevy::camera::{CameraOutputMode, ClearColorConfig, Exposure, Viewport};
use bevy::ecs as bevy_ecs;
use bevy::gltf::{GltfExtras, GltfMaterialExtras};
use bevy::prelude::*;
use bevy::render::{
    Render, RenderApp, RenderSystems,
    camera::ExtractedCamera,
    extract_component::{ExtractComponent, ExtractComponentPlugin},
    render_resource::{Extent3d, TextureDescriptor, TextureDimension, TextureUsages},
    renderer::RenderDevice,
    texture::TextureCache,
    view::NoIndirectDrawing,
};
use bevy::world_serialization::{WorldAssetRoot, WorldInstanceReady};
use std::collections::HashMap;
use std::f32::consts::PI;

/// With `ev100` 0, Bevy multiplies light by 1 / 1.2. Unity diffuse has no 1 / pi.
/// These factors make a source intensity of 1 give albedo times 1 at 1 m.
const LIGHT_SCALE: f32 = PI * 1.2;

/// Marks a stage camera in both worlds.
#[derive(Component, Clone, Copy)]
pub struct StageCamera;
impl bevy::render::sync_component::SyncComponent for StageCamera {
    type Target = Self;
}
impl ExtractComponent for StageCamera {
    type QueryData = &'static Self;
    type QueryFilter = ();
    type Out = Self;
    fn extract_component(_: bevy::ecs::query::QueryItem<'_, '_, Self::QueryData>) -> Option<Self> {
        Some(Self)
    }
}

struct StagePrimitive {
    entity: Entity,
    character: usize,
    /// Outfit segment and `disable_able`.
    segment: Option<(usize, bool)>,
}

#[derive(Default)]
pub struct FigureStage {
    /// The stage root. It owns the camera, the lights, the figure and its joints.
    pub stage: Option<Entity>,
    pub camera: Option<Entity>,
    primitives: Vec<StagePrimitive>,
    /// Material of each (character, segment).
    segments: Vec<(usize, usize, Handle<StandardMaterial>)>,
    pub bound: bool,
    dressed: bool,
    /// Joint entities of each character, in figure record order.
    pub skeletons: Vec<Vec<Entity>>,
    /// The nearest joint ancestor of each figure joint, in figure record order.
    pub parents: Vec<Option<usize>>,
}

/// A resource that owns one stage.
pub trait StageOwner: Resource<Mutability = bevy::ecs::component::Mutable> {
    fn stage(&mut self) -> &mut FigureStage;
    fn figure(manifest: &ReferenceManifest) -> &PreviewFigure;
}

/// The stage setup that differs between owners.
pub struct StageSpec<'a> {
    pub origin: Vec3,
    pub layer: usize,
    pub order: isize,
    pub camera: &'a PreviewCamera,
    pub clear: Color,
    /// Ambient color and the factor on albedo.
    pub ambient: (Color, f32),
    pub lights: &'a [PreviewLight],
    pub figure: &'a PreviewFigure,
}

pub struct FigureStagePlugin;
impl Plugin for FigureStagePlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins(ExtractComponentPlugin::<StageCamera>::default());
        app.sub_app_mut(RenderApp).add_systems(
            Render,
            prepare_depth.in_set(RenderSystems::PrepareResources),
        );
    }
}

/// The source schedule removes the Bevy 3D depth textures. Each stage camera gets its own.
fn prepare_depth(
    mut commands: Commands,
    mut cache: ResMut<TextureCache>,
    device: Res<RenderDevice>,
    views: Query<(Entity, &ExtractedCamera, &Msaa), With<StageCamera>>,
) {
    for (entity, camera, msaa) in &views {
        let Some(size) = camera.physical_target_size else {
            continue;
        };
        let texture = cache.get(
            &device,
            TextureDescriptor {
                label: Some("stage_depth"),
                size: Extent3d {
                    width: size.x,
                    height: size.y,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: msaa.samples(),
                dimension: TextureDimension::D2,
                format: bevy::core_pipeline::core_3d::CORE_3D_DEPTH_FORMAT,
                usage: TextureUsages::RENDER_ATTACHMENT,
                view_formats: &[],
            },
        );
        commands
            .entity(entity)
            .insert(bevy::render::view::ViewDepthTexture::new(texture, Some(0.)));
    }
}

/// Copy of the authored albedo surface with the segment color. The stage camera draws Bevy PBR.
fn surface(source: &SourceMaterials, key: (crate::reference::AssetId, u32)) -> StandardMaterial {
    let mut material = authored_caster_material(key, source);
    if !matches!(material.alpha_mode, AlphaMode::Opaque | AlphaMode::Mask(_)) {
        // The source schedule removes the transparent sort, so the figure draws without blending.
        material.alpha_mode = AlphaMode::Mask(0.5);
    }
    material
}

/// Bind the figure: keep the character primitives, give them stage materials and pose the joints.
fn bind<R: StageOwner>(
    event: On<WorldInstanceReady>,
    mut commands: Commands,
    reference: Res<RuntimeReference>,
    source: Res<SourceMaterials>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut owner: ResMut<R>,
    children: Query<&Children>,
    parents: Query<&ChildOf>,
    extras: Query<&GltfExtras>,
    material_extras: Query<&GltfMaterialExtras>,
    meshes: Query<(), With<Mesh3d>>,
    layers: Query<&RenderLayers>,
) {
    let preview = owner.stage();
    let Some(stage) = preview.stage else {
        return;
    };
    let Ok(layer) = layers.get(stage).cloned() else {
        return;
    };
    let manifest = &reference.0;
    let clothing = &manifest.clothing;
    let characters = &clothing.characters;
    let figure = R::figure(manifest);
    let asset = characters[0].asset;
    let mut joints = HashMap::<NodeId, Entity>::new();
    let mut segments = HashMap::<(usize, usize), Handle<StandardMaterial>>::new();
    let mut shared = HashMap::<u32, Handle<StandardMaterial>>::new();
    for entity in children.iter_descendants(event.entity) {
        let Ok(extra) = extras.get(entity) else {
            continue;
        };
        let Some(node) = serde_json::from_str::<Identity>(&extra.value)
            .ok()
            .and_then(|identity| identity.reference_id)
            .and_then(|id| {
                manifest
                    .nodes
                    .iter()
                    .find(|n| n.source == id && n.asset == asset)
            })
        else {
            continue;
        };
        if characters
            .iter()
            .any(|c| c.skins.iter().any(|s| s.joints.contains(&node.id)))
        {
            joints.insert(node.id, entity);
        }
        let Ok(kids) = children.get(entity) else {
            continue;
        };
        let drawn = character_renderer(characters, node.id);
        for primitive in kids.iter().filter(|e| meshes.contains(*e)) {
            let Some((character, renderer)) = drawn else {
                commands.entity(primitive).despawn();
                continue;
            };
            // The player glTF has no renderer extras, as in `player_body`.
            commands
                .entity(primitive)
                .insert(AuthoredRenderer(renderer.state.clone()));
            let slot = extras
                .get(primitive)
                .map_err(|e| e.to_string())
                .and_then(|e| {
                    outfit_slot(
                        0,
                        manifest,
                        characters,
                        &clothing.invisible_material,
                        character,
                        node.id,
                        e,
                    )
                });
            let (handle, segment) = match slot {
                Ok(Some(slot)) => {
                    let segment = usize::from(slot.segment);
                    let handle = segments
                        .entry((character, segment))
                        .or_insert_with(|| materials.add(surface(&source, (asset, slot.material))))
                        .clone();
                    (handle, Some((segment, slot.invisible.is_some())))
                }
                Ok(None) => {
                    let Some(identity) = material_extras
                        .get(primitive)
                        .ok()
                        .and_then(|e| serde_json::from_str::<MaterialIdentity>(&e.value).ok())
                    else {
                        error!("Figure stage: node {} lacks a material identity", node.id.0);
                        continue;
                    };
                    let handle = shared
                        .entry(identity.gltf_material)
                        .or_insert_with(|| {
                            materials
                                .add(surface(&source, (identity.asset, identity.gltf_material)))
                        })
                        .clone();
                    (handle, None)
                }
                Err(error) => {
                    error!("Figure stage: {error}");
                    continue;
                }
            };
            commands
                .entity(primitive)
                .remove::<MeshMaterial3d<SourceMaterial>>()
                .insert((MeshMaterial3d(handle), layer.clone(), NoFrustumCulling));
            preview.primitives.push(StagePrimitive {
                entity: primitive,
                character,
                segment,
            });
        }
    }
    // The figure joint poses are local to the stage, so each joint is a direct child of the stage.
    // Both characters use the same joint names.
    let mut skeletons = Vec::with_capacity(characters.len());
    for character in characters {
        let names = named_joints(manifest, character);
        let mut skeleton = Vec::with_capacity(figure.joints.len());
        for joint in &figure.joints {
            let Some(entity) = names
                .iter()
                .find(|(_, name)| *name == joint.name)
                .and_then(|(node, _)| joints.get(node))
            else {
                error!("Figure stage: the figure lacks joint {}", joint.name);
                return;
            };
            commands
                .entity(*entity)
                .insert((ChildOf(stage), source_pose(&joint.pose)));
            skeleton.push(*entity);
        }
        skeletons.push(skeleton);
    }
    // The nearest joint ancestor in the glTF hierarchy. `ChildOf` changes only when the commands apply.
    let index: HashMap<Entity, usize> = skeletons[0]
        .iter()
        .enumerate()
        .map(|(i, e)| (*e, i))
        .collect();
    preview.parents = skeletons[0]
        .iter()
        .map(|joint| {
            parents
                .iter_ancestors(*joint)
                .take_while(|e| *e != event.entity)
                .find_map(|e| index.get(&e).copied())
        })
        .collect();
    preview.skeletons = skeletons;
    preview.segments = segments
        .into_iter()
        .map(|((character, segment), handle)| (character, segment, handle))
        .collect();
    preview.bound = true;
    commands.entity(event.entity).insert(Visibility::Inherited);
}

/// `ColorChange` and `PressedToggle` on the figure: show the chosen character with the outfit colors.
fn dress(
    outfit: &Outfit,
    preview: &FigureStage,
    materials: &mut Assets<StandardMaterial>,
    visibility: &mut Query<&mut Visibility>,
) {
    for (character, segment, handle) in &preview.segments {
        let [r, g, b] = outfit.characters[*character].segments[*segment].color();
        if let Some(mut material) = materials.get_mut(handle) {
            material.base_color = Color::srgb(r, g, b);
        }
    }
    for p in &preview.primitives {
        let shown = p.character == outfit.character
            && p.segment.is_none_or(|(segment, disable_able)| {
                !disable_able || outfit.characters[p.character].segments[segment].enabled
            });
        if let Ok(mut v) = visibility.get_mut(p.entity) {
            v.set_if_neq(if shown {
                Visibility::Inherited
            } else {
                Visibility::Hidden
            });
        }
    }
}

impl FigureStage {
    /// Spawn the stage: camera, lights and the figure asset. The figure binds when its scene is ready.
    pub fn spawn<R: StageOwner>(
        &mut self,
        commands: &mut Commands,
        gltf: &bevy::gltf::Gltf,
        spec: StageSpec,
    ) {
        let layer = RenderLayers::layer(spec.layer);
        let stage = commands
            .spawn((
                Transform::from_translation(spec.origin),
                Visibility::Inherited,
                layer.clone(),
            ))
            .id();
        let camera = spec.camera;
        let (ambient, factor) = spec.ambient;
        self.camera = Some(
            commands
                .spawn((
                    StageCamera,
                    Camera3d::default(),
                    Camera {
                        order: spec.order,
                        clear_color: ClearColorConfig::Custom(spec.clear),
                        // Keep the source frame outside the viewport.
                        output_mode: CameraOutputMode::Write {
                            blend_state: None,
                            clear_color: ClearColorConfig::None,
                        },
                        ..default()
                    },
                    Projection::Perspective(PerspectiveProjection {
                        fov: camera.fov.to_radians(),
                        near: camera.near,
                        far: camera.far,
                        ..default()
                    }),
                    bevy::core_pipeline::tonemapping::Tonemapping::None,
                    Msaa::Off,
                    NoIndirectDrawing,
                    Exposure { ev100: 0. },
                    AmbientLight {
                        color: ambient,
                        brightness: factor * 1.2,
                        affects_lightmapped_meshes: false,
                    },
                    layer.clone(),
                    source_pose(&camera.pose),
                    ChildOf(stage),
                ))
                .id(),
        );
        for light in spec.lights {
            let color = Color::srgb(light.color[0], light.color[1], light.color[2]);
            // Lumens that give the source intensity at 1 m.
            let lumens = light.intensity * 4. * PI * LIGHT_SCALE;
            let mut entity =
                commands.spawn((layer.clone(), source_pose(&light.pose), ChildOf(stage)));
            match light.kind {
                PreviewLightKind::Directional => entity.insert(DirectionalLight {
                    color,
                    illuminance: light.intensity * LIGHT_SCALE,
                    ..default()
                }),
                PreviewLightKind::Point => entity.insert(PointLight {
                    color,
                    intensity: lumens,
                    range: light.range,
                    ..default()
                }),
                PreviewLightKind::Spot => entity.insert(SpotLight {
                    color,
                    intensity: lumens,
                    range: light.range,
                    outer_angle: (light.spot_angle * 0.5).to_radians(),
                    ..default()
                }),
            };
        }
        commands
            .spawn((
                WorldAssetRoot(gltf.scenes[0].clone()),
                source_pose(&spec.figure.root_pose),
                Visibility::Hidden,
                ChildOf(stage),
            ))
            .observe(bind::<R>);
        self.stage = Some(stage);
    }

    pub fn despawn(&mut self, commands: &mut Commands) {
        if let Some(stage) = self.stage {
            commands.entity(stage).despawn();
            *self = Self::default();
        }
    }

    /// Place the camera viewport. `rect` is x, y, width and height in window fractions.
    pub fn place(
        &self,
        rect: [f32; 4],
        window: &Window,
        cameras: &mut Query<&mut Camera, With<StageCamera>>,
    ) {
        let Some(mut camera) = self.camera.and_then(|c| cameras.get_mut(c).ok()) else {
            return;
        };
        let size = UVec2::new(window.physical_width(), window.physical_height());
        let viewport = (size.x > 0 && size.y > 0).then(|| {
            let full = size.as_vec2();
            let position = (Vec2::new(rect[0], rect[1]) * full)
                .round()
                .as_uvec2()
                .min(size - 1);
            let extent = (Vec2::new(rect[2], rect[3]) * full)
                .round()
                .as_uvec2()
                .clamp(UVec2::ONE, size - position);
            Viewport {
                physical_position: position,
                physical_size: extent,
                ..default()
            }
        });
        let key = |v: &Option<Viewport>| v.as_ref().map(|v| (v.physical_position, v.physical_size));
        if key(&camera.viewport) != key(&viewport) {
            camera.viewport = viewport;
        }
    }

    /// Dress the figure once bound and again on each outfit change.
    pub fn dress(
        &mut self,
        outfit: &Res<Outfit>,
        materials: &mut Assets<StandardMaterial>,
        visibility: &mut Query<&mut Visibility>,
    ) {
        if self.bound && (!self.dressed || outfit.is_changed()) {
            dress(outfit, self, materials, visibility);
            self.dressed = true;
        }
    }
}

/// The player glTF, once the source visuals and textures are ready.
pub fn player_gltf<'a>(
    reference: &ReferenceManifest,
    visuals: &super::reference::ReferenceVisuals,
    source: &SourceMaterials,
    gltfs: &'a Assets<bevy::gltf::Gltf>,
) -> Option<&'a bevy::gltf::Gltf> {
    let asset = reference.clothing.characters[0].asset;
    visuals
        .bound
        .then_some(())
        .filter(|_| source.textures_ready())
        .and_then(|_| visuals.gltfs.iter().find(|(id, _)| *id == asset))
        .and_then(|(_, handle)| gltfs.get(handle))
}
