//! Source thumbleweedMain spawner and thumbleweedScript gusts on the fixed step.
use super::StormRandom;
use crate::reference::{ItemId, TumbleweedReference};
use rapier3d::math::Vector;

/// One source importantObjs entry in physics space.
#[derive(Clone, Copy, Debug)]
pub struct ImportantObject {
    pub position: Vector,
    /// Source transform.forward.
    pub forward: Vector,
}

#[derive(Clone, Copy, Debug)]
struct Weed {
    item: ItemId,
    next_wind_time: f64,
    wind_live_time: f32,
    wind: Vector,
    /// The force of the last `gust` step. Zero when the weed did not push.
    force: Vector,
}

pub struct Tumbleweeds {
    weeds: Vec<Weed>,
    /// Source spawnTime.
    spawn_time: f64,
    random: StormRandom,
}

impl Tumbleweeds {
    pub fn new(seed: u64) -> Self {
        Self {
            weeds: Vec::new(),
            // The source spawnTime starts at 0, so the first fixed step tries a spawn.
            spawn_time: f64::NEG_INFINITY,
            random: StormRandom(seed ^ 0x7477_6565_6473_0001),
        }
    }
    pub fn len(&self) -> usize {
        self.weeds.len()
    }
    pub fn is_empty(&self) -> bool {
        self.weeds.is_empty()
    }

    /// Source thumbleweedMain.FixedUpdate. Returns the spot for a new weed.
    /// The caller spawns the prefab there and calls `add`.
    /// `live` tells if a weed item still exists. `cast` is a ray query:
    /// origin, unit direction, length and layer mask give the hit distance.
    pub fn spawn(
        &mut self,
        r: &TumbleweedReference,
        now: f64,
        important: &[ImportantObject],
        live: impl Fn(ItemId) -> bool,
        mut cast: impl FnMut(Vector, Vector, f32, u32) -> Option<f32>,
    ) -> Option<Vector> {
        if now <= self.spawn_time {
            return None;
        }
        let spot = self.spot(r, important, live, &mut cast);
        let range = if self.random.unit() < r.long_spawn_chance {
            r.long_spawn_time_range
        } else {
            r.short_spawn_time_range
        };
        self.spawn_time = now + self.random.range(range[0], range[1]) as f64;
        spot
    }

    /// Source Spawn(false) up to Instantiate.
    fn spot(
        &mut self,
        r: &TumbleweedReference,
        important: &[ImportantObject],
        live: impl Fn(ItemId) -> bool,
        cast: &mut impl FnMut(Vector, Vector, f32, u32) -> Option<f32>,
    ) -> Option<Vector> {
        self.weeds.retain(|weed| live(weed.item));
        if self.weeds.len() >= r.limit as usize || important.is_empty() {
            return None;
        }
        let index =
            ((self.random.unit() * important.len() as f32) as usize).min(important.len() - 1);
        let object = important[index];
        let mut offset = self.random.horizontal()
            * self
                .random
                .range(r.spawn_dist_range[0], r.spawn_dist_range[1]);
        if object.forward.angle_between(offset).to_degrees() < r.fov {
            offset = offset.normalize_or_zero()
                * self
                    .random
                    .range(r.spawn_dist_in_fov_range[0], r.spawn_dist_in_fov_range[1]);
        }
        let mut spot = object.position + offset;
        let top = spot + Vector::Y * r.ray_height;
        let distance = cast(top, -Vector::Y, f32::INFINITY, r.spawn_ray_lm)?;
        spot.y = top.y - distance + r.spawn_height;
        (!walls(r, spot, cast)).then_some(spot)
    }

    /// Source Spawn after Instantiate: the weed joins the list with bwind set.
    pub fn add(&mut self, item: ItemId) {
        self.weeds.push(Weed {
            item,
            next_wind_time: 0.,
            wind_live_time: 0.,
            wind: Vector::ZERO,
            force: Vector::ZERO,
        });
    }

    /// Source thumbleweedScript.FixedUpdate. `position` gives the body position of an active weed.
    pub fn gust(
        &mut self,
        r: &TumbleweedReference,
        now: f64,
        dt: f32,
        position: impl Fn(ItemId) -> Option<Vector>,
        mut cast: impl FnMut(Vector, Vector, f32, u32) -> Option<f32>,
    ) {
        for weed in &mut self.weeds {
            weed.force = Vector::ZERO;
            let Some(at) = position(weed.item) else {
                continue;
            };
            if now > weed.next_wind_time {
                weed.next_wind_time = now
                    + self
                        .random
                        .range(r.wind_time_range[0], r.wind_time_range[1])
                        as f64;
                if !walls(r, at, &mut cast) {
                    weed.wind_live_time = self
                        .random
                        .range(r.wind_live_time_range[0], r.wind_live_time_range[1]);
                    weed.wind = self.random.horizontal()
                        * self
                            .random
                            .range(r.wind_force_range[0], r.wind_force_range[1]);
                }
            }
            if weed.wind_live_time > 0. {
                weed.force = weed.wind;
            }
            weed.wind_live_time -= dt;
        }
    }

    /// Source AddForce values of the last `gust` step, in ForceMode.Force units.
    pub fn forces(&self) -> impl Iterator<Item = (ItemId, Vector)> + '_ {
        self.weeds
            .iter()
            .filter(|weed| weed.force != Vector::ZERO)
            .map(|weed| (weed.item, weed.force))
    }
}

/// Source CheckWalls: `ray_count_to_stay` or more ring rays hit.
fn walls(
    r: &TumbleweedReference,
    at: Vector,
    cast: &mut impl FnMut(Vector, Vector, f32, u32) -> Option<f32>,
) -> bool {
    let hits = (0..r.ray_count)
        .filter(|&i| {
            let angle = (i as f32 / r.ray_count as f32 * r.ring_degrees).to_radians();
            // Quaternion.Euler(0, angle, 0) * forward. Source +Z is port -Z.
            let direction = Vector::new(angle.sin(), 0., -angle.cos());
            cast(at, direction, r.ray_length, r.ray_lm).is_some()
        })
        .count();
    hits >= r.ray_count_to_stay as usize
}

impl StormRandom {
    /// Random.onUnitSphere with y = 0, normalized.
    fn horizontal(&mut self) -> Vector {
        let angle = self.range(0., std::f32::consts::TAU);
        Vector::new(angle.cos(), 0., angle.sin())
    }
}
