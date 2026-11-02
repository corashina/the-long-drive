//! Source candidate membership is independent of camera-visible phase lists.
use super::surface_views::VIEW_COUNT;
use super::{
    lighting_assets::LightingAssets,
    lighting_scene::{SourceSceneLighting, SourceWorldOrigin},
    materials::{RenderingState, SourceMaterials},
    readiness::RendererStateHandle,
    renderer_visibility::SourceMaterialWarmup,
    shadow_changes::{ShadowChanges, ShadowMaterials, ShadowMesh, ShadowMeshItem},
    view_frame::SourceViewFrame,
    world::{
        gpu_props::{GpuProps, NO_BATCH, world_bounds},
        instances::SourceInstances,
    },
};
use crate::reference::{
    lamp_shadows::{LAMP_BIT, LAMP_SLOTS, LampMasks},
    shadow_bounds::{BoundsKey, WorldBoundsCache},
    shadow_candidates::CandidateState,
    shadow_frame::{CasterInput, ShadowViewCache},
    shadow_grid::CasterGrid,
    shadow_planes::CullPlanes,
    shadow_scene::{CasterData, CasterIndex},
    shadows::{ShadowFit, ShadowFitInput},
};
use bevy::camera::visibility::{SetViewVisibility, ViewVisibility};
use bevy::{ecs as bevy_ecs, prelude::*};

/// A caster is a renderer entity or a GPU prop row.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum CasterKey {
    Entity(Entity),
    Row(u32),
}

#[derive(Resource, Default)]
pub struct ShadowScene {
    pub frame: u32,
    pub origin: [f64; 3],
    pub imported_capacity: usize,
    pub generated_capacity: usize,
    pub caster_data: CasterData<CasterKey>,
    pub masks: [Vec<u8>; VIEW_COUNT],
    /// Rows that can have a nonzero mask in each view, in row order. Other masks are zero.
    pub marked: [Vec<u32>; VIEW_COUNT],
    pub fits: [Option<ShadowFit>; VIEW_COUNT],
    pub candidates: [usize; VIEW_COUNT],
    pub split_counts: [[usize; 4]; VIEW_COUNT],
    pub shadow_only: usize,
    bounds: WorldBoundsCache<Entity>,
    index: CasterIndex<CasterKey>,
    grid: CasterGrid,
    lamp_masks: LampMasks,
    /// GPU prop owner with its row membership.
    rows: Option<(u64, u64)>,
    /// Caster state of each GPU prop batch material.
    batch_casters: Vec<bool>,
    /// Row slots with changed shadow inputs. Retained storage.
    row_changes: Vec<u32>,
    casters_ready: bool,
    views: [ShadowViewCache; VIEW_COUNT],
    mask_revisions: [u64; VIEW_COUNT],
    /// Changes when a lamp bit of the world view masks changes.
    lamp_revision: u64,
    /// Caster content counter of each lamp slot. It changes when the face content would change.
    pub lamp_contents: [u64; LAMP_SLOTS],
    /// Held sun rotation of the shadow fit. It follows the sun in steps of `SUN_STEP`.
    pub sun: Quat,
    /// Static caster content counter of each view and cascade. It changes when the static tile content would change.
    pub contents: [[u64; 4]; VIEW_COUNT],
    /// Cascade bits of each view with an animated caster.
    pub animated_tiles: [u8; VIEW_COUNT],
    /// Caster rows read again this frame. Retained storage.
    touched: Vec<u32>,
    /// Caster rows that change each frame, in row order. Retained storage.
    pub animated: Vec<u32>,
    /// Entity rows with a nonzero mask in any view. Retained storage.
    visible: Vec<u32>,
}
/// About one texel of a 2048 tile, in radians. A smaller sun turn keeps the cascade tiles.
const SUN_STEP: f32 = 1. / 2048.;
impl ShadowScene {
    pub(crate) fn view_revision(
        &self,
        index: usize,
        frame: &SourceViewFrame,
    ) -> super::shadow_preparation::ViewRevision {
        super::shadow_preparation::ViewRevision {
            session: frame.session,
            scene: frame.scene,
            fit: self.mask_revisions[index],
        }
    }
    pub fn mask_revision(&self, index: usize) -> u64 {
        self.mask_revisions[index]
    }
    pub fn capacity(&self) -> usize {
        self.imported_capacity + self.generated_capacity
    }
    fn reserve(&mut self, imported: usize, generated: usize) {
        self.imported_capacity = imported;
        self.generated_capacity = generated;
        let count = self.capacity();
        self.caster_data.reserve(count);
        for masks in &mut self.masks {
            masks.reserve_exact(count.saturating_sub(masks.len()));
        }
    }
}
/// Runs after both camera culling systems and before visibility removal/extraction.
/// Only ViewVisibility changes; camera VisibleEntities retain their camera-only membership.
pub(super) fn prepare_scene(
    frame: Res<SourceViewFrame>,
    live: Res<SourceSceneLighting>,
    origin: Res<SourceWorldOrigin>,
    assets: Option<Res<LightingAssets>>,
    source: Res<SourceMaterials>,
    materials: ShadowMaterials,
    mut updates: ParamSet<(ShadowChanges, Query<&mut ViewVisibility>)>,
    mut scene: ResMut<ShadowScene>,
    status: Res<RendererStateHandle>,
    master: Res<super::lighting::SourceLighting>,
    camera: Res<super::renderer_visibility::CameraVisibility>,
    meshes: Query<ShadowMesh, (With<ViewVisibility>, Without<SourceMaterialWarmup>)>,
    mut instances: ResMut<SourceInstances>,
    lamps: Res<super::lamp_shadows::LampShadows>,
) {
    scene.frame = 0;
    scene.fits = [None; VIEW_COUNT];
    scene.candidates = [0; VIEW_COUNT];
    scene.split_counts = [[0; 4]; VIEW_COUNT];
    let was_ready = scene.casters_ready;
    scene.casters_ready = false;
    if !frame.required_views_ready()
        || source.state != RenderingState::Ready
        || master.road_lamps.recipe.is_none()
        || status.failure().is_some()
    {
        return;
    }
    let (Some(assets), Some(live)) = (assets, live.0) else {
        return;
    };
    scene.touched.clear();
    let mut changes = updates.p0();
    let mut rebuild = changes.removed() || !changes.warmup.is_empty();
    rebuild |= scene.bounds.begin(frame.session, frame.scene, origin.0) || !was_ready;
    let props = instances.bypass_change_detection().props_mut();
    let rows = (props.id(), props.membership());
    rebuild |= scene.rows != Some(rows);
    scene.rows = Some(rows);
    props.take_shadow_dirty(&mut scene.row_changes);
    let props = &*props;
    let fail = |message: &str| {
        status.fail(message.into());
    };
    // Asset resources include same-frame edits, before AssetEvent delivery in Last.
    // A material edit refreshes eligibility without rebuilding unchanged membership.
    if !rebuild {
        if materials.changed() {
            for mesh in &meshes {
                match update_caster(&mut scene, &materials, mesh, false) {
                    Ok(true) => {}
                    Ok(false) => {
                        rebuild = true;
                        break;
                    }
                    Err(error) => {
                        fail(error);
                        return;
                    }
                }
            }
        } else {
            for entity in &changes.changed {
                let Ok(mesh) = meshes.get(entity) else {
                    continue;
                };
                match update_caster(&mut scene, &materials, mesh, false) {
                    Ok(true) => {}
                    Ok(false) => {
                        rebuild = true;
                        break;
                    }
                    Err(error) => {
                        fail(error);
                        return;
                    }
                }
            }
        }
        if !rebuild && let Err(error) = update_rows(&mut scene, &materials, props, false) {
            fail(error);
            return;
        }
    }
    if rebuild {
        let imported = meshes.iter().count() + props.live() as usize;
        if imported > scene.imported_capacity {
            scene.reserve(imported, 0);
            scene.bounds.reserve(imported);
            scene.index.reserve(imported);
        }
        let scene = &mut *scene;
        scene.index.clear(&mut scene.caster_data);
        scene.bounds.clear();
        scene.animated.clear();
        for mesh in &meshes {
            if let Err(error) = update_caster(scene, &materials, mesh, true) {
                fail(error);
                return;
            }
        }
        if let Err(error) = update_rows(scene, &materials, props, true) {
            fail(error);
            return;
        }
    }
    let scene = &mut *scene;
    if rebuild {
        // Rows changed order. Masks and their row lists start again from zero.
        scene.grid.rebuild(&scene.caster_data.casters);
        for (masks, view) in scene.masks.iter_mut().zip(&mut scene.views) {
            masks.clear();
            view.clear();
        }
        scene.lamp_masks.clear();
    } else {
        // Animated rows are not in `touched`, but their bounds change too.
        for &row in scene.touched.iter().chain(&scene.animated) {
            scene
                .grid
                .update(row, scene.caster_data.casters[row as usize].bounds);
        }
    }
    if let Err(error) = scene.caster_data.publish() {
        fail(error);
        return;
    }
    let light = &assets.record.lights[0];
    if scene.sun.angle_between(live.sun_rotation) > SUN_STEP {
        scene.sun = live.sun_rotation;
    }
    let rotation = scene.sun;
    let all_changed = rebuild || materials.changed() || materials.meshes_changed();
    let h = |v: Vec3| [v.x, v.y, -v.z];
    let light_basis = [
        h(rotation * Vec3::X),
        h(rotation * Vec3::Y),
        h(rotation * -Vec3::Z),
    ];
    for index in 0..VIEW_COUNT {
        let Some(v) = frame.views[index] else {
            scene.masks[index].clear();
            scene.marked[index].clear();
            scene.views[index].clear();
            scene.mask_revisions[index] = scene.views[index].revision();
            scene.contents[index] = scene.contents[index].map(|c| c + 1);
            scene.animated_tiles[index] = 0;
            continue;
        };
        let atlas = frame
            .atlas_size(index)
            .expect("validated source view extent");
        let w = v.source_world_from_view;
        let mut input = ShadowFitInput {
            frustum_points: v.frustum_points,
            camera_basis: [
                w.x_axis.truncate().to_array(),
                w.y_axis.truncate().to_array(),
                w.z_axis.truncate().to_array(),
            ],
            camera_position: w.w_axis.truncate().to_array(),
            light_basis,
            caster_min: [0.; 3],
            caster_max: [0.; 3],
            near: v.near,
            far: v.far,
            vertical_fov: v.fov_degrees,
            shadow_distance: v.shadow_distance,
            split_fractions: assets.record.shadows.split_fractions,
            tile_size: [atlas / 2; 2],
            quality_near_offset: assets.record.shadows.near_offset,
            shadow_strength: light.shadows.strength,
        };
        scene.masks[index].resize(scene.caster_data.casters.len(), 0);
        match scene.views[index].prepare(
            scene.caster_data.revision(),
            &scene.caster_data.casters,
            &scene.grid,
            &mut scene.masks[index],
            CullPlanes {
                camera: v.source_camera_planes,
                shadow: v.source_shadow_planes,
            },
            light.mask,
            v.mask,
            &mut input,
            &scene.animated,
        ) {
            Ok(Some(value)) => {
                scene.fits[index] = Some(value.fit);
                scene.candidates[index] = value.candidate_count;
                scene.split_counts[index] = value.split_counts;
            }
            Ok(None) => {}
            Err(error) => {
                fail(error);
                return;
            }
        }
        scene.mask_revisions[index] = scene.views[index].revision();
        scene.marked[index].clear();
        scene.marked[index].extend_from_slice(scene.views[index].rows());
        let masks = &scene.masks[index];
        let dirty = if all_changed {
            0b1111
        } else {
            scene.views[index].split_changes()
                | scene
                    .touched
                    .iter()
                    .fold(0, |dirty, &row| dirty | masks[row as usize])
        };
        scene.animated_tiles[index] = scene
            .animated
            .iter()
            .fold(0, |tiles, &row| tiles | masks[row as usize])
            & 0b1111;
        for (cascade, content) in scene.contents[index].iter_mut().enumerate() {
            *content += u64::from(dirty >> cascade & 1);
        }
    }
    // Lamp slots draw only in the world view atlas. Both revisions only grow, so their sum changes with either.
    let mut lamp_dirty = 0xf0;
    if frame.views[0].is_some() {
        let changed = scene.lamp_masks.write(
            &scene.caster_data.casters,
            &scene.grid,
            &lamps.spheres,
            &mut scene.masks[0],
        );
        if changed != 0 {
            scene.lamp_revision += 1;
        }
        if !all_changed {
            let masks = &scene.masks[0];
            lamp_dirty = scene
                .touched
                .iter()
                .chain(&scene.animated)
                .fold(changed, |dirty, &row| dirty | (masks[row as usize] & 0xf0));
        }
        let marked = &mut scene.marked[0];
        marked.extend(scene.lamp_masks.rows());
        marked.sort_unstable();
        marked.dedup();
    } else {
        scene.lamp_masks.clear();
    }
    for (slot, content) in scene.lamp_contents.iter_mut().enumerate() {
        *content += u64::from(lamp_dirty >> (LAMP_BIT as usize + slot) & 1);
    }
    scene.mask_revisions[0] = scene.views[0].revision() + scene.lamp_revision;
    drop(changes);
    let mut visibility = updates.p1();
    scene.shadow_only = 0;
    let entities = &scene.caster_data.entities;
    scene.visible.clear();
    for (masks, marked) in scene.masks.iter().zip(&scene.marked) {
        scene.visible.extend(marked.iter().copied().filter(|&row| {
            masks[row as usize] != 0 && matches!(entities[row as usize], CasterKey::Entity(_))
        }));
    }
    scene.visible.sort_unstable();
    scene.visible.dedup();
    for &row in &scene.visible {
        let CasterKey::Entity(entity) = entities[row as usize] else {
            continue;
        };
        let Ok(mut visible) = visibility.get_mut(entity) else {
            continue;
        };
        if !camera.entities.contains(&entity) {
            scene.shadow_only += 1;
        }
        visible.set_visible();
    }
    scene.casters_ready = true;
    scene.origin = origin.0;
    scene.frame = frame.frame;
}
/// Return false when eligibility changes the ordered membership.
fn update_caster(
    scene: &mut ShadowScene,
    materials: &ShadowMaterials,
    mesh: ShadowMeshItem<'_, '_>,
    rebuilding: bool,
) -> Result<bool, &'static str> {
    let key = CasterKey::Entity(mesh.entity);
    let Some(has_caster) = materials.has_caster(&mesh)? else {
        return Ok(!scene.index.contains(key));
    };
    let layer = if let Some(layers) = mesh.layers {
        let mut iter = layers.iter();
        let Some(layer) = iter.next() else {
            return Ok(!scene.index.contains(key));
        };
        if layer >= 32 || iter.next().is_some() {
            return Err("Source shadow renderer requires one source layer");
        }
        layer as u8
    } else {
        0
    };
    if !rebuilding && !scene.index.contains(key) {
        return Ok(false);
    }
    let animated = mesh.animated();
    let world = mesh.transform.affine();
    let bounds = mesh.bounds;
    let bounds_key = BoundsKey::new(
        world.to_cols_array(),
        bounds.center.to_array(),
        bounds.half_extents.to_array(),
    );
    let bounds = scene.bounds.get_or_update(mesh.entity, bounds_key, || {
        world_bounds(
            world,
            bounds.center.to_array(),
            bounds.half_extents.to_array(),
        )
    })?;
    let row = scene.index.update(
        &mut scene.caster_data,
        key,
        CasterInput {
            bounds,
            state: CandidateState {
                shadow_casting_mode: mesh.record.0.cast_shadows,
                layer,
                renderer_present: true,
                disabled: !mesh.inherited.get(),
                force_off: false,
                lod_group: 0,
                lod_index_mask: 0,
                active_lod: 0,
            },
            material_has_caster: has_caster,
        },
    ) as u32;
    if rebuilding {
        if animated {
            scene.animated.push(row);
        }
    } else if animated != scene.animated.binary_search(&row).is_ok() {
        return Ok(false);
    }
    // Animated casters draw in each refresh, so their changes keep the static tile.
    if !animated {
        scene.touched.push(row);
    }
    Ok(true)
}
/// Rows enter the caster list after the entities, in slot order.
/// `all` reads every row. Else only changed rows, or every row when a batch material changes its caster state.
fn update_rows(
    scene: &mut ShadowScene,
    materials: &ShadowMaterials,
    props: &GpuProps,
    mut all: bool,
) -> Result<(), &'static str> {
    if all || materials.changed() {
        scene.batch_casters.resize(props.batches().len(), false);
        for (batch, caster) in props.batches().iter().zip(&mut scene.batch_casters) {
            let next = batch.rows != 0 && materials.row_caster(batch.key.material)?;
            all |= *caster != next;
            *caster = next;
        }
    }
    let changes = std::mem::take(&mut scene.row_changes);
    if all {
        for slot in 0..props.rows().len() as u32 {
            update_row(scene, props, slot);
        }
    } else {
        for &slot in &changes {
            update_row(scene, props, slot);
        }
    }
    scene.row_changes = changes;
    scene.row_changes.clear();
    Ok(())
}
fn update_row(scene: &mut ShadowScene, props: &GpuProps, slot: u32) {
    let row = &props.rows()[slot as usize];
    if row.batch == NO_BATCH {
        return;
    }
    let index = scene.index.update(
        &mut scene.caster_data,
        CasterKey::Row(slot),
        CasterInput {
            bounds: row.bounds,
            state: CandidateState {
                shadow_casting_mode: row.cast_shadows,
                layer: row.layer,
                renderer_present: true,
                disabled: row.hidden,
                force_off: false,
                lod_group: 0,
                lod_index_mask: 0,
                active_lod: 0,
            },
            material_has_caster: scene.batch_casters[row.batch as usize],
        },
    );
    scene.touched.push(index as u32);
}
/// Copy bounded arrays into retained render-world storage. No manifest, material, or handle clones.
pub fn extract_scene(
    input: bevy::render::Extract<Res<ShadowScene>>,
    mut output: ResMut<ShadowScene>,
) {
    output.frame = 0;
    if input.frame == 0 {
        return;
    }
    if output.capacity() < input.capacity() {
        output.reserve(input.imported_capacity, input.generated_capacity);
    }
    assert_eq!(
        output.capacity(),
        input.capacity(),
        "Source shadow extraction capacity changed"
    );
    output.caster_data.extract_from(&input.caster_data);
    for i in 0..VIEW_COUNT {
        if output.mask_revisions[i] != input.mask_revisions[i] {
            output.masks[i].clear();
            output.masks[i].extend_from_slice(&input.masks[i]);
            output.marked[i].clear();
            output.marked[i].extend_from_slice(&input.marked[i]);
            output.mask_revisions[i] = input.mask_revisions[i];
        }
    }
    output.fits = input.fits;
    output.candidates = input.candidates;
    output.split_counts = input.split_counts;
    output.shadow_only = input.shadow_only;
    output.origin = input.origin;
    output.sun = input.sun;
    output.contents = input.contents;
    output.animated_tiles = input.animated_tiles;
    output.animated.clear();
    output.animated.extend_from_slice(&input.animated);
    output.lamp_contents = input.lamp_contents;
    output.frame = input.frame;
}
