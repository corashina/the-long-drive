//! Source sleep deadlines use normal simulation time.
use crate::reference::SleepReference;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SleepCamera {
    pub absolute_position: [f64; 3],
    pub rotation: [f32; 4],
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u32)]
pub enum SleepPhase {
    Idle,
    Charging,
    Sleeping,
}

#[derive(Clone, Copy, Debug)]
pub struct SleepInput {
    pub held: bool,
    pub active: bool,
    pub camera_allowed: bool,
    pub dead: bool,
    pub show_text: bool,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SleepOutput {
    pub phase: SleepPhase,
    pub eligible: bool,
    pub show_hint: bool,
    pub charge: f32,
    pub wake_time: f64,
    pub cooldown_remaining: f64,
    pub progress: f32,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ClockFactors {
    pub time_scale: f32,
    pub need_scale: f32,
    pub fluid_scale: f32,
    pub cycle_extra_per_sim_second: f32,
    pub cloud_scale: f32,
}

#[derive(Clone, Debug)]
pub struct SleepState {
    output: SleepOutput,
}

impl SleepState {
    pub fn new(reference: &SleepReference) -> Self {
        Self {
            output: SleepOutput {
                phase: SleepPhase::Idle,
                eligible: false,
                show_hint: false,
                charge: 0.,
                wake_time: -f64::from(reference.cooldown_seconds),
                cooldown_remaining: 0.,
                progress: 0.,
            },
        }
    }

    /// Call at the step endpoint, after consumers used the sampled factors.
    pub fn step(
        &mut self,
        r: &SleepReference,
        input: SleepInput,
        now: f64,
        dt: f32,
    ) -> SleepOutput {
        if !now.is_finite() || !dt.is_finite() || dt < 0. {
            return self.output;
        }
        if input.dead {
            self.interrupt();
        }
        if !input.active {
            self.cancel_charge();
        }
        if self.output.phase == SleepPhase::Sleeping && input.active {
            if now >= self.output.wake_time {
                self.interrupt();
            } else {
                self.output.progress = (1.
                    - (self.output.wake_time - now) / f64::from(r.sleep_real_seconds * 2.))
                .clamp(0., 1.) as f32;
            }
        } else if self.output.phase != SleepPhase::Sleeping {
            self.output.eligible = !input.dead
                && input.active
                && input.camera_allowed
                && now > self.output.wake_time + f64::from(r.cooldown_seconds);
            if self.output.eligible && input.held {
                if self.output.charge >= 1. {
                    self.output.phase = SleepPhase::Sleeping;
                    self.output.charge = 0.;
                    self.output.eligible = false;
                    self.output.progress = 0.;
                    self.output.wake_time = now + f64::from(r.sleep_real_seconds * 2.);
                } else {
                    self.output.phase = SleepPhase::Charging;
                    self.output.charge = (self.output.charge + r.hold_gain * dt).min(1.);
                }
            } else {
                let eligible = self.output.eligible;
                self.cancel_charge();
                self.output.eligible = eligible;
            }
        }
        self.output.show_hint = self.output.eligible && input.show_text;
        self.output.cooldown_remaining =
            (self.output.wake_time + f64::from(r.cooldown_seconds) - now).max(0.);
        self.output
    }

    pub fn clock_factors(&self, r: &SleepReference) -> ClockFactors {
        let sleeping = self.output.phase == SleepPhase::Sleeping;
        let scale = if sleeping {
            r.sleep_cycle_seconds / r.sleep_real_seconds * 0.5
        } else {
            1.
        };
        ClockFactors {
            time_scale: if sleeping { 2. } else { 1. },
            need_scale: if sleeping {
                scale * r.need_modifier
            } else {
                1.
            },
            fluid_scale: scale,
            cycle_extra_per_sim_second: if sleeping { scale } else { 0. },
            cloud_scale: scale,
        }
    }

    /// Source load: sleepEndTime = Time.time - save_player.sleep. The player is awake.
    pub fn restore(&mut self, since_wake: f64, now: f64) {
        self.interrupt();
        self.output.wake_time = now - since_wake;
    }

    /// Source save: save_player.sleep = Time.time - sleepEndTime.
    pub fn since_wake(&self, now: f64) -> f64 {
        now - self.output.wake_time
    }

    pub fn output(&self) -> SleepOutput {
        self.output
    }
    pub fn cancel_charge(&mut self) {
        if self.output.phase == SleepPhase::Charging {
            self.output.phase = SleepPhase::Idle;
        }
        self.output.charge = 0.;
        self.output.eligible = false;
    }
    pub fn interrupt(&mut self) {
        self.output.phase = SleepPhase::Idle;
        self.output.charge = 0.;
        self.output.eligible = false;
        self.output.progress = 0.;
    }
}
