//! Source probe capture stages. GPU work stays in `probes.rs`.
use crate::reference::reflections::ProbeJob;

/// One capture job moves through these stages in order.
#[derive(Default, Debug, Clone, Copy, PartialEq)]
enum ProbeStage {
    #[default]
    Idle,
    /// The commands of this job are encoded and wait for submission.
    Encoded(u32, ProbeJob),
    /// The filter submission waits for GPU validation.
    Validating(u32),
    /// The capture is valid. Publication is not encoded yet.
    Approved(u32),
    /// The publication copy is encoded and waits for its submission.
    Publishing(u32),
}

/// Capture completion is separate from the main-world eight-job scheduler.
/// A failed capture leaves the last published cube valid.
#[derive(Default, Debug)]
pub struct ProbePublication {
    faces: u8,
    mips: u8,
    stage: ProbeStage,
    published: bool,
    generation: u32,
}
impl ProbePublication {
    pub fn face_mask(&self) -> u8 {
        self.faces
    }
    pub fn mip_mask(&self) -> u8 {
        self.mips
    }
    pub fn published(&self) -> bool {
        self.published
    }
    pub fn generation(&self) -> u32 {
        self.generation
    }
    pub fn validation_token(&self) -> Option<u32> {
        match self.stage {
            ProbeStage::Validating(token) => Some(token),
            _ => None,
        }
    }
    pub fn waiting(&self) -> bool {
        self.stage != ProbeStage::Idle
    }
    /// Record a fully encoded job. Call before its commands enter a submission.
    pub fn encoded(&mut self, token: u32, job: ProbeJob) -> Result<(), &'static str> {
        if token == 0 || self.waiting() {
            return Err("Source probe has a pending job or invalid token");
        }
        let valid = match job {
            // Face 0 starts a new capture. It drops the faces of an abandoned capture.
            ProbeJob::Face(0) => {
                self.discard_capture();
                true
            }
            ProbeJob::Face(face) if face < 6 => self.faces == (1u8 << face) - 1 && self.mips == 0,
            ProbeJob::FirstMip => self.faces == 0x3f && self.mips == 1,
            ProbeJob::RemainingMipsAndPublish => self.faces == 0x3f && self.mips == 3,
            _ => false,
        };
        if !valid {
            return Err("Source probe face or mip work is incomplete");
        }
        self.stage = ProbeStage::Encoded(token, job);
        Ok(())
    }
    /// Acknowledge exactly the job whose complete commands entered the queue.
    /// Return true for jobs that the main-world scheduler can advance now.
    pub fn submitted(&mut self, token: u32) -> Result<bool, &'static str> {
        let ProbeStage::Encoded(expected, job) = self.stage else {
            return Err("Source probe submission has no encoded job");
        };
        if token != expected {
            return Err("Source probe submission token differs");
        }
        self.stage = ProbeStage::Idle;
        match job {
            ProbeJob::Face(face) => {
                self.faces |= 1 << face;
                if self.faces == 0x3f {
                    self.mips = 1;
                }
                Ok(true)
            }
            ProbeJob::FirstMip => {
                self.mips |= 2;
                Ok(true)
            }
            ProbeJob::RemainingMipsAndPublish => {
                self.mips = 0x7f;
                self.stage = ProbeStage::Validating(token);
                Ok(false)
            }
        }
    }
    /// Call after deferred device validation and completion for the filter submission.
    pub fn validated(&mut self, token: u32, success: bool) -> Result<(), &'static str> {
        if self.stage != ProbeStage::Validating(token) {
            return Err("Source probe validation token differs");
        }
        if !success {
            self.discard_capture();
            return Err("Source probe capture failed GPU validation");
        }
        if self.faces != 0x3f || self.mips != 0x7f {
            return Err("Source probe publication is incomplete");
        }
        self.stage = ProbeStage::Approved(token);
        Ok(())
    }
    /// Encode publication only after capture validation. Retain the texture allocation.
    /// `publish` encodes the copy and returns false while its pipeline is pending.
    pub fn encode_publication(
        &mut self,
        publish: impl FnOnce() -> bool,
    ) -> Result<u32, &'static str> {
        let ProbeStage::Approved(token) = self.stage else {
            return Err("Source probe capture is not validated");
        };
        if !publish() {
            return Err("Source probe copy pipeline is pending");
        }
        self.stage = ProbeStage::Publishing(token);
        Ok(token)
    }
    /// After this acknowledgement, advance job eight in the main-world scheduler.
    pub fn publication_submitted(&mut self, token: u32) -> Result<(), &'static str> {
        if self.stage != ProbeStage::Publishing(token) {
            return Err("Source probe publication token differs");
        }
        self.generation = self
            .generation
            .checked_add(1)
            .ok_or("Source probe generation exhausted")?;
        self.published = true;
        self.discard_capture();
        Ok(())
    }
    /// Retain the prior valid publication when capture work fails or restarts.
    pub fn discard_capture(&mut self) {
        self.faces = 0;
        self.mips = 0;
        self.stage = ProbeStage::Idle;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn capture(publication: &mut ProbePublication, token: u32) {
        for face in 0..6 {
            publication.encoded(token, ProbeJob::Face(face)).unwrap();
            assert!(publication.submitted(token).unwrap());
        }
        publication.encoded(token, ProbeJob::FirstMip).unwrap();
        assert!(publication.submitted(token).unwrap());
        publication
            .encoded(token, ProbeJob::RemainingMipsAndPublish)
            .unwrap();
        assert!(!publication.submitted(token).unwrap());
    }

    #[test]
    fn stages_run_in_order() {
        let mut publication = ProbePublication::default();
        capture(&mut publication, 7);
        assert_eq!(publication.validation_token(), Some(7));
        assert!(publication.encode_publication(|| true).is_err());
        publication.validated(7, true).unwrap();
        assert_eq!(
            publication.encode_publication(|| false),
            Err("Source probe copy pipeline is pending")
        );
        assert_eq!(publication.encode_publication(|| true), Ok(7));
        publication.publication_submitted(7).unwrap();
        assert!(publication.published() && !publication.waiting());
        assert_eq!(publication.generation(), 1);
        assert_eq!((publication.face_mask(), publication.mip_mask()), (0, 0));
    }

    #[test]
    fn wrong_token_keeps_stage() {
        let mut publication = ProbePublication::default();
        publication.encoded(3, ProbeJob::Face(0)).unwrap();
        assert_eq!(
            publication.submitted(4),
            Err("Source probe submission token differs")
        );
        assert!(publication.submitted(3).unwrap());
        for face in 1..6 {
            publication.encoded(3, ProbeJob::Face(face)).unwrap();
            publication.submitted(3).unwrap();
        }
        publication.encoded(3, ProbeJob::FirstMip).unwrap();
        publication.submitted(3).unwrap();
        publication
            .encoded(3, ProbeJob::RemainingMipsAndPublish)
            .unwrap();
        publication.submitted(3).unwrap();
        assert_eq!(
            publication.validated(4, true),
            Err("Source probe validation token differs")
        );
        publication.validated(3, true).unwrap();
        publication.encode_publication(|| true).unwrap();
        assert_eq!(
            publication.publication_submitted(4),
            Err("Source probe publication token differs")
        );
        assert!(publication.waiting() && !publication.published());
    }
}
