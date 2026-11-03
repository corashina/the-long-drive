//! The renderer share of the stream budgets: credit, want, cover, reclaim and admission.
//! Pure arithmetic over counts. The stream passes its own use in each call.
use super::{EXTERNAL_JOBS, MAX_PENDING, PROTOTYPE_CPU, PROTOTYPE_GPU, STAGING};
use crate::reference::ReferenceError;
use crate::world::WorldMetrics;
use crate::world::metrics::{BUDGET_LABELS, ResourceBudget};
use std::sync::atomic::{AtomicU8, Ordering};

/// Renderer-owned allocations, including loader staging.
#[derive(Clone, Copy, Debug, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct PresentationAllocation {
    pub grass_worker_wasm_bytes: usize,
    pub grass_worker_cache_bytes: usize,
    pub grass_worker_reserved_bytes: usize,
    pub active_cpu: usize,
    pub active_gpu: usize,
    pub prototype_cpu: usize,
    pub prototype_gpu: usize,
    pub jobs: usize,
    pub pending: usize,
    pub staging_bytes: usize,
    pub dependency_staging_bytes: usize,
    pub objects: usize,
}
impl PresentationAllocation {
    /// Applies `f` to each pair of fields.
    pub fn zip(self, other: Self, f: impl Fn(usize, usize) -> usize) -> Self {
        Self {
            grass_worker_wasm_bytes: f(self.grass_worker_wasm_bytes, other.grass_worker_wasm_bytes),
            grass_worker_cache_bytes: f(
                self.grass_worker_cache_bytes,
                other.grass_worker_cache_bytes,
            ),
            grass_worker_reserved_bytes: f(
                self.grass_worker_reserved_bytes,
                other.grass_worker_reserved_bytes,
            ),
            active_cpu: f(self.active_cpu, other.active_cpu),
            active_gpu: f(self.active_gpu, other.active_gpu),
            prototype_cpu: f(self.prototype_cpu, other.prototype_cpu),
            prototype_gpu: f(self.prototype_gpu, other.prototype_gpu),
            jobs: f(self.jobs, other.jobs),
            pending: f(self.pending, other.pending),
            staging_bytes: f(self.staging_bytes, other.staging_bytes),
            dependency_staging_bytes: f(
                self.dependency_staging_bytes,
                other.dependency_staging_bytes,
            ),
            objects: f(self.objects, other.objects),
        }
    }
}

/// The renderer use of one frame.
#[derive(Clone, Copy, Debug, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct PresentationReport {
    pub used: PresentationAllocation,
    /// Totals the renderer wanted to reach this frame.
    pub want: PresentationAllocation,
    /// Credit the renderer gave back. It no longer spends it.
    pub release: PresentationAllocation,
}
impl PresentationReport {
    /// Adds a later report: its use and wants, and both releases.
    pub fn merge(self, later: Self) -> Self {
        Self {
            used: later.used,
            want: later.want,
            release: self.release.zip(later.release, usize::saturating_add),
        }
    }
}

/// Renderer fields that own stream work waited for this step. The stream grants none of them,
/// and the renderer gives back their unused credit.
#[derive(Clone, Copy, Debug, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Reclaim {
    pub jobs: bool,
    pub pending: bool,
    /// Staging and dependency staging.
    pub staging: bool,
    pub prototype_cpu: bool,
}
impl Reclaim {
    pub(super) const JOBS: u8 = 1;
    pub(super) const PENDING: u8 = 2;
    pub(super) const STAGING: u8 = 4;
    pub(super) const PROTOTYPE_CPU: u8 = 8;
    fn from_bits(bits: u8) -> Self {
        Self {
            jobs: bits & Self::JOBS != 0,
            pending: bits & Self::PENDING != 0,
            staging: bits & Self::STAGING != 0,
            prototype_cpu: bits & Self::PROTOTYPE_CPU != 0,
        }
    }
    /// `allocation` without the fields own work waited for.
    fn keep(self, mut allocation: PresentationAllocation) -> PresentationAllocation {
        if self.jobs {
            allocation.jobs = 0;
        }
        if self.pending {
            allocation.pending = 0;
        }
        if self.staging {
            allocation.staging_bytes = 0;
            allocation.dependency_staging_bytes = 0;
        }
        if self.prototype_cpu {
            allocation.prototype_cpu = 0;
        }
        allocation
    }
    /// Only the fields of `allocation` that own work waited for.
    pub fn take(self, allocation: PresentationAllocation) -> PresentationAllocation {
        allocation.zip(self.keep(allocation), usize::saturating_sub)
    }
}

/// The stream answer: added credit, and the fields own work waits for.
#[derive(Clone, Copy, Debug, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct PresentationGrant {
    pub credit: PresentationAllocation,
    pub reclaim: Reclaim,
    /// The stream lends its publication slot of this frame.
    pub publication: bool,
}
impl PresentationGrant {
    /// Adds a later grant: both credits. The later step decides reclaim and the lend.
    pub fn merge(self, later: Self) -> Self {
        Self {
            credit: self.credit.zip(later.credit, usize::saturating_add),
            reclaim: later.reclaim,
            publication: later.publication,
        }
    }
}

/// Own stream use at one ledger call.
pub(super) struct StreamUse {
    /// Stream totals. They include the current renderer credit.
    pub totals: WorldMetrics,
    /// Generation staging, with the binding reservation.
    pub generation_pending: usize,
    pub binding_reserved: usize,
    /// Own external jobs: terrain, road and replay.
    pub jobs: usize,
    /// External jobs open to the renderer: the initial generation is ready.
    pub jobs_open: bool,
    pub active_cpu_limit: usize,
    pub active_gpu_limit: usize,
    pub objects_limit: usize,
}

/// The renderer credit and the state between its report and the grant.
#[derive(Default)]
pub(super) struct PresentationLedger {
    credit: PresentationAllocation,
    /// Renderer want of the last report above its covered use.
    want: PresentationAllocation,
    /// The last grant: renderer want above its covered use, and the fields own work waited for.
    last_wait: (PresentationAllocation, Reclaim),
    /// Renderer use of the last report above its credit. The grant credits it back.
    cover: PresentationAllocation,
    /// Renderer use of the last report.
    used: PresentationAllocation,
    /// Generation staging kept for renderer dependency loads. Zero: not configured.
    dependency_staging_reserve: usize,
    /// Fields that own work waited for this step, as `Reclaim` bits.
    reclaim: AtomicU8,
}

impl PresentationLedger {
    /// Renderer credit: the renderer share of the budgets.
    pub fn credit(&self) -> PresentationAllocation {
        self.credit
    }
    /// Renderer use of the last report.
    pub fn used(&self) -> PresentationAllocation {
        self.used
    }
    pub fn last_wait(&self) -> (PresentationAllocation, Reclaim) {
        self.last_wait
    }

    pub fn set(
        &mut self,
        own: &StreamUse,
        next: PresentationAllocation,
    ) -> Result<(), ReferenceError> {
        for budget in self.budgets(own, next) {
            if budget.used > budget.limit {
                return Err(ReferenceError(format!(
                    "{} budget exceeded: requested {} {}, limit {} {}.",
                    budget.name, budget.used, budget.unit, budget.limit, budget.unit,
                )));
            }
        }
        self.credit = next;
        Ok(())
    }

    /// Takes the renderer report before own work: given-back credit returns and use is
    /// covered. Use above the credit must fit the limits. Err: the failure message.
    pub fn take_report(
        &mut self,
        own: &StreamUse,
        report: PresentationReport,
    ) -> Result<(), String> {
        let credit = self.credit.zip(report.release, usize::saturating_sub);
        let mut covered = credit.zip(report.used, usize::max);
        // Dependency staging is part of the staging credit. Per-field give-backs can leave
        // more dependency credit than staging credit.
        covered.dependency_staging_bytes = covered
            .dependency_staging_bytes
            .min(covered.staging_bytes)
            .max(report.used.dependency_staging_bytes);
        let result = self.set(own, covered).map_err(|error| {
            self.credit = credit;
            format!(
                "{} Renderer asset CPU: use {} bytes, credit {} bytes.",
                error.0, report.used.prototype_cpu, credit.prototype_cpu,
            )
        });
        self.cover = self.cover.zip(
            covered.zip(credit, usize::saturating_sub),
            usize::saturating_add,
        );
        self.want = report.want.zip(covered, usize::saturating_sub);
        self.used = report.used;
        result
    }

    /// Answers the last report after own work. The renderer gets the credit for its covered
    /// use, and the room that own work left, but no field that own work waited for.
    /// `own` is None when the stream failed: it grants nothing.
    pub fn grant(&mut self, own: Option<&StreamUse>, publication: bool) -> PresentationGrant {
        let reclaim = Reclaim::from_bits(self.reclaim.swap(0, Ordering::Relaxed));
        self.last_wait = (self.want, reclaim);
        let want = reclaim.keep(std::mem::take(&mut self.want));
        let Some(own) = own else {
            return PresentationGrant {
                reclaim,
                ..Default::default()
            };
        };
        let covered = self.credit;
        let mut target = covered.zip(want.zip(self.room(own), usize::min), |a, b| {
            a.saturating_add(b)
        });
        // Dependency staging is part of the staging credit.
        target.dependency_staging_bytes = target
            .dependency_staging_bytes
            .min(target.staging_bytes)
            .max(covered.dependency_staging_bytes);
        if self.set(own, target).is_err() {
            target = covered;
            self.credit = covered;
        }
        let cover = std::mem::take(&mut self.cover);
        PresentationGrant {
            credit: target
                .zip(covered, usize::saturating_sub)
                .zip(cover, usize::saturating_add),
            reclaim,
            publication,
        }
    }

    /// Free room of each renderer field next to the current credit.
    fn room(&self, own: &StreamUse) -> PresentationAllocation {
        let budgets = self.budgets(own, self.credit);
        let free = |i: usize| budgets[i].limit.saturating_sub(budgets[i].used);
        let pending = MAX_PENDING
            .saturating_sub(own.totals.pending_work)
            .min(free(5));
        let jobs = if own.jobs_open { free(9) } else { 0 };
        PresentationAllocation {
            grass_worker_wasm_bytes: usize::MAX,
            grass_worker_cache_bytes: usize::MAX,
            grass_worker_reserved_bytes: usize::MAX,
            active_cpu: free(0),
            active_gpu: free(1),
            prototype_cpu: free(2),
            prototype_gpu: free(3),
            objects: free(4),
            pending,
            staging_bytes: free(6).min(self.dependency_room(own.generation_pending)),
            dependency_staging_bytes: usize::MAX,
            jobs,
        }
    }

    pub fn budgets(&self, own: &StreamUse, next: PresentationAllocation) -> [ResourceBudget; 10] {
        let current = &own.totals;
        let old = self.credit;
        let swap =
            |total: usize, old: usize, next: usize| total.saturating_sub(old).saturating_add(next);
        let used = [
            swap(current.active_cpu_bytes, old.active_cpu, next.active_cpu),
            swap(current.active_gpu_bytes, old.active_gpu, next.active_gpu),
            swap(
                current.prototype_cpu_bytes,
                old.prototype_cpu,
                next.prototype_cpu,
            ),
            swap(
                current.prototype_gpu_bytes,
                old.prototype_gpu,
                next.prototype_gpu,
            ),
            swap(current.generated_objects, old.objects, next.objects),
            swap(current.pending_work, old.pending, next.pending),
            next.staging_bytes + own.generation_pending,
            next.dependency_staging_bytes,
            own.generation_pending - own.binding_reserved,
            next.jobs + own.jobs,
        ];
        let limit = [
            own.active_cpu_limit,
            own.active_gpu_limit,
            PROTOTYPE_CPU,
            PROTOTYPE_GPU,
            own.objects_limit,
            32,
            STAGING,
            next.staging_bytes,
            self.staging_limit(),
            EXTERNAL_JOBS,
        ];
        std::array::from_fn(|i| ResourceBudget {
            name: BUDGET_LABELS[i].0,
            used: used[i],
            limit: limit[i],
            unit: BUDGET_LABELS[i].1,
        })
    }

    /// Keeps `field` from the next grant when own work does not fit.
    pub fn wait_for_room(&self, fits: bool, field: u8) -> bool {
        if !fits {
            self.reclaim.fetch_or(field, Ordering::Relaxed);
        }
        fits
    }

    /// Generation staging without the dependency reserve.
    pub fn staging_limit(&self) -> usize {
        STAGING - self.dependency_staging_reserve
    }
    /// Free generation staging. The dependency reserve stays for the renderer.
    pub fn generation_share(&self, generation_pending: usize, binding_reserved: usize) -> usize {
        self.staging_limit()
            .saturating_sub(generation_pending - binding_reserved)
    }
    /// Staging free of both generation and renderer staging.
    pub fn dependency_room(&self, generation_pending: usize) -> usize {
        STAGING.saturating_sub(self.credit.staging_bytes + generation_pending)
    }
    /// True if `bytes` of generation staging fit. Generation reclaims renderer staging only
    /// when renderer credit blocks the bytes. Credit that comes back does not enlarge the
    /// generation share. Work at that share can wait for renderer loads, and a reclaim then
    /// stops those loads: the first page load stalls (K4).
    pub fn wait_for_staging(
        &self,
        bytes: usize,
        generation_pending: usize,
        binding_reserved: usize,
    ) -> bool {
        bytes <= self.generation_share(generation_pending, binding_reserved)
            && self.wait_for_room(
                bytes <= self.dependency_room(generation_pending),
                Reclaim::STAGING,
            )
    }

    pub fn dependency_staging_configured(&self) -> bool {
        self.dependency_staging_reserve != 0
    }
    pub fn configure_dependency_staging(&mut self, reserve: usize) -> Result<(), ReferenceError> {
        if reserve == 0 || reserve >= STAGING {
            return Err(ReferenceError(
                "Invalid dependency staging reservation".into(),
            ));
        }
        if !self.dependency_staging_configured() {
            self.dependency_staging_reserve = reserve;
        }
        Ok(())
    }

    /// Moves one renderer structure claim out of the credit. False: no claim.
    pub fn take_claim(&mut self) -> bool {
        match self.credit.pending.checked_sub(1) {
            Some(pending) => {
                self.credit.pending = pending;
                true
            }
            None => false,
        }
    }
    /// Adds one renderer structure claim, as the renderer does.
    #[cfg(test)]
    pub fn add_claim(&mut self) {
        self.credit.pending += 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn own(generation_pending: usize) -> StreamUse {
        StreamUse {
            totals: WorldMetrics::default(),
            generation_pending,
            binding_reserved: 0,
            jobs: 0,
            jobs_open: true,
            active_cpu_limit: 64 << 20,
            active_gpu_limit: 64 << 20,
            objects_limit: 1000,
        }
    }

    fn report(used: PresentationAllocation, want: PresentationAllocation) -> PresentationReport {
        PresentationReport {
            used,
            want,
            release: PresentationAllocation::default(),
        }
    }

    #[test]
    fn grant_keeps_dependency_staging_inside_staging_and_withholds_reclaimed_fields() {
        let mut ledger = PresentationLedger::default();
        ledger.configure_dependency_staging(8 << 20).unwrap();
        let want = PresentationAllocation {
            staging_bytes: 16 << 20,
            dependency_staging_bytes: 20 << 20,
            jobs: 2,
            ..Default::default()
        };
        let own = own(0);
        ledger
            .take_report(&own, report(PresentationAllocation::default(), want))
            .unwrap();
        let grant = ledger.grant(Some(&own), false);
        assert_eq!(grant.credit.staging_bytes, 16 << 20);
        assert_eq!(grant.credit.dependency_staging_bytes, 16 << 20);
        assert_eq!(grant.credit.jobs, 2);

        // Generation fits its share but renderer credit blocks it: the next grant holds
        // staging back.
        let more = PresentationAllocation {
            staging_bytes: 24 << 20,
            ..want
        };
        ledger
            .take_report(&own, report(ledger.credit(), more))
            .unwrap();
        assert!(!ledger.wait_for_staging(20 << 20, 0, 0));
        let grant = ledger.grant(Some(&own), false);
        assert!(grant.reclaim.staging);
        assert_eq!(grant.credit.staging_bytes, 0);
        assert!(ledger.credit().dependency_staging_bytes <= ledger.credit().staging_bytes);
    }

    #[test]
    fn use_above_the_limits_fails_and_keeps_the_old_credit() {
        let mut ledger = PresentationLedger::default();
        let own = own(0);
        let over = PresentationAllocation {
            prototype_cpu: PROTOTYPE_CPU + 1,
            ..Default::default()
        };
        assert!(ledger.take_report(&own, report(over, over)).is_err());
        assert_eq!(ledger.credit(), PresentationAllocation::default());
    }
}
