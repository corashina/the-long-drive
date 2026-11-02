//! Selected descriptor ownership and shared IO accounting, independent of the asset backend.
use crate::ownership::PresentationLedger;
use crate::{
    reference::{FileCatalog, ReferenceError, prototype_plan::PrototypePlan},
    world::PresentationAllocation,
};
use std::sync::Arc;

pub fn control_bytes<E>(
    entries: usize,
    sources: usize,
    retired: usize,
    retired_sources: usize,
) -> usize {
    entries * std::mem::size_of::<E>()
        + sources
        + retired * std::mem::size_of::<crate::reference::SourceId>()
        + retired_sources
}

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
/// The Arc header and the SourcePlan asset retain storage outside the parsed plan.
pub fn retained_plan_bytes(plan: &PrototypePlan) -> usize {
    plan.retained_bytes()
        + 2 * std::mem::size_of::<usize>()
        + std::mem::size_of::<Arc<PrototypePlan>>()
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
    pub fn ready_control_bytes() -> usize {
        std::mem::size_of::<LoadedPrototype<P, A>>() + 2 * std::mem::size_of::<usize>()
    }
    pub fn payload_reservation(&self) -> Result<Reservation, ReferenceError> {
        let plan = self
            .plan()
            .ok_or_else(|| ReferenceError("Missing selected metadata".into()))?;
        let mut reservation = Reservation::payload(plan)?;
        reservation.cpu = reservation
            .cpu
            .checked_add(Self::ready_control_bytes())
            .ok_or_else(|| ReferenceError("Prototype owner overflow".into()))?;
        Reservation::checked(reservation.cpu, reservation.gpu, reservation.staging)
    }
    pub fn complete_payload(&mut self) {
        let Self::Loading { plan, asset } = std::mem::replace(self, Self::Queued) else {
            panic!("Payload completion requires an admitted load")
        };
        *self = Self::Ready(Arc::new(LoadedPrototype { plan, asset }));
    }
    /// A retired physical load still owns its IO charge until the asset backend settles.
    pub fn can_retire(&self, terminal: bool) -> bool {
        !matches!(self, Self::PlanLoading(_) | Self::Loading { .. }) || terminal
    }
    pub fn io(&self, staging: usize) -> IoLedger {
        match self {
            Self::Queued => IoLedger {
                pending: 1,
                ..Default::default()
            },
            Self::PlanLoading(_) | Self::Loading { .. } => IoLedger {
                jobs: 1,
                pending: 1,
                staging,
            },
            Self::Planned(_) | Self::Ready(_) => IoLedger::default(),
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct IoLedger {
    pub jobs: usize,
    pub pending: usize,
    pub staging: usize,
}
impl IoLedger {
    pub fn add(&mut self, other: Self) {
        self.jobs += other.jobs;
        self.pending += other.pending;
        self.staging += other.staging;
    }
    pub fn replace(
        self,
        old: Self,
        mut allocation: PresentationAllocation,
    ) -> PresentationAllocation {
        allocation.jobs = allocation.jobs.saturating_sub(old.jobs) + self.jobs;
        allocation.pending = allocation.pending.saturating_sub(old.pending) + self.pending;
        allocation.staging_bytes =
            allocation.staging_bytes.saturating_sub(old.staging) + self.staging;
        allocation.dependency_staging_bytes = allocation
            .dependency_staging_bytes
            .saturating_sub(old.staging)
            + self.staging;
        allocation
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Reservation {
    pub cpu: usize,
    pub gpu: usize,
    pub staging: usize,
}
impl Reservation {
    pub fn metadata(catalog: &FileCatalog, path: &str) -> Result<Self, ReferenceError> {
        let bytes = catalog.plan(path)?.bytes as usize;
        let cpu = bytes
            .checked_mul(18)
            .ok_or_else(|| ReferenceError("Plan metadata overflow".into()))?;
        let staging = bytes
            .checked_mul(2)
            .ok_or_else(|| ReferenceError("Plan staging overflow".into()))?;
        catalog.check_plan_reservation(path, cpu, staging)?;
        Self::checked(cpu, 0, staging)
    }
    pub fn payload(plan: &PrototypePlan) -> Result<Self, ReferenceError> {
        let (cpu, gpu) = plan.reservation()?;
        let cpu = cpu
            .checked_add(retained_plan_bytes(plan))
            .ok_or_else(|| ReferenceError("Plan ownership overflow".into()))?;
        let staging = plan
            .json_bytes
            .checked_add(plan.buffer_bytes)
            .and_then(|n| n.checked_mul(2))
            .ok_or_else(|| ReferenceError("Prototype staging overflow".into()))?;
        Self::checked(cpu, gpu, staging)
    }
    fn checked(cpu: usize, gpu: usize, staging: usize) -> Result<Self, ReferenceError> {
        if cpu > 256 * 1024 * 1024
            || gpu > 512 * 1024 * 1024
            || staging == 0
            || staging > 32 * 1024 * 1024
        {
            return Err(ReferenceError(
                "Selected input exceeds shared resource limits".into(),
            ));
        }
        Ok(Self { cpu, gpu, staging })
    }
    /// True if the input fits the credit. `old_cpu` and `old_gpu` are already charged.
    pub fn fits(
        self,
        ledger: &mut PresentationLedger,
        old_cpu: usize,
        old_gpu: usize,
        queued: bool,
    ) -> bool {
        // Every request runs, so the report asks for all missing credit at once.
        let jobs = ledger.available_external_jobs() > 0;
        let pending = queued || ledger.can_request_independent(1);
        let staging = ledger.request_staging(self.staging);
        let prototype = ledger.request_prototype(
            self.cpu.saturating_sub(old_cpu),
            self.gpu.saturating_sub(old_gpu),
        );
        jobs && pending && staging && prototype
    }
}
