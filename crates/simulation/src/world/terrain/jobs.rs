use super::{TerrainPoll, TerrainWorker};
use crate::reference::ReferenceError;
use crate::world::{SurfaceKey, TerrainTile, WorldRequest, WorldSurface};

#[derive(Default)]
pub(in crate::world) struct TerrainExecution {
    worker: TerrainWorker,
    pending: Option<PendingTerrain>,
}

struct PendingTerrain {
    metadata: TerrainMetadata,
    input: Vec<u8>,
    submission: Submission,
}

enum Submission {
    Queued,
    Submitted(JobId),
}

#[derive(Clone, Copy, PartialEq, Eq)]
struct JobId(u32);

pub(in crate::world) struct TerrainMetadata {
    pub request: WorldRequest,
    pub key: SurfaceKey,
    pub tile: TerrainTile,
    pub pending_bytes: usize,
}

pub(in crate::world) struct CompletedTerrain {
    pub metadata: TerrainMetadata,
    pub surface: WorldSurface,
}

impl TerrainExecution {
    pub(in crate::world) fn key(&self) -> Option<SurfaceKey> {
        self.pending.as_ref().map(|pending| pending.metadata.key)
    }

    pub(in crate::world) fn has_pending(&self) -> bool {
        self.pending.is_some()
    }

    pub(in crate::world) fn pending_bytes(&self) -> usize {
        self.pending
            .as_ref()
            .map_or(0, |pending| pending.metadata.pending_bytes)
    }

    pub(in crate::world) fn reconcile(&mut self, request: WorldRequest, retain: bool) {
        let Some(pending) = &mut self.pending else {
            return;
        };
        if retain && pending.metadata.request.session == request.session {
            pending.metadata.request = request;
        } else {
            self.pending = None;
            self.worker.discard();
        }
    }

    pub(in crate::world) fn enqueue(&mut self, metadata: TerrainMetadata, input: Vec<u8>) {
        assert!(
            self.pending.is_none(),
            "Terrain execution already has a job"
        );
        self.pending = Some(PendingTerrain {
            metadata,
            input,
            submission: Submission::Queued,
        });
    }

    pub(in crate::world) fn advance(
        &mut self,
        request: WorldRequest,
        should_yield: impl FnMut() -> bool,
    ) -> Result<Option<CompletedTerrain>, ReferenceError> {
        let mut poll = self.worker.poll(should_yield)?;
        if self.pending.as_ref().is_some_and(|pending| {
            pending.metadata.request.session != request.session
                || pending.metadata.request.revision != request.revision
        }) {
            self.pending = None;
            self.worker.discard();
            return Ok(None);
        }
        if let TerrainPoll::Ready(id) = poll {
            let owner = self.pending.as_ref().is_some_and(|pending| {
                matches!(&pending.submission, Submission::Submitted(job) if *job == JobId(id))
            });
            if owner {
                let pending = self.pending.take().unwrap();
                let mut surface = WorldSurface::default();
                self.worker.read(
                    pending.metadata.key.unit,
                    pending.metadata.tile,
                    &mut surface,
                );
                return Ok(Some(CompletedTerrain {
                    metadata: pending.metadata,
                    surface,
                }));
            }
            self.worker.discard();
            poll = TerrainPoll::Idle;
        }
        if poll == TerrainPoll::Idle
            && let Some(pending) = &mut self.pending
            && matches!(pending.submission, Submission::Queued)
        {
            pending.submission = Submission::Submitted(JobId(self.worker.submit(&pending.input)));
        }
        Ok(None)
    }
}
