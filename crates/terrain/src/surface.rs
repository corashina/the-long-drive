use super::{TerrainEvaluator, noise::inverse_lerp};
use glam::Vec3 as Vector;

/// Strict source thresholds. A supplied index is already zero based.
pub fn biome(color: [f32; 4], override_index: Option<u8>) -> u8 {
    override_index.unwrap_or(if color[2] > 0.25 {
        1
    } else if color[0] > 0.5 {
        0
    } else {
        2
    })
}

/// Source `GetTerrainColor`. A fixed biome forces the color and keeps alpha.
pub fn forced_color(color: [f32; 4], override_index: Option<u8>) -> [f32; 4] {
    match override_index {
        Some(0) => [1.0, 0.0, 0.0, color[3]],
        Some(1) => [0.0, 0.0, 1.0, color[3]],
        Some(2) => [0.0, 1.0, 0.0, color[3]],
        _ => color,
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TerrainTile {
    pub tier: usize,
    pub size: u32,
    pub resolution: u32,
    pub draw_distance: u32,
    pub y: f32,
    pub material_scale: f32,
}

#[derive(Clone, Copy, Debug)]
pub struct TerrainRoadPoint {
    /// Frozen source coordinates. These are never rebased.
    pub position: [f32; 3],
    pub segment_origin: [f32; 2],
    pub range: f32,
}

#[derive(Clone, Copy, Debug, serde::Serialize, serde::Deserialize)]
pub struct TerrainFoundation {
    pub position: [f32; 3],
    pub range_squared: [f32; 2],
}

#[derive(Clone, Copy)]
pub struct TerrainContext<'a> {
    pub evaluator: &'a TerrainEvaluator,
    pub tile: TerrainTile,
    /// Source caller order controls ties and foundation precedence.
    pub roads: &'a [TerrainRoadPoint],
    /// Cell index of `roads`. It gives the same nearest roads as the full scan.
    pub road_grid: Option<&'a TerrainRoadGrid>,
    pub road_radius_squared: Option<f32>,
    pub foundations: &'a [TerrainFoundation],
    /// Source corner order, in frozen source coordinates.
    pub holes: &'a [[[f32; 3]; 4]],
    pub biome_override: Option<u8>,
}

fn lerp(a: f32, b: f32, t: f32) -> f32 {
    (a as f64 + (b as f64 - a as f64) * t.clamp(0.0, 1.0) as f64) as f32
}
fn distance_squared(a: [f32; 2], b: [f32; 2]) -> f32 {
    let x = (a[0] as f64 - b[0] as f64) as f32;
    let y = (a[1] as f64 - b[1] as f64) as f32;
    (x as f64 * x as f64 + y as f64 * y as f64) as f32
}
fn projection(a: [f32; 2], b: [f32; 2]) -> f32 {
    let magnitude = (b[0] as f64 * b[0] as f64 + b[1] as f64 * b[1] as f64).sqrt() as f32;
    if magnitude <= 0.00001 {
        return 0.0;
    }
    let b = b.map(|v| (v as f64 / magnitude as f64) as f32);
    let dot = (a[0] as f64 * b[0] as f64 + a[1] as f64 * b[1] as f64) as f32;
    let p = b.map(|v| (v as f64 * dot as f64) as f32);
    (p[0] as f64 * p[0] as f64 + p[1] as f64 * p[1] as f64).sqrt() as f32
}

/// Source road surface half width. Terrain under it sinks below the road.
pub const ROAD_HALF_WIDTH: f32 = 4.5;

/// Distance across and fraction along the line from `a` to `b`.
fn road_offset(a: TerrainRoadPoint, b: TerrainRoadPoint, point: [f32; 2]) -> (f32, f32) {
    let delta = [
        (b.position[0] as f64 - a.position[0] as f64) as f32,
        (b.position[2] as f64 - a.position[2] as f64) as f32,
    ];
    let offset = [
        (point[0] as f64 - a.position[0] as f64) as f32,
        (point[1] as f64 - a.position[2] as f64) as f32,
    ];
    let across = projection(offset, [-delta[1], delta[0]]);
    let length = distance_squared(
        [a.position[0], a.position[2]],
        [b.position[0], b.position[2]],
    )
    .sqrt();
    (across, inverse_lerp(0.0, length, projection(offset, delta)))
}

impl TerrainContext<'_> {
    /// Distance across the road line through the two nearest road points.
    pub fn road_across(&self, point: [f32; 2]) -> Option<f32> {
        self.nearest_roads(point)
            .map(|(a, b, _)| road_offset(a, b, point).0)
    }
    /// Source caller order controls ties.
    fn nearest_roads(&self, point: [f32; 2]) -> Option<(TerrainRoadPoint, TerrainRoadPoint, f32)> {
        let mut nearest = [(usize::MAX, f32::MAX); 2];
        match self.road_grid {
            Some(grid) => grid.search(point, |index| self.offer(&mut nearest, point, index)),
            None => {
                for index in 0..self.roads.len() {
                    self.offer(&mut nearest, point, index);
                }
            }
        }
        (nearest[1].0 != usize::MAX).then(|| {
            (
                self.roads[nearest[0].0],
                self.roads[nearest[1].0],
                nearest[0].1,
            )
        })
    }
    /// Keeps the two least (distance, index) pairs. In index order this is the source scan.
    /// Returns the second distance.
    fn offer(&self, nearest: &mut [(usize, f32); 2], point: [f32; 2], index: usize) -> f32 {
        let road = &self.roads[index];
        if self
            .road_radius_squared
            .is_some_and(|radius| distance_squared(point, road.segment_origin) >= radius)
        {
            return nearest[1].1;
        }
        let dist = distance_squared(point, [road.position[0], road.position[2]]);
        let before = |(i, d): (usize, f32)| dist < d || (dist == d && index < i);
        if before(nearest[0]) {
            nearest[1] = nearest[0];
            nearest[0] = (index, dist);
        } else if before(nearest[1]) {
            nearest[1] = (index, dist);
        }
        nearest[1].1
    }
    pub fn height(&self, point: [f32; 2]) -> f32 {
        let mut value = self.evaluator.base_height(point, true);
        if self.tile.tier != 0 {
            return value;
        }
        let mut close = None;
        for helper in self.foundations {
            let dist = distance_squared(point, [helper.position[0], helper.position[2]]);
            if dist < helper.range_squared[0] {
                close = Some(helper.position[1]);
                value = helper.position[1];
                break;
            }
            if dist < helper.range_squared[1] {
                value = lerp(
                    helper.position[1],
                    value,
                    inverse_lerp(helper.range_squared[0], helper.range_squared[1], dist),
                );
            }
        }
        if let Some((a, b, dist)) = self.nearest_roads(point)
            && dist < a.range * a.range
        {
            let (across, along) = road_offset(a, b, point);
            let road_height = lerp(a.position[1], b.position[1], along);
            let fade = inverse_lerp(a.range * 0.5, a.range, across);
            value = lerp(road_height, value, fade);
            if across < ROAD_HALF_WIDTH {
                value = (value as f64 - 0.1_f32 as f64) as f32;
                close = Some(value);
            }
        }
        close.unwrap_or(value)
    }
}

/// Road points in square cells, for the nearest road search of one tile job.
pub struct TerrainRoadGrid {
    origin: [f64; 2],
    /// A power of two, so cell edges are exact in f64.
    cell: f64,
    size: [usize; 2],
    /// Start of each cell in `indices`, and one end entry.
    starts: Vec<u32>,
    /// Road indices in cell order, then index order.
    indices: Vec<u32>,
}
const ROAD_GRID_CELL: f64 = 64.0;
const ROAD_GRID_SIDE: usize = 128;
/// Covers the f32 rounding of `distance_squared`.
const ROAD_GRID_MARGIN: f64 = 1.0 - 1e-5;

impl TerrainRoadGrid {
    pub fn new(roads: &[TerrainRoadPoint]) -> Self {
        let xz = |road: &TerrainRoadPoint| [road.position[0] as f64, road.position[2] as f64];
        let finite = |road: &&TerrainRoadPoint| xz(road).iter().all(|v| v.is_finite());
        let mut min = [0.0; 2];
        let mut max = [0.0; 2];
        for (n, road) in roads.iter().filter(finite).enumerate() {
            let p = xz(road);
            for axis in 0..2 {
                min[axis] = if n == 0 {
                    p[axis]
                } else {
                    p[axis].min(min[axis])
                };
                max[axis] = if n == 0 {
                    p[axis]
                } else {
                    p[axis].max(max[axis])
                };
            }
        }
        let mut cell = ROAD_GRID_CELL;
        while (max[0] - min[0]).max(max[1] - min[1]) / cell >= ROAD_GRID_SIDE as f64 {
            cell *= 2.0;
        }
        let size = [0, 1].map(|axis| ((max[axis] - min[axis]) / cell) as usize + 1);
        let mut grid = Self {
            origin: min,
            cell,
            size,
            starts: vec![0; size[0] * size[1] + 1],
            indices: Vec::new(),
        };
        for road in roads.iter().filter(finite) {
            let slot = grid.slot(grid.cell_of(xz(road)));
            grid.starts[slot + 1] += 1;
        }
        for slot in 1..grid.starts.len() {
            grid.starts[slot] += grid.starts[slot - 1];
        }
        grid.indices = vec![0; grid.starts[grid.starts.len() - 1] as usize];
        let mut fill = grid.starts.clone();
        for (index, road) in roads.iter().enumerate().filter(|(_, road)| finite(road)) {
            let slot = grid.slot(grid.cell_of(xz(road)));
            grid.indices[fill[slot] as usize] = index as u32;
            fill[slot] += 1;
        }
        grid
    }
    fn cell_of(&self, point: [f64; 2]) -> [usize; 2] {
        [0, 1].map(|axis| {
            (((point[axis] - self.origin[axis]) / self.cell).floor() as usize)
                .min(self.size[axis] - 1)
        })
    }
    fn slot(&self, cell: [usize; 2]) -> usize {
        cell[1] * self.size[0] + cell[0]
    }
    /// Least distance from `point` to a cell outside ring `ring - 1` around `cell`.
    /// None when no such cell exists.
    fn ring_bound(&self, point: [f64; 2], cell: [usize; 2], ring: usize) -> Option<f64> {
        let mut bound: Option<f64> = None;
        for axis in 0..2 {
            let c = cell[axis];
            if c >= ring {
                let low = self.origin[axis] + (c + 1 - ring) as f64 * self.cell;
                bound = Some(bound.map_or(point[axis] - low, |b| b.min(point[axis] - low)));
            }
            if c + ring < self.size[axis] {
                let high = self.origin[axis] + (c + ring) as f64 * self.cell;
                bound = Some(bound.map_or(high - point[axis], |b| b.min(high - point[axis])));
            }
        }
        bound
    }
    /// Offers road indices ring by ring around `point`. `offer` returns the second best
    /// distance. The search stops when no unvisited road can be at or below it.
    fn search(&self, point: [f32; 2], mut offer: impl FnMut(usize) -> f32) {
        let q = [point[0] as f64, point[1] as f64];
        let cell = self.cell_of(q);
        let mut second = f32::MAX;
        let mut visit = |x: usize, z: usize, second: &mut f32| {
            let slot = self.slot([x, z]);
            for &index in &self.indices[self.starts[slot] as usize..self.starts[slot + 1] as usize]
            {
                *second = offer(index as usize);
            }
        };
        visit(cell[0], cell[1], &mut second);
        for ring in 1.. {
            match self.ring_bound(q, cell, ring) {
                Some(bound) if bound * bound * ROAD_GRID_MARGIN <= second as f64 => {}
                _ => return,
            }
            let x0 = cell[0].saturating_sub(ring);
            let x1 = (cell[0] + ring).min(self.size[0] - 1);
            let z0 = cell[1].saturating_sub(ring);
            let z1 = (cell[1] + ring).min(self.size[1] - 1);
            for z in z0..=z1 {
                if z + ring == cell[1] || z == cell[1] + ring {
                    (x0..=x1).for_each(|x| visit(x, z, &mut second));
                } else {
                    if cell[0] >= ring {
                        visit(cell[0] - ring, z, &mut second);
                    }
                    if cell[0] + ring < self.size[0] {
                        visit(cell[0] + ring, z, &mut second);
                    }
                }
            }
        }
    }
}

/// Source terrain tile mesh in its local frame.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct TerrainMesh {
    pub origin: [f64; 3],
    pub vertices: Vec<[f32; 3]>,
    pub normals: Vec<[f32; 3]>,
    pub tangents: Vec<[f32; 4]>,
    pub uv: Vec<[f32; 2]>,
    pub colors: Vec<[f32; 4]>,
    pub triangles: Vec<[u32; 3]>,
}

pub fn surface_origin(unit: [i64; 2], tile: TerrainTile) -> [f64; 3] {
    [
        unit[0] as f64 * tile.size as f64,
        tile.y as f64,
        -(unit[1] as f64 * tile.size as f64),
    ]
}

/// Local coordinate of grid line `index` from the tile center, in phase 0 math.
/// -1 and the resolution are border lines.
pub fn grid_coordinate(tile: TerrainTile, index: f64) -> f32 {
    let spacing = (tile.size as f64 / (tile.resolution as usize - 1) as f64) as f32;
    let offset = -(tile.size as f32) * 0.5;
    (index * spacing as f64 + offset as f64) as f32
}

/// Source point of local grid coordinates `x` and source `z` of a tile at `origin`.
fn grid_point(origin: [f64; 3], x: f32, z: f32) -> [f32; 2] {
    [
        (x as f64 + origin[0] as f32 as f64) as f32,
        (z as f64 + (-origin[2]) as f32 as f64) as f32,
    ]
}

impl TerrainContext<'_> {
    /// Height of grid vertex `x`, `z` (source z order) of tile `unit`. It is the vertex height
    /// of a tile without holes.
    pub fn grid_height(&self, unit: [i64; 2], x: usize, z: usize) -> f32 {
        self.height(grid_point(
            surface_origin(unit, self.tile),
            grid_coordinate(self.tile, x as f64),
            grid_coordinate(self.tile, z as f64),
        ))
    }
}

/// All phases retain their cursors across deadline checks and the 4096-work cap.
/// A check follows at least 32 work units. One operation can use up to seven units.
#[derive(Default)]
pub struct TerrainSurfaceJob {
    border: Vec<[f32; 3]>,
    border_normals: Vec<[f32; 3]>,
    cursor: usize,
    phase: u8,
    hole: usize,
    nearest: (usize, f32),
    write: usize,
    work: usize,
}
impl TerrainSurfaceJob {
    pub fn prepared_vertices(&self) -> usize {
        self.work
    }
    pub fn allocation_bound(tile: TerrainTile) -> usize {
        let n = tile.resolution as usize;
        (n + 2) * (n + 2) * 24
            + n * n * 64
            + (n - 1) * (n - 1) * 24
            + std::mem::size_of::<std::ops::Range<usize>>()
    }
    pub fn retained_bytes(&self) -> usize {
        (self.border.capacity() + self.border_normals.capacity()) * 12
    }
    pub fn begin(&mut self, unit: [i64; 2], context: TerrainContext<'_>, output: &mut TerrainMesh) {
        let tile = context.tile;
        let n = tile.resolution as usize;
        *self = Self::default();
        output.origin = surface_origin(unit, tile);
        output.vertices.clear();
        output.normals.clear();
        output.tangents.clear();
        output.colors.clear();
        output.uv.clear();
        output.triangles.clear();
        self.border.reserve_exact((n + 2) * (n + 2));
        self.border_normals.reserve_exact((n + 2) * (n + 2));
        output.vertices.reserve_exact(n * n);
        output.normals.reserve_exact(n * n);
        output.tangents.reserve_exact(n * n);
        output.colors.reserve_exact(n * n);
        output.uv.reserve_exact(n * n);
        output.triangles.reserve_exact((n - 1) * (n - 1) * 2);
    }
    pub fn advance(
        &mut self,
        context: TerrainContext<'_>,
        output: &mut TerrainMesh,
        mut should_yield: impl FnMut() -> bool,
    ) -> bool {
        let tile = context.tile;
        let n = tile.resolution as usize;
        let side = n + 2;
        let spacing = (tile.size as f64 / (n - 1) as f64) as f32;
        let mut remaining = 4096;
        let mut checkpoint = 0;
        while self.phase < 8 {
            // Always advance one small slice, even after the caller's deadline.
            if checkpoint >= 32 {
                if should_yield() {
                    break;
                }
                checkpoint = 0;
            }
            let cost = match self.phase {
                1 => 6,
                3 => 5,
                2 => 7,
                5 | 7 => 3,
                _ => 1,
            };
            if remaining < cost {
                break;
            }
            remaining -= cost;
            self.work += cost;
            checkpoint += cost;
            let i = self.cursor;
            match self.phase {
                0 => {
                    let x = grid_coordinate(tile, (i % side) as f64 - 1.0);
                    let z = grid_coordinate(tile, (i / side) as f64 - 1.0);
                    let point = grid_point(output.origin, x, z);
                    self.border.push([x, context.height(point), z]);
                    self.border_normals.push([0.0; 3]);
                    self.cursor += 1;
                    if self.cursor == side * side {
                        self.phase = 1;
                        self.cursor = 0;
                    }
                }
                1 => {
                    let a = (i / (side - 1)) * side + i % (side - 1);
                    let b = a + side;
                    for triangle in [[a, b, b + 1], [b + 1, a + 1, a]] {
                        let [a, b, c] = triangle.map(|j| Vector::from_array(self.border[j]));
                        let cross = (b - a).cross(c - a);
                        let normal = if cross.length() > 0.00001 {
                            cross.normalize()
                        } else {
                            Vector::ZERO
                        };
                        for j in triangle {
                            self.border_normals[j] =
                                (Vector::from_array(self.border_normals[j]) + normal).to_array();
                        }
                    }
                    self.cursor += 1;
                    if self.cursor == (side - 1) * (side - 1) {
                        self.phase = 2;
                        self.cursor = 0;
                    }
                }
                2 => {
                    let x = i % n;
                    let z = i / n;
                    let j = (z + 1) * side + x + 1;
                    output.vertices.push(self.border[j]);
                    let normal = Vector::from_array(self.border_normals[j]);
                    output.normals.push(if normal.length() > 0.00001 {
                        normal.normalize().to_array()
                    } else {
                        [0.0; 3]
                    });
                    if x < n - 1 && z < n - 1 {
                        let a = i as u32;
                        let b = a + n as u32;
                        output.triangles.extend([[a, b, b + 1], [b + 1, a + 1, a]]);
                    }
                    self.cursor += 1;
                    if self.cursor == n * n {
                        self.phase = 3;
                        self.cursor = 0;
                        self.nearest = (usize::MAX, f32::MAX);
                    }
                }
                3 | 4 | 5 => {
                    if tile.tier != 0 || self.hole == context.holes.len() {
                        self.phase = 6;
                        self.cursor = 0;
                        continue;
                    }
                    let hole = context.holes[self.hole].map(|p| {
                        [
                            (p[0] as f64 - output.origin[0]) as f32,
                            p[1],
                            (p[2] as f64 + output.origin[2]) as f32,
                        ]
                    });
                    if self.phase == 3 {
                        let j = (i / (n - 1)) * n + i % (n - 1);
                        let d = horizontal_squared(output.vertices[j], hole[2]);
                        if d < self.nearest.1 {
                            self.nearest = (j, d);
                        }
                        self.cursor += 1;
                        if self.cursor == (n - 1) * (n - 1) {
                            if self.nearest.1 < (spacing as f64 * 3.0) as f32 {
                                let j = self.nearest.0;
                                output.vertices[j] = hole[2];
                                output.vertices[j + 1] = hole[3];
                                output.vertices[j + n] = hole[0];
                                output.vertices[j + n + 1] = hole[1];
                            }
                            self.phase = 4;
                            self.cursor = 0;
                        }
                    } else if self.phase == 4 {
                        let point = &mut output.vertices[i];
                        let inside = inside_source_hole(*point, &hole);
                        let mut nearest = None;
                        let mut closest = f32::MAX;
                        let squared = (spacing as f64 * spacing as f64) as f32;
                        for (j, corner) in hole.iter().enumerate() {
                            let d = horizontal_squared(*point, *corner);
                            if d < closest && (inside || d < squared) {
                                nearest = Some(j);
                                closest = d;
                            }
                        }
                        if let Some(j) = nearest {
                            *point = hole[j];
                        }
                        self.cursor += 1;
                        if self.cursor == n * n {
                            self.phase = 5;
                            self.cursor = 0;
                            self.write = 0;
                        }
                    } else {
                        if i < output.triangles.len() {
                            let triangle = output.triangles[i];
                            let center = std::array::from_fn(|axis| {
                                let ab = (output.vertices[triangle[0] as usize][axis] as f64
                                    + output.vertices[triangle[1] as usize][axis] as f64)
                                    as f32;
                                let sum = (ab as f64
                                    + output.vertices[triangle[2] as usize][axis] as f64)
                                    as f32;
                                (sum as f64 / 3.0) as f32
                            });
                            if !inside_source_hole(center, &hole) {
                                output.triangles[self.write] = triangle;
                                self.write += 1;
                            }
                            self.cursor += 1;
                        }
                        if self.cursor == output.triangles.len() {
                            output.triangles.truncate(self.write);
                            self.hole += 1;
                            self.phase = 3;
                            self.cursor = 0;
                            self.nearest = (usize::MAX, f32::MAX);
                        }
                    }
                }
                6 => {
                    let p = output.vertices[i];
                    let reciprocal = (1.0 / tile.size as f64) as f32;
                    output.uv.push([
                        (reciprocal as f64 * p[0] as f64) as f32,
                        (reciprocal as f64 * p[2] as f64) as f32,
                    ]);
                    let grid = self.border[(i / n + 1) * side + i % n + 1];
                    let color = context.evaluator.color(
                        [
                            (grid[0] as f64 + output.origin[0] as f32 as f64) as f32,
                            (grid[2] as f64 + (-output.origin[2]) as f32 as f64) as f32,
                        ],
                        p[1],
                    );
                    output
                        .colors
                        .push(forced_color(color, context.biome_override));
                    output.vertices[i][2] = -p[2];
                    output.normals[i][2] = -output.normals[i][2];
                    // The source has no tangent. Use (1,0,0,1), reflected once.
                    output.tangents.push([1.0, 0.0, 0.0, -1.0]);
                    self.cursor += 1;
                    if self.cursor == n * n {
                        self.phase = 7;
                        self.cursor = 0;
                    }
                }
                7 => {
                    if i < output.triangles.len() {
                        output.triangles[i].swap(1, 2);
                        self.cursor += 1;
                    }
                    if self.cursor == output.triangles.len() {
                        self.phase = 8;
                    }
                }
                _ => unreachable!(),
            }
        }
        self.phase == 8
    }
}

/// True when a finished tile is the full regular grid: n * n vertices on their grid
/// x and z, and every grid triangle. A building hole moves vertices or removes triangles.
pub fn regular_grid(tile: TerrainTile, vertices: &[[f32; 3]], triangles: &[[u32; 3]]) -> bool {
    let n = tile.resolution as usize;
    // This is the same math as phase 0, with the phase 6 z flip.
    let grid = |index: usize| grid_coordinate(tile, index as f64);
    vertices.len() == n * n
        && triangles.len() == 2 * (n - 1) * (n - 1)
        && vertices
            .iter()
            .enumerate()
            .all(|(i, v)| v[0] == grid(i % n) && v[2] == -grid(i / n))
}

/// Triangle count of a regular grid tile of `resolution`.
pub fn grid_triangle_count(resolution: u32) -> usize {
    let cells = resolution as usize - 1;
    2 * cells * cells
}

/// Triangles of a regular grid tile of `resolution`, as phases 2 and 7 build them.
pub fn grid_triangles(resolution: u32) -> Vec<[u32; 3]> {
    let n = resolution;
    let mut triangles = Vec::with_capacity(grid_triangle_count(resolution));
    for z in 0..n - 1 {
        for x in 0..n - 1 {
            let a = z * n + x;
            let b = a + n;
            triangles.extend([[a, b + 1, b], [b + 1, a, a + 1]]);
        }
    }
    triangles
}

/// UV of a regular grid tile of `size` and `resolution`, as phase 6 builds it.
pub fn grid_uv(size: u32, resolution: u32) -> Vec<[f32; 2]> {
    let tile = TerrainTile {
        tier: 0,
        size,
        resolution,
        draw_distance: 0,
        y: 0.0,
        material_scale: 0.0,
    };
    let n = resolution as usize;
    let reciprocal = (1.0 / size as f64) as f32;
    (0..n * n)
        .map(|i| {
            let x = grid_coordinate(tile, (i % n) as f64);
            let z = grid_coordinate(tile, (i / n) as f64);
            [
                (reciprocal as f64 * x as f64) as f32,
                (reciprocal as f64 * z as f64) as f32,
            ]
        })
        .collect()
}

/// Source terrain tile generation, in source phase order.
pub fn surface(unit: [i64; 2], context: TerrainContext<'_>) -> TerrainMesh {
    let mut output = TerrainMesh::default();
    let mut job = TerrainSurfaceJob::default();
    job.begin(unit, context, &mut output);
    while !job.advance(context, &mut output, || false) {}
    output
}

fn horizontal_squared(a: [f32; 3], b: [f32; 3]) -> f32 {
    distance_squared([a[0], a[2]], [b[0], b[2]])
}
fn triangle_area(a: [f32; 3], b: [f32; 3], c: [f32; 3]) -> f32 {
    let ab = horizontal_squared(a, b).sqrt();
    let bc = horizontal_squared(b, c).sqrt();
    let ca = horizontal_squared(c, a).sqrt();
    let half = ((ab as f64 + bc as f64 + ca as f64) * 0.5) as f32;
    ((half as f64
        * (half as f64 - ab as f64)
        * (half as f64 - bc as f64)
        * (half as f64 - ca as f64)) as f32)
        .sqrt()
}
fn inside_source_hole(point: [f32; 3], hole: &[[f32; 3]; 4]) -> bool {
    let area = triangle_area(hole[0], hole[1], hole[3]) as f64
        + triangle_area(hole[0], hole[2], hole[3]) as f64;
    let pieces = triangle_area(hole[0], hole[1], point) as f64
        + triangle_area(hole[1], hole[3], point) as f64
        + triangle_area(hole[3], hole[2], point) as f64
        + triangle_area(hole[2], hole[0], point) as f64;
    ((area - pieces) as f32).abs() < 0.2
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::TerrainSettings;

    /// A tile without holes has the grid triangles and UV, on near and distant tiers.
    #[test]
    fn grid_rebuild_matches_surface() {
        let evaluator = TerrainEvaluator::new(TerrainSettings {
            seed: 1,
            edge: 2.0e7,
            edge_gradient: 20000.0,
        });
        for (tier, size, resolution) in [(0, 1111, 33), (1, 7708, 64)] {
            let tile = TerrainTile {
                tier,
                size,
                resolution,
                draw_distance: 1,
                y: 0.0,
                material_scale: 1.0,
            };
            let mesh = surface(
                [3, -2],
                TerrainContext {
                    evaluator: &evaluator,
                    tile,
                    roads: &[],
                    road_grid: None,
                    road_radius_squared: None,
                    foundations: &[],
                    holes: &[],
                    biome_override: None,
                },
            );
            assert!(regular_grid(tile, &mesh.vertices, &mesh.triangles));
            assert_eq!(grid_triangles(resolution), mesh.triangles);
            assert_eq!(grid_uv(size, resolution), mesh.uv);
        }
    }

    #[test]
    fn road_across_follows_road_points() {
        let evaluator = TerrainEvaluator::new(TerrainSettings {
            seed: 1,
            edge: 0.0,
            edge_gradient: 0.0,
        });
        let road = |x, z| TerrainRoadPoint {
            position: [x, 0.0, z],
            segment_origin: [0.0; 2],
            range: 20.0,
        };
        // A bend: the road leaves the straight line at Z 10.
        let roads = [
            road(0.0, 0.0),
            road(0.0, 10.0),
            road(5.0, 20.0),
            road(10.0, 30.0),
        ];
        let context = TerrainContext {
            evaluator: &evaluator,
            tile: TerrainTile {
                tier: 0,
                size: 1,
                resolution: 1,
                draw_distance: 1,
                y: 0.0,
                material_scale: 1.0,
            },
            roads: &roads,
            road_grid: None,
            road_radius_squared: None,
            foundations: &[],
            holes: &[],
            biome_override: None,
        };
        let on_bend = context.road_across([7.5, 25.0]).unwrap();
        assert!(on_bend < 0.01, "{on_bend}");
        let beside = context.road_across([3.0, 5.0]).unwrap();
        assert!((beside - 3.0).abs() < 0.01, "{beside}");
        let empty = TerrainContext {
            roads: &[],
            ..context
        };
        assert!(empty.road_across([0.0; 2]).is_none());
    }

    #[test]
    fn road_grid_matches_full_scan() {
        let evaluator = TerrainEvaluator::new(TerrainSettings {
            seed: 1,
            edge: 0.0,
            edge_gradient: 0.0,
        });
        let mut seed = 7_u64;
        let mut next = |range: f32| {
            seed = seed
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            // Whole and half metres give many equal distances.
            ((seed >> 33) % (range as u64 * 2)) as f32 * 0.5 - range * 0.5
        };
        let mut roads: Vec<_> = (0..800)
            .map(|_| TerrainRoadPoint {
                position: [next(1500.0) + 40_000.0, 0.0, next(1500.0) - 20_000.0],
                segment_origin: [next(1500.0) + 40_000.0, next(1500.0) - 20_000.0],
                range: 20.0,
            })
            .collect();
        // Copies tie with their originals. The range tells them apart.
        roads.extend_from_within(0..50);
        roads[800..].iter_mut().for_each(|road| road.range = 30.0);
        let grid = TerrainRoadGrid::new(&roads);
        let tile = TerrainTile {
            tier: 0,
            size: 1,
            resolution: 1,
            draw_distance: 1,
            y: 0.0,
            material_scale: 1.0,
        };
        for radius in [None, Some(250_000.0)] {
            let full = TerrainContext {
                evaluator: &evaluator,
                tile,
                roads: &roads,
                road_grid: None,
                road_radius_squared: radius,
                foundations: &[],
                holes: &[],
                biome_override: None,
            };
            let indexed = TerrainContext {
                road_grid: Some(&grid),
                ..full
            };
            for _ in 0..4000 {
                // Some points fall outside the grid.
                let point = [next(4000.0) + 40_000.0, next(4000.0) - 20_000.0];
                let a = full
                    .nearest_roads(point)
                    .map(|(a, b, d)| (a.position, a.range, b.position, b.range, d));
                let b = indexed
                    .nearest_roads(point)
                    .map(|(a, b, d)| (a.position, a.range, b.position, b.range, d));
                assert_eq!(a, b, "{point:?}");
            }
        }
    }
}
