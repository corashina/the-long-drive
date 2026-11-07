//! Lamp mask and probe pair of each source surface entity.
//! The vertex stage reads the value by the entity `MeshTag`. Lamps and probes are the same in every view.
//! A value changes only when its object, a lamp or a probe changes. A lamp change tests only that lamp.
use super::lighting::{GpuLight, LightingUniform, SourceLighting};
use super::lighting_scene::{PROBE_COUNT, SourceSceneLighting};
use super::materials::{
    SourceMaterial, grass::material::GrassMaterial, specular::SpecularMaterial,
};
use super::renderer_visibility::SourceRenderer;
use bevy::camera::visibility::{ViewVisibility, VisibilitySystems};
use bevy::ecs as bevy_ecs;
use bevy::mesh::{MeshTag, morph::MeshMorphWeights, skinning::SkinnedMesh};
use bevy::platform::collections::HashSet;
use bevy::prelude::*;
use bevy::render::{
    Extract, ExtractSchedule, RenderApp,
    render_resource::{Buffer, BufferDescriptor, BufferUsages},
    renderer::{RenderDevice, RenderQueue},
};

/// One packed value per object. It has the same layout as `object_lighting` in source_mesh.wgsl.
/// x, y: lamp mask, low bit first. z: first probe slot, second slot (0xff: none) << 8, renderer flags << 16,
/// LOD code (`lod_code`) << 24. w: first probe weight bits.
pub type ObjectValue = [u32; 4];
/// First GPU buffer size, in objects.
const INITIAL_OBJECTS: u64 = 1024;
const VALUE_BYTES: u64 = 16;
const NO_SLOT: u32 = 0xff;

/// Signed LOD crossfade of a primary or specular entity. Without it, the entity has no fade (1).
/// The fade is in the object value, so a fade change does not prepare the material again.
#[derive(Component, Clone, Copy, PartialEq)]
pub struct LodFade(pub f32);

/// Values by slot. A slot belongs to one entity `MeshTag` until the tag goes.
#[derive(Resource)]
pub struct ObjectLighting {
    values: Vec<ObjectValue>,
    /// Inputs of each value, by slot.
    inputs: Vec<SlotInputs>,
    free: Vec<u32>,
    /// Slots written in this frame, as one range.
    dirty: std::ops::Range<usize>,
    /// Count of `update_values` runs with lighting.
    frame: u32,
    /// Last frame with a lamp or probe change.
    changed_at: u32,
    /// Every value must be computed in full: a run had no lighting, so it lost changes.
    stale: bool,
    lamps: Vec<LampKey>,
    probes: Option<Probes>,
}
impl Default for ObjectLighting {
    fn default() -> Self {
        Self {
            values: Vec::new(),
            inputs: Vec::new(),
            free: Vec::new(),
            dirty: 0..0,
            frame: 0,
            changed_at: 0,
            stale: true,
            lamps: Vec::with_capacity(super::lighting::MAX_LIGHTS),
            probes: None,
        }
    }
}
impl ObjectLighting {
    fn allocate(&mut self) -> u32 {
        if let Some(slot) = self.free.pop() {
            self.inputs[slot as usize] = SlotInputs::STALE;
            return slot;
        }
        self.values.push([0, 0, 0, 0]);
        self.inputs.push(SlotInputs::STALE);
        self.values.len() as u32 - 1
    }
    /// Lamps and probes that changed since the last call. It keeps the new state.
    fn light_changes(&mut self, lighting: &LightingUniform, probes: &Probes) -> LightChanges {
        let count = (lighting.counts.x as usize).min(lighting.lights.len());
        let mut lamps = [0u32; 2];
        for i in 0..count.max(self.lamps.len()) {
            let next = (i < count).then(|| LampKey::new(&lighting.lights[i]));
            if self.lamps.get(i) != next.as_ref() {
                lamps[i >> 5] |= 1 << (i & 31);
            }
        }
        self.lamps.clear();
        self.lamps
            .extend(lighting.lights[..count].iter().map(LampKey::new));
        let probes = self.probes.replace(*probes).as_ref() != Some(probes);
        LightChanges { lamps, probes }
    }
}

/// The lamp fields that a lamp mask reads. NaN never compares equal, so it always counts as a change.
#[derive(Clone, Copy, PartialEq)]
struct LampKey {
    position_range: Vec4,
    kind: f32,
    mask: u32,
    lit: bool,
}
impl LampKey {
    fn new(lamp: &GpuLight) -> Self {
        Self {
            position_range: lamp.position_range,
            kind: lamp.direction_kind.w,
            mask: lamp.mask.x,
            lit: lamp.color.x.max(lamp.color.y.max(lamp.color.z)) > 0.,
        }
    }
}

struct LightChanges {
    /// One bit per lamp slot.
    lamps: [u32; 2],
    probes: bool,
}
impl LightChanges {
    fn any(&self) -> bool {
        self.lamps != [0, 0] || self.probes
    }
}

/// What the value of one slot was computed from.
#[derive(Clone, Copy)]
struct SlotInputs {
    /// Frame of the last computation. 0: compute the value in full.
    stamp: u32,
    receiver: u32,
    bounds: WorldBounds,
    usage: bool,
    bounded: bool,
    fade: f32,
    flags: u32,
    probe: ProbeBlend,
}
impl SlotInputs {
    const STALE: Self = Self {
        stamp: 0,
        receiver: 0,
        bounds: WorldBounds::EMPTY,
        usage: false,
        bounded: false,
        fade: 1.,
        flags: 0,
        probe: NO_PROBE,
    };
}

/// Material assets modified in this frame. Their receiver masks can be new.
#[derive(Default)]
struct ModifiedMaterials {
    standard: HashSet<AssetId<SourceMaterial>>,
    specular: HashSet<AssetId<SpecularMaterial>>,
    grass: HashSet<AssetId<GrassMaterial>>,
}
fn modified<A: Asset>(set: &mut HashSet<AssetId<A>>, events: &mut MessageReader<AssetEvent<A>>) {
    set.clear();
    for event in events.read() {
        if let AssetEvent::Modified { id } = event {
            set.insert(*id);
        }
    }
}

/// GPU copy of the values. The buffer grows by powers of two. A growth changes the generation.
#[derive(Resource)]
pub struct ObjectLightingGpu {
    pub buffer: Buffer,
    capacity: u64,
    pub generation: u32,
}
impl FromWorld for ObjectLightingGpu {
    fn from_world(world: &mut World) -> Self {
        let device = world.resource::<RenderDevice>();
        Self {
            buffer: buffer(device, INITIAL_OBJECTS),
            capacity: INITIAL_OBJECTS,
            generation: 0,
        }
    }
}
fn buffer(device: &RenderDevice, objects: u64) -> Buffer {
    device.create_buffer(&BufferDescriptor {
        label: Some("source_object_lighting"),
        size: objects * VALUE_BYTES,
        usage: BufferUsages::STORAGE | BufferUsages::COPY_DST,
        mapped_at_creation: false,
    })
}

pub struct ObjectLightingPlugin;
impl Plugin for ObjectLightingPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<ObjectLighting>()
            .add_observer(assign_slot)
            .add_observer(release_slot)
            .add_systems(
                PostUpdate,
                update_values
                    .after(VisibilitySystems::MarkNewlyHiddenEntitiesInvisible)
                    .after(bevy::transform::TransformSystems::Propagate)
                    .after(super::lighting::sync_lighting)
                    .after(super::lighting_scene::sync_lighting)
                    .after(super::lamp_shadows::select),
            );
    }
    fn finish(&self, app: &mut App) {
        if let Some(render_app) = app.get_sub_app_mut(RenderApp) {
            render_app
                .init_resource::<ObjectLightingGpu>()
                .add_systems(ExtractSchedule, upload_values);
        }
    }
}

fn assign_slot(
    add: On<Add, SourceRenderer>,
    mut lighting: ResMut<ObjectLighting>,
    mut commands: Commands,
) {
    let slot = lighting.allocate();
    commands.entity(add.entity).insert(MeshTag(slot));
}
/// `Discard` also runs when an insert replaces the tag.
fn release_slot(
    remove: On<Discard, MeshTag>,
    tags: Query<&MeshTag>,
    mut lighting: ResMut<ObjectLighting>,
) {
    if let Ok(tag) = tags.get(remove.entity) {
        lighting.free.push(tag.0);
    }
}

type ObjectQuery = (
    &'static MeshTag,
    Ref<'static, SourceRenderer>,
    Ref<'static, GlobalTransform>,
    &'static ViewVisibility,
    Option<Ref<'static, MeshMaterial3d<SourceMaterial>>>,
    Option<Ref<'static, MeshMaterial3d<SpecularMaterial>>>,
    Has<SkinnedMesh>,
    Has<MeshMorphWeights>,
    Option<&'static LodFade>,
);

type GrassQuery = (
    &'static MeshTag,
    Ref<'static, GlobalTransform>,
    &'static ViewVisibility,
    Ref<'static, MeshMaterial3d<GrassMaterial>>,
    Ref<'static, bevy::camera::primitives::Aabb>,
);

/// Values of the objects that a view draws in this frame. Hidden objects keep their old value.
/// A value is computed in full when its object changed or it missed a lamp or probe change.
/// Otherwise a lamp or probe change of this frame updates only those parts.
/// A grass entity value holds only its lamp mask.
#[allow(clippy::too_many_arguments)]
fn update_values(
    mut lighting: ResMut<ObjectLighting>,
    master: Option<Res<SourceLighting>>,
    scene: Res<SourceSceneLighting>,
    standard: Res<Assets<SourceMaterial>>,
    specular: Res<Assets<SpecularMaterial>>,
    grass_materials: Res<Assets<GrassMaterial>>,
    mut standard_events: MessageReader<AssetEvent<SourceMaterial>>,
    mut specular_events: MessageReader<AssetEvent<SpecularMaterial>>,
    mut grass_events: MessageReader<AssetEvent<GrassMaterial>>,
    mut materials: Local<ModifiedMaterials>,
    grass: Query<GrassQuery>,
    objects: Query<
        ObjectQuery,
        Or<(
            With<MeshMaterial3d<SourceMaterial>>,
            With<MeshMaterial3d<SpecularMaterial>>,
        )>,
    >,
) {
    lighting.dirty = 0..0;
    modified(&mut materials.standard, &mut standard_events);
    modified(&mut materials.specular, &mut specular_events);
    modified(&mut materials.grass, &mut grass_events);
    let (Some(master), Some(scene)) = (master, scene.0) else {
        lighting.stale = true;
        return;
    };
    let probes = Probes {
        center: scene.probe_center,
        extents: scene.probe_extents,
    };
    let lights = &master.uniform;
    let lighting = &mut *lighting;
    lighting.frame += 1;
    let frame = lighting.frame;
    let changes = lighting.light_changes(lights, &probes);
    // A value computed before this frame is current when no change came after it.
    let current_from = if std::mem::take(&mut lighting.stale) {
        frame
    } else {
        lighting.changed_at
    };
    if changes.any() {
        lighting.changed_at = frame;
    }
    let ObjectLighting {
        values,
        inputs,
        dirty,
        ..
    } = lighting;
    for (tag, renderer, transform, visibility, source, spec, skinned, morphed, fade) in &objects {
        let slot = tag.0 as usize;
        let input = &mut inputs[slot];
        let fade = fade.map_or(1., |f| f.0);
        let bounded = !skinned && !morphed;
        let changed = transform.is_changed()
            || renderer.is_changed()
            || fade != input.fade
            || bounded != input.bounded
            || source
                .as_ref()
                .is_some_and(|m| m.is_changed() || materials.standard.contains(&m.id()))
            || spec
                .as_ref()
                .is_some_and(|m| m.is_changed() || materials.specular.contains(&m.id()));
        if !visibility.get() {
            if changed {
                input.stamp = 0;
            }
            continue;
        }
        let mask = if changed || input.stamp == 0 || input.stamp < current_from {
            let receiver = match (&source, &spec) {
                (Some(handle), _) => standard.get(&handle.0).map(|m| m.extension.receiver_mask),
                (_, Some(handle)) => specular.get(&handle.0).map(SpecularMaterial::receiver_mask),
                _ => None,
            };
            let Some(receiver) = receiver else {
                input.stamp = 0;
                continue;
            };
            let record = &renderer.0;
            let bounds = WorldBounds::new(
                &transform.to_matrix(),
                Vec3::from_array(record.bounds_center),
                Vec3::from_array(record.bounds_extents),
                bounded,
            );
            let usage = record.reflection_probe_usage != 0;
            *input = SlotInputs {
                stamp: frame,
                receiver,
                bounds,
                usage,
                bounded,
                fade,
                flags: record.shader_flags(),
                probe: probe_blend(&probes, &bounds, usage),
            };
            light_mask(lights, |lamp| lamp_reaches(lamp, receiver, &bounds))
        } else if changes.any() {
            input.stamp = frame;
            if changes.probes {
                input.probe = probe_blend(&probes, &input.bounds, input.usage);
            }
            let (receiver, bounds) = (input.receiver, &input.bounds);
            refresh_mask(values[slot], changes.lamps, lights, |lamp| {
                lamp_reaches(lamp, receiver, bounds)
            })
        } else {
            continue;
        };
        let value = pack(mask, input.probe, input.flags, lod_code(input.fade));
        write(values, dirty, slot, value);
    }
    for (tag, transform, visibility, material, aabb) in &grass {
        let slot = tag.0 as usize;
        let input = &mut inputs[slot];
        let changed = transform.is_changed()
            || material.is_changed()
            || aabb.is_changed()
            || materials.grass.contains(&material.id());
        if !visibility.get() {
            if changed {
                input.stamp = 0;
            }
            continue;
        }
        let mask = if changed || input.stamp == 0 || input.stamp < current_from {
            let Some(material) = grass_materials.get(&material.0) else {
                input.stamp = 0;
                continue;
            };
            let receiver = material.receiver_mask;
            let bounds = WorldBounds::new(
                &transform.to_matrix(),
                Vec3::from(aabb.center),
                Vec3::from(aabb.half_extents),
                true,
            );
            *input = SlotInputs {
                stamp: frame,
                receiver,
                bounds,
                ..SlotInputs::STALE
            };
            light_mask(lights, |lamp| grass_lamp_reaches(lamp, receiver, &bounds))
        } else if changes.lamps != [0, 0] {
            input.stamp = frame;
            let (receiver, bounds) = (input.receiver, &input.bounds);
            refresh_mask(values[slot], changes.lamps, lights, |lamp| {
                grass_lamp_reaches(lamp, receiver, bounds)
            })
        } else {
            input.stamp = frame;
            continue;
        };
        write(values, dirty, slot, pack(mask, NO_PROBE, 0, lod_code(1.)));
    }
}

/// Writes `value` to `slot` and marks the slot for upload when the value is new.
fn write(
    values: &mut [ObjectValue],
    dirty: &mut std::ops::Range<usize>,
    slot: usize,
    value: ObjectValue,
) {
    if values[slot] != value {
        values[slot] = value;
        mark(dirty, slot);
    }
}

fn mark(dirty: &mut std::ops::Range<usize>, slot: usize) {
    *dirty = if dirty.start == dirty.end {
        slot..slot + 1
    } else {
        dirty.start.min(slot)..dirty.end.max(slot + 1)
    };
}

/// Pack a lamp mask, a probe blend, the renderer flags and the LOD code as `ObjectValue`.
pub fn pack(mask: [u32; 2], probe: ProbeBlend, flags: u32, lod: u32) -> ObjectValue {
    let second = probe.slots[1].unwrap_or(NO_SLOT);
    [
        mask[0],
        mask[1],
        probe.slots[0].unwrap_or(0) | second << 8 | flags << 16 | lod << 24,
        probe.weight.to_bits(),
    ]
}

fn upload_values(
    lighting: Extract<Res<ObjectLighting>>,
    mut gpu: ResMut<ObjectLightingGpu>,
    device: Res<RenderDevice>,
    queue: Res<RenderQueue>,
) {
    let values = &lighting.values;
    let count = values.len() as u64;
    // A new buffer is empty, so it takes every slot.
    let range = if count > gpu.capacity {
        gpu.capacity = count.next_power_of_two();
        gpu.buffer = buffer(&device, gpu.capacity);
        gpu.generation += 1;
        0..values.len()
    } else {
        lighting.dirty.clone()
    };
    if !range.is_empty() {
        queue.write_buffer(
            &gpu.buffer,
            range.start as u64 * VALUE_BYTES,
            bytemuck::cast_slice(&values[range]),
        );
    }
}

/// The LOD dither cells that `fade` keeps: the count of kept cells (0 to 16), and bit 5 when the fade is positive.
/// It uses the same f32 test as `source_lod_code` in source_lod.wgsl, so both keep the same pixels.
pub fn lod_code(fade: f32) -> u32 {
    let sign = if fade > 0. { 1. } else { -1. };
    let kept = (0..16u32)
        .filter(|level| fade - (level * 16) as f32 / 255. * sign >= 0.)
        .count() as u32;
    kept | u32::from(fade > 0.) << 5
}

#[derive(Clone, Copy, PartialEq)]
pub struct Probes {
    pub center: [Vec4; PROBE_COUNT],
    /// w=1 when the slot is enabled and published.
    pub extents: [Vec4; PROBE_COUNT],
}
/// Two probe slots with the largest box overlap, and the first weight.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ProbeBlend {
    pub weight: f32,
    pub slots: [Option<u32>; 2],
}
const NO_PROBE: ProbeBlend = ProbeBlend {
    weight: 0.,
    slots: [None, None],
};

/// It uses the same f32 operations and order as `probe_blend` in source_props_cull.wgsl.
/// Unity BlendProbes with equal importance. Renderer extents keep the native Vector3f::epsilon clamp.
fn probe_blend(probes: &Probes, bounds: &WorldBounds, usage: bool) -> ProbeBlend {
    if !usage {
        return NO_PROBE;
    }
    let center = bounds.center;
    let extents = bounds.extents.max(Vec3::splat(9.999999747378752e-06));
    let mut volumes = [0f32; 2];
    let mut slots = [None; 2];
    for slot in 0..PROBE_COUNT {
        let probe_extents = probes.extents[slot];
        if probe_extents.w == 0. {
            continue;
        }
        let probe_center = probes.center[slot].truncate();
        let probe_extents = probe_extents.truncate();
        let minimum = (center - extents).max(probe_center - probe_extents);
        let maximum = (center + extents).min(probe_center + probe_extents);
        let size = (maximum - minimum).max(Vec3::ZERO);
        let volume = size.x * size.y * size.z;
        if volume > volumes[0] {
            volumes = [volume, volumes[0]];
            slots = [Some(slot as u32), slots[0]];
        } else if volume > volumes[1] {
            volumes[1] = volume;
            slots[1] = Some(slot as u32);
        }
    }
    if slots[0].is_none() {
        return NO_PROBE;
    }
    let weight = if slots[1].is_none() {
        1.
    } else {
        volumes[0] / (volumes[0] + volumes[1])
    };
    ProbeBlend { weight, slots }
}

/// A bound can exclude a lamp only when all arithmetic stays finite.
fn finite(value: Vec3) -> bool {
    value.is_finite()
}
/// Lamps for which `reaches` is true, one bit per lamp slot.
fn light_mask(lighting: &LightingUniform, reaches: impl Fn(&GpuLight) -> bool) -> [u32; 2] {
    let mut selected = [0u32; 2];
    let count = (lighting.counts.x as usize).min(lighting.lights.len());
    for (i, lamp) in lighting.lights[..count].iter().enumerate() {
        if reaches(lamp) {
            selected[i >> 5] |= 1 << (i & 31);
        }
    }
    selected
}

/// `value` mask with the bits of the `changed` lamps tested again. Other bits stay.
fn refresh_mask(
    value: ObjectValue,
    changed: [u32; 2],
    lighting: &LightingUniform,
    reaches: impl Fn(&GpuLight) -> bool,
) -> [u32; 2] {
    let mut selected = [value[0], value[1]];
    let count = (lighting.counts.x as usize).min(lighting.lights.len());
    for (word, mut bits) in changed.into_iter().enumerate() {
        while bits != 0 {
            let bit = bits.trailing_zeros() as usize;
            bits &= bits - 1;
            let i = word * 32 + bit;
            if i < count && reaches(&lighting.lights[i]) {
                selected[word] |= 1 << bit;
            } else {
                selected[word] &= !(1 << bit);
            }
        }
    }
    selected
}

/// A lamp that the grass fragment loop of source_grass.wgsl can add to the bounds: a lamp of the
/// receiver mask, but not a point or spot lamp whose range ends before the bounds.
fn grass_lamp_reaches(lamp: &GpuLight, receiver: u32, bounds: &WorldBounds) -> bool {
    receiver & lamp.mask.x != 0
        && !(lamp.direction_kind.w != 0. && bounds.valid && bounds.beyond(lamp))
}

/// World AABB of local bounds, with the magnitudes of its terms for rounding padding.
#[derive(Clone, Copy)]
struct WorldBounds {
    center: Vec3,
    extents: Vec3,
    magnitude: Vec3,
    /// A bound can exclude a lamp only when all arithmetic stays finite.
    valid: bool,
}
impl WorldBounds {
    const EMPTY: Self = Self {
        center: Vec3::ZERO,
        extents: Vec3::ZERO,
        magnitude: Vec3::ZERO,
        valid: false,
    };
    fn new(model: &Mat4, local_center: Vec3, local_extents: Vec3, bounded: bool) -> Self {
        let axes = [
            model.x_axis.truncate().abs(),
            model.y_axis.truncate().abs(),
            model.z_axis.truncate().abs(),
        ];
        let center = model.mul_vec4(local_center.extend(1.)).truncate();
        let extents =
            axes[0] * local_extents.x + axes[1] * local_extents.y + axes[2] * local_extents.z;
        // Absolute terms also cover cancellation in the center transform.
        let magnitude = axes[0] * local_center.x.abs()
            + axes[1] * local_center.y.abs()
            + axes[2] * local_center.z.abs()
            + model.w_axis.truncate().abs()
            + extents;
        let valid = bounded
            && local_extents.cmpge(Vec3::ZERO).all()
            && finite(center)
            && finite(extents)
            && finite(magnitude);
        Self {
            center,
            extents,
            magnitude,
            valid,
        }
    }
    /// The range of a point or spot lamp ends before the bounds.
    fn beyond(&self, lamp: &GpuLight) -> bool {
        let position = lamp.position_range.truncate();
        let range = lamp.position_range.w.abs();
        // 128 f32 epsilons cover transform, subtraction, and squared-distance rounding.
        // Expand rather than round toward exclusion. Boundary contact keeps the lamp.
        let padding =
            (self.magnitude + position.abs() + Vec3::splat(range + 1.)) * 0.0000152587890625;
        let separation = ((position - self.center).abs() - self.extents - padding).max(Vec3::ZERO);
        let distance2 = separation.dot(separation);
        let range2 = range * range;
        finite(padding) && finite(Vec3::new(distance2, range2, range)) && distance2 > range2
    }
}

/// A lamp that can reach the object bounds. It uses the same f32 operations and order as
/// `object_light_mask` in source_props_cull.wgsl.
fn lamp_reaches(lamp: &GpuLight, receiver: u32, bounds: &WorldBounds) -> bool {
    if lamp.direction_kind.w == 0. || receiver & lamp.mask.x == 0 {
        return false;
    }
    if lamp.color.x.max(lamp.color.y.max(lamp.color.z)) <= 0. {
        return false;
    }
    !bounds.valid || !bounds.beyond(lamp)
}
