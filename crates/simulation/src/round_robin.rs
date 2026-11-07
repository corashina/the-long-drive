//! Spreads a periodic scan of a list over the fixed steps of its period.

use std::ops::Range;

/// The cursor of one spread scan. Each entry keeps the scan period, with another phase.
#[derive(Debug, Default)]
pub struct RoundRobin {
    next: usize,
}

impl RoundRobin {
    /// The entries to visit this step, of `len` entries in `steps` steps per period.
    /// A range that starts at 0 begins a new period.
    pub fn take(&mut self, len: usize, steps: usize) -> Range<usize> {
        let start = if self.next >= len { 0 } else { self.next };
        let end = (start + len.div_ceil(steps.max(1))).min(len);
        self.next = end;
        start..end
    }

    pub fn reset(&mut self) {
        self.next = 0;
    }
}

#[cfg(test)]
mod tests {
    use super::RoundRobin;

    #[test]
    fn visits_each_entry_once_per_period() {
        let mut scan = RoundRobin::default();
        let mut visits = [0; 10];
        for _ in 0..4 {
            for i in scan.take(visits.len(), 4) {
                visits[i] += 1;
            }
        }
        assert_eq!(visits, [1; 10]);
        assert_eq!(scan.take(10, 4), 0..3);
    }
}
