//! Source `nyulscript` evil behaviour: `GetEvil`, `Update`, `SeeTarget`, `Roar` and `Attack`.
use super::brain::Brain;
use crate::reference::EvilReference;
use crate::weather::StormRandom;

/// `EyeEvil` from `GetEvil(natural: true)`, `EyeGreen` otherwise.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EyeKind {
    Natural,
    Green,
}

/// Clips and animator triggers of one call.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Events {
    /// A `Sroar` clip and the `Roar` trigger.
    pub roar: bool,
    /// A `Sattack` clip and the `Attack` trigger.
    pub attack: bool,
    /// A `Sidle` clip.
    pub idle: bool,
}

/// `nyulscript` state. Times are simulation seconds. Unity starts the private times at 0.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Evil {
    pub kind: Option<EyeKind>,
    pub roaring: bool,
    pub attacking: bool,
    /// `playerDeathTrigger.activeSelf`.
    pub trigger: bool,
    pub idle_sound: bool,
    pub last_roar: f64,
    pub last_attack: f64,
    pub last_see: f64,
    pub last_idle: f64,
    pub current_idle: f32,
}

impl Evil {
    /// `GetEvil`: once per rabbit. The caller skips it in peaceful mode.
    pub(crate) fn get_evil(
        &mut self,
        r: &EvilReference,
        brain: &mut Brain,
        natural: bool,
        now: f64,
        random: &mut StormRandom,
    ) {
        if self.kind.is_some() {
            return;
        }
        brain.avoid_obstacles = false;
        self.idle_sound = true;
        self.current_idle = random.range(r.idle_time[0], r.idle_time[1]);
        brain.enable_invert = true;
        // `minRoarTime` is 0.
        self.last_roar = now;
        self.kind = Some(if natural {
            EyeKind::Natural
        } else {
            EyeKind::Green
        });
    }

    /// `move.modifier ? modifierValue : 1`.
    pub fn speed_modifier(&self, r: &EvilReference) -> f32 {
        if self.attacking {
            r.attack_modifier
        } else {
            1.
        }
    }

    /// `Update`. `distance_sq` is the squared player distance; the source compares it with
    /// `idleDistance` directly.
    pub(crate) fn update(
        &mut self,
        r: &EvilReference,
        brain: &mut Brain,
        now: f64,
        distance_sq: f32,
        random: &mut StormRandom,
    ) -> Events {
        let mut events = Events::default();
        if self.roaring && now - self.last_roar > f64::from(r.roar_duration) {
            self.roaring = false;
            brain.go_to_target = true;
        }
        if self.attacking && now - self.last_attack > f64::from(r.attack_duration) {
            self.attacking = false;
            self.trigger = false;
        }
        if self.idle_sound
            && distance_sq < r.idle_distance
            && now - self.last_idle > f64::from(self.current_idle)
        {
            self.last_idle = now;
            self.current_idle = random.range(r.idle_time[0], r.idle_time[1]);
            events.idle = true;
        }
        events
    }

    /// `SeeTarget(angle, dist)`, then `Roar` or `Attack`.
    pub fn see_target(
        &mut self,
        r: &EvilReference,
        brain: &mut Brain,
        angle: f32,
        distance: f32,
        size: f32,
        now: f64,
    ) -> Events {
        let mut events = Events::default();
        if angle >= r.attack_angle {
            return events;
        }
        if now - self.last_see > f64::from(r.target_forget_time) {
            self.last_roar = now;
            brain.go_to_target = false;
            self.roaring = true;
            events.roar = true;
        } else if distance < r.attack_distance
            && now - self.last_attack > f64::from(r.attack_cooldown + r.attack_duration)
        {
            if size > 0.5 {
                self.trigger = true;
            }
            self.last_attack = now;
            self.attacking = true;
            events.attack = true;
        }
        self.last_see = now;
        events
    }

    /// `Die`: the trigger turns off.
    pub fn die(&mut self) {
        self.trigger = false;
        self.idle_sound = false;
    }
}
