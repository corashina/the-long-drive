//! Selected descriptor states and their renderer counts, independent of the asset backend.
use crate::ownership::{PresentationLedger, RendererCounts};
use crate::reference::prototype_plan::PrototypePlan;
use std::sync::Arc;

pub struct RetainedPlan<P> {
    pub handle: P,
    pub data: Arc<PrototypePlan>,
}
impl<P> std::ops::Deref for RetainedPlan<P> {
    type Target = PrototypePlan;
    fn deref(&self) -> &Self::Target {
        &self.data
    }
}
pub struct LoadedPrototype<P, A> {
    pub plan: RetainedPlan<P>,
    pub asset: A,
}

pub enum SelectedMetadata<P, A> {
    Queued,
    PlanLoading(P),
    Planned(RetainedPlan<P>),
    Loading { plan: RetainedPlan<P>, asset: A },
    Ready(Arc<LoadedPrototype<P, A>>),
}
impl<P, A> SelectedMetadata<P, A> {
    pub fn plan(&self) -> Option<&PrototypePlan> {
        match self {
            Self::Planned(plan) | Self::Loading { plan, .. } => Some(plan),
            Self::Ready(loaded) => Some(&loaded.plan),
            _ => None,
        }
    }
    pub fn complete_metadata(&mut self, data: Arc<PrototypePlan>) {
        let Self::PlanLoading(handle) = std::mem::replace(self, Self::Queued) else {
            panic!("Metadata completion requires an admitted load")
        };
        *self = Self::Planned(RetainedPlan { handle, data });
    }
    pub fn start_payload(&mut self, asset: A) {
        let Self::Planned(plan) = std::mem::replace(self, Self::Queued) else {
            panic!("Payload requires retained metadata")
        };
        *self = Self::Loading { plan, asset };
    }
    pub fn complete_payload(&mut self) {
        let Self::Loading { plan, asset } = std::mem::replace(self, Self::Queued) else {
            panic!("Payload completion requires an admitted load")
        };
        *self = Self::Ready(Arc::new(LoadedPrototype { plan, asset }));
    }
    pub fn io(&self) -> RendererCounts {
        match self {
            Self::Queued => RendererCounts {
                jobs: 0,
                pending: 1,
            },
            Self::PlanLoading(_) | Self::Loading { .. } => RendererCounts {
                jobs: 1,
                pending: 1,
            },
            Self::Planned(_) | Self::Ready(_) => RendererCounts::default(),
        }
    }
}

/// True if a selected input load fits the renderer caps. A queued input holds its claim.
pub fn input_room(ledger: &PresentationLedger, queued: bool) -> bool {
    ledger.available_external_jobs() > 0 && (queued || ledger.can_request_independent(1))
}
