//! Regional weather cells. A new feature: the source has only the sandstorm.
//! A cell is a moving front with a kind, a radius and a life. The weather at a point is the sum of the cells there.
use super::{StormRandom, WeatherKind, WeatherLook, WeatherMix};
use serde::{Deserialize, Serialize};

pub const MAX_CELLS: usize = 8;
const KINDS: usize = WeatherKind::ALL.len();
/// The cells near the player: spawn at once below the first value, spawn on a timer below the second.
const NEAR: [usize; 2] = [2, 4];
/// Meters from the player to the center of a near cell.
const NEAR_DISTANCE: f64 = 6000.;
/// A cell farther than this from the player is removed.
const RETIRE_DISTANCE: f64 = 12_000.;
/// Meters from the player to a new cell, upwind.
const SPAWN_DISTANCE: [f32; 2] = [3000., 6000.];
/// Radians to each side of upwind for a new cell.
const SPAWN_SPREAD: f32 = 1.;
/// Seconds between timer spawns.
const SPAWN_WAIT: [f32; 2] = [180., 600.];
const RADIUS: [f32; 2] = [1500., 5000.];
const STORM_RADIUS: [f32; 2] = [1000., 2500.];
/// Game seconds of one cell life.
const LIFE: [f32; 2] = [1200., 3600.];
/// The default radius of an F4 cell.
pub const ADD_RADIUS: f32 = 3000.;
/// The radius of the lock cell.
pub const LOCK_RADIUS: f32 = 20_000.;
/// The life share of the growth and of the decay.
const GROW: f32 = 0.2;
const DECAY: f32 = 0.3;
/// The radius share at full weight. The weight falls to 0 at the radius.
const EDGE: f32 = 0.7;
/// Spawn weights by the biome under the spawn point, in `WeatherKind` order.
/// Rows: desert, snow, grass (`terrain::biome`).
const BIOME_WEIGHTS: [[f32; KINDS]; 3] = [
    [0.5, 0.4, 0.1, 0., 0.],
    [0.3, 0.35, 0.15, 0., 0.2],
    [0.35, 0.25, 0.18, 0.1, 0.12],
];

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WeatherCell {
    pub kind: WeatherKind,
    /// Absolute world `x`, `z`.
    pub center: [f64; 2],
    pub radius: f32,
    /// Game seconds.
    pub life: f32,
    pub age: f32,
}

impl WeatherCell {
    /// The life curve, 0 to 1.
    pub fn strength(&self) -> f32 {
        life_strength(self.age, self.life)
    }

    /// The cell weight at absolute `x`, `z`, 0 to 1.
    pub fn weight(&self, xz: [f64; 2]) -> f32 {
        let distance = (self.center[0] - xz[0]).hypot(self.center[1] - xz[1]) as f32;
        let t = ((distance / self.radius - EDGE) / (1. - EDGE)).clamp(0., 1.);
        self.strength() * (1. - t * t * (3. - 2. * t))
    }

    /// Finite values in range. Save and net use this rule.
    pub fn valid(&self) -> bool {
        self.center
            .iter()
            .all(|v| v.is_finite() && v.abs() <= crate::world::MAX_ABSOLUTE_COORDINATE)
            && self.radius.is_finite()
            && self.radius > 0.
            && self.radius <= LOCK_RADIUS
            && self.life.is_finite()
            && self.life > 0.
            && self.age.is_finite()
            && (0. ..=self.life).contains(&self.age)
    }
}

/// 0 to 1 in the first `GROW` share of the life, 1, then 1 to 0 in the last `DECAY` share.
fn life_strength(age: f32, life: f32) -> f32 {
    let t = (age / life).clamp(0., 1.);
    (t / GROW).min((1. - t) / DECAY).min(1.)
}

/// The kind weights at one point, each 0 to 1.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct CellSample {
    pub weights: [f32; KINDS],
}

impl CellSample {
    pub fn weight(&self, kind: WeatherKind) -> f32 {
        self.weights[kind as usize]
    }

    /// The sum of the kind targets by weight. Wetness and flash are 0.
    pub fn mix(&self) -> WeatherMix {
        let mut mix = WeatherMix::default();
        for (kind, &w) in WeatherKind::ALL.iter().zip(&self.weights) {
            let t = kind.target();
            mix.rain += t.rain * w;
            mix.temperature += t.temperature * w;
        }
        mix.rain = mix.rain.min(1.);
        mix
    }

    /// The kind looks blended by weight. Clear also gets the share that no cell covers.
    /// The weights are scaled to a sum of 1. The grey shades blend by their grey share.
    pub fn look(&self) -> WeatherLook {
        let covered: f32 = self.weights[1..].iter().sum();
        let clear = self.weights[0].max(1. - covered);
        let scale = 1. / (clear + covered);
        let mut look = WeatherLook {
            cloud_shade: 0.,
            fog_shade: [0.; 3],
            ..WeatherLook::default()
        };
        for kind in WeatherKind::ALL {
            let w = match kind {
                WeatherKind::Clear => clear * scale,
                _ => self.weight(kind) * scale,
            };
            let row = kind.look();
            look.sun += row.sun * w;
            look.ambient += row.ambient * w;
            look.sky += row.sky * w;
            look.cloud_cover += row.cloud_cover * w;
            look.cloud_grey += row.cloud_grey * w;
            look.cloud_shade += row.cloud_shade * row.cloud_grey * w;
            look.cloud_hide += row.cloud_hide * w;
            look.fog_density += row.fog_density * w;
            look.fog_grey += row.fog_grey * w;
            for (shade, row_shade) in look.fog_shade.iter_mut().zip(row.fog_shade) {
                *shade += row_shade * row.fog_grey * w;
            }
            look.saturation += row.saturation * w;
            look.contrast += row.contrast * w;
            look.warmth += row.warmth * w;
            look.flare += row.flare * w;
        }
        look.cloud_shade = shade(look.cloud_shade, look.cloud_grey);
        look.fog_shade = look.fog_shade.map(|s| shade(s, look.fog_grey));
        look
    }

    /// The kind with the largest weight. Clear also gets the share that no cell covers.
    pub fn dominant(&self) -> WeatherKind {
        let covered: f32 = self.weights[1..].iter().sum();
        let mut best = (WeatherKind::Clear, self.weights[0].max(1. - covered));
        for kind in &WeatherKind::ALL[1..] {
            if self.weight(*kind) > best.1 {
                best = (*kind, self.weight(*kind));
            }
        }
        best.0
    }
}

#[derive(Debug)]
pub struct WeatherCells {
    cells: [Option<WeatherCell>; MAX_CELLS],
    random: StormRandom,
    /// Game seconds until the next timer spawn.
    wait: f32,
    /// The F4 lock: one lock cell at the player and no spawns.
    locked: bool,
}

impl WeatherCells {
    pub fn new(seed: u64) -> Self {
        let mut random = StormRandom(seed ^ 0x6365_6c6c_7300_0001);
        let wait = random.range(SPAWN_WAIT[0], SPAWN_WAIT[1]);
        Self {
            cells: [None; MAX_CELLS],
            random,
            wait,
            locked: false,
        }
    }

    /// The live cells.
    pub fn iter(&self) -> impl Iterator<Item = &WeatherCell> {
        self.cells.iter().flatten()
    }

    pub fn len(&self) -> usize {
        self.iter().count()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn locked(&self) -> bool {
        self.locked
    }

    /// The kind weights at absolute `x`, `z`. Each is clamped to 1.
    pub fn sample(&self, xz: [f64; 2]) -> CellSample {
        let mut sample = CellSample::default();
        for cell in self.iter() {
            sample.weights[cell.kind as usize] += cell.weight(xz);
        }
        for w in &mut sample.weights {
            *w = w.min(1.);
        }
        sample
    }

    /// Moves the cells by `drift` in meters per second. The host also ages, removes and spawns cells.
    /// A `guest` only moves them: the host sends the list. `biome_at` gives the biome at absolute `x`, `z`.
    pub fn advance(
        &mut self,
        dt: f32,
        drift: [f32; 2],
        player: [f64; 2],
        guest: bool,
        biome_at: impl FnOnce([f64; 2]) -> u8,
    ) {
        if self.locked {
            // The lock cell stays at the player at full strength.
            for cell in self.cells.iter_mut().flatten() {
                cell.center = player;
            }
            return;
        }
        for cell in self.cells.iter_mut().flatten() {
            cell.center[0] += f64::from(drift[0] * dt);
            cell.center[1] += f64::from(drift[1] * dt);
            if !guest {
                cell.age += dt;
            }
        }
        if guest {
            return;
        }
        let distance = |c: &WeatherCell| (c.center[0] - player[0]).hypot(c.center[1] - player[1]);
        for slot in &mut self.cells {
            if slot.is_some_and(|c| c.age >= c.life || distance(&c) > RETIRE_DISTANCE) {
                *slot = None;
            }
        }
        let near = self.iter().filter(|c| distance(c) <= NEAR_DISTANCE).count();
        self.wait -= dt;
        if near < NEAR[0] || (near < NEAR[1] && self.wait <= 0.) {
            self.spawn(drift, player, biome_at);
        }
    }

    /// One new cell upwind of the player, if a slot is free.
    fn spawn(&mut self, drift: [f32; 2], player: [f64; 2], biome_at: impl FnOnce([f64; 2]) -> u8) {
        let Some(index) = self.cells.iter().position(Option::is_none) else {
            return;
        };
        self.wait = self.random.range(SPAWN_WAIT[0], SPAWN_WAIT[1]);
        let upwind = if drift == [0.; 2] {
            self.random.range(0., std::f32::consts::TAU)
        } else {
            (-drift[1]).atan2(-drift[0])
        };
        let angle = upwind + self.random.range(-SPAWN_SPREAD, SPAWN_SPREAD);
        let distance = self.random.range(SPAWN_DISTANCE[0], SPAWN_DISTANCE[1]);
        let center = [
            player[0] + f64::from(angle.cos() * distance),
            player[1] + f64::from(angle.sin() * distance),
        ];
        let kind = self.pick(biome_at(center));
        let radius = if kind == WeatherKind::Thunderstorm {
            STORM_RADIUS
        } else {
            RADIUS
        };
        self.cells[index] = Some(WeatherCell {
            kind,
            center,
            radius: self.random.range(radius[0], radius[1]),
            life: self.random.range(LIFE[0], LIFE[1]),
            age: 0.,
        });
    }

    /// A weighted kind for `biome`.
    fn pick(&mut self, biome: u8) -> WeatherKind {
        let weights = &BIOME_WEIGHTS[usize::from(biome.min(2))];
        let mut draw = self.random.unit() * weights.iter().sum::<f32>();
        for (kind, &w) in WeatherKind::ALL.iter().zip(weights) {
            if draw < w {
                return *kind;
            }
            draw -= w;
        }
        WeatherKind::ALL[weights
            .iter()
            .rposition(|&w| w > 0.)
            .expect("one kind has weight")]
    }

    /// `Some` removes every cell, adds a lock cell of the kind at the player and stops spawns.
    /// `None` removes the lock cell and starts spawns again.
    pub fn lock(&mut self, kind: Option<WeatherKind>, player: [f64; 2]) {
        self.cells = [None; MAX_CELLS];
        self.locked = kind.is_some();
        if let Some(kind) = kind {
            // The middle of the life is at full strength. A locked cell does not age.
            self.cells[0] = Some(WeatherCell {
                kind,
                center: player,
                radius: LOCK_RADIUS,
                life: LIFE[1],
                age: LIFE[1] * 0.5,
            });
        }
    }

    /// Adds a cell at full strength, for F4. It removes the lock. With no free slot, it replaces the weakest cell.
    pub fn add(&mut self, kind: WeatherKind, center: [f64; 2], radius: f32) {
        if self.locked {
            self.lock(None, center);
        }
        let index = self
            .cells
            .iter()
            .position(Option::is_none)
            .unwrap_or_else(|| {
                let strength = |i: usize| self.cells[i].map_or(0., |c| c.strength());
                (0..MAX_CELLS)
                    .min_by(|&a, &b| strength(a).total_cmp(&strength(b)))
                    .expect("MAX_CELLS is not 0")
            });
        // The full strength starts at the end of the growth.
        self.cells[index] = Some(WeatherCell {
            kind,
            center,
            radius,
            life: LIFE[1],
            age: LIFE[1] * GROW,
        });
    }

    /// Replaces the cells, for a load or a host message. Cells after `MAX_CELLS` are dropped.
    pub fn set(&mut self, cells: impl IntoIterator<Item = WeatherCell>, locked: bool) {
        self.cells = [None; MAX_CELLS];
        for (slot, cell) in self.cells.iter_mut().zip(cells) {
            *slot = Some(cell);
        }
        self.locked = locked;
    }
}

/// The grey shade from its grey-weighted sum. No grey gives 1.
fn shade(sum: f32, grey: f32) -> f32 {
    if grey > 0. { sum / grey } else { 1. }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(seed: u64, biome: u8, steps: usize) -> WeatherCells {
        let mut cells = WeatherCells::new(seed);
        for _ in 0..steps {
            cells.advance(10., [3., 1.], [0.; 2], false, |_| biome);
        }
        cells
    }

    #[test]
    fn one_seed_gives_one_sequence() {
        let list = |cells: WeatherCells| cells.iter().copied().collect::<Vec<_>>();
        let first = list(run(42, 2, 2000));
        assert!(first.len() >= NEAR[0]);
        assert_eq!(first, list(run(42, 2, 2000)));
    }

    #[test]
    fn the_sample_is_zero_outside_every_cell() {
        let cells = run(7, 2, 100);
        assert!(!cells.is_empty());
        let sample = cells.sample([1e6, 1e6]);
        assert_eq!(sample, CellSample::default());
        assert_eq!(sample.mix(), WeatherMix::default());
        assert_eq!(sample.dominant(), WeatherKind::Clear);
    }

    #[test]
    fn the_desert_spawns_no_thunderstorm() {
        let mut cells = WeatherCells::new(5);
        assert!((0..1000).all(|_| cells.pick(0) != WeatherKind::Thunderstorm));
        assert!((0..1000).any(|_| cells.pick(2) == WeatherKind::Thunderstorm));
    }

    #[test]
    fn lock_gives_the_full_kind_at_the_player() {
        let mut cells = run(3, 2, 100);
        cells.lock(Some(WeatherKind::Thunderstorm), [0.; 2]);
        let player = [5000., -8000.];
        for _ in 0..1000 {
            cells.advance(10., [3., 1.], player, false, |_| 2);
        }
        let sample = cells.sample(player);
        assert_eq!(sample.dominant(), WeatherKind::Thunderstorm);
        assert_eq!(sample.mix(), WeatherKind::Thunderstorm.target());
        assert_eq!(cells.len(), 1);
        cells.lock(None, player);
        assert!(cells.is_empty() && !cells.locked());
    }

    #[test]
    fn add_replaces_the_weakest_cell_when_full() {
        let mut cells = WeatherCells::new(1);
        for i in 0..MAX_CELLS {
            cells.add(WeatherKind::Rain, [i as f64 * 100., 0.], ADD_RADIUS);
        }
        let mut list: Vec<_> = cells.iter().copied().collect();
        list[3].age = list[3].life;
        cells.set(list, false);
        cells.add(WeatherKind::Fog, [0.; 2], ADD_RADIUS);
        assert_eq!(cells.len(), MAX_CELLS);
        let fog: Vec<_> = cells
            .iter()
            .filter(|c| c.kind == WeatherKind::Fog)
            .collect();
        assert_eq!(fog.len(), 1);
        assert_eq!(fog[0].strength(), 1.);
        assert!(cells.iter().all(|c| c.strength() == 1.));
    }

    #[test]
    fn each_full_kind_looks_distinct() {
        let looks = WeatherKind::ALL.map(|kind| {
            let mut sample = CellSample::default();
            sample.weights[kind as usize] = 1.;
            sample.look()
        });
        for (i, a) in looks.iter().enumerate() {
            for b in &looks[i + 1..] {
                let apart = |x: f32, y: f32| (x - y).abs() > 0.05;
                assert!(
                    apart(a.sun, b.sun)
                        || apart(a.fog_density * 1000., b.fog_density * 1000.)
                        || apart(a.saturation, b.saturation),
                    "{a:?} {b:?}"
                );
            }
        }
    }
}
