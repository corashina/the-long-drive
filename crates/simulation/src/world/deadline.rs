//! Wall clock budget of one work slice.

/// Start time and millisecond budget of one work slice.
#[derive(Clone, Copy)]
pub(crate) struct Deadline {
    #[cfg(target_arch = "wasm32")]
    started: f64,
    #[cfg(not(target_arch = "wasm32"))]
    started: std::time::Instant,
    budget_ms: f64,
}
impl Deadline {
    pub fn start(budget_ms: f64) -> Self {
        Self {
            #[cfg(target_arch = "wasm32")]
            started: crate::host::game_now(),
            #[cfg(not(target_arch = "wasm32"))]
            started: std::time::Instant::now(),
            budget_ms,
        }
    }
    /// No budget. Use it to measure elapsed time only.
    pub fn unbounded() -> Self {
        Self::start(f64::INFINITY)
    }
    pub fn elapsed_ms(&self) -> f64 {
        #[cfg(target_arch = "wasm32")]
        return crate::host::game_now() - self.started;
        #[cfg(not(target_arch = "wasm32"))]
        return self.started.elapsed().as_secs_f64() * 1000.;
    }
    pub fn expired(&self) -> bool {
        self.elapsed_ms() >= self.budget_ms
    }
}
