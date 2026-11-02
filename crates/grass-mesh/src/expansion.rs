//! Per-instance Grass preparation. The owning world scheduler supplies the job slots and admission.
use super::{GrassAnchor, GrassDomain, GrassPatch, GrassSettings, GrassView};
use std::sync::Arc;

#[derive(Clone, Copy)]
struct PatchPlan {
    real: f32,
    factor: u32,
}
#[derive(Default)]
pub struct GrassMeshData {
    pub anchors: Vec<[f32; 3]>,
    pub uvs: Vec<[f32; 2]>,
    /// Fraction along blade, side, source domain smoothing, previous fraction.
    pub blades: Vec<[f32; 4]>,
    pub patches: Vec<u32>,
    pub indices: Vec<u32>,
}
impl GrassMeshData {
    pub fn retained_bytes(&self) -> usize {
        self.anchors.capacity() * 12
            + self.uvs.capacity() * 8
            + self.blades.capacity() * 16
            + self.patches.capacity() * 4
            + self.indices.capacity() * 4
    }
}
/// Exact reservations for the pending plan. Source patches and cached domains are separate.
#[derive(Clone, Copy, Debug)]
pub struct GrassAllocation {
    pub vertices: usize,
    pub indices: usize,
    pub scratch_bytes: usize,
    pub output_bytes: usize,
    pub gpu_bytes: usize,
}
#[derive(Clone, Copy, PartialEq)]
enum Phase {
    Planning,
    Planned,
    Emitting,
    Complete,
}
pub struct GrassExpansion {
    pub view: GrassView,
    settings: GrassSettings,
    patches: Arc<[GrassPatch]>,
    domains: Arc<[GrassDomain; 4]>,
    plans: Vec<PatchPlan>,
    patch: usize,
    triangle: usize,
    vertices: usize,
    indices: usize,
    phase: Phase,
    pub output: GrassMeshData,
}
impl GrassExpansion {
    pub fn planning_bytes(patches: usize) -> Result<usize, String> {
        u32::try_from(patches).map_err(|_| "Grass patch address range exceeded")?;
        patches
            .checked_mul(std::mem::size_of::<PatchPlan>())
            .and_then(|bytes| bytes.checked_add(std::mem::size_of::<Self>()))
            .filter(|bytes| *bytes <= isize::MAX as usize)
            .ok_or_else(|| "Grass planning allocation overflow".into())
    }
    pub fn new(
        view: GrassView,
        settings: GrassSettings,
        patches: Arc<[GrassPatch]>,
        domains: Arc<[GrassDomain; 4]>,
        admit: impl FnOnce(usize) -> Result<(), String>,
    ) -> Result<Self, String> {
        let plan_bytes = Self::planning_bytes(patches.len())?;
        admit(plan_bytes)?;
        let plans = Vec::with_capacity(patches.len());
        Ok(Self {
            view,
            settings,
            patches,
            domains,
            plans,
            patch: 0,
            triangle: 0,
            vertices: 0,
            indices: 0,
            phase: Phase::Planning,
            output: GrassMeshData::default(),
        })
    }
    /// Count at most 4096 prospective vertices or culled patches per call.
    pub fn plan(&mut self) -> Result<Option<GrassAllocation>, String> {
        match self.phase {
            Phase::Planned => return self.allocation().map(Some),
            Phase::Planning => {}
            _ => return Err("Grass plan is already approved".into()),
        }
        if !self.advance(false)? {
            return Ok(None);
        }
        let allocation = self.allocation()?;
        self.phase = Phase::Planned;
        Ok(Some(allocation))
    }
    fn allocation(&self) -> Result<GrassAllocation, String> {
        if self.vertices > u32::MAX as usize {
            return Err("Grass vertex address range exceeded".into());
        }
        let geometry = self
            .vertices
            .checked_mul(40)
            .and_then(|bytes| bytes.checked_add(self.indices.checked_mul(4)?))
            .ok_or("Grass allocation size overflow")?;
        if self
            .vertices
            .checked_mul(16)
            .is_none_or(|bytes| bytes > isize::MAX as usize)
            || self
                .indices
                .checked_mul(4)
                .is_none_or(|bytes| bytes > isize::MAX as usize)
        {
            return Err("Grass output allocation address range exceeded".into());
        }
        Ok(GrassAllocation {
            vertices: self.vertices,
            indices: self.indices,
            scratch_bytes: std::mem::size_of::<Self>()
                + self.plans.capacity() * std::mem::size_of::<PatchPlan>(),
            output_bytes: geometry,
            gpu_bytes: geometry,
        })
    }
    /// The caller must reserve output and copy overlap before this acknowledgement.
    pub fn approve(&mut self) -> Result<(), String> {
        if self.phase != Phase::Planned {
            return Err("Grass plan is not awaiting approval".into());
        }
        self.output = GrassMeshData {
            anchors: Vec::with_capacity(self.vertices),
            uvs: Vec::with_capacity(self.vertices),
            blades: Vec::with_capacity(self.vertices),
            patches: Vec::with_capacity(self.vertices),
            indices: Vec::with_capacity(self.indices),
        };
        self.phase = Phase::Emitting;
        self.patch = 0;
        self.triangle = 0;
        Ok(())
    }
    pub fn approved(&self) -> bool {
        matches!(self.phase, Phase::Emitting | Phase::Complete)
    }
    /// Emit at most 4096 vertices or culled patches per call after approval.
    pub fn prepare(&mut self) -> Result<bool, String> {
        match self.phase {
            Phase::Complete => return Ok(true),
            Phase::Emitting => {}
            _ => return Err("Grass output has not been approved".into()),
        }
        if !self.advance(true)? {
            return Ok(false);
        }
        self.phase = Phase::Complete;
        Ok(true)
    }
    fn advance(&mut self, emit: bool) -> Result<bool, String> {
        let mut work = 0usize;
        while self.patch < self.patches.len() {
            if work >= 4096 {
                return Ok(false);
            }
            if self.plans.len() == self.patch {
                let (real, factor) = self
                    .settings
                    .tessellation(&self.patches[self.patch], self.view);
                self.plans.push(PatchPlan { real, factor });
            }
            let plan = self.plans[self.patch];
            if plan.factor == 0 {
                self.patch += 1;
                self.triangle = 0;
                // Culled patches also consume work, so long invisible instances stay bounded.
                work += 1;
                if work >= 4096 {
                    return Ok(false);
                }
                continue;
            }
            let domain_index = match plan.factor {
                2 => 0,
                4 => 1,
                8 => 2,
                16 => 3,
                _ => {
                    return Err("Grass hull produced an unsupported partition".into());
                }
            };
            let domain = &self.domains[domain_index];
            if self.triangle == domain.triangles.len() {
                self.patch += 1;
                self.triangle = 0;
                continue;
            }
            let leading = domain.triangles[self.triangle][0] as usize;
            let anchor = self.settings.anchor(
                &self.patches[self.patch],
                domain.points[leading],
                plan.real,
                plan.factor,
            );
            let segments = self
                .settings
                .segments(anchor.position.distance(self.view.position));
            let emitted = (segments as usize + 1) * 2;
            if work + emitted > 4096 {
                return Ok(false);
            }
            work += emitted;
            if emit {
                self.emit(anchor, segments);
            } else {
                self.vertices = self
                    .vertices
                    .checked_add(emitted)
                    .ok_or("Grass vertex count overflow")?;
                self.indices = self
                    .indices
                    .checked_add(segments as usize * 6)
                    .ok_or("Grass index count overflow")?;
            }
            self.triangle += 1;
        }
        Ok(true)
    }
    fn emit(&mut self, anchor: GrassAnchor, segments: u32) {
        let start = self.output.anchors.len() as u32;
        for step in 0..=segments {
            let fraction = step as f32 / segments as f32;
            let previous = step.saturating_sub(1) as f32 / segments as f32;
            for side in [-1.0, 1.0] {
                self.output.anchors.push(anchor.position.to_array());
                self.output.uvs.push(anchor.uv.to_array());
                self.output
                    .blades
                    .push([fraction, side, anchor.smoothing, previous]);
                self.output.patches.push(self.patch as u32);
            }
            if step < segments {
                let a = start + step * 2;
                // Source triangle strip, with one source-to-runtime winding reflection.
                self.output
                    .indices
                    .extend_from_slice(&[a, a + 2, a + 1, a + 2, a + 3, a + 1]);
            }
        }
    }
    pub fn finish(self) -> (GrassView, GrassMeshData, usize) {
        assert!(self.phase == Phase::Complete, "Grass output is incomplete");
        let scratch =
            std::mem::size_of::<Self>() + self.plans.capacity() * std::mem::size_of::<PatchPlan>();
        (self.view, self.output, scratch)
    }
    pub fn retained_bytes(&self) -> usize {
        self.plans.capacity() * std::mem::size_of::<PatchPlan>() + self.output.retained_bytes()
    }
}
