//! Source `sens` trigger hold: a contact stays sensed for a number of fixed steps.

#[derive(Clone, Copy, Debug, Default)]
pub(super) struct SensorContact {
    counter: u8,
    sensed: bool,
}
impl SensorContact {
    pub(super) fn step(&mut self, contact: bool, hold: u8) {
        if self.counter > 0 {
            self.counter -= 1;
        } else {
            self.sensed = false;
        }
        if contact {
            self.sensed = true;
            self.counter = hold;
        }
    }
    pub(super) fn sensed(&self) -> bool {
        self.sensed
    }
}
