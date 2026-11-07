//! Renderer counts of fixed Bevy imports.
use super::PresentationLedger;
use crate::reference::ReferenceError;

fn invalid(field: &str) -> ReferenceError {
    ReferenceError(format!("Invalid reference field: {field}"))
}

/// Phase of one fixed image. Rows load one at a time, then the image is assembled and uploaded.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum FixedImage {
    #[default]
    Idle,
    RowLive,
    /// At least one row finished and no row is live.
    RowDone,
    Assembled,
    Complete,
    /// Cancelled after a row started. A live row stays live until `finish_row` ends it.
    Cancelled {
        row_live: bool,
    },
    /// Cancelled and retired.
    Discarded,
}

/// Production lifetime of one fixed assembled image.
#[derive(Clone, Copy, Debug, Default)]
pub struct FixedImageOwnership {
    phase: FixedImage,
}
impl FixedImageOwnership {
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
    /// Loads in flight and pending claims.
    pub fn io(&self) -> (usize, usize) {
        let held = !matches!(
            self.phase,
            FixedImage::Idle | FixedImage::Complete | FixedImage::Discarded
        );
        let live = matches!(
            self.phase,
            FixedImage::RowLive | FixedImage::Cancelled { row_live: true }
        );
        (usize::from(live), usize::from(held))
    }
}

/// True if a fixed load fits the renderer caps. `job`: it starts a load. `pending`: it takes
/// a claim.
pub fn fixed_room(ledger: &PresentationLedger, job: bool, pending: bool) -> bool {
    (!job || ledger.available_external_jobs() > 0)
        && (!pending || ledger.can_request_independent(1))
}

#[cfg(test)]
mod tests {
    use super::FixedImageOwnership;

    #[test]
    fn assemble_while_row_live_errors() {
        let mut image = FixedImageOwnership::default();
        assert!(image.begin_row().unwrap());
        assert!(image.assemble().is_err());
    }

    #[test]
    fn upload_before_assemble_errors() {
        let mut image = FixedImageOwnership::default();
        assert!(image.begin_row().unwrap());
        image.finish_row(true);
        assert!(image.uploaded().is_err());
    }
}
