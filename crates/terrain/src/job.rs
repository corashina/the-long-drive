//! Byte boundary for one terrain tile job. Values cross it without change.
use super::{
    TerrainContext, TerrainEvaluator, TerrainFoundation, TerrainMesh, TerrainRoadPoint,
    TerrainSettings, TerrainTile,
};

/// One tile job. Tile jobs use every road point without a radius filter.
#[derive(Clone, Copy)]
pub struct TerrainJob<'a> {
    pub settings: TerrainSettings,
    pub unit: [i64; 2],
    pub tile: TerrainTile,
    pub biome_override: Option<u8>,
    pub roads: &'a [TerrainRoadPoint],
    pub foundations: &'a [TerrainFoundation],
    pub holes: &'a [[[f32; 3]; 4]],
}

impl TerrainJob<'_> {
    pub fn encode(&self, out: &mut Vec<u8>) {
        out.clear();
        out.extend(self.settings.seed.to_le_bytes());
        f32s(out, &[self.settings.edge, self.settings.edge_gradient]);
        out.extend(self.unit[0].to_le_bytes());
        out.extend(self.unit[1].to_le_bytes());
        for value in [
            self.tile.tier as u32,
            self.tile.size,
            self.tile.resolution,
            self.tile.draw_distance,
            self.biome_override.map_or(u32::MAX, u32::from),
        ] {
            out.extend(value.to_le_bytes());
        }
        f32s(out, &[self.tile.y, self.tile.material_scale]);
        out.extend((self.roads.len() as u32).to_le_bytes());
        for road in self.roads {
            f32s(out, &road.position);
            f32s(out, &road.segment_origin);
            f32s(out, &[road.range]);
        }
        out.extend((self.foundations.len() as u32).to_le_bytes());
        for foundation in self.foundations {
            f32s(out, &foundation.position);
            f32s(out, &foundation.range_squared);
        }
        out.extend((self.holes.len() as u32).to_le_bytes());
        for hole in self.holes {
            f32s(out, hole.as_flattened());
        }
    }
}

fn f32s(out: &mut Vec<u8>, values: &[f32]) {
    for value in values {
        out.extend(value.to_le_bytes());
    }
}

struct Reader<'a>(&'a [u8]);
impl Reader<'_> {
    fn bytes<const N: usize>(&mut self) -> Result<[u8; N], String> {
        let (head, tail) = self
            .0
            .split_first_chunk::<N>()
            .ok_or("Terrain job input is truncated")?;
        self.0 = tail;
        Ok(*head)
    }
    fn u32(&mut self) -> Result<u32, String> {
        self.bytes().map(u32::from_le_bytes)
    }
    fn f32(&mut self) -> Result<f32, String> {
        self.bytes().map(f32::from_le_bytes)
    }
    fn f32s<const N: usize>(&mut self) -> Result<[f32; N], String> {
        let mut values = [0.0; N];
        for value in &mut values {
            *value = self.f32()?;
        }
        Ok(values)
    }
    fn count(&mut self, item: usize) -> Result<usize, String> {
        let count = self.u32()? as usize;
        if count.saturating_mul(item) > self.0.len() {
            return Err("Terrain job count exceeds its input".into());
        }
        Ok(count)
    }
}

/// One decoded tile job with retained phase and output storage.
pub struct TerrainTask {
    evaluator: TerrainEvaluator,
    tile: TerrainTile,
    biome_override: Option<u8>,
    roads: Vec<TerrainRoadPoint>,
    foundations: Vec<TerrainFoundation>,
    holes: Vec<[[f32; 3]; 4]>,
    job: crate::TerrainSurfaceJob,
    output: TerrainMesh,
}
impl TerrainTask {
    pub fn new(input: &[u8]) -> Result<Self, String> {
        let mut reader = Reader(input);
        let settings = TerrainSettings {
            seed: reader.bytes().map(i32::from_le_bytes)?,
            edge: reader.f32()?,
            edge_gradient: reader.f32()?,
        };
        let unit = [
            reader.bytes().map(i64::from_le_bytes)?,
            reader.bytes().map(i64::from_le_bytes)?,
        ];
        let tier = reader.u32()?;
        let size = reader.u32()?;
        let resolution = reader.u32()?;
        let draw_distance = reader.u32()?;
        let biome = reader.u32()?;
        if tier > 2 || size == 0 || !(4..=1024).contains(&resolution) {
            return Err(format!(
                "Invalid terrain job tile {tier} {size} {resolution}"
            ));
        }
        let tile = TerrainTile {
            tier: tier as usize,
            size,
            resolution,
            draw_distance,
            y: reader.f32()?,
            material_scale: reader.f32()?,
        };
        let biome_override = match biome {
            u32::MAX => None,
            0..=2 => Some(biome as u8),
            _ => return Err(format!("Invalid terrain job biome {biome}")),
        };
        let count = reader.count(24)?;
        let mut roads = Vec::with_capacity(count);
        for _ in 0..count {
            roads.push(TerrainRoadPoint {
                position: reader.f32s()?,
                segment_origin: reader.f32s()?,
                range: reader.f32()?,
            });
        }
        let count = reader.count(20)?;
        let mut foundations = Vec::with_capacity(count);
        for _ in 0..count {
            foundations.push(TerrainFoundation {
                position: reader.f32s()?,
                range_squared: reader.f32s()?,
            });
        }
        let count = reader.count(48)?;
        let mut holes = Vec::with_capacity(count);
        for _ in 0..count {
            let corners: [f32; 12] = reader.f32s()?;
            holes.push(std::array::from_fn(|i| {
                [corners[i * 3], corners[i * 3 + 1], corners[i * 3 + 2]]
            }));
        }
        if !reader.0.is_empty() {
            return Err("Terrain job input has trailing bytes".into());
        }
        let evaluator = TerrainEvaluator::new(settings);
        let mut job = crate::TerrainSurfaceJob::default();
        let mut output = TerrainMesh::default();
        job.begin(
            unit,
            TerrainContext {
                evaluator: &evaluator,
                tile,
                roads: &roads,
                road_radius_squared: None,
                foundations: &foundations,
                holes: &holes,
                biome_override,
            },
            &mut output,
        );
        Ok(Self {
            evaluator,
            tile,
            biome_override,
            roads,
            foundations,
            holes,
            job,
            output,
        })
    }
    pub fn advance(&mut self, should_yield: impl FnMut() -> bool) -> bool {
        self.job.advance(
            TerrainContext {
                evaluator: &self.evaluator,
                tile: self.tile,
                roads: &self.roads,
                road_radius_squared: None,
                foundations: &self.foundations,
                holes: &self.holes,
                biome_override: self.biome_override,
            },
            &mut self.output,
            should_yield,
        )
    }
    pub fn into_mesh(self) -> TerrainMesh {
        self.output
    }
}

/// Decode one job and generate its tile in the background worker.
pub fn evaluate(input: &[u8]) -> Result<TerrainMesh, String> {
    let mut task = TerrainTask::new(input)?;
    while !task.advance(|| false) {}
    Ok(task.into_mesh())
}
