//! Source `carscript` coolant steam: the boil fraction sets the `CoolantSteam` emission rate.
use crate::particles::{ParticleOutput, ParticleState};
use crate::reference::CoolantSteamReference;
use crate::vehicle::PhysicsWorld;
use rapier3d::math::Rotation;

/// Per-car `carscript.CoolantSteam` system. A car without `CoolantBoil` has none.
#[derive(Debug)]
pub struct CoolantSteamState {
    system: Option<(ParticleState, f32)>,
    boil: f32,
}
impl CoolantSteamState {
    pub fn new(reference: Option<&CoolantSteamReference>, seed: u64) -> Self {
        Self {
            system: reference.map(|r| (ParticleState::new(&r.particle, seed), r.max_rate)),
            boil: 0.,
        }
    }
    /// `boil` is the engine boil fraction. `root_absolute` and `rotation` are the car root pose.
    pub fn advance(
        &mut self,
        dt: f32,
        boil: f32,
        root_absolute: [f64; 3],
        rotation: Rotation,
        origin: [f64; 3],
        physics: &PhysicsWorld,
    ) {
        let Some((system, max_rate)) = &mut self.system else {
            return;
        };
        self.boil = boil;
        system.rotation = rotation;
        system.set_origin(origin);
        system.advance(dt, root_absolute, steam_rate(boil, *max_rate), physics);
    }
    /// The boil fraction of the last step. The boil loop volume follows it.
    pub fn boil(&self) -> f32 {
        self.boil
    }
    pub fn particles(&self) -> &[ParticleOutput] {
        self.system.as_ref().map_or(&[], |(s, _)| s.particles())
    }
}

/// Source `CoolantSteamEmission.rateOverTime = Lerp(0, CoolantSteamMaxRate, f)`.
pub fn steam_rate(boil: f32, max_rate: f32) -> f32 {
    max_rate * boil.clamp(0., 1.)
}

/// Source `CoolantBoil.volume = Lerp(0, coolantBoilMaxSound, f)`.
pub fn boil_gain(boil: f32, max_gain: f32) -> f32 {
    max_gain * boil.clamp(0., 1.)
}
