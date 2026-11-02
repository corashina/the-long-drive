//! Per-view source candidate selection, projection fit, and split masks without heap allocation.
use super::{
    shadow_candidates::{
        Bounds, CandidateState, DirectionalHull, ProjectionUnion, no_umbra_candidate,
    },
    shadow_grid::CasterGrid,
    shadow_planes::CullPlanes,
    shadow_splits::{SplitCull, visible_split_mask},
    shadows::{ShadowFit, ShadowFitInput, fit_cascades, frustum_scale},
};

/// Each material primitive carries its complete renderer AABB.
/// Repeated bounds from multiple valid material parts do not change the geometric union.
#[derive(Clone, Copy, Debug)]
pub struct CasterInput {
    pub bounds: Bounds,
    pub state: CandidateState,
    pub material_has_caster: bool,
}
impl CasterInput {
    /// Compare every input bit, including signed zero in geometry.
    pub fn unchanged(&self, other: &Self) -> bool {
        self.bounds.center.map(f32::to_bits) == other.bounds.center.map(f32::to_bits)
            && self.bounds.extents.map(f32::to_bits) == other.bounds.extents.map(f32::to_bits)
            && self.state == other.state
            && self.material_has_caster == other.material_has_caster
    }
}
#[derive(Clone, Copy)]
pub struct ShadowFrame {
    pub fit: ShadowFit,
    pub candidate_count: usize,
    pub split_counts: [usize; 4],
}

/// Run once when the ordered caster set changes, before preparing any view.
pub fn validate_casters(casters: &[CasterInput]) -> Result<(), &'static str> {
    if casters.iter().any(|caster| {
        caster
            .bounds
            .center
            .iter()
            .chain(caster.bounds.extents.iter())
            .any(|value| !value.is_finite())
            || caster.bounds.extents.iter().any(|value| *value < 0.)
    }) {
        return Err("Source shadow scene has malformed bounds or planes");
    }
    Ok(())
}

#[derive(Clone, Copy)]
struct ViewInput {
    revision: u64,
    planes: CullPlanes,
    light_mask: u32,
    view_mask: u32,
    fit: ShadowFitInput,
}
impl ViewInput {
    fn unchanged(&self, other: &Self) -> bool {
        let words = |value: &Self| {
            value
                .planes
                .camera
                .into_iter()
                .flatten()
                .chain(value.planes.shadow.into_iter().flatten())
                .chain(value.fit.frustum_points.into_iter().flatten())
                .chain(value.fit.camera_basis.into_iter().flatten())
                .chain(value.fit.camera_position)
                .chain(value.fit.light_basis.into_iter().flatten())
                .chain(value.fit.caster_min)
                .chain(value.fit.caster_max)
                .chain([
                    value.fit.near,
                    value.fit.far,
                    value.fit.vertical_fov,
                    value.fit.shadow_distance,
                ])
                .chain(value.fit.split_fractions)
                .chain([value.fit.quality_near_offset, value.fit.shadow_strength])
                .map(f32::to_bits)
        };
        self.revision == other.revision
            && self.light_mask == other.light_mask
            && self.view_mask == other.view_mask
            && self.fit.tile_size == other.fit.tile_size
            && words(self).eq(words(other))
    }
}

/// Sun cascade bits of a caster mask. Bits 4..7 belong to the lamp slots.
const SUN_BITS: u8 = 0x0f;

/// Masks stay in the caller's retained storage. Clear this cache if that storage is cleared.
/// Advance revision after any caster order, geometry, or eligibility change.
#[derive(Default)]
pub struct ShadowViewCache {
    revision: u64,
    input: Option<ViewInput>,
    result: Option<ShadowFrame>,
    caster_min: [f32; 3],
    caster_max: [f32; 3],
    /// Candidate rows of the last preparation, in row order. Other rows have zero sun bits.
    rows: Vec<u32>,
    /// Rows and sun bits before the last preparation. Retained storage.
    previous: Vec<(u32, u8)>,
    split_changes: u8,
}
impl ShadowViewCache {
    /// Splits with a static caster that entered or left them in the last `prepare`.
    pub fn split_changes(&self) -> u8 {
        self.split_changes
    }
    pub fn revision(&self) -> u64 {
        self.revision
    }
    /// Rows that can have nonzero sun bits, in row order.
    pub fn rows(&self) -> &[u32] {
        &self.rows
    }
    fn advance_revision(&mut self) {
        self.revision = self
            .revision
            .checked_add(1)
            .expect("Source shadow view revision exhausted");
    }
    pub fn clear(&mut self) {
        if self.input.take().is_some() {
            self.advance_revision();
        }
        self.result = None;
        self.rows.clear();
    }
    #[allow(clippy::too_many_arguments)]
    pub fn prepare(
        &mut self,
        revision: u64,
        casters: &[CasterInput],
        grid: &CasterGrid,
        masks: &mut [u8],
        planes: CullPlanes,
        light_mask: u32,
        view_mask: u32,
        fit: &mut ShadowFitInput,
        animated: &[u32],
    ) -> Result<Option<ShadowFrame>, &'static str> {
        if casters.len() != masks.len() {
            return Err("Source shadow candidate and mask counts differ");
        }
        let input = ViewInput {
            revision,
            planes,
            light_mask,
            view_mask,
            fit: *fit,
        };
        if self
            .input
            .as_ref()
            .is_some_and(|previous| previous.unchanged(&input))
        {
            fit.caster_min = self.caster_min;
            fit.caster_max = self.caster_max;
            self.split_changes = 0;
            return Ok(self.result);
        }
        self.input = None;
        self.result = None;
        // Failed preparation can also change masks. Recovery must invalidate extraction.
        self.advance_revision();
        self.previous.clear();
        for &row in &self.rows {
            let mask = &mut masks[row as usize];
            self.previous.push((row, *mask & SUN_BITS));
            *mask &= !SUN_BITS;
        }
        self.split_changes = 0b1111;
        let result = prepare_frame(
            casters,
            grid,
            masks,
            &mut self.rows,
            &planes,
            light_mask,
            view_mask,
            fit,
        )?;
        // Both lists are in row order. A row in one list only had zero sun bits in the other.
        // Animated casters draw in each refresh, so their changes do not count.
        let (old, new) = (&self.previous, &self.rows);
        let change = |row: u32, bits: u8| {
            if animated.binary_search(&row).is_ok() {
                0
            } else {
                bits ^ (masks[row as usize] & SUN_BITS)
            }
        };
        let (mut a, mut b, mut changes) = (0, 0, 0);
        while a < old.len() || b < new.len() {
            match (old.get(a), new.get(b)) {
                (Some(&(row, bits)), Some(&next)) if row == next => {
                    changes |= change(row, bits);
                    a += 1;
                    b += 1;
                }
                (Some(&(row, bits)), next) if next.is_none_or(|&next| row < next) => {
                    // The row is not a candidate now, so its sun bits are zero.
                    changes |= change(row, bits);
                    a += 1;
                }
                (_, Some(&next)) => {
                    changes |= change(next, 0);
                    b += 1;
                }
                (_, None) => unreachable!(),
            }
        }
        self.split_changes = changes;
        self.input = Some(input);
        self.result = result;
        self.caster_min = fit.caster_min;
        self.caster_max = fit.caster_max;
        Ok(result)
    }
}

/// The caller supplies stable entity order and preallocated mask storage of the same length.
/// Sun bits must be zero outside `rows`. This function replaces `rows` with the candidate rows,
/// in row order, and writes only their sun bits. Lamp bits stay.
/// fit_input contains this live view's matrices and source settings. This function fills its caster bounds.
/// A valid empty candidate set returns None.
/// The caller must validate caster bounds after each change with validate_casters.
#[allow(clippy::too_many_arguments)]
pub fn prepare_frame(
    casters: &[CasterInput],
    grid: &CasterGrid,
    masks: &mut [u8],
    rows: &mut Vec<u32>,
    planes: &CullPlanes,
    light_mask: u32,
    view_mask: u32,
    fit_input: &mut ShadowFitInput,
) -> Result<Option<ShadowFrame>, &'static str> {
    rows.clear();
    if casters.len() != masks.len() {
        return Err("Source shadow candidate and mask counts differ");
    }
    if grid.len() != casters.len() {
        return Err("Source shadow grid and caster counts differ");
    }
    if planes
        .camera
        .iter()
        .chain(planes.shadow.iter())
        .flatten()
        .any(|value| !value.is_finite())
    {
        return Err("Source shadow scene has malformed bounds or planes");
    }
    // Source far camera near300 exceeds shadowDistance100. The native scale test
    // at 0x2b42c4..0x2b42cb rejects its cascades. The collector stays fully lit.
    if frustum_scale(fit_input)? <= 9.999999747378752e-6 {
        return Ok(None);
    }
    let hull = DirectionalHull::new(planes.shadow, fit_input.light_basis[2])?;
    grid.visit_hull(hull.planes(), |row| {
        let caster = &casters[row as usize];
        if caster.material_has_caster
            && no_umbra_candidate(&caster.state, caster.bounds, &hull, light_mask, view_mask)
        {
            rows.push(row);
        }
    });
    // Row order keeps the projection union bit-identical to a linear pass.
    rows.sort_unstable();
    let mut union = ProjectionUnion::default();
    for &row in rows.iter() {
        let mask = &mut masks[row as usize];
        *mask = (*mask & !SUN_BITS) | 1;
        union.include(casters[row as usize].bounds, &planes.camera);
    }
    let Some(bounds) = union.bounds() else {
        return Ok(None);
    };
    fit_input.caster_min = bounds.min;
    fit_input.caster_max = bounds.max;
    let fit = fit_cascades(fit_input)?;
    let make_split = |index: usize| {
        let cascade = &fit.cascades[index];
        SplitCull::new(
            index,
            [
                cascade.sphere_center[0],
                cascade.sphere_center[1],
                cascade.sphere_center[2],
                cascade.sphere_radius,
            ],
            fit_input.light_basis,
            planes.shadow,
            fit_input.near,
            fit_input.shadow_distance.min(fit_input.far),
            cascade.min_view_distance,
            cascade.max_view_distance,
        )
    };
    let splits = [
        make_split(0)?,
        make_split(1)?,
        make_split(2)?,
        make_split(3)?,
    ];
    let mut split_counts = [0; 4];
    for &row in rows.iter() {
        let sun = visible_split_mask(
            casters[row as usize].bounds,
            fit_input.light_basis,
            &planes.shadow,
            &splits,
        );
        let mask = &mut masks[row as usize];
        *mask = (*mask & !SUN_BITS) | sun;
        for (index, count) in split_counts.iter_mut().enumerate() {
            *count += usize::from(sun & (1 << index) != 0);
        }
    }
    Ok(Some(ShadowFrame {
        fit,
        candidate_count: rows.len(),
        split_counts,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::reference::{
        lamp_shadows::{LAMP_BIT, LAMP_SLOTS, LampMasks, LampSphere, lamp_caster},
        shadow_planes::implicit_cull_planes,
        view_math::source_culling_view,
    };
    use glam::{EulerRot, Mat4, Quat, Vec3, Vec4};

    struct Random(u64);
    impl Random {
        fn next(&mut self) -> f32 {
            self.0 ^= self.0 << 13;
            self.0 ^= self.0 >> 7;
            self.0 ^= self.0 << 17;
            (self.0 >> 40) as f32 / (1u64 << 24) as f32
        }
        fn range(&mut self, low: f32, high: f32) -> f32 {
            low + (high - low) * self.next()
        }
        fn chance(&mut self, p: f32) -> bool {
            self.next() < p
        }
    }

    /// The old linear pass over every caster: the reference for the indexed path.
    fn linear_frame(
        casters: &[CasterInput],
        masks: &mut [u8],
        planes: &CullPlanes,
        light_mask: u32,
        view_mask: u32,
        fit_input: &mut ShadowFitInput,
    ) -> Result<Option<ShadowFrame>, &'static str> {
        masks.fill(0);
        if frustum_scale(fit_input)? <= 9.999999747378752e-6 {
            return Ok(None);
        }
        let hull = DirectionalHull::new(planes.shadow, fit_input.light_basis[2])?;
        let mut union = ProjectionUnion::default();
        let mut candidate_count = 0;
        for (caster, mask) in casters.iter().zip(masks.iter_mut()) {
            if !caster.material_has_caster
                || !no_umbra_candidate(&caster.state, caster.bounds, &hull, light_mask, view_mask)
            {
                continue;
            }
            *mask = 1;
            candidate_count += 1;
            union.include(caster.bounds, &planes.camera);
        }
        let Some(bounds) = union.bounds() else {
            return Ok(None);
        };
        fit_input.caster_min = bounds.min;
        fit_input.caster_max = bounds.max;
        let fit = fit_cascades(fit_input)?;
        let splits: Vec<_> = (0..4)
            .map(|index| {
                let cascade = &fit.cascades[index];
                SplitCull::new(
                    index,
                    [
                        cascade.sphere_center[0],
                        cascade.sphere_center[1],
                        cascade.sphere_center[2],
                        cascade.sphere_radius,
                    ],
                    fit_input.light_basis,
                    planes.shadow,
                    fit_input.near,
                    fit_input.shadow_distance.min(fit_input.far),
                    cascade.min_view_distance,
                    cascade.max_view_distance,
                )
            })
            .collect::<Result<_, _>>()?;
        let mut split_counts = [0; 4];
        for (caster, mask) in casters.iter().zip(masks) {
            if *mask == 0 {
                continue;
            }
            *mask = visible_split_mask(
                caster.bounds,
                fit_input.light_basis,
                &planes.shadow,
                &splits,
            );
            for (index, count) in split_counts.iter_mut().enumerate() {
                *count += usize::from(*mask & (1 << index) != 0);
            }
        }
        Ok(Some(ShadowFrame {
            fit,
            candidate_count,
            split_counts,
        }))
    }
    fn linear_lamps(casters: &[CasterInput], slots: &[Option<LampSphere>; 4], masks: &mut [u8]) {
        for (caster, mask) in casters.iter().zip(masks) {
            for (slot, lamp) in slots.iter().enumerate() {
                if lamp.is_some_and(|lamp| lamp_caster(caster, &lamp)) {
                    *mask |= 1 << (LAMP_BIT as usize + slot);
                }
            }
        }
    }
    fn caster(random: &mut Random) -> CasterInput {
        let mut center = [
            random.range(-400., 400.),
            random.range(-2., 30.),
            random.range(-400., 400.),
        ];
        if random.chance(0.05) {
            center[0] = -0.;
        }
        let extents = if random.chance(0.05) {
            [0.; 3]
        } else if random.chance(0.02) {
            [random.range(20., 80.); 3]
        } else {
            std::array::from_fn(|_| random.range(0.05, 6.))
        };
        CasterInput {
            bounds: Bounds { center, extents },
            state: CandidateState {
                shadow_casting_mode: u8::from(!random.chance(0.05)),
                layer: (random.next() * 3.) as u8,
                renderer_present: true,
                disabled: random.chance(0.05),
                force_off: false,
                lod_group: 0,
                lod_index_mask: 0,
                active_lod: 0,
            },
            material_has_caster: !random.chance(0.05),
        }
    }
    fn view(random: &mut Random, light_basis: [[f32; 3]; 3]) -> (CullPlanes, ShadowFitInput) {
        let (near, far, fov, distance) = (0.3f32, 1000., 60f32, 100.);
        let position = Vec3::new(
            random.range(-300., 300.),
            random.range(1., 20.),
            random.range(-300., 300.),
        );
        let turn = Quat::from_euler(
            EulerRot::YXZ,
            random.range(-3.1, 3.1),
            random.range(-0.6, 0.4),
            0.,
        );
        let view = Mat4::look_to_rh(position, turn * Vec3::NEG_Z, Vec3::Y);
        let projection = Mat4::perspective_rh_gl(fov.to_radians(), 16. / 9., near, far);
        let source_view = Mat4::from_cols_array_2d(&source_culling_view(view.to_cols_array_2d()));
        let world_from_view = source_view.inverse();
        let planes = implicit_cull_planes(
            (projection * source_view).to_cols_array(),
            world_from_view.to_cols_array(),
            near,
            far,
            distance,
            0.,
        );
        let inverse = projection.inverse();
        let corner = |x: f32, y: f32, z: f32| {
            let v = inverse * Vec4::new(x, y, z, 1.);
            (v.truncate() / v.w).to_array()
        };
        let input = ShadowFitInput {
            frustum_points: [
                corner(-1., -1., -1.),
                corner(1., -1., -1.),
                corner(1., 1., -1.),
                corner(-1., 1., -1.),
                corner(-1., -1., 1.),
                corner(1., -1., 1.),
                corner(1., 1., 1.),
                corner(-1., 1., 1.),
            ],
            camera_basis: [
                world_from_view.x_axis.truncate().to_array(),
                world_from_view.y_axis.truncate().to_array(),
                world_from_view.z_axis.truncate().to_array(),
            ],
            camera_position: world_from_view.w_axis.truncate().to_array(),
            light_basis,
            caster_min: [0.; 3],
            caster_max: [0.; 3],
            near,
            far,
            vertical_fov: fov,
            shadow_distance: distance,
            split_fractions: [0.067, 0.2, 0.467],
            tile_size: [1024; 2],
            quality_near_offset: 2.,
            shadow_strength: 1.,
        };
        (planes, input)
    }

    /// Random scenes: the indexed path gives the same masks, fit and counts as the linear pass.
    #[test]
    fn indexed_masks_match_the_linear_pass() {
        let mut random = Random(0x9e37_79b9_7f4a_7c15);
        let mut casters: Vec<_> = (0..3000).map(|_| caster(&mut random)).collect();
        let mut grid = CasterGrid::default();
        grid.rebuild(&casters);
        let mut cache = ShadowViewCache::default();
        let mut lamps = LampMasks::default();
        let mut masks = vec![0u8; casters.len()];
        let mut expected = vec![0u8; casters.len()];
        let mut previous = expected.clone();
        // Animated rows do not count in the split changes.
        let animated = [3, 50, 700, 2999];
        let (mut partial, mut fits) = (0, 0);
        for frame in 0..300u64 {
            // Some casters move, some far, as cars do.
            for _ in 0..20 {
                let row = (random.next() * casters.len() as f32) as usize % casters.len();
                let bounds = &mut casters[row].bounds;
                let step = if random.chance(0.1) { 300. } else { 4. };
                bounds.center[0] += random.range(-step, step);
                bounds.center[2] += random.range(-step, step);
                grid.update(row as u32, *bounds);
            }
            let sun = Quat::from_euler(
                EulerRot::YXZ,
                random.range(-3.1, 3.1),
                random.range(-1.5, -0.1),
                0.,
            );
            let light_basis = [
                (sun * Vec3::X).to_array(),
                (sun * Vec3::Y).to_array(),
                (sun * Vec3::NEG_Z).to_array(),
            ];
            let (planes, mut input) = view(&mut random, light_basis);
            let mut linear_input = input;
            let (light_mask, view_mask) = (0b111, if random.chance(0.2) { 0b001 } else { 0b111 });
            let result = cache.prepare(
                frame, &casters, &grid, &mut masks, planes, light_mask, view_mask, &mut input,
                &animated,
            );
            let linear = linear_frame(
                &casters,
                &mut expected,
                &planes,
                light_mask,
                view_mask,
                &mut linear_input,
            );
            match (result, linear) {
                (Ok(Some(a)), Ok(Some(b))) => {
                    fits += 1;
                    assert_eq!(a.candidate_count, b.candidate_count);
                    assert_eq!(a.split_counts, b.split_counts);
                    assert_eq!(
                        format!("{:?}", a.fit.cascades),
                        format!("{:?}", b.fit.cascades)
                    );
                    assert_eq!(a.fit.fade.map(f32::to_bits), b.fit.fade.map(f32::to_bits));
                    assert_eq!(
                        a.fit.fade_center.map(f32::to_bits),
                        b.fit.fade_center.map(f32::to_bits)
                    );
                    partial += usize::from(b.candidate_count > 0 && b.candidate_count < 2000);
                }
                (Ok(None), Ok(None)) => {}
                (Err(a), Err(b)) => assert_eq!(a, b),
                _ => panic!("indexed and linear results differ in frame {frame}"),
            }
            assert_eq!(
                input.caster_min.map(f32::to_bits),
                linear_input.caster_min.map(f32::to_bits)
            );
            assert_eq!(
                input.caster_max.map(f32::to_bits),
                linear_input.caster_max.map(f32::to_bits)
            );
            let sun_bits = |m: &u8| m & SUN_BITS;
            assert!(masks.iter().map(sun_bits).eq(expected.iter().copied()));
            let changes = (previous.iter().zip(&expected).enumerate())
                .filter(|(row, _)| animated.binary_search(&(*row as u32)).is_err())
                .fold(0, |c, (_, (old, new))| c | (old ^ new));
            assert_eq!(cache.split_changes(), changes);
            previous.copy_from_slice(&expected);
            let rows = cache.rows();
            assert!(rows.windows(2).all(|pair| pair[0] < pair[1]));
            assert!(
                expected
                    .iter()
                    .enumerate()
                    .all(|(row, mask)| *mask == 0 || rows.binary_search(&(row as u32)).is_ok())
            );
            // Lamp bits on the same masks.
            let slots: [Option<LampSphere>; LAMP_SLOTS] = std::array::from_fn(|light| {
                random.chance(0.6).then(|| LampSphere {
                    light,
                    center: casters[light * 7].bounds.center,
                    range: random.range(2., 40.),
                    culling_mask: 0b011,
                })
            });
            let old: Vec<u8> = masks.iter().map(|mask| mask & !SUN_BITS).collect();
            let changed = lamps.write(&casters, &grid, &slots, &mut masks);
            let lamp_changes =
                (old.iter().zip(&masks)).fold(0, |c, (o, m)| c | (o ^ (m & !SUN_BITS)));
            assert_eq!(changed, lamp_changes);
            linear_lamps(&casters, &slots, &mut expected);
            assert!(masks == expected, "lamp masks differ in frame {frame}");
            assert!(
                lamps.rows().eq(expected
                    .iter()
                    .enumerate()
                    .filter(|(_, mask)| **mask & !SUN_BITS != 0)
                    .map(|(row, _)| row as u32))
            );
            for mask in &mut expected {
                *mask &= SUN_BITS;
            }
        }
        assert!(
            fits > 100 && partial > 100,
            "{fits} fits, {partial} partial"
        );
    }
}
