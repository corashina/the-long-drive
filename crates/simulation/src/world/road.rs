//! Source road progression in a fixed generation frame.
use super::{SourceRandom, TerrainEvaluator, WorldBounds, source_perlin};
use crate::reference::{ReferenceError, WorldReference};
use road_mesh::half_angle;
use std::sync::Arc;

pub const ROAD_SEGMENTS: u32 = 50_000;
pub const ROAD_CONTROLS: usize = 10;
pub const ROAD_CHECKPOINT_INTERVAL: u32 = 256;
const BLOCKS: usize = ROAD_SEGMENTS.div_ceil(ROAD_CHECKPOINT_INTERVAL) as usize;

#[derive(Clone, Debug)]
struct CurveOctave {
    denominator: f64,
    ordinate: f64,
    amplitude: f32,
}

/// The source OneValue curve, before multiplication by road width.
#[derive(Clone, Debug)]
pub struct RoadCurve {
    noises: Vec<Vec<CurveOctave>>,
    seed: f32,
    offset: f32,
}
impl RoadCurve {
    pub fn new(reference: &WorldReference, seed: i32) -> Self {
        let seed = if reference.terrain.random_seed != 0 {
            seed
        } else {
            reference.terrain.seed
        };
        let seed =
            (SourceRandom::new(seed).next_range(-100000, 100000) as f64 * 1E-05_f32 as f64) as f32;
        let road = &reference.road;
        let noises = road
            .noises
            .iter()
            .map(|noise| {
                let mut amplitude = noise.amplitude;
                let mut frequency = noise.frequency;
                (1..=noise.octaves + 1)
                    .map(|i| {
                        let octave = CurveOctave {
                            denominator: road.length as f64 * road.scale as f64 * frequency as f64,
                            ordinate: seed as f64
                                + (i as f32 as f64 / noise.octaves as f32 as f64 + 1.0),
                            amplitude,
                        };
                        amplitude = (amplitude as f64 * noise.persistence as f64) as f32;
                        frequency = (frequency as f64 * noise.lacunarity as f64) as f32;
                        octave
                    })
                    .collect()
            })
            .collect();
        let mut curve = Self {
            noises,
            seed,
            offset: 0.0,
        };
        // Source first-call recursion evaluates zero before the requested distance.
        curve.offset = -curve.sum(0.0);
        curve
    }

    fn sum(&self, distance: f32) -> f32 {
        let mut total = 0.0_f32;
        for noise in &self.noises {
            let mut value = 0.0_f32;
            for octave in noise {
                let x = (self.seed as f64 + distance as f64 / octave.denominator) as f32;
                let y = ((octave.ordinate + distance as f64) / octave.denominator) as f32;
                let sample = source_perlin(x, y) as f64 * octave.amplitude as f64;
                value = (value as f64 + sample) as f32;
            }
            total = (total as f64 + value as f64) as f32;
        }
        total
    }

    pub fn value(&self, distance: f32) -> f32 {
        (self.sum(distance) as f64 + self.offset as f64) as f32
    }
}

/// Carried source values at the boundary before the next segment.
#[derive(Clone, Copy, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct RoadState {
    pub next_segment: u32,
    pub source_position: [f64; 3],
    pub source_rotation: [f32; 3],
    pub distance: f64,
}

#[derive(Clone, Copy, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct RoadCheckpoint {
    state: RoadState,
}
impl RoadCheckpoint {
    pub fn state(&self) -> RoadState {
        self.state
    }
}

/// Source controls remain unconverted for source skinning and terrain alignment.
#[derive(Clone, Copy, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct RoadSegment {
    pub index: u32,
    pub biome: u8,
    pub source_position: [f64; 3],
    pub source_origin: [f64; 3],
    pub source_rotations: [[f32; 3]; ROAD_CONTROLS],
    pub source_coords: [[f32; 3]; ROAD_CONTROLS],
    pub bounds: WorldBounds,
}
impl RoadSegment {
    /// Absolute runtime origin; apply the source Z reflection once.
    pub fn position(&self) -> [f64; 3] {
        runtime_point(std::array::from_fn(|a| {
            self.source_origin[a] + self.source_position[a]
        }))
    }
}

struct RoadInputs {
    curve: RoadCurve,
    origin: [f64; 3],
    bone_length: f32,
    segment_length: f32,
    above_terrain: f32,
    angle_limits: [f32; 2],
    width: f32,
    start_straight: u32,
    biome_override: Option<u8>,
    straight_modifier: f32,
    edge: bool,
    bounds_radius: f64,
}

#[derive(Clone)]
pub struct RoadSequence {
    inputs: Arc<RoadInputs>,
    state: RoadState,
}
impl RoadSequence {
    pub fn new(
        reference: &WorldReference,
        seed: i32,
        biome_override: Option<u8>,
    ) -> Result<Self, ReferenceError> {
        let road = &reference.road;
        let frame = &reference.generation_frame;
        if road.road_bone_num != ROAD_CONTROLS as u32
            || road.length != 5_000_000.0
            || road.road_segment_length != 100.0
            || road.road_bone_length != 10.0
            || road.aliign_road != 0
            || road.align_buildings != 0
            || frame.translation != [0.0; 3]
            || frame.rotation != [0.0, 0.0, 0.0, 1.0]
            || frame.scale != [1.0; 3]
        {
            return Err(ReferenceError(format!(
                "Invalid reference field: {}:{} frozen road frame/extent",
                road.source.file, road.source.path_id
            )));
        }
        // FStart samples terrain before StartObjs receives its shared garage offset.
        // The road parent translation is not the generation origin.
        let terrain = super::terrain_evaluator(reference, seed);
        let origin = [
            0.0,
            terrain.base_height([0.0; 2], road.align_edge_of_the_world != 0) as f64,
            0.0,
        ];
        Ok(Self {
            inputs: Arc::new(RoadInputs {
                curve: RoadCurve::new(reference, seed),
                origin,
                bone_length: road.road_bone_length,
                segment_length: road.road_segment_length,
                above_terrain: road.road_above_terrain,
                angle_limits: [road.xmax_angle, road.ymax_angle],
                width: road.width,
                start_straight: road.start_straight,
                biome_override,
                straight_modifier: road.straight_modifier,
                edge: road.align_edge_of_the_world != 0,
                bounds_radius: bounds_radius(reference)?,
            }),
            state: RoadState {
                next_segment: 0,
                source_position: [0.0; 3],
                source_rotation: [0.0; 3],
                distance: 0.0,
            },
        })
    }

    pub fn checkpoint(&self) -> RoadCheckpoint {
        RoadCheckpoint { state: self.state }
    }

    /// Restore a checkpoint from this sequence's checked inputs and seed.
    pub fn restore(&mut self, checkpoint: &RoadCheckpoint) {
        self.state = checkpoint.state;
    }

    pub fn advance(&mut self, terrain: &TerrainEvaluator) -> Option<RoadSegment> {
        if self.state.next_segment == ROAD_SEGMENTS {
            return None;
        }
        let input = &self.inputs;
        let mut segment = RoadSegment {
            index: self.state.next_segment,
            biome: super::biome(
                terrain.color(
                    [
                        (input.origin[0] + self.state.source_position[0]) as f32,
                        (input.origin[2] + self.state.source_position[2]) as f32,
                    ],
                    (input.origin[1] + self.state.source_position[1]) as f32,
                ),
                input.biome_override,
            ),
            source_position: self.state.source_position,
            source_origin: input.origin,
            source_rotations: [[0.0; 3]; ROAD_CONTROLS],
            source_coords: [[0.0; 3]; ROAD_CONTROLS],
            bounds: empty_bounds(),
        };
        segment.source_rotations[0] = self.state.source_rotation;
        let mut last = forward_step(self.state.source_rotation, input.bone_length);
        self.state.source_position =
            std::array::from_fn(|a| self.state.source_position[a] + last[a] as f64);
        segment.source_coords[0] = [
            last[0],
            (last[1] as f64 - input.above_terrain as f64) as f32,
            last[2],
        ];
        for bone in 1..ROAD_CONTROLS {
            let step = forward_step(self.state.source_rotation, input.bone_length);
            let look: [f32; 3] = std::array::from_fn(|a| {
                let absolute =
                    (input.origin[a] + segment.source_position[a] + last[a] as f64) as f32;
                (absolute as f64 + step[a] as f64) as f32
            });
            let mut height = (input.above_terrain as f64
                + terrain.base_height([look[0], look[2]], input.edge) as f64)
                as f32;
            for offset in [1.0, 2.0, -1.0, -2.0] {
                let xz = [0, 2].map(|a| {
                    let first =
                        look[a] as f64 + step[a] as f64 * if offset > 0.0 { 1.0 } else { -1.0 };
                    (if offset == 2.0 {
                        first + step[a] as f64
                    } else if offset == -2.0 {
                        first - step[a] as f64
                    } else {
                        first
                    }) as f32
                });
                height = ((height as f64
                    + input.above_terrain as f64
                    + terrain.base_height(xz, input.edge) as f64)
                    * 0.5) as f32;
            }
            // OneCoord projects onto the checked identity world's forward axis.
            let distance = (look[2] as f64 - input.origin[2]) as f32;
            let target_x = (input.curve.value(distance.abs()) as f64 * input.width as f64) as f32;
            let error_x = (target_x as f64 - look[0] as f64) as f32;
            let mut rotation = [
                turn_angle(
                    (look[1] as f64 - height as f64) as f32,
                    input.bone_length,
                    input.angle_limits[0],
                ),
                turn_angle(error_x, input.bone_length, input.angle_limits[1]),
                0.0,
            ];
            if segment.index < input.start_straight {
                rotation[1] = (rotation[1] as f64 * input.straight_modifier as f64) as f32;
            }
            segment.source_rotations[bone] = rotation;
            self.state.source_rotation = std::array::from_fn(|a| {
                ((self.state.source_rotation[a] as f64 + rotation[a] as f64) as f32)
                    .clamp(-90.0, 90.0)
            });
            let step = forward_step(self.state.source_rotation, input.bone_length);
            last = std::array::from_fn(|a| (last[a] as f64 + step[a] as f64) as f32);
            self.state.source_position =
                std::array::from_fn(|a| self.state.source_position[a] + step[a] as f64);
            segment.source_coords[bone] = [
                last[0],
                (last[1] as f64 - input.above_terrain as f64) as f32,
                last[2],
            ];
        }
        self.state.next_segment += 1;
        self.state.distance += input.segment_length as f64;
        let position = segment.position();
        segment.bounds = WorldBounds {
            min: position.map(|x| x - input.bounds_radius),
            max: position.map(|x| x + input.bounds_radius),
        };
        Some(segment)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct RoadBlock {
    pub first: u32,
    pub end: u32,
    pub bounds: WorldBounds,
}

const _: () = assert!(
    (BLOCKS + 1) * std::mem::size_of::<RoadCheckpoint>()
        + BLOCKS * std::mem::size_of::<RoadBlock>()
        + ROAD_CHECKPOINT_INTERVAL as usize * std::mem::size_of::<RoadSegment>()
        < 16 * 1024 * 1024
);

/// Sparse source states and conservative spatial bounds. Inputs remain private and frozen.
pub struct RoadIndex {
    latest: Option<RoadSegment>,
    sequence: RoadSequence,
    replay: RoadSequence,
    terrain: TerrainEvaluator,
    checkpoints: Vec<RoadCheckpoint>,
    blocks: Vec<RoadBlock>,
    buffer: Vec<RoadSegment>,
    buffer_first: u32,
    replay_end: u32,
    replay_work: usize,
    replay_ms: f64,
    peak_replay_ms: f64,
}
impl RoadIndex {
    pub fn new(sequence: RoadSequence, terrain: TerrainEvaluator) -> Self {
        assert_eq!(
            sequence.state.next_segment, 0,
            "road index requires a fresh sequence"
        );
        let mut checkpoints = Vec::with_capacity(BLOCKS + 1);
        checkpoints.push(sequence.checkpoint());
        Self {
            latest: None,
            replay: sequence.clone(),
            sequence,
            terrain,
            checkpoints,
            blocks: Vec::with_capacity(BLOCKS),
            buffer: Vec::with_capacity(ROAD_CHECKPOINT_INTERVAL as usize),
            buffer_first: 0,
            replay_end: 0,
            replay_work: 0,
            replay_ms: 0.,
            peak_replay_ms: 0.,
        }
    }

    /// Advance at most max_segments source segments. Return true at the finite source end.
    pub fn prepare(&mut self, max_segments: usize) -> bool {
        for _ in 0..max_segments {
            let Some(segment) = self.sequence.advance(&self.terrain) else {
                break;
            };
            self.latest = Some(segment);
            if segment.index % ROAD_CHECKPOINT_INTERVAL == 0 {
                self.blocks.push(RoadBlock {
                    first: segment.index,
                    end: segment.index,
                    bounds: empty_bounds(),
                });
            }
            let block = self.blocks.last_mut().unwrap();
            block.end = segment.index + 1;
            for a in 0..3 {
                block.bounds.min[a] = block.bounds.min[a].min(segment.bounds.min[a]);
                block.bounds.max[a] = block.bounds.max[a].max(segment.bounds.max[a]);
            }
            let next = self.sequence.state.next_segment;
            if next % ROAD_CHECKPOINT_INTERVAL == 0 || next == ROAD_SEGMENTS {
                self.checkpoints.push(self.sequence.checkpoint());
            }
        }
        self.sequence.state.next_segment == ROAD_SEGMENTS
    }

    pub fn latest_segment(&self) -> Option<RoadSegment> {
        self.latest
    }
    pub fn candidate_count(
        &self,
        bounds: WorldBounds,
        required: u32,
    ) -> (super::QueryStatus, usize) {
        let count = self
            .blocks
            .iter()
            .filter(|block| {
                (0..3).all(|a| {
                    block.bounds.min[a] <= bounds.max[a] && block.bounds.max[a] >= bounds.min[a]
                })
            })
            .map(|block| block.end.min(required).saturating_sub(block.first) as usize)
            .sum();
        (
            super::QueryStatus::for_prefix(self.coverage(), required),
            count,
        )
    }

    /// Return candidate segment identities from intersecting prepared blocks.
    /// Call segment to refine candidates against their segment bounds.
    pub fn segments_in(
        &self,
        bounds: WorldBounds,
        required: u32,
        output: &mut Vec<u32>,
    ) -> super::QueryStatus {
        output.clear();
        for block in &self.blocks {
            if (0..3).all(|a| {
                block.bounds.min[a] <= bounds.max[a] && block.bounds.max[a] >= bounds.min[a]
            }) {
                output.extend(block.first..block.end.min(required));
            }
        }
        super::QueryStatus::for_prefix(self.coverage(), required)
    }

    /// Advance replay by a bounded number of source segments.
    pub fn prepare_segment(
        &mut self,
        index: u32,
        limit: usize,
    ) -> Result<Option<RoadSegment>, ReferenceError> {
        self.replay_work = 0;
        self.replay_ms = 0.;
        if index >= self.sequence.state.next_segment {
            return Ok(None);
        }
        if (self.buffer_first..self.buffer_first + self.buffer.len() as u32).contains(&index) {
            return Ok(Some(self.buffer[(index - self.buffer_first) as usize]));
        }
        #[cfg(target_arch = "wasm32")]
        let replay_started = crate::host::game_now();
        #[cfg(not(target_arch = "wasm32"))]
        let replay_started = std::time::Instant::now();
        let block = self.blocks[(index / ROAD_CHECKPOINT_INTERVAL) as usize];
        if self.buffer_first != block.first || self.replay_end == 0 {
            self.replay
                .restore(&self.checkpoints[(index / ROAD_CHECKPOINT_INTERVAL) as usize]);
            self.buffer.clear();
            self.buffer_first = block.first;
            self.replay_end = block.first;
        }
        for _ in 0..limit {
            if self.replay_end > index {
                break;
            }
            let segment = self
                .replay
                .advance(&self.terrain)
                .ok_or_else(super::generation::invalid)?;
            if !(0..3).all(|a| {
                segment.bounds.min[a] >= block.bounds.min[a]
                    && segment.bounds.max[a] <= block.bounds.max[a]
            }) {
                self.buffer.clear();
                self.replay_end = 0;
                return Err(ReferenceError(format!(
                    "Saved road block {} bounds differ",
                    block.first
                )));
            }
            self.buffer.push(segment);
            self.replay_end += 1;
            self.replay_work += 1;
        }
        #[cfg(target_arch = "wasm32")]
        {
            self.replay_ms = crate::host::game_now() - replay_started;
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            self.replay_ms = replay_started.elapsed().as_secs_f64() * 1000.;
        }
        self.peak_replay_ms = self.peak_replay_ms.max(self.replay_ms);
        if self.replay_end == block.end {
            let expected = self
                .checkpoints
                .get((block.end / ROAD_CHECKPOINT_INTERVAL) as usize)
                .filter(|c| c.state.next_segment == block.end)
                .map(|c| c.state)
                .unwrap_or(self.sequence.state);
            let mut bounds = empty_bounds();
            for s in &self.buffer {
                for a in 0..3 {
                    bounds.min[a] = bounds.min[a].min(s.bounds.min[a]);
                    bounds.max[a] = bounds.max[a].max(s.bounds.max[a]);
                }
            }
            if self.replay.state != expected || bounds != block.bounds {
                self.buffer.clear();
                self.replay_end = 0;
                return Err(ReferenceError(format!(
                    "Saved road block {} checkpoint differs",
                    block.first
                )));
            }
        }
        Ok((self.replay_end > index).then(|| self.buffer[(index - self.buffer_first) as usize]))
    }

    /// Synchronous access for acceptance callers. Runtime jobs use prepare_segment.
    pub fn segment(&mut self, index: u32) -> Result<RoadSegment, ReferenceError> {
        self.prepare_segment(index, ROAD_CHECKPOINT_INTERVAL as usize)?
            .ok_or_else(|| ReferenceError(format!("Road segment {index} is not prepared")))
    }

    pub fn replay_ms(&self) -> f64 {
        self.replay_ms
    }
    pub fn peak_replay_ms(&self) -> f64 {
        self.peak_replay_ms
    }

    pub fn replay_work(&self) -> usize {
        self.replay_work
    }

    pub fn prepared_segments(&self) -> u32 {
        self.sequence.state.next_segment
    }
    pub fn checkpoint_count(&self) -> usize {
        self.checkpoints.len()
    }
    pub fn retained_index_bytes(&self) -> usize {
        self.checkpoints.capacity() * std::mem::size_of::<RoadCheckpoint>()
            + self.blocks.capacity() * std::mem::size_of::<RoadBlock>()
            + self.buffer.capacity() * std::mem::size_of::<RoadSegment>()
    }
}

fn runtime_point([x, y, z]: [f64; 3]) -> [f64; 3] {
    [x, y, -z]
}
fn empty_bounds() -> WorldBounds {
    WorldBounds {
        min: [f64::INFINITY; 3],
        max: [f64::NEG_INFINITY; 3],
    }
}

pub fn bounds_radius(reference: &WorldReference) -> Result<f64, ReferenceError> {
    let mut radius = 0.0_f64;
    for identity in &reference.road.prototypes {
        let prototype = reference
            .prototypes
            .iter()
            .find(|p| &p.source == identity)
            .ok_or_else(|| {
                ReferenceError(format!(
                    "Missing road prototype {}:{}",
                    identity.file, identity.path_id
                ))
            })?;
        for skin in &prototype.skins {
            for (bone, bind) in skin.bones.iter().zip(&skin.bind_poses) {
                for vertex in &skin.positions {
                    let local: [f64; 3] = std::array::from_fn(|a| {
                        bind[a] as f64 * vertex[0] as f64
                            + bind[4 + a] as f64 * vertex[1] as f64
                            + bind[8 + a] as f64 * vertex[2] as f64
                            + bind[12 + a] as f64
                    });
                    let mut reach = local.into_iter().map(|v| v * v).sum::<f64>().sqrt();
                    let mut node = Some(*bone);
                    while let Some(index) = node {
                        let frame = &prototype.nodes[index as usize];
                        let scale = frame
                            .transform
                            .scale
                            .into_iter()
                            .map(|v| (v as f64).abs())
                            .fold(0.0, f64::max);
                        let translation = frame
                            .transform
                            .translation
                            .into_iter()
                            .map(|v| (v as f64).powi(2))
                            .sum::<f64>()
                            .sqrt();
                        // Cover float quaternion length and transform rounding at each node.
                        reach = translation + reach * scale * 1.00001;
                        node = frame.parent;
                    }
                    radius = radius.max(reach);
                }
            }
        }
    }
    // Include the source control path even if a skin has no influence at its final control.
    radius = radius.max(reference.road.road_segment_length as f64);
    Ok(radius
        + super::placement::building_offset(&reference.buildings)
        + reference.road.road_above_terrain.abs() as f64)
}

fn turn_angle(error: f32, length: f32, limit: f32) -> f32 {
    let ratio = ((error as f64 / length as f64) as f32).clamp(-1.0, 1.0);
    let radians = (ratio as f64).asin() as f32;
    let degrees = (radians as f64 * 57.29578_f32 as f64) as f32;
    degrees.clamp(-limit, limit)
}

fn forward_step(rotation: [f32; 3], length: f32) -> [f32; 3] {
    let (sx, cx) = half_angle(rotation[0]);
    let (sy, cy) = half_angle(rotation[1]);
    // Source Euler order is Y * X * Z. The source road holds Z at zero.
    // Keep native f32 products before the managed vector overload's f64 locals.
    let x = (sx * cy) as f64;
    let y = (cx * sy) as f64;
    let z = (0.0_f32 - sx * sy) as f64;
    let w = (cx * cy) as f64;
    let x2 = x * 2.0;
    let y2 = y * 2.0;
    let z2 = z * 2.0;
    [
        ((x * z2 + w * y2) * length as f64) as f32,
        ((y * z2 - w * x2) * length as f64) as f32,
        ((1.0 - (x * x2 + y * y2)) * length as f64) as f32,
    ]
}

/// Retained bytes for one converted source prototype, before allocation.
pub fn road_prototype_bytes(prototype: &crate::reference::WorldPrototypeReference) -> usize {
    prototype.nodes.len() * std::mem::size_of::<road_mesh::RoadNode>()
        + prototype.controls.len() * std::mem::size_of::<u32>()
        + prototype.skins.len() * std::mem::size_of::<road_mesh::RoadSkin>()
        + prototype
            .skins
            .iter()
            .map(|s| {
                s.bones.len() * 4
                    + s.bind_poses.len() * 64
                    + s.positions.len() * 12
                    + s.normals.len() * 12
                    + s.tangents.len() * 16
                    + s.uv.len() * 8
                    + s.joints.len() * 16
                    + s.weights.len() * 16
                    + s.triangles.len() * std::mem::size_of::<Vec<[u32; 3]>>()
                    + s.triangles
                        .iter()
                        .map(|group| group.len() * 12)
                        .sum::<usize>()
            })
            .sum::<usize>()
}

pub fn road_prototype(
    prototype: &crate::reference::WorldPrototypeReference,
) -> road_mesh::RoadPrototype {
    road_mesh::RoadPrototype {
        nodes: prototype
            .nodes
            .iter()
            .map(|node| road_mesh::RoadNode {
                parent: node.parent,
                translation: node.transform.translation,
                rotation: node.transform.rotation,
                scale: node.transform.scale,
            })
            .collect(),
        controls: prototype.controls.clone(),
        skins: prototype
            .skins
            .iter()
            .map(|skin| road_mesh::RoadSkin {
                bones: skin.bones.clone(),
                bind_poses: skin.bind_poses.clone(),
                positions: skin.positions.clone(),
                normals: skin.normals.clone(),
                tangents: skin.tangents.clone(),
                uv: skin.uv.clone(),
                joints: skin.joints.clone(),
                weights: skin.weights.clone(),
                triangles: skin.triangles.clone(),
            })
            .collect(),
    }
}

#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RoadIndexSnapshot {
    pub state: RoadState,
    pub checkpoints: Vec<RoadCheckpoint>,
    pub blocks: Vec<RoadBlock>,
}
impl RoadIndex {
    pub fn coverage(&self) -> u32 {
        self.sequence.state.next_segment
    }
    pub fn capture_index(&self) -> RoadIndexSnapshot {
        RoadIndexSnapshot {
            state: self.sequence.state,
            checkpoints: self.checkpoints.clone(),
            blocks: self.blocks.clone(),
        }
    }
    pub fn validate_index(&self, s: &RoadIndexSnapshot) -> Result<(), ReferenceError> {
        let valid_state = |v: RoadState| {
            v.next_segment <= ROAD_SEGMENTS
                && v.distance == f64::from(v.next_segment) * 100.
                && v.source_position
                    .iter()
                    .all(|x| x.is_finite() && x.abs() <= super::MAX_ABSOLUTE_COORDINATE)
                && v.source_rotation
                    .iter()
                    .all(|x| x.is_finite() && x.abs() <= 90.)
        };
        let end = s.state.next_segment;
        let checkpoint_count = 1
            + (end / ROAD_CHECKPOINT_INTERVAL) as usize
            + usize::from(end == ROAD_SEGMENTS && end % ROAD_CHECKPOINT_INTERVAL != 0);
        if !valid_state(s.state)
            || s.blocks.len() != end.div_ceil(ROAD_CHECKPOINT_INTERVAL) as usize
            || s.checkpoints.len() != checkpoint_count
        {
            return Err(super::generation::invalid());
        }
        let initial = self.checkpoints[0];
        if s.checkpoints[0] != initial {
            return Err(super::generation::invalid());
        }
        for (i, c) in s.checkpoints.iter().enumerate() {
            if !valid_state(c.state)
                || c.state.next_segment != (i as u32 * ROAD_CHECKPOINT_INTERVAL).min(ROAD_SEGMENTS)
            {
                return Err(super::generation::invalid());
            }
        }
        for (i, b) in s.blocks.iter().enumerate() {
            if b.first != i as u32 * ROAD_CHECKPOINT_INTERVAL
                || b.end != (b.first + ROAD_CHECKPOINT_INTERVAL).min(end)
                || !super::generation::valid_bounds(b.bounds)
            {
                return Err(super::generation::invalid());
            }
        }
        if s.checkpoints
            .last()
            .is_some_and(|c| c.state.next_segment == end && c.state != s.state)
        {
            return Err(super::generation::invalid());
        }
        // Only the last block needs replay at restore. Earlier blocks remain saved state.
        if let Some(b) = s.blocks.last() {
            let mut replay = self.sequence.clone();
            replay.restore(&s.checkpoints[s.blocks.len() - 1]);
            let mut bounds = empty_bounds();
            for _ in b.first..b.end {
                let segment = replay
                    .advance(&self.terrain)
                    .ok_or_else(super::generation::invalid)?;
                for a in 0..3 {
                    bounds.min[a] = bounds.min[a].min(segment.bounds.min[a]);
                    bounds.max[a] = bounds.max[a].max(segment.bounds.max[a]);
                }
            }
            if replay.state != s.state || bounds != b.bounds {
                return Err(super::generation::invalid());
            }
        }
        Ok(())
    }
    pub fn restore_index(&mut self, s: &RoadIndexSnapshot) -> Result<(), ReferenceError> {
        self.validate_index(s)?;
        self.sequence.state = s.state;
        self.latest = None;
        self.checkpoints.clone_from(&s.checkpoints);
        self.blocks.clone_from(&s.blocks);
        self.buffer.clear();
        self.buffer_first = 0;
        self.replay_end = 0;
        self.replay_work = 0;
        Ok(())
    }
}
