//! Source `newAiScript` human monster: the live body, `Start`, `Die`, `save_newai`, the
//! `newAiAnim` Animator and the voice. Buildings spawn it through the `humanmonster` category.
//! The world placement key names it.
mod damage;

use super::human_ai::{Frame, Hit, HumanAi, Owned, PlayerView, Touch, Usable, World};
use super::human_voice::{Voice, VoiceInput, VoicePlay};
use super::ragdoll::{self, Bone};
use crate::animation::humanoid::{Humanoid, HumanoidInput, HumanoidPose};
use crate::damage::BreakHealth;
use crate::reference::TriggerRole;
use crate::reference::{
    ColliderGeometry, ColliderId, HumanAiReference, HumanReference, ItemId, NodeId, NodeRecord,
    ReferenceError,
};
use crate::save::{AbsolutePose, HumanSnapshot};
use crate::vehicle::{PhysicsWorld, collider_builder, step_surface_bits, tag_bits};
use crate::weather::StormRandom;
use crate::world::TriggerRoles;
use crate::world::spawn_state::HumanRoll;
use crate::world::{ObjectKey, WorldOrigin};
use rapier3d::math::{Pose, Rotation, Vector};
use rapier3d::prelude::{
    ActiveEvents, Collider, ColliderHandle, Group, InteractionGroups, InteractionTestMode,
    QueryFilter, Ray, RigidBodyBuilder, RigidBodyHandle, SharedShape,
};

/// `TargetReset`: `transform.up * 0.75 + transform.forward * 0.05`. Forward is -z.
const TARGET_OFFSET: Vector = Vector::new(0., 0.75, -0.05);

/// One human monster. Positions are local physics positions.
pub struct Human {
    /// Runtime id for the renderer.
    pub id: u32,
    /// The world placement. It is the save identity.
    pub key: ObjectKey,
    /// The live root body. Invalid after `Die`.
    pub body: RigidBodyHandle,
    pub died: bool,
    pub sleeping: bool,
    /// Source `first`: `Update` wakes a sleeping human.
    pub first: bool,
    pub voice_pitch: f32,
    /// Source `patrolCurrentTime`, set by `NewPatrolTime`.
    pub patrol_time: f32,
    /// Source `target` position.
    pub target: Vector,
    /// The `breakablescript` health values.
    pub health: BreakHealth,
    /// Ragdoll bones after `Die`, in body order.
    pub bones: Vec<Bone>,
    pub ai: HumanAi,
    pub animation: HumanoidPose,
    pub voice: Voice,
    /// `SourceVoice` played a new clip this step.
    pub voice_started: bool,
}

/// A live human voice for audio: the clip that plays and the `SourceVoice` position.
#[derive(Clone, Copy, Debug)]
pub struct HumanSound {
    pub id: u32,
    pub play: VoicePlay,
    pub started: bool,
    pub position: Vector,
}

/// One human for the renderer.
#[derive(Clone, Copy, Debug)]
pub struct HumanView {
    pub id: u32,
    pub pose: Pose,
    pub died: bool,
}

pub struct Humans {
    humans: Vec<Human>,
    /// `savedatascript.farStuff`: humans that `itemPlaceRemoveScript` removed.
    far: Vec<HumanSnapshot>,
    /// Live collider poses under the prefab root, in `body.colliders` order.
    colliders: Vec<Pose>,
    /// Per live collider: its `breakchilds` index in `breakable.children`.
    children: Vec<Option<usize>>,
    /// Humans that `breakablescript.Break` kills this step. `Die(true)` plays the death sound.
    deaths: Vec<u32>,
    /// `sens` trigger shapes under the prefab root and their layer groups.
    sens: Vec<(Pose, SharedShape, InteractionGroups)>,
    /// Index of `col` in `body.colliders`, its capsule axis and radius.
    col: (usize, Vector, f32),
    random: StormRandom,
    next_id: u32,
    rig: Humanoid,
    /// The Animator Transform under the prefab root.
    animator: Pose,
}

/// Pose of `node` under the prefab `root`, from the rest transforms. Parent scales move the
/// children; the pose itself has no scale.
fn model_pose(nodes: &[NodeRecord], root: NodeId, node: NodeId) -> Result<Pose, ReferenceError> {
    let find = |id: NodeId| {
        nodes
            .iter()
            .find(|n| n.id == id)
            .ok_or_else(|| ReferenceError("Human node outside the asset".into()))
    };
    let mut chain = Vec::new();
    let mut id = node;
    while id != root {
        let n = find(id)?;
        chain.push(n);
        id = n
            .parent
            .ok_or_else(|| ReferenceError("Human node outside the root".into()))?;
    }
    let mut pose = Pose::IDENTITY;
    let mut scale = Vector::from_array(find(root)?.transform.scale);
    for n in chain.iter().rev() {
        let t = &n.transform;
        pose.translation += pose.rotation * (scale * Vector::from_array(t.translation));
        pose.rotation = pose.rotation * Rotation::from_array(t.rotation);
        scale *= Vector::from_array(t.scale);
    }
    Ok(pose)
}

impl Humans {
    pub fn new(
        r: &HumanReference,
        nodes: &[NodeRecord],
        seed: u64,
    ) -> Result<Self, ReferenceError> {
        let colliders = r
            .body
            .colliders
            .iter()
            .map(|c| model_pose(nodes, r.root, c.node))
            .collect::<Result<_, _>>()?;
        let children = damage::children(r, nodes);
        let ai = &r.ai;
        let sens = ai
            .sens
            .iter()
            .map(|c| {
                let builder = collider_builder(&c.geometry)?;
                Ok((
                    model_pose(nodes, r.root, c.node)? * builder.position,
                    builder.shape,
                    InteractionGroups::new(
                        Group::from_bits_truncate(c.membership),
                        Group::from_bits_truncate(c.filter),
                        InteractionTestMode::And,
                    ),
                ))
            })
            .collect::<Result<_, ReferenceError>>()?;
        let index = r
            .body
            .colliders
            .iter()
            .position(|c| c.node == ai.col)
            .ok_or_else(|| ReferenceError("Human col is not a live collider".into()))?;
        let ColliderGeometry::Capsule { a, b, radius } = r.body.colliders[index].geometry else {
            return Err(ReferenceError("Human col is not a capsule".into()));
        };
        let axis = (Vector::from_array(b) - Vector::from_array(a)).normalize_or(Vector::Y);
        Ok(Self {
            humans: Vec::new(),
            far: Vec::new(),
            colliders,
            children,
            deaths: Vec::new(),
            sens,
            col: (index, axis, radius),
            random: StormRandom(seed ^ 0x6875_6d61_6e00_0001),
            next_id: 1,
            rig: Humanoid::new(nodes, r)?,
            animator: model_pose(nodes, r.root, r.animator.root)?,
        })
    }

    /// `col.height` and `col.center` lerped from `colStand` to `colDuck` by `duck`.
    fn duck_shape(col: (usize, Vector, f32), r: &HumanAiReference, duck: f32) -> SharedShape {
        let (_, axis, radius) = col;
        let height = r.stand.height + (r.duck.height - r.stand.height) * duck;
        let center =
            Vector::from_array(r.stand.center).lerp(Vector::from_array(r.duck.center), duck);
        let half = (height * 0.5 - radius).max(0.);
        SharedShape::capsule(center - axis * half, center + axis * half, radius)
    }

    pub fn iter(&self) -> impl Iterator<Item = &Human> {
        self.humans.iter()
    }

    pub fn iter_mut(&mut self) -> impl Iterator<Item = &mut Human> {
        self.humans.iter_mut()
    }

    /// The placement already spawned a human. Source buildings spawn their contents once.
    pub fn known(&self, key: &ObjectKey) -> bool {
        self.humans.iter().any(|h| &h.key == key) || self.far.iter().any(|s| &s.key == key)
    }

    /// Instantiate and `Start`: the live body at `pose` with the spawn roll.
    pub fn place(
        &mut self,
        r: &HumanReference,
        physics: &mut PhysicsWorld,
        key: ObjectKey,
        pose: Pose,
        roll: HumanRoll,
    ) -> Result<u32, ReferenceError> {
        let body = self.insert_body(r, physics, pose)?;
        let id = self.next_id;
        self.next_id += 1;
        self.humans.push(Human {
            id,
            key,
            body,
            died: false,
            sleeping: roll.sleeping,
            first: r.start.first,
            voice_pitch: roll.voice_pitch,
            patrol_time: roll.patrol_time,
            target: pose * TARGET_OFFSET,
            health: BreakHealth::new(&r.breakable),
            bones: Vec::new(),
            ai: HumanAi::new(),
            animation: self.rig.pose(),
            voice: Voice::new(roll.voice_pitch, &r.voice),
            voice_started: false,
        });
        Ok(id)
    }

    fn insert_body(
        &self,
        r: &HumanReference,
        physics: &mut PhysicsWorld,
        pose: Pose,
    ) -> Result<RigidBodyHandle, ReferenceError> {
        let b = &r.body;
        // Rigidbody constraints freeze the rotation.
        let body = physics.insert_body(
            RigidBodyBuilder::dynamic()
                .pose(pose)
                .ccd_enabled(true)
                .lock_rotations()
                .additional_mass(b.mass_kg)
                .linear_damping(b.linear_damping)
                .angular_damping(b.angular_damping),
        );
        for (c, local) in b.colliders.iter().zip(&self.colliders) {
            let collider = collider_builder(&c.geometry)?;
            let offset = collider.position;
            let collider = collider
                .position(*local * offset)
                .friction(c.friction)
                .restitution(c.restitution)
                .sensor(c.sensor)
                .density(0.)
                .collision_groups(InteractionGroups::new(
                    Group::from_bits_truncate(c.membership),
                    Group::from_bits_truncate(c.filter),
                    InteractionTestMode::And,
                ))
                .user_data(step_surface_bits(c.step_surface) | tag_bits(c.tag))
                // `breakablescript.OnCollisionEnter` reads the contacts of the root body.
                .active_events(ActiveEvents::COLLISION_EVENTS)
                .build();
            physics.insert_collider_with_parent(collider, body);
        }
        physics.refresh_queries();
        Ok(body)
    }

    /// `newAiScript.Update` of each live human, with the `sens` and `ladderscript` trigger stays
    /// of this fixed step. Pushes the `Use` doors to `doors`. Returns the `Attack` damage to the
    /// player.
    pub fn step_ai(
        &mut self,
        r: &HumanReference,
        physics: &mut PhysicsWorld,
        input: &AiInput,
        doors: &mut Vec<Usable>,
    ) -> f32 {
        let ai = &r.ai;
        let mut damage = 0.;
        let deaths = &self.deaths;
        for h in self
            .humans
            .iter_mut()
            .filter(|h| !h.died && !deaths.contains(&h.id))
        {
            let body = &physics.rapier().bodies[h.body];
            let pose = *body.position();
            let velocity = body.linvel();
            let touch = |handle: ColliderHandle, c: &Collider| match input.player {
                Some((player, view)) if c.parent() == Some(player) => Touch::Player(view),
                _ => (input.usable)(handle, physics).map_or(Touch::Other, Touch::Usable),
            };
            let sensors = &mut h.ai.sensors;
            sensors.fixed_update();
            let slots = [
                &mut sensors.up,
                &mut sensors.head,
                &mut sensors.down,
                &mut sensors.ground,
                &mut sensors.can_get_up,
            ];
            for (sens, (local, shape, groups)) in slots.into_iter().zip(&self.sens) {
                let filter = QueryFilter::default()
                    .exclude_rigid_body(h.body)
                    .groups(*groups);
                if let Some((handle, c)) = physics
                    .queries(filter)
                    .intersect_shape(pose * *local, shape.as_ref())
                    .next()
                {
                    sens.stay(touch(handle, c));
                }
            }
            // `ladderscript.OnTriggerStay` on the live colliders.
            let ladder = body.colliders().iter().any(|own| {
                let own = &physics.rapier().colliders[*own];
                physics
                    .queries(
                        QueryFilter::default()
                            .exclude_solids()
                            .exclude_rigid_body(h.body),
                    )
                    .intersect_shape(*own.position(), own.shape())
                    .any(|(handle, c)| {
                        matches!(input.roles.get(handle, c), Some(TriggerRole::Ladder { .. }))
                    })
            });
            if ladder {
                h.ai.on_ladder();
            }
            let frame = Frame {
                now: input.now,
                dt: input.dt,
                pose,
                velocity,
                head: pose * self.animator * self.rig.head(&h.animation),
                player: input.player.map(|(_, view)| view),
                peaceful: input.peaceful,
                debug_target: None,
            };
            let mut world = Rays {
                physics,
                own: h.body,
                player: input.player,
                mask: ai.mask,
                usable: input.usable,
            };
            let duck = h.ai.duck_value;
            let out = h.ai.update(
                ai,
                Owned {
                    sleeping: &mut h.sleeping,
                    first: &mut h.first,
                    target: &mut h.target,
                    patrol_time: &mut h.patrol_time,
                    patrol_range: r.start.patrol_time,
                },
                &frame,
                &mut world,
                &mut self.random,
            );
            physics.edit_body(h.body, |b| {
                b.set_linvel(out.velocity, true);
                b.set_rotation(out.rotation, true);
                b.set_gravity_scale(if out.gravity { 1. } else { 0. }, true);
            });
            if h.ai.duck_value != duck {
                let col = physics.rapier().bodies[h.body].colliders()[self.col.0];
                let local = self.colliders[self.col.0];
                let shape = Self::duck_shape(self.col, ai, h.ai.duck_value);
                physics.edit_collider(col, |c| {
                    c.set_shape(shape);
                    c.set_position_wrt_parent(local);
                });
            }
            doors.extend(out.door);
            damage += out.damage;
        }
        damage
    }

    /// The voice block of `newAiScript.Update`. It runs before the AI and reads the flags of the
    /// previous step.
    pub fn voice_block(&mut self, r: &HumanReference, now: f64, dt: f32) {
        for h in &mut self.humans {
            h.voice_started = false;
            if h.died {
                continue;
            }
            let input = VoiceInput {
                died: false,
                sleeping: h.sleeping,
                chasing: h.ai.is_chasing,
                seeing: h.ai.is_seeing,
                attacking: h.ai.is_attacking,
            };
            h.voice_started = h
                .voice
                .update(&r.voice, input, now, dt, &mut self.random)
                .is_some();
        }
    }

    /// After the AI: `SoundNotice`, `newAiAnim.Update`, the Animator and `LateUpdate`. Dead
    /// humans keep their ragdoll.
    pub fn animate(&mut self, r: &HumanReference, physics: &PhysicsWorld, now: f64, dt: f32) {
        for h in self.humans.iter_mut().filter(|h| !h.died) {
            if h.ai.noticed {
                h.voice_started |= h.voice.notice(&r.voice, now, &mut self.random).is_some();
            }
            let anim = h.ai.anim;
            let root = *physics.rapier().bodies[h.body].position() * self.animator;
            self.rig.update(
                &mut h.animation,
                &HumanoidInput {
                    now,
                    dt,
                    root,
                    target_forward: anim.forward,
                    target_turn: anim.turn,
                    crouch: anim.duck,
                    look: h.target,
                    holds: anim.hold,
                    mouth: h.voice.mouth,
                },
            );
        }
    }

    /// `SoundDeath` of `Die(true)`. A saved dead human does not call it.
    pub fn death_voice(&mut self, r: &HumanReference, id: u32, now: f64) {
        if let Some(h) = self.humans.iter_mut().find(|h| h.id == id) {
            h.voice_started |= h.voice.death(&r.voice, now, &mut self.random).is_some();
        }
    }

    /// The voice of each human while its clip plays.
    pub fn sounds<'a>(
        &'a self,
        r: &'a HumanReference,
        physics: &'a PhysicsWorld,
        now: f64,
    ) -> impl Iterator<Item = HumanSound> + 'a {
        let local = Vector::from_array(r.voice.position);
        self.humans.iter().filter_map(move |h| {
            let play = h.voice.current.filter(|_| h.voice.playing(now))?;
            Some(HumanSound {
                id: h.id,
                play,
                started: h.voice_started,
                position: root_pose(physics, h) * local,
            })
        })
    }

    /// Animated rig node and pose under the view pose of each live human node.
    pub fn each_animated(&self, id: u32, mut f: impl FnMut(NodeId, Pose)) {
        let Some(h) = self.humans.iter().find(|h| h.id == id && !h.died) else {
            return;
        };
        for i in 1..self.rig.len() {
            let frame = self.rig.converted(&h.animation, i);
            f(
                self.rig.node(i),
                self.animator * Pose::from_parts(frame.position, frame.rotation),
            );
        }
    }

    /// `Die`: `SetRagdol(true)` replaces the live body with the bones, at the rest pose and
    /// the live velocity. `poses` gives saved bone poses instead.
    pub fn die(
        &mut self,
        r: &HumanReference,
        physics: &mut PhysicsWorld,
        id: u32,
        poses: Option<Vec<Pose>>,
        items: &[ItemId],
        collider_id: &mut dyn FnMut() -> Result<ColliderId, ReferenceError>,
    ) -> Result<&[Bone], ReferenceError> {
        let human = self
            .humans
            .iter_mut()
            .find(|h| h.id == id && !h.died)
            .ok_or_else(|| ReferenceError("Missing live human".into()))?;
        let body = &physics.rapier().bodies[human.body];
        let (root, velocity) = (*body.position(), body.linvel());
        let poses = poses.unwrap_or_else(|| {
            r.ragdoll
                .bodies
                .iter()
                .map(|b| {
                    root * Pose::from_parts(
                        Vector::from_array(b.pose.translation),
                        Rotation::from_array(b.pose.rotation),
                    )
                })
                .collect()
        });
        physics.remove_body(human.body);
        human.body = RigidBodyHandle::invalid();
        human.died = true;
        human.bones = ragdoll::spawn(
            &r.ragdoll,
            &poses,
            1.,
            None,
            velocity,
            items,
            collider_id,
            physics,
        )?;
        Ok(&human.bones)
    }

    /// Spawns a saved human: Instantiate at the saved pose, then `save_newai.SetTosave`.
    /// The caller calls `die` for a saved dead human.
    pub fn restore(
        &mut self,
        r: &HumanReference,
        physics: &mut PhysicsWorld,
        origin: &WorldOrigin,
        saved: &HumanSnapshot,
    ) -> Result<u32, ReferenceError> {
        let id = self.place(
            r,
            physics,
            saved.key.clone(),
            local(origin, &saved.pose),
            HumanRoll {
                sleeping: saved.sleeping,
                voice_pitch: saved.voice_pitch,
                patrol_time: saved.patrol_time,
            },
        )?;
        let human = self.humans.last_mut().unwrap();
        human.first = saved.first;
        human.health = BreakHealth {
            health: saved.health,
            shoot_health: saved.shoot_health,
            explosion_health: saved.explosion_health,
        };
        Ok(id)
    }

    /// Source `save_newai` and the prefab pose of each human, far ones included.
    pub fn snapshot(&self, physics: &PhysicsWorld, origin: &WorldOrigin) -> Vec<HumanSnapshot> {
        self.humans
            .iter()
            .map(|h| snapshot(physics, origin, h))
            .chain(self.far.iter().cloned())
            .collect()
    }

    /// `itemPlaceRemoveScript.RemoveItems`: each human that `remove` selects by its local root
    /// position goes to the far humans. The caller releases the bone items and bodies in `bones`.
    pub fn unload(
        &mut self,
        physics: &mut PhysicsWorld,
        origin: &WorldOrigin,
        remove: impl Fn(Vector) -> bool,
        bones: &mut Vec<(ItemId, RigidBodyHandle)>,
    ) {
        let mut i = 0;
        while i < self.humans.len() {
            let h = &self.humans[i];
            if !remove(root_pose(physics, h).translation) {
                i += 1;
                continue;
            }
            self.far.push(snapshot(physics, origin, h));
            let h = self.humans.swap_remove(i);
            if !h.died {
                physics.remove_body(h.body);
            }
            bones.extend(h.bones.iter().map(|b| (b.item, b.body)));
        }
    }

    /// `PlaceItems`: the next far human that `near` selects by its absolute position.
    pub fn take_far(&mut self, near: impl Fn([f64; 3]) -> bool) -> Option<HumanSnapshot> {
        let i = self.far.iter().position(|s| near(s.pose.position))?;
        Some(self.far.swap_remove(i))
    }

    /// Saved humans wait as far humans until a place check brings them back.
    pub fn load_far(&mut self, saved: &[HumanSnapshot]) {
        self.far.extend_from_slice(saved);
    }

    pub fn rebase(&mut self, shift: Vector) {
        for h in &mut self.humans {
            h.target -= shift;
            h.ai.rebase(shift);
        }
    }

    /// Humans for the renderer.
    pub fn views<'a>(&'a self, physics: &'a PhysicsWorld) -> impl Iterator<Item = HumanView> + 'a {
        self.humans.iter().map(|h| HumanView {
            id: h.id,
            pose: root_pose(physics, h),
            died: h.died,
        })
    }

    /// Ragdoll body index and pose of each bone of a dead human, relative to its view pose.
    pub fn each_bone(&self, physics: &PhysicsWorld, id: u32, mut f: impl FnMut(usize, Pose)) {
        let Some(h) = self.humans.iter().find(|h| h.id == id) else {
            return;
        };
        let root = root_pose(physics, h).inverse();
        for (i, bone) in h.bones.iter().enumerate() {
            f(i, root * *physics.rapier().bodies[bone.body].position());
        }
    }
}

fn local(origin: &WorldOrigin, pose: &AbsolutePose) -> Pose {
    Pose::from_parts(
        Vector::from_array(origin.local(pose.position)),
        Rotation::from_array(pose.rotation),
    )
}

/// The live body pose, or the first bone pose of a dead human.
fn root_pose(physics: &PhysicsWorld, h: &Human) -> Pose {
    let body = h.bones.first().map_or(h.body, |b| b.body);
    *physics.rapier().bodies[body].position()
}

fn snapshot(physics: &PhysicsWorld, origin: &WorldOrigin, h: &Human) -> HumanSnapshot {
    let absolute = |pose: &Pose| AbsolutePose {
        position: origin.absolute(pose.translation.to_array()),
        rotation: pose.rotation.to_array(),
    };
    let pose = if h.died {
        // The prefab root stays where `Die` left it; the first bone stands in for it.
        root_pose(physics, h)
    } else {
        *physics.rapier().bodies[h.body].position()
    };
    HumanSnapshot {
        key: h.key.clone(),
        pose: absolute(&pose),
        died: h.died,
        sleeping: h.sleeping,
        first: h.first,
        voice_pitch: h.voice_pitch,
        patrol_time: h.patrol_time,
        health: h.health.health,
        shoot_health: h.health.shoot_health,
        explosion_health: h.health.explosion_health,
        bones: h
            .bones
            .iter()
            .map(|b| absolute(physics.rapier().bodies[b.body].position()))
            .collect(),
    }
}

/// What `step_ai` reads of the world.
pub struct AiInput<'a> {
    pub now: f64,
    pub dt: f32,
    /// The player body and its `fpscontroller` view.
    pub player: Option<(RigidBodyHandle, PlayerView)>,
    /// `peacefulEnemies == 2`.
    pub peaceful: bool,
    pub roles: &'a TriggerRoles,
    /// `GetComponentInParent<usablescript>` of a collider.
    pub usable: &'a dyn Fn(ColliderHandle, &PhysicsWorld) -> Option<Usable>,
}

/// The `lm` rays of one human. Rays that start inside a collider do not hit it, as in Unity.
struct Rays<'a> {
    physics: &'a PhysicsWorld,
    own: RigidBodyHandle,
    player: Option<(RigidBodyHandle, PlayerView)>,
    mask: u32,
    usable: &'a dyn Fn(ColliderHandle, &PhysicsWorld) -> Option<Usable>,
}

/// Unity layer 23.
const GLASS_LAYER: u32 = 23;

impl Rays<'_> {
    fn each(&self, origin: Vector, direction: Vector, range: f32, mut f: impl FnMut(Hit)) {
        let Some(direction) = direction.try_normalize() else {
            return;
        };
        let mask = self.mask;
        let predicate =
            |_, c: &Collider| c.is_enabled() && c.collision_groups().memberships.bits() & mask != 0;
        let filter = QueryFilter::default().predicate(&predicate);
        for (handle, c, hit) in
            self.physics
                .queries(filter)
                .intersect_ray(Ray::new(origin, direction), range, true)
        {
            if hit.time_of_impact <= 0. {
                continue;
            }
            let parent = c.parent();
            f(Hit {
                distance: hit.time_of_impact,
                point: origin + direction * hit.time_of_impact,
                normal: hit.normal,
                player: self
                    .player
                    .filter(|(body, _)| parent == Some(*body))
                    .map(|(_, view)| view),
                own: parent == Some(self.own),
                glass: c.collision_groups().memberships.bits() == 1 << GLASS_LAYER,
                usable: (self.usable)(handle, self.physics),
            });
        }
    }
}

impl World for Rays<'_> {
    fn ray_all(&mut self, origin: Vector, direction: Vector, range: f32, hits: &mut Vec<Hit>) {
        self.each(origin, direction, range, |hit| hits.push(hit));
        hits.sort_by(|a, b| a.distance.total_cmp(&b.distance));
    }

    fn ray(&mut self, origin: Vector, direction: Vector, range: f32) -> Option<Hit> {
        let mut best: Option<Hit> = None;
        self.each(origin, direction, range, |hit| {
            if best.is_none_or(|b| hit.distance < b.distance) {
                best = Some(hit);
            }
        });
        best
    }
}
