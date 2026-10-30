//! Per-view source candidate selection, projection fit, and split masks without heap allocation.
use super::{
    shadow_candidates::{
        Bounds, CandidateState, DirectionalHull, ProjectionUnion, no_umbra_candidate,
    },
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

/// Masks stay in the caller's retained storage. Clear this cache if that storage is cleared.
/// Advance revision after any caster order, geometry, or eligibility change.
#[derive(Default)]
pub struct ShadowViewCache {
    revision: u64,
    input: Option<ViewInput>,
    result: Option<ShadowFrame>,
    caster_min: [f32; 3],
    caster_max: [f32; 3],
    /// Masks before the last preparation. Retained storage.
    previous: Vec<u8>,
    split_changes: u8,
}
impl ShadowViewCache {
    /// Splits with a caster that entered or left them in the last `prepare`.
    pub fn split_changes(&self) -> u8 {
        self.split_changes
    }
    pub fn revision(&self) -> u64 {
        self.revision
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
    }
    pub fn prepare(
        &mut self,
        revision: u64,
        casters: &[CasterInput],
        masks: &mut [u8],
        planes: CullPlanes,
        light_mask: u32,
        view_mask: u32,
        fit: &mut ShadowFitInput,
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
        self.previous.extend_from_slice(masks);
        self.split_changes = 0b1111;
        let result = prepare_frame(casters, masks, &planes, light_mask, view_mask, fit)?;
        self.split_changes = (self.previous.iter().zip(masks.iter()))
            .fold(0, |changes, (old, new)| changes | (old ^ new));
        self.input = Some(input);
        self.result = result;
        self.caster_min = fit.caster_min;
        self.caster_max = fit.caster_max;
        Ok(result)
    }
}

/// The caller supplies stable entity order and preallocated output storage of the same length.
/// fit_input contains this live view's matrices and source settings. This function fills its caster bounds.
/// A valid empty candidate set returns None and leaves every output mask zero.
/// The caller must validate caster bounds after each change with validate_casters.
pub fn prepare_frame(
    casters: &[CasterInput],
    masks: &mut [u8],
    planes: &CullPlanes,
    light_mask: u32,
    view_mask: u32,
    fit_input: &mut ShadowFitInput,
) -> Result<Option<ShadowFrame>, &'static str> {
    if casters.len() != masks.len() {
        return Err("Source shadow candidate and mask counts differ");
    }
    masks.fill(0);
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
