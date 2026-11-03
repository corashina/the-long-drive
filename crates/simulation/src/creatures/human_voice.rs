//! Source `newAiScript` voice: `SoundIdle`, `SoundChase`, `SoundNotice`, `SoundAttack`,
//! `SoundDeath`, the voice block of `Update` and the `TVoice` mouth move.
//! `SourceVoice.isPlaying` holds until the clip length divided by the pitch.
use crate::reference::{HumanVoiceClip, HumanVoiceReference};
use crate::weather::StormRandom;

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
    /// The last clip that `SourceVoice` played.
    pub current: Option<VoicePlay>,
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
    pub fn new(pitch: f32, s: &HumanVoiceReference) -> Self {
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

    fn play(&mut self, c: HumanVoiceClip, pitch: f32, now: f64) -> VoicePlay {
        self.end = now + f64::from(c.seconds / pitch.abs().max(1e-3));
        let play = VoicePlay {
            clip: c.clip,
            pitch,
        };
        self.current = Some(play);
        play
    }

    /// The mouth move and the voice block of `Update`. `input` holds the previous `Update` flags.
    pub(crate) fn update(
        &mut self,
        s: &HumanVoiceReference,
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
        s: &HumanVoiceReference,
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
        s: &HumanVoiceReference,
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
        s: &HumanVoiceReference,
        now: f64,
        random: &mut StormRandom,
    ) -> Option<VoicePlay> {
        self.next = now + f64::from(random.range(s.chase_time[0], s.chase_time[1]));
        let i = draw(&mut self.attack, s.attack.len(), random)?;
        let pitch = self.pitch * random.range(0.9, 1.1);
        Some(self.play(s.attack[i], pitch, now))
    }

    /// `SoundNotice`. The AI sees the player and was not chasing.
    pub(crate) fn notice(
        &mut self,
        s: &HumanVoiceReference,
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
        s: &HumanVoiceReference,
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
