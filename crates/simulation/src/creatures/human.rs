//! Source `newAiScript` human monster: the live body, `Start`, `Die` and `save_newai`.
//! Buildings spawn it through the `humanmonster` category. The world placement key names it.
use super::ragdoll::{self, Bone};
use crate::damage::BreakHealth;
use crate::reference::{ColliderId, HumanReference, ItemId, NodeId, NodeRecord, ReferenceError};
use crate::save::{AbsolutePose, HumanSnapshot};
use crate::vehicle::{PhysicsWorld, collider_builder, step_surface_bits, tag_bits};
use crate::world::spawn_state::HumanRoll;
use crate::world::{ObjectKey, WorldOrigin};
use rapier3d::math::{Pose, Rotation, Vector};
use rapier3d::prelude::{
    ActiveEvents, Group, InteractionGroups, InteractionTestMode, RigidBodyBuilder, RigidBodyHandle,
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
    next_id: u32,
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
    pub fn new(r: &HumanReference, nodes: &[NodeRecord]) -> Result<Self, ReferenceError> {
        let colliders = r
            .body
            .colliders
            .iter()
            .map(|c| model_pose(nodes, r.root, c.node))
            .collect::<Result<_, _>>()?;
        Ok(Self {
            humans: Vec::new(),
            far: Vec::new(),
            colliders,
            next_id: 1,
        })
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

    /// `newAiScript.Update` before the AI: a sleeping `first` human wakes.
    pub fn step(&mut self) {
        for h in &mut self.humans {
            if h.sleeping && h.first {
                h.sleeping = false;
            }
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
            h.target += shift;
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
