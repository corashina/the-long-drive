//! Counted renderer owners serialize pipeline work through their existing pending claim.
use super::{PresentationLedger, upload_receipt::UploadReceipt};
use crate::reference::ReferenceError;
use crate::world::{GeneratedId, RendererWorld, SurfaceKey, WorldRequest};

pub enum RendererOwner {
    Surface(WorldRequest, SurfaceKey),
    Structure(WorldRequest, GeneratedId),
    Grass(WorldRequest, u64),
}
impl RendererOwner {
    fn request(&self) -> WorldRequest {
        match self {
            Self::Surface(r, _) | Self::Structure(r, _) | Self::Grass(r, _) => *r,
        }
    }
}
/// This proof has no Clone implementation. The renderer makes one per counted owner.
pub struct PipelineContinuation {
    pub(super) owner: RendererOwner,
    held: bool,
    flight: Option<UploadReceipt>,
    upload: Option<UploadReceipt>,
}
impl PipelineContinuation {
    pub fn counted(owner: RendererOwner) -> Self {
        Self {
            owner,
            held: true,
            flight: None,
            upload: None,
        }
    }
    pub fn request(&self) -> WorldRequest {
        self.owner.request()
    }
    pub fn watch_upload(&mut self, receipt: UploadReceipt) {
        self.upload = Some(receipt);
    }
    pub fn drained(&self) -> bool {
        self.flight.as_ref().is_none_or(UploadReceipt::completed)
            && self.upload.as_ref().is_none_or(UploadReceipt::completed)
    }
    pub fn release(&mut self) -> Result<(), ReferenceError> {
        if !self.held || !self.drained() {
            return Err(ReferenceError("Renderer claim is live or released".into()));
        }
        self.held = false;
        Ok(())
    }
    pub fn validate(&self, stream: &RendererWorld) -> Result<(), ReferenceError> {
        if !self.held
            || self.request().session != stream.frame().request.session
            || self.flight.as_ref().is_some_and(UploadReceipt::stopped)
        {
            return Err(ReferenceError("Stale renderer continuation proof".into()));
        }
        Ok(())
    }
    pub fn stop_flight(&self) {
        if let Some(flight) = &self.flight {
            flight.stop();
        }
    }
    pub fn shares(&self, receipt: &UploadReceipt) -> bool {
        self.flight
            .as_ref()
            .is_some_and(|flight| flight.same(receipt))
    }
    pub fn start(
        &mut self,
        stream: &RendererWorld,
        ledger: &mut PresentationLedger,
    ) -> Result<Option<PipelineOperation>, ReferenceError> {
        self.validate(stream)?;
        if self.flight.as_ref().is_some_and(|f| !f.completed()) {
            return Ok(None);
        }
        let operation = PipelineOperation::admit(ledger, false);
        if let Some(operation) = &operation {
            self.flight = Some(operation.physical.clone());
        }
        Ok(operation)
    }
}
/// A shared ticket retains one physical operation. Copies add no pending claim.
#[derive(Clone)]
pub struct PipelineOperation {
    pub owns_pending: bool,
    pub physical: UploadReceipt,
}
impl PipelineOperation {
    /// Fails if the batch already finished or stopped.
    pub fn live(&self) -> Result<(), ReferenceError> {
        if self.physical.completed() || self.physical.stopped() {
            return Err(ReferenceError("Stale source pipeline batch".into()));
        }
        Ok(())
    }
    pub fn independent(ledger: &mut PresentationLedger) -> Option<Self> {
        Self::admit(ledger, true)
    }
    fn admit(ledger: &mut PresentationLedger, owns_pending: bool) -> Option<Self> {
        let job = ledger.available_external_jobs() > 0;
        let claim = !owns_pending || ledger.can_request_independent(1);
        if !(job && claim) {
            return None;
        }
        ledger.start_jobs(1);
        ledger.add_pending(usize::from(owns_pending));
        Some(Self {
            owns_pending,
            physical: UploadReceipt::default(),
        })
    }
}
/// Byte cap of a pipeline diagnostic. It allocates only when a diagnostic exists.
pub const PIPELINE_DIAGNOSTIC_BYTES: usize = 4096;
const DIAGNOSTIC_TRUNCATED: &str = " [truncated]";
struct PipelineDiagnosticWriter<'a> {
    text: &'a mut String,
    truncated: bool,
}
impl std::fmt::Write for PipelineDiagnosticWriter<'_> {
    fn write_str(&mut self, value: &str) -> std::fmt::Result {
        if self.truncated {
            return Ok(());
        }
        if value.len() <= PIPELINE_DIAGNOSTIC_BYTES - self.text.len() {
            self.text.push_str(value);
            return Ok(());
        }
        let limit = PIPELINE_DIAGNOSTIC_BYTES - DIAGNOSTIC_TRUNCATED.len();
        if self.text.len() > limit {
            let mut end = limit;
            while !self.text.is_char_boundary(end) {
                end -= 1;
            }
            self.text.truncate(end);
        }
        let mut end = (limit - self.text.len()).min(value.len());
        while !value.is_char_boundary(end) {
            end -= 1;
        }
        self.text.push_str(&value[..end]);
        self.text.push_str(DIAGNOSTIC_TRUNCATED);
        self.truncated = true;
        Ok(())
    }
}
pub fn write_pipeline_diagnostic(output: &mut Option<String>, args: std::fmt::Arguments<'_>) {
    let text = output.get_or_insert_with(|| String::with_capacity(PIPELINE_DIAGNOSTIC_BYTES));
    assert_eq!(text.capacity(), PIPELINE_DIAGNOSTIC_BYTES);
    text.clear();
    let mut writer = PipelineDiagnosticWriter {
        text,
        truncated: false,
    };
    std::fmt::write(&mut writer, args).expect("pipeline diagnostic formatter failed");
    assert!(writer.text.len() <= PIPELINE_DIAGNOSTIC_BYTES);
    assert_eq!(writer.text.capacity(), PIPELINE_DIAGNOSTIC_BYTES);
}
pub fn pipeline_diagnostic(args: std::fmt::Arguments<'_>) -> String {
    let mut output = None;
    write_pipeline_diagnostic(&mut output, args);
    output.unwrap()
}
pub fn pipeline_pair_ready(receipt: &UploadReceipt, spill: bool) -> bool {
    !spill && !receipt.stopped() && receipt.completed()
}
/// Base requests include cache hits. Companions retain their existing constructors.
pub const PIPELINE_BATCH_KEYS: usize = 32;
#[derive(Default)]
pub struct PipelineBatchWork {
    pub pairs: usize,
    pub base_keys: usize,
}
impl PipelineBatchWork {
    pub fn accept(&mut self, keys: usize) -> Result<bool, ReferenceError> {
        if keys == 0 || keys > PIPELINE_BATCH_KEYS {
            return Err(ReferenceError(
                "Source material pipeline reservation exceeded".into(),
            ));
        }
        if self.base_keys + keys > PIPELINE_BATCH_KEYS {
            return Ok(false);
        }
        self.base_keys += keys;
        self.pairs += 1;
        Ok(true)
    }
}
/// Fixed render-stack keys preserve first occurrence and the shadow and view order.
pub struct PipelineKeys<K> {
    values: [Option<K>; PIPELINE_BATCH_KEYS],
    pub len: usize,
}
impl<K: Copy + PartialEq> PipelineKeys<K> {
    pub fn new() -> Self {
        Self {
            values: [None; PIPELINE_BATCH_KEYS],
            len: 0,
        }
    }
    pub fn push(&mut self, key: K) -> Result<(), ReferenceError> {
        if self.values[..self.len].contains(&Some(key)) {
            return Ok(());
        }
        if self.len == PIPELINE_BATCH_KEYS {
            return Err(ReferenceError(
                "Source material pipeline reservation exceeded".into(),
            ));
        }
        self.values[self.len] = Some(key);
        self.len += 1;
        Ok(())
    }
    pub fn iter(&self) -> impl Iterator<Item = K> + '_ {
        self.values[..self.len].iter().map(|v| v.unwrap())
    }
}
pub struct PipelineBatch<P> {
    pub operation: PipelineOperation,
    pub pairs: [std::sync::Weak<P>; PIPELINE_BATCH_KEYS],
    pub len: usize,
    pub next: usize,
    pub open: bool,
    pub stopped: bool,
    pub work: PipelineBatchWork,
    #[cfg(target_arch = "wasm32")]
    pub dispatched_keys: usize,
}
impl<P> PipelineBatch<P> {
    pub fn new(operation: PipelineOperation) -> Self {
        Self {
            operation,
            pairs: std::array::from_fn(|_| std::sync::Weak::new()),
            len: 0,
            next: 0,
            open: true,
            stopped: false,
            work: PipelineBatchWork::default(),
            #[cfg(target_arch = "wasm32")]
            dispatched_keys: 0,
        }
    }
    pub fn push(&mut self, pair: &std::sync::Arc<P>) {
        assert!(self.open && self.len < PIPELINE_BATCH_KEYS);
        self.pairs[self.len] = std::sync::Arc::downgrade(pair);
        self.len += 1;
    }
    pub fn select(&mut self, keys: Option<usize>) -> Result<bool, ReferenceError> {
        let Some(keys) = keys else {
            self.stopped |= self.next != 0;
            return Ok(false);
        };
        match self.work.accept(keys) {
            Ok(true) => {
                self.next += 1;
                Ok(true)
            }
            Ok(false) => {
                self.stopped = true;
                Ok(false)
            }
            Err(error) => {
                self.stopped = true;
                Err(error)
            }
        }
    }
    pub fn register(&self, settled: impl Iterator<Item = bool>) -> bool {
        let drained = settled.fold(true, |all, pair| pair & all);
        (self.stopped || self.next == self.len) && drained && self.operation.physical.register()
    }
    pub fn retire_pairs(&mut self) {
        if self.operation.physical.completed() {
            for pair in &mut self.pairs[..self.len] {
                if pair.strong_count() <= 1 {
                    *pair = std::sync::Weak::new();
                }
            }
        }
    }
    pub fn retain(&self) -> bool {
        if !self.operation.physical.completed() {
            return true;
        }
        let mut receipt_owners = 1;
        for pair in &self.pairs[..self.len] {
            let owners = pair.strong_count();
            if owners > 1 {
                return true;
            }
            receipt_owners += usize::from(owners != 0);
        }
        // The parent can still retain the completed flight. Charge its control until release.
        self.operation.physical.owners() > receipt_owners
    }
}

/// Jobs and pending claims of the unfinished pipeline operations.
#[derive(Clone, Copy, Default)]
pub struct PipelineAccounting {
    pub jobs: usize,
    pub pending: usize,
}
impl PipelineAccounting {
    pub fn admitted(&mut self, operation: &PipelineOperation) {
        self.jobs += 1;
        self.pending += usize::from(operation.owns_pending);
    }
    pub fn reconcile(
        &mut self,
        ledger: &mut PresentationLedger,
        operations: impl Iterator<Item = PipelineOperation>,
    ) -> Result<(), ReferenceError> {
        let mut next = Self::default();
        for operation in operations {
            if !operation.physical.completed() {
                next.jobs += 1;
                next.pending += usize::from(operation.owns_pending);
            }
        }
        ledger.end_jobs(self.jobs)?;
        ledger.start_jobs(next.jobs);
        ledger.end_pending(self.pending)?;
        ledger.add_pending(next.pending);
        *self = next;
        Ok(())
    }
}

/// Physical drain checks inspect every admitted operation, including siblings after a failure.
#[derive(Clone, Copy)]
pub enum PhysicalPipelineState {
    Queued,
    Compiling,
    Retryable,
    Ready,
    Failed,
}
pub fn pipeline_states_drained(states: impl Iterator<Item = PhysicalPipelineState>) -> bool {
    states.fold(true, |drained, state| {
        matches!(
            state,
            PhysicalPipelineState::Ready | PhysicalPipelineState::Failed
        ) & drained
    })
}
/// Once retirement starts, a returning source must wait for a new parent record.
#[derive(Default)]
pub struct RendererRetirement(bool);
impl RendererRetirement {
    pub fn keep(&mut self, wanted: bool) -> bool {
        self.0 |= !wanted;
        !self.0
    }
    pub fn started(&self) -> bool {
        self.0
    }
    pub fn drained(&self, parent: bool, children: bool) -> bool {
        self.0 && parent && children
    }
}
