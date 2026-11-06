//! Source terrain inputs from the reference, and the tile worker client.
mod input;
mod jobs;

pub(in crate::world) use input::TileInput;
pub(in crate::world) use jobs::{CompletedTerrain, TerrainExecution, TerrainMetadata};

use super::WorldSurface;
use crate::reference::{ReferenceError, WorldReference, WorldValue};
use terrain::{TerrainEvaluator, TerrainSettings, TerrainTile, surface_origin};

pub fn terrain_tile(
    reference: &WorldReference,
    distances: &super::DrawDistances,
    tier: usize,
) -> Result<TerrainTile, ReferenceError> {
    let mut tile = grid_tile(reference, distances, tier)?;
    if tier != 0 {
        let finer = grid_error(grid_tile(reference, distances, 1)?);
        let sink = ROAD_CUT + finer;
        // Tier 2 also sinks under the lowest point of tier 1: its sink and error.
        tile.y -= if tier == 2 {
            sink + finer + grid_error(tile)
        } else {
            sink
        };
    }
    Ok(tile)
}

/// The tile without the port choice sink.
fn grid_tile(
    reference: &WorldReference,
    distances: &super::DrawDistances,
    tier: usize,
) -> Result<TerrainTile, ReferenceError> {
    let tile = reference
        .terrain
        .tiles
        .get(tier)
        .ok_or_else(|| ReferenceError(format!("Terrain tier {tier}")))?;
    let settings = reference
        .records
        .iter()
        .find(|r| r.source == reference.terrain.source)
        .ok_or_else(|| ReferenceError("Missing TerrainGenerationSettings".into()))?;
    let number = |field: &str| -> Result<f32, ReferenceError> {
        match settings.fields.get(field) {
            Some(WorldValue::Number(v)) => Ok(*v as f32),
            Some(WorldValue::Integer(v)) => Ok(*v as f32),
            _ => Err(ReferenceError(format!("TerrainGenerationSettings.{field}"))),
        }
    };
    let d = distances;
    let (scale, draw) = match tier {
        0 => (1.0, d.terrain_gen_dist),
        1 => (
            ((d.draw_distance as f64 / 3.0 / 3.0 * 4.0) / number("defDistantTerrainSize")? as f64)
                as f32,
            d.distant_terrain_gen_dist,
        ),
        2 => (
            ((d.cam2_far_plane as f64 / 3.0 / 3.0 * 4.0) / number("defDistantTerrain2Size")? as f64)
                as f32,
            d.distant_terrain2_gen_dist,
        ),
        _ => return Err(ReferenceError(format!("Terrain tier {tier}"))),
    };
    let size = (tile.tile_size as f64 * scale as f64) as u32;
    if size == 0 || !scale.is_finite() || scale <= 0.0 {
        return Err(ReferenceError(format!("Terrain tier {tier} scale")));
    }
    let resolution = ((tile.resolution as f64 * scale as f64) as u32).max(4);
    let mut result = TerrainTile {
        tier,
        size,
        resolution,
        draw_distance: ((draw as f64 / size as f64) as u32).max(1),
        y: tile.y_coord,
        material_scale: scale,
    };
    if tier != 0 {
        // Port choice: finer distant tiers. See `DISTANT_DETAIL`.
        result.resolution = (resolution * DISTANT_DETAIL).min(DISTANT_RESOLUTION);
    }
    Ok(result)
}

/// Port choice: distant tiers have this many times the source vertices, up to
/// `DISTANT_RESOLUTION` per side. The source spacing of about 350 m puts a tier up to 21 m
/// above the true ground. It then covers the road and the near tiles past the near ring.
/// At the 2500 m default the spacing is 36 m for tier 1 and 249 m for tier 2.
const DISTANT_DETAIL: u32 = 8;
const DISTANT_RESOLUTION: u32 = 64;
/// Largest height of the terrain above a road point, plus the 0.1 m road cut of a near tile.
/// The road averages the ground over 40 m and turns at most 6 degrees per bone.
const ROAD_CUT: f32 = 5.;

/// Port choice: upper bound of the height error of the grid of `tile`, fit over the source
/// noise with a margin. Measured maxima: 1.7 m at 44 m spacing, 32 m at 453 m. A distant tier
/// sinks by it, so it stays under the finer tier and the roads. It replaces the source
/// `_NearDist`/`_FarDist`/`_Amount` sink near the camera, which showed pits where a near tile was
/// not loaded.
fn grid_error(tile: TerrainTile) -> f32 {
    let spacing = tile.size as f32 / (tile.resolution - 1) as f32;
    spacing / 20. + spacing * spacing / 10000.
}

pub fn terrain_settings(reference: &WorldReference, seed: i32) -> TerrainSettings {
    let terrain = &reference.terrain;
    TerrainSettings {
        seed: if terrain.random_seed != 0 {
            seed
        } else {
            terrain.seed
        },
        edge: terrain.edge_of_world,
        edge_gradient: terrain.edge_of_world_gradient,
    }
}

pub fn terrain_evaluator(reference: &WorldReference, seed: i32) -> TerrainEvaluator {
    TerrainEvaluator::new(terrain_settings(reference, seed))
}

/// Main-thread bytes for one tile result. Border work stays in the worker.
pub fn terrain_output_bound(tile: TerrainTile) -> usize {
    let n = tile.resolution as usize;
    n * n * 64 + (n - 1) * (n - 1) * 24 + std::mem::size_of::<std::ops::Range<usize>>()
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum TerrainPoll {
    Idle,
    Busy,
    Ready(u32),
}

/// One tile job at a time. The browser runs it in a Web Worker with its own WASM.
/// Native resumes it during polling, through the same byte boundary.
#[derive(Default)]
struct TerrainWorker {
    #[cfg(not(target_arch = "wasm32"))]
    next: u32,
    #[cfg(not(target_arch = "wasm32"))]
    result: Option<(u32, Result<terrain::TerrainTask, String>)>,
}

#[cfg(not(target_arch = "wasm32"))]
impl TerrainWorker {
    /// Call only when `poll` returns `Idle`.
    fn submit(&mut self, input: &[u8]) -> u32 {
        self.next = self.next.wrapping_add(1);
        self.result = Some((self.next, terrain::TerrainTask::new(input)));
        self.next
    }
    fn poll(&mut self, should_yield: impl FnMut() -> bool) -> Result<TerrainPoll, ReferenceError> {
        match &mut self.result {
            None => Ok(TerrainPoll::Idle),
            Some((_, Err(error))) => Err(ReferenceError(format!("Terrain worker: {error}"))),
            Some((id, Ok(task))) => Ok(if task.advance(should_yield) {
                TerrainPoll::Ready(*id)
            } else {
                TerrainPoll::Busy
            }),
        }
    }
    /// Move the ready result into an empty `surface`.
    fn read(&mut self, unit: [i64; 2], tile: TerrainTile, surface: &mut WorldSurface) {
        let Some((_, Ok(mesh))) = self.result.take() else {
            unreachable!("Terrain worker has no result");
        };
        let mesh = mesh.into_mesh();
        surface.origin = surface_origin(unit, tile);
        surface.vertices = mesh.vertices;
        surface.normals = mesh.normals;
        surface.tangents = mesh.tangents;
        surface.uv = mesh.uv;
        surface.colors = mesh.colors;
        surface.triangles = mesh.triangles;
        surface.submeshes.push(0..surface.triangles.len());
    }
    fn discard(&mut self) {
        self.result = None;
    }
}

#[cfg(target_arch = "wasm32")]
impl TerrainWorker {
    /// Call only when `poll` returns `Idle`.
    fn submit(&mut self, input: &[u8]) -> u32 {
        crate::host::game_terrain_submit(input)
    }
    fn poll(&mut self, _should_yield: impl FnMut() -> bool) -> Result<TerrainPoll, ReferenceError> {
        match crate::host::game_terrain_poll() {
            Ok(-1) => Ok(TerrainPoll::Idle),
            Ok(-2) => Ok(TerrainPoll::Busy),
            Ok(id) => Ok(TerrainPoll::Ready(id as u32)),
            Err(error) => {
                use wasm_bindgen::JsCast;
                let message = error
                    .dyn_ref::<js_sys::Error>()
                    .map_or_else(|| format!("{error:?}"), |e| String::from(e.message()));
                Err(ReferenceError(format!("Terrain worker: {message}")))
            }
        }
    }
    /// Copy the ready result once, from the worker buffers into an empty `surface`.
    fn read(&mut self, unit: [i64; 2], tile: TerrainTile, surface: &mut WorldSurface) {
        let vertices = crate::host::game_terrain_vertex_count() as usize;
        let triangles = crate::host::game_terrain_triangle_count() as usize;
        let profile =
            crate::cpu_profile::Timer::new(crate::cpu_profile::Stage::TerrainResultAllocation);
        surface.origin = surface_origin(unit, tile);
        surface.vertices = vec![[0.0; 3]; vertices];
        surface.normals = vec![[0.0; 3]; vertices];
        surface.tangents = vec![[0.0; 4]; vertices];
        surface.uv = vec![[0.0; 2]; vertices];
        surface.colors = vec![[0.0; 4]; vertices];
        surface.triangles = vec![[0; 3]; triangles];
        drop(profile);
        crate::host::game_terrain_read(
            surface.vertices.as_flattened_mut(),
            surface.normals.as_flattened_mut(),
            surface.tangents.as_flattened_mut(),
            surface.uv.as_flattened_mut(),
            surface.colors.as_flattened_mut(),
            surface.triangles.as_flattened_mut(),
        );
        surface.submeshes.push(0..triangles);
    }
    fn discard(&mut self) {
        crate::host::game_terrain_discard();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The sink holds the triangle grid of a distant tile under the source noise.
    #[test]
    fn grid_error_bounds_the_source_noise() {
        for (size, resolution) in [(1111, 26), (7708, 32), (11111, 64)] {
            let tile = TerrainTile {
                tier: 1,
                size,
                resolution,
                draw_distance: 1,
                y: 0.,
                material_scale: 1.,
            };
            let h = size as f32 / (resolution - 1) as f32;
            let bound = grid_error(tile);
            for seed in [1, -1705217151, 7777] {
                let e = TerrainEvaluator::new(TerrainSettings {
                    seed,
                    edge: 2.0e7,
                    edge_gradient: 20000.,
                });
                for cell in 0..400 {
                    let [x, z] = [cell % 20, cell / 20].map(|i| (i as f32 - 10.) * 997. * h);
                    let c = [[0., 0.], [h, 0.], [0., h], [h, h]]
                        .map(|[u, v]| e.base_height([x + u, z + v], true));
                    for i in 0..=8 {
                        for j in 0..=8 - i {
                            let [u, v] = [i as f32 / 8., j as f32 / 8.];
                            let grid = c[0] + (c[1] - c[0]) * u + (c[2] - c[0]) * v;
                            let truth = e.base_height([x + u * h, z + v * h], true);
                            assert!((grid - truth).abs() <= bound, "{h} m: {grid} vs {truth}");
                        }
                    }
                }
            }
        }
    }
}
