//! Stream count limits from the draw distances of one world session.
use super::{DrawDistances, TerrainTile};
use crate::reference::{ReferenceError, WorldReference};
use std::f64::consts::{FRAC_1_SQRT_2, PI, SQRT_2};

/// Limits tested at the 500 m minimum before the draw distance setting.
const TESTED: StreamLimits = StreamLimits {
    roads: 512,
    tiles: [512, 256, 256],
    objects: 32768,
};
/// Road path length per metre of source z. The yaw clamp of 90 degrees each way keeps source z
/// monotonic, so the road crosses a selection window once. The yaw follows the low-frequency
/// source curve, and the square root of 2 allows a mean heading of 45 degrees inside the
/// window. The export test measures it.
const ROAD_BEND: f64 = SQRT_2;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StreamLimits {
    pub roads: usize,
    /// Per terrain tier.
    pub tiles: [usize; 3],
    pub objects: usize,
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
    objects: f64,
}
impl Bounds {
    fn new(
        world: &WorldReference,
        inputs: &Inputs,
        d: &DrawDistances,
    ) -> Result<Self, ReferenceError> {
        let mut tiles = [0.; 3];
        for (tier, count) in tiles.iter_mut().enumerate() {
            *count = 2. * circle_tiles(super::terrain_tile(world, d, tier)?) as f64;
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
    fn scaled(bounds: Bounds, minimum: Bounds) -> Self {
        let headroom =
            |tested: usize, bound: f64, low: f64| (bound * tested as f64 / low).max(bound).ceil();
        Self {
            roads: headroom(TESTED.roads, bounds.roads, minimum.roads) as usize,
            tiles: std::array::from_fn(|t| TESTED.tiles[t].max(bounds.tiles[t] as usize)),
            objects: headroom(TESTED.objects, bounds.objects, minimum.objects) as usize,
        }
    }
}
fn minimum(world: &WorldReference) -> DrawDistances {
    let r = &world.draw_distance;
    DrawDistances::resolve(r, r.minimum.draw_distance)
}
