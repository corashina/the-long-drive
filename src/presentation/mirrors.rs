//! Source live car mirrors: `mainscript.MirrorsUpdate`, the "Mirrors" toggle and the
//! "Mirror draw distance" setting. Each live mirror draws a surface view into its slot image.
use super::surface_views::{MIRROR_COUNT, MIRROR_EXTENT, MIRROR_START, SourceSurfaceView};
use super::world::{instance_materials::InstanceMaterials, instances::SourceInstances};
use bevy::camera::visibility::RenderLayers;
use bevy::render::extract_resource::ExtractResource;
use bevy::{ecs as bevy_ecs, prelude::*, render as bevy_render};

/// The "Mirrors" and "Mirror draw distance" settings. Off until the browser sends them.
#[derive(Resource, Default)]
pub struct MirrorSettings {
    pub on: bool,
    /// Source `FMirrorDrawDistance`, meters.
    pub draw_distance: f32,
}

/// One mirror camera of this frame.
#[derive(Clone, Copy)]
pub struct MirrorView {
    pub world_from_view: Mat4,
    pub fov_degrees: f32,
    pub near: f32,
    pub far: f32,
    pub mask: u32,
}

/// Mirror cameras of this frame by slot, and the slot images that the mirror materials sample.
#[derive(Resource, Clone, Default, ExtractResource)]
pub struct MirrorFrame {
    pub views: [Option<MirrorView>; MIRROR_COUNT],
    pub images: Vec<Handle<Image>>,
}

/// One `mirrorscript` of a shown car.
struct Candidate {
    root: Entity,
    instance: usize,
    prototype: usize,
    mirror: usize,
    distance: f32,
}

/// Retained buffers. A slot stays with its mirror while the mirror stays live.
#[derive(Default)]
pub(crate) struct MirrorState {
    candidates: Vec<Candidate>,
    /// Instance root and renderer node of each slot.
    slots: [Option<(Entity, u32)>; MIRROR_COUNT],
    masks: [u32; MIRROR_COUNT],
}

/// The `rt` of each slot: R8G8B8A8_UNorm, linear, bilinear, clamped. Rows are bottom-up.
pub(crate) fn slot_images(images: &mut Assets<Image>) -> Vec<Handle<Image>> {
    use bevy::image::{ImageAddressMode, ImageFilterMode, ImageSampler, ImageSamplerDescriptor};
    use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat, TextureUsages};
    (0..MIRROR_COUNT)
        .map(|_| {
            let mut image = Image::new_fill(
                Extent3d {
                    width: MIRROR_EXTENT,
                    height: MIRROR_EXTENT,
                    depth_or_array_layers: 1,
                },
                TextureDimension::D2,
                &[0, 0, 0, 255],
                TextureFormat::Rgba8Unorm,
                bevy::asset::RenderAssetUsages::RENDER_WORLD,
            );
            image.texture_descriptor.usage |= TextureUsages::RENDER_ATTACHMENT;
            image.sampler = ImageSampler::Descriptor(ImageSamplerDescriptor {
                address_mode_u: ImageAddressMode::ClampToEdge,
                address_mode_v: ImageAddressMode::ClampToEdge,
                mag_filter: ImageFilterMode::Linear,
                min_filter: ImageFilterMode::Linear,
                ..default()
            });
            images.add(image)
        })
        .collect()
}

/// Read the settings that the browser sends at start and after each change.
pub(crate) fn poll(mut settings: ResMut<MirrorSettings>) {
    let mut value = [0.; 2];
    if crate::browser::game_mirrors(&mut value) {
        settings.on = value[0] != 0.;
        settings.draw_distance = value[1];
    }
}

/// Source `Mathf.InverseLerp`.
fn inverse_lerp(a: f32, b: f32, value: f32) -> f32 {
    if a == b {
        0.
    } else {
        ((value - a) / (b - a)).clamp(0., 1.)
    }
}

/// Source `MirrorsUpdate`. Runs after transform propagation, before the view snapshot.
/// The source skips it while the cursor is free. The port keeps the mirrors live in menus.
#[allow(clippy::too_many_arguments)]
pub(crate) fn select(
    settings: Res<MirrorSettings>,
    reference: Option<Res<crate::app::RuntimeReference>>,
    drive: Res<crate::app::Drive>,
    sim: Res<super::sim_frame::SimFrameResource>,
    instances: Option<ResMut<SourceInstances>>,
    mut materials: InstanceMaterials,
    transforms: Query<&GlobalTransform>,
    visibility: Query<&ViewVisibility>,
    head: Query<&GlobalTransform, With<super::camera::DriveCamera>>,
    mut cameras: Query<(&SourceSurfaceView, &mut Camera, &mut RenderLayers)>,
    mut frame: ResMut<MirrorFrame>,
    mut state: Local<MirrorState>,
    mut commands: Commands,
    status: Res<super::readiness::RendererStateHandle>,
) {
    frame.views = [None; MIRROR_COUNT];
    let (Some(reference), Some(mut instances), Ok(head)) = (reference, instances, head.single())
    else {
        return;
    };
    if frame.images.len() != MIRROR_COUNT {
        return;
    }
    let world = &reference.0.world;
    let head = head.translation();
    let player = Vec3::from_array(
        drive
            .rendered_body_pose(sim.frame.state.body_pose.pose())
            .position,
    );
    let MirrorState {
        candidates,
        slots,
        masks,
    } = &mut *state;
    candidates.clear();
    for (instance, root, prototype, nodes) in instances.mirror_cars() {
        let record = &world.prototypes[prototype];
        let Some(vehicle) = record.vehicle.as_ref() else {
            continue;
        };
        for (index, mirror) in vehicle.mirrors.iter().enumerate() {
            let plane = nodes[record.nodes[mirror.plane as usize].gltf_node as usize];
            let Some(plane) = plane.and_then(|e| transforms.get(e).ok()) else {
                continue;
            };
            candidates.push(Candidate {
                root,
                instance,
                prototype,
                mirror: index,
                distance: player.distance(plane.translation()),
            });
        }
    }
    candidates.sort_by(|a, b| a.distance.total_cmp(&b.distance));
    let globals = &world.mirrors;
    // Pass 1: the live mirrors and their views. Pass 2 gives new live mirrors free slots.
    let mut live: [Option<(usize, MirrorView)>; MIRROR_COUNT] = [None; MIRROR_COUNT];
    for (rank, candidate) in candidates.iter().enumerate().take(MIRROR_COUNT) {
        if !settings.on {
            break;
        }
        let record = &world.prototypes[candidate.prototype];
        let mirror = &record.vehicle.as_ref().unwrap().mirrors[candidate.mirror];
        let node = |index: u32| {
            instances.node_entity(candidate.instance, record.nodes[index as usize].gltf_node)
        };
        let (Some(plane), Some(camera)) = (
            node(mirror.plane).and_then(|e| transforms.get(e).ok()),
            node(mirror.camera).and_then(|e| transforms.get(e).ok()),
        ) else {
            continue;
        };
        let up = plane.up();
        let position = camera.translation();
        let fov = globals.min_fov.lerp(
            globals.max_fov,
            inverse_lerp(
                globals.max_distance,
                globals.min_distance,
                position.distance(head),
            ),
        );
        let reflected = (plane.translation() - head).reflect(*up);
        let k = inverse_lerp(
            globals.max_distance,
            globals.min_distance,
            candidate.distance,
        );
        let visible = instances
            .mirror_renderers(candidate.instance, mirror.renderer)
            .any(|entity| visibility.get(entity).is_ok_and(|v| v.get()));
        let far = settings.draw_distance * k;
        // Angle(T.up, v) < 90 degrees. Near the max distance, far drops to the near plane:
        // that camera has no frustum and fails the shadow fit. Treat it as out of range.
        if far <= mirror.near || up.dot(reflected) <= 0. || !visible {
            continue;
        }
        let pose = Transform::from_translation(position)
            .looking_at(plane.translation() + reflected, plane.forward());
        live[rank] = Some((
            rank,
            MirrorView {
                world_from_view: pose.to_matrix(),
                fov_degrees: fov,
                near: mirror.near,
                far,
                mask: mirror.mask,
            },
        ));
    }
    // Instance roots and renderer nodes identify slots across frames.
    let key = |c: &Candidate| {
        let vehicle = world.prototypes[c.prototype].vehicle.as_ref().unwrap();
        (c.root, vehicle.mirrors[c.mirror].renderer)
    };
    let mut next: [Option<(Entity, u32)>; MIRROR_COUNT] = [None; MIRROR_COUNT];
    let mut assigned = [None; MIRROR_COUNT];
    for (rank, _) in live.iter().flatten() {
        let identity = key(&candidates[*rank]);
        if let Some(slot) = slots.iter().position(|s| *s == Some(identity)) {
            next[slot] = Some(identity);
            assigned[*rank] = Some(slot);
        }
    }
    for (rank, _) in live.iter().flatten() {
        if assigned[*rank].is_none() {
            let slot = next.iter().position(Option::is_none).unwrap();
            next[slot] = Some(key(&candidates[*rank]));
            assigned[*rank] = Some(slot);
        }
    }
    *slots = next;
    let mut result = Ok(());
    for (rank, candidate) in candidates.iter().enumerate() {
        let mirror = &world.prototypes[candidate.prototype]
            .vehicle
            .as_ref()
            .unwrap()
            .mirrors[candidate.mirror];
        let slot = assigned.get(rank).copied().flatten();
        if let Some(slot) = slot {
            frame.views[slot] = live[rank].map(|(_, view)| view);
        }
        let shown = slot.map(|slot| (&mirror.material.source, &frame.images[slot]));
        result = result.and(instances.show_mirror(
            candidate.instance,
            mirror.renderer,
            shown,
            &mut materials,
            &mut commands,
        ));
    }
    if let Err(error) = result {
        status.fail(error.0);
        return;
    }
    for (view, mut camera, mut layers) in &mut cameras {
        let Some(slot) = view.index.checked_sub(MIRROR_START) else {
            continue;
        };
        let active = frame.views[slot].is_some();
        if camera.is_active != active {
            camera.is_active = active;
        }
        if let Some(view) = frame.views[slot]
            && masks[slot] != view.mask
        {
            masks[slot] = view.mask;
            *layers = (0..32)
                .filter(|layer| view.mask & (1u32 << layer) != 0)
                .collect();
        }
    }
}

pub(crate) fn install(app: &mut App) {
    use bevy::render::extract_resource::ExtractResourcePlugin;
    app.init_resource::<MirrorSettings>()
        .init_resource::<MirrorFrame>()
        .add_plugins(ExtractResourcePlugin::<MirrorFrame>::default())
        .add_systems(Update, poll)
        .add_systems(
            PostUpdate,
            select
                .after(bevy::transform::TransformSystems::Propagate)
                .before(super::view_frame::snapshot),
        );
}
