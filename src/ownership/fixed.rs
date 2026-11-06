//! Renderer counts of fixed Bevy imports.
use super::PresentationLedger;
use crate::reference::ReferenceError;

fn require(ok: bool, field: &str) -> Result<(), ReferenceError> {
    if ok {
        Ok(())
    } else {
        Err(ReferenceError(format!("Invalid reference field: {field}")))
    }
}

/// Production lifetime of one fixed assembled image.
#[derive(Clone, Copy, Debug, Default)]
pub struct FixedImageOwnership {
    started: bool,
    row_live: bool,
    assembled: bool,
    complete: bool,
    cancelled: bool,
}
impl FixedImageOwnership {
    pub fn begin_row(&mut self) -> Result<bool, ReferenceError> {
        if self.cancelled {
            return Ok(false);
        }
        require(
            !self.row_live && !self.assembled,
            "fixed row admission state",
        )?;
        self.started = true;
        self.row_live = true;
        Ok(true)
    }
    pub fn finish_row(&mut self, terminal: bool) -> bool {
        if terminal {
            self.row_live = false;
        }
        terminal
    }
    pub fn assemble(&mut self) -> Result<(), ReferenceError> {
        require(
            self.started && !self.row_live && !self.cancelled,
            "fixed row assembly state",
        )?;
        self.assembled = true;
        Ok(())
    }
    pub fn uploaded(&mut self) -> Result<(), ReferenceError> {
        require(self.assembled && !self.row_live, "fixed image upload state")?;
        self.complete = true;
        Ok(())
    }
    pub fn cancel(&mut self) {
        self.cancelled = true;
    }
    pub fn discard(&mut self) -> Result<(), ReferenceError> {
        require(
            self.cancelled && !self.row_live && !self.assembled,
            "fixed row physical retirement",
        )?;
        self.started = false;
        Ok(())
    }
    /// Loads in flight and pending claims.
    pub fn io(&self) -> (usize, usize) {
        (
            usize::from(self.row_live),
            usize::from(self.started && !self.complete),
        )
    }
}

/// True if a fixed load fits the renderer caps. `job`: it starts a load. `pending`: it takes
/// a claim.
pub fn fixed_room(ledger: &PresentationLedger, job: bool, pending: bool) -> bool {
    (!job || ledger.available_external_jobs() > 0)
        && (!pending || ledger.can_request_independent(1))
}
