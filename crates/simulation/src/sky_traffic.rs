//! Source AirPlaneSpawnScript: planes that fly over the player. Not saved.
use crate::particles::ParticleState;
use crate::reference::SkyReference;
use crate::vehicle::PhysicsWorld;
use crate::weather::StormRandom;
use rapier3d::math::{Rotation, Vector};

#[derive(Debug)]
pub struct Plane {
    /// Never repeats in a session. Presentation and audio key the plane by it.
    pub id: u32,
    pub prefab: usize,
    /// Position relative to the anchor: player + up * height.
    pub offset: Vector,
    pub heading: Rotation,
    /// Source planetrail. It simulates in the anchor space: its positions are anchor offsets.
    pub contrail: Option<ParticleState>,
}

pub struct SkyTraffic {
    seed: u64,
    last_spawn: f64,
    wait: f32,
    next_id: u32,
    planes: Vec<Plane>,
    random: StormRandom,
}

impl SkyTraffic {
    pub fn new(r: &SkyReference, seed: u64) -> Self {
        Self {
            seed,
            // The source wait starts at 0, so the first step spawns.
            last_spawn: 0.,
            wait: 0.,
            next_id: 0,
            planes: Vec::with_capacity(r.max_planes as usize),
            random: StormRandom(seed ^ 0x736b_7974_7261_0001),
        }
    }
    pub fn planes(&self) -> &[Plane] {
        &self.planes
    }

    /// Source AirPlaneSpawnScript.Update.
    pub fn advance(&mut self, r: &SkyReference, now: f64, dt: f32) {
        for plane in &mut self.planes {
            // Source +Z is port -Z.
            plane.offset += plane.heading * Vector::NEG_Z * (r.speed * dt);
        }
        // The anchor follows the player, so the offset length is the source distance.
        self.planes
            .retain(|plane| plane.offset.length() <= r.destroy_distance);
        if now - self.last_spawn < self.wait as f64 || self.planes.len() >= r.max_planes as usize {
            return;
        }
        self.last_spawn = now;
        self.wait = self
            .random
            .range(r.spawn_time_range[0], r.spawn_time_range[1]);
        let prefab = self.prefab(&r.chances);
        // insideUnitCircle on the horizontal plane.
        let angle = self.random.range(0., std::f32::consts::TAU);
        let radius = self.random.unit().sqrt() * r.spawn_distance;
        let offset = Vector::new(angle.cos() * radius, 0., angle.sin() * radius);
        // A source yaw about +Y is the negative port yaw, because source +Z is port -Z.
        let yaw = self.random.range(-r.max_angle, r.max_angle);
        self.planes.push(Plane {
            id: self.next_id,
            prefab,
            offset,
            heading: Rotation::from_rotation_y(-yaw.to_radians()),
            contrail: r
                .prefabs
                .get(prefab)
                .and_then(|p| p.contrail.as_ref())
                .map(|c| ParticleState::new(c, self.seed.wrapping_add(u64::from(self.next_id)))),
        });
        self.next_id += 1;
    }

    /// Advances the contrails after `advance`. `origin` is the physics origin minus the anchor.
    /// It maps anchor offsets to physics positions for the collision module.
    pub fn advance_contrails(&mut self, dt: f32, origin: [f64; 3], physics: &PhysicsWorld) {
        for plane in &mut self.planes {
            let Some(contrail) = &mut plane.contrail else {
                continue;
            };
            contrail.rotation = plane.heading;
            // Set the field directly: set_origin resets the emitter, and the anchor moves each step.
            contrail.origin = origin;
            contrail.advance(dt, plane.offset.to_array().map(f64::from), 1., physics);
        }
    }

    /// Source cumulative chance pick. Falls back to prefab 0.
    fn prefab(&mut self, chances: &[f32]) -> usize {
        let draw = self.random.unit() * chances.iter().sum::<f32>();
        let mut bound = 0.;
        chances
            .iter()
            .position(|chance| {
                bound += chance;
                draw < bound
            })
            .unwrap_or(0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::reference::ReferenceManifest;
    use crate::simulation::FIXED_DT;

    /// CR1. Source `AirPlaneSpawnScript.Update`: the first plane spawns at start, then one every
    /// `Random.Range(minSpawnTime, maxSpawnTime)` s while fewer than `maxPlaneNum` fly.
    /// Planes fly at `height` above the player. About one in ten is the UFO.
    #[test]
    #[ignore = "requires the installed source export"]
    fn tp_cr1_planes_spawn_in_gaps_with_one_ufo_in_ten() {
        let manifest = ReferenceManifest::from_runtime_json(
            &std::fs::read(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../../web/assets/reference/manifest.json"
            ))
            .unwrap(),
        )
        .unwrap();
        let r = &manifest.sky;
        assert_eq!(r.height, 600.);
        assert_eq!(r.spawn_time_range, [50., 500.]);
        assert_eq!(r.max_planes, 3);
        assert_eq!(r.chances, [0.9, 0.1]);
        let ufo = 1;
        assert!(r.prefabs[0].contrail.is_some(), "planes leave a contrail");
        assert!(r.prefabs[ufo].spin.is_some(), "the UFO spins");
        assert!(
            r.prefabs.iter().all(|p| !p.voices.is_empty()),
            "each prefab sounds"
        );
        let (mut spawns, mut ufos) = (0, 0);
        for seed in 0..20 {
            let mut sky = SkyTraffic::new(r, seed);
            let mut now = 0.;
            sky.advance(r, now, FIXED_DT);
            assert_eq!(sky.planes().len(), 1, "seed {seed}: first plane at start");
            let (mut last, mut blocked, mut next_id) = (now, false, 1);
            while now < 7200. {
                now += f64::from(FIXED_DT);
                blocked |= sky.planes().len() >= r.max_planes as usize;
                sky.advance(r, now, FIXED_DT);
                assert!(sky.planes().len() <= r.max_planes as usize);
                let Some(plane) = sky.planes().iter().find(|p| p.id == next_id) else {
                    continue;
                };
                let gap = now - last;
                assert!(gap >= 50., "seed {seed}: gap {gap}");
                assert!(
                    blocked || gap <= 500. + f64::from(FIXED_DT),
                    "seed {seed}: gap {gap}"
                );
                assert!(plane.offset.y == 0. && plane.offset.length() <= r.spawn_distance);
                spawns += 1;
                ufos += usize::from(plane.prefab == ufo);
                (last, blocked, next_id) = (now, false, next_id + 1);
            }
        }
        let ratio = ufos as f32 / spawns as f32;
        assert!(spawns > 300, "{spawns} spawns");
        assert!((0.05..0.15).contains(&ratio), "UFO ratio {ratio}");
    }
}
