//! Source `breakablescript` of the human monster: shots, explosions, collisions and triggers
//! break it, and `DamageFrom` wakes it and turns its target to the hit.
use super::{Human, Humans};
use crate::damage::{body_force, collision_force, try_break, try_break_explosion, try_break_shoot};
use crate::reference::{HumanReference, NodeRecord};
use crate::vehicle::PhysicsWorld;
use rapier3d::math::Vector;
use rapier3d::prelude::ColliderHandle;

/// `breakablescript.OnCollisionEnter` of an AI ignores colliders on these layers.
const IGNORED_HIT_LAYERS: [u32; 2] = [15, 17];
/// `DamageFrom`. The target moves this far from the body toward the hit.
const DAMAGE_FROM_DISTANCE: f32 = 5.;

/// Per live collider: `GetComponentInParent<breakchilds>` as an index in `breakable.children`.
pub(super) fn children(r: &HumanReference, nodes: &[NodeRecord]) -> Vec<Option<usize>> {
    let children = &r.breakable.children;
    r.body
        .colliders
        .iter()
        .map(|c| {
            let mut node = nodes.iter().find(|n| n.id == c.node);
            while let Some(n) = node {
                if let Some(i) = children
                    .iter()
                    .position(|child| child.transform == n.source)
                {
                    return Some(i);
                }
                node = n
                    .parent
                    .and_then(|parent| nodes.iter().find(|p| p.id == parent));
            }
            None
        })
        .collect()
}

/// `DamageFrom` sets `sleeping = false`. The target goes `5` m from the body along `direction`.
fn damage_from(physics: &PhysicsWorld, human: &mut Human, direction: Vector) {
    human.sleeping = false;
    let position = physics.rapier().bodies[human.body].translation();
    human.target = position + direction.normalize_or_zero() * DAMAGE_FROM_DISTANCE;
}

/// `Break` of a live human: `newAI.Die(_playSound: true)`.
fn kill(human: &mut Human, deaths: &mut Vec<u32>) {
    if !deaths.contains(&human.id) {
        deaths.push(human.id);
    }
}

impl Humans {
    /// The index of the live human that owns `collider`, and the collider index in
    /// `body.colliders`.
    fn live_of(&self, physics: &PhysicsWorld, collider: ColliderHandle) -> Option<(usize, usize)> {
        let rapier = physics.rapier();
        let body = rapier.colliders.get(collider)?.parent()?;
        let human = self
            .humans
            .iter()
            .position(|h| !h.died && h.body == body && !self.deaths.contains(&h.id))?;
        let index = rapier.bodies[body]
            .colliders()
            .iter()
            .position(|c| *c == collider)?;
        Some((human, index))
    }

    /// `weaponscript.Shot` on a human collider: `breakchilds` or `breakablescript.TryBreakShoot`.
    /// False when the collider is not a live human.
    pub fn shoot(
        &mut self,
        r: &HumanReference,
        physics: &PhysicsWorld,
        collider: ColliderHandle,
        force: f32,
        direction: Vector,
    ) -> bool {
        let Some((human, index)) = self.live_of(physics, collider) else {
            return false;
        };
        let rules = &r.breakable;
        let modifier = self.children[index].map_or(1., |i| rules.children[i].modifier);
        if rules.no_shoot {
            return true;
        }
        let human = &mut self.humans[human];
        damage_from(physics, human, -direction);
        if try_break_shoot(rules, &mut human.health, force * modifier) {
            kill(human, &mut self.deaths);
        }
        true
    }

    /// `explosionscript.Explode` on a human collider: `breakablescript.TryBreakExplosion`.
    pub fn explode(
        &mut self,
        r: &HumanReference,
        physics: &PhysicsWorld,
        collider: ColliderHandle,
        power: f32,
    ) {
        if let Some((i, _)) = self.live_of(physics, collider) {
            let human = &mut self.humans[i];
            if try_break_explosion(&r.breakable, &mut human.health, power) {
                kill(human, &mut self.deaths);
            }
        }
    }

    /// `OnCollisionEnter` and `OnTriggerEnter` of the last physics step.
    pub fn collide(&mut self, r: &HumanReference, physics: &PhysicsWorld) {
        let rules = &r.breakable;
        let rapier = physics.rapier();
        for hit in physics.hits() {
            for (k, own) in hit.colliders.iter().enumerate() {
                let other = rapier.colliders.get(hit.colliders[1 - k]);
                let layer = other.map(|c| c.collision_groups().memberships.bits().trailing_zeros());
                if layer.is_some_and(|l| IGNORED_HIT_LAYERS.contains(&l)) {
                    continue;
                }
                let Some(force) = collision_force(rules, hit.speed) else {
                    continue;
                };
                let Some((i, _)) = self.live_of(physics, *own) else {
                    continue;
                };
                let human = &mut self.humans[i];
                let position = rapier.bodies[human.body].translation();
                damage_from(physics, human, Vector::from_array(hit.point) - position);
                if try_break(rules, &mut human.health.health, force) {
                    kill(human, &mut self.deaths);
                }
            }
        }
        if rules.no_trigger {
            return;
        }
        for pair in physics.trigger_enters() {
            for (k, own) in pair.iter().enumerate() {
                // `c.attachedRigidbody`. A static collider does not break the human.
                let Some(other) = rapier
                    .colliders
                    .get(pair[1 - k])
                    .and_then(|c| c.parent())
                    .map(|b| &rapier.bodies[b])
                else {
                    continue;
                };
                let Some((i, _)) = self.live_of(physics, *own) else {
                    continue;
                };
                let human = &mut self.humans[i];
                if other.colliders().contains(own) {
                    continue;
                }
                let force = body_force(rules, other.linvel().length(), other.mass());
                let position = rapier.bodies[human.body].translation();
                damage_from(physics, human, other.translation() - position);
                if try_break(rules, &mut human.health.health, force) {
                    kill(human, &mut self.deaths);
                }
            }
        }
    }

    /// The next human that broke this step. The caller calls `die` and plays the death sound.
    pub fn take_death(&mut self) -> Option<u32> {
        self.deaths.pop()
    }
}
