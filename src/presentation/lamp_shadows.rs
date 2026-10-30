//! "Lamp shadows" display setting and the shadowed lamp slots of each frame.
//! Point and spot lights with a source shadow mode draw their casters into the lamp atlas.
use super::caster::CasterUniform;
use super::lighting::{SourceLighting, record_of};
use super::surface_views::{WORLD_VIEWS, player_dist};
use super::view_frame::SourceViewFrame;
use crate::app::RuntimeReference;
use crate::reference::LightKind;
use crate::reference::lamp_shadows::{LAMP_BIT, LAMP_SLOTS, LampSphere, assign_slots};
use bevy::render::extract_resource::ExtractResource;
use bevy::render::render_resource::{ShaderType, encase};
use bevy::render::renderer::{RenderDevice, RenderQueue};
use bevy::{ecs as bevy_ecs, prelude::*, render as bevy_render};

/// Texels for a source bias of 1. The sun cascades scale their normal bias the same way.
/// The value for lamps is an estimate: check acne and gaps in the final test pass.
const BIAS_TEXELS: f32 = 3.65;
/// Source quality "High": point light faces of 512, spot lights of 1024.
pub const FACE: u32 = 512;
/// Four slot regions of 3 x 2 faces, in 2 x 2.
pub const ATLAS: UVec2 = UVec2::new(6 * FACE, 4 * FACE);
/// Caster slots: 4 sun cascades, then 6 faces for each lamp slot.
pub const TILES: usize = 4 + LAMP_SLOTS * 6;

/// The caster mask bit of a caster slot.
pub fn tile_bit(tile: usize) -> usize {
    if tile < 4 {
        tile
    } else {
        LAMP_BIT as usize + (tile - 4) / 6
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, ShaderType)]
pub struct LampShadowUniform {
    pub faces: [Mat4; 6],
    /// xy: atlas UV of face 0, zw: face UV size.
    pub rect: Vec4,
    /// x: strength, y: 0 none, 1 hard, 2 soft, z: normal bias, w: depth bias. Biases are in texels.
    pub params: Vec4,
    /// x: tangent of the half face FOV, y: face texels, z: 1 for a point light.
    pub shape: Vec4,
}
impl LampShadowUniform {
    pub fn point(&self) -> bool {
        self.shape.z != 0.
    }
    /// Atlas viewport of one face in texels: x, y, width, height.
    pub fn viewport(slot: usize, face: usize, point: bool) -> [u32; 4] {
        let x = (slot as u32 % 2) * 3 * FACE;
        let y = (slot as u32 / 2) * 2 * FACE;
        if point {
            let face = face as u32;
            [x + (face % 3) * FACE, y + (face / 3) * FACE, FACE, FACE]
        } else {
            [x, y, 2 * FACE, 2 * FACE]
        }
    }
}

/// Main world state, extracted each frame.
#[derive(Resource, Clone, ExtractResource)]
pub struct LampShadows {
    pub on: bool,
    pub spheres: [Option<LampSphere>; LAMP_SLOTS],
    pub uniforms: [LampShadowUniform; LAMP_SLOTS],
    /// Light view matrices of each face: view_from_world and clip_from_view.
    pub views: [[(Mat4, Mat4); 6]; LAMP_SLOTS],
}
impl Default for LampShadows {
    fn default() -> Self {
        Self {
            on: true,
            spheres: [None; LAMP_SLOTS],
            uniforms: [LampShadowUniform::default(); LAMP_SLOTS],
            views: [[(Mat4::IDENTITY, Mat4::IDENTITY); 6]; LAMP_SLOTS],
        }
    }
}
impl LampShadows {
    /// Caster slot uniform of one lamp face. Caster bias is 0: the receiver applies it.
    pub fn caster(&self, slot: usize, face: usize, player_dist: f32) -> CasterUniform {
        let (view_from_world, clip_from_view) = self.views[slot][face];
        CasterUniform {
            clip_from_world: self.uniforms[slot].faces[face],
            direction_to_light: Vec4::Y,
            bias: Vec4::new(0., 0., 0., player_dist),
            view_from_world,
            world_from_view: view_from_world.inverse(),
            clip_from_view,
        }
    }
    /// Faces to draw in a slot. 0 when the slot is empty.
    pub fn faces(&self, slot: usize) -> usize {
        match self.uniforms[slot].params.y {
            0. => 0,
            _ if self.uniforms[slot].point() => 6,
            _ => 1,
        }
    }
}

/// Read the setting that the browser sends at start and after each change.
pub(crate) fn poll(mut lamps: ResMut<LampShadows>) {
    if let Some(on) = crate::browser::game_lamp_shadows() {
        lamps.on = on;
    }
}

/// Reversed-depth perspective with a Y flip. The caster pipelines cull clockwise faces, like the sun cascades.
fn face_projection(fov: f32, near: f32, far: f32) -> Mat4 {
    Mat4::from_scale(Vec3::new(1., -1., 1.)) * Mat4::perspective_rh(fov, 1., far, near)
}

/// Point light faces in the order of `lamp_shadow` in source_view_lighting.wgsl: +X, -X, +Y, -Y, +Z, -Z.
const POINT_FACES: [(Vec3, Vec3); 6] = [
    (Vec3::X, Vec3::Y),
    (Vec3::NEG_X, Vec3::Y),
    (Vec3::Y, Vec3::Z),
    (Vec3::NEG_Y, Vec3::Z),
    (Vec3::Z, Vec3::Y),
    (Vec3::NEG_Z, Vec3::Y),
];

/// After the lamp uniform and the view snapshot, before the caster masks.
pub(crate) fn select(
    reference: Res<RuntimeReference>,
    frame: Res<SourceViewFrame>,
    mut source: ResMut<SourceLighting>,
    mut lamps: ResMut<LampShadows>,
    mut candidates: Local<Vec<LampSphere>>,
) {
    let lamps = &mut *lamps;
    let source = &mut *source;
    let quality = reference.0.lighting.quality.shadows;
    candidates.clear();
    for bound in &source.lights {
        let gpu = &mut source.uniform.lights[bound.index];
        gpu.mask.z = 0;
        let record = record_of(&reference, bound);
        if !lamps.on
            || quality == 0
            || record.kind == LightKind::Directional
            || record.shadows.kind == 0
            || gpu.color.truncate().max_element() <= 0.
        {
            continue;
        }
        let p = gpu.position_range;
        candidates.push(LampSphere {
            light: bound.index,
            center: [p.x, p.y, -p.z],
            range: record.range_meters,
            culling_mask: record.culling_mask,
        });
    }
    match frame.views[0] {
        Some(view) => assign_slots(
            &candidates,
            view.source_world_from_view.w_axis.truncate().to_array(),
            &view.source_camera_planes,
            view.shadow_distance,
            &mut lamps.spheres,
        ),
        None => lamps.spheres = [None; LAMP_SLOTS],
    }
    let texel = ATLAS.as_vec2().recip();
    for slot in 0..LAMP_SLOTS {
        lamps.uniforms[slot] = LampShadowUniform::default();
        let Some(sphere) = lamps.spheres[slot] else {
            continue;
        };
        let Some(bound) = source.lights.iter().find(|l| l.index == sphere.light) else {
            continue;
        };
        let record = record_of(&reference, bound);
        let gpu = &mut source.uniform.lights[sphere.light];
        gpu.mask.z = slot as u32 + 1;
        let position = gpu.position_range.truncate();
        let point = record.kind == LightKind::Point;
        let range = record.range_meters;
        let near = record.shadows.near_plane.max(0.05).min(range * 0.5);
        let (texels, fov) = if point {
            // Two spare texels on each side keep the PCF taps of the edge inside the face.
            let texels = FACE as f32;
            (texels, 2. * (texels / (texels - 4.)).atan())
        } else {
            (
                2. * FACE as f32,
                record.full_cone_degrees.min(179.).to_radians(),
            )
        };
        let projection = face_projection(fov, near, range);
        let uniform = &mut lamps.uniforms[slot];
        for face in 0..if point { 6 } else { 1 } {
            let (forward, up) = if point {
                POINT_FACES[face]
            } else {
                let forward = gpu.direction_kind.truncate();
                (
                    forward,
                    if forward.y.abs() < 0.99 {
                        Vec3::Y
                    } else {
                        Vec3::X
                    },
                )
            };
            let view = Mat4::look_to_rh(position, forward, up);
            uniform.faces[face] = projection * view;
            lamps.views[slot][face] = (view, projection);
        }
        let [x, y, w, h] = LampShadowUniform::viewport(slot, 0, point);
        uniform.rect = Vec4::new(
            x as f32 * texel.x,
            y as f32 * texel.y,
            w as f32 * texel.x,
            h as f32 * texel.y,
        );
        // Source quality 1 is "Hard shadows only".
        let soft = record.shadows.kind == 2 && quality == 2;
        uniform.params = Vec4::new(
            record.shadows.strength,
            if soft { 2. } else { 1. },
            record.shadows.normal_bias * BIAS_TEXELS,
            record.shadows.bias * BIAS_TEXELS,
        );
        uniform.shape = Vec4::new((fov * 0.5).tan(), texels, f32::from(u8::from(point)), 0.);
    }
}

/// Lamp face caster slots of the world view.
pub(crate) fn write_casters(
    lamps: Res<LampShadows>,
    mut views: Query<(
        &super::surface_views::SourceSurfaceView,
        &mut super::caster::CasterViews,
    )>,
    device: Res<RenderDevice>,
    queue: Res<RenderQueue>,
) {
    for (view, mut slots) in &mut views {
        if view.index >= WORLD_VIEWS {
            continue;
        }
        for slot in 0..LAMP_SLOTS {
            for face in 0..lamps.faces(slot) {
                let value = lamps.caster(slot, face, player_dist(view.index));
                slots.write(4 + slot * 6 + face, value, &device, &queue);
            }
        }
    }
}

pub fn install(app: &mut App) {
    use bevy::camera::visibility::VisibilitySystems;
    use bevy::render::{Render, RenderApp, RenderSystems};
    app.sub_app_mut(RenderApp).add_systems(
        Render,
        write_casters
            .in_set(RenderSystems::PrepareResources)
            .after(super::caster::prepare_views),
    );
    app.init_resource::<LampShadows>()
        .add_plugins(bevy::render::extract_resource::ExtractResourcePlugin::<
            LampShadows,
        >::default())
        .add_systems(Update, poll)
        .add_systems(
            PostUpdate,
            select
                .after(super::lighting::sync_lighting)
                .after(VisibilitySystems::CheckVisibility)
                .before(super::shadow_scene::prepare_scene),
        );
}
