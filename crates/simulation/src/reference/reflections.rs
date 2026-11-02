//! Native IndividualFaces order. A job advances only after its GPU work is encoded.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProbeJob {
    Face(u8),
    FirstMip,
    RemainingMipsAndPublish,
}
/// Warmup prepares every face. Runtime prepares only the current face job.
pub fn prepare_probe_face(job: Option<ProbeJob>, face: usize, warmup: bool) -> bool {
    face < 6 && (warmup || job == Some(ProbeJob::Face(face as u8)))
}
#[derive(Default)]
pub struct ProbeSchedule {
    next: u8,
    published: bool,
}
impl ProbeSchedule {
    pub fn job(&self) -> ProbeJob {
        match self.next {
            0..=5 => ProbeJob::Face(self.next),
            6 => ProbeJob::FirstMip,
            7 => ProbeJob::RemainingMipsAndPublish,
            _ => unreachable!(),
        }
    }
    pub fn published(&self) -> bool {
        self.published
    }
    /// An enabled probe captures again after each publication.
    /// Finish an active capture even when distance checks disable its probe.
    pub fn pending_job(&self, enabled: bool) -> Option<ProbeJob> {
        let job = self.job();
        (enabled || job != ProbeJob::Face(0)).then_some(job)
    }
    /// Call after encoding this frame's job. Pipeline waits must leave the job unchanged.
    pub fn complete(&mut self, job: ProbeJob) -> Result<(), &'static str> {
        if job != self.job() {
            return Err("Source probe job completed out of order");
        }
        if self.next == 7 {
            self.published = true;
            self.next = 0;
        } else {
            self.next += 1;
        }
        Ok(())
    }
}

/// Native kCubemapOrthoBases, RVA0x1a62fa0. Each face stores right, up, back.
pub const CUBE_BASES: [[[f32; 3]; 3]; 6] = [
    [[0., 0., -1.], [0., -1., 0.], [-1., 0., 0.]],
    [[0., 0., 1.], [0., -1., 0.], [1., 0., 0.]],
    [[1., 0., 0.], [0., 0., 1.], [0., -1., 0.]],
    [[1., 0., 0.], [0., 0., -1.], [0., 1., 0.]],
    [[1., 0., 0.], [0., -1., 0.], [0., 0., -1.]],
    [[-1., 0., 0.], [0., -1., 0.], [0., 0., 1.]],
];
/// Native remap increments its roughness fraction in f32. It does not divide each index.
pub fn source_filter_levels() -> [f32; 6] {
    let step = 0.1666666716337204_f32;
    let mut fraction = step;
    std::array::from_fn(|index| {
        let radius = (128_f64 * f64::from(fraction.powf(1.899999976158142_f32))) as f32;
        fraction += step;
        if index == 5 {
            return 7.;
        }
        let mut level = 7;
        let mut lower = 256_f32;
        while lower > radius {
            lower *= 0.5;
            level -= 1;
        }
        level as f32 + (radius - lower) / lower
    })
}

/// Native ReflectionProbes::UpdateProbe uses position plus offset and unscaled box size.
/// Rotation and transform scale do not enter this axis-aligned influence volume.
pub fn probe_influence_bounds(
    position: [f32; 3],
    size: [f32; 3],
    offset: [f32; 3],
) -> ([f32; 3], [f32; 3]) {
    (
        std::array::from_fn(|axis| position[axis] + offset[axis]),
        size.map(|value| (value * 0.5).abs()),
    )
}

/// Native cube view rows are right/up/back, followed by translation(-source_position).
/// Target world coordinates reflect Z. Preserve the resulting improper view matrix.
pub fn capture_view_from_world(face: usize, target_position: [f32; 3]) -> [[f32; 4]; 4] {
    let basis = CUBE_BASES[face];
    let source_position = [target_position[0], target_position[1], -target_position[2]];
    let mut matrix = [[0.; 4]; 4];
    for row in 0..3 {
        for column in 0..3 {
            matrix[column][row] = basis[row][column] * if column == 2 { -1. } else { 1. };
        }
        // Native Matrix4x4::operator*= adds Y, X, Z, then W, without FMA.
        matrix[3][row] = ((basis[row][1] * -source_position[1]
            + basis[row][0] * -source_position[0])
            + basis[row][2] * -source_position[2])
            + 0.;
    }
    matrix[3][3] = 1.;
    matrix
}

/// The next enabled slot after `current` in cyclic order. `current` comes last.
pub fn next_probe_index<const N: usize>(current: usize, enabled: [bool; N]) -> usize {
    (1..=N)
        .map(|offset| (current + offset) % N)
        .find(|&index| enabled[index])
        .unwrap_or(current)
}

/// Source ReflectionDisableDistance keeps probes within 50 m.
pub const PROBE_DISABLE_DISTANCE_SQUARED: f32 = 2500.;

/// Distance check assignment. `candidates` are within distance, nearest first.
/// A slot keeps its probe while the probe stays among the nearest `N`. Free slots take the
/// other candidates. The `locked` slot does not change. Returns the mask of changed slots.
pub fn assign_probe_slots<K: Copy + PartialEq, const N: usize>(
    slots: &mut [Option<K>; N],
    candidates: &[K],
    locked: usize,
) -> u32 {
    let wanted = &candidates[..candidates.len().min(N)];
    let mut changed = 0;
    for (index, slot) in slots.iter_mut().enumerate() {
        if index != locked && slot.is_some_and(|key| !wanted.contains(&key)) {
            *slot = None;
            changed |= 1 << index;
        }
    }
    for key in wanted {
        if slots.contains(&Some(*key)) {
            continue;
        }
        let Some(index) = (0..N).find(|&index| index != locked && slots[index].is_none()) else {
            break;
        };
        slots[index] = Some(*key);
        changed |= 1 << index;
    }
    changed
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slots_keep_near_probes_and_skip_the_locked_slot() {
        let mut slots = [Some(1), Some(2), None, Some(3)];
        // 2 leaves, 4 and 5 arrive. Slot 3 is capturing, so 3 stays although it left.
        let changed = assign_probe_slots(&mut slots, &[1, 4, 5], 3);
        assert_eq!(slots, [Some(1), Some(4), Some(5), Some(3)]);
        assert_eq!(changed, 0b0110);
        // Later the lock moves and 3 is freed.
        assert_eq!(assign_probe_slots(&mut slots, &[1, 4, 5], 0), 0b1000);
        assert_eq!(slots, [Some(1), Some(4), Some(5), None]);
        // Only the nearest N take slots.
        let mut slots = [None; 2];
        assign_probe_slots(&mut slots, &[7, 8, 9], 1);
        assert_eq!(slots, [Some(7), None]);
    }

    #[test]
    fn enabled_probes_refresh_in_turn() {
        let mut probe = ProbeSchedule::default();
        for _ in 0..2 {
            for _ in 0..8 {
                let job = probe.pending_job(true).unwrap();
                probe.complete(job).unwrap();
            }
            assert!(probe.published());
        }
        assert_eq!(probe.pending_job(false), None);
        assert_eq!(next_probe_index(1, [true, false, true, false]), 2);
        assert_eq!(next_probe_index(2, [true, false, true, false]), 0);
        assert_eq!(next_probe_index(1, [false; 4]), 1);
    }
}
