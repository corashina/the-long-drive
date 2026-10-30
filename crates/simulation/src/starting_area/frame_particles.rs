//! Live particles of one tick for the renderer.
use super::GarageRuntime;
use crate::boundary::{ParticleGroupRow, ParticleRow, SimFrame, car_particle_sources};
use crate::interaction::fluid_contact::FluidContact;
use crate::particles::ParticleOutput;
use crate::reference::ReferenceManifest;

impl GarageRuntime {
    /// Writes every particle system of the runtime into `frame`, with its live particles.
    pub(super) fn fill_particles(&self, reference: &ReferenceManifest, frame: &mut SimFrame) {
        let origin = self.stream.origin.offset;
        let anchor = self.sky_anchor(reference);
        // Contrail positions are anchor offsets.
        let sky_origin: [f64; 3] = std::array::from_fn(|i| origin[i] - anchor[i]);
        let mut out = Rows { frame, origin };
        out.frame.particle_groups.clear();
        out.frame.particles.clear();
        let area = &self.area;
        let interaction = &area.interaction;
        for (i, p) in reference.particles.iter().enumerate() {
            let (kind, id, fluid) = if p.role.starts_with("urine") {
                (ParticleGroupRow::URINE, 0, Some(&self.urine))
            } else {
                let id: u32 = p.role.rsplit('_').next().unwrap().parse().unwrap();
                let fluid = interaction
                    .fluid_particles()
                    .find(|(key, _)| key.0 == id)
                    .map(|(_, f)| f);
                (ParticleGroupRow::OUTLET, id, fluid)
            };
            let group = ParticleGroupRow {
                kind,
                a: id,
                source: i as u32,
                ..Default::default()
            };
            let splash = p.role.starts_with("splash") || p.role == "urine_splash";
            match fluid {
                Some(f) if splash => out.particles(group, f.splash()),
                Some(f) => out.fluid(group, f),
                None => out.particles(group, &[]),
            }
        }
        let cleaning = &interaction.cleaning;
        for (wash, w) in reference.conditions.washes.iter().enumerate() {
            for source in 0..w.particles.len() {
                out.owner(
                    ParticleGroupRow::WASH,
                    wash,
                    source,
                    cleaning.wash_particles(wash, source),
                );
            }
        }
        for (tool, b) in reference.conditions.brushes.iter().enumerate() {
            for source in 0..b.tool.particles.len() {
                out.owner(
                    ParticleGroupRow::BRUSH,
                    tool,
                    source,
                    cleaning.particles(b.item, source),
                );
            }
        }
        for index in 0..2 {
            let particles = self.stream.small_item_particles(index);
            out.owner(ParticleGroupRow::SPRAY_BOTTLE, index, 0, particles);
        }
        for (tool, b) in reference.conditions.sprays.iter().enumerate() {
            for source in 0..b.tool.particles.len() {
                let particles = cleaning.particles(b.item, source);
                out.owner(ParticleGroupRow::SPRAY, tool, source, particles);
            }
        }
        for (item, tool, count) in cleaning.prototype_tools() {
            for source in 0..count {
                let group = ParticleGroupRow {
                    kind: ParticleGroupRow::LOOT_TOOL,
                    a: item.0,
                    b: tool as u32,
                    source: source as u32,
                    ..Default::default()
                };
                out.particles(group, cleaning.particles(item, source));
            }
        }
        for source in 0..reference.melee.particles.len() {
            let particles = interaction.melee.particles(source);
            out.owner(ParticleGroupRow::MELEE, 0, source, particles);
        }
        for source in 0..2 {
            let particles = interaction.guns.tracers(source);
            out.owner(ParticleGroupRow::TRACER, 0, source, particles);
        }
        for source in 0..crate::reference::EXPLOSION_TYPES {
            let particles = interaction.explosions.particles(source);
            out.owner(ParticleGroupRow::EXPLOSION, 0, source, particles);
        }
        for source in 0..reference.survival.food_particles.len() {
            let particles = self.food_particles.particles(source);
            out.owner(ParticleGroupRow::FOOD, 0, source, particles);
        }
        let cigarettes = &area.cigarettes;
        out.owner(
            ParticleGroupRow::EXHALE,
            0,
            0,
            cigarettes.exhale_particles(),
        );
        let body = crate::vehicle::to_rapier(area.player.body_pose(&self.physics));
        out.local(
            ParticleGroupRow {
                kind: ParticleGroupRow::HEAT_HAZE,
                ..Default::default()
            },
            self.heat_haze.particles(),
            body,
        );
        for effect in 0..reference.small_items.wash_effects.len() {
            let particles = self.stream.cobweb_particles(effect);
            out.owner(ParticleGroupRow::COBWEB, effect, 0, particles);
        }
        for (id, particles) in cigarettes.embers() {
            out.owner(ParticleGroupRow::EMBER, id.0 as usize, 0, particles);
        }
        for (key, vehicle) in &self.vehicles {
            let car = self.physics.chassis_handle(key).into_raw_parts().0;
            for (system, source) in car_particle_sources(reference, vehicle.prototype) {
                let particles = match system {
                    ParticleGroupRow::CAR_EXHAUST => vehicle.exhaust.particles(source),
                    ParticleGroupRow::CAR_STEAM => vehicle.coolant_steam.particles(),
                    ParticleGroupRow::CAR_WHEEL => {
                        vehicle.wheel_effects.particles(source / 8, source % 8)
                    }
                    _ => vehicle.wheel_effects.burst_particles(source),
                };
                let group = ParticleGroupRow {
                    kind: ParticleGroupRow::CAR,
                    a: system,
                    b: vehicle.prototype as u32,
                    c: car,
                    source: source as u32,
                    ..Default::default()
                };
                out.particles(group, particles);
            }
        }
        for storm in self.weather.storms() {
            for (source, particles) in storm.particles.iter().enumerate() {
                out.owner(
                    ParticleGroupRow::STORM,
                    storm.id.0 as usize,
                    source,
                    particles.particles(),
                );
            }
        }
        out.origin = sky_origin;
        for plane in self.sky().planes() {
            if let Some(contrail) = &plane.contrail {
                let group = ParticleGroupRow {
                    kind: ParticleGroupRow::SKY,
                    a: plane.id,
                    source: plane.prefab as u32,
                    ..Default::default()
                };
                out.particles(group, contrail.particles());
            }
        }
    }
}

struct Rows<'a> {
    frame: &'a mut SimFrame,
    origin: [f64; 3],
}

impl Rows<'_> {
    fn owner(&mut self, kind: u32, id: usize, source: usize, particles: &[ParticleOutput]) {
        let group = ParticleGroupRow {
            kind,
            a: id as u32,
            source: source as u32,
            ..Default::default()
        };
        self.particles(group, particles);
    }
    fn particles(&mut self, mut group: ParticleGroupRow, particles: &[ParticleOutput]) {
        group.start = self.frame.particles.len() as u32;
        group.count = particles.len() as u32;
        let origin = self.origin;
        self.frame
            .particles
            .extend(particles.iter().map(|p| ParticleRow {
                position: std::array::from_fn(|j| (p.absolute_position[j] - origin[j]) as f32),
                size: p.size,
                color: p.color,
                rotation: p.rotation,
                age: p.age,
                lifetime: p.lifetime,
                uv_draw: p.uv_draw(),
            }));
        self.frame.particle_groups.push(group);
    }
    /// `particles` are in `space`: the system simulates in its owner space.
    fn local(
        &mut self,
        mut group: ParticleGroupRow,
        particles: &[ParticleOutput],
        space: rapier3d::math::Pose,
    ) {
        group.start = self.frame.particles.len() as u32;
        group.count = particles.len() as u32;
        self.frame.particles.extend(particles.iter().map(|p| {
            ParticleRow {
                position: (space.translation
                    + space.rotation
                        * rapier3d::math::Vector::from_array(
                            p.absolute_position.map(|v| v as f32),
                        ))
                .to_array(),
                size: p.size,
                color: p.color,
                rotation: p.rotation,
                age: p.age,
                lifetime: p.lifetime,
                uv_draw: p.uv_draw(),
            }
        }));
        self.frame.particle_groups.push(group);
    }
    fn fluid(&mut self, mut group: ParticleGroupRow, fluid: &FluidContact) {
        group.start = self.frame.particles.len() as u32;
        group.count = fluid.particles().len() as u32;
        group.fluid = 1;
        self.frame
            .particles
            .extend(fluid.particles().iter().map(|p| ParticleRow {
                position: p.position.to_array(),
                size: p.size,
                color: p.color,
                rotation: p.rotation,
                ..Default::default()
            }));
        self.frame.particle_groups.push(group);
    }
}
