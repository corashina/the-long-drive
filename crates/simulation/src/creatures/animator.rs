//! The `Roar` and `Attack` states of the rabbit Animator base layer and their fixed-duration
//! transitions. Each Unity cross-fade is a linear weight from the blend pose to the action clip.
use crate::reference::{ActionState, ActionTransition, AnimatorActions};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    Roar,
    Attack,
}

impl Action {
    pub fn index(self) -> usize {
        self as usize
    }
    pub fn state(self, actions: &AnimatorActions) -> &ActionState {
        match self {
            Action::Roar => &actions.roar,
            Action::Attack => &actions.attack,
        }
    }
}

/// The time rate and the transitions of one action state.
#[derive(Clone, Copy, Debug)]
pub struct ActionTiming {
    /// Normalized state time per second: the state speed over the clip length.
    pub rate: f32,
    pub enter: ActionTransition,
    pub leave: ActionTransition,
}

/// The action layer that the rig samples over the blend pose.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ActionPose {
    pub action: Action,
    /// Normalized state time.
    pub time: f32,
    pub weight: f32,
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct AnimatorState {
    /// Set triggers. A trigger stays set until a transition uses it.
    pub roar: bool,
    pub attack: bool,
    pub pose: Option<ActionPose>,
    /// Weight change per second while a transition runs. Transitions have no interruption.
    fade: f32,
}

/// True when the normalized time passes `exit` between `previous` and `current`.
fn crossed(previous: f32, current: f32, exit: f32) -> bool {
    let (a, b) = (previous.rem_euclid(1.), current.rem_euclid(1.));
    if a <= b {
        a < exit && exit <= b
    } else {
        a < exit || exit <= b
    }
}

impl AnimatorState {
    fn start(&mut self, action: Action, timing: &ActionTiming) {
        let duration = timing.enter.duration;
        self.pose = Some(ActionPose {
            action,
            time: timing.enter.offset,
            weight: if duration > 0. { 0. } else { 1. },
        });
        self.fade = if duration > 0. { 1. / duration } else { 0. };
    }

    /// One step after the blend advanced from `previous` to `phase`. The return to `Idle,move`
    /// starts the blend at the transition offset.
    pub fn step(&mut self, timings: &[ActionTiming; 2], previous: f32, phase: &mut f32, dt: f32) {
        if self.fade == 0. {
            if self.roar {
                self.roar = false;
                self.start(Action::Roar, &timings[Action::Roar.index()]);
            } else if self.attack
                && self.pose.is_none()
                && timings[Action::Attack.index()]
                    .enter
                    .exit_time
                    .is_none_or(|exit| crossed(previous, *phase, exit))
            {
                self.attack = false;
                self.start(Action::Attack, &timings[Action::Attack.index()]);
                return;
            }
        }
        let Some(pose) = &mut self.pose else {
            return;
        };
        let timing = &timings[pose.action.index()];
        pose.time += dt * timing.rate;
        pose.weight = (pose.weight + self.fade * dt).clamp(0., 1.);
        if self.fade > 0. && pose.weight >= 1. {
            self.fade = 0.;
        }
        if self.fade == 0. && pose.time >= timing.leave.exit_time.unwrap_or(1.) {
            let duration = timing.leave.duration;
            *phase = timing.leave.offset;
            if duration > 0. {
                self.fade = -1. / duration;
            } else {
                pose.weight = 0.;
            }
        }
        if pose.weight <= 0. && self.fade <= 0. {
            self.pose = None;
            self.fade = 0.;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn transition(exit_time: Option<f32>, duration: f32, offset: f32) -> ActionTransition {
        ActionTransition {
            exit_time,
            duration,
            offset,
        }
    }

    /// The controller 4512 values. Clips of 2 s and 1 s.
    fn timings() -> [ActionTiming; 2] {
        [
            ActionTiming {
                rate: 0.5,
                enter: transition(None, 0.25, 0.),
                leave: transition(Some(0.9), 0.25, 0.),
            },
            ActionTiming {
                rate: 2.,
                enter: transition(Some(0.2058), 0.0676, 0.),
                leave: transition(Some(0.9074), 0.1643, 0.0048),
            },
        ]
    }

    #[test]
    fn attack_waits_for_the_exit_time_and_fades_back() {
        let t = timings();
        let mut a = AnimatorState {
            attack: true,
            ..Default::default()
        };
        let mut phase = 0.1;
        a.step(&t, 0.05, &mut phase, 0.02);
        assert!(a.attack && a.pose.is_none());
        phase = 0.21;
        a.step(&t, 0.19, &mut phase, 0.02);
        assert!(!a.attack);
        let pose = a.pose.unwrap();
        assert_eq!(
            (pose.action, pose.time, pose.weight),
            (Action::Attack, 0., 0.)
        );
        // Speed 2 on a 1 s clip: 0.4537 s to the exit time 0.9074.
        let mut time = 0.;
        while a.pose.is_some_and(|p| p.weight > 0. || time < 0.1) && time < 2. {
            time += 0.01;
            a.step(&t, phase, &mut phase, 0.01);
            if (time - 0.07_f32).abs() < 1e-3 {
                assert_eq!(a.pose.unwrap().weight, 1.);
            }
            if (time - 0.45_f32).abs() < 1e-3 {
                assert_eq!(a.pose.unwrap().weight, 1.);
            }
        }
        // The exit at 0.46 s, then 0.1643 s back to the blend.
        assert!((0.61..0.64).contains(&time), "{time}");
        assert!(phase < 0.1);
    }

    #[test]
    fn roar_starts_from_any_steady_state() {
        let t = timings();
        let mut a = AnimatorState {
            roar: true,
            attack: true,
            ..Default::default()
        };
        let mut phase = 0.5;
        a.step(&t, 0.5, &mut phase, 0.02);
        assert_eq!(a.pose.unwrap().action, Action::Roar);
        // The attack trigger waits while the roar plays.
        for _ in 0..100 {
            a.step(&t, phase, &mut phase, 0.02);
        }
        assert!(a.attack && a.pose.unwrap().action == Action::Roar);
    }

    #[test]
    fn exit_time_crossing_wraps() {
        assert!(crossed(0.95, 1.25, 0.2));
        assert!(!crossed(0.25, 0.9, 0.2));
        assert!(crossed(0.1, 0.3, 0.2));
    }
}
