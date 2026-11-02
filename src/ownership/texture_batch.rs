//! Two ordered batch owners. Loading and submitted cancellation retain their charges.
use crate::reference::texture_batch::{TextureBatchReference, TextureBatchRegion};
pub const STAGING_LIMIT: usize = 32 * 1024 * 1024;
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BatchIdentity {
    pub session: u64,
    pub request: u64,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BatchState {
    Queued,
    Loading,
    Ready,
    Submitted,
    Complete,
    Failed,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Admission {
    Wait,
    Permanent,
}
#[derive(Clone, Copy, Debug)]
pub struct BatchOwner {
    pub identity: BatchIdentity,
    pub state: BatchState,
    pub cancelled: bool,
    pub bytes: usize,
    pub shared_with: Option<BatchIdentity>,
    shared_bytes: usize,
}
#[derive(Default)]
pub struct BatchQueue {
    pub slots: [Option<BatchOwner>; 2],
}
impl BatchQueue {
    pub fn bytes(&self) -> usize {
        self.slots.iter().flatten().map(|o| o.bytes).sum()
    }
    pub fn jobs(&self) -> usize {
        self.slots
            .iter()
            .flatten()
            .filter(|o| o.state == BatchState::Loading)
            .count()
    }
    pub fn admit(
        &mut self,
        requests: impl IntoIterator<Item = (BatchIdentity, usize)>,
        available: usize,
        releasable: bool,
    ) -> Result<(), Admission> {
        let mut proposed = [None; 2];
        let mut total = 0usize;
        for (i, (identity, bytes)) in requests.into_iter().enumerate() {
            if i >= 2 {
                return Err(Admission::Permanent);
            }
            total = total.checked_add(bytes).ok_or(Admission::Permanent)?;
            proposed[i] = Some(BatchOwner {
                identity,
                bytes,
                state: BatchState::Queued,
                cancelled: false,
                shared_with: None,
                shared_bytes: 0,
            });
        }
        if total > STAGING_LIMIT {
            return Err(Admission::Permanent);
        }
        if self.slots.iter().any(Option::is_some) || total > available {
            return Err(if releasable || self.slots.iter().any(Option::is_some) {
                Admission::Wait
            } else {
                Admission::Permanent
            });
        }
        self.slots = proposed;
        Ok(())
    }
    /// Reserve a new prefetch without changing the retained current owner.
    pub fn refill(
        &mut self,
        identity: BatchIdentity,
        bytes: usize,
        available: usize,
    ) -> Result<(), Admission> {
        let Some(current) = self.slots[0] else {
            return Err(Admission::Permanent);
        };
        if self.slots[1].is_some() {
            return Err(Admission::Wait);
        }
        if current
            .bytes
            .checked_add(bytes)
            .is_none_or(|n| n > STAGING_LIMIT)
        {
            return Err(Admission::Permanent);
        }
        if bytes > available {
            return Err(Admission::Wait);
        }
        self.slots[1] = Some(BatchOwner {
            identity,
            bytes,
            state: BatchState::Queued,
            cancelled: false,
            shared_with: None,
            shared_bytes: 0,
        });
        Ok(())
    }
    pub fn start(&mut self, available_jobs: usize) -> Vec<BatchIdentity> {
        let mut started = Vec::new();
        let free = available_jobs.min(2usize.saturating_sub(self.jobs()));
        for o in self
            .slots
            .iter_mut()
            .flatten()
            .filter(|o| o.state == BatchState::Queued && !o.cancelled)
            .take(free)
        {
            o.state = BatchState::Loading;
            started.push(o.identity);
        }
        started
    }
    /// The second strong handle shares input and payload storage, but owns its control bytes.
    pub fn share(&mut self, first: BatchIdentity, second: BatchIdentity, shared_bytes: usize) {
        assert!(self.owner(first).is_some());
        let other = self
            .slots
            .iter_mut()
            .flatten()
            .find(|o| o.identity == second)
            .unwrap();
        assert_eq!(other.state, BatchState::Queued);
        other.shared_with = Some(first);
        other.shared_bytes = shared_bytes;
    }
    pub fn owner(&self, identity: BatchIdentity) -> Option<&BatchOwner> {
        self.slots.iter().flatten().find(|o| o.identity == identity)
    }
    pub fn ready(&mut self, identity: BatchIdentity) {
        if let Some(o) = self
            .slots
            .iter_mut()
            .flatten()
            .find(|o| o.identity == identity && o.state == BatchState::Loading)
        {
            o.state = if o.cancelled {
                BatchState::Complete
            } else {
                BatchState::Ready
            };
        }
    }
    pub fn fail(&mut self, identity: BatchIdentity) {
        if let Some(o) = self
            .slots
            .iter_mut()
            .flatten()
            .find(|o| o.identity == identity && o.state != BatchState::Submitted)
        {
            o.state = BatchState::Failed;
        }
    }
    pub fn submit(&mut self, identity: BatchIdentity) -> bool {
        if let Some(o) = self.slots[0]
            .as_mut()
            .filter(|o| o.identity == identity && o.state == BatchState::Ready && !o.cancelled)
        {
            o.state = BatchState::Submitted;
            true
        } else {
            false
        }
    }
    pub fn acknowledge(&mut self, identity: BatchIdentity) -> bool {
        if let Some(o) = self
            .slots
            .iter_mut()
            .flatten()
            .find(|o| o.identity == identity && o.state == BatchState::Submitted)
        {
            o.state = BatchState::Complete;
            true
        } else {
            false
        }
    }
    pub fn cancel(&mut self, session: u64) {
        for o in self
            .slots
            .iter_mut()
            .flatten()
            .filter(|o| o.identity.session == session)
        {
            o.cancelled = true;
            if matches!(o.state, BatchState::Queued | BatchState::Ready) {
                o.state = BatchState::Complete;
            }
        }
    }
    pub fn release(&mut self, identity: BatchIdentity) -> Option<usize> {
        let i = self.slots.iter().position(|o| {
            o.is_some_and(|o| {
                o.identity == identity
                    && matches!(o.state, BatchState::Complete | BatchState::Failed)
            })
        })?;
        let bytes = self.slots[i].take()?.bytes;
        let mut transferred = 0;
        for other in self
            .slots
            .iter_mut()
            .flatten()
            .filter(|o| o.shared_with == Some(identity))
        {
            transferred += other.shared_bytes;
            other.bytes += other.shared_bytes;
            other.shared_with = None;
            other.shared_bytes = 0;
        }
        if i == 0 {
            self.slots[0] = self.slots[1].take();
        }
        Some(bytes - transferred)
    }
}
/// Checked source IO holds JS input, a Rust reader and decoder input concurrently.
/// Keep parsed receipts, handles and queue staging until acknowledgement.
pub fn reservation(batch: &TextureBatchReference) -> Option<usize> {
    (batch.descriptor.bytes as usize)
        .checked_mul(4)?
        .checked_add(batch.largest_raw_input_bytes.checked_mul(4)?)?
        .checked_add(batch.decoded_bytes.checked_mul(2)?)?
        .checked_add(
            batch
                .region_count
                .checked_mul(std::mem::size_of::<TextureBatchRegion>() * 2)?,
        )?
        .checked_add(4096)
}
