//! Car crash rule: source `carscript.DamageStuff`, `HornHit` and `partscript.WouldFallOff`.
use crate::interaction::wheels::WheelSlotId;
use crate::reference::{CarCrashReference, ColliderId, CrashReference, ItemId};
use crate::world::UnityRandom;
use rapier3d::prelude::ColliderHandle;

/// A part that can fall off: source `carscript.currentParts` entry.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Candidate {
    Part(ItemId),
    Wheel(WheelSlotId),
}

/// The random source and the reused candidate list of all cars.
pub struct CrashState {
    pub random: UnityRandom,
    pub candidates: Vec<Candidate>,
}

impl Default for CrashState {
    fn default() -> Self {
        Self {
            random: UnityRandom::new(0x6372_6173),
            candidates: Vec::with_capacity(64),
        }
    }
}

/// Source `HornHit`: `hornTime` and the time since the hit.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct HornHit {
    pub time: f32,
    pub elapsed: f32,
}

impl HornHit {
    pub fn new(time: f32) -> Self {
        Self { time, elapsed: 0. }
    }
    /// Source `currenthorn = Lerp(1, 0, InverseLerp(0, hornTime, t))` while `t < hornTime`.
    pub fn gain(&self) -> f32 {
        lerp(1., 0., inverse_lerp(0., self.time, self.elapsed))
    }
    /// The hit after `dt`. None when the hit ends.
    pub fn advanced(self, dt: f32) -> Option<Self> {
        let elapsed = self.elapsed + dt;
        (elapsed < self.time).then_some(Self { elapsed, ..self })
    }
}

/// Source `carscript.blood` state of one car copy. The wash registry holds `repairValue`.
pub struct Blood {
    /// Source `ON`.
    pub on: bool,
    /// The trigger and `objects` nodes are active. None: the prototype state.
    pub shown: Option<bool>,
    pub permission: u8,
    pub never_repair: bool,
    /// The trigger node, then the `objects` nodes.
    pub nodes: Vec<u32>,
    /// Enabled colliders at and below the trigger node.
    pub colliders: Vec<(ColliderId, ColliderHandle)>,
    /// Trigger position in the chassis frame.
    pub point: [f32; 3],
}

impl Blood {
    /// Source `GotDirty` without `repairValue`.
    pub fn got_dirty(&mut self) {
        self.on = true;
        self.shown = Some(true);
    }
    /// Source `GotClean` without `repairValue`.
    pub fn got_clean(&mut self) {
        self.on = false;
        self.shown = Some(false);
    }
    /// Source `SetActive` on the trigger node: its enabled colliders follow it.
    pub fn set_colliders(&self, physics: &mut crate::vehicle::PhysicsWorld, enabled: bool) {
        for &(_, handle) in &self.colliders {
            physics.edit_collider(handle, |c| crate::vehicle::set_collider_enabled(c, enabled));
        }
        physics.refresh_queries();
    }
    /// The active state that the trigger sets on `node`. None: the trigger does not set it.
    pub fn visible(&self, node: u32) -> Option<bool> {
        self.shown.filter(|_| self.nodes.contains(&node))
    }
}

/// Car state for one `DamageStuff` call.
#[derive(Clone, Copy, Debug, Default)]
pub struct Flags {
    /// Source `driving`.
    pub driving: bool,
    /// Source `isPlayerDriving`.
    pub player_driving: bool,
    /// Source `crsrLocked`.
    pub unpaused: bool,
    /// Source `blood != null`.
    pub has_blood: bool,
    /// Source `blood.ON` before this crash.
    pub blood_on: bool,
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Outcome {
    /// Source `hornTime` from `HornHit`.
    pub horn: Option<f32>,
    /// The blood test passed: `GotDirty` and the blood sound.
    pub blood: bool,
    /// Source `player.Death()`.
    pub kill: bool,
    /// `crashLerp` and `numberToFallOff`.
    pub fall_off: Option<(f32, usize)>,
}

/// Source `minFalloffSpeed`, `maxFalloffSpeed`.
pub fn fall_off_range(r: &CrashReference, car_condition: Option<u8>) -> [f32; 2] {
    let scale = car_condition.map_or(1.0, |state| {
        lerp(1.0, 0.2, inverse_lerp(0.0, 4.0, f32::from(state)))
    });
    [r.fall_off[0] * scale, r.fall_off[1] * scale]
}

/// Source `partscript.WouldFallOff` without the slot check.
pub fn would_fall_off(
    r: &CrashReference,
    category: u8,
    part_condition: Option<u8>,
    lerp: f32,
) -> bool {
    let need = r.fall_off_power[usize::from(category)];
    if need == -1.0 {
        return false;
    }
    let k = match part_condition {
        Some(1) => 0.8,
        Some(2) => 0.6,
        Some(3) => 0.4,
        Some(4) => 0.2,
        _ => 1.0,
    };
    lerp > need * k
}

/// Source `carscript.DamageStuff(v)`. The draw order matches the source.
pub fn damage(
    r: &CrashReference,
    car: &CarCrashReference,
    v: f32,
    flags: Flags,
    car_condition: Option<u8>,
    random: &mut UnityRandom,
) -> Outcome {
    let [min, max] = fall_off_range(r, car_condition);
    let v = v * car.multiplier;
    let mut outcome = Outcome::default();
    if flags.driving {
        // C# `&&`: the chance draw runs only when the speed test passes.
        if v * car.horn > random.range(r.horn[0], r.horn[1])
            && random.range(0.0, 1.0) < r.horn_chance
        {
            let on = flags.has_blood && flags.blood_on;
            let (r1, r2) = if on { (2.0, 5.0) } else { (0.3, 1.0) };
            outcome.horn = Some(random.range(r1, r2));
        }
        if v * car.blood > random.range(r.blood[0], r.blood[1]) && flags.has_blood && flags.unpaused
        {
            outcome.blood = true;
            outcome.kill = flags.blood_on && flags.player_driving;
        }
    }
    if v * car.fall_off > random.range(min, max) {
        let crash_lerp = inverse_lerp(r.fall_off[0], r.fall_off_max, v);
        let [low, high] = r.count[random.range_int(0, 2) as usize];
        let count = lerp(low as f32, high as f32, crash_lerp) as i32;
        outcome.fall_off = Some((crash_lerp, count.max(0) as usize));
    }
    outcome
}

/// Source `Mathf.Lerp`: `t` clamps to 0..1.
fn lerp(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t.clamp(0.0, 1.0)
}

/// Source `Mathf.InverseLerp`: clamps to 0..1, 0 when `a == b`.
fn inverse_lerp(a: f32, b: f32, value: f32) -> f32 {
    if a == b {
        0.0
    } else {
        ((value - a) / (b - a)).clamp(0.0, 1.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::reference::SourceId;

    const HUBCAP: u8 = 6;
    const WHEEL: u8 = 7;
    const GUMI: u8 = 8;

    fn crash() -> CrashReference {
        CrashReference {
            source: SourceId {
                file: "level1".into(),
                path_id: 1,
            },
            blood: [40.0, 80.0],
            horn: [10.0, 20.0],
            fall_off: [30.0, 60.0],
            fall_off_max: 90.0,
            horn_chance: 1.0,
            count: [[1, 2], [2, 4]],
            fall_off_power: [
                -1.0, 0.2, 0.2, 0.2, 0.2, 0.2, 0.0, 0.6, -1.0, 0.3, 0.6, 0.7, 0.6, 0.5, 0.3, 0.3,
                0.7, 0.01,
            ],
        }
    }

    const CAR: CarCrashReference = CarCrashReference {
        multiplier: 1.0,
        blood: 1.0,
        horn: 1.0,
        fall_off: 1.0,
    };

    fn driving(blood_on: bool) -> Flags {
        Flags {
            driving: true,
            player_driving: true,
            unpaused: true,
            has_blood: true,
            blood_on,
        }
    }

    #[test]
    fn would_fall_off_follows_power_table() {
        let r = crash();
        assert!(would_fall_off(&r, HUBCAP, None, 0.001));
        assert!(!would_fall_off(&r, HUBCAP, None, 0.0));
        assert!(!would_fall_off(&r, GUMI, Some(4), 1.0));
        assert!(!would_fall_off(&r, WHEEL, Some(0), 0.5));
        assert!(would_fall_off(&r, WHEEL, Some(4), 0.5));
        assert!(!would_fall_off(&r, WHEEL, Some(9), 0.5));
    }

    #[test]
    fn fall_off_range_scales_with_condition() {
        let r = crash();
        assert_eq!(fall_off_range(&r, None), [30.0, 60.0]);
        assert_eq!(fall_off_range(&r, Some(0)), [30.0, 60.0]);
        let [min, max] = fall_off_range(&r, Some(4));
        assert!((min - 6.0).abs() < 1e-4 && (max - 12.0).abs() < 1e-4);
    }

    #[test]
    fn first_hard_crash_bloodies_second_kills() {
        let r = crash();
        let mut random = UnityRandom::new(3);
        let mut blood = Blood {
            on: false,
            shown: None,
            permission: 0,
            never_repair: false,
            nodes: vec![4, 5],
            colliders: Vec::new(),
            point: [0.; 3],
        };
        let first = damage(&r, &CAR, 200.0, driving(blood.on), None, &mut random);
        assert!(first.blood && !first.kill);
        blood.got_dirty();
        assert_eq!(blood.visible(5), Some(true));
        let second = damage(&r, &CAR, 200.0, driving(blood.on), None, &mut random);
        assert!(second.blood && second.kill);
        blood.got_clean();
        assert_eq!(blood.visible(4), Some(false));
        let third = damage(&r, &CAR, 200.0, driving(blood.on), None, &mut random);
        assert!(third.blood && !third.kill);
    }

    #[test]
    fn damage_depends_on_speed_and_blood() {
        let r = crash();
        let mut random = UnityRandom::new(7);
        let still = damage(&r, &CAR, 0.0, driving(true), None, &mut random);
        assert_eq!(still, Outcome::default());

        let hard = damage(&r, &CAR, 200.0, driving(false), None, &mut random);
        let horn = hard.horn.unwrap();
        assert!((0.3..=1.0).contains(&horn));
        assert!(hard.blood && !hard.kill);
        let (crash_lerp, count) = hard.fall_off.unwrap();
        assert_eq!(crash_lerp, 1.0);
        assert!(count == 2 || count == 4);

        let fatal = damage(&r, &CAR, 200.0, driving(true), None, &mut random);
        assert!((2.0..=5.0).contains(&fatal.horn.unwrap()));
        assert!(fatal.blood && fatal.kill);
    }
}
