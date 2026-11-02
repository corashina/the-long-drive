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
        // Insertion into the sorted fixed list.
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

/// What a lamp face tile holds.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LampFace {
    /// Face projection, as bits.
    pub clip_from_world: [u32; 16],
    pub point: bool,
    /// Caster content counter of the slot. It changes when the face content would change.
    pub contents: u64,
}

/// The face of each lamp tile at its last draw.
#[derive(Clone, Copy, Debug, Default)]
pub struct LampHistory([Option<LampFace>; LAMP_FACES]);
impl LampHistory {
    /// A face draws when the atlas is forced or its tile holds a different face. The tiles keep the drawn faces.
    /// An unused tile forgets its face: a spot face can cover it.
    pub fn plan(
        &mut self,
        forced: bool,
        current: &[Option<LampFace>; LAMP_FACES],
    ) -> [bool; LAMP_FACES] {
        std::array::from_fn(|tile| {
            let draw = current[tile].is_some_and(|face| forced || self.0[tile] != Some(face));
            if draw || current[tile].is_none() {
                self.0[tile] = current[tile];
            }
            draw
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::reference::shadow_candidates::CandidateState;

    const OPEN: [Plane; 6] = [[0., 0., 0., 1.]; 6];
    fn lamp(light: usize, x: f32) -> LampSphere {
        LampSphere {
            light,
            center: [x, 0., 0.],
            range: 1.,
            culling_mask: 1,
        }
    }

    #[test]
    fn slots_keep_lamps_and_take_the_nearest() {
        let mut slots = [None; LAMP_SLOTS];
        let lamps: Vec<_> = (0..6).map(|i| lamp(i, i as f32 * 10.)).collect();
        assign_slots(&lamps, [0.; 3], &OPEN, 100., &mut slots);
        let lights = |s: &[Option<LampSphere>; 4]| s.map(|l| l.map(|l| l.light));
        assert_eq!(lights(&slots), [Some(0), Some(1), Some(2), Some(3)]);
        // The camera moves: lamp 0 drops out, lamp 4 takes its free slot. The others keep theirs.
        assign_slots(&lamps, [30., 0., 0.], &OPEN, 100., &mut slots);
        assert_eq!(lights(&slots), [Some(4), Some(1), Some(2), Some(3)]);
        // Lamps past the shadow distance get no slot.
        assign_slots(&lamps, [0.; 3], &OPEN, 5., &mut slots);
        assert_eq!(lights(&slots), [Some(0), None, None, None]);
    }

    /// Only the 4 nearest lamps cast. Two chosen lamps that swap order keep their slots and caster bits.
    #[test]
    fn tp_ar9_only_the_4_nearest_lamps_cast_and_a_swap_does_not_pop() {
        let mut slots = [None; LAMP_SLOTS];
        let lights = |s: &[Option<LampSphere>; 4]| s.map(|l| l.map(|l| l.light));
        let mut lamps: Vec<_> = (0..6).map(|i| lamp(i, 2. + i as f32 * 3.)).collect();
        assign_slots(&lamps, [0.; 3], &OPEN, 100., &mut slots);
        assert_eq!(lights(&slots), [Some(0), Some(1), Some(2), Some(3)]);
        let caster = |x: f32| CasterInput {
            bounds: Bounds {
                center: [x, 0., 0.],
                extents: [0.5; 3],
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
        };
        // One caster at each lamp. Each caster takes the bit of its lamp's slot.
        let casters: Vec<_> = [2., 5., 8., 11.].into_iter().map(caster).collect();
        let mut grid = CasterGrid::default();
        grid.rebuild(&casters);
        let (mut masks, mut bits) = (LampMasks::default(), [0u8; 4]);
        masks.write(&casters, &grid, &slots, &mut bits);
        let first = bits;
        // Lamps 1 and 2 swap places, and their casters too: slots and caster bits stay.
        lamps[1].center[0] = 8.;
        lamps[2].center[0] = 5.;
        assign_slots(&lamps, [0.; 3], &OPEN, 100., &mut slots);
        assert_eq!(lights(&slots), [Some(0), Some(1), Some(2), Some(3)]);
        let moved: Vec<_> = [2., 8., 5., 11.].into_iter().map(caster).collect();
        grid.rebuild(&moved);
        masks.write(&moved, &grid, &slots, &mut bits);
        assert_eq!(bits, first);
        // A fifth lamp nearer than lamp 3 takes lamp 3's slot only.
        lamps[5].center[0] = 1.;
        assign_slots(&lamps, [0.; 3], &OPEN, 100., &mut slots);
        assert_eq!(lights(&slots), [Some(0), Some(1), Some(2), Some(5)]);
    }

    #[test]
    fn casters_need_the_sphere_and_the_layer() {
        let caster = |x: f32, layer: u8| CasterInput {
            bounds: Bounds {
                center: [x, 0., 0.],
                extents: [0.5; 3],
            },
            state: CandidateState {
                shadow_casting_mode: 1,
                layer,
                renderer_present: true,
                disabled: false,
                force_off: false,
                lod_group: 0,
                lod_index_mask: 0,
                active_lod: 0,
            },
            material_has_caster: true,
        };
        let casters = [caster(1.4, 0), caster(1.6, 0), caster(0., 1)];
        let mut grid = CasterGrid::default();
        grid.rebuild(&casters);
        let mut lamps = LampMasks::default();
        let mut masks = [0b0001, 0, 0];
        let slots = [None, Some(lamp(0, 0.)), None, None];
        assert_eq!(
            lamps.write(&casters, &grid, &slots, &mut masks),
            0b0010_0000
        );
        assert_eq!(masks, [0b0010_0001, 0, 0]);
        assert_eq!(lamps.write(&casters, &grid, &slots, &mut masks), 0);
        let none = [None; LAMP_SLOTS];
        assert_eq!(lamps.write(&casters, &grid, &none, &mut masks), 0b0010_0000);
        assert_eq!(masks, [0b0001, 0, 0]);
    }

    #[test]
    fn unchanged_faces_skip_the_draw() {
        let face = |contents| {
            Some(LampFace {
                clip_from_world: [0; 16],
                point: true,
                contents,
            })
        };
        let mut history = LampHistory::default();
        let mut current = [None; LAMP_FACES];
        current[0] = face(0);
        current[1] = face(0);
        assert_eq!(history.plan(false, &current)[..3], [true, true, false]);
        assert_eq!(history.plan(false, &current), [false; LAMP_FACES]);
        // A content change draws the face. A forced plan draws all used faces.
        current[1] = face(1);
        assert_eq!(history.plan(false, &current)[..3], [false, true, false]);
        assert_eq!(history.plan(true, &current)[..3], [true, true, false]);
        // An unused tile forgets its face.
        current[1] = None;
        history.plan(false, &current);
        current[1] = face(1);
        assert_eq!(history.plan(false, &current)[..3], [false, true, false]);
    }
}
