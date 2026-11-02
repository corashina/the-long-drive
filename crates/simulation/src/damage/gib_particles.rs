//! ParticleSystems of a gib prefab. Source `breakablescript.Break` instantiates the gib, and each
//! system plays its burst and its duration of emission once. As explosion particles, one shared
//! system per gib particle emits all of these particles at the break. Gib particles are not saved.
use crate::particles::{ParticleOutput, ParticleState};
use crate::reference::ItemPrototype;
use crate::vehicle::PhysicsWorld;
use rapier3d::math::Rotation;

/// One gib ParticleSystem: the prototype index, its particle index, its count and its state.
#[derive(Debug)]
struct GibSystem {
    prototype: usize,
    particle: usize,
    count: usize,
    state: ParticleState,
}

#[derive(Debug)]
pub struct GibParticles {
    systems: Vec<GibSystem>,
}

impl GibParticles {
    pub fn new(prototypes: &[ItemPrototype], seed: u64) -> Self {
        let mut systems = Vec::new();
        for (prototype, p) in prototypes.iter().enumerate() {
            for (particle, r) in p.behavior.gib_particles.iter().enumerate() {
                let burst = if r.scalars.get("EmissionModule.m_BurstCount") == Some(&1.) {
                    r.curves["EmissionModule.m_Bursts.0.countCurve"].scalar
                } else {
                    0.
                };
                systems.push(GibSystem {
                    prototype,
                    particle,
                    count: (burst + r.rate_over_time * r.duration).round() as usize,
                    state: ParticleState::emit_only(r, seed.wrapping_add(systems.len() as u64)),
                });
            }
        }
        Self { systems }
    }
    /// The gib of item prototype `prototype` spawns at `absolute` with `rotation`.
    pub fn spawn(&mut self, prototype: usize, absolute: [f64; 3], rotation: Rotation) {
        for s in self.systems.iter_mut().filter(|s| s.prototype == prototype) {
            s.state.emit(s.count, absolute, rotation);
        }
    }
    pub fn advance(&mut self, dt: f32, origin: [f64; 3], physics: &PhysicsWorld) {
        for s in &mut self.systems {
            s.state.set_origin(origin);
            s.state.advance(dt, origin, 0., physics);
        }
    }
    /// `(prototype, particle, live particles)` of each gib ParticleSystem.
    pub fn systems(&self) -> impl Iterator<Item = (usize, usize, &[ParticleOutput])> {
        self.systems
            .iter()
            .map(|s| (s.prototype, s.particle, s.state.particles()))
    }
}
