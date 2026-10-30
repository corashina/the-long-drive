//! Source terrain inputs from the reference, and the tile worker client.
mod jobs;

pub(in crate::world) use jobs::{CompletedTerrain, TerrainExecution, TerrainMetadata};

use super::WorldSurface;
use crate::reference::{ReferenceError, WorldReference, WorldValue};
use terrain::{TerrainEvaluator, TerrainSettings, TerrainTile, surface_origin};

pub fn terrain_tile(
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
    Ok(TerrainTile {
        tier,
        size,
        resolution: ((tile.resolution as f64 * scale as f64) as u32).max(4),
        draw_distance: ((draw as f64 / size as f64) as u32).max(1),
        y: tile.y_coord,
        material_scale: scale,
    })
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
#[path = "terrain_tests.rs"]
mod tests;
