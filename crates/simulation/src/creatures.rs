//! Source creature AI: `aispawn`, `aiscript`, `movecreaturescript`, `sensoryscript`,
//! `stimulscript` and `nyulscript`. Live rabbits run before the physics step.
pub mod animator;
pub mod brain;
pub mod evil;
pub mod motion;
pub mod rabbit;
pub mod ragdoll;
pub mod rig;
pub mod senses;
pub mod spawner;
pub mod stimuli;

use crate::damage::{BreakHealth, collision_force, try_break, try_break_shoot};
use crate::reference::{
    ColliderId, CreatureDamage, ItemId, NodeId, RabbitReference, ReferenceError, ReferenceManifest,
    StimulusKind, TriggerRole,
};
use crate::save::{AbsolutePose, RabbitSnapshot};
use crate::vehicle::PhysicsWorld;
use crate::weather::StormRandom;
use crate::world::WorldOrigin;
use animator::{Action, ActionPose, ActionTiming};
use brain::BrainInput;
use evil::{Events, EyeKind};
use rabbit::{Rabbit, Size};
use ragdoll::Bone;
use rapier3d::math::{Pose, Rotation, Vector};
use rapier3d::prelude::{ColliderHandle, QueryFilter, Ray, RigidBodyHandle};
use rig::{Rig, RigPose};
use senses::{ScanInput, Sensed, cast};
use spawner::{Player, Spawner};
use stimuli::{PlayerStimuli, SoundStimulus, Stimuli};

/// Seconds between the world collision checks of each rabbit.
const SUPPORT_INTERVAL: f64 = 1.;
/// `breakablescript.OnCollisionEnter` of an AI ignores colliders on these layers.
const IGNORED_HIT_LAYERS: [u32; 2] = [15, 17];
/// `SetBasedOnSize` calls `GetEvil(natural: true)` above this size.
const EVIL_SIZE: f32 = 0.75;

/// What the rabbits read of the world this step.
pub struct StepInput<'a> {
    pub now: f64,
    pub dt: f32,
    /// The `aispawn` transform.
    pub player: Player,
    /// The player head stimulus position and the player body.
    pub head: (Vector, RigidBodyHandle),
    pub origin: &'a WorldOrigin,
    /// True when the world has collision around a local point.
    pub supports: &'a dyn Fn(Vector) -> bool,
}

/// One rabbit for the renderer.
#[derive(Clone, Copy, Debug)]
pub struct RabbitView {
    pub id: u32,
    pub pose: Pose,
    pub scale: f32,
    pub phase: f32,
    pub value: f32,
    pub twitch: f32,
    pub visible: bool,
    pub dead: bool,
    pub eyes: bool,
    pub evil: Option<EyeKind>,
    pub action: Option<ActionPose>,
}

/// One `PlayClipAtPoint` of `nyulscript`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Sound {
    pub rabbit: u32,
    pub clip: u32,
    pub position: Vector,
    pub pitch: f32,
}

pub struct Creatures {
    rig: Rig,
    /// Per live collider: it sits under a ragdoll bone.
    bones: Vec<bool>,
    /// Per live collider: the `breakchilds` index of `damage.children`.
    children: Vec<Option<usize>>,
    /// Rig node of each ragdoll body.
    bone_nodes: Vec<usize>,
    /// Rabbit id and the die sound flag of each `Die` this step.
    deaths: Vec<(u32, bool)>,
    sounds: Vec<Sound>,
    /// Attack trigger colliders to add to (true) or remove from (false) the trigger roles.
    triggers: Vec<(ColliderHandle, bool)>,
    /// Colliders that an attack trigger touches this step.
    touched: Vec<ColliderHandle>,
    /// `Roar` and `Attack` state timing.
    timings: [ActionTiming; 2],
    eye: usize,
    sources: PlayerStimuli,
    stimuli: Stimuli,
    /// Engine, horn and radio sound stimuli of this step.
    bound: Vec<SoundStimulus>,
    spawner: Spawner,
    rabbits: Vec<Rabbit>,
    next_id: u32,
    pose: RigPose,
    random: StormRandom,
    /// `peacefulEnemies == 2`: rabbits do not get evil.
    pub peaceful: bool,
}

impl Creatures {
    pub fn new(reference: &ReferenceManifest, seed: u64, now: f64) -> Result<Self, ReferenceError> {
        let c = &reference.creatures;
        let r = &c.rabbit;
        let rig = Rig::new(&reference.nodes, r.root, &r.animator)?;
        // `GetComponentInParent`: the node or its nearest ancestor that matches.
        let ancestor = |mut node: Option<NodeId>, matches: &dyn Fn(NodeId) -> bool| {
            while let Some(id) = node {
                if matches(id) {
                    return Some(id);
                }
                node = reference
                    .nodes
                    .iter()
                    .find(|n| n.id == id)
                    .and_then(|n| n.parent);
            }
            None
        };
        let body_nodes: Vec<_> = r.ragdoll.bodies.iter().map(|b| b.node).collect();
        let colliders = &r.body.colliders;
        let bones = colliders
            .iter()
            .map(|c| ancestor(Some(c.node), &|id| body_nodes.contains(&id)).is_some())
            .collect();
        let children = colliders
            .iter()
            .map(|c| {
                let node = ancestor(Some(c.node), &|id| {
                    r.damage.children.iter().any(|child| child.node == id)
                })?;
                r.damage
                    .children
                    .iter()
                    .position(|child| child.node == node)
            })
            .collect();
        let bone_nodes = body_nodes
            .iter()
            .map(|node| {
                rig.index(*node)
                    .ok_or_else(|| ReferenceError("Rabbit bone outside the rig".into()))
            })
            .collect::<Result<_, _>>()?;
        let eye = rig
            .index(r.senses.eyes)
            .ok_or_else(|| ReferenceError("Rabbit eye outside the rig".into()))?;
        let sources = PlayerStimuli::new(c)
            .ok_or_else(|| ReferenceError("Missing player stimulus sources".into()))?;
        let durations = rig.action_durations();
        let timings = [Action::Roar, Action::Attack].map(|a| {
            let state = a.state(&r.animator.actions);
            ActionTiming {
                rate: state.speed / durations[a.index()],
                enter: state.enter,
                leave: state.leave,
            }
        });
        let max = r.spawner.max_count as usize;
        Ok(Self {
            pose: rig.pose(),
            rig,
            bones,
            children,
            bone_nodes,
            deaths: Vec::with_capacity(max),
            sounds: Vec::with_capacity(max),
            triggers: Vec::with_capacity(max),
            touched: Vec::new(),
            timings,
            eye,
            sources,
            stimuli: Stimuli::new(),
            bound: Vec::with_capacity(32),
            spawner: Spawner::new(now),
            rabbits: Vec::with_capacity(max),
            next_id: 1,
            random: StormRandom(seed ^ 0x7261_6262_6974_0001),
            peaceful: false,
        })
    }

    pub fn rabbits(&self) -> impl Iterator<Item = &Rabbit> {
        self.rabbits.iter()
    }

    /// The next `Die` of this step: the rabbit id and the die sound flag.
    pub fn take_death(&mut self) -> Option<(u32, bool)> {
        self.deaths.pop()
    }

    /// The next evil rabbit clip of this step.
    pub fn take_sound(&mut self) -> Option<Sound> {
        self.sounds.pop()
    }

    /// The next attack trigger collider and its `playerdeathtriggerscript` role, or none when
    /// the role ends.
    pub fn take_trigger(
        &mut self,
        r: &RabbitReference,
    ) -> Option<(ColliderHandle, Option<TriggerRole>)> {
        let t = &r.evil.trigger;
        self.triggers.pop().map(|(handle, on)| {
            let role = TriggerRole::Hazard {
                damage_player: t.damage_player,
                damage_breakable: t.damage_breakable,
            };
            (handle, on.then_some(role))
        })
    }

    /// `nyulscript.Die`: a random `Sdie` clip.
    pub fn die_clip(&mut self, r: &RabbitReference) -> u32 {
        pick(&r.die_clips, &mut self.random)
    }

    /// `weaponscript.Shot` on a live rabbit collider. A `breakchilds` collider multiplies the
    /// force and can take the senses. False when the collider is not a live rabbit.
    pub fn shoot(
        &mut self,
        r: &RabbitReference,
        physics: &PhysicsWorld,
        collider: ColliderHandle,
        force: f32,
        direction: Vector,
    ) -> bool {
        let Some((rabbit, index)) = self.rabbits.iter_mut().find_map(|rabbit| {
            let index = rabbit.colliders.iter().position(|c| c.handle == collider)?;
            Some((rabbit, index)).filter(|(rabbit, _)| rabbit.alive)
        }) else {
            return false;
        };
        let random = &mut self.random;
        let effect = shot_effect(
            &r.damage,
            self.children[index],
            &mut rabbit.health,
            force,
            &mut |max| random.range(0., max),
        );
        if let Some(value) = effect.threat {
            let root = physics.rapier().bodies[rabbit.body].translation();
            rabbit.threat = Some(Sensed {
                position: root - direction,
                kind: StimulusKind::Threat,
                value,
            });
        }
        if effect.broken {
            kill(rabbit, true, &mut self.deaths);
        }
        match effect.lost {
            Some(Sense::Eyes) => rabbit.senses.has_eyes = false,
            Some(Sense::Ears) => rabbit.senses.has_ears = false,
            None => {}
        }
        true
    }

    /// `aiscript.Die`: `SetRagdol(true)` replaces the live body with the bones. The bones take
    /// the animated pose and the live velocity. Returns the rabbit position.
    pub fn ragdoll(
        &mut self,
        r: &RabbitReference,
        physics: &mut PhysicsWorld,
        id: u32,
        items: &[ItemId],
        collider_id: &mut dyn FnMut() -> Result<ColliderId, ReferenceError>,
    ) -> Result<(Vector, &[Bone]), ReferenceError> {
        let rabbit = self
            .rabbits
            .iter_mut()
            .find(|rabbit| rabbit.id == id && rabbit.bones.is_empty())
            .ok_or_else(|| ReferenceError("Missing live rabbit".into()))?;
        let body = &physics.rapier().bodies[rabbit.body];
        let (root, velocity) = (*body.position(), body.linvel());
        self.rig.sample(
            &r.animator,
            rabbit.phase,
            rabbit.blend_value(),
            rabbit.animator.pose,
            rabbit.twitch,
            rabbit.size.scale,
            &mut self.pose,
        );
        let poses = ragdoll::poses(r, &self.bone_nodes, root, &self.pose);
        rabbit.evil.die();
        if let Some(trigger) = rabbit.trigger.take() {
            self.triggers.push((trigger.handle, false));
        }
        physics.remove_body(rabbit.body);
        rabbit.body = RigidBodyHandle::invalid();
        rabbit.colliders.clear();
        rabbit.alive = false;
        rabbit.bones = ragdoll::spawn(
            r,
            &poses,
            rabbit.size.scale,
            rabbit.size.live_mass(r),
            velocity,
            items,
            collider_id,
            physics,
        )?;
        Ok((root.translation, &rabbit.bones))
    }

    /// A saved dead rabbit: `StartDie` with the saved bone poses.
    #[allow(clippy::too_many_arguments)]
    pub fn restore_dead(
        &mut self,
        r: &RabbitReference,
        physics: &mut PhysicsWorld,
        origin: &WorldOrigin,
        saved: &RabbitSnapshot,
        now: f64,
        items: &[ItemId],
        collider_id: &mut dyn FnMut() -> Result<ColliderId, ReferenceError>,
    ) -> Result<&[Bone], ReferenceError> {
        if saved.alive || saved.bones.len() != r.ragdoll.bodies.len() {
            return Err(ReferenceError("Invalid saved rabbit bones".into()));
        }
        let size = Size::new(r, saved.size);
        let poses: Vec<_> = saved
            .bones
            .iter()
            .map(|p| {
                Pose::from_parts(
                    Vector::from_array(origin.local(p.position)),
                    Rotation::from_array(p.rotation),
                )
            })
            .collect();
        let mut rabbit = Rabbit::new(
            self.next_id,
            r,
            RigidBodyHandle::invalid(),
            Vec::new(),
            None,
            size,
            poses[0].translation,
            now,
            &mut self.random,
        );
        self.next_id += 1;
        rabbit.alive = false;
        rabbit.health.shoot_health = saved.shoot_health;
        rabbit.senses.has_eyes = saved.eyes;
        rabbit.senses.has_ears = saved.ears;
        rabbit.bones = ragdoll::spawn(
            r,
            &poses,
            size.scale,
            size.live_mass(r),
            Vector::ZERO,
            items,
            collider_id,
            physics,
        )?;
        self.rabbits.push(rabbit);
        Ok(&self.rabbits.last().unwrap().bones)
    }

    /// The engine, horn and radio sound stimuli that the next step reads.
    pub fn bound_sounds(&mut self) -> &mut Vec<SoundStimulus> {
        &mut self.bound
    }

    /// `PlayClipAtPoint`: a sound stimulus at `position` for `seconds`.
    pub fn sound(&mut self, position: [f64; 3], volume: f32, now: f64, seconds: f32) {
        self.stimuli.sound(position, volume, now, seconds);
    }

    pub fn rebase(&mut self, shift: Vector) {
        for rabbit in &mut self.rabbits {
            rabbit.brain.rebase(shift);
        }
    }

    /// One fixed step: the spawner, then each live rabbit sets its body velocity and rotation.
    pub fn step(&mut self, r: &RabbitReference, physics: &mut PhysicsWorld, input: &StepInput) {
        let now = input.now;
        let origin = *input.origin;
        self.stimuli
            .rebuild(&self.sources, input.head, &self.bound, now, |p| {
                Vector::from_array(origin.local(p))
            });
        let count = self.rabbits.len();
        let mut spawned = Vec::new();
        self.spawner.step(
            &r.spawner,
            now,
            count,
            &input.player,
            physics,
            &mut self.random,
            |place, random| {
                let yaw = random.range(0., 360.);
                spawned.push((place, Size::new(r, random.unit()), yaw));
            },
        );
        for (place, size, yaw) in spawned {
            let pose = Pose::from_parts(place, Rotation::from_rotation_y(-yaw.to_radians()));
            self.spawn(r, physics, pose, size, size.shoot_health, now)
                .expect("validated rabbit reference");
        }
        let rules = &r.damage.rules;
        for hit in physics.hits() {
            for (k, own) in hit.colliders.iter().enumerate() {
                let Some(rabbit) = self.rabbits.iter_mut().find(|rabbit| {
                    rabbit.alive && rabbit.colliders.iter().any(|c| !c.bone && c.handle == *own)
                }) else {
                    continue;
                };
                let layer = physics
                    .rapier()
                    .colliders
                    .get(hit.colliders[1 - k])
                    .map(|c| c.collision_groups().memberships.bits().trailing_zeros());
                if layer.is_some_and(|l| IGNORED_HIT_LAYERS.contains(&l)) {
                    continue;
                }
                if let Some(force) = collision_force(rules, hit.speed)
                    && try_break(rules, &mut rabbit.health.health, force)
                {
                    kill(rabbit, true, &mut self.deaths);
                }
            }
        }
        // `Start` squares `accelerationDeath`.
        let limit = r.brain.acceleration_death;
        for rabbit in &mut self.rabbits {
            if !rabbit.alive {
                continue;
            }
            if now >= rabbit.next_support_check {
                rabbit.next_support_check = now + SUPPORT_INTERVAL;
                let position = physics.rapier().bodies[rabbit.body].translation();
                let active = (input.supports)(position);
                if active != rabbit.active {
                    rabbit.active = active;
                    physics.edit_body(rabbit.body, |b| b.set_enabled(active));
                }
            }
            if !rabbit.active {
                continue;
            }
            let velocity = physics.rapier().bodies[rabbit.body].linvel();
            if accelerated(limit, velocity, rabbit.last_velocity) {
                kill(rabbit, true, &mut self.deaths);
                continue;
            }
            rabbit.last_velocity = velocity;
            step_rabbit(
                r,
                &Shared {
                    rig: &self.rig,
                    eye: self.eye,
                    timings: &self.timings,
                    player: input.player.position,
                },
                &mut self.pose,
                rabbit,
                self.stimuli.list(),
                physics,
                input.dt,
                now,
                &mut self.random,
                &mut self.sounds,
            );
        }
        self.attack_rabbits(r, physics, input.dt);
    }

    /// `playerdeathtriggerscript.OnTriggerStay` on other rabbits: `TryBreak(damageBreakable * dt)`
    /// for each of their colliders in the trigger.
    fn attack_rabbits(&mut self, r: &RabbitReference, physics: &PhysicsWorld, dt: f32) {
        let rules = &r.damage.rules;
        let damage = r.evil.trigger.damage_breakable * dt;
        for i in 0..self.rabbits.len() {
            let attacker = &self.rabbits[i];
            let Some(trigger) = attacker
                .trigger
                .as_ref()
                .filter(|_| attacker.alive && attacker.active && attacker.evil.trigger)
            else {
                continue;
            };
            let collider = &physics.rapier().colliders[trigger.handle];
            let filter = QueryFilter::default()
                .exclude_sensors()
                .exclude_rigid_body(attacker.body)
                .groups(collider.collision_groups());
            self.touched.clear();
            self.touched.extend(
                physics
                    .queries(filter)
                    .intersect_shape(*collider.position(), collider.shape())
                    .map(|(handle, _)| handle),
            );
            for handle in &self.touched {
                let Some(rabbit) = self.rabbits.iter_mut().find(|rabbit| {
                    rabbit.alive && rabbit.colliders.iter().any(|c| c.handle == *handle)
                }) else {
                    continue;
                };
                if try_break(rules, &mut rabbit.health.health, damage) {
                    kill(rabbit, true, &mut self.deaths);
                }
            }
        }
    }

    fn spawn(
        &mut self,
        r: &RabbitReference,
        physics: &mut PhysicsWorld,
        pose: Pose,
        size: Size,
        shoot_health: f32,
        now: f64,
    ) -> Result<&mut Rabbit, ReferenceError> {
        let (body, colliders, trigger) =
            rabbit::insert_body(r, &self.rig, &self.bones, &size, pose, physics)?;
        self.triggers.push((trigger.handle, true));
        let mut rabbit = Rabbit::new(
            self.next_id,
            r,
            body,
            colliders,
            Some(trigger),
            size,
            pose.translation,
            now,
            &mut self.random,
        );
        rabbit.health.shoot_health = shoot_health;
        if size.size > EVIL_SIZE && !self.peaceful {
            rabbit
                .evil
                .get_evil(&r.evil, &mut rabbit.brain, true, now, &mut self.random);
        }
        self.next_id += 1;
        self.rabbits.push(rabbit);
        Ok(self.rabbits.last_mut().unwrap())
    }

    /// Rabbits for the renderer.
    pub fn views<'a>(&'a self, physics: &'a PhysicsWorld) -> impl Iterator<Item = RabbitView> + 'a {
        self.rabbits.iter().map(|rabbit| RabbitView {
            id: rabbit.id,
            pose: root_pose(physics, rabbit),
            scale: rabbit.size.scale,
            phase: rabbit.phase,
            value: rabbit.blend_value(),
            twitch: rabbit.twitch,
            visible: rabbit.active || !rabbit.alive,
            dead: !rabbit.bones.is_empty(),
            eyes: rabbit.senses.has_eyes,
            evil: rabbit.evil.kind,
            action: rabbit.animator.pose,
        })
    }

    /// Rig node and pose of each bone of a dead rabbit, relative to its view pose.
    pub fn each_bone(&self, physics: &PhysicsWorld, id: u32, mut f: impl FnMut(usize, Pose)) {
        let Some(rabbit) = self.rabbits.iter().find(|rabbit| rabbit.id == id) else {
            return;
        };
        let root = root_pose(physics, rabbit).inverse();
        for (bone, node) in rabbit.bones.iter().zip(&self.bone_nodes) {
            f(*node, root * *physics.rapier().bodies[bone.body].position());
        }
    }

    /// Source `save_nyulscript` and the prefab pose of each rabbit.
    pub fn snapshot(&self, physics: &PhysicsWorld, origin: &WorldOrigin) -> Vec<RabbitSnapshot> {
        self.rabbits
            .iter()
            .map(|rabbit| {
                let absolute = |pose: &Pose| AbsolutePose {
                    position: origin.absolute(pose.translation.to_array()),
                    rotation: pose.rotation.to_array(),
                };
                let pose = root_pose(physics, rabbit);
                RabbitSnapshot {
                    pose: absolute(&pose),
                    size: rabbit.size.size,
                    shoot_health: rabbit.health.shoot_health,
                    alive: rabbit.alive,
                    eyes: rabbit.senses.has_eyes,
                    ears: rabbit.senses.has_ears,
                    bones: rabbit
                        .bones
                        .iter()
                        .map(|b| absolute(physics.rapier().bodies[b.body].position()))
                        .collect(),
                }
            })
            .collect()
    }

    /// Spawns the saved live rabbits. `SetBasedOnSize` runs with the saved size, then
    /// `SetHealth`. `restore_dead` spawns the dead ones.
    pub fn restore(
        &mut self,
        r: &RabbitReference,
        physics: &mut PhysicsWorld,
        origin: &WorldOrigin,
        saved: &[RabbitSnapshot],
        now: f64,
    ) -> Result<(), ReferenceError> {
        for s in saved.iter().filter(|s| s.alive) {
            let pose = Pose::from_parts(
                Vector::from_array(origin.local(s.pose.position)),
                Rotation::from_array(s.pose.rotation),
            );
            let rabbit = self.spawn(r, physics, pose, Size::new(r, s.size), s.shoot_health, now)?;
            rabbit.senses.has_eyes = s.eyes;
            rabbit.senses.has_ears = s.ears;
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Sense {
    Eyes,
    Ears,
}

/// What one shot does to a rabbit.
#[derive(Debug, PartialEq)]
struct ShotEffect {
    /// `overRideStimul` threat value.
    threat: Option<f32>,
    broken: bool,
    lost: Option<Sense>,
}

/// `breakchilds.TryBreakShoot` when `child` is set, else `breakablescript.TryBreakShoot`.
/// `roll(max)` is `Random.Range(0, max)`.
fn shot_effect(
    damage: &CreatureDamage,
    child: Option<usize>,
    health: &mut BreakHealth,
    force: f32,
    roll: &mut dyn FnMut(f32) -> f32,
) -> ShotEffect {
    let rules = &damage.rules;
    let scaled = force * child.map_or(1., |i| rules.children[i].modifier);
    let threat = (!rules.no_shoot).then_some(scaled);
    let broken = try_break_shoot(rules, health, scaled);
    let lost = child.map(|i| &damage.children[i]).and_then(|c| {
        if !c.senses || force < c.lose_sense_force {
            return None;
        }
        let value = roll(c.lose_eye_chance + c.lose_ear_chance);
        if value < c.lose_eye_chance {
            Some(Sense::Eyes)
        } else if value < c.lose_eye_chance + c.lose_ear_chance {
            Some(Sense::Ears)
        } else {
            None
        }
    });
    ShotEffect {
        threat,
        broken,
        lost,
    }
}

/// `aiscript.Update`: a velocity change above `limit` in one step kills the rabbit.
fn accelerated(limit: f32, velocity: Vector, last: Vector) -> bool {
    limit > 0. && (velocity - last).length_squared() > limit * limit
}

/// `aiscript.Die` once per rabbit.
fn kill(rabbit: &mut Rabbit, sound: bool, deaths: &mut Vec<(u32, bool)>) {
    if rabbit.alive {
        rabbit.alive = false;
        rabbit.evil.die();
        deaths.push((rabbit.id, sound));
    }
}

/// The live body pose, or the first bone pose of a dead rabbit.
fn root_pose(physics: &PhysicsWorld, rabbit: &Rabbit) -> Pose {
    let body = rabbit.bones.first().map_or(rabbit.body, |b| b.body);
    *physics.rapier().bodies[body].position()
}

/// What every rabbit step reads.
struct Shared<'a> {
    rig: &'a Rig,
    eye: usize,
    timings: &'a [ActionTiming; 2],
    /// `mainscript.M.player` position.
    player: Vector,
}

/// A random clip of `clips`.
fn pick(clips: &[u32], random: &mut StormRandom) -> u32 {
    let index = (random.unit() * clips.len() as f32) as usize;
    clips[index.min(clips.len() - 1)]
}

/// `aiscript.Update`, `movecreaturescript.Update` and `nyulscript.Update` and `SetAnims` of one
/// rabbit.
#[allow(clippy::too_many_arguments)]
fn step_rabbit(
    r: &RabbitReference,
    shared: &Shared,
    pose: &mut RigPose,
    rabbit: &mut Rabbit,
    stimuli: &[stimuli::Stimulus],
    physics: &mut PhysicsWorld,
    dt: f32,
    now: f64,
    random: &mut StormRandom,
    sounds: &mut Vec<Sound>,
) {
    let rig = shared.rig;
    let body = &physics.rapier().bodies[rabbit.body];
    let root = *body.position();
    let velocity = body.linvel();
    rig.sample(
        &r.animator,
        rabbit.phase,
        rabbit.blend_value(),
        rabbit.animator.pose,
        rabbit.twitch,
        rabbit.size.scale,
        pose,
    );
    let evil = &r.evil;
    let mut events = rabbit.evil.update(
        evil,
        &mut rabbit.brain,
        now,
        (shared.player - root.translation).length_squared(),
        random,
    );
    if now >= rabbit.senses.next_scan {
        let e = pose.model[shared.eye];
        let input = ScanInput {
            physics,
            own: rabbit.body,
            root: root.translation,
            eye: root * Pose::from_parts(e.position, e.rotation),
            stimuli,
        };
        rabbit.senses.scan(&r.senses, &input);
        rabbit.senses.next_scan = now + f64::from(r.senses.scan_interval);
    }
    let forward = root.rotation * -Vector::Z;
    let own = rabbit.body;
    let ray = |direction: Vector, length: f32, mask: u32| {
        cast(
            physics,
            own,
            None,
            Ray::new(root.translation, direction),
            length,
            mask,
            true,
        )
        .is_some()
    };
    let jump_ray = || ray(forward, r.brain.jump_sense_distance, r.brain.jump_mask);
    let steer = rabbit.brain.update(
        &r.brain,
        &BrainInput {
            now,
            position: root.translation,
            forward,
            right: root.rotation * Vector::X,
            sensed: &rabbit.senses.sensed,
            threat: rabbit.threat.take(),
            ray_sum: rabbit.senses.ray_sum(),
            speed: velocity.length(),
            has_eyes: rabbit.senses.has_eyes,
            jump_ray: &jump_ray,
        },
        random,
    );
    if let Some((angle, distance)) = steer.see_target {
        let seen = rabbit.evil.see_target(
            evil,
            &mut rabbit.brain,
            angle,
            distance,
            rabbit.size.size,
            now,
        );
        events.roar |= seen.roar;
        events.attack |= seen.attack;
    }
    play(evil, rabbit, root.translation, events, random, sounds);
    let m = &r.motion;
    let mut velocity = velocity;
    if steer.jump && ray(-Vector::Y, m.grounded_ray_distance, m.grounded_mask) {
        velocity.y += m.jump_force;
    }
    let velocity = motion::velocity(
        velocity,
        steer.move_dir.unwrap_or(Vector::ZERO),
        rabbit.size.speed,
        rabbit.evil.speed_modifier(evil),
        m.acceleration,
        dt,
    );
    let rotation = motion::turn(
        root.rotation,
        steer.rot.unwrap_or(0.),
        rabbit.size.rot_speed,
        dt,
    );
    rabbit.run_value +=
        (steer.move_speed - rabbit.run_value) * (dt * r.size.run_lerp).clamp(0., 1.);
    let duration = rig.duration(&r.animator, rabbit.blend_value(), pose);
    let previous = rabbit.phase;
    if duration > 0. {
        rabbit.phase = (rabbit.phase + dt / duration).rem_euclid(1.);
    }
    rabbit
        .animator
        .step(shared.timings, previous, &mut rabbit.phase, dt);
    rabbit.twitch = (rabbit.twitch + dt / rig.overlay_duration()).rem_euclid(1.);
    physics.edit_body(rabbit.body, |b| {
        b.set_linvel(velocity, true);
        b.set_rotation(rotation, true);
    });
    for c in rabbit.colliders.iter().filter(|c| c.bone) {
        let f = pose.model[c.node];
        let local = Pose::from_parts(f.position, f.rotation) * c.offset;
        physics.edit_collider(c.handle, |collider| collider.set_position_wrt_parent(local));
    }
    if let Some(t) = &rabbit.trigger {
        let on = rabbit.evil.trigger;
        if on || physics.rapier().colliders[t.handle].is_enabled() {
            let f = pose.model[t.node];
            let local = Pose::from_parts(f.position, f.rotation) * t.offset;
            physics.edit_collider(t.handle, |collider| {
                collider.set_enabled(on);
                collider.set_position_wrt_parent(local);
            });
        }
    }
}

/// `Roar`, `Attack` and the idle growl: the animator triggers and the clips.
fn play(
    r: &crate::reference::EvilReference,
    rabbit: &mut Rabbit,
    position: Vector,
    events: Events,
    random: &mut StormRandom,
    sounds: &mut Vec<Sound>,
) {
    let mut sound = |clips: &[u32], pitch| {
        sounds.push(Sound {
            rabbit: rabbit.id,
            clip: pick(clips, random),
            position,
            pitch,
        })
    };
    if events.roar {
        sound(&r.roar_clips, r.roar_pitch);
    }
    if events.attack {
        sound(&r.attack_clips, 1.);
    }
    if events.idle {
        sound(&r.idle_clips, 1.);
    }
    rabbit.animator.roar |= events.roar;
    rabbit.animator.attack |= events.attack;
}

#[cfg(test)]
mod tests {
    use super::*;

    fn damage() -> CreatureDamage {
        let source = serde_json::json!({"file": "f", "path_id": 1});
        serde_json::from_value(serde_json::json!({
            "rules": {
                "source": source, "owners": [], "health": 30., "shoot_health": 5.,
                "shoot_different": true, "explosion_health": 0., "explosion_different": false,
                "no_shoot": false, "decrease_after_attack": true, "only_decrease_above": true,
                "decrease_above_health": 8., "only_velocity": true, "mass_modifier": 1.,
                "child_modifier": 1.,
                "children": [
                    {"transform": source, "own_modifier": true, "modifier": 2.5},
                    {"transform": source, "own_modifier": true, "modifier": 0.}
                ],
                "no_trigger": true, "no_collider": false, "no_gib": true, "no_destroy": true,
                "gib": null, "reduced_gib": null, "clips": [], "lamp": null
            },
            "children": [
                {"source": source, "node": 1, "lose_eye_chance": 0.8, "lose_ear_chance": 0.4,
                 "lose_sense_force": 4., "senses": true},
                {"source": source, "node": 2, "lose_eye_chance": 0., "lose_ear_chance": 0.,
                 "lose_sense_force": 0., "senses": false}
            ]
        }))
        .unwrap()
    }

    fn health() -> BreakHealth {
        BreakHealth {
            health: 30.,
            shoot_health: 5.,
            explosion_health: 0.,
        }
    }

    #[test]
    fn acceleration_death_needs_a_change_above_the_limit() {
        assert!(!accelerated(25., Vector::new(0., -24.9, 0.), Vector::ZERO));
        assert!(accelerated(25., Vector::new(0., -25.1, 0.), Vector::ZERO));
        assert!(!accelerated(0., Vector::splat(100.), Vector::ZERO));
    }

    #[test]
    fn head_shots_multiply_the_force_and_take_senses() {
        let d = damage();
        let mut h = health();
        // 1.5 * 2.5 is below the shoot health 5. The force is below the sense force 4.
        let e = shot_effect(&d, Some(0), &mut h, 1.5, &mut |_| 0.);
        assert_eq!((e.threat, e.broken, e.lost), (Some(3.75), false, None));
        assert_eq!(h.shoot_health, 1.25);
        let mut h = health();
        let e = shot_effect(&d, Some(0), &mut h, 4., &mut |_| 0.9);
        assert!(e.broken);
        assert_eq!(e.lost, Some(Sense::Ears));
        let e = shot_effect(&d, Some(0), &mut health(), 4., &mut |_| 0.1);
        assert_eq!(e.lost, Some(Sense::Eyes));
        // The body child has modifier 0: the shot does no damage.
        let mut h = health();
        let e = shot_effect(&d, Some(1), &mut h, 100., &mut |_| 0.);
        assert_eq!((e.broken, e.lost, h.shoot_health), (false, None, 5.));
    }
}
