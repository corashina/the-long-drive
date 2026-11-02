//! Source `newAiScript` voice: `SoundIdle`, `SoundChase`, `SoundNotice`, `SoundAttack`,
//! `SoundDeath`, the voice block of `Update` and the `TVoice` mouth move.
//! `SourceVoice.isPlaying` holds until the clip length divided by the pitch.
use crate::weather::StormRandom;

/// One voice clip: the audio clip id and its length in seconds.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct VoiceClip {
    pub clip: u32,
    pub seconds: f32,
}

/// `Sound_Idle`, `Sound_Chase`, `Sound_Notice`, `Sound_Attack` and `Sound_Death`, and the voice times.
#[derive(Clone, Debug, PartialEq)]
pub struct VoiceSet {
    pub idle: Vec<VoiceClip>,
    pub chase: Vec<VoiceClip>,
    pub notice: Vec<VoiceClip>,
    pub attack: Vec<VoiceClip>,
    pub death: Vec<VoiceClip>,
    /// `idleVoiceMinTime`, `idleVoiceMaxTime`.
    pub idle_time: [f32; 2],
    /// `chaseVoiceMinTime`, `chaseVoiceMaxTime`.
    pub chase_time: [f32; 2],
    /// `chaseChaseVoiceMinTime`, `chaseChaseVoiceMaxTime`.
    pub seeing_time: [f32; 2],
}

/// A `SourceVoice.Play`. It stops the clip that plays.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct VoicePlay {
    pub clip: u32,
    pub pitch: f32,
}

/// The `newAiScript` flags that the voice block reads, from the previous `Update`.
#[derive(Clone, Copy, Debug, Default)]
pub struct VoiceInput {
    pub died: bool,
    pub sleeping: bool,
    pub chasing: bool,
    pub seeing: bool,
    pub attacking: bool,
}

/// Voice state of one human monster. Times are simulation seconds. Unity starts them at 0.
#[derive(Clone, Debug, Default)]
pub struct Voice {
    /// `voicePitch`, rolled in `Start`.
    pub pitch: f32,
    /// `SIdleList`, `SChaseList`, `SNoticeList`, `SAttackList`: indices of clips not played yet.
    idle: Vec<u8>,
    chase: Vec<u8>,
    notice: Vec<u8>,
    attack: Vec<u8>,
    /// `nextVoice`.
    next: f64,
    /// End of the clip that plays.
    end: f64,
    next_move: f64,
    move_target: f32,
    /// `currentVoiceMove`: 0 is `TVoiceClose`, 1 is `TVoiceOpen`.
    pub mouth: f32,
}

/// `Random.Range(0, count)` for integers.
fn index(random: &mut StormRandom, count: usize) -> usize {
    ((random.unit() * count as f32) as usize).min(count - 1)
}

/// `ResetXSounds` when the list is empty, then a random entry. Removes it from the list.
fn draw(list: &mut Vec<u8>, count: usize, random: &mut StormRandom) -> Option<usize> {
    if list.is_empty() {
        list.extend((0..count).map(|i| i as u8));
    }
    if list.is_empty() {
        return None;
    }
    let rs = index(random, list.len());
    Some(usize::from(list.remove(rs)))
}

impl Voice {
    pub fn new(pitch: f32, s: &VoiceSet) -> Self {
        let capacity = [&s.idle, &s.chase, &s.notice, &s.attack].map(Vec::len);
        Self {
            pitch,
            idle: Vec::with_capacity(capacity[0]),
            chase: Vec::with_capacity(capacity[1]),
            notice: Vec::with_capacity(capacity[2]),
            attack: Vec::with_capacity(capacity[3]),
            ..Self::default()
        }
    }

    /// `SourceVoice.isPlaying`.
    pub fn playing(&self, now: f64) -> bool {
        now < self.end
    }

    fn play(&mut self, c: VoiceClip, pitch: f32, now: f64) -> VoicePlay {
        self.end = now + f64::from(c.seconds / pitch.abs().max(1e-3));
        VoicePlay {
            clip: c.clip,
            pitch,
        }
    }

    /// The mouth move and the voice block of `Update`. `input` holds the previous `Update` flags.
    pub(crate) fn update(
        &mut self,
        s: &VoiceSet,
        input: VoiceInput,
        now: f64,
        dt: f32,
        random: &mut StormRandom,
    ) -> Option<VoicePlay> {
        if self.playing(now) && now > self.next_move {
            self.next_move = now + f64::from(random.range(0.1, 0.6));
            self.move_target = random.unit();
        }
        self.mouth = move_towards(self.mouth, self.move_target, dt * 1.5);
        if input.died {
            return None;
        }
        let playing = self.playing(now);
        if !playing {
            self.move_target = 0.;
        }
        if input.sleeping || playing {
            return None;
        }
        if input.chasing {
            if input.attacking {
                self.sound_attack(s, now, random)
            } else if now > self.next {
                self.sound_chase(s, input.seeing, now, random)
            } else {
                None
            }
        } else if now > self.next {
            self.sound_idle(s, now, random)
        } else {
            None
        }
    }

    /// `SoundIdle`.
    fn sound_idle(
        &mut self,
        s: &VoiceSet,
        now: f64,
        random: &mut StormRandom,
    ) -> Option<VoicePlay> {
        self.next = now + f64::from(random.range(s.idle_time[0], s.idle_time[1]));
        let i = draw(&mut self.idle, s.idle.len(), random)?;
        let pitch = self.pitch * random.range(0.95, 1.05);
        Some(self.play(s.idle[i], pitch, now))
    }

    /// `SoundChase`. The source plays `Sound_Chase[rs]` with the list index `rs`.
    fn sound_chase(
        &mut self,
        s: &VoiceSet,
        seeing: bool,
        now: f64,
        random: &mut StormRandom,
    ) -> Option<VoicePlay> {
        let time = if seeing { s.seeing_time } else { s.chase_time };
        self.next = now + f64::from(random.range(time[0], time[1]));
        if self.chase.is_empty() {
            self.chase.extend((0..s.chase.len()).map(|i| i as u8));
        }
        if self.chase.is_empty() {
            return None;
        }
        let rs = index(random, self.chase.len());
        self.chase.remove(rs);
        let pitch = self.pitch * random.range(0.9, 1.);
        Some(self.play(s.chase[rs], pitch, now))
    }

    /// `SoundAttack`.
    fn sound_attack(
        &mut self,
        s: &VoiceSet,
        now: f64,
        random: &mut StormRandom,
    ) -> Option<VoicePlay> {
        self.next = now + f64::from(random.range(s.chase_time[0], s.chase_time[1]));
        let i = draw(&mut self.attack, s.attack.len(), random)?;
        let pitch = self.pitch * random.range(0.9, 1.1);
        Some(self.play(s.attack[i], pitch, now))
    }

    /// `SoundNotice`: the AI sees the player and was not chasing.
    pub(crate) fn notice(
        &mut self,
        s: &VoiceSet,
        now: f64,
        random: &mut StormRandom,
    ) -> Option<VoicePlay> {
        self.end = now;
        let i = draw(&mut self.notice, s.notice.len(), random)?;
        let pitch = self.pitch * random.range(1., 1.1);
        Some(self.play(s.notice[i], pitch, now))
    }

    /// `SoundDeath`, from `Die(true)`.
    pub(crate) fn death(
        &mut self,
        s: &VoiceSet,
        now: f64,
        random: &mut StormRandom,
    ) -> Option<VoicePlay> {
        self.end = now;
        if s.death.is_empty() {
            return None;
        }
        let i = index(random, s.death.len());
        let pitch = self.pitch * random.range(0.9, 1.1);
        Some(self.play(s.death[i], pitch, now))
    }
}

fn move_towards(current: f32, target: f32, delta: f32) -> f32 {
    if (target - current).abs() <= delta {
        target
    } else {
        current + (target - current).signum() * delta
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn set() -> VoiceSet {
        let clips = |base: u32| {
            (0..3)
                .map(|i| VoiceClip {
                    clip: base + i,
                    seconds: 1.,
                })
                .collect()
        };
        VoiceSet {
            idle: clips(10),
            chase: clips(20),
            notice: clips(30),
            attack: clips(40),
            death: clips(50),
            idle_time: [5., 6.],
            chase_time: [2., 3.],
            seeing_time: [1., 1.5],
        }
    }

    #[test]
    fn idle_plays_each_clip_once_per_cycle() {
        let s = set();
        let mut v = Voice::new(1., &s);
        let mut random = StormRandom(7);
        let mut played = Vec::new();
        let mut now = 0.;
        while played.len() < 3 {
            if let Some(p) = v.update(&s, VoiceInput::default(), now, 0.1, &mut random) {
                assert!((0.95..=1.05).contains(&p.pitch));
                played.push(p.clip);
            }
            now += 0.1;
        }
        played.sort_unstable();
        assert_eq!(played, [10, 11, 12]);
        // The next idle clip waits for `idleVoiceMinTime`.
        assert!(now > 10.);
    }

    #[test]
    fn chase_waits_for_the_clip_and_attack_does_not_wait_for_next_voice() {
        let s = set();
        let mut v = Voice::new(1., &s);
        let mut random = StormRandom(3);
        let chase = VoiceInput {
            chasing: true,
            ..VoiceInput::default()
        };
        let p = v.update(&s, chase, 0.5, 0.1, &mut random).unwrap();
        assert!((20..23).contains(&p.clip));
        // The clip plays: no new clip, and the mouth opens.
        assert!(v.update(&s, chase, 0.9, 0.1, &mut random).is_none());
        assert!(v.playing(0.9));
        let attack = VoiceInput {
            attacking: true,
            ..chase
        };
        let p = v.update(&s, attack, 1.7, 0.1, &mut random).unwrap();
        assert!((40..43).contains(&p.clip));
        // Notice stops the clip that plays.
        let p = v.notice(&s, 1.8, &mut random).unwrap();
        assert!((30..33).contains(&p.clip) && p.pitch >= 1.);
        assert!(v.update(&s, attack, 1.9, 0.1, &mut random).is_none());
        // Sleep and death stop the voice block, not the clip.
        let dead = VoiceInput {
            died: true,
            ..VoiceInput::default()
        };
        assert!(v.update(&s, dead, 10., 0.1, &mut random).is_none());
        assert!(v.death(&s, 10., &mut random).is_some());
    }

    #[test]
    fn mouth_closes_when_the_clip_ends() {
        let s = set();
        let mut v = Voice::new(1., &s);
        let mut random = StormRandom(11);
        let mut now = 0.;
        let mut open = 0f32;
        while now < 1. {
            v.update(&s, VoiceInput::default(), now, 0.02, &mut random);
            open = open.max(v.mouth);
            now += 0.02;
        }
        assert!(open > 0.);
        while now < 3. {
            v.update(&s, VoiceInput::default(), now, 0.02, &mut random);
            now += 0.02;
        }
        assert_eq!(v.mouth, 0.);
    }
}
