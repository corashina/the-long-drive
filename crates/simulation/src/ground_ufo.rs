//! Source `fedospawnscript` on the Player and the `fedoscript` AI of the UFO that it spawns.
//! The UFO follows the road above the terrain and chases the player. Not saved.
//! Positions are absolute port coordinates. Source +Z is port -Z.
use crate::lighting::CyclePhase;
use crate::reference::GroundUfoReference;
use crate::sky_traffic::Plane;
use crate::weather::StormRandom;
use rapier3d::math::{Rotation, Vector};

/// Ground UFO plane ids start here, above the sky plane ids.
const ID_BASE: u32 = 1 << 31;
/// Source `OneCoord(transform.position + transform.forward * 50f)`.
const LOOK_AHEAD: f64 = 50.;

#[derive(Debug)]
pub struct Ufo {
    pub id: u32,
    pub position: [f64; 3],
    /// Source `targetObj`.
    target: [f64; 3],
    current_speed: f32,
    visited: bool,
    last_player: [f64; 3],
}

impl Ufo {
    /// Source `fedoscript.Go` and `AiInput`, then the rigidbody move. Returns false when the
    /// source destroys the UFO.
    fn step(
        &mut self,
        r: &GroundUfoReference,
        player: [f64; 3],
        dt: f32,
        road: &impl Fn([f64; 3], f32) -> [f64; 3],
    ) -> bool {
        // AiInput.
        let dist = distance(self.position, player) as f32;
        let visited_distance = r.visited_distance + r.road_height_offset;
        if dist < visited_distance {
            self.visited = true;
        }
        let target_speed = distance(player, self.last_player) as f32 / dt;
        let multiplier = lerp(
            r.ai_multiplier_range[0],
            r.ai_multiplier_range[1],
            inverse_lerp(r.ai_distance_range[0], r.ai_distance_range[1], dist),
        );
        if self.visited {
            if dist > visited_distance {
                self.current_speed = self.current_speed.max(target_speed * multiplier);
            } else {
                self.current_speed = target_speed * multiplier;
            }
            if dist > r.disappear_distance {
                return false;
            }
        } else {
            self.current_speed = target_speed.clamp(r.speed, r.high_speed) * multiplier;
        }
        self.last_player = player;
        // followRoad. The rotation stays identity: the rigidbody freezes it.
        let mut ahead = self.position;
        ahead[2] -= LOOK_AHEAD;
        self.target = road(ahead, r.road_height_offset);
        // Go.
        self.current_speed = self.current_speed.clamp(r.speed, r.high_speed);
        let delta: [f64; 3] = std::array::from_fn(|i| self.target[i] - self.position[i]);
        let length = delta.iter().map(|v| v * v).sum::<f64>().sqrt();
        let reach = f64::from(r.target_distance);
        if length >= reach && length > 0. {
            let step = f64::from(self.current_speed * dt);
            for (p, d) in self.position.iter_mut().zip(delta) {
                *p += d / length * step;
            }
        }
        true
    }
}

pub struct GroundUfos {
    random: StormRandom,
    current_night: i32,
    selected_night: i32,
    added: bool,
    next_id: u32,
    ufos: Vec<Ufo>,
    /// The UFOs as sky planes, relative to the sky anchor.
    planes: Vec<Plane>,
}

impl GroundUfos {
    /// Source `Start`: `selectedNight = Random.Range(minNight, maxNight + 1)`.
    pub fn new(r: &GroundUfoReference, seed: u64) -> Self {
        let mut random = StormRandom(seed ^ 0x6675_666f_0000_0001);
        let selected_night = night(&mut random, r);
        Self {
            random,
            current_night: 0,
            selected_night,
            added: false,
            next_id: ID_BASE,
            ufos: Vec::new(),
            planes: Vec::new(),
        }
    }
    pub fn ufos(&self) -> &[Ufo] {
        &self.ufos
    }
    pub fn planes(&self) -> &[Plane] {
        &self.planes
    }

    /// Source `fedospawnscript.Update` and each `fedoscript.Update`. `spawns` is false on a
    /// guest: the source spawns only on the server. `road` gives the road point under an
    /// absolute position at a height above the base terrain.
    pub fn advance(
        &mut self,
        r: &GroundUfoReference,
        phase: CyclePhase,
        player: [f64; 3],
        spawns: bool,
        dt: f32,
        road: impl Fn([f64; 3], f32) -> [f64; 3],
    ) {
        // A new UFO first moves in the next frame.
        self.ufos.retain_mut(|ufo| ufo.step(r, player, dt, &road));
        match phase {
            CyclePhase::Dawn => self.added = false,
            CyclePhase::Night if !self.added => {
                self.added = true;
                self.current_night += 1;
                if self.current_night >= self.selected_night {
                    self.selected_night = self.current_night + night(&mut self.random, r);
                    if self.random.unit() < r.chance {
                        if spawns {
                            self.spawn(r, player);
                        }
                        self.selected_night += r.nights_after_spawn;
                    }
                }
            }
            _ => {}
        }
    }

    /// Refreshes `planes` for the sky anchor after `advance`.
    pub fn place(&mut self, r: &GroundUfoReference, anchor: [f64; 3]) {
        self.planes.clear();
        self.planes.extend(self.ufos.iter().map(|ufo| Plane {
            id: ufo.id,
            prefab: r.prefab,
            offset: Vector::from_array(std::array::from_fn(|i| {
                (ufo.position[i] - anchor[i]) as f32
            })),
            heading: Rotation::IDENTITY,
            contrail: None,
        }));
    }

    /// Source `DoSpawn` and `fedoscript.Spawned`.
    fn spawn(&mut self, r: &GroundUfoReference, player: [f64; 3]) {
        let side = self.random.range(-r.side_distance, r.side_distance);
        let back = self.random.range(r.back_range[0], r.back_range[1]);
        // Source -Z back is port +Z.
        let position = [
            player[0] + f64::from(side),
            player[1] + f64::from(r.up_distance),
            player[2] + f64::from(back),
        ];
        self.ufos.push(Ufo {
            id: self.next_id,
            position,
            target: position,
            current_speed: 0.,
            visited: false,
            last_player: player,
        });
        self.next_id = self.next_id.wrapping_add(1).max(ID_BASE);
    }
}

/// `Random.Range(minNight, maxNight + 1)`.
fn night(random: &mut StormRandom, r: &GroundUfoReference) -> i32 {
    let count = r.night_range[1] + 1 - r.night_range[0];
    r.night_range[0] + ((random.unit() * count as f32) as i32).min(count - 1)
}

fn distance(a: [f64; 3], b: [f64; 3]) -> f64 {
    (0..3).map(|i| (a[i] - b[i]).powi(2)).sum::<f64>().sqrt()
}

/// Unity `Mathf.InverseLerp`.
fn inverse_lerp(a: f32, b: f32, value: f32) -> f32 {
    if a == b {
        0.
    } else {
        ((value - a) / (b - a)).clamp(0., 1.)
    }
}

fn lerp(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t.clamp(0., 1.)
}
