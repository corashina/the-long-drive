//! Source exhaust smoke rules: `fluidcontainer.GetBurnColor` and `carscript` smoke rate.
use crate::fluids::FluidContainer;
use crate::particles::{ParticleOutput, ParticleState};
use crate::reference::ParticleReference;
use crate::simulation::dials::inverse_lerp;
use crate::vehicle::PhysicsWorld;
use rapier3d::math::Rotation;

/// Source `carscript` sets the smoke start color this often while the engine runs.
const COLOR_INTERVAL: f32 = 2.;

pub struct ExhaustInput<'a> {
    pub running: bool,
    pub rpm: f32,
    /// Mounted engine `maxRpm` and `defSmokeRate`.
    pub max_rpm: f32,
    pub smoke_rate: f32,
    pub tank: Option<&'a FluidContainer>,
    /// Absolute position of the car root node.
    pub root_absolute: [f64; 3],
    /// Rotation of the car root node.
    pub rotation: Rotation,
    /// Absolute position of the physics origin.
    pub origin: [f64; 3],
}

/// Per-car `carscript.psSmoke` systems.
#[derive(Debug)]
pub struct ExhaustState {
    systems: Vec<ParticleState>,
    next_color: f32,
    elapsed: f32,
}
impl ExhaustState {
    pub fn new(systems: &[ParticleReference], seed: u64) -> Self {
        Self {
            systems: systems
                .iter()
                .enumerate()
                .map(|(i, r)| ParticleState::new(r, seed.wrapping_add(i as u64)))
                .collect(),
            next_color: 0.,
            elapsed: 0.,
        }
    }
    pub fn advance(
        &mut self,
        dt: f32,
        input: ExhaustInput,
        colors: &[[f32; 4]; 6],
        physics: &PhysicsWorld,
    ) {
        self.elapsed += dt;
        // Source: without a tank the smoke keeps its last color.
        let color = (input.running && self.elapsed >= self.next_color)
            .then_some(input.tank)
            .flatten()
            .map(|tank| {
                self.next_color = self.elapsed + COLOR_INTERVAL;
                burn_color(tank, colors)
            });
        let strength = smoke_rate(input.running, input.rpm, input.max_rpm, input.smoke_rate);
        for system in &mut self.systems {
            if color.is_some() {
                system.start_color = color;
            }
            system.rotation = input.rotation;
            system.set_origin(input.origin);
            system.advance(dt, input.root_absolute, strength, physics);
        }
    }
    pub fn particles(&self, system: usize) -> &[ParticleOutput] {
        self.systems[system].particles()
    }
}

/// Source `GetBurnColor`: each fluid adds its color by share; each later fluid halves the sum.
pub fn burn_color(tank: &FluidContainer, colors: &[[f32; 4]; 6]) -> [f32; 4] {
    let total = tank.total();
    let mut color = [0.; 4];
    if !(total > 0. && total.is_finite()) {
        return color;
    }
    for (i, entry) in tank.entries().enumerate() {
        let weight = inverse_lerp(0., 0.15, entry.amount / total);
        for (c, f) in color.iter_mut().zip(colors[entry.fluid_type as usize]) {
            *c += f * weight;
            if i != 0 {
                *c *= 0.5;
            }
        }
    }
    color
}

/// Source smoke emission rate: scales with rpm while the engine runs.
pub fn smoke_rate(running: bool, rpm: f32, max_rpm: f32, rate: f32) -> f32 {
    if running {
        inverse_lerp(0., max_rpm, rpm) * rate
    } else {
        0.
    }
}

#[cfg(test)]
pub mod tests {
    use super::*;
    use crate::reference::{
        GradientReference, ParticleColor, ParticleCurve, SourceId, TorqueCurve, Transform,
    };

    fn curve(scalar: f32) -> ParticleCurve {
        let flat = || TorqueCurve {
            keys: Vec::new(),
            pre_wrap_mode: 0,
            post_wrap_mode: 0,
        };
        ParticleCurve {
            mode: 0,
            min_scalar: scalar,
            scalar,
            min_curve: flat(),
            max_curve: flat(),
        }
    }

    /// A cone system: 1 per second, start speed 5, size 1.
    pub fn smoke(capacity: u32, lifetime: f32) -> ParticleReference {
        let gradient = || GradientReference {
            mode: 0,
            colors: Vec::new(),
            alphas: Vec::new(),
        };
        ParticleReference {
            source: SourceId {
                file: "level1".into(),
                path_id: 1,
            },
            role: "exhaust_0_0".into(),
            capacity,
            rate_over_time: 1.,
            rate_over_distance: 0.,
            duration: 5.,
            simulation_space: 2,
            local_pose: Transform {
                translation: [0.; 3],
                rotation: [0., 0., 0., 1.],
                scale: [1.; 3],
            },
            module_flags: [("ShapeModule".into(), true)].into(),
            curves: [
                ("InitialModule.startSpeed", curve(5.)),
                ("InitialModule.startLifetime", curve(lifetime)),
                ("InitialModule.startSize", curve(1.)),
                ("InitialModule.gravityModifier", curve(0.)),
                ("InitialModule.startRotation", curve(0.)),
            ]
            .into_iter()
            .map(|(k, v)| (k.into(), v))
            .collect(),
            colors: [(
                "InitialModule.startColor".into(),
                ParticleColor {
                    mode: 0,
                    min_color: [1.; 4],
                    max_color: [1.; 4],
                    min_gradient: gradient(),
                    max_gradient: gradient(),
                },
            )]
            .into(),
            vectors: [
                ("ShapeModule.m_Scale".into(), [1.; 3]),
                ("ShapeModule.m_Rotation".into(), [0.; 3]),
            ]
            .into(),
            scalars: [
                ("ShapeModule.type", 4.),
                ("ShapeModule.angle", 10.),
                ("ShapeModule.radiusThickness", 1.),
                ("ShapeModule.radius.value", 0.1),
                ("ShapeModule.randomPositionAmount", 0.),
                ("ringBufferMode", 0.),
            ]
            .into_iter()
            .map(|(k, v)| (k.into(), v))
            .collect(),
            flags: [("gameObject.active".into(), true)].into(),
            links: Default::default(),
            materials: Vec::new(),
            limits: Vec::new(),
        }
    }
}
