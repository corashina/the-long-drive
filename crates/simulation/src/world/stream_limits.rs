//! Stream count and byte limits from the draw distances of one world session.
use super::stream::{CHECKPOINTS, PROTOTYPE_CPU, PROTOTYPE_GPU, STAGING};
use super::{DrawDistances, TerrainTile};
use crate::reference::{ReferenceError, WorldReference};
use std::f64::consts::{FRAC_1_SQRT_2, PI, SQRT_2};

const MIB: usize = 1024 * 1024;
/// Limits tested at the 500 m minimum before the draw distance setting.
const TESTED: StreamLimits = StreamLimits {
    roads: 512,
    tiles: [512, 256, 256],
    objects: 32768,
    active_cpu: 256 * MIB,
    active_gpu: 128 * MIB,
    terrain_job: 0,
};
/// WASM32 linear memory.
const ADDRESS_SPACE: u64 = 4 << 30;
/// Road path length per metre of source z. The yaw clamp of 90 degrees each way keeps source z
/// monotonic, so the road crosses a selection window once. The yaw follows the low-frequency
/// source curve, and the square root of 2 allows a mean heading of 45 degrees inside the
/// window. The export test measures it.
const ROAD_BEND: f64 = SQRT_2;
const STEP: f32 = 100.;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StreamLimits {
    pub roads: usize,
    /// Per terrain tier.
    pub tiles: [usize; 3],
    pub objects: usize,
    pub active_cpu: usize,
    pub active_gpu: usize,
    /// Output bytes of the largest terrain tile job.
    pub terrain_job: usize,
}

/// Distance-independent inputs.
struct Inputs {
    /// Road light margin plus segment bounds radius.
    road_reach: f64,
    segment_length: f64,
    building_spacing: f64,
    building_offset: f64,
    /// Chunk size and candidates per chunk of each prop layer.
    layers: [(f64, f64); 4],
}
impl Inputs {
    fn new(world: &WorldReference, road_margin: f64) -> Result<Self, ReferenceError> {
        let mut layers = [(0., 0.); 4];
        for (i, layer) in layers.iter_mut().enumerate() {
            let g = world
                .generators
                .get(i)
                .ok_or_else(|| ReferenceError(format!("Missing source generator {i}")))?;
            *layer = (
                g.chunk_dist as f64,
                super::props::candidate_side(g).pow(2) as f64,
            );
        }
        Ok(Self {
            road_reach: road_margin + super::road::bounds_radius(world)?,
            segment_length: world.road.road_segment_length as f64,
            building_spacing: super::placement::building_spacing(&world.buildings) as f64,
            building_offset: super::placement::building_offset(&world.buildings),
            layers,
        })
    }
}

/// Upper bounds of what the stream can request at one draw distance, for two focus points.
#[derive(Clone, Copy)]
struct Bounds {
    roads: f64,
    tiles: [f64; 3],
    /// Retained CPU and GPU bytes of all terrain tiles, without collision.
    terrain: [f64; 2],
    terrain_job: usize,
    objects: f64,
}
impl Bounds {
    fn new(
        world: &WorldReference,
        inputs: &Inputs,
        d: &DrawDistances,
    ) -> Result<Self, ReferenceError> {
        let mut tiles = [0.; 3];
        let mut terrain = [0.; 2];
        let mut terrain_job = 0;
        for (tier, count) in tiles.iter_mut().enumerate() {
            let tile = super::terrain_tile(world, d, tier)?;
            *count = 2. * circle_tiles(tile) as f64;
            // One submesh: `surface_bytes`, `surface_render_cpu` and `surface_render_bytes`.
            let n = tile.resolution as f64;
            let render = n * n * 96. + (n - 1.) * (n - 1.) * 24. + 8.;
            terrain[0] += *count * (super::terrain_output_bound(tile) as f64 + 2. * render + 4096.);
            terrain[1] += *count * render;
            terrain_job = terrain_job.max(super::terrain_output_bound(tile));
        }
        // Selection takes segments whose bounds meet the window around a focus point.
        let window = d.road_gen_dist as f64 + inputs.road_reach;
        let roads = 2. * ((2. * window * ROAD_BEND / inputs.segment_length).ceil() + 1.);
        // Building source z grows by at least the spacing. The lateral offset shifts it at most
        // by the offset.
        let buildings = 2.
            * ((2. * (d.building_gen_dist as f64 + inputs.building_offset)
                / inputs.building_spacing)
                .floor()
                + 1.);
        // A chunk stays until both focus points are at the removal distance. Removal and request
        // take one chunk per update, so at most one chunk exceeds the removal discs. Chunk units
        // are truncated to whole metres. Each candidate places at most one object.
        let mut props = 0.;
        for (i, &(size, candidates)) in inputs.layers.iter().enumerate() {
            let remove = super::prop_layers::layer_distances(d, i).1 as f64;
            let disc = (PI * ((remove + 1.) / size + FRAC_1_SQRT_2).powi(2)).ceil();
            props += ((2. * disc + 1.) * candidates).ceil() + 1.;
        }
        Ok(Self {
            roads,
            tiles,
            terrain,
            terrain_job,
            // One road light pole per selected road.
            objects: props + buildings + roads,
        })
    }
}

/// Tiles around one focus point, as the stream selects them.
pub(super) fn circle_tiles(tile: TerrainTile) -> usize {
    (((tile.draw_distance as f64 + 0.5).powi(2) * std::f32::consts::PI as f64) as usize).max(5)
}

impl StreamLimits {
    pub fn new(
        world: &WorldReference,
        distances: &DrawDistances,
        road_margin: f64,
    ) -> Result<Self, ReferenceError> {
        let inputs = Inputs::new(world, road_margin)?;
        let minimum = Bounds::new(world, &inputs, &minimum(world))?;
        Ok(Self::scaled(
            Bounds::new(world, &inputs, distances)?,
            minimum,
        ))
    }
    /// Tile bounds are exact, so keep the tested limit as a floor. Road and object bounds do not
    /// count content items, ramps and supplies. Keep the tested headroom over the 500 m bound.
    /// Byte budgets scale with the fastest growing count of objects, roads and near tiles
    /// (collision). Terrain mesh bytes add their exact growth. Distant tile resolution grows
    /// with the draw distance.
    fn scaled(bounds: Bounds, minimum: Bounds) -> Self {
        let headroom =
            |tested: usize, bound: f64, low: f64| (bound * tested as f64 / low).max(bound).ceil();
        let scale = (bounds.objects / minimum.objects)
            .max(bounds.roads / minimum.roads)
            .max(bounds.tiles[0] / minimum.tiles[0])
            .max(1.);
        Self {
            roads: headroom(TESTED.roads, bounds.roads, minimum.roads) as usize,
            tiles: std::array::from_fn(|t| TESTED.tiles[t].max(bounds.tiles[t] as usize)),
            objects: headroom(TESTED.objects, bounds.objects, minimum.objects) as usize,
            active_cpu: (TESTED.active_cpu as f64 * scale
                + (bounds.terrain[0] - minimum.terrain[0]).max(0.))
            .ceil() as usize,
            active_gpu: (TESTED.active_gpu as f64 * scale
                + (bounds.terrain[1] - minimum.terrain[1]).max(0.))
            .ceil() as usize,
            terrain_job: bounds.terrain_job,
        }
    }
    /// All budgets fit the WASM32 address space. Asset budgets hold the fixed reservations
    /// (`FixedAllocation::replace`). Worker memories are separate instances. A terrain job must
    /// fit staging next to the dependency reserve, which is below the staging budget.
    pub fn fits(&self) -> bool {
        self.terrain_job <= STAGING / 2
            && [
                self.active_cpu,
                self.active_gpu,
                PROTOTYPE_CPU,
                PROTOTYPE_GPU,
                STAGING,
                CHECKPOINTS,
            ]
            .into_iter()
            .fold(0u64, |sum, n| sum.saturating_add(n as u64))
                <= ADDRESS_SPACE
    }
    /// Largest draw distance on the setting step whose limits fit.
    pub fn largest_fitting(
        world: &WorldReference,
        road_margin: f64,
    ) -> Result<f32, ReferenceError> {
        let inputs = Inputs::new(world, road_margin)?;
        let r = &world.draw_distance;
        let low = Bounds::new(world, &inputs, &minimum(world))?;
        let mut value = r.maximum.draw_distance;
        while value > r.minimum.draw_distance {
            let bounds = Bounds::new(world, &inputs, &DrawDistances::resolve(r, value))?;
            if Self::scaled(bounds, low).fits() {
                return Ok(value);
            }
            value -= STEP;
        }
        Ok(r.minimum.draw_distance)
    }
}
fn minimum(world: &WorldReference) -> DrawDistances {
    let r = &world.draw_distance;
    DrawDistances::resolve(r, r.minimum.draw_distance)
}
