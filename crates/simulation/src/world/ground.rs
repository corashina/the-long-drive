//! Ground patches. A near tile loads over many frames. Until its collider publishes, small
//! heightfields from the terrain formula carry the player and the car, so the step never
//! waits for a tile job. A patch has the vertex heights of its tile.
use super::{COLLISION_REACH, SurfaceKey, WorldStream, window};
use crate::vehicle::{ColliderHandle, PhysicsWorld};
use crate::world::{TerrainContext, TerrainFoundation, TerrainRoadPoint, WorldBounds, WorldLayer};
use rapier3d::math::Vector;
use rapier3d::prelude::{Array2, ColliderBuilder, Group, InteractionGroups, InteractionTestMode};

/// Grid cells on one side of a patch.
const PATCH_CELLS: usize = 8;
/// A patch covers the collision window and this margin, so a car never outruns it in a frame.
const PATCH_MARGIN: f64 = 4.;
/// A patch outside every window that is this much larger is removed.
const PATCH_KEEP: f64 = 16.;

struct Patch {
    unit: [i64; 2],
    chunk: [usize; 2],
    context: u64,
    collider: ColliderHandle,
    bytes: usize,
}

#[derive(Default)]
pub(super) struct Ground {
    /// Road points of the last complete road discovery. Tile jobs use the same set.
    roads: Vec<TerrainRoadPoint>,
    /// Zero before the first complete road discovery. A change makes new patches.
    context: u64,
    /// True from a road plan change until its discovery completes.
    roads_stale: bool,
    patches: Vec<Patch>,
    holes: Vec<[[f32; 3]; 4]>,
    foundations: Vec<TerrainFoundation>,
    generated: Vec<super::super::FoundationPoint>,
    tile_roads: Vec<TerrainRoadPoint>,
}

impl Ground {
    pub(super) fn new() -> Self {
        Self {
            roads_stale: true,
            ..Self::default()
        }
    }
    /// The road plan changed. Patches keep the last complete road points until discovery ends.
    pub(super) fn roads_changed(&mut self) {
        self.roads_stale = true;
    }
    /// Road discovery is complete with `points`.
    pub(super) fn roads_discovered(&mut self, points: &[TerrainRoadPoint]) {
        if self.roads_stale {
            self.roads.clear();
            self.roads.extend_from_slice(points);
            self.context += 1;
            self.roads_stale = false;
        }
    }
    /// Roads or building foundations changed. Tile jobs restart, and patches too.
    pub(super) fn terrain_changed(&mut self) {
        if self.context != 0 {
            self.context += 1;
        }
    }
    pub(super) fn retained_bytes(&self) -> usize {
        self.roads.capacity() * std::mem::size_of::<TerrainRoadPoint>()
            + self.patches.capacity() * std::mem::size_of::<Patch>()
            + self.patches.iter().map(|p| p.bytes).sum::<usize>()
            + self.holes.capacity() * std::mem::size_of::<[[f32; 3]; 4]>()
            + self.foundations.capacity() * std::mem::size_of::<TerrainFoundation>()
            + self.generated.capacity() * std::mem::size_of::<super::super::FoundationPoint>()
            + self.tile_roads.capacity() * std::mem::size_of::<TerrainRoadPoint>()
    }
    fn covers(&self, unit: [i64; 2], chunk: [usize; 2]) -> bool {
        self.patches
            .iter()
            .any(|p| p.unit == unit && p.chunk == chunk)
    }
}

impl WorldStream {
    /// The windows that patches cover: the player and the car, with the margin.
    fn patch_windows(&self, margin: f64) -> [WorldBounds; 2] {
        let reach = COLLISION_REACH + margin;
        [
            window(self.request.player, reach),
            window(self.request.vehicle, reach),
        ]
    }
    /// Chunk range `[x0, z0, x1, z1]` of tile `unit` in `bounds`. Z is the source grid order.
    fn patch_chunks(&self, unit: [i64; 2], bounds: WorldBounds) -> Option<[usize; 4]> {
        let tile = self.tiles[0];
        let size = f64::from(tile.size);
        let cells = tile.resolution as usize - 1;
        let spacing = size / cells as f64;
        let chunks = cells.div_ceil(PATCH_CELLS);
        // Grid index along one axis from a local coordinate; source z is the negated world z.
        let index = |local: f64| (local + size * 0.5) / spacing / PATCH_CELLS as f64;
        let x = [bounds.min[0], bounds.max[0]].map(|v| index(v - unit[0] as f64 * size));
        let z = [-bounds.max[2], -bounds.min[2]].map(|v| index(v - unit[1] as f64 * size));
        if x[1] < 0. || z[1] < 0. || x[0] >= chunks as f64 || z[0] >= chunks as f64 {
            return None;
        }
        let clamp = |v: f64| (v.max(0.) as usize).min(chunks - 1);
        Some([clamp(x[0]), clamp(z[0]), clamp(x[1]), clamp(z[1])])
    }
    /// True when patches cover the part of tile `unit` in `swept`.
    pub(super) fn patched(&self, unit: [i64; 2], swept: WorldBounds) -> bool {
        self.patch_chunks(unit, swept)
            .is_some_and(|[x0, z0, x1, z1]| {
                (x0..=x1).all(|x| (z0..=z1).all(|z| self.ground.covers(unit, [x, z])))
            })
    }
    pub(super) fn tile_collides(&self, unit: [i64; 2]) -> bool {
        let key = SurfaceKey {
            layer: WorldLayer::Terrain,
            unit,
        };
        self.surface(key).is_some_and(|s| s.collider.is_some())
    }
    /// Removes patches that a tile replaced, that the windows left, or that have an old
    /// terrain context. Then adds patches where the windows have no tile collider.
    pub fn lay_ground(&mut self, physics: &mut PhysicsWorld) {
        let context = self.ground.context;
        let keep = self.patch_windows(PATCH_KEEP);
        let mut patches = std::mem::take(&mut self.ground.patches);
        let count = patches.len();
        patches.retain(|p| {
            let keep = p.context == context
                && !self.tile_collides(p.unit)
                && keep.iter().any(|bounds| {
                    self.patch_chunks(p.unit, *bounds)
                        .is_some_and(|[x0, z0, x1, z1]| {
                            (x0..=x1).contains(&p.chunk[0]) && (z0..=z1).contains(&p.chunk[1])
                        })
                });
            if !keep {
                physics.remove_collider(p.collider);
            }
            keep
        });
        if patches.len() != count {
            self.support_changed();
        }
        self.ground.patches = patches;
        if self.failure.is_some() || !self.initial_generation_ready || context == 0 {
            return;
        }
        let mut added = false;
        for bounds in self.patch_windows(PATCH_MARGIN) {
            let Some([x0, z0, x1, z1]) = self.tile_units(bounds) else {
                continue;
            };
            for unit in (x0..=x1).flat_map(|x| (z0..=z1).map(move |z| [x, z])) {
                added |= self.patch_tile(physics, unit, bounds, context);
            }
        }
        if added {
            physics.refresh_queries();
        }
    }
    /// Adds the missing patches of tile `unit` in `bounds`. Returns true when it added one.
    fn patch_tile(
        &mut self,
        physics: &mut PhysicsWorld,
        unit: [i64; 2],
        bounds: WorldBounds,
        context: u64,
    ) -> bool {
        let Some([x0, z0, x1, z1]) = self.patch_chunks(unit, bounds) else {
            return false;
        };
        let missing = |stream: &Self| {
            (x0..=x1).any(|x| (z0..=z1).any(|z| !stream.ground.covers(unit, [x, z])))
        };
        if self.tile_collides(unit) || !missing(self) {
            return false;
        }
        let mut ground = std::mem::take(&mut self.ground);
        let result = self.tile_foundations(
            unit,
            &mut ground.holes,
            &mut ground.foundations,
            &mut ground.generated,
        );
        super::tile_roads(&ground.roads, unit, self.tiles[0], &mut ground.tile_roads);
        self.ground = ground;
        if let Err(error) = result {
            self.failure = Some(error.0);
            return false;
        }
        // A building hole moves tile vertices. That tile has no patch; the step waits for it.
        if !self.ground.holes.is_empty() {
            return false;
        }
        for x in x0..=x1 {
            for z in z0..=z1 {
                if !self.ground.covers(unit, [x, z]) {
                    let (collider, bytes) = self.build_patch(physics, unit, [x, z]);
                    self.ground.patches.push(Patch {
                        unit,
                        chunk: [x, z],
                        context,
                        collider,
                        bytes,
                    });
                }
            }
        }
        self.support_changed();
        true
    }
    /// A collider over one chunk of tile `unit`.
    fn build_patch(
        &self,
        physics: &mut PhysicsWorld,
        unit: [i64; 2],
        chunk: [usize; 2],
    ) -> (ColliderHandle, usize) {
        let context = TerrainContext {
            evaluator: &self.evaluator,
            tile: self.tiles[0],
            roads: &self.ground.tile_roads,
            road_radius_squared: None,
            foundations: &self.ground.foundations,
            holes: &[],
            biome_override: self.mode.biome_override(),
        };
        let (shape, center, bytes) = patch_shape(&context, unit, chunk);
        let layer = self.terrain_layer;
        let collider = shape
            .friction(0.6)
            .restitution(0.0)
            .translation(Vector::from_array(self.origin.local(center)))
            .collision_groups(InteractionGroups::new(
                Group::from_bits_truncate(1u32 << layer),
                Group::from_bits_truncate(self.reference.collision_rules.layers[layer as usize]),
                InteractionTestMode::And,
            ))
            .build();
        (physics.insert_collider(collider), bytes)
    }
}

/// The heightfield over `chunk` of tile `unit` on the tile grid, its absolute center and its
/// bytes. The tile job context gives the tile vertex heights.
fn patch_shape(
    context: &TerrainContext<'_>,
    unit: [i64; 2],
    chunk: [usize; 2],
) -> (ColliderBuilder, [f64; 3], usize) {
    let tile = context.tile;
    let last = tile.resolution as usize - 1;
    let x0 = chunk[0] * PATCH_CELLS;
    let z0 = chunk[1] * PATCH_CELLS;
    let x1 = (x0 + PATCH_CELLS).min(last);
    let z1 = (z0 + PATCH_CELLS).min(last);
    // Heightfield rows run from -z, stored column major. Source z is the negated world z.
    let rows = z1 - z0 + 1;
    let mut heights = Vec::with_capacity((x1 - x0 + 1) * rows);
    for x in x0..=x1 {
        for row in 0..rows {
            heights.push(context.grid_height(unit, x, z1 - row));
        }
    }
    let grid = |index: usize| f64::from(::terrain::grid_coordinate(tile, index as f64));
    let origin = ::terrain::surface_origin(unit, tile);
    let center = [
        origin[0] + (grid(x0) + grid(x1)) * 0.5,
        origin[1],
        origin[2] - (grid(z0) + grid(z1)) * 0.5,
    ];
    let scale = Vector::new(
        (grid(x1) - grid(x0)) as f32,
        1.0,
        (grid(z1) - grid(z0)) as f32,
    );
    let bytes = std::mem::size_of::<rapier3d::prelude::Collider>()
        + heights.len() * std::mem::size_of::<f32>() * 2;
    let shape = ColliderBuilder::heightfield(Array2::new(rows, x1 - x0 + 1, heights), scale);
    (shape, center, bytes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use rapier3d::prelude::{QueryFilter, Ray};

    /// A patch has the ground of its tile: the same heights at every point of the chunk.
    #[test]
    fn patch_matches_tile_collider() {
        let evaluator = crate::world::TerrainEvaluator::new(::terrain::TerrainSettings {
            seed: 7,
            edge: 0.0,
            edge_gradient: 0.0,
        });
        let road = |x: f32, z: f32| TerrainRoadPoint {
            position: [x, 30.0, z],
            segment_origin: [140.0, -80.0],
            range: 20.0,
        };
        // The road crosses the chunk, so the patch has the road cut.
        let roads = [road(130.0, -110.0), road(140.0, -80.0), road(150.0, -50.0)];
        let tile = crate::world::TerrainTile {
            tier: 0,
            size: 64,
            resolution: 17,
            draw_distance: 1,
            y: 0.0,
            material_scale: 1.0,
        };
        let context = TerrainContext {
            evaluator: &evaluator,
            tile,
            roads: &roads,
            road_radius_squared: None,
            foundations: &[],
            holes: &[],
            biome_override: None,
        };
        let unit = [2, -1];
        let mesh = ::terrain::surface(unit, context);
        assert!(::terrain::regular_grid(
            tile,
            &mesh.vertices,
            &mesh.triangles
        ));
        let surface = crate::world::WorldSurface {
            origin: mesh.origin,
            vertices: mesh.vertices,
            triangles: mesh.triangles,
            ..Default::default()
        };
        let mut physics = PhysicsWorld::empty(0, 2);
        let local = |p: [f64; 3]| p.map(|v| v as f32);
        let (tile_collider, _) = physics
            .insert_source_mesh(
                &surface,
                Some(tile.size as f32),
                local(surface.origin),
                0,
                u32::MAX,
                0.6,
                0.0,
                usize::MAX,
            )
            .unwrap();
        let (shape, center, _) = patch_shape(&context, unit, [1, 0]);
        let patch = physics.insert_collider(shape.translation(Vector::from_array(local(center))));
        physics.refresh_queries();
        let height = |collider: ColliderHandle, x: f32, z: f32| {
            let only = |h: ColliderHandle, _: &rapier3d::prelude::Collider| h == collider;
            let ray = Ray::new(Vector::new(x, 1000., z), -Vector::Y);
            let (_, distance) = physics
                .queries(QueryFilter::default().predicate(&only))
                .cast_ray(&ray, 2000., true)
                .unwrap();
            1000. - distance
        };
        // Chunk [1, 0]: local x 0..32 and source z -32..0, so world z 0..32 from the center.
        for (dx, dz) in [
            (0.5, 0.5),
            (3.1, 17.3),
            (16.0, 8.0),
            (29.9, 31.5),
            (11.2, 2.4),
        ] {
            let x = surface.origin[0] as f32 + dx;
            let z = surface.origin[2] as f32 + dz;
            let (a, b) = (height(tile_collider, x, z), height(patch, x, z));
            assert!((a - b).abs() < 1e-3, "{x} {z}: tile {a}, patch {b}");
        }
    }
}
