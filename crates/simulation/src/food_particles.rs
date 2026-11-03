//! Source `mainscript.FoodParticle`. A bite emits from the shared system of its food type.
use crate::particles::{ParticleOutput, ParticleState};
use crate::reference::ParticleReference;
use crate::vehicle::PhysicsWorld;
use crate::world::unity_random::UnityRandom;
use rapier3d::math::Vector;

/// The `FoodParticles` systems, in `foodParticleType` order.
#[derive(Debug)]
pub struct FoodParticles {
    systems: Vec<ParticleState>,
    random: UnityRandom,
}
impl FoodParticles {
    pub fn new(references: &[ParticleReference], seed: u64) -> Self {
        Self {
            systems: references
                .iter()
                .enumerate()
                .map(|(i, r)| ParticleState::emit_only(r, seed.wrapping_add(i as u64)))
                .collect(),
            random: UnityRandom::new(seed as i32),
        }
    }
    /// `ediblescript.Eat` with `hasParticle`: `Random.Range(min, max + 1)` particles when above 0.
    /// `forward` is the `particlePos` forward. A zero forward keeps the last system rotation.
    pub fn bite(&mut self, kind: usize, [min, max]: [i32; 2], absolute: [f64; 3], forward: Vector) {
        let count = self.random.range_int(min, max.saturating_add(1));
        if count <= 0 {
            return;
        }
        let system = &mut self.systems[kind];
        let rotation = if forward == Vector::ZERO {
            system.rotation
        } else {
            crate::interaction::melee::look_rotation(forward)
        };
        system.emit(count as usize, absolute, rotation);
    }
    pub fn advance(&mut self, dt: f32, origin: [f64; 3], physics: &PhysicsWorld) {
        for p in &mut self.systems {
            p.set_origin(origin);
            p.advance(dt, origin, 0., physics);
        }
    }
    pub fn particles(&self, kind: usize) -> &[ParticleOutput] {
        self.systems[kind].particles()
    }
}
