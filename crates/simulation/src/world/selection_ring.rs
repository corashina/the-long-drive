//! Time-sliced descriptor selection. The units are the prop layer updates, then the content step.
//! A tick resumes at the saved unit. It stops when its time is over or after one full round.

/// Prop layer updates per round.
pub const PROP_LAYERS: u8 = 4;
const UNITS: u8 = PROP_LAYERS + 1;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Unit {
    PropLayer,
    Contents,
}

#[derive(Default)]
pub struct SelectionRing {
    next: u8,
}
impl SelectionRing {
    /// Runs at least one unit. `run` returns false to stop the tick, for example on failure.
    pub fn tick(&mut self, mut run: impl FnMut(Unit) -> bool, mut over: impl FnMut() -> bool) {
        loop {
            let unit = if self.next < PROP_LAYERS {
                Unit::PropLayer
            } else {
                Unit::Contents
            };
            self.next = (self.next + 1) % UNITS;
            if !run(unit) || self.next == 0 || over() {
                return;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use Unit::{Contents as C, PropLayer as P};

    /// A slow tick resumes at the next unit. A fast tick stops after one full round.
    #[test]
    fn resumes_and_caps_one_round() {
        let mut ring = SelectionRing::default();
        let mut tick = |budget: usize| {
            let units = std::cell::RefCell::new(Vec::new());
            ring.tick(
                |unit| {
                    units.borrow_mut().push(unit);
                    true
                },
                || units.borrow().len() >= budget,
            );
            units.into_inner()
        };
        assert_eq!(tick(0), [P]);
        assert_eq!(tick(2), [P, P]);
        assert_eq!(tick(9), [P, C]);
        assert_eq!(tick(9), [P, P, P, P, C]);
        assert_eq!(tick(1), [P]);
    }
}
