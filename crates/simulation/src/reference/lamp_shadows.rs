//! Lamp shadow slots: the point and spot lights that cast shadows this frame, and their casters.
//! Positions, bounds and planes use the source frame.
use super::shadow_candidates::{Bounds, Plane};
use super::shadow_frame::CasterInput;

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

/// Replace bits 4..7 of each mask. Returns true when a mask changes.
pub fn write_lamp_bits(
    casters: &[CasterInput],
    slots: &[Option<LampSphere>; LAMP_SLOTS],
    masks: &mut [u8],
) -> bool {
    let mut changed = false;
    for (caster, mask) in casters.iter().zip(masks.iter_mut()) {
        let mut bits = 0u8;
        for (slot, lamp) in slots.iter().enumerate() {
            if lamp.as_ref().is_some_and(|lamp| lamp_caster(caster, lamp)) {
                bits |= 1 << (LAMP_BIT as usize + slot);
            }
        }
        let next = (*mask & 0x0f) | bits;
        changed |= next != *mask;
        *mask = next;
    }
    changed
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
        let mut masks = [0b0001, 0, 0b1111_0000];
        let slots = [None, Some(lamp(0, 0.)), None, None];
        assert!(write_lamp_bits(&casters, &slots, &mut masks));
        assert_eq!(masks, [0b0010_0001, 0, 0]);
        assert!(!write_lamp_bits(&casters, &slots, &mut masks));
    }
}
