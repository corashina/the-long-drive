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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::reference::{BrainReference, SourceId};
    use rapier3d::math::Vector;

    fn evil() -> EvilReference {
        let link = serde_json::json!({"asset": 1, "gltf_material": 0,
            "source": {"file": "f", "path_id": 656}, "condition_key": null});
        serde_json::from_value(serde_json::json!({
            "source": {"file": "f", "path_id": 139619},
            "attack_angle": 10., "attack_distance": 4., "attack_duration": 2.,
            "attack_cooldown": 1., "attack_modifier": 1.5, "roar_duration": 2.,
            "target_forget_time": 10., "idle_time": [10., 30.], "idle_distance": 15.,
            "roar_pitch": 0.985, "roar_clips": [1], "idle_clips": [2], "attack_clips": [3],
            "eye_materials": {"evil": link, "green": link},
            "trigger": {"damage_player": 5., "damage_breakable": 5., "collider": {"node": 1,
                "source": {"file": "f", "path_id": 124453}, "sensor": true,
                "membership": 1, "filter": 1, "friction": 0.6, "restitution": 0.,
                "geometry": {"kind": "sphere", "center": [0., 0., 0.], "radius": 0.25},
                "step_surface": null, "tag": null}}
        }))
        .unwrap()
    }

    fn brain() -> Brain {
        let r: BrainReference = serde_json::from_value(serde_json::json!({
            "source": SourceId { file: "f".into(), path_id: 1 },
            "acceleration_death": 25., "go_to_target": true, "flee": true, "pre_calc_flee": true,
            "avoid_obstacles": true, "roam": true, "attack": true, "jump": true,
            "jump_if_speed": 1., "jump_sense_distance": 1., "jump_mask": 1,
            "enable_invert": false, "go_to_player_target": false,
            "target_pre_calc_distance": 15., "target_distance_goal": 1., "target_angle_goal": 2.,
            "target_angle_move": 50., "walk_distance": 5., "run_distance": 10.,
            "attack_speed": 0.5, "flee_distance": 100., "min_flee_speed": 0.5, "flee_time": 10.,
            "walk_speed_modifier": 0.3, "move_towards_target_modifier": 1.,
            "move_forward_modifier": 2., "roam_x": [-10., 10.], "roam_z": [5., 10.],
            "roam_time": [5., 10.], "invert_flight_if_above": -11., "preferences": []
        }))
        .unwrap();
        Brain::new(&r, Vector::ZERO)
    }

    #[test]
    fn get_evil_inverts_flight_once() {
        let r = evil();
        let mut b = brain();
        let mut e = Evil::default();
        let mut random = StormRandom(3);
        e.get_evil(&r, &mut b, true, 5., &mut random);
        assert!(b.enable_invert && !b.avoid_obstacles && e.idle_sound);
        assert!((10. ..=30.).contains(&e.current_idle));
        e.get_evil(&r, &mut b, false, 6., &mut random);
        assert_eq!(e.kind, Some(EyeKind::Natural));
    }

    #[test]
    fn roar_first_then_attack_after_cooldown() {
        let r = evil();
        let mut b = brain();
        let mut e = Evil::default();
        let mut random = StormRandom(3);
        // Outside the attack angle nothing happens.
        assert_eq!(
            e.see_target(&r, &mut b, 12., 3., 0.8, 20.),
            Events::default()
        );
        let first = e.see_target(&r, &mut b, 5., 3., 0.8, 20.);
        assert!(first.roar && !first.attack && e.roaring && !b.go_to_target);
        // The roar ends after roarDuration.
        e.update(&r, &mut b, 22.5, 100., &mut random);
        assert!(!e.roaring && b.go_to_target);
        // In range: attack, the trigger and the speed modifier turn on.
        let attack = e.see_target(&r, &mut b, 5., 3., 0.8, 23.);
        assert!(attack.attack && e.trigger && e.speed_modifier(&r) == 1.5);
        // No new attack before cooldown plus duration.
        assert!(!e.see_target(&r, &mut b, 5., 3., 0.8, 25.5).attack);
        e.update(&r, &mut b, 25.6, 100., &mut random);
        assert!(!e.attacking && !e.trigger && e.speed_modifier(&r) == 1.);
        assert!(e.see_target(&r, &mut b, 5., 3., 0.8, 26.1).attack);
        // A small rabbit attacks without the trigger.
        let mut small = Evil::default();
        small.see_target(&r, &mut b, 5., 3., 0.4, 20.);
        assert!(small.see_target(&r, &mut b, 5., 3., 0.4, 21.).attack && !small.trigger);
    }
}
