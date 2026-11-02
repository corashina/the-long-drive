use super::TerrainEvaluator;
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

/// Source `GetTerrainColor`: a fixed biome forces the color and keeps alpha.
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
    pub road_radius_squared: Option<f32>,
    pub foundations: &'a [TerrainFoundation],
    /// Source corner order, in frozen source coordinates.
    pub holes: &'a [[[f32; 3]; 4]],
    pub biome_override: Option<u8>,
}

fn inverse_lerp(a: f32, b: f32, value: f32) -> f32 {
    if a == b {
        0.0
    } else {
        ((value as f64 - a as f64) / (b as f64 - a as f64)).clamp(0.0, 1.0) as f32
    }
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

impl TerrainContext<'_> {
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
        let mut nearest = [(usize::MAX, f32::MAX); 2];
        for (index, road) in self.roads.iter().enumerate() {
            if self
                .road_radius_squared
                .is_some_and(|radius| distance_squared(point, road.segment_origin) >= radius)
            {
                continue;
            }
            let dist = distance_squared(point, [road.position[0], road.position[2]]);
            if dist < nearest[0].1 {
                nearest[1] = nearest[0];
                nearest[0] = (index, dist);
            } else if dist < nearest[1].1 {
                nearest[1] = (index, dist);
            }
        }
        if nearest[1].0 != usize::MAX {
            let a = self.roads[nearest[0].0];
            let b = self.roads[nearest[1].0];
            if nearest[0].1 < a.range * a.range {
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
                let along = inverse_lerp(0.0, length, projection(offset, delta));
                let road_height = lerp(a.position[1], b.position[1], along);
                let fade = inverse_lerp(a.range * 0.5, a.range, across);
                value = lerp(road_height, value, fade);
                if across < 4.5 {
                    value = (value as f64 - 0.1_f32 as f64) as f32;
                    close = Some(value);
                }
            }
        }
        close.unwrap_or(value)
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
        output.origin = [
            unit[0] as f64 * tile.size as f64,
            tile.y as f64,
            -(unit[1] as f64 * tile.size as f64),
        ];
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
        let offset = -(tile.size as f32) * 0.5;
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
                    let x = (((i % side) as f64 - 1.0) * spacing as f64 + offset as f64) as f32;
                    let z = (((i / side) as f64 - 1.0) * spacing as f64 + offset as f64) as f32;
                    let point = [
                        (x as f64 + output.origin[0] as f32 as f64) as f32,
                        (z as f64 + (-output.origin[2]) as f32 as f64) as f32,
                    ];
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
                    // Source missing tangent (1,0,0,1), reflected once.
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
    let spacing = (tile.size as f64 / (n - 1) as f64) as f32;
    let offset = -(tile.size as f32) * 0.5;
    // Same math as phase 0, with the phase 6 z flip.
    let grid = |index: usize| (index as f64 * spacing as f64 + offset as f64) as f32;
    vertices.len() == n * n
        && triangles.len() == 2 * (n - 1) * (n - 1)
        && vertices
            .iter()
            .enumerate()
            .all(|(i, v)| v[0] == grid(i % n) && v[2] == -grid(i / n))
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
