//! Source melee swing: `weaponhandlingscript` (input, animator, swing sound) and
//! `meleeweaponscript` (hand grips, hit box, force, hit particles, hit sound, digging,
//! breakable damage, food cuts, tank and explosive hits).
use super::buried::BuriedItems;
use super::items::ItemStore;
use crate::animation::controller::{MotionController, StateEvent};
use crate::damage::Breakables;
use crate::particles::ParticleState;
use crate::reference::{
    BreakableReference, ColliderTag, ItemId, MeleeMotion, MeleeWeaponReference, ReferenceError,
    ReferenceManifest, SourceId,
};
use crate::simulation::FIXED_DT;
use crate::vehicle::{PhysicsWorld, VehiclePose, collider_tag, to_rapier};
use crate::world::unity_random::UnityRandom;
use rapier3d::math::{Pose, Rotation, Vector};
use rapier3d::prelude::{ColliderHandle, Cuboid, QueryFilter, RigidBodyHandle};

/// `mainscript.soundHitType`.
const METAL: usize = 0;
const BLOOD: usize = 1;
const PLANT: usize = 2;
const WOOD: usize = 3;
const SAND: usize = 4;
const GLASS: usize = 5;
/// Collider layers with their own hit type.
const PLAYER_LAYER: u32 = 9;
const SAND_LAYER: u32 = 17;
const GLASS_LAYER: u32 = 23;
/// Particle index in `MELEE_HIT_PARTICLES` order for each hit type. Glass has none.
const HIT_PARTICLE: [Option<usize>; 6] = [Some(0), Some(1), Some(2), Some(3), Some(4), None];
/// `weaponhandlingscript`: the swing sound waits `0.1 / speed` seconds.
const SWING_SOUND_DELAY: f32 = 0.1;
/// `meleeweaponscript`: the hit sound waits 0.125 s after the swing starts.
const HIT_SOUND_DELAY: f32 = 0.125;
const OVERLAP_CAPACITY: usize = 32;

/// `meleeweaponscript.attackType` triggers: front, left, right, up, stab.
const TRIGGERS: [&str; 5] = [
    "AttackForward",
    "AttackLeft",
    "AttackRight",
    "AttackBack",
    "AttackStab",
];

#[derive(Clone, Copy, Debug, Default)]
pub struct MeleeInput {
    pub pressed: bool,
    pub released: bool,
    pub held: bool,
    pub back: bool,
    pub left: bool,
    pub right: bool,
}
impl MeleeInput {
    /// `weaponhandlingscript`: S with A or D is a stab, then D, A, S, else front.
    fn attack(self) -> usize {
        if self.back && (self.right || self.left) {
            4
        } else if self.right {
            2
        } else if self.left {
            1
        } else if self.back {
            3
        } else {
            0
        }
    }
}

/// A one-shot sound for `mainscript.PlayClipAtPoint`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MeleeSound {
    pub clip: u32,
    pub gain: f32,
    pub position: [f64; 3],
}

/// One `meleeweaponscript` record with its poses and clips.
struct Weapon {
    record: MeleeWeaponReference,
    /// Index in `MeleeState::motions`.
    motion: usize,
    /// `T` and `Trigger` in the item root frame, and the `OverlapBox` half extents.
    tip: Pose,
    trigger: Pose,
    half_extents: Vector,
    swing_clips: Vec<u32>,
    /// `LHs` and `RHs` in the item root frame, and the start of `LeftHandPos` and
    /// `RightHandPos`. A side without poses or grip has no grip.
    hand_poses: [Vec<Pose>; 2],
    grips: [Option<Pose>; 2],
}

/// One weapon controller with its parameter indices.
struct Motion {
    motion: MeleeMotion,
    controller: MotionController,
    triggers: [usize; 5],
    hit_trigger: usize,
}

pub struct MeleeState {
    weapons: Vec<Weapon>,
    motions: Vec<Motion>,
    /// The held weapon. `None` is the animator off.
    current: Option<usize>,
    /// `meleeweaponscript.currentAttackType`: 0 idle, 1 front, 2 left, 3 right, 4 up, 5 stab.
    attack_type: usize,
    /// `LeftHandPos` and `RightHandPos` of the held weapon in the item root frame.
    grips: [Pose; 2],
    /// `mainscript.meleeHitSounds` per hit type, with clip lengths.
    hit_clips: [Vec<(u32, f32)>; 6],
    hit_volumes: [f32; 6],
    particles: [ParticleState; 5],
    random: UnityRandom,
    time: f32,
    speed: f32,
    attacking: i32,
    attacking_now: bool,
    started_attack: bool,
    swing_sounded: bool,
    attack_started: f32,
    last_tip: Option<Vector>,
    sounded: bool,
    has_to_sound: bool,
    next_sound: f32,
    attacked: f32,
    hit_type: usize,
    hit_point: Vector,
    overlaps: Vec<ColliderHandle>,
    sounds: Vec<MeleeSound>,
    /// Food items that `hurtFood` hits cut this frame.
    cuts: Vec<ItemId>,
    /// Items hit this frame with `damageTank`: tanks and explosive props try to explode.
    strikes: Vec<(ItemId, f32)>,
}

impl MeleeState {
    pub fn new(r: &ReferenceManifest) -> Result<Self, ReferenceError> {
        let melee = &r.melee;
        let motions = melee
            .motions
            .iter()
            .map(|motion| {
                let parameter = |name| {
                    MotionController::parameter(motion, name)
                        .ok_or_else(|| ReferenceError(format!("Missing melee parameter {name}")))
                };
                let mut triggers = [0; 5];
                for (slot, name) in triggers.iter_mut().zip(TRIGGERS) {
                    *slot = parameter(name)?;
                }
                Ok(Motion {
                    controller: MotionController::new(motion),
                    triggers,
                    hit_trigger: parameter("Hit")?,
                    motion: motion.clone(),
                })
            })
            .collect::<Result<_, ReferenceError>>()?;
        let clips = clip_table(r);
        let clip = |source: &SourceId| {
            clips
                .iter()
                .find(|(s, _, _)| s == source)
                .map(|(_, id, length)| (*id, *length))
                .ok_or_else(|| ReferenceError("Missing melee clip".into()))
        };
        let mut hit_clips: [Vec<(u32, f32)>; 6] = Default::default();
        for (group, sources) in hit_clips.iter_mut().zip(&melee.hit_clips) {
            *group = sources.iter().map(clip).collect::<Result<_, _>>()?;
        }
        let weapons = melee
            .weapons
            .iter()
            .map(|w| weapon(r, w, &clip))
            .collect::<Result<_, _>>()?;
        Ok(Self {
            weapons,
            motions,
            current: None,
            attack_type: 0,
            grips: [Pose::IDENTITY; 2],
            hit_clips,
            hit_volumes: melee.hit_volumes,
            particles: std::array::from_fn(|i| {
                ParticleState::emit_only(&melee.particles[i], 0x6d65_6c65 + i as u64)
            }),
            random: UnityRandom::new(0x6d65_6c65),
            time: 0.,
            speed: 1.,
            attacking: 0,
            attacking_now: false,
            started_attack: false,
            swing_sounded: false,
            attack_started: 0.,
            last_tip: None,
            sounded: false,
            has_to_sound: false,
            next_sound: 0.,
            attacked: 0.,
            hit_type: METAL,
            hit_point: Vector::ZERO,
            overlaps: Vec::with_capacity(OVERLAP_CAPACITY),
            sounds: Vec::with_capacity(4),
            cuts: Vec::with_capacity(OVERLAP_CAPACITY),
            strikes: Vec::with_capacity(OVERLAP_CAPACITY),
        })
    }
    /// The weapon whose pickup rule is the held item's rule.
    pub fn weapon_for(&self, pickup: &SourceId) -> Option<usize> {
        weapon_index(self.weapons.iter().map(|w| &w.record.pickup), pickup)
    }
    pub fn attack_type(&self) -> usize {
        self.attack_type
    }
    /// `LeftHandPos` and `RightHandPos` in the item root frame while the held weapon moves
    /// them to its `LHs` and `RHs`.
    pub fn grips(&self) -> [Option<Pose>; 2] {
        let Some(w) = self.current else {
            return [None; 2];
        };
        let weapon = &self.weapons[w];
        std::array::from_fn(|side| weapon.grips[side].and(Some(self.grips[side])))
    }
    pub fn particles(&self, index: usize) -> &[crate::particles::ParticleOutput] {
        self.particles[index].particles()
    }
    /// `mainscript.Particles`, shared with the gun hits.
    pub fn particles_mut(&mut self) -> &mut [ParticleState; 5] {
        &mut self.particles
    }
    pub fn take_sounds(&mut self) -> impl Iterator<Item = MeleeSound> + '_ {
        self.sounds.drain(..)
    }
    /// The next hit item and `damageTank`. The caller takes every hit after each step.
    pub fn take_strike(&mut self) -> Option<(ItemId, f32)> {
        self.strikes.pop()
    }
    /// The next food item to cut. The caller takes every cut after each step.
    pub fn take_cut(&mut self) -> Option<ItemId> {
        self.cuts.pop()
    }
    pub fn rebase(&mut self, shift: [f32; 3]) {
        if let Some(tip) = &mut self.last_tip {
            *tip += Vector::from_array(shift);
        }
        self.hit_point += Vector::from_array(shift);
    }
    /// Animated RH pose in its parent frame, or `None` for `defRH`.
    pub fn hand(&self) -> Option<VehiclePose> {
        let m = &self.motions[self.weapons[self.current?].motion];
        Some(m.controller.pose(&m.motion))
    }
    /// One frame. `held` is the item in the right hand and its weapon index.
    #[allow(clippy::too_many_arguments)]
    pub fn step(
        &mut self,
        input: MeleeInput,
        dt: f32,
        held: Option<(ItemId, usize)>,
        items: &mut ItemStore,
        physics: &mut PhysicsWorld,
        buried: &mut BuriedItems,
        breakables: &mut Breakables,
        records: &[BreakableReference],
        player: Option<RigidBodyHandle>,
        origin: [f64; 3],
    ) {
        self.time += dt;
        for p in &mut self.particles {
            p.set_origin(origin);
            p.advance(dt, origin, 0., physics);
        }
        let held = held.and_then(|(id, w)| {
            let item = items.get(id)?;
            let body = physics.rapier().bodies.get(item.body?)?;
            Some((*body.position() * to_rapier(item.local_pose), w))
        });
        if resets(self.current, held.map(|(_, w)| w)) {
            self.leave();
        }
        let Some((root, w)) = held else {
            return;
        };
        if self.current.is_none() {
            self.current = Some(w);
            self.grips = self.weapons[w].grips.map(|g| g.unwrap_or(Pose::IDENTITY));
        }
        let weapon = &self.weapons[w];
        let m = &mut self.motions[weapon.motion];
        let melee_speed = weapon.record.speed;
        m.controller.set_speed(self.speed);
        if input.pressed {
            self.speed = melee_speed;
            m.controller.reset_trigger(m.hit_trigger);
            m.controller.set_trigger(m.triggers[input.attack()]);
            self.attack_type = input.attack() + 1;
            self.started_attack = true;
        }
        if input.released {
            self.speed = melee_speed;
            self.attack_started = self.time;
        }
        if self.attacking_now {
            if !self.swing_sounded
                && self.time - self.attack_started > SWING_SOUND_DELAY / melee_speed
                && !weapon.swing_clips.is_empty()
            {
                self.swing_sounded = true;
                let index = self.random.range_int(0, weapon.swing_clips.len() as i32) as usize;
                self.sounds.push(MeleeSound {
                    clip: weapon.swing_clips[index],
                    gain: weapon.record.attack_volume,
                    position: absolute(origin, root.translation),
                });
            }
        } else if !self.started_attack {
            self.attack_type = 0;
        }
        // meleeweaponscript.Update: the grips move to the pose of the attack type.
        for (side, poses) in weapon.hand_poses.iter().enumerate() {
            if let Some(target) = poses.get(self.attack_type) {
                let grip = &mut self.grips[side];
                grip.translation = grip
                    .translation
                    .lerp(target.translation, (dt * 16.).min(1.));
                grip.rotation = grip.rotation.lerp(target.rotation, (dt * 8.).min(1.));
            }
        }
        self.attacking_now = self.attacking > 0 && self.speed > 0.;
        let tip = (root * weapon.tip).translation;
        if self.attacking_now {
            let trigger = root * weapon.trigger;
            self.hit(
                w, tip, trigger, dt, items, physics, buried, breakables, records, player, origin,
            );
        } else {
            self.sounded = false;
            self.attacked = self.time;
        }
        self.last_tip = Some(tip);
        if self.has_to_sound
            && !self.sounded
            && self.time > self.next_sound
            && self.time - self.attacked > HIT_SOUND_DELAY
        {
            self.sounded = true;
            self.has_to_sound = false;
            let group = &self.hit_clips[self.hit_type];
            if !group.is_empty() {
                let (clip, length) = group[self.random.range_int(0, group.len() as i32) as usize];
                self.next_sound = self.time + length;
                self.sounds.push(MeleeSound {
                    clip,
                    gain: self.hit_volumes[self.hit_type],
                    position: absolute(origin, self.hit_point),
                });
            }
        }
        let held = input.held;
        let (attacking, speed, swing_sounded, started_attack) = (
            &mut self.attacking,
            &mut self.speed,
            &mut self.swing_sounded,
            &mut self.started_attack,
        );
        let m = &mut self.motions[self.weapons[w].motion];
        // animationEventScript: OnStateEnter is AttackStart, OnStateExit is AttackEnd.
        m.controller.advance(&m.motion, dt, |event| match event {
            StateEvent::Enter(_) => {
                *attacking += 1;
                if held {
                    *speed = 0.;
                }
                *swing_sounded = false;
            }
            StateEvent::Exit(_) => {
                *attacking -= 1;
                *started_attack = false;
            }
        });
    }
    /// The weapon left the hand or changed: the animator is off and RH returns to `defRH`.
    fn leave(&mut self) {
        let Some(w) = self.current.take() else {
            return;
        };
        let m = &mut self.motions[self.weapons[w].motion];
        m.controller.reset(&m.motion);
        self.attack_type = 0;
        self.speed = 1.;
        self.swing_sounded = false;
        self.attacking_now = false;
        self.started_attack = false;
        self.attacking = 0;
        self.last_tip = None;
    }
    #[allow(clippy::too_many_arguments)]
    fn hit(
        &mut self,
        weapon: usize,
        tip: Vector,
        trigger: Pose,
        dt: f32,
        items: &mut ItemStore,
        physics: &mut PhysicsWorld,
        buried: &mut BuriedItems,
        breakables: &mut Breakables,
        records: &[BreakableReference],
        player: Option<RigidBodyHandle>,
        origin: [f64; 3],
    ) {
        let record = &self.weapons[weapon].record;
        let mask = record.contact_mask;
        let mut filter = QueryFilter::default();
        if let Some(player) = player {
            filter = filter.exclude_rigid_body(player);
        }
        let predicate = |_, c: &rapier3d::prelude::Collider| {
            let layers = c.collision_groups().memberships.bits();
            c.is_enabled() && layers & mask != 0 && layers != 1 << PLAYER_LAYER
        };
        filter.predicate = Some(&predicate);
        self.overlaps.clear();
        let shape = Cuboid::new(self.weapons[weapon].half_extents);
        self.overlaps.extend(
            physics
                .queries(filter)
                .intersect_shape(trigger, &shape)
                .map(|(h, _)| h)
                .take(OVERLAP_CAPACITY),
        );
        let delta = tip - self.last_tip.unwrap_or(tip);
        let force = (delta.normalize_or_zero() * record.contact.force + delta) * 0.5 * dt;
        let (contact, shovel_speed) = (&record.contact, record.shovel_speed);
        for i in 0..self.overlaps.len() {
            let rapier = physics.rapier();
            let collider = &rapier.colliders[self.overlaps[i]];
            let aabb = collider.compute_aabb();
            let point = tip.clamp(aabb.mins, aabb.maxs);
            let layer = collider
                .collision_groups()
                .memberships
                .bits()
                .trailing_zeros();
            let tag = collider_tag(collider.user_data);
            let parent = collider.parent();
            let owned = (collider.user_data & 0xffff_ffff) as u32;
            let body_item =
                parent.and_then(|b| items.iter().find(|i| i.body == Some(b)).map(|i| i.id));
            let mut item = owned.checked_sub(1).map(ItemId).or(body_item);
            if let Some(body) = parent
                && item.is_none_or(|id| !buried.recently_unburied(id))
            {
                if body_item.is_some() && body_item != item {
                    item = body_item;
                } else if rapier.bodies[body].is_dynamic() {
                    // Update AddForce acts over the next fixed step.
                    physics.edit_body(body, |b| b.apply_impulse(force * FIXED_DT, true));
                }
            }
            let strength = force.length() * contact.all_particles;
            let low = strength;
            let high = strength * self.random.range(0.9, 1.1);
            let sparks = self.random.range(low, high) as i32;
            let (hit_type, factor) = if layer == SAND_LAYER {
                (SAND, contact.sand)
            } else if layer == GLASS_LAYER {
                (GLASS, 0.)
            } else {
                match tag {
                    Some(ColliderTag::Flesh) => (BLOOD, contact.blood),
                    Some(ColliderTag::Plant) => (PLANT, contact.plant),
                    Some(ColliderTag::Wood) => (WOOD, contact.wood),
                    _ => (METAL, contact.spark),
                }
            };
            self.hit_type = hit_type;
            if let Some(system) = HIT_PARTICLE[hit_type] {
                let direction = tip - point;
                let rotation = if direction == Vector::ZERO {
                    self.particles[system].rotation
                } else {
                    look_rotation(direction)
                };
                self.particles[system].emit(
                    (sparks as f32 * factor).max(0.) as usize,
                    absolute(origin, point),
                    rotation,
                );
            }
            if let Some(id) = item
                && shovel_speed != 0.
                && (items.get(id).is_some_and(|i| i.buried) || buried.recently_unburied(id))
            {
                buried.lift(id, dt, shovel_speed, items, physics, player);
            } else {
                if let Some(target) = breakables.target_of(physics, self.overlaps[i]) {
                    breakables.try_break_melee(records, target, contact.damage, dt);
                }
                // tankscript.TryExplode(damageTank, add: false) and explosivescript.TryExplode.
                if let Some(id) = item {
                    self.strikes.push((id, contact.damage_tank));
                }
                // ediblescript.Shot(onlySeparate: true) on the item that owns the collider.
                if contact.hurt_food
                    && let Some(id) = item
                    && items.get(id).is_some_and(|s| s.food.is_some())
                {
                    self.cuts.push(id);
                }
            }
            if !self.sounded {
                self.has_to_sound = true;
                self.hit_point = point;
            }
        }
    }
}

/// `ediblescript.Shot(onlySeparate: true)`: with two or more `remaining` bites, the bites
/// that move to a new piece. The cut item keeps the rest.
pub fn cut(remaining: u8) -> Option<u8> {
    (remaining >= 2).then_some(remaining / 2)
}

/// Poses, half extents and clips of one weapon record.
fn weapon(
    r: &ReferenceManifest,
    record: &MeleeWeaponReference,
    clip: &impl Fn(&SourceId) -> Result<(u32, f32), ReferenceError>,
) -> Result<Weapon, ReferenceError> {
    let motion = r
        .melee
        .motions
        .iter()
        .position(|m| m.controller == record.controller)
        .ok_or_else(|| ReferenceError("Missing melee motion".into()))?;
    // The item glTF and the world glTF are separate assets of one prefab source.
    let prototype = r
        .world
        .prototypes
        .iter()
        .find(|p| p.source == record.prototype.0)
        .ok_or_else(|| ReferenceError("Missing melee world prototype".into()))?;
    let node = |source: &SourceId| {
        prototype
            .nodes
            .iter()
            .position(|n| n.source == *source)
            .map(|i| prototype.root_relative(i))
            .ok_or_else(|| ReferenceError("Missing melee node".into()))
    };
    let (tip, _) = node(&record.target)?;
    let (trigger, scale) = node(&record.trigger)?;
    let swing_clips = record
        .attack_clips
        .iter()
        .map(|s| clip(s).map(|(id, _)| id))
        .collect::<Result<_, _>>()?;
    let poses = |sources: &[SourceId]| {
        sources
            .iter()
            .map(|s| node(s).map(|(pose, _)| pose))
            .collect::<Result<Vec<_>, _>>()
    };
    let hand_poses = [
        poses(&record.left_hand_poses)?,
        poses(&record.right_hand_poses)?,
    ];
    // A weapon without an item pickup cannot be held and has no grip.
    let pickup = r
        .item_prototypes
        .iter()
        .flat_map(|p| &p.pickups)
        .find(|p| p.pickup.source == record.pickup)
        .map(|p| &p.pickup);
    let grip = |side: usize, source: Option<&SourceId>| match source {
        Some(s) if !hand_poses[side].is_empty() => node(s).map(|(pose, _)| Some(pose)),
        _ => Ok(None),
    };
    let grips = [
        grip(0, pickup.and_then(|p| p.left_hand_grip.as_ref()))?,
        grip(1, pickup.and_then(|p| p.right_hand_grip.as_ref()))?,
    ];
    Ok(Weapon {
        record: record.clone(),
        motion,
        tip,
        trigger,
        half_extents: scale.abs(),
        swing_clips,
        hand_poses,
        grips,
    })
}

/// Index of the weapon with the pickup rule `pickup`.
fn weapon_index<'a>(
    mut pickups: impl Iterator<Item = &'a SourceId>,
    pickup: &SourceId,
) -> Option<usize> {
    pickups.position(|p| p == pickup)
}

/// The original swaps `runtimeAnimatorController` to the held weapon's controller: the
/// animator of a weapon that leaves the hand or changes restarts.
fn resets(current: Option<usize>, held: Option<usize>) -> bool {
    current.is_some() && current != held
}

/// Source `Quaternion.LookRotation(dir, up)` in converted space: source +Z is -Z.
pub fn look_rotation(direction: Vector) -> Rotation {
    let back = -direction.normalize();
    let right = Vector::Y.cross(back).try_normalize().unwrap_or(Vector::X);
    let up = back.cross(right);
    Rotation::from_mat3(&rapier3d::glamx::Mat3::from_cols(right, up, back))
}

fn absolute(origin: [f64; 3], local: Vector) -> [f64; 3] {
    std::array::from_fn(|i| origin[i] + local[i] as f64)
}

/// Exported audio clips: source, id and source length.
pub(crate) fn clip_table(r: &ReferenceManifest) -> Vec<(SourceId, u32, f32)> {
    r.audio["clips"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|c| {
            Some((
                serde_json::from_value(c["source"].clone()).ok()?,
                c["id"].as_u64()? as u32,
                c["source_duration_seconds"].as_f64()? as f32,
            ))
        })
        .collect()
}
