//! Renderer ledger charges for fixed Bevy imports.
use super::PresentationLedger;
use crate::reference::ReferenceError;

fn overflow() -> ReferenceError {
    ReferenceError("Fixed Bevy reservation overflow".into())
}
fn require(ok: bool, field: &str) -> Result<(), ReferenceError> {
    if ok {
        Ok(())
    } else {
        Err(ReferenceError(format!("Invalid reference field: {field}")))
    }
}

/// Per-stream snapshot of global fixed ownership in the existing presentation ledger.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct FixedAllocation {
    pub cpu: usize,
    pub gpu: usize,
    pub jobs: usize,
    pub pending: usize,
    pub staging: usize,
}
impl FixedAllocation {
    pub fn replace(
        self,
        old: Self,
        mut a: crate::world::PresentationAllocation,
    ) -> Result<crate::world::PresentationAllocation, ReferenceError> {
        let replace = |value: usize, before: usize, after: usize| {
            value
                .checked_sub(before)
                .and_then(|n| n.checked_add(after))
                .ok_or_else(overflow)
        };
        a.prototype_cpu = replace(a.prototype_cpu, old.cpu, self.cpu)?;
        a.prototype_gpu = replace(a.prototype_gpu, old.gpu, self.gpu)?;
        a.jobs = replace(a.jobs, old.jobs, self.jobs)?;
        a.pending = replace(a.pending, old.pending, self.pending)?;
        a.staging_bytes = replace(a.staging_bytes, old.staging, self.staging)?;
        a.dependency_staging_bytes =
            replace(a.dependency_staging_bytes, old.staging, self.staging)?;
        Ok(a)
    }
}

/// Admit shared control bytes before its allocation closure can run.
pub fn admit_control<T>(
    ledger: &mut PresentationLedger,
    bytes: usize,
    allocate: impl FnOnce() -> T,
) -> Result<(FixedAllocation, T), ReferenceError> {
    let fixed = FixedAllocation {
        cpu: bytes,
        ..FixedAllocation::default()
    };
    let next = fixed.replace(FixedAllocation::default(), ledger.allocation())?;
    ledger.set_allocation(next)?;
    Ok((fixed, allocate()))
}

/// Production lifetime of one fixed assembled image and its bounded row scratch.
#[derive(Clone, Copy, Debug)]
pub struct FixedImageOwnership {
    pixels: usize,
    main_world: bool,
    scratch: usize,
    started: bool,
    row_live: bool,
    assembled: bool,
    complete: bool,
    cancelled: bool,
}
impl FixedImageOwnership {
    pub fn new(pixels: usize, main_world: bool, scratch: usize) -> Result<Self, ReferenceError> {
        pixels
            .checked_mul(3)
            .and_then(|n| n.checked_add(scratch))
            .ok_or_else(overflow)?;
        Ok(Self {
            pixels,
            main_world,
            scratch,
            started: false,
            row_live: false,
            assembled: false,
            complete: false,
            cancelled: false,
        })
    }
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
    pub fn bytes(&self) -> (usize, usize) {
        if !self.started {
            return (0, 0);
        }
        let cpu = if self.complete {
            if self.main_world { self.pixels } else { 0 }
        } else if self.cancelled && !self.assembled {
            self.pixels
        } else {
            3 * self.pixels
        };
        (
            cpu + if self.row_live { self.scratch } else { 0 },
            self.pixels,
        )
    }
    pub fn io(&self) -> (usize, usize, usize) {
        (
            usize::from(self.row_live),
            usize::from(self.started && !self.complete),
            if self.row_live { self.scratch } else { 0 },
        )
    }
}

pub fn fixed_room(
    ledger: &mut PresentationLedger,
    cpu: usize,
    gpu: usize,
    staging: usize,
    pending: bool,
) -> Result<bool, ReferenceError> {
    if cpu > 256 * 1024 * 1024 || gpu > 512 * 1024 * 1024 || staging > 32 * 1024 * 1024 {
        return Err(ReferenceError("Fixed input exceeds shared limits".into()));
    }
    // Every request runs, so the report asks for all missing credit at once.
    let jobs = staging == 0 || ledger.available_external_jobs() > 0;
    let pending = !pending || ledger.can_request_independent(1);
    let staging = ledger.request_staging(staging);
    let prototype = ledger.request_prototype(cpu, gpu);
    Ok(jobs && pending && staging && prototype)
}
