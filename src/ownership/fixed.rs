//! Renderer ledger charges for fixed Bevy imports.
use super::PresentationLedger;
use crate::reference::ReferenceError;

fn overflow() -> ReferenceError {
    ReferenceError("Fixed Bevy reservation overflow".into())
}
fn invalid(field: &str) -> ReferenceError {
    ReferenceError(format!("Invalid reference field: {field}"))
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

/// Phase of one fixed image. Rows load one at a time, then the image is assembled and uploaded.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum FixedImage {
    Idle,
    RowLive,
    /// At least one row finished and no row is live.
    RowDone,
    Assembled,
    Complete,
    /// Cancelled after a row started. A live row still holds scratch until `finish_row` ends it.
    Cancelled {
        row_live: bool,
    },
    /// Cancelled and retired. It holds no bytes.
    Discarded,
}

/// Production lifetime of one fixed assembled image and its bounded row scratch.
#[derive(Clone, Copy, Debug)]
pub struct FixedImageOwnership {
    pixels: usize,
    main_world: bool,
    scratch: usize,
    phase: FixedImage,
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
            phase: FixedImage::Idle,
        })
    }
    pub fn begin_row(&mut self) -> Result<bool, ReferenceError> {
        match self.phase {
            FixedImage::Cancelled { .. } | FixedImage::Discarded => Ok(false),
            FixedImage::Idle | FixedImage::RowDone => {
                self.phase = FixedImage::RowLive;
                Ok(true)
            }
            _ => Err(invalid("fixed row admission state")),
        }
    }
    pub fn finish_row(&mut self, terminal: bool) -> bool {
        if terminal {
            self.phase = match self.phase {
                FixedImage::RowLive => FixedImage::RowDone,
                FixedImage::Cancelled { .. } => FixedImage::Cancelled { row_live: false },
                phase => phase,
            };
        }
        terminal
    }
    pub fn assemble(&mut self) -> Result<(), ReferenceError> {
        if self.phase != FixedImage::RowDone {
            return Err(invalid("fixed row assembly state"));
        }
        self.phase = FixedImage::Assembled;
        Ok(())
    }
    pub fn uploaded(&mut self) -> Result<(), ReferenceError> {
        match self.phase {
            FixedImage::Assembled | FixedImage::Complete => {
                self.phase = FixedImage::Complete;
                Ok(())
            }
            _ => Err(invalid("fixed image upload state")),
        }
    }
    /// Stops row loads. An assembled image belongs to its upload, so cancel leaves it.
    pub fn cancel(&mut self) {
        self.phase = match self.phase {
            FixedImage::Idle => FixedImage::Discarded,
            FixedImage::RowLive => FixedImage::Cancelled { row_live: true },
            FixedImage::RowDone => FixedImage::Cancelled { row_live: false },
            phase => phase,
        };
    }
    pub fn discard(&mut self) -> Result<(), ReferenceError> {
        match self.phase {
            FixedImage::Cancelled { row_live: false } | FixedImage::Discarded => {
                self.phase = FixedImage::Discarded;
                Ok(())
            }
            _ => Err(invalid("fixed row physical retirement")),
        }
    }
    fn row_live(&self) -> bool {
        matches!(
            self.phase,
            FixedImage::RowLive | FixedImage::Cancelled { row_live: true }
        )
    }
    pub fn bytes(&self) -> (usize, usize) {
        let cpu = match self.phase {
            FixedImage::Idle | FixedImage::Discarded => return (0, 0),
            FixedImage::Complete => {
                if self.main_world {
                    self.pixels
                } else {
                    0
                }
            }
            FixedImage::Cancelled { .. } => self.pixels,
            FixedImage::RowLive | FixedImage::RowDone | FixedImage::Assembled => 3 * self.pixels,
        };
        (
            cpu + if self.row_live() { self.scratch } else { 0 },
            self.pixels,
        )
    }
    pub fn io(&self) -> (usize, usize, usize) {
        let held = !matches!(
            self.phase,
            FixedImage::Idle | FixedImage::Complete | FixedImage::Discarded
        );
        let live = self.row_live();
        (
            usize::from(live),
            usize::from(held),
            if live { self.scratch } else { 0 },
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

#[cfg(test)]
mod tests {
    use super::FixedImageOwnership;

    #[test]
    fn assemble_while_row_live_errors() {
        let mut image = FixedImageOwnership::new(4, false, 2).unwrap();
        assert!(image.begin_row().unwrap());
        assert!(image.assemble().is_err());
    }

    #[test]
    fn upload_before_assemble_errors() {
        let mut image = FixedImageOwnership::new(4, false, 2).unwrap();
        assert!(image.begin_row().unwrap());
        image.finish_row(true);
        assert!(image.uploaded().is_err());
    }
}
