//! Source `stimulscript` sources that rabbits sense.
//! The player head gives the visible stimulus. Each `PlayClipAtPoint` sound gives a sound
//! stimulus of its volume while its clip plays. Engines, horns and radios give bound sound
//! stimuli.
use crate::reference::{CreaturesReference, StimulusKind, StimulusSource};
use rapier3d::math::Vector;
use rapier3d::prelude::RigidBodyHandle;

/// Source `Stimul` SphereCollider radius on the player head.
pub const HEAD_RADIUS: f32 = 0.2;
/// Most one-shot sounds that live at once.
const SOUND_CAPACITY: usize = 64;
/// Usual count of bound sound stimuli.
const BOUND_CAPACITY: usize = 32;
/// `stimulscript.CheckAudioSources` interval in seconds.
const CHECK_INTERVAL: f64 = 0.5;

/// One `stimulscript` with audio sources: the loudest playing source volume times
/// `maxSoundValue`. `key` names its owner voice set.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SoundStimulus {
    pub key: u64,
    /// Local physics position.
    pub position: Vector,
    pub value: f32,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Stimulus {
    /// Local physics position of the stimulus collider.
    pub position: Vector,
    pub kind: StimulusKind,
    pub value: f32,
    pub visible: bool,
    pub sound: bool,
    /// Collider radius. A vision ray that reaches it hits the stimulus.
    pub radius: f32,
    /// The body that carries the stimulus collider. It does not block the vision ray.
    pub body: Option<RigidBodyHandle>,
}

/// One `mainscript.Audio` copy: its stimulus lives for the clip length.
#[derive(Clone, Copy, Debug)]
struct Sound {
    position: [f64; 3],
    value: f32,
    end: f64,
}

#[derive(Default)]
pub struct Stimuli {
    list: Vec<Stimulus>,
    sounds: Vec<Sound>,
    /// Bound stimulus values of the last `CheckAudioSources`, by key.
    held: Vec<(u64, f32)>,
    next_check: f64,
}

impl Stimuli {
    pub fn new() -> Self {
        Self {
            list: Vec::with_capacity(SOUND_CAPACITY + BOUND_CAPACITY + 1),
            sounds: Vec::with_capacity(SOUND_CAPACITY),
            held: Vec::with_capacity(BOUND_CAPACITY),
            next_check: 0.,
        }
    }
    pub fn list(&self) -> &[Stimulus] {
        &self.list
    }
    /// `PlayClipAtPoint(clip, position, volume)`: `SetAudioValue(volume)` on the prefab stimulus.
    pub fn sound(&mut self, position: [f64; 3], volume: f32, now: f64, seconds: f32) {
        if self.sounds.len() == SOUND_CAPACITY {
            self.sounds.remove(0);
        }
        self.sounds.push(Sound {
            position,
            value: volume,
            end: now + f64::from(seconds),
        });
    }
    /// Rebuilds the list. `head` is the player head stimulus and its body. `bound` holds the
    /// live bound stimuli: their values refresh each `CHECK_INTERVAL`, their positions each step.
    pub fn rebuild(
        &mut self,
        sources: &PlayerStimuli,
        head: (Vector, RigidBodyHandle),
        bound: &[SoundStimulus],
        now: f64,
        local: impl Fn([f64; 3]) -> Vector,
    ) {
        self.sounds.retain(|s| s.end > now);
        if now >= self.next_check {
            self.next_check = now + CHECK_INTERVAL;
            self.held.clear();
            self.held.extend(bound.iter().map(|s| (s.key, s.value)));
        }
        self.list.clear();
        for v in &sources.head.stimuli {
            self.list.push(Stimulus {
                position: head.0,
                kind: v.kind,
                value: v.value,
                visible: sources.head.visible,
                sound: sources.head.sound,
                radius: HEAD_RADIUS,
                body: Some(head.1),
            });
        }
        for s in &self.sounds {
            self.list.push(Stimulus {
                position: local(s.position),
                kind: sources.sound.stimuli[0].kind,
                value: s.value * sources.sound.max_sound_value,
                visible: sources.sound.visible,
                sound: sources.sound.sound,
                radius: 0.,
                body: None,
            });
        }
        for s in bound {
            let Some(&(_, value)) = self.held.iter().find(|(key, _)| *key == s.key) else {
                continue;
            };
            if value > 0. {
                self.list.push(Stimulus {
                    position: s.position,
                    kind: StimulusKind::Threat,
                    value,
                    visible: false,
                    sound: true,
                    radius: 0.,
                    body: None,
                });
            }
        }
    }
}

/// The stimulus rows that the port binds.
pub struct PlayerStimuli {
    /// `Stimul` on the player Head.
    pub head: StimulusSource,
    /// The stimulus on the `mainscript.Audio` prefab.
    pub sound: StimulusSource,
}

impl PlayerStimuli {
    pub fn new(r: &CreaturesReference) -> Option<Self> {
        Some(Self {
            head: r.stimulus(&r.player_stimulus)?.clone(),
            sound: r.stimulus(&r.sound_stimulus)?.clone(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::reference::{SourceId, StimulusValue};

    fn sources() -> PlayerStimuli {
        let source = || StimulusSource {
            source: SourceId {
                file: "f".into(),
                path_id: 1,
            },
            node: SourceId {
                file: "f".into(),
                path_id: 2,
            },
            root: SourceId {
                file: "f".into(),
                path_id: 3,
            },
            visible: false,
            sound: true,
            smell: false,
            stimuli: vec![StimulusValue {
                kind: StimulusKind::Threat,
                value: 0.,
            }],
            max_sound_value: 1.,
            check_audio_sources: false,
            audio_sources: Vec::new(),
        };
        PlayerStimuli {
            head: StimulusSource {
                stimuli: Vec::new(),
                ..source()
            },
            sound: source(),
        }
    }

    #[test]
    fn bound_values_refresh_each_check() {
        let mut s = Stimuli::new();
        let engine = |value| SoundStimulus {
            key: 7,
            position: Vector::new(1., 0., 2.),
            value,
        };
        let head = (Vector::ZERO, RigidBodyHandle::invalid());
        // An engine at gain 0.5 with maxSoundValue 60.
        s.rebuild(&sources(), head, &[engine(30.)], 0., |_| Vector::ZERO);
        assert_eq!(s.list().len(), 1);
        assert_eq!(
            (s.list()[0].value, s.list()[0].position),
            (30., Vector::new(1., 0., 2.))
        );
        // The value holds until the next check.
        s.rebuild(&sources(), head, &[engine(90.)], 0.4, |_| Vector::ZERO);
        assert_eq!(s.list()[0].value, 30.);
        s.rebuild(&sources(), head, &[engine(90.)], 0.5, |_| Vector::ZERO);
        assert_eq!(s.list()[0].value, 90.);
    }
}
