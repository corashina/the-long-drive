//! Source breakablescript damage rules and bound break targets.
//! The rules are pure functions. `Breakables` holds the targets of active content.
use crate::reference::{BreakableReference, ItemId, LightControlId, SourceId, TriggerRole};
use crate::vehicle::{ColliderHandle, PhysicsWorld};
use crate::world::{GeneratedId, TriggerRoles, UnityRandom};
use rapier3d::prelude::{QueryFilter, RigidBodyHandle};
use std::collections::VecDeque;

pub mod gibs;

/// Source breakablescript.velocityToMass.
const VELOCITY_TO_MASS: f32 = 0.25;

/// Current breakablescript health values. They start at the record values.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BreakHealth {
    pub health: f32,
    pub shoot_health: f32,
    pub explosion_health: f32,
}
impl BreakHealth {
    pub fn new(rules: &BreakableReference) -> Self {
        Self {
            health: rules.health,
            shoot_health: rules.shoot_health,
            explosion_health: rules.explosion_health,
        }
    }
}

/// Source breakablescript.OnCollisionEnter: the force passed to TryBreak.
pub fn collision_force(rules: &BreakableReference, relative_speed: f32) -> Option<f32> {
    if rules.no_collider
        || rules.only_decrease_above && relative_speed < rules.decrease_above_health
    {
        return None;
    }
    Some(relative_speed)
}
/// Source breakablescript.TryBreak(Rigidbody): the force of a body.
pub fn body_force(rules: &BreakableReference, speed: f32, mass: f32) -> f32 {
    if rules.only_velocity {
        speed * VELOCITY_TO_MASS
    } else {
        speed * VELOCITY_TO_MASS * mass * rules.mass_modifier
    }
}
/// Source breakablescript.TryBreak(float). True breaks.
pub fn try_break(rules: &BreakableReference, health: &mut f32, force: f32) -> bool {
    if force >= *health {
        return true;
    }
    if rules.decrease_after_attack {
        *health -= force;
    }
    false
}
/// Source breakablescript.TryBreakShoot. True breaks.
pub fn try_break_shoot(rules: &BreakableReference, state: &mut BreakHealth, force: f32) -> bool {
    if rules.no_shoot {
        return false;
    }
    if rules.shoot_different {
        let broken = force >= state.shoot_health;
        state.shoot_health -= force;
        return broken;
    }
    try_break(rules, &mut state.health, force)
}
/// Source breakablescript.TryBreakExplosion. True breaks.
pub fn try_break_explosion(
    rules: &BreakableReference,
    state: &mut BreakHealth,
    force: f32,
) -> bool {
    if rules.explosion_different {
        let broken = force >= state.explosion_health;
        state.explosion_health -= force;
        return broken;
    }
    try_break(rules, &mut state.health, force)
}

/// The object that a breakablescript breaks.
#[derive(Clone, Debug, PartialEq)]
pub enum BreakKind {
    /// A world content item. Its collider user data holds the item.
    Item(ItemId),
    /// A lamp trigger of a structure. The record lamp lists the lamps of the control.
    Lamp {
        parent: GeneratedId,
        control: LightControlId,
    },
    /// A static prop structure, such as Cacti. Its trigger colliders are bound.
    Prop { parent: GeneratedId },
}

/// One bound breakablescript. `record` is an index in `ReferenceManifest::breakables`.
#[derive(Clone, Debug)]
pub struct BreakTarget {
    pub kind: BreakKind,
    pub record: usize,
    pub health: BreakHealth,
    /// Source breakablescript.destroyed: a target breaks once.
    pub destroyed: bool,
    /// Source Break destroyed the lamp trigger: its colliders stay disabled.
    pub removed: bool,
}

/// Break targets of active content and the breaks of this step.
pub struct Breakables {
    targets: Vec<BreakTarget>,
    /// Lamp and prop colliders and their target.
    colliders: Vec<(ColliderHandle, usize)>,
    queue: VecDeque<usize>,
    scan: Option<[u64; 4]>,
    /// UnityEngine.Random draws of breakablescript.Break.
    random: UnityRandom,
}

impl Default for Breakables {
    fn default() -> Self {
        Self {
            targets: Vec::new(),
            colliders: Vec::new(),
            queue: VecDeque::new(),
            scan: None,
            random: UnityRandom::new(0x6272_6b6c),
        }
    }
}

impl Breakables {
    pub fn targets(&self) -> &[BreakTarget] {
        &self.targets
    }
    /// The target of a collider: an item collider or a lamp trigger collider.
    pub fn target_of(&self, physics: &PhysicsWorld, collider: ColliderHandle) -> Option<usize> {
        if let Some((_, target)) = self.colliders.iter().find(|(c, _)| *c == collider) {
            return Some(*target);
        }
        // Collider user data low 32 bits hold the item id plus one. Zero is no item.
        let data = physics.rapier().colliders.get(collider)?.user_data;
        let item = ((data & u32::MAX as u128) as u32)
            .checked_sub(1)
            .map(ItemId)?;
        self.targets
            .iter()
            .position(|t| t.kind == BreakKind::Item(item))
    }
    /// Source breakablescript.TryBreak(float).
    pub fn try_break(&mut self, records: &[BreakableReference], target: usize, force: f32) {
        let t = &mut self.targets[target];
        let broken = try_break(&records[t.record], &mut t.health.health, force);
        self.queue_break(target, broken);
    }
    /// Source meleeweaponscript hit: `TryBreak(decreaseAfterAttack ? damage * dt : damage)`.
    pub fn try_break_melee(
        &mut self,
        records: &[BreakableReference],
        target: usize,
        damage: f32,
        dt: f32,
    ) {
        let rules = &records[self.targets[target].record];
        let force = if rules.decrease_after_attack {
            damage * dt
        } else {
            damage
        };
        self.try_break(records, target, force);
    }
    /// Source breakablescript.TryBreakShoot.
    pub fn try_break_shoot(&mut self, records: &[BreakableReference], target: usize, force: f32) {
        let t = &mut self.targets[target];
        let broken = try_break_shoot(&records[t.record], &mut t.health, force);
        self.queue_break(target, broken);
    }
    /// Source breakablescript.TryBreakExplosion.
    pub fn try_break_explosion(
        &mut self,
        records: &[BreakableReference],
        target: usize,
        force: f32,
    ) {
        let t = &mut self.targets[target];
        let broken = try_break_explosion(&records[t.record], &mut t.health, force);
        self.queue_break(target, broken);
    }
    /// Source breakablescript.Break runs once per target.
    fn queue_break(&mut self, target: usize, broken: bool) {
        let t = &mut self.targets[target];
        if broken && !t.destroyed {
            t.destroyed = true;
            self.queue.push_back(target);
        }
    }
    /// Run after the physics step. Source breakablescript.OnCollisionEnter for started contacts,
    /// OnTriggerEnter of a body in a bound trigger collider, and
    /// playerdeathtriggerscript.OnTriggerStay for item bodies in hazard triggers.
    pub fn step_hits(
        &mut self,
        records: &[BreakableReference],
        physics: &PhysicsWorld,
        roles: &TriggerRoles,
        body_of: impl Fn(ItemId) -> Option<RigidBodyHandle>,
        dt: f32,
    ) {
        if self.targets.is_empty() {
            return;
        }
        for hit in physics.hits() {
            for handle in hit.colliders {
                let Some(target) = self.target_of(physics, handle) else {
                    continue;
                };
                if let Some(force) =
                    collision_force(&records[self.targets[target].record], hit.speed)
                {
                    self.try_break(records, target, force);
                }
            }
        }
        // OnTriggerEnter: `TryBreak(c.attachedRigidbody)` unless noTrigger.
        for pair in physics.trigger_enters() {
            for (sensor, other) in [(pair[0], pair[1]), (pair[1], pair[0])] {
                let Some(target) = self
                    .colliders
                    .iter()
                    .find(|(c, _)| *c == sensor)
                    .map(|(_, t)| *t)
                else {
                    continue;
                };
                let rules = &records[self.targets[target].record];
                let rapier = physics.rapier();
                let Some(body) = rapier
                    .colliders
                    .get(other)
                    .and_then(|c| c.parent())
                    .and_then(|b| rapier.bodies.get(b))
                else {
                    continue;
                };
                if !rules.no_trigger {
                    let force = body_force(rules, body.linvel().length(), body.mass());
                    self.try_break(records, target, force);
                }
            }
        }
        // Unity skips sleeping bodies in OnTriggerStay.
        if roles.is_empty() {
            return;
        }
        let rapier = physics.rapier();
        let filter = QueryFilter::default().exclude_solids();
        for target in 0..self.targets.len() {
            let t = &self.targets[target];
            let BreakKind::Item(item) = t.kind else {
                continue;
            };
            if t.destroyed {
                continue;
            }
            let Some(body) = body_of(item).and_then(|h| rapier.bodies.get(h)) else {
                continue;
            };
            if !body.is_enabled() || body.is_sleeping() {
                continue;
            }
            for &collider in body.colliders() {
                let collider = &rapier.colliders[collider];
                if !collider.is_enabled() || collider.is_sensor() {
                    continue;
                }
                for (trigger, other) in physics
                    .queries(filter)
                    .intersect_shape(*collider.position(), collider.shape())
                {
                    if let Some(TriggerRole::Hazard {
                        damage_breakable, ..
                    }) = roles.get(trigger, other)
                    {
                        self.try_break(records, target, damage_breakable * dt);
                    }
                }
            }
        }
    }
    /// Next broken target, in break order.
    pub fn take_break(&mut self) -> Option<usize> {
        self.queue.pop_front()
    }
    /// Source Break: `clips[Random.Range(0, clips.Length)]`. No draw without clips.
    pub fn pick_clip<'a>(&mut self, record: &'a BreakableReference) -> Option<&'a SourceId> {
        if record.clips.is_empty() {
            return None;
        }
        let index = self.random.range_int(0, record.clips.len() as i32);
        record.clips.get(index as usize)
    }
    /// UnityEngine.Random of breaks and gib starts.
    pub fn random(&mut self) -> &mut UnityRandom {
        &mut self.random
    }
    /// Stores the binding scan. False when it did not change.
    pub fn rescan(&mut self, scan: [u64; 4]) -> bool {
        if self.scan == Some(scan) {
            return false;
        }
        self.scan = Some(scan);
        true
    }
    /// Keeps targets in order and moves queued breaks with them. Clears lamp colliders.
    pub fn retain(&mut self, mut keep: impl FnMut(&BreakTarget) -> bool) {
        let mut next = 0;
        for index in 0..self.targets.len() {
            if !keep(&self.targets[index]) {
                self.queue.retain(|q| *q != index);
                continue;
            }
            self.targets.swap(next, index);
            for q in &mut self.queue {
                if *q == index {
                    *q = next;
                }
            }
            next += 1;
        }
        self.targets.truncate(next);
        self.colliders.clear();
    }
    /// Adds a target unless the same kind and record is bound.
    pub fn bind(&mut self, kind: BreakKind, record: usize, rules: &BreakableReference) {
        if !self
            .targets
            .iter()
            .any(|t| t.record == record && t.kind == kind)
        {
            self.targets.push(BreakTarget {
                kind,
                record,
                health: BreakHealth::new(rules),
                destroyed: false,
                removed: false,
            });
        }
    }
    pub fn bind_collider(&mut self, collider: ColliderHandle, target: usize) {
        self.colliders.push((collider, target));
    }
    /// The lamp trigger colliders of a target.
    pub fn colliders_of(&self, target: usize) -> impl Iterator<Item = ColliderHandle> + '_ {
        self.colliders
            .iter()
            .filter(move |(_, t)| *t == target)
            .map(|(c, _)| *c)
    }
    /// Source Break destroys the lamp trigger of a target. A destroyed target does not break again.
    pub fn remove(&mut self, target: usize) {
        self.targets[target].destroyed = true;
        self.targets[target].removed = true;
    }
    /// True for a collider of a destroyed lamp trigger.
    pub fn removed_collider(&self, collider: ColliderHandle) -> bool {
        self.colliders
            .iter()
            .any(|(c, t)| *c == collider && self.targets[*t].removed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rapier3d::math::Vector;
    use rapier3d::prelude::{ActiveEvents, ColliderBuilder, RigidBodyBuilder};

    /// The Cacti breakablescript.
    fn cacti() -> BreakableReference {
        let source = SourceId {
            file: "sharedassets1.assets".into(),
            path_id: 154437,
        };
        BreakableReference {
            source,
            owners: Vec::new(),
            health: 64.,
            shoot_health: 10.,
            shoot_different: true,
            explosion_health: 2.,
            explosion_different: true,
            no_shoot: false,
            decrease_after_attack: true,
            only_decrease_above: false,
            decrease_above_health: 0.,
            only_velocity: false,
            mass_modifier: 1.,
            child_modifier: 1.,
            children: Vec::new(),
            no_trigger: false,
            no_collider: false,
            no_gib: false,
            no_destroy: false,
            gib: None,
            reduced_gib: None,
            clips: Vec::new(),
            lamp: None,
        }
    }

    /// Breaks of a 100 kg body that moves through a fixed trigger at `speed`.
    fn drive_through(speed: f32) -> usize {
        let records = [cacti()];
        let mut physics = PhysicsWorld::empty_for_tests();
        let sensor = physics.insert_collider(
            ColliderBuilder::ball(0.5)
                .sensor(true)
                .active_events(ActiveEvents::COLLISION_EVENTS),
        );
        let body = physics.insert_body(
            RigidBodyBuilder::dynamic()
                .translation(Vector::new(-2., 0., 0.))
                .linvel(Vector::new(speed, 0., 0.)),
        );
        physics.insert_collider_with_parent(ColliderBuilder::ball(0.3).mass(100.), body);
        let mut breakables = Breakables::default();
        let parent = GeneratedId {
            layer: crate::world::WorldLayer::Prop,
            generator: records[0].source.clone(),
            unit: [0, 0],
            ordinal: 0,
            children: Vec::new(),
        };
        breakables.bind(BreakKind::Prop { parent }, 0, &records[0]);
        breakables.bind_collider(sensor, 0);
        let mut breaks = 0;
        for _ in 0..200 {
            physics.step_physics();
            breakables.step_hits(&records, &physics, &TriggerRoles::default(), |_| None, 0.02);
            while breakables.take_break().is_some() {
                breaks += 1;
            }
        }
        breaks
    }

    #[test]
    fn x02_leftovers_cactus_trigger_breaks_once() {
        // TryBreak(RB): 0.25 * 100 kg * 4 m/s = 100 >= 64. The prop is destroyed after one break.
        assert_eq!(drive_through(4.), 1);
        // 0.25 * 100 kg * 2 m/s = 50 < 64 only lowers the health.
        assert_eq!(drive_through(2.), 0);
    }
}
