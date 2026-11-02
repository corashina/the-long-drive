//! Source `explosionscript.Explode`, `tankscript.TryExplode`, `explosivescript.TryExplode`,
//! `fluidcontainer.isExplosive` and the blast particle and sound.
use crate::fluids::FluidContainer;
use crate::particles::{ParticleOutput, ParticleState};
use crate::reference::{
    EXPLOSION_TYPES, ExplosionsReference, ItemId, ReferenceError, ReferenceManifest,
};
use crate::vehicle::PhysicsWorld;
use crate::world::{Explosive, TankExplosion};
use rapier3d::math::{Pose, Rotation, Vector};
use rapier3d::prelude::{Ball, ColliderHandle, QueryFilter, RigidBodyHandle};

/// `Rigidbody.AddExplosionForce` upwards modifier of `explosionscript.Explode`.
const UPWARDS: f32 = 0.3;

/// One blast: the prefab type, `explosionForce` after `pF`, `explosionDist`, the point in
/// physics space, and the particle start color.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Blast {
    pub kind: usize,
    pub force: f32,
    pub radius: f32,
    pub position: Vector,
    pub tint: Option<[f32; 4]>,
}

/// `explosionscript.CalcPower`.
pub fn power(distance: f32, radius: f32, force: f32) -> f32 {
    let t = if radius > 0. {
        (distance / radius).clamp(0., 1.)
    } else {
        1.
    };
    let angle = 90. - 180. * t;
    (f32::to_radians(angle).sin() + 1.) * 0.5 * force
}

/// `fluidcontainer.isExplosive`: explosive fluids are over 40 % of more than 0.2 L.
pub fn is_explosive(fluid: &FluidContainer, explosive: &[u8]) -> bool {
    let total = fluid.total();
    let f: f32 = fluid
        .entries()
        .filter(|e| explosive.contains(&e.fluid_type))
        .map(|e| e.amount)
        .sum();
    f / total > 0.4 && total > 0.2
}

/// `tankscript.TryExplode` without `alreadyExploded`: the `pF` of the blast, or `None`.
pub fn tank_factor(
    e: &TankExplosion,
    fluid: &FluidContainer,
    explosive: &[u8],
    force: f32,
) -> Option<f32> {
    (!e.never_explode
        && (e.always_explode || is_explosive(fluid, explosive))
        && force >= e.force_needed)
        .then(|| {
            if e.always_explode {
                e.always_explode_force
            } else {
                fluid.total()
            }
        })
}

/// `explosivescript.TryExplode`: the `pF` of the blast, or `None`. `rounds` is the item's
/// ammo count when the prop has an ammo link.
pub fn prop_factor(e: &Explosive, rounds: Option<f32>, force: f32) -> Option<f32> {
    if force <= e.force_needed {
        return None;
    }
    match rounds {
        None => Some(e.explode_force),
        Some(r) => (r >= e.min_ammo).then_some(r * e.explode_force),
    }
}

/// `Rigidbody.AddExplosionForce(force, at, radius, UPWARDS, Impulse)` on a body with its
/// center of mass at `com`: the impulse falls linearly to zero at `radius`.
pub fn explosion_impulse(force: f32, at: Vector, radius: f32, com: Vector) -> Vector {
    let distance = com.distance(at);
    if radius > 0. && distance > radius {
        return Vector::ZERO;
    }
    let falloff = if radius > 0. {
        1. - distance / radius
    } else {
        1.
    };
    let centre = at - Vector::Y * UPWARDS;
    (com - centre).normalize_or(Vector::Y) * force * falloff
}

struct ExplosionType {
    force: f32,
    sound: (u32, f32),
    /// Particles that one Instantiate starts: the burst and one duration of the rate.
    count: usize,
}

/// The blasts of one step and their particles.
pub struct ExplosionState {
    types: Vec<ExplosionType>,
    particles: Vec<ParticleState>,
    explosive_fluids: Vec<u8>,
    queue: Vec<Blast>,
    next: usize,
    /// `alreadyExploded` and `exploded` of this step.
    exploded: Vec<ItemId>,
    hits: Vec<(ColliderHandle, f32)>,
    bodies: Vec<RigidBodyHandle>,
}

impl ExplosionState {
    pub fn new(r: &ReferenceManifest) -> Result<Self, ReferenceError> {
        let clips = super::melee::clip_table(r);
        let e: &ExplosionsReference = &r.explosions;
        let mut types = Vec::with_capacity(EXPLOSION_TYPES);
        for t in &e.types {
            let clip = clips
                .iter()
                .find(|(s, _, _)| *s == t.sound.clip)
                .ok_or_else(|| ReferenceError("Missing explosion clip".into()))?
                .1;
            let p = &t.particle;
            let burst = if p.scalars.get("EmissionModule.m_BurstCount") == Some(&1.) {
                p.curves["EmissionModule.m_Bursts.0.countCurve"].scalar
            } else {
                0.
            };
            types.push(ExplosionType {
                force: t.explosion_force,
                sound: (clip, t.sound.volume),
                count: (burst + p.rate_over_time * p.duration).round() as usize,
            });
        }
        Ok(Self {
            types,
            particles: e
                .types
                .iter()
                .enumerate()
                .map(|(i, t)| ParticleState::emit_only(&t.particle, 0x626c_7374 + i as u64))
                .collect(),
            explosive_fluids: e.explosive_fluids.clone(),
            queue: Vec::with_capacity(16),
            next: 0,
            exploded: Vec::with_capacity(16),
            hits: Vec::with_capacity(256),
            bodies: Vec::with_capacity(64),
        })
    }
    pub fn explosive_fluids(&self) -> &[u8] {
        &self.explosive_fluids
    }
    /// `GetExplosionType(kind)` and `explosionscript.Explode(pF, distAdd)` at `position`.
    pub fn blast(
        &self,
        kind: i32,
        factor: f32,
        dist_add: f32,
        position: Vector,
        tint: Option<[f32; 4]>,
    ) -> Blast {
        let kind = usize::try_from(kind)
            .ok()
            .filter(|k| *k < EXPLOSION_TYPES)
            .unwrap_or(0);
        let force = self.types[kind].force * factor;
        Blast {
            kind,
            force,
            radius: force * 0.5 + dist_add,
            position,
            tint,
        }
    }
    /// Queues the blast of `item`. False when `item` already exploded this step.
    pub fn trigger(&mut self, item: ItemId, blast: Blast) -> bool {
        if self.exploded.contains(&item) {
            return false;
        }
        self.exploded.push(item);
        self.queue.push(blast);
        true
    }
    /// The next queued blast. Blasts that it triggers run after it.
    pub fn next(&mut self) -> Option<Blast> {
        let blast = self.queue.get(self.next).copied()?;
        self.next += 1;
        Some(blast)
    }
    /// Ends the step: the queue is empty.
    pub fn finish(&mut self) {
        self.queue.clear();
        self.exploded.clear();
        self.next = 0;
    }
    /// `Physics.OverlapSphere` with all layers and triggers: each collider and its power, and
    /// each body once.
    pub fn query(&mut self, physics: &PhysicsWorld, blast: &Blast) {
        self.hits.clear();
        self.bodies.clear();
        let predicate = |_, c: &rapier3d::prelude::Collider| c.is_enabled();
        let filter = QueryFilter::default().predicate(&predicate);
        let ball = Ball::new(blast.radius.max(f32::EPSILON));
        let at = Pose::from_translation(blast.position);
        for (handle, collider) in physics.queries(filter).intersect_shape(at, &ball) {
            let d = collider.translation().distance(blast.position);
            self.hits
                .push((handle, power(d, blast.radius, blast.force)));
            if let Some(body) = collider.parent()
                && !self.bodies.contains(&body)
            {
                self.bodies.push(body);
            }
        }
    }
    pub fn hits(&self) -> &[(ColliderHandle, f32)] {
        &self.hits
    }
    /// Each body of the last query once.
    pub fn bodies(&self) -> &[RigidBodyHandle] {
        &self.bodies
    }
    /// `AddExplosionForce(2 * force)` on each dynamic body of the last query but the player.
    pub fn push(&self, physics: &mut PhysicsWorld, blast: &Blast, player: Option<RigidBodyHandle>) {
        for &body in &self.bodies {
            // A chained blast removes its imported tank body.
            let Some(rb) = physics.rapier().bodies.get(body) else {
                continue;
            };
            if Some(body) == player || !rb.is_dynamic() {
                continue;
            }
            let impulse = explosion_impulse(
                2. * blast.force,
                blast.position,
                blast.radius,
                rb.center_of_mass(),
            );
            physics.edit_body(body, |b| b.apply_impulse(impulse, true));
        }
    }
    /// The prefab particle at the point with its start color, and the blast clip and gain.
    pub fn effect(&mut self, blast: &Blast, absolute: [f64; 3]) -> (u32, f32) {
        let t = &self.types[blast.kind];
        let p = &mut self.particles[blast.kind];
        p.start_color = blast.tint;
        p.emit(t.count, absolute, Rotation::IDENTITY);
        t.sound
    }
    pub fn advance(&mut self, dt: f32, origin: [f64; 3], physics: &PhysicsWorld) {
        for p in &mut self.particles {
            p.set_origin(origin);
            p.advance(dt, origin, 0., physics);
        }
    }
    pub fn particles(&self, kind: usize) -> &[ParticleOutput] {
        self.particles[kind].particles()
    }
}
