//! Source guns: `weaponscript` (trigger, CanShoot, Shot, Csoletores break barrel, Reload, flash,
//! recoil, trigger parts, reload hand), `ammoscript.CanReload`, `revolverScript`, `bbgunscript`,
//! `geppityuscript`, and `mainscript` tracers, hit particles, hit clips and bullet-hole spawns.
use super::items::ItemStore;
use super::spray::Ammo;
use crate::damage::Breakables;
use crate::particles::{ParticleOutput, ParticleState};
use crate::reference::{
    BreakableReference, ColliderTag, GunBreakRule, GunKind, GunReference, GunSound, ItemId,
    ReferenceError, ReferenceManifest, SourceId, TorqueCurve, Transform, WorldPrototypeReference,
};
use crate::simulation::dials::unity_euler;
use crate::vehicle::{PhysicsWorld, collider_tag, to_rapier};
use crate::world::unity_random::UnityRandom;
use rapier3d::math::{Pose, Rotation, Vector};
use rapier3d::prelude::{ColliderHandle, QueryFilter, Ray, RigidBodyHandle};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Collider layers with their own shot rules.
const PLAYER_LAYER: u32 = 9;
const SAND_LAYERS: [u32; 2] = [17, 19];
const GLASS_LAYER: u32 = 23;
/// `MELEE_HIT_PARTICLES` index of `mainscript.particle.spark` and `sand`.
const SPARK: usize = 0;
const SAND: usize = 4;
/// `weaponscript.hit` is a `RaycastHit[32]`.
const HIT_CAPACITY: usize = 32;
/// `mainscript.PlayClipAtPoint` with `randomPitch`.
const RANDOM_PITCH: [f32; 2] = [0.85, 1.15];
/// `weaponscript.soundsMinPitch` and `soundsMaxPitch` of the `SSource` clips.
const SOURCE_PITCH: [f32; 2] = [0.9, 1.1];
/// `mainscript.holdButtonTime`: a shorter R press is a tap.
const HOLD_BUTTON_TIME: f32 = 0.25;
/// `weaponscript.reloading` stays set this long after `Reload`.
const RELOADING_TIME: f32 = 0.3;
/// `BBcsomozgatasv`: frames that keep the `SMove` clip after the last move.
const MOVE_FRAMES: u8 = 5;
/// `revolverScript.DoRotate` divides the duration by the queued turns up to this count.
const MAX_TURN_DIVISOR: i32 = 5;
/// The chamber bits are a `u32`.
const MAX_CHAMBERS: usize = 32;
/// `mainscript.GetDecal` order.
const DECAL_METAL: u8 = 1;
const DECAL_FLESH: u8 = 2;
const DECAL_WOOD: u8 = 3;
const DECAL_PLANT: u8 = 4;
const DECAL_GLASS: u8 = 5;

/// A one-shot clip for `mainscript.PlayClipAtPoint`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ShotSound {
    pub clip: u32,
    pub gain: f32,
    pub pitch: f32,
    pub position: [f64; 3],
}

/// One bullet hole: `ShootDecals[kind]` at `pose` in physics space, parented to `collider`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DecalSpawn {
    pub kind: u8,
    pub collider: ColliderHandle,
    pub pose: Pose,
    pub scale: f32,
}

/// One shot hit on an item: tanks and explosive props try to explode with the shot power.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Strike {
    pub item: ItemId,
    pub force: f32,
    /// `explodeStuffs && Random.Range(0, 1) < explodeChance`.
    pub explode: bool,
    /// `hithole`: the hit point and normal in physics space.
    pub hole: Option<[Vector; 2]>,
}

/// One shot hit on a collider without an item: rabbits take it as `breakchilds` or
/// `breakablescript.TryBreakShoot` with the shot direction.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CreatureHit {
    pub collider: ColliderHandle,
    pub force: f32,
    pub direction: Vector,
}

/// `fpscontroller` weapon crosshair: the `UiReference::sprites` index and the color.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Crosshair {
    pub sprite: Option<u32>,
    pub color: [f32; 4],
}

/// Saved state of one gun item.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GunSnapshot {
    pub ammo: f32,
    /// `FRValue`.
    pub fr: f32,
    /// `revolverScript.bullets`: bit i is a full chamber i.
    pub chambers: u32,
    /// `revolverScript.CylinderRotated`.
    pub cylinder: i32,
    /// `bbgunscript.power`.
    pub power: u32,
    pub has_mag: bool,
}

/// One `ammoscript` outside the gun: a loose ammo item or a magazine.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct AmmoItem {
    pub ammo: Ammo,
    pub is_mag: bool,
}

/// A prototype node and its rest local pose.
#[derive(Clone, Copy)]
struct Part {
    node: u32,
    rest: Transform,
}

/// `MovingPart`: `T` moves between `T0` and `T1`. Poses are in the item root frame.
struct MovingNode {
    part: Part,
    /// The parent of `T` in the root frame, and its scale.
    parent: Pose,
    scale: Vector,
    t0: Vector,
    t1: Vector,
}

/// `revolverScript.Cylinder` with its `Bullets` and `ShootBulletPos`.
struct Cylinder {
    part: Part,
    /// The cylinder in the root frame.
    root: Pose,
    /// Chamber positions in the cylinder frame.
    chambers: Vec<Vector>,
    /// `ShootBulletPos` in the root frame.
    shoot: Vector,
    axis: [f32; 3],
    angle: f32,
    duration: f32,
}

/// `ReloadLHPos` targets in the root frame.
struct ReloadHand {
    rest: Pose,
    default: Pose,
    barrel: Option<Pose>,
    reload: Option<Pose>,
}

/// One `weaponscript` record with its node poses and clip ids.
struct Gun {
    record: GunReference,
    /// `S` in the item root frame.
    muzzle: Pose,
    mass: f32,
    velocity: f32,
    one_shot: Vec<u32>,
    false_shot: Vec<u32>,
    hit_sand: Vec<u32>,
    hit_concrete: Vec<u32>,
    hit_flesh: Vec<u32>,
    hit_glass: Vec<u32>,
    /// `SReload`, `SOpen`, `SOpenEnd`, `SClose`, `SMove` and `teker` with volumes.
    reload: Option<(u32, f32)>,
    open: Option<(u32, f32)>,
    open_end: Option<(u32, f32)>,
    close: Option<(u32, f32)>,
    r#move: Option<u32>,
    turn: Option<(u32, f32)>,
    /// `TRecoil` target: converted `recoilPos` and `recoilRot`.
    recoil: Option<(Vector, Rotation)>,
    recoil_part: Option<Part>,
    barrels: Vec<Part>,
    moving: Vec<MovingNode>,
    triggers: Vec<Part>,
    trigger_triggers: Vec<Part>,
    cylinder: Option<Cylinder>,
    /// Revolver `Bullets` or BB gun `bullet` nodes.
    bullets: Vec<u32>,
    mag: Option<u32>,
    /// `magPos` in the item root frame: the dropped magazine pose.
    mag_pos: Option<Pose>,
    /// The flash light node.
    flash: Option<u32>,
    hand: Option<ReloadHand>,
    crosshair: Option<u32>,
}

/// `weaponscript` runtime of one gun item.
#[derive(Clone, Debug)]
pub struct GunRuntime {
    gun: usize,
    pub ammo: Ammo,
    /// `bbgunscript.power`.
    pub power: u32,
    pub has_mag: bool,
    /// `FRValue`.
    pub fr: f32,
    last_fr: f32,
    /// `revolverScript.bullets` bits.
    pub chambers: u32,
    /// `CylinderRotated`.
    pub cylinder: i32,
    /// `tekeres`: queued cylinder turns.
    queued: i32,
    /// The running `DoRotate`: start time and direction.
    turn: Option<(f32, i32)>,
    shot_time: f32,
    actually_shot_time: f32,
    anim_time: f32,
    reload_time: f32,
    pulling: bool,
    last_pulling: bool,
    already_shot_false: bool,
    current_shot_count: u32,
    move_frames: u8,
    moving_sound: bool,
    /// `TriggerTriggers` local rotations and `dontTrigger` flags.
    trigger_rotations: Vec<Rotation>,
    dont_trigger: Vec<bool>,
    /// `ReloadLHPos` in the root frame.
    hand: Pose,
    initial: GunSnapshot,
}
impl GunRuntime {
    fn new(gun: usize, g: &Gun, count: Option<f32>) -> Self {
        let r = &g.record;
        let (power, has_mag) = match r.kind {
            GunKind::BbGun { power, .. } => (power, false),
            GunKind::Magazine { has_mag, .. } => (0, has_mag),
            _ => (0, false),
        };
        let ammo = Ammo::new(&r.ammo);
        let ammo = count.and_then(|c| ammo.with_count(c)).unwrap_or(ammo);
        // revolverScript.Start fills one chamber per round.
        let full = (ammo.count.ceil().max(0.) as usize).min(g.bullets.len());
        let chambers = match r.kind {
            GunKind::Revolver { .. } => low_bits(full),
            _ => 0,
        };
        let mut rt = Self {
            gun,
            ammo,
            power,
            has_mag,
            fr: r.fr.value,
            last_fr: r.fr.value,
            chambers,
            cylinder: 0,
            queued: 0,
            turn: None,
            shot_time: f32::NEG_INFINITY,
            actually_shot_time: f32::NEG_INFINITY,
            anim_time: f32::NEG_INFINITY,
            reload_time: f32::NEG_INFINITY,
            pulling: false,
            last_pulling: false,
            already_shot_false: false,
            current_shot_count: 0,
            move_frames: 0,
            moving_sound: false,
            trigger_rotations: g
                .trigger_triggers
                .iter()
                .map(|p| Rotation::from_array(p.rest.rotation))
                .collect(),
            dont_trigger: r.trigger_triggers.iter().map(|t| t.dont_trigger).collect(),
            hand: g.hand.as_ref().map_or(Pose::IDENTITY, |h| h.rest),
            initial: GunSnapshot {
                ammo: 0.,
                fr: 0.,
                chambers: 0,
                cylinder: 0,
                power: 0,
                has_mag: false,
            },
        };
        rt.initial = rt.snapshot();
        rt
    }
    pub fn snapshot(&self) -> GunSnapshot {
        GunSnapshot {
            ammo: self.ammo.count,
            fr: self.fr,
            chambers: self.chambers,
            cylinder: self.cylinder,
            power: self.power,
            has_mag: self.has_mag,
        }
    }
    /// A saved state, or false when it does not fit the gun.
    fn restore(&mut self, g: &Gun, s: &GunSnapshot) -> bool {
        let Some(ammo) = self.ammo.with_count(s.ammo) else {
            return false;
        };
        let max_power = match g.record.kind {
            GunKind::BbGun {
                max_power, power, ..
            } => max_power.max(power),
            _ => 0,
        };
        let revolver = matches!(g.record.kind, GunKind::Revolver { .. });
        if !(0. ..=1.).contains(&s.fr)
            || (revolver && u64::from(s.chambers) >> g.bullets.len() != 0)
            || s.power > max_power
        {
            return false;
        }
        self.ammo = ammo;
        self.fr = s.fr;
        self.last_fr = s.fr;
        self.chambers = s.chambers;
        // revolverScript.RotateTo.
        self.cylinder = s.cylinder;
        self.power = s.power;
        self.has_mag = s.has_mag;
        true
    }
    /// `weaponscript.HasPower`.
    fn has_power(&self, kind: &GunKind) -> bool {
        match kind {
            GunKind::BbGun { dont_use_power, .. } => self.power > 0 || *dont_use_power,
            GunKind::Revolver { .. } | GunKind::Magazine { .. } => true,
            GunKind::None => false,
        }
    }
    /// `weaponscript.HasMag`.
    fn has_mag(&self, kind: &GunKind) -> bool {
        !matches!(kind, GunKind::Magazine { .. }) || self.has_mag
    }
    /// `weaponscript.CanShoot`.
    fn can_shoot(&self, r: &GunReference) -> bool {
        self.has_power(&r.kind) && fr_allows(&r.fr.shoot, self.fr)
    }
    /// `weaponscript.Can`: the FR rule of a reload.
    fn can(&self, r: &GunReference) -> bool {
        fr_allows(&r.fr.reload, self.fr)
    }
    /// `CylinderRotated` with the running turn, in turns.
    fn steps(&self, g: &Gun, time: f32) -> f32 {
        let Some((start, dir)) = self.turn else {
            return self.cylinder as f32;
        };
        let duration = g.cylinder.as_ref().map_or(0., |c| c.duration);
        let t = inverse_lerp(start, start + turn_duration(duration, self.queued), time);
        self.cylinder as f32 - dir as f32 * (1. - t)
    }
    /// `bbgunscript`, `revolverScript` and `geppityuscript.Shot`. True fires.
    fn kind_shot(&mut self, g: &Gun, use_ammo: bool, time: f32) -> bool {
        let r = &g.record;
        let has_ammo = self.ammo.count > 0.;
        let fires = match r.kind {
            GunKind::None => false,
            GunKind::BbGun { dont_use_power, .. } => {
                if !self.has_power(&r.kind) {
                    return false;
                }
                if !dont_use_power {
                    self.power -= 1;
                }
                has_ammo
            }
            GunKind::Revolver { .. } => {
                if !self.can_shoot(r) {
                    return false;
                }
                self.queued += 1;
                let Some(c) = &g.cylinder else {
                    return false;
                };
                let selected = chamber(c, self.steps(g, time));
                let full = self.chambers & 1 << selected != 0 && has_ammo;
                if full && !use_ammo {
                    return true;
                }
                self.chambers &= !(1 << selected);
                full
            }
            GunKind::Magazine { .. } => self.has_mag && has_ammo,
        };
        if fires && use_ammo {
            self.ammo.decrease(1.);
        }
        fires
    }
}

/// One ray hit.
#[derive(Clone, Copy)]
struct Hit {
    distance: f32,
    collider: ColliderHandle,
    normal: Vector,
}

/// The world that one shot touches.
pub struct ShotWorld<'a> {
    pub items: &'a ItemStore,
    pub physics: &'a mut PhysicsWorld,
    pub breakables: &'a mut Breakables,
    pub records: &'a [BreakableReference],
    /// `mainscript.Particles` in `MELEE_HIT_PARTICLES` order.
    pub hit_particles: &'a mut [ParticleState; 5],
    pub player: Option<RigidBodyHandle>,
    pub origin: [f64; 3],
}

/// The held gun of one frame.
#[derive(Clone, Copy, Debug)]
pub struct HeldGun {
    pub item: ItemId,
    pub gun: usize,
    /// `fpscontroller.Th` in physics space.
    pub view: Pose,
    /// `partconditionscript.state`, or `None` without a condition.
    pub condition: Option<f32>,
}

pub struct GunState {
    guns: Vec<Gun>,
    runtimes: HashMap<ItemId, GunRuntime>,
    /// Saved states of spawned guns, applied when their runtime binds.
    pending: Vec<(ItemId, GunSnapshot)>,
    /// The `ItemStore` membership of the last bind.
    revision: Option<u64>,
    current: Option<ItemId>,
    /// `ParticleShootStart` and `ParticleShootEnd`.
    tracers: [ParticleState; 2],
    random: UnityRandom,
    time: f32,
    dt: f32,
    hits: Vec<Hit>,
    pushed: Vec<RigidBodyHandle>,
    sounds: Vec<ShotSound>,
    decals: Vec<DecalSpawn>,
    foods: Vec<ItemId>,
    strikes: Vec<Strike>,
    creature_hits: Vec<CreatureHit>,
    /// Items whose part poses changed since the presentation took them.
    presented: Vec<ItemId>,
    /// Items whose saved state changed since the caller took them.
    changed: Vec<ItemId>,
    /// `letores`: the gun that `Csoletores` moved this frame.
    breaking: Option<ItemId>,
    /// Seconds since the R press.
    reload_hold: Option<f32>,
    /// R is held with a gun: `fpscontroller.reloading` blocks look and scroll.
    pub reloading: bool,
    /// `ItemPrompt` bits of the gun reload.
    pub prompt: u8,
    /// The gun whose flash light is on.
    lit: Option<ItemId>,
}

impl GunState {
    pub fn new(r: &ReferenceManifest) -> Result<Self, ReferenceError> {
        let clips = super::melee::clip_table(r);
        let guns = r
            .guns
            .weapons
            .iter()
            .map(|w| gun(r, w, &clips))
            .collect::<Result<_, _>>()?;
        let tracer = &r.guns.tracer;
        Ok(Self {
            guns,
            runtimes: HashMap::new(),
            pending: Vec::new(),
            revision: None,
            current: None,
            tracers: [
                ParticleState::emit_only(&tracer.start, 0x6775_6e73),
                ParticleState::emit_only(&tracer.end, 0x6775_6e74),
            ],
            random: UnityRandom::new(0x6775_6e73),
            time: 0.,
            dt: 0.,
            hits: Vec::with_capacity(HIT_CAPACITY + 1),
            pushed: Vec::with_capacity(HIT_CAPACITY),
            sounds: Vec::with_capacity(16),
            decals: Vec::with_capacity(HIT_CAPACITY),
            foods: Vec::with_capacity(HIT_CAPACITY),
            strikes: Vec::with_capacity(HIT_CAPACITY),
            creature_hits: Vec::with_capacity(HIT_CAPACITY),
            presented: Vec::with_capacity(8),
            changed: Vec::with_capacity(8),
            breaking: None,
            reload_hold: None,
            reloading: false,
            prompt: 0,
            lit: None,
        })
    }
    /// The gun whose pickup rule is the held item's rule.
    pub fn weapon_for(&self, pickup: &SourceId) -> Option<usize> {
        self.guns.iter().position(|g| g.record.pickup == *pickup)
    }
    pub fn runtime(&self, item: ItemId) -> Option<&GunRuntime> {
        self.runtimes.get(&item)
    }
    pub fn runtime_mut(&mut self, item: ItemId) -> Option<&mut GunRuntime> {
        self.runtimes.get_mut(&item)
    }
    pub fn record(&self, item: ItemId) -> Option<&GunReference> {
        Some(&self.guns[self.runtimes.get(&item)?.gun].record)
    }
    /// The flash light node of a gun item.
    pub fn flash_node(&self, item: ItemId) -> Option<u32> {
        self.guns[self.runtimes.get(&item)?.gun].flash
    }
    /// True when the items changed since the last call.
    pub fn needs_bind(&mut self, revision: u64) -> bool {
        self.revision.replace(revision) != Some(revision)
    }
    /// Drops the runtimes of gone items.
    pub fn retain(&mut self, mut keep: impl FnMut(ItemId) -> bool) {
        self.runtimes.retain(|id, _| keep(*id));
        self.pending.retain(|(id, _)| keep(*id));
    }
    /// A saved state for a gun item that binds later.
    pub fn restore(&mut self, item: ItemId, saved: GunSnapshot) {
        self.pending.push((item, saved));
    }
    /// Gives a gun item its runtime: the saved state, or a fresh gun with `count` rounds
    /// (`RandomSpawned`), or the record count. Returns false when it already had one.
    pub fn bind(
        &mut self,
        item: ItemId,
        gun: usize,
        saved: Option<GunSnapshot>,
        count: Option<f32>,
    ) -> bool {
        if self.runtimes.contains_key(&item) {
            return false;
        }
        let g = &self.guns[gun];
        let mut rt = GunRuntime::new(gun, g, count);
        let pending = self.pending.iter().position(|(id, _)| *id == item);
        let saved = pending.map(|i| self.pending.swap_remove(i).1).or(saved);
        if let Some(s) = saved {
            rt.restore(g, &s);
        }
        self.runtimes.insert(item, rt);
        touch(&mut self.presented, item);
        true
    }
    /// The saved state of a gun item, or `None` at its fresh state.
    pub fn snapshot(&self, item: ItemId) -> Option<GunSnapshot> {
        let rt = self.runtimes.get(&item)?;
        let s = rt.snapshot();
        (s != rt.initial).then_some(s)
    }
    /// Items whose saved state changed. The caller marks them dirty.
    pub fn take_changed(&mut self) -> std::vec::Drain<'_, ItemId> {
        self.changed.drain(..)
    }
    /// Items whose part poses changed. The presentation writes them again.
    pub fn take_presented(&mut self) -> std::vec::Drain<'_, ItemId> {
        self.presented.drain(..)
    }
    pub fn tracers(&self, index: usize) -> &[ParticleOutput] {
        self.tracers[index].particles()
    }
    pub fn take_sounds(&mut self) -> impl Iterator<Item = ShotSound> + '_ {
        self.sounds.drain(..)
    }
    /// Bullet holes of this step. `DecalStore` keeps them.
    pub fn take_decals(&mut self) -> impl Iterator<Item = DecalSpawn> + '_ {
        self.decals.drain(..)
    }
    /// The next food item that a shot hits. The caller takes every hit after each step.
    pub fn take_food(&mut self) -> Option<ItemId> {
        self.foods.pop()
    }
    /// The next item that a shot hits. The caller takes every hit after each step.
    pub fn take_strike(&mut self) -> Option<Strike> {
        self.strikes.pop()
    }
    /// The next shot hit on a collider without an item. The caller takes every hit after each step.
    pub fn take_creature_hit(&mut self) -> Option<CreatureHit> {
        self.creature_hits.pop()
    }
    /// `fpscontroller` crosshair of the gun in the hand.
    pub fn crosshair(&self) -> Option<Crosshair> {
        let rt = self.runtimes.get(&self.current?)?;
        let g = &self.guns[rt.gun];
        let r = &g.record;
        let ready = rt.can_shoot(r) && (r.infinite || rt.ammo.count > 0.);
        Some(Crosshair {
            sprite: g.crosshair,
            color: if ready {
                r.crosshair.color
            } else {
                r.crosshair.empty_color
            },
        })
    }
    /// `flash.intensity` of a gun item, or `None` while the flash is off.
    pub fn flash(&self, item: ItemId) -> Option<f32> {
        let rt = self.runtimes.get(&item)?;
        let f = self.guns[rt.gun].record.flash.as_ref()?;
        let start = rt.actually_shot_time;
        (self.time - start <= f.time)
            .then(|| curve(&f.curve, progress(start, f.time, self.time)) * f.intensity)
    }
    /// A flash light change: the gun item, its light node and the intensity, or `None` when
    /// the light turns off.
    pub fn flash_light(&mut self) -> Option<(ItemId, u32, Option<f32>)> {
        let on = self
            .current
            .and_then(|id| Some((id, self.flash_node(id)?, self.flash(id)?)));
        match on {
            Some((id, node, intensity)) => {
                self.lit = Some(id);
                Some((id, node, Some(intensity)))
            }
            None => {
                let id = self.lit.take()?;
                Some((id, self.flash_node(id)?, None))
            }
        }
    }
    /// `ReloadLHPos` of the gun in the hand, in the item root frame.
    pub fn left_grip(&self, item: ItemId) -> Option<Pose> {
        let rt = self.runtimes.get(&item)?;
        self.guns[rt.gun].hand.as_ref().map(|_| rt.hand)
    }
    /// R press timing. Returns true on the release of a tap (`reloadUp` within
    /// `holdButtonTime`).
    pub fn reload_key(&mut self, dt: f32, pressed: bool, held: bool, released: bool) -> bool {
        if pressed {
            self.reload_hold = Some(0.);
        }
        let hold = self.reload_hold;
        if held && let Some(h) = &mut self.reload_hold {
            *h += dt;
        }
        if !released {
            return false;
        }
        self.reload_hold = None;
        hold.is_some_and(|h| h < HOLD_BUTTON_TIME)
    }
    /// The gun's own ammo as a reload source, when `HasMag` lets it give rounds.
    pub fn gun_ammo(&self, item: ItemId) -> Option<AmmoItem> {
        let rt = self.runtimes.get(&item)?;
        rt.has_mag(&self.guns[rt.gun].record.kind)
            .then_some(AmmoItem {
                ammo: rt.ammo,
                is_mag: false,
            })
    }
    /// `weaponscript.CanReload(from)`.
    pub fn can_reload(&self, item: ItemId, from: &AmmoItem) -> bool {
        self.runtimes.get(&item).is_some_and(|rt| {
            rt.can(&self.guns[rt.gun].record) && accepts(&rt.ammo, &from.ammo, from.is_mag)
        })
    }
    /// `weaponscript.CanUnload(to)`.
    pub fn can_unload(&self, item: ItemId, to: &AmmoItem) -> bool {
        self.runtimes.get(&item).is_some_and(|rt| {
            let r = &self.guns[rt.gun].record;
            rt.can(r)
                && unloads(&r.kind)
                && rt.has_mag(&r.kind)
                && accepts(&to.ammo, &rt.ammo, false)
        })
    }
    /// True when `Reload(other, unload: true)` moves rounds out of this gun.
    pub fn unloads(&self, item: ItemId) -> bool {
        self.record(item).is_some_and(|r| unloads(&r.kind))
    }
    /// `weaponscript.Reload(other, unload)` at the gun `position`. `held`: `other` is picked up.
    pub fn reload(
        &mut self,
        item: ItemId,
        other: &mut AmmoItem,
        unload: bool,
        held: bool,
        position: [f64; 3],
    ) -> Reloaded {
        let Some(rt) = self.runtimes.get_mut(&item) else {
            return Reloaded::default();
        };
        let g = &self.guns[rt.gun];
        rt.reload_time = self.time;
        let result = kind_reload(rt, &g.record.kind, g.bullets.len(), other, unload, held);
        if let Some((clip, volume)) = g.reload {
            let pitch = self.random.range(RANDOM_PITCH[0], RANDOM_PITCH[1]);
            self.sounds.push(ShotSound {
                clip,
                gain: volume,
                pitch,
                position,
            });
        }
        touch(&mut self.changed, item);
        touch(&mut self.presented, item);
        result
    }
    /// `magPos` of a magazine gun in physics space, and the magazine prefab.
    pub fn mag_drop(
        &self,
        item: ItemId,
        items: &ItemStore,
        physics: &PhysicsWorld,
    ) -> Option<(Pose, &SourceId)> {
        let g = &self.guns[self.runtimes.get(&item)?.gun];
        let GunKind::Magazine { mag_prefab, .. } = &g.record.kind else {
            return None;
        };
        Some((root_pose(items, physics, item)? * g.mag_pos?, mag_prefab))
    }
    /// `weaponscript.Csoletores(LOOKX, LOOKY)` while R is held: the break barrel moves.
    pub fn break_barrel(&mut self, item: ItemId, look: [f32; 2], position: [f64; 3]) {
        let Some(rt) = self.runtimes.get_mut(&item) else {
            return;
        };
        let g = &self.guns[rt.gun];
        let fr = &g.record.fr;
        if !fr.breakable {
            return;
        }
        self.breaking = Some(item);
        let mut delta = 0.;
        if fr.use_x {
            delta = look[0];
        }
        if fr.use_y {
            delta = look[1];
        }
        if delta != 0. {
            rt.move_frames = MOVE_FRAMES;
        }
        rt.fr = (rt.fr + delta * fr.fr_speed).clamp(0., 1.);
        let (value, last) = (rt.fr, rt.last_fr);
        // CsoletoresSound: SSource plays one clip at a time.
        let mut play = |clip: Option<(u32, f32)>, random: &mut UnityRandom| {
            if let Some((clip, gain)) = clip {
                let pitch = random.range(SOURCE_PITCH[0], SOURCE_PITCH[1]);
                self.sounds.push(ShotSound {
                    clip,
                    gain,
                    pitch,
                    position,
                });
            }
        };
        if let Some(clip) = g.r#move
            && value != last
            && !rt.moving_sound
        {
            rt.moving_sound = true;
            let gain = ((value - last).abs() * self.dt * 1000.).min(1.);
            play(Some((clip, gain)), &mut self.random);
        }
        if value != 0. && last == 0. {
            play(g.open, &mut self.random);
        }
        if g.open_end.is_some() && value == 1. && last != 1. {
            play(g.open_end, &mut self.random);
            if let GunKind::BbGun {
                power_gain,
                max_power,
                ..
            } = g.record.kind
            {
                rt.power = felhuzva(rt.power, power_gain, max_power);
            }
        }
        if value == 0. && last != 0. {
            play(g.close, &mut self.random);
        }
        // Refresh.
        rt.last_fr = value;
        if value != last {
            touch(&mut self.changed, item);
            touch(&mut self.presented, item);
        }
    }
    /// `weaponscript.RWheel`: R and scroll queue revolver turns.
    pub fn wheel(&mut self, item: ItemId, scroll: f32) {
        let Some(rt) = self.runtimes.get_mut(&item) else {
            return;
        };
        if !matches!(self.guns[rt.gun].record.kind, GunKind::Revolver { .. }) {
            return;
        }
        if scroll < 0. {
            rt.queued += 1;
        } else if scroll > 0. {
            rt.queued -= 1;
        }
    }
    /// `TRecoil` node and its local pose of a gun item.
    fn recoil(&self, rt: &GunRuntime) -> Option<(Part, Pose)> {
        let gun = &self.guns[rt.gun];
        let (r, (translation, rotation)) = (gun.record.recoil.as_ref()?, gun.recoil?);
        let start = rt.actually_shot_time;
        let t = curve(&r.curve, progress(start, r.duration, self.time));
        Some((
            gun.recoil_part?,
            Pose::from_parts(
                Vector::ZERO.lerp(translation, t),
                Rotation::IDENTITY.lerp(rotation, t),
            ),
        ))
    }
    /// Local poses of the moved gun parts (`pose`) and the hidden nodes (`hide`) of an item.
    /// Nodes index the world prototype.
    pub fn parts(
        &self,
        item: ItemId,
        mut pose: impl FnMut(u32, Transform),
        mut hide: impl FnMut(u32),
    ) {
        let Some(rt) = self.runtimes.get(&item) else {
            return;
        };
        let g = &self.guns[rt.gun];
        let r = &g.record;
        // Refresh: barrels and moving parts follow FRValue.
        for (b, part) in r.barrels.iter().zip(&g.barrels) {
            let Some(axis) = b.axis else { continue };
            let t = b.curve.as_ref().map_or(rt.fr, |c| curve(c, rt.fr));
            let mut e = [0.; 3];
            e[axis as usize] = if b.inv { -1. } else { 1. } * b.max_angle * t.clamp(0., 1.);
            pose(
                part.node,
                Transform {
                    rotation: unity_euler(e),
                    ..part.rest
                },
            );
        }
        for (m, n) in r.moving_parts.iter().zip(&g.moving) {
            let t = if m.inv { 1. - rt.fr } else { rt.fr };
            let p = n.t0.lerp(n.t1, t.clamp(0., 1.));
            let local = (n.parent.rotation.inverse() * (p - n.parent.translation)) / n.scale;
            pose(
                n.part.node,
                Transform {
                    translation: local.to_array(),
                    ..n.part.rest
                },
            );
        }
        for (t, part) in r.triggers.iter().zip(&g.triggers) {
            let start = if t.dont_work_without_shoot {
                rt.actually_shot_time
            } else {
                rt.anim_time
            };
            let f = progress(start, t.duration, self.time);
            let mut local = part.rest;
            if t.use_angle {
                local.rotation = euler(t.def_angle)
                    .lerp(euler(t.angle), curve(&t.angle_curve, f))
                    .to_array();
            }
            if t.use_pos {
                local.translation = unity_position(t.def_pos)
                    .lerp(unity_position(t.pos), curve(&t.pos_curve, f))
                    .to_array();
            }
            pose(part.node, local);
        }
        for (part, rotation) in g.trigger_triggers.iter().zip(&rt.trigger_rotations) {
            pose(
                part.node,
                Transform {
                    rotation: rotation.to_array(),
                    ..part.rest
                },
            );
        }
        if let Some((part, local)) = self.recoil(rt) {
            pose(
                part.node,
                Transform {
                    translation: local.translation.to_array(),
                    rotation: local.rotation.to_array(),
                    scale: part.rest.scale,
                },
            );
        }
        match r.kind {
            GunKind::Revolver { .. } => {
                if let Some(c) = &g.cylinder {
                    pose(
                        c.part.node,
                        Transform {
                            rotation: (Rotation::from_array(c.part.rest.rotation)
                                * cylinder_turn(c, rt.steps(g, self.time)))
                            .to_array(),
                            ..c.part.rest
                        },
                    );
                }
                for (i, node) in g.bullets.iter().enumerate() {
                    if rt.chambers & 1 << i == 0 {
                        hide(*node);
                    }
                }
            }
            GunKind::BbGun { .. } => {
                for (i, node) in g.bullets.iter().enumerate() {
                    if rt.ammo.count <= i as f32 {
                        hide(*node);
                    }
                }
            }
            GunKind::Magazine { .. } => {
                if let Some(node) = g.mag
                    && !rt.has_mag
                {
                    hide(node);
                }
            }
            GunKind::None => {}
        }
    }
    /// One frame of `weaponscript.Update` and `revolverScript.Update`. `pull` is
    /// `PullTrigger` for the held gun.
    pub fn step(&mut self, dt: f32, pull: bool, held: Option<HeldGun>, world: &mut ShotWorld) {
        self.time += dt;
        self.dt = dt;
        for p in &mut self.tracers {
            p.set_origin(world.origin);
            p.advance(dt, world.origin, 0., world.physics);
        }
        self.turn_cylinders(world);
        let item = held.map(|h| h.item);
        if self.current != item
            && let Some(id) = self.current
            && let Some(rt) = self.runtimes.get_mut(&id)
        {
            rt.last_pulling = false;
            rt.already_shot_false = false;
            settle_triggers(&self.guns[rt.gun], rt);
            touch(&mut self.presented, id);
        }
        self.current = item;
        let breaking = self.breaking.take();
        let Some(h) = held else {
            return;
        };
        let Some(mut rt) = self.runtimes.remove(&h.item) else {
            return;
        };
        let gun = &self.guns[h.gun];
        rt.pulling = pull;
        let r = &gun.record;
        if rt.pulling
            && fires(
                r.automatic,
                rt.last_pulling,
                self.time,
                rt.shot_time,
                r.min_shoot_time,
            )
        {
            rt.current_shot_count = pellets(r.shot_count);
            if r.use_conditional_trigger_triggers {
                let count = rt.ammo.count as i32;
                for c in r.conditional_trigger_triggers.iter() {
                    if count == c.ammo_count as i32 {
                        for (dont, used) in rt.dont_trigger.iter_mut().zip(&c.r#use) {
                            *dont = !used;
                        }
                    }
                }
            }
            if let Some(root) = root_pose(world.items, world.physics, h.item) {
                for k in 0..rt.current_shot_count {
                    self.shot(h, root, k, &mut rt, world);
                }
            }
            rt.anim_time = self.time;
            touch(&mut self.changed, h.item);
        }
        let gun = &self.guns[h.gun];
        // TriggerTriggers ease to the pulled or default rotation.
        for (i, t) in gun.record.trigger_triggers.iter().enumerate() {
            let (target, rate) = if rt.pulling && !rt.dont_trigger[i] {
                (t.pulled_rotation, 20.)
            } else {
                (t.def_rotation, 10.)
            };
            rt.trigger_rotations[i] =
                rt.trigger_rotations[i].lerp(euler(target), (dt * rate).min(1.));
        }
        // ReloadLHPos follows the reload, the barrel or the default target.
        if let Some(hand) = &gun.hand {
            let reloading = self.time - rt.reload_time <= RELOADING_TIME;
            let letores = breaking == Some(h.item);
            let mut follow = |target: Pose| {
                rt.hand = Pose::from_parts(
                    rt.hand
                        .translation
                        .lerp(target.translation, (dt * 25.).min(1.)),
                    rt.hand.rotation.lerp(target.rotation, (dt * 10.).min(1.)),
                );
            };
            if let Some(target) = hand.reload.filter(|_| reloading) {
                follow(target);
            }
            if let Some(target) = hand.barrel.filter(|_| letores) {
                follow(target);
            }
            if !reloading && !letores {
                follow(hand.default);
            }
        }
        if !rt.pulling {
            rt.already_shot_false = false;
        }
        rt.last_pulling = rt.pulling;
        rt.pulling = false;
        // BBcsomozgatasv: the SMove clip ends after the last move.
        if rt.move_frames > 0 {
            rt.move_frames -= 1;
        } else {
            rt.moving_sound = false;
        }
        self.runtimes.insert(h.item, rt);
        touch(&mut self.presented, h.item);
    }
    /// `revolverScript.Update` and `DoRotate` of every revolver.
    fn turn_cylinders(&mut self, world: &ShotWorld) {
        for (id, rt) in self.runtimes.iter_mut() {
            let g = &self.guns[rt.gun];
            let Some(c) = &g.cylinder else { continue };
            if let Some((start, _)) = rt.turn {
                touch(&mut self.presented, *id);
                if self.time - start < turn_duration(c.duration, rt.queued) {
                    continue;
                }
                rt.turn = None;
            }
            if rt.queued == 0 {
                continue;
            }
            let dir = rt.queued.signum();
            rt.queued -= dir;
            rt.cylinder += dir;
            rt.turn = Some((self.time, dir));
            touch(&mut self.presented, *id);
            touch(&mut self.changed, *id);
            if let Some((clip, gain)) = g.turn
                && let Some(root) = root_pose(world.items, world.physics, *id)
            {
                let pitch = self.random.range(RANDOM_PITCH[0], RANDOM_PITCH[1]);
                self.sounds.push(ShotSound {
                    clip,
                    gain,
                    pitch,
                    position: absolute(world.origin, root * c.root.translation),
                });
            }
        }
    }
    /// `weaponscript.Shot(k)`.
    fn shot(&mut self, h: HeldGun, root: Pose, k: u32, rt: &mut GunRuntime, world: &mut ShotWorld) {
        let gun = &self.guns[h.gun];
        let r = &gun.record;
        let condition = h.condition.filter(|_| r.condition.is_some());
        let force = power(gun.mass, gun.velocity, r.max_condition_weakness, condition);
        if !rt.can_shoot(r) && !r.fr.play_sound_even_if_cant_shoot {
            return;
        }
        rt.shot_time = self.time;
        let muzzle = root * gun.muzzle;
        let at = absolute(world.origin, muzzle.translation);
        if !(rt.kind_shot(gun, k + 1 >= rt.current_shot_count, self.time) || r.infinite) {
            if !r.automatic_false_shot && rt.already_shot_false {
                return;
            }
            rt.already_shot_false = true;
            if k == 0 {
                play(
                    &mut self.random,
                    &mut self.sounds,
                    &gun.false_shot,
                    r.audio_m,
                    at,
                    true,
                );
            }
            return;
        }
        rt.actually_shot_time = self.time;
        if k == 0 {
            play(
                &mut self.random,
                &mut self.sounds,
                &gun.one_shot,
                r.audio_m,
                at,
                true,
            );
        }
        let forward = muzzle.rotation * -Vector::Z;
        let mut direction = h.view.rotation * -Vector::Z;
        if r.shot_count > 1 {
            let s = self.random.inside_unit_sphere();
            direction = direction * 10. + Vector::new(s[0], s[1], -s[2]) * r.shot_count_spread;
        }
        let direction = direction.normalize_or_zero();
        let mask = r.mask;
        self.cast(h, direction, mask, world);
        self.pushed.clear();
        let gun = &self.guns[h.gun];
        if self.hits.is_empty() {
            shoot_start(
                &mut self.tracers[0],
                world.origin,
                muzzle.translation,
                direction,
            );
        }
        for i in 0..self.hits.len() {
            let hit = self.hits[i];
            let point = h.view.translation + direction * hit.distance;
            shoot_start(
                &mut self.tracers[0],
                world.origin,
                muzzle.translation,
                point - muzzle.translation,
            );
            let rapier = world.physics.rapier();
            let collider = &rapier.colliders[hit.collider];
            let tag = collider_tag(collider.user_data);
            let layer = collider
                .collision_groups()
                .memberships
                .bits()
                .trailing_zeros();
            let parent = collider.parent();
            let item = ((collider.user_data & 0xffff_ffff) as u32)
                .checked_sub(1)
                .map(ItemId)
                .or_else(|| {
                    parent
                        .and_then(|b| world.items.iter().find(|i| i.body == Some(b)).map(|i| i.id))
                });
            if skips(tag, layer) {
                continue;
            }
            // ShootEnd: the end tracer starts 3 m before the hit.
            let back = point - direction * 3.;
            self.tracers[1].emit(1, absolute(world.origin, back), look(direction));
            let at = absolute(world.origin, point);
            let normal_rotation = look(hit.normal);
            if SAND_LAYERS.contains(&layer) {
                let count = self.random.range_int(10, 20) as usize;
                world.hit_particles[SAND].emit(count, at, normal_rotation);
                play(
                    &mut self.random,
                    &mut self.sounds,
                    &gun.hit_sand,
                    1.,
                    at,
                    false,
                );
            } else {
                let kind = decal(tag, layer);
                let clips = if layer == GLASS_LAYER {
                    &gun.hit_glass
                } else {
                    match tag {
                        Some(ColliderTag::Flesh | ColliderTag::Plant) => &gun.hit_flesh,
                        Some(ColliderTag::Wood) => &gun.hit_concrete,
                        _ => {
                            let count = self.random.range_int(10, 20) as usize;
                            world.hit_particles[SPARK].emit(count, at, normal_rotation);
                            &gun.hit_concrete
                        }
                    }
                };
                play(&mut self.random, &mut self.sounds, clips, 1., at, false);
                let scale = 0.3 * self.random.range(0.6, 1.2);
                // LookRotation(normal), then Rotate(forward, roll, Space.World): a turn about local Z.
                let roll = self.random.range(0., 360.).to_radians();
                let rotation = normal_rotation * Rotation::from_rotation_z(roll);
                self.decals.push(DecalSpawn {
                    kind,
                    collider: hit.collider,
                    pose: Pose::from_parts(point + hit.normal * 0.01, rotation),
                    scale,
                });
            }
            if let Some(target) = world.breakables.target_of(world.physics, hit.collider) {
                world
                    .breakables
                    .try_break_shoot(world.records, target, force);
            }
            // ediblescript.Shot(onlySeparate: false). No food item is attachable.
            if let Some(id) = item
                && world.items.get(id).is_some_and(|s| s.food.is_some())
                && !self.foods.contains(&id)
            {
                self.foods.push(id);
            }
            if item.is_none() && self.creature_hits.len() < HIT_CAPACITY {
                self.creature_hits.push(CreatureHit {
                    collider: hit.collider,
                    force,
                    direction,
                });
            }
            // The tank and explosivescript hits of weaponscript.Shot.
            if let Some(id) = item {
                let r = &gun.record;
                let explode = r.explode_stuffs && self.random.range(0., 1.) < r.explode_chance;
                self.strikes.push(Strike {
                    item: id,
                    force,
                    explode,
                    hole: r.hithole.then_some([point, hit.normal]),
                });
            }
            if let Some(body) = parent
                && !self.pushed.contains(&body)
            {
                self.pushed.push(body);
                if world.physics.rapier().bodies[body].is_dynamic() {
                    let impulse = forward * force.sqrt() * 5.;
                    world
                        .physics
                        .edit_body(body, |b| b.apply_impulse_at_point(impulse, point, true));
                }
            }
            if !passes(tag, layer) {
                break;
            }
        }
    }
    /// `Physics.RaycastNonAlloc` with `LM`: the nearest `HIT_CAPACITY` hits, sorted.
    fn cast(&mut self, h: HeldGun, direction: Vector, mask: u32, world: &ShotWorld) {
        self.hits.clear();
        let gun_body = world.items.get(h.item).and_then(|i| i.body);
        let predicate = |_, c: &rapier3d::prelude::Collider| {
            c.is_enabled()
                && c.collision_groups().memberships.bits() & mask != 0
                && (c.parent().is_none() || c.parent() != gun_body)
        };
        let mut filter = QueryFilter::default();
        if let Some(player) = world.player {
            filter = filter.exclude_rigid_body(player);
        }
        filter.predicate = Some(&predicate);
        let ray = Ray::new(h.view.translation, direction);
        let hits = &mut self.hits;
        for (collider, _, hit) in world
            .physics
            .queries(filter)
            .intersect_ray(ray, f32::MAX, true)
        {
            // A ray that starts inside a collider does not hit it.
            if hit.time_of_impact <= 0. {
                continue;
            }
            let at = hits.partition_point(|h| h.distance <= hit.time_of_impact);
            if at < HIT_CAPACITY {
                hits.insert(
                    at,
                    Hit {
                        distance: hit.time_of_impact,
                        collider,
                        normal: hit.normal,
                    },
                );
                hits.truncate(HIT_CAPACITY);
            }
        }
    }
}

/// The item root pose in physics space.
fn root_pose(items: &ItemStore, physics: &PhysicsWorld, item: ItemId) -> Option<Pose> {
    let state = items.get(item)?;
    Some(*physics.rapier().bodies.get(state.body?)?.position() * to_rapier(state.local_pose))
}

/// `TriggerTriggers` of a gun that left the hand rest at their default rotation.
fn settle_triggers(g: &Gun, rt: &mut GunRuntime) {
    for (rotation, t) in rt
        .trigger_rotations
        .iter_mut()
        .zip(&g.record.trigger_triggers)
    {
        *rotation = euler(t.def_rotation);
    }
}

/// What `Reload(other, unload)` leaves for the world to do.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Reloaded {
    /// The magazine `other` went into the gun: remove it.
    pub used: bool,
    /// geppityuscript drops the old magazine at `magPos` with these rounds.
    pub dropped: Option<f32>,
    /// A loose magazine `other` moves to `magPos` before the swap.
    pub to_mag_pos: bool,
}

/// The kind `Reload(other, unload)`.
fn kind_reload(
    rt: &mut GunRuntime,
    kind: &GunKind,
    chambers: usize,
    other: &mut AmmoItem,
    unload: bool,
    held: bool,
) -> Reloaded {
    let mut result = Reloaded::default();
    match *kind {
        GunKind::BbGun { ammo_gain, .. } => {
            let gain = ammo_gain as f32;
            if unload {
                if accepts(&other.ammo, &rt.ammo, false) {
                    other.ammo.reload(&mut rt.ammo, gain);
                }
            } else if accepts(&rt.ammo, &other.ammo, other.is_mag) {
                rt.ammo.reload(&mut other.ammo, gain);
            }
        }
        GunKind::Revolver { .. } => {
            if unload {
                if accepts(&other.ammo, &rt.ammo, false) {
                    other.ammo.reload(&mut rt.ammo, 1.);
                    rt.chambers = unload_chamber(rt.chambers, chambers);
                }
            } else if accepts(&rt.ammo, &other.ammo, other.is_mag) {
                rt.ammo.reload(&mut other.ammo, 1.);
                rt.chambers = load_chamber(rt.chambers, chambers);
            }
        }
        // geppityuscript.Reload: unload drops the magazine with the gun rounds.
        GunKind::Magazine { .. } if unload => drop_magazine(rt, &mut result),
        // A picked-up magazine drops the old one and goes in. A loose magazine moves to
        // `magPos` and swaps rounds; without a magazine in the gun it is used up.
        GunKind::Magazine { .. } if other.is_mag => {
            if held {
                drop_magazine(rt, &mut result);
            } else {
                result.to_mag_pos = true;
            }
            let (gun, mag) = swap_magazine(rt.ammo.count, other.ammo.count);
            rt.ammo.count = gun;
            other.ammo.count = mag;
            result.used = !rt.has_mag;
            rt.has_mag = true;
        }
        _ => {}
    }
    result
}

/// `geppityuscript.Reload(null, unload: true)`.
fn drop_magazine(rt: &mut GunRuntime, result: &mut Reloaded) {
    if rt.has_mag {
        result.dropped = Some(rt.ammo.count);
        rt.ammo.count = 0.;
    }
    rt.has_mag = false;
}

fn unloads(kind: &GunKind) -> bool {
    !matches!(kind, GunKind::None)
}

/// `ammoscript.CanReload`: `to` takes rounds of its type, or a magazine.
pub fn accepts(to: &Ammo, from: &Ammo, from_mag: bool) -> bool {
    to.kind == from.kind && ((to.count < to.maximum && from.count > 0.) || from_mag)
}

/// `revolverScript.Reload`: the first empty chamber fills.
pub fn load_chamber(bits: u32, chambers: usize) -> u32 {
    (0..chambers)
        .find(|i| bits & 1 << i == 0)
        .map_or(bits, |i| bits | 1 << i)
}

/// `revolverScript.Reload(unload)`: the last full chamber empties.
pub fn unload_chamber(bits: u32, chambers: usize) -> u32 {
    (0..chambers)
        .rev()
        .find(|i| bits & 1 << i != 0)
        .map_or(bits, |i| bits & !(1 << i))
}

/// `SetIntAmmo` of both sides: the gun takes the magazine rounds and the magazine the gun's.
pub fn swap_magazine(gun: f32, mag: f32) -> (f32, f32) {
    (mag.trunc(), gun.trunc())
}

/// `bbgunscript.Felhuzva`.
pub fn felhuzva(power: u32, gain: u32, max: u32) -> u32 {
    power.saturating_add(gain).min(max)
}

/// `revolverScript.DoRotate` duration with `tekeres` queued turns.
pub fn turn_duration(duration: f32, queued: i32) -> f32 {
    duration / queued.abs().clamp(1, MAX_TURN_DIVISOR) as f32
}

/// `Quaternion.Euler(Axis * angle * steps)`.
fn cylinder_turn(c: &Cylinder, steps: f32) -> Rotation {
    euler(c.axis.map(|a| a * c.angle * steps))
}

/// `revolverScript.GetSelectedBullet`: the chamber nearest `ShootBulletPos`.
fn chamber(c: &Cylinder, steps: f32) -> usize {
    let cylinder = c.root * Pose::from_parts(Vector::ZERO, cylinder_turn(c, steps));
    nearest(c.chambers.iter().map(|p| cylinder * *p), c.shoot)
}

/// The index of the first point nearest `target`.
pub fn nearest(points: impl Iterator<Item = Vector>, target: Vector) -> usize {
    let mut best = (f32::MAX, 0);
    for (i, p) in points.enumerate() {
        let d = (target - p).length_squared();
        if d < best.0 {
            best = (d, i);
        }
    }
    best.1
}

fn low_bits(n: usize) -> u32 {
    if n >= MAX_CHAMBERS {
        u32::MAX
    } else {
        (1 << n) - 1
    }
}

fn touch(list: &mut Vec<ItemId>, item: ItemId) {
    if !list.contains(&item) {
        list.push(item);
    }
}

fn euler(degrees: [f32; 3]) -> Rotation {
    Rotation::from_array(unity_euler(degrees))
}

/// A source local position in converted axes.
fn unity_position(p: [f32; 3]) -> Vector {
    Vector::new(p[0], p[1], -p[2])
}

/// Node poses, parts and clip ids of one record.
fn gun(
    r: &ReferenceManifest,
    record: &GunReference,
    clips: &[(SourceId, u32, f32)],
) -> Result<Gun, ReferenceError> {
    let prototype = r
        .world
        .prototypes
        .iter()
        .find(|p| p.source == record.prototype.0)
        .ok_or_else(|| ReferenceError("Missing gun world prototype".into()))?;
    let index = |s: &SourceId| {
        prototype
            .nodes
            .iter()
            .position(|n| n.source == *s)
            .ok_or_else(|| ReferenceError("Missing gun node".into()))
    };
    let root = |s: &SourceId| Ok::<_, ReferenceError>(prototype.root_relative(index(s)?).0);
    let part = |s: &SourceId| {
        let i = index(s)?;
        Ok::<_, ReferenceError>(Part {
            node: i as u32,
            rest: prototype.nodes[i].transform,
        })
    };
    let clip = |s: &SourceId| {
        clips
            .iter()
            .find(|(c, _, _)| c == s)
            .map(|(_, id, _)| *id)
            .ok_or_else(|| ReferenceError("Missing gun clip".into()))
    };
    let ids = |sources: &[SourceId]| sources.iter().map(clip).collect::<Result<Vec<_>, _>>();
    let sound = |s: &Option<GunSound>| s.as_ref().map(|s| Ok((clip(&s.clip)?, s.volume)));
    let c = &record.clips;
    let s = &record.sounds;
    let kind = record.ammo.r#type as usize;
    let moving = record
        .moving_parts
        .iter()
        .map(|m| {
            let part = part(&m.node)?;
            let parent = prototype.nodes[part.node as usize]
                .parent
                .map(|p| parent_frame(prototype, p as usize))
                .unwrap_or((Pose::IDENTITY, Vector::ONE));
            Ok(MovingNode {
                part,
                parent: parent.0,
                scale: parent.1,
                t0: root(&m.t0)?.translation,
                t1: root(&m.t1)?.translation,
            })
        })
        .collect::<Result<_, ReferenceError>>()?;
    let (cylinder, bullets, mag, turn) = match &record.kind {
        GunKind::Revolver {
            chambers,
            cylinder,
            axis,
            angle,
            duration,
            shoot_bullet_pos,
            turn_clip,
            turn_volume,
            ..
        } => {
            if chambers.len() > MAX_CHAMBERS {
                return Err(ReferenceError("Too many revolver chambers".into()));
            }
            let at = root(cylinder)?;
            let c = Cylinder {
                part: part(cylinder)?,
                root: at,
                chambers: chambers
                    .iter()
                    .map(|b| Ok(at.inverse() * root(b)?.translation))
                    .collect::<Result<_, ReferenceError>>()?,
                shoot: root(shoot_bullet_pos)?.translation,
                axis: *axis,
                angle: *angle,
                duration: *duration,
            };
            let nodes = chambers
                .iter()
                .map(|b| Ok(index(b)? as u32))
                .collect::<Result<_, ReferenceError>>()?;
            let turn = turn_clip
                .as_ref()
                .map(|t| Ok::<_, ReferenceError>((clip(t)?, *turn_volume)))
                .transpose()?;
            (Some(c), nodes, None, turn)
        }
        GunKind::BbGun { bullets, .. } => (
            None,
            bullets
                .iter()
                .map(|b| Ok(index(b)? as u32))
                .collect::<Result<_, ReferenceError>>()?,
            None,
            None,
        ),
        GunKind::Magazine { mag, .. } => (None, Vec::new(), Some(index(mag)? as u32), None),
        GunKind::None => (None, Vec::new(), None, None),
    };
    let hand = record
        .reload_hand
        .as_ref()
        .map(|h| {
            Ok::<_, ReferenceError>(ReloadHand {
                rest: root(&h.node)?,
                default: root(&h.default)?,
                barrel: h.barrel.as_ref().map(root).transpose()?,
                reload: h.reload.as_ref().map(root).transpose()?,
            })
        })
        .transpose()?;
    let mag_pos = match &record.kind {
        GunKind::Magazine { mag_pos, .. } => Some(root(mag_pos)?),
        _ => None,
    };
    Ok(Gun {
        mag_pos,
        muzzle: root(&record.muzzle)?,
        mass: r.guns.ammo_mass[kind],
        velocity: r.guns.ammo_velocity[kind],
        one_shot: ids(&c.one_shot)?,
        false_shot: ids(&c.false_shot)?,
        hit_sand: ids(&c.hit_sand)?,
        hit_concrete: ids(&c.hit_concrete)?,
        hit_flesh: ids(&c.hit_flesh)?,
        hit_glass: ids(&c.hit_glass)?,
        reload: sound(&s.reload).transpose()?,
        open: sound(&s.open).transpose()?,
        open_end: sound(&s.open_end).transpose()?,
        close: sound(&s.close).transpose()?,
        r#move: s.r#move.as_ref().map(clip).transpose()?,
        turn,
        recoil: record.recoil.as_ref().map(|v| {
            (
                unity_position(v.pos),
                Rotation::from_array(unity_euler(v.rot)),
            )
        }),
        recoil_part: record.recoil.as_ref().map(|v| part(&v.node)).transpose()?,
        barrels: record
            .barrels
            .iter()
            .map(|b| part(&b.node))
            .collect::<Result<_, _>>()?,
        moving,
        triggers: record
            .triggers
            .iter()
            .map(|t| part(&t.node))
            .collect::<Result<_, _>>()?,
        trigger_triggers: record
            .trigger_triggers
            .iter()
            .map(|t| part(&t.node))
            .collect::<Result<_, _>>()?,
        cylinder,
        bullets,
        mag,
        flash: record
            .flash
            .as_ref()
            .map(|f| Ok::<_, ReferenceError>(index(&f.node)? as u32))
            .transpose()?,
        hand,
        crosshair: record
            .crosshair
            .icon
            .as_ref()
            .and_then(|icon| r.ui.sprite(icon)),
        record: record.clone(),
    })
}

/// A node frame in the prototype root frame, with its scale for child positions.
fn parent_frame(prototype: &WorldPrototypeReference, index: usize) -> (Pose, Vector) {
    let (pose, scale) = prototype.root_relative(index);
    if prototype.nodes[index].parent.is_none() {
        // The root frame itself.
        return (Pose::IDENTITY, Vector::ONE);
    }
    (pose, scale)
}

/// `weaponscript.Shot` tempForce: kinetic energy at the condition factor.
pub fn power(mass: f32, velocity: f32, weakness: f32, condition: Option<f32>) -> f32 {
    let factor = condition.map_or(1., |state| {
        let t = inverse_lerp(4., 0., state);
        weakness + (1. - weakness) * t
    });
    0.5 * mass * (velocity * factor).powi(2)
}

/// `currentShotCount`: at least one ray.
pub fn pellets(shot_count: u32) -> u32 {
    shot_count.max(1)
}

/// `weaponscript.Update`: a pulled trigger fires when automatic or newly pulled, and after
/// `minShootTime`.
pub fn fires(automatic: bool, last_pulling: bool, time: f32, shot_time: f32, min: f32) -> bool {
    (automatic || !last_pulling) && time - shot_time >= min
}

/// `CanShoot` and `Can` FR checks.
pub fn fr_allows(rule: &GunBreakRule, value: f32) -> bool {
    (rule.equal && value == rule.value)
        || (rule.bigger && value > rule.value)
        || (rule.smaller && value < rule.value)
        || (rule.near && (value - rule.value).abs() < rule.near_value)
}

/// The shot ignores this hit: tag `Ignore` or the player layer.
pub fn skips(tag: Option<ColliderTag>, layer: u32) -> bool {
    tag == Some(ColliderTag::Ignore) || layer == PLAYER_LAYER
}

/// The shot continues through `Plant`, glass and `Cobweb`.
pub fn passes(tag: Option<ColliderTag>, layer: u32) -> bool {
    matches!(tag, Some(ColliderTag::Plant | ColliderTag::Cobweb)) || layer == GLASS_LAYER
}

/// `mainscript.GetDecal`.
fn decal(tag: Option<ColliderTag>, layer: u32) -> u8 {
    match tag {
        Some(ColliderTag::Metal) => DECAL_METAL,
        Some(ColliderTag::Flesh) => DECAL_FLESH,
        Some(ColliderTag::Wood) => DECAL_WOOD,
        Some(ColliderTag::Plant) => DECAL_PLANT,
        _ if layer == GLASS_LAYER => DECAL_GLASS,
        _ => 0,
    }
}

/// `mainscript.ShootStart`.
fn shoot_start(p: &mut ParticleState, origin: [f64; 3], start: Vector, direction: Vector) {
    p.emit(1, absolute(origin, start), look(direction));
}

/// `Quaternion.LookRotation(dir, up)`, keeping a zero direction's identity.
fn look(direction: Vector) -> Rotation {
    if direction == Vector::ZERO {
        Rotation::IDENTITY
    } else {
        super::melee::look_rotation(direction)
    }
}

/// `mainscript.PlayClipAtPoint` of a random clip.
fn play(
    random: &mut UnityRandom,
    sounds: &mut Vec<ShotSound>,
    clips: &[u32],
    gain: f32,
    position: [f64; 3],
    random_pitch: bool,
) {
    if clips.is_empty() {
        return;
    }
    let clip = clips[random.range_int(0, clips.len() as i32) as usize];
    let pitch = if random_pitch {
        random.range(RANDOM_PITCH[0], RANDOM_PITCH[1])
    } else {
        1.
    };
    sounds.push(ShotSound {
        clip,
        gain,
        pitch,
        position,
    });
}

fn curve(c: &TorqueCurve, t: f32) -> f32 {
    if c.keys.is_empty() {
        0.
    } else {
        crate::simulation::engine::torque(c, t)
    }
}

/// `InverseLerp(start, start + duration, time)`. A gun that never ran it is at the end.
fn progress(start: f32, duration: f32, time: f32) -> f32 {
    if start.is_finite() {
        inverse_lerp(start, start + duration, time)
    } else {
        1.
    }
}

/// Unity `Mathf.InverseLerp`.
fn inverse_lerp(a: f32, b: f32, value: f32) -> f32 {
    if a == b {
        0.
    } else {
        ((value - a) / (b - a)).clamp(0., 1.)
    }
}

fn absolute(origin: [f64; 3], local: Vector) -> [f64; 3] {
    std::array::from_fn(|i| origin[i] + local[i] as f64)
}
