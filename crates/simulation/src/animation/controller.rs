//! Source Animator state machine for one checked layer: triggers, the `Speed` parameter,
//! AnyState and state transitions, exit times, fixed-duration crossfades and clip sampling.
//! Transitions have interruption source None: a crossfade runs to its end.
use super::clip::{LocalPart, sample_clip};
use crate::reference::{MeleeMotion, MotionTransition};
use crate::vehicle::VehiclePose;
use rapier3d::math::{Rotation, Vector};

/// State behaviour callbacks (`StateMachineBehaviour.OnStateEnter` and `OnStateExit`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StateEvent {
    Enter(usize),
    Exit(usize),
}

#[derive(Clone, Copy, Debug)]
struct Playing {
    state: usize,
    /// Normalized time.
    time: f32,
}

#[derive(Clone, Copy, Debug)]
struct Crossfade {
    next: Playing,
    elapsed: f32,
    duration: f32,
}

#[derive(Debug)]
pub struct MotionController {
    triggers: u64,
    speed: f32,
    current: Playing,
    crossfade: Option<Crossfade>,
}
impl MotionController {
    pub fn new(m: &MeleeMotion) -> Self {
        Self {
            triggers: 0,
            speed: 0.,
            current: Playing {
                state: m.default_state as usize,
                time: 0.,
            },
            crossfade: None,
        }
    }
    /// The Animator is off: it restarts in the default state.
    pub fn reset(&mut self, m: &MeleeMotion) {
        *self = Self::new(m);
    }
    /// Parameter index for `SetTrigger` and `ResetTrigger`.
    pub fn parameter(m: &MeleeMotion, name: &str) -> Option<usize> {
        m.parameters.iter().position(|p| p.name == name)
    }
    pub fn set_trigger(&mut self, parameter: usize) {
        self.triggers |= 1 << parameter;
    }
    pub fn reset_trigger(&mut self, parameter: usize) {
        self.triggers &= !(1 << parameter);
    }
    pub fn set_speed(&mut self, speed: f32) {
        self.speed = speed;
    }
    pub fn state(&self) -> usize {
        self.current.state
    }
    fn trigger(&self, m: &MeleeMotion, id: u32) -> Option<usize> {
        m.parameters.iter().position(|p| p.id == id)
    }
    fn length(m: &MeleeMotion, state: usize) -> f32 {
        let s = &m.states[state];
        let c = &m.clips[s.clip_slot as usize];
        let length = c.stop_time - c.start_time;
        if length > 0. { length } else { s.clip_duration }
    }
    fn state_speed(&self, m: &MeleeMotion, state: usize) -> f32 {
        let s = &m.states[state];
        if s.speed_parameter != 0 {
            s.speed * self.speed
        } else {
            s.speed
        }
    }
    fn step(&self, m: &MeleeMotion, playing: &mut Playing, dt: f32) {
        playing.time += dt * self.state_speed(m, playing.state) / Self::length(m, playing.state);
    }
    /// True when every condition trigger is set and the exit time, if any, passed in this step.
    fn ready(&self, m: &MeleeMotion, t: &MotionTransition, from: f32, to: f32) -> bool {
        if t.has_exit_time && !crossed(from, to, t.exit_time) {
            return false;
        }
        t.conditions.iter().all(|c| {
            self.trigger(m, c.parameter)
                .is_some_and(|p| self.triggers & (1 << p) != 0)
        })
    }
    pub fn advance(&mut self, m: &MeleeMotion, dt: f32, mut event: impl FnMut(StateEvent)) {
        let callback = |state: usize| m.states[state].callback.is_some();
        let from = self.current.time;
        let mut current = self.current;
        self.step(m, &mut current, dt);
        self.current = current;
        if let Some(mut fade) = self.crossfade {
            self.step(m, &mut fade.next, dt);
            fade.elapsed += dt;
            self.crossfade = Some(fade);
            if fade.elapsed >= fade.duration {
                if callback(self.current.state) {
                    event(StateEvent::Exit(self.current.state));
                }
                self.current = fade.next;
                self.crossfade = None;
            }
            return;
        }
        let state = self.current.state;
        let to = self.current.time;
        let Some(t) = m
            .any_state_transitions
            .iter()
            .filter(|t| t.can_transition_to_self || t.destination as usize != state)
            .chain(&m.states[state].transitions)
            .find(|t| self.ready(m, t, from, to))
        else {
            return;
        };
        for c in &t.conditions {
            if let Some(p) = self.trigger(m, c.parameter) {
                self.reset_trigger(p);
            }
        }
        let destination = t.destination as usize;
        let duration = if t.fixed_duration {
            t.duration
        } else {
            t.duration * Self::length(m, state)
        };
        let next = Playing {
            state: destination,
            time: t.offset,
        };
        if callback(destination) {
            event(StateEvent::Enter(destination));
        }
        if duration > 0. {
            self.crossfade = Some(Crossfade {
                next,
                elapsed: 0.,
                duration,
            });
        } else {
            if callback(state) {
                event(StateEvent::Exit(state));
            }
            self.current = next;
        }
    }
    fn sample(m: &MeleeMotion, playing: Playing) -> (Vector, Rotation) {
        let s = &m.states[playing.state];
        let clip = &m.clips[s.clip_slot as usize];
        let length = clip.stop_time - clip.start_time;
        let fraction = if s.r#loop {
            playing.time.rem_euclid(1.)
        } else {
            playing.time.clamp(0., 1.)
        };
        let time = clip.start_time + fraction * length.max(0.);
        let mut position = Vector::ZERO;
        let mut rotation = Rotation::IDENTITY;
        sample_clip(clip, time, |_, part| match part {
            LocalPart::Position(p) => position = p,
            LocalPart::Rotation(r) => rotation = r,
            LocalPart::Scale(_) => {}
        });
        (position, rotation)
    }
    /// Animated target pose in its parent frame, in converted space.
    pub fn pose(&self, m: &MeleeMotion) -> VehiclePose {
        let (mut position, mut rotation) = Self::sample(m, self.current);
        if let Some(fade) = self.crossfade {
            let weight = (fade.elapsed / fade.duration).clamp(0., 1.);
            let (p, r) = Self::sample(m, fade.next);
            position = position.lerp(p, weight);
            rotation = rotation.slerp(r, weight);
        }
        VehiclePose {
            position: position.to_array(),
            rotation: rotation.to_array(),
        }
    }
}

/// True when a loop of normalized time passes `exit` between `from` and `to`.
fn crossed(from: f32, to: f32, exit: f32) -> bool {
    if to <= from {
        return false;
    }
    if exit >= 1. {
        from < exit && exit <= to
    } else {
        (to - exit).floor() > (from - exit).floor()
    }
}
