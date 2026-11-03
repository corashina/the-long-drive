//! Lamp shadow slots: the point and spot lights that cast shadows this frame, and their casters.
//! Positions, bounds and planes use the source frame.
use super::shadow_candidates::{Bounds, Plane};
use super::shadow_frame::CasterInput;
use super::shadow_grid::CasterGrid;

/// Shadowed lamps at one time. Caster mask bits 4..7 hold the slots.
pub const LAMP_SLOTS: usize = 4;
/// The first caster mask bit of the lamp slots. Bits 0..3 are the sun cascades.
pub const LAMP_BIT: u32 = 4;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LampSphere {
    /// Index of the light in the lamp uniform.
    pub light: usize,
    pub center: [f32; 3],
    pub range: f32,
    pub culling_mask: u32,
}

fn sphere_touches_planes(center: [f32; 3], radius: f32, planes: &[Plane; 6]) -> bool {
    planes.iter().all(|p| {
        let distance = (p[0] * center[0] + p[1] * center[1]) + p[2] * center[2] + p[3];
        distance + radius >= 0.
    })
}
fn distance2(a: [f32; 3], b: [f32; 3]) -> f32 {
    (0..3).map(|i| (a[i] - b[i]) * (a[i] - b[i])).sum()
}

/// Keep the slots of lamps that stay selected. Only the nearest lamps in the view and the shadow distance get a slot.
/// A lamp keeps its slot while it stays selected, so caster masks do not change when two lamps swap order.
pub fn assign_slots(
    candidates: &[LampSphere],
    camera: [f32; 3],
    planes: &[Plane; 6],
    shadow_distance: f32,
    slots: &mut [Option<LampSphere>; LAMP_SLOTS],
) {
    let mut nearest: [Option<(f32, LampSphere)>; LAMP_SLOTS] = [None; LAMP_SLOTS];
    for lamp in candidates {
        let near = shadow_distance + lamp.range;
        let d2 = distance2(lamp.center, camera);
        if d2 > near * near || !sphere_touches_planes(lamp.center, lamp.range, planes) {
            continue;
        }
        // Insert into the sorted fixed list.
        let mut item = Some((d2, *lamp));
        for entry in &mut nearest {
            if entry.is_none_or(|(d, _)| item.is_some_and(|(n, _)| n < d)) {
                std::mem::swap(entry, &mut item);
            }
        }
    }
    let chosen = |light: usize| nearest.iter().flatten().any(|(_, l)| l.light == light);
    for slot in slots.iter_mut() {
        if slot.is_some_and(|s| !chosen(s.light)) {
            *slot = None;
        }
    }
    for (_, lamp) in nearest.into_iter().flatten() {
        if let Some(slot) = slots
            .iter_mut()
            .find(|s| s.is_some_and(|s| s.light == lamp.light))
        {
            *slot = Some(lamp);
        } else if let Some(slot) = slots.iter_mut().find(|s| s.is_none()) {
            *slot = Some(lamp);
        }
    }
}

/// A caster draws in a lamp slot when the lamp sphere touches its bounds.
pub fn lamp_caster(caster: &CasterInput, lamp: &LampSphere) -> bool {
    let s = &caster.state;
    let Bounds { center, extents } = caster.bounds;
    let gap2: f32 = (0..3)
        .map(|i| {
            let gap = ((lamp.center[i] - center[i]).abs() - extents[i]).max(0.);
            gap * gap
        })
        .sum();
    caster.material_has_caster
        && s.shadow_casting_mode != 0
        && s.layer < 32
        && lamp.culling_mask & (1u32 << s.layer) != 0
        && s.renderer_present
        && !s.disabled
        && !s.force_off
        && (s.lod_group == 0 || s.active_lod & s.lod_index_mask != 0)
        && gap2 <= lamp.range * lamp.range
}

/// Lamp bits of the world view masks. It keeps the rows of the last write, so a write
/// touches only old and new caster rows.
#[derive(Default)]
pub struct LampMasks {
    /// Rows and their lamp bits, in row order.
    rows: Vec<(u32, u8)>,
    /// Retained storage.
    next: Vec<(u32, u8)>,
}
impl LampMasks {
    /// Call when the mask storage is cleared or its rows change order.
    pub fn clear(&mut self) {
        self.rows.clear();
    }
    /// Rows with lamp bits, in row order.
    pub fn rows(&self) -> impl Iterator<Item = u32> + '_ {
        self.rows.iter().map(|(row, _)| *row)
    }
    /// Rows and their lamp bits, in row order.
    pub fn entries(&self) -> &[(u32, u8)] {
        &self.rows
    }
    /// Replace bits 4..7 of each mask. Returns the lamp bits that changed for any caster.
    pub fn write(
        &mut self,
        casters: &[CasterInput],
        grid: &CasterGrid,
        slots: &[Option<LampSphere>; LAMP_SLOTS],
        masks: &mut [u8],
    ) -> u8 {
        self.next.clear();
        for (slot, lamp) in slots.iter().enumerate() {
            let Some(lamp) = lamp else { continue };
            let bit = 1 << (LAMP_BIT as usize + slot);
            grid.visit_sphere(lamp.center, lamp.range, |row| {
                if lamp_caster(&casters[row as usize], lamp) {
                    self.next.push((row, bit));
                }
            });
        }
        self.next.sort_unstable();
        self.next.dedup_by(|row, kept| {
            let same = row.0 == kept.0;
            if same {
                kept.1 |= row.1;
            }
            same
        });
        for &(row, _) in &self.rows {
            masks[row as usize] &= 0x0f;
        }
        for &(row, bits) in &self.next {
            masks[row as usize] |= bits;
        }
        // Both lists are in row order. A row in one list only has zero lamp bits in the other.
        let (old, new) = (&self.rows, &self.next);
        let (mut a, mut b, mut changed) = (0, 0, 0);
        while a < old.len() || b < new.len() {
            match (old.get(a), new.get(b)) {
                (Some(&(row, bits)), Some(&(next, next_bits))) if row == next => {
                    changed |= bits ^ next_bits;
                    a += 1;
                    b += 1;
                }
                (Some(&(row, bits)), next) if next.is_none_or(|&(next, _)| row < next) => {
                    changed |= bits;
                    a += 1;
                }
                (_, Some(&(_, bits))) => {
                    changed |= bits;
                    b += 1;
                }
                (_, None) => unreachable!(),
            }
        }
        std::mem::swap(&mut self.rows, &mut self.next);
        changed
    }
}

/// Lamp face tiles: 6 for each lamp slot.
pub const LAMP_FACES: usize = LAMP_SLOTS * 6;

/// What the static tile of a lamp face holds.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LampFace {
    /// Face projection, as bits.
    pub clip_from_world: [u32; 16],
    pub point: bool,
    /// Static caster content of the face. It changes when the face content would change.
    pub contents: u64,
}

/// Four side planes and the plane through the light of a face, from its clip matrix (columns, source frame).
/// The planes use x, y and w only, so the depth convention does not matter.
pub fn face_planes(clip: [[f32; 4]; 4]) -> [Plane; 5] {
    let row = |r: usize| [clip[0][r], clip[1][r], clip[2][r], clip[3][r]];
    let (x, y, w) = (row(0), row(1), row(3));
    let add = |a: [f32; 4], b: [f32; 4], s: f32| std::array::from_fn(|i| a[i] + s * b[i]);
    // w > 0 is the half space in front of the light.
    [
        add(w, x, 1.),
        add(w, x, -1.),
        add(w, y, 1.),
        add(w, y, -1.),
        w,
    ]
}

/// The bounds touch all planes.
fn touches(bounds: &Bounds, planes: &[Plane; 5]) -> bool {
    planes.iter().all(|p| {
        let d = p[0] * bounds.center[0] + p[1] * bounds.center[1] + p[2] * bounds.center[2] + p[3];
        let r = p[0].abs() * bounds.extents[0]
            + p[1].abs() * bounds.extents[1]
            + p[2].abs() * bounds.extents[2];
        d + r >= 0.
    })
}

/// Spread the bits of a pose hash, so a sum of poses does not cancel.
fn mix(mut v: u64) -> u64 {
    v ^= v >> 33;
    v = v.wrapping_mul(0xff51_afd7_ed55_8ccd);
    v ^= v >> 33;
    v = v.wrapping_mul(0xc4ce_b9fe_1a85_ec53);
    v ^ v >> 33
}

/// Static caster content and animated casters of each lamp face tile.
pub struct FaceScan {
    pub contents: [u64; LAMP_FACES],
    pub animated: [bool; LAMP_FACES],
}
/// `poses` holds a hash of the identity and pose of each caster row. `animated` is in row order.
/// The content is a sum, so it does not depend on row order.
pub fn scan_faces(
    masks: &LampMasks,
    casters: &[CasterInput],
    poses: &[u64],
    animated: &[u32],
    faces: &[[Option<[Plane; 5]>; 6]; LAMP_SLOTS],
) -> FaceScan {
    let mut scan = FaceScan {
        contents: [0; LAMP_FACES],
        animated: [false; LAMP_FACES],
    };
    for &(row, bits) in masks.entries() {
        let bounds = &casters[row as usize].bounds;
        let moving = animated.binary_search(&row).is_ok();
        for (slot, planes) in faces.iter().enumerate() {
            if bits >> (LAMP_BIT as usize + slot) & 1 == 0 {
                continue;
            }
            for (face, planes) in planes.iter().enumerate() {
                let Some(planes) = planes else { continue };
                if !touches(bounds, planes) {
                    continue;
                }
                let tile = slot * 6 + face;
                if moving {
                    scan.animated[tile] = true;
                } else {
                    scan.contents[tile] =
                        scan.contents[tile].wrapping_add(mix(poses[row as usize]));
                }
            }
        }
    }
    scan
}

/// `draw`: static casters draw into the static tile. `refresh`: the atlas tile copies the
/// static tile, then the animated casters draw.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LampPlan {
    pub draw: [bool; LAMP_FACES],
    pub refresh: [bool; LAMP_FACES],
}

/// The face of each lamp tile at its last draw, and its animated state at its last refresh.
#[derive(Clone, Copy, Debug, Default)]
pub struct LampHistory {
    faces: [Option<LampFace>; LAMP_FACES],
    animated: [bool; LAMP_FACES],
}
impl LampHistory {
    /// A face draws when the atlas is forced or its tile holds a different face.
    /// It refreshes when it draws, or it holds animated casters now or at its last refresh.
    /// An unused tile forgets its face, because a spot face can cover it.
    pub fn plan(
        &mut self,
        forced: bool,
        current: &[Option<LampFace>; LAMP_FACES],
        animated: &[bool; LAMP_FACES],
    ) -> LampPlan {
        let mut plan = LampPlan {
            draw: [false; LAMP_FACES],
            refresh: [false; LAMP_FACES],
        };
        for tile in 0..LAMP_FACES {
            let Some(face) = current[tile] else {
                self.faces[tile] = None;
                self.animated[tile] = false;
                continue;
            };
            let draw = forced || self.faces[tile] != Some(face);
            let refresh = draw || animated[tile] || self.animated[tile];
            if draw {
                self.faces[tile] = Some(face);
            }
            if refresh {
                self.animated[tile] = animated[tile];
            }
            plan.draw[tile] = draw;
            plan.refresh[tile] = refresh;
        }
        plan
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::reference::shadow_candidates::CandidateState;
    use glam::{Mat4, Vec3};

    fn caster(center: [f32; 3]) -> CasterInput {
        CasterInput {
            bounds: Bounds {
                center,
                extents: [0.2; 3],
            },
            state: CandidateState {
                shadow_casting_mode: 1,
                layer: 0,
                renderer_present: true,
                disabled: false,
                force_off: false,
                lod_group: 0,
                lod_index_mask: 0,
                active_lod: 0,
            },
            material_has_caster: true,
        }
    }

    /// A face is dirty only when a static caster inside it moves. Animated casters only refresh it.
    #[test]
    fn only_the_face_with_a_moved_caster_is_dirty() {
        let lamp = LampSphere {
            light: 0,
            center: [0.; 3],
            range: 10.,
            culling_mask: u32::MAX,
        };
        let slots = [Some(lamp), None, None, None];
        let projection = Mat4::perspective_rh(1.6, 1., 10., 0.05);
        let directions = [
            (Vec3::X, Vec3::Y),
            (Vec3::NEG_X, Vec3::Y),
            (Vec3::Y, Vec3::Z),
            (Vec3::NEG_Y, Vec3::Z),
            (Vec3::Z, Vec3::Y),
            (Vec3::NEG_Z, Vec3::Y),
        ];
        let mut faces = [[None; 6]; LAMP_SLOTS];
        let mut clips = [[0; 16]; 6];
        for (face, (forward, up)) in directions.into_iter().enumerate() {
            let clip = projection * Mat4::look_to_rh(Vec3::ZERO, forward, up);
            faces[0][face] = Some(face_planes(clip.to_cols_array_2d()));
            clips[face] = clip.to_cols_array().map(f32::to_bits);
        }
        // Row 0: static caster on +X. Row 1: animated caster on -Y.
        let mut casters = vec![caster([3., 0., 0.]), caster([0., -3., 0.])];
        let mut poses = vec![1, 2];
        let animated = [1];
        let mut grid = CasterGrid::default();
        let mut masks = vec![0; 2];
        let mut lamp_masks = LampMasks::default();
        let mut history = LampHistory::default();
        let mut frame = |casters: &[CasterInput], poses: &[u64], history: &mut LampHistory| {
            grid.rebuild(casters);
            lamp_masks.write(casters, &grid, &slots, &mut masks);
            let scan = scan_faces(&lamp_masks, casters, poses, &animated, &faces);
            let current = std::array::from_fn(|tile| {
                (tile < 6).then(|| LampFace {
                    clip_from_world: clips[tile],
                    point: true,
                    contents: scan.contents[tile],
                })
            });
            history.plan(false, &current, &scan.animated)
        };
        let first = frame(&casters, &poses, &mut history);
        assert!(first.draw[..6].iter().all(|d| *d));
        let still = frame(&casters, &poses, &mut history);
        assert_eq!(still.draw, [false; LAMP_FACES]);
        // -Y holds the animated caster, so only it refreshes.
        assert!((0..LAMP_FACES).all(|tile| still.refresh[tile] == (tile == 3)));
        casters[0].bounds.center = [3., 0.5, 0.];
        poses[0] = 3;
        let moved = frame(&casters, &poses, &mut history);
        assert!((0..LAMP_FACES).all(|tile| moved.draw[tile] == (tile == 0)));
    }
}
