//! Port choice: drawn terrain tiles of the finer tiers, so a distant tier skips their pixels.
//! A tile that is not drawn yet shows the coarser tier a few metres below its true height
//! (`terrain_tile`), not the deep source sink. A tile hole shows nothing under it.
use crate::world::{RendererFrame, WorldLayer};
#[cfg(target_arch = "wasm32")]
use bevy::render::{
    self as bevy_render,
    extract_resource::ExtractResource,
    render_resource::{ShaderType, encase},
};
use bevy::{ecs as bevy_ecs, prelude::*};

/// Cells per grid side. The grid is centered on the player.
pub const SIDE: i64 = 64;
const WORDS: usize = (SIDE * SIDE) as usize / 128;

/// One finer tier. `base` is the local x and z of cell 0, the tile size, and 1 when valid.
#[derive(Clone, Copy, Debug, PartialEq)]
#[cfg_attr(target_arch = "wasm32", derive(ShaderType))]
pub struct CoverageGrid {
    pub base: Vec4,
    pub bits: [UVec4; WORDS],
}
impl Default for CoverageGrid {
    fn default() -> Self {
        Self {
            base: Vec4::ZERO,
            bits: [UVec4::ZERO; WORDS],
        }
    }
}
/// Grid 0 holds tier 0 tiles for tier 1. Grid 1 holds tier 1 tiles for tier 2.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
#[cfg_attr(target_arch = "wasm32", derive(ShaderType))]
pub struct TerrainCoverageUniform {
    pub grids: [CoverageGrid; 2],
}

#[derive(Resource, Clone, Default, PartialEq)]
#[cfg_attr(target_arch = "wasm32", derive(ExtractResource))]
pub struct TerrainCoverage(pub TerrainCoverageUniform);

impl TerrainCoverageUniform {
    /// Grids of the published terrain tiles of `frame`. All of them have a drawn visual.
    pub fn of(frame: &RendererFrame<'_>) -> Self {
        let mut result = Self::default();
        for (grid, layer) in result
            .grids
            .iter_mut()
            .zip([WorldLayer::Terrain, WorldLayer::DistantTerrain])
        {
            let tiles = || frame.surfaces.iter().filter(move |s| s.key.layer == layer);
            // A tile origin is the unit times the tile size, with z negated.
            let Some(size) = tiles().find_map(|s| {
                let [x, z] = s.key.unit;
                if x != 0 {
                    Some(s.surface.origin[0] / x as f64)
                } else if z != 0 {
                    Some(-s.surface.origin[2] / z as f64)
                } else {
                    None
                }
            }) else {
                continue;
            };
            let player = frame.request.player;
            let origin = [
                (player[0] / size).round() as i64 - SIDE / 2,
                (-player[2] / size).round() as i64 - SIDE / 2,
            ];
            let base =
                frame
                    .origin
                    .local([origin[0] as f64 * size, 0., -(origin[1] as f64 * size)]);
            grid.base = Vec4::new(base[0], base[2], size as f32, 1.);
            for surface in tiles() {
                let [x, z] = [
                    surface.key.unit[0] - origin[0],
                    surface.key.unit[1] - origin[1],
                ];
                if !(0..SIDE).contains(&x) || !(0..SIDE).contains(&z) {
                    continue;
                }
                let index = (z * SIDE + x) as usize;
                grid.bits[index / 128][(index / 32) % 4] |= 1 << (index % 32);
            }
        }
        result
    }
}

#[cfg(target_arch = "wasm32")]
pub use gpu::{TerrainCoverageGpu, TerrainCoveragePlugin};
#[cfg(target_arch = "wasm32")]
mod gpu {
    use super::*;
    use bevy::render::{
        Render, RenderApp, RenderStartup, RenderSystems,
        extract_resource::ExtractResourcePlugin,
        render_resource::*,
        renderer::{RenderDevice, RenderQueue},
    };

    /// The uniform buffer the terrain materials bind. It is never replaced, so bind groups stay valid.
    #[derive(Resource)]
    pub struct TerrainCoverageGpu(pub Buffer);

    fn init(mut commands: Commands, device: Res<RenderDevice>) {
        commands.insert_resource(TerrainCoverageGpu(device.create_buffer(
            &BufferDescriptor {
                label: Some("source terrain coverage"),
                size: TerrainCoverageUniform::min_size().get(),
                usage: BufferUsages::UNIFORM | BufferUsages::COPY_DST,
                mapped_at_creation: false,
            },
        )));
    }

    fn write(
        coverage: Res<TerrainCoverage>,
        gpu: Res<TerrainCoverageGpu>,
        queue: Res<RenderQueue>,
    ) {
        if !coverage.is_changed() {
            return;
        }
        let mut bytes = encase::UniformBuffer::new(Vec::new());
        bytes.write(&coverage.0).expect("terrain coverage uniform");
        queue.write_buffer(&gpu.0, 0, bytes.as_ref());
    }

    pub struct TerrainCoveragePlugin;
    impl Plugin for TerrainCoveragePlugin {
        fn build(&self, app: &mut App) {
            app.init_resource::<TerrainCoverage>()
                .add_plugins(ExtractResourcePlugin::<TerrainCoverage>::default());
            app.sub_app_mut(RenderApp)
                .init_resource::<TerrainCoverage>()
                .add_systems(RenderStartup, init)
                .add_systems(Render, write.in_set(RenderSystems::PrepareResources));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::world::{
        RendererSurface, SurfaceKey, WorldBounds, WorldOrigin, WorldRequest, WorldSurface,
    };
    use std::sync::Arc;

    fn tile(layer: WorldLayer, unit: [i64; 2], size: f64) -> RendererSurface {
        RendererSurface {
            key: SurfaceKey { layer, unit },
            surface: Arc::new(WorldSurface {
                origin: [unit[0] as f64 * size, 0., -(unit[1] as f64 * size)],
                ..default()
            }),
            prototype: None,
            bounds: WorldBounds {
                min: [0.; 3],
                max: [0.; 3],
            },
        }
    }

    /// The shader cell of a local point, as `covered` in source_terrain.wgsl computes it.
    fn covered(grid: &CoverageGrid, x: f32, z: f32) -> bool {
        let cell = [
            ((x - grid.base.x) / grid.base.z).round() as i64,
            ((grid.base.y - z) / grid.base.z).round() as i64,
        ];
        if grid.base.w == 0. || !(0..SIDE).contains(&cell[0]) || !(0..SIDE).contains(&cell[1]) {
            return false;
        }
        let index = (cell[1] * SIDE + cell[0]) as usize;
        grid.bits[index / 128][(index / 32) % 4] >> (index % 32) & 1 != 0
    }

    #[test]
    fn drawn_tiles_cover_their_cells_in_local_space() {
        let surfaces = [
            tile(WorldLayer::Terrain, [3, -2], 100.),
            tile(WorldLayer::Terrain, [4, -2], 100.),
            tile(WorldLayer::DistantTerrain, [1, 0], 1111.),
        ];
        let frame = RendererFrame {
            request: WorldRequest {
                player: [320., 0., 180.],
                ..default()
            },
            origin: WorldOrigin {
                offset: [250., 0., -150.],
            },
            surfaces: &surfaces,
            surface_revision: 0,
            lights: &[],
            structures: &[],
            supplies: &[],
            startup: None,
            failure: None,
        };
        let coverage = TerrainCoverageUniform::of(&frame);
        let near = &coverage.grids[0];
        // Tile [3, -2] spans source x 250..350 and z 150..250, local x 0..100 and z 300..400.
        assert!(covered(near, 10., 310.));
        assert!(covered(near, 140., 390.));
        assert!(!covered(near, 10., 210.));
        assert!(!covered(near, 460., 310.));
        assert_eq!(coverage.grids[1].base.z, 1111.);
        assert!(covered(&coverage.grids[1], 1111. - 250., 150.));
    }
}
