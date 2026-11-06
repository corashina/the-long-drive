//! Renderer count caps: loads in flight and pending instance claims.
use super::renderer_pending::{PipelineContinuation, RendererOwner};
use crate::reference::ReferenceError;
use crate::world::{GeneratedId, PresentationGrant, WorldRequest};

/// Renderer loads in flight.
pub const RENDERER_JOBS: usize = 4;
/// Renderer instance claims: structures and Grass.
pub const RENDERER_PENDING: usize = 16;

/// Loads in flight and pending claims of one renderer owner, as last applied to the ledger.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct RendererCounts {
    pub jobs: usize,
    pub pending: usize,
}
impl RendererCounts {
    pub fn add(&mut self, other: Self) {
        self.jobs += other.jobs;
        self.pending += other.pending;
    }
    /// Moves the ledger counts from `old` to `self`.
    pub fn apply(self, old: Self, ledger: &mut PresentationLedger) -> Result<(), ReferenceError> {
        ledger.end_jobs(old.jobs)?;
        ledger.start_jobs(self.jobs);
        ledger.end_pending(old.pending)?;
        ledger.add_pending(self.pending);
        Ok(())
    }
}

/// Renderer counts against fixed caps, and the publication slot the stream lends.
#[derive(Debug, Default)]
pub struct PresentationLedger {
    jobs: usize,
    pending: usize,
    publication: bool,
}

impl PresentationLedger {
    /// Loads in flight and pending claims.
    pub fn counts(&self) -> (usize, usize) {
        (self.jobs, self.pending)
    }
    /// Free job slots.
    pub fn available_external_jobs(&self) -> usize {
        RENDERER_JOBS.saturating_sub(self.jobs)
    }
    /// True if `claims` more pending claims fit the cap.
    pub fn can_request_independent(&self, claims: usize) -> bool {
        self.pending + claims <= RENDERER_PENDING
    }
    pub fn start_jobs(&mut self, jobs: usize) {
        self.jobs += jobs;
    }
    pub fn end_jobs(&mut self, jobs: usize) -> Result<(), ReferenceError> {
        self.jobs = self
            .jobs
            .checked_sub(jobs)
            .ok_or_else(|| ReferenceError("Missing renderer job".into()))?;
        Ok(())
    }
    pub fn add_pending(&mut self, claims: usize) {
        self.pending += claims;
    }
    pub fn end_pending(&mut self, claims: usize) -> Result<(), ReferenceError> {
        self.pending = self
            .pending
            .checked_sub(claims)
            .ok_or_else(|| ReferenceError("Missing renderer pending claim".into()))?;
        Ok(())
    }
    /// Takes one pending claim for a Grass renderer.
    pub fn claim_grass(&mut self, request: WorldRequest, entity: u64) -> PipelineContinuation {
        self.pending += 1;
        PipelineContinuation::counted(RendererOwner::Grass(request, entity))
    }
    /// Takes one pending claim for a structure renderer.
    pub fn claim_structure(
        &mut self,
        request: WorldRequest,
        id: GeneratedId,
    ) -> PipelineContinuation {
        self.pending += 1;
        PipelineContinuation::counted(RendererOwner::Structure(request, id))
    }
    /// Gives a drained structure claim to the stream ready queue.
    pub fn hand_over_claim(
        &mut self,
        claim: &mut PipelineContinuation,
    ) -> Result<(), ReferenceError> {
        if !matches!(claim.owner, RendererOwner::Structure(..)) {
            return Err(ReferenceError("Wrong structure claim".into()));
        }
        claim.release()?;
        self.end_pending(1)
    }
    /// Gives back a drained Grass or structure claim.
    pub fn release_claim(
        &mut self,
        claim: &mut PipelineContinuation,
    ) -> Result<(), ReferenceError> {
        if matches!(claim.owner, RendererOwner::Surface(..)) {
            return Err(ReferenceError(
                "Surface claim belongs to its ready owner".into(),
            ));
        }
        claim.release()?;
        self.end_pending(1)
    }
    /// True once per frame when the stream lends its publication slot.
    pub fn take_publication(&mut self) -> bool {
        std::mem::take(&mut self.publication)
    }
    pub fn grant(&mut self, grant: PresentationGrant) {
        self.publication = grant.publication;
    }
}
