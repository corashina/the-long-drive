//! fpscontroller `heatHaze`: the old particle heat haze. The system simulates in player root space.
use crate::particles::{ParticleOutput, ParticleState, particle_curve};
use crate::reference::{ParticleCurve, ParticleReference};
use crate::vehicle::PhysicsWorld;

pub struct OldHeatHaze {
    state: ParticleState,
    rate: ParticleCurve,
    duration: f32,
    active: bool,
}
impl OldHeatHaze {
    pub fn new(r: &ParticleReference) -> Self {
        Self {
            state: ParticleState::new(r, r.source.path_id as u64),
            rate: r.curves["EmissionModule.rateOverTime"].clone(),
            duration: r.duration,
            active: false,
        }
    }
    /// `heatHaze.SetActive(BOldHeatHaze)`. Activation starts the system again with its prewarm.
    pub fn step(&mut self, on: bool, dt: f32, physics: &PhysicsWorld) {
        if !on {
            self.active = false;
            self.state.clear();
            return;
        }
        if !self.active {
            self.active = true;
            self.state.restart();
            // Prewarm: one full loop before the first visible frame.
            for _ in 0..(self.duration / dt).ceil() as u32 {
                self.advance(dt, physics);
            }
        }
        self.advance(dt, physics);
    }
    fn advance(&mut self, dt: f32, physics: &PhysicsWorld) {
        // Random between two constants: one draw for each emission update.
        let draw = self.state.random_draw();
        let rate = particle_curve(&self.rate, self.state.time() / self.duration, draw);
        self.state.set_rates(rate, 0.);
        self.state.advance(dt, [0.; 3], 1., physics);
    }
    /// Positions are in player root space.
    pub fn particles(&self) -> &[ParticleOutput] {
        self.state.particles()
    }
}
