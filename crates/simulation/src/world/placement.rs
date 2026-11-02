//! Source building selection, spatial replay, and physical supply locations.
use super::{
    AbsolutePose, GeneratedId, Placement, RandomState, RoadCurve, SourceRandom, TerrainEvaluator,
    WorldBounds, WorldLayer,
};
use crate::reference::{
    ColliderGeometry, ReferenceError, SourceId, Transform, WorldBuildingGroup,
    WorldBuildingReference, WorldPrototypeReference, WorldReference, WorldSourceRecord, WorldValue,
};
use std::{collections::HashMap, sync::Arc};

pub const BUILDING_CHECKPOINT_INTERVAL: u32 = 32;
const SOURCE_END: f64 = 5_000_000.0;

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct BuildingGroupState {
    pub available: Vec<i32>,
    pub current_available: Vec<i32>,
    pub current_count: i32,
    pub current_max_count: i32,
}

/// Boundary after a complete selection, including its following spacing draw.
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct BuildingCheckpoint {
    pub random: RandomState,
    pub groups: Vec<BuildingGroupState>,
    pub current_group: usize,
    pub previous_type: i32,
    pub next_distance: f64,
    pub next_building: u32,
    pub draws: u64,
}

#[derive(Clone, Copy, Debug, serde::Serialize, serde::Deserialize)]
pub struct SupplyFluid {
    pub fluid_type: i32,
    pub amount: f32,
}

/// Checked physical supply in the structure's converted local frame.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct SupplyDefinition {
    pub tank: SourceId,
    pub cap: SourceId,
    pub tank_pose: Transform,
    pub cap_pose: Transform,
    pub fluids: Vec<SupplyFluid>,
    pub capacity: f32,
    pub no_fill_from: bool,
    pub no_fill_this: bool,
    pub no_pour: bool,
    pub emission_min_rate: f32,
    pub emission_max_rate: f32,
    pub explosion: super::TankExplosion,
}

#[derive(Clone, Copy, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct SupplyPoint {
    pub building_index: u32,
    pub supply_index: usize,
    pub cap_pose: AbsolutePose,
    pub tank_pose: AbsolutePose,
    prototype_index: usize,
}

impl SupplyPoint {
    pub fn nested(tank_pose: AbsolutePose, cap_pose: AbsolutePose) -> Self {
        Self {
            building_index: 0,
            supply_index: 0,
            tank_pose,
            cap_pose,
            prototype_index: 0,
        }
    }
}
#[derive(Clone, Copy, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct FoundationPoint {
    pub building_index: u32,
    pub helper_index: usize,
    pub position: [f64; 3],
    /// Source squared inner and outer radii.
    pub range_squared: [f32; 2],
}

struct Helper {
    position: [f32; 3],
    range_squared: [f32; 2],
}
struct Prototype {
    source: SourceId,
    type_id: i32,
    scale: [f32; 3],
    radius: f64,
    helpers: Vec<Helper>,
    supplies: Vec<SupplyDefinition>,
}
struct Inputs {
    rules: WorldBuildingReference,
    prototypes: Vec<Prototype>,
    curve: RoadCurve,
    terrain: TerrainEvaluator,
    width: f32,
    zero_height: f32,
    road_height: f32,
    seed: i32,
    max_buildings: usize,
}

pub struct BuildingSequence {
    inputs: Arc<Inputs>,
    state: BuildingCheckpoint,
    random: SourceRandom,
}

impl BuildingSequence {
    pub fn new(reference: &WorldReference, seed: i32) -> Self {
        let inputs = Arc::new(Inputs::new(reference, seed));
        Self::from_inputs(inputs)
    }

    fn from_inputs(inputs: Arc<Inputs>) -> Self {
        // FStart seeds from mainscript, independently of the terrain seed override.
        let mut random = SourceRandom::new(inputs.seed);
        let distance = hundredth(
            &mut random,
            inputs.rules.min_start_dist,
            inputs.rules.max_start_dist,
        ) as f64
            * inputs.rules.dist_modifier as f64;
        let current_group = random.next_range(0, inputs.rules.groups.len() as i32) as usize;
        let mut groups: Vec<_> = inputs
            .rules
            .groups
            .iter()
            .map(|g| BuildingGroupState {
                available: g.available.clone(),
                current_available: g.current_available.clone(),
                current_count: g.current_count,
                current_max_count: g.current_max_count,
            })
            .collect();
        for (state, rule) in groups.iter_mut().zip(&inputs.rules.groups) {
            state.available.reserve(rule.types.len());
            state.current_available.reserve(rule.types.len());
        }
        let group = &inputs.rules.groups[current_group];
        groups[current_group].current_count = 0;
        groups[current_group].current_max_count =
            random.next_range(group.min_count, group.max_count);
        let state = BuildingCheckpoint {
            random: random.snapshot(),
            groups,
            current_group,
            previous_type: 0,
            next_distance: distance,
            next_building: 0,
            draws: 3,
        };
        Self {
            inputs,
            state,
            random,
        }
    }

    pub fn checkpoint(&self) -> BuildingCheckpoint {
        let mut state = self.state.clone();
        state.random = self.random.snapshot();
        state
    }

    /// Restore only checkpoints from the same checked inputs and seed.
    pub fn restore(&mut self, checkpoint: &BuildingCheckpoint) {
        assert_eq!(checkpoint.groups.len(), self.inputs.rules.groups.len());
        self.state.clone_from(checkpoint);
        self.random = SourceRandom::restore(checkpoint.random);
    }

    pub fn next(&mut self) -> Option<Placement> {
        let building = self.advance()?;
        Some(self.placement(building))
    }

    fn advance(&mut self) -> Option<Building> {
        if self.state.next_distance >= SOURCE_END {
            return None;
        }
        let inputs = &self.inputs;
        let state = &mut self.state;
        let group = &state.groups[state.current_group];
        if group.current_count >= group.current_max_count {
            state.current_group = (state.current_group + 1) % state.groups.len();
            let rule = &inputs.rules.groups[state.current_group];
            let group = &mut state.groups[state.current_group];
            group.current_count = 0;
            group.current_max_count = self.random.next_range(rule.min_count, rule.max_count);
            state.draws += 1;
        }
        let rule = &inputs.rules.groups[state.current_group];
        let group = &mut state.groups[state.current_group];
        let mut selected = select(rule, group, &mut self.random, &mut state.draws);
        if rule.types[selected].type_id == state.previous_type {
            // Exactly one extra selection; it also increments the source group count.
            selected = select(rule, group, &mut self.random, &mut state.draws);
        }
        let selection = &rule.types[selected];
        state.previous_type = selection.type_id;
        let side =
            &selection.sides[self.random.next_range(0, selection.sides.len() as i32) as usize];
        let distance = state.next_distance;
        // GenAll temporarily resets both world transforms and double origins to zero.
        let z = distance as f32;
        let ahead = (z as f64 + 30.0) as f32;
        let x = (inputs.curve.value(z) as f64 * inputs.width as f64) as f32;
        let next_x = (inputs.curve.value(ahead) as f64 * inputs.width as f64) as f32;
        let yaw = source_heading(
            (next_x as f64 - x as f64) as f32,
            (ahead as f64 - z as f64) as f32,
        );
        let offset = hundredth(&mut self.random, side.min_xpos, side.max_xpos);
        let q = yaw_rotation(yaw);
        let lateral = rotate(q, [offset, 0.0, 0.0]);
        let y = inputs.terrain.base_height([x, z], false) as f64 - inputs.zero_height as f64;
        let source_position = [
            x as f64 + lateral[0] as f64,
            inputs.road_height as f64 + y + lateral[1] as f64,
            distance + lateral[2] as f64,
        ];
        let yaw = (yaw as f64
            + hundredth(&mut self.random, side.min_y_angle, side.max_y_angle) as f64)
            as f32;
        let spacing = (hundredth(&mut self.random, selection.min_dist, selection.max_dist) as f64
            * inputs.rules.dist_modifier as f64) as f32;
        state.draws += 4;
        state.next_distance += spacing as f64;
        let index = state.next_building;
        state.next_building += 1;
        let prototype_index = inputs
            .prototypes
            .iter()
            .position(|p| p.type_id == selection.type_id)
            .unwrap();
        // PlaceOne restores the road origin. Reflect source Z only at this boundary.
        let position = [source_position[0], source_position[1], -source_position[2]];
        let q = yaw_rotation(yaw);
        let pose = AbsolutePose {
            position,
            rotation: [-q[0], -q[1], q[2], q[3]],
        };
        // buildingscript.FStart quantizes the placed float position to 20 m, ties to even.
        let object_seed = source_position.into_iter().fold(inputs.seed, |seed, v| {
            seed.wrapping_add((((v as f32 as f64 / 20.0).round_ties_even()) * 20.0) as i32)
        });
        Some(Building {
            index,
            prototype_index,
            pose,
            object_seed,
        })
    }

    fn placement(&self, building: Building) -> Placement {
        let p = &self.inputs.prototypes[building.prototype_index];
        Placement {
            id: GeneratedId {
                layer: WorldLayer::Building,
                generator: self.inputs.rules.source.clone(),
                unit: [building.index as i64, 0],
                ordinal: 0,
                children: Vec::new(),
            },
            prototype: p.source.clone(),
            pose: building.pose,
            scale: p.scale,
            bounds: building.bounds(p.radius),
            object_seed: building.object_seed,
        }
    }
}

#[derive(Clone, Copy)]
struct Building {
    index: u32,
    prototype_index: usize,
    pose: AbsolutePose,
    object_seed: i32,
}
impl Building {
    fn bounds(self, radius: f64) -> WorldBounds {
        WorldBounds {
            min: self.pose.position.map(|v| v - radius),
            max: self.pose.position.map(|v| v + radius),
        }
    }
}

#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
struct Block {
    checkpoint: BuildingCheckpoint,
    end: u32,
    bounds: WorldBounds,
    helpers: std::ops::Range<usize>,
    supplies: std::ops::Range<usize>,
}

pub struct PlacementIndex {
    sequence: BuildingSequence,
    replay: BuildingSequence,
    blocks: Vec<Block>,
    foundations: Vec<FoundationPoint>,
    supplies: Vec<SupplyPoint>,
    checked_blocks: Vec<bool>,
}
impl PlacementIndex {
    pub fn new(reference: &WorldReference, seed: i32) -> Self {
        let sequence = BuildingSequence::new(reference, seed);
        let inputs = &sequence.inputs;
        let count = inputs.max_buildings;
        let max_helpers = inputs
            .prototypes
            .iter()
            .map(|p| p.helpers.len())
            .max()
            .unwrap();
        let max_supplies = inputs
            .prototypes
            .iter()
            .map(|p| p.supplies.len())
            .max()
            .unwrap();
        let replay = BuildingSequence::from_inputs(Arc::clone(inputs));
        Self {
            sequence,
            replay,
            blocks: Vec::with_capacity(count.div_ceil(BUILDING_CHECKPOINT_INTERVAL as usize)),
            checked_blocks: Vec::with_capacity(
                count.div_ceil(BUILDING_CHECKPOINT_INTERVAL as usize),
            ),
            foundations: Vec::with_capacity(count * max_helpers),
            supplies: Vec::with_capacity(count * max_supplies),
        }
    }

    /// Prepare at most this many selections. Queries use only the prepared region.
    pub fn prepare(&mut self, max_buildings: usize) -> bool {
        for _ in 0..max_buildings {
            if self.sequence.state.next_distance >= SOURCE_END {
                break;
            }
            if self.sequence.state.next_building % BUILDING_CHECKPOINT_INTERVAL == 0 {
                self.checked_blocks.push(true);
                self.blocks.push(Block {
                    checkpoint: self.sequence.checkpoint(),
                    end: self.sequence.state.next_building,
                    bounds: WorldBounds {
                        min: [f64::INFINITY; 3],
                        max: [f64::NEG_INFINITY; 3],
                    },
                    helpers: self.foundations.len()..self.foundations.len(),
                    supplies: self.supplies.len()..self.supplies.len(),
                });
            }
            let building = self.sequence.advance().unwrap();
            let p = &self.sequence.inputs.prototypes[building.prototype_index];
            let block = self.blocks.last_mut().unwrap();
            extend(&mut block.bounds, building.bounds(p.radius));
            for (helper_index, helper) in p.helpers.iter().enumerate() {
                let position = place_point(building.pose, p.scale, helper.position);
                let radius = helper.range_squared[0].max(helper.range_squared[1]).sqrt() as f64;
                // Helper influence is horizontal and can change terrain at any raw height.
                extend(
                    &mut block.bounds,
                    WorldBounds {
                        min: position.map(|v| v - radius),
                        max: position.map(|v| v + radius),
                    },
                );
                self.foundations.push(FoundationPoint {
                    building_index: building.index,
                    helper_index,
                    position,
                    range_squared: helper.range_squared,
                });
            }
            for (supply_index, supply) in p.supplies.iter().enumerate() {
                self.supplies.push(SupplyPoint {
                    building_index: building.index,
                    supply_index,
                    cap_pose: place_pose(building.pose, p.scale, supply.cap_pose),
                    tank_pose: place_pose(building.pose, p.scale, supply.tank_pose),
                    prototype_index: building.prototype_index,
                });
            }
            block.end = building.index + 1;
            block.helpers.end = self.foundations.len();
            block.supplies.end = self.supplies.len();
        }
        self.sequence.state.next_distance >= SOURCE_END
    }

    /// Refresh source membership without cloning unchanged placement identities.
    pub fn update_buildings_in(
        &mut self,
        bounds: WorldBounds,
        required: u32,
        output: &mut Vec<Placement>,
        accept: impl Fn([f64; 3]) -> bool,
    ) -> Result<super::QueryStatus, ReferenceError> {
        self.check_blocks(Some(bounds), false, required)?;
        output.retain(|p| p.id.unit[0] < i64::from(required) && accept(p.pose.position));
        for block in &self.blocks {
            if !intersects(block.bounds, bounds, false) {
                continue;
            }
            self.replay.restore(&block.checkpoint);
            for _ in block.checkpoint.next_building..block.end.min(required) {
                let building = self.replay.advance().unwrap();
                if accept(building.pose.position)
                    && !output.iter().any(|p| p.id.unit[0] == building.index as i64)
                {
                    output.push(self.replay.placement(building));
                }
            }
        }
        Ok(super::QueryStatus::for_prefix(self.coverage(), required))
    }

    pub fn foundation_count(&self) -> usize {
        self.foundations.len()
    }

    pub fn foundations_in(
        &mut self,
        bounds: WorldBounds,
        required: u32,
        output: &mut Vec<FoundationPoint>,
    ) -> Result<super::QueryStatus, ReferenceError> {
        self.check_blocks(Some(bounds), true, required)?;
        output.clear();
        for block in &self.blocks {
            if !intersects(block.bounds, bounds, true) {
                continue;
            }
            for point in &self.foundations[block.helpers.clone()] {
                let r = point.range_squared[0].max(point.range_squared[1]).sqrt() as f64;
                let influence = WorldBounds {
                    min: point.position.map(|v| v - r),
                    max: point.position.map(|v| v + r),
                };
                if point.building_index < required && intersects(influence, bounds, true) {
                    output.push(*point);
                }
            }
        }
        Ok(super::QueryStatus::for_prefix(self.coverage(), required))
    }

    pub fn supplies_for_building(
        &mut self,
        index: u32,
        output: &mut Vec<SupplyPoint>,
    ) -> Result<super::QueryStatus, ReferenceError> {
        let required = index
            .checked_add(1)
            .ok_or_else(super::generation::invalid)?;
        self.check_blocks(None, false, required)?;
        output.clear();
        output.extend(
            self.supplies
                .iter()
                .filter(|point| point.building_index == index)
                .copied(),
        );
        Ok(super::QueryStatus::for_prefix(self.coverage(), required))
    }

    pub fn nearest_supply_point(
        &mut self,
        point: [f64; 3],
        required: u32,
    ) -> Result<(super::QueryStatus, Option<SupplyPoint>), ReferenceError> {
        self.check_blocks(None, false, required)?;
        Ok((
            super::QueryStatus::for_prefix(self.coverage(), required),
            self.supplies
                .iter()
                .filter(|p| p.building_index < required)
                .min_by(|a, b| {
                    distance_squared(a.cap_pose.position, point)
                        .total_cmp(&distance_squared(b.cap_pose.position, point))
                })
                .copied(),
        ))
    }

    pub fn supply_definition(&self, point: &SupplyPoint) -> &SupplyDefinition {
        &self.sequence.inputs.prototypes[point.prototype_index].supplies[point.supply_index]
    }

    pub fn prepared_buildings(&self) -> u32 {
        self.sequence.state.next_building
    }
    pub fn checkpoint_count(&self) -> usize {
        self.blocks.len()
    }
    pub fn retained_index_bytes(&self) -> usize {
        self.checked_blocks.capacity() * std::mem::size_of::<bool>()
            + self.blocks.capacity() * std::mem::size_of::<Block>()
            + self.foundations.capacity() * std::mem::size_of::<FoundationPoint>()
            + self.supplies.capacity() * std::mem::size_of::<SupplyPoint>()
            + self
                .blocks
                .iter()
                .map(|b| checkpoint_heap(&b.checkpoint))
                .sum::<usize>()
            + checkpoint_heap(&self.sequence.state)
            + checkpoint_heap(&self.replay.state)
    }
}

fn checkpoint_heap(state: &BuildingCheckpoint) -> usize {
    state.groups.capacity() * std::mem::size_of::<BuildingGroupState>()
        + state
            .groups
            .iter()
            .map(|g| {
                (g.available.capacity() + g.current_available.capacity())
                    * std::mem::size_of::<i32>()
            })
            .sum::<usize>()
}
fn select(
    rule: &WorldBuildingGroup,
    state: &mut BuildingGroupState,
    random: &mut SourceRandom,
    draws: &mut u64,
) -> usize {
    state.current_count += 1;
    if state.current_available.is_empty() {
        if state.available.is_empty() {
            state.available.extend(0..rule.types.len() as i32);
        }
        for _ in 0..state.current_max_count {
            if state.available.is_empty() {
                break;
            }
            let index = random.next_range(0, state.available.len() as i32) as usize;
            *draws += 1;
            state.current_available.push(state.available.remove(index));
        }
    }
    let index = random.next_range(0, state.current_available.len() as i32) as usize;
    *draws += 1;
    state.current_available.remove(index) as usize
}
fn hundredth(random: &mut SourceRandom, min: f32, max: f32) -> f32 {
    let min = (min as f64 * 100.0) as i32;
    let max = (max as f64 * 100.0) as i32;
    (random.next_range(min, max) as f32 as f64 * 0.01_f32 as f64) as f32
}
fn source_heading(x: f32, z: f32) -> f32 {
    let square = (x as f64 * x as f64 + z as f64 * z as f64) as f32;
    let denominator = (square as f64).sqrt() as f32;
    if denominator < 1E-15 {
        return 0.0;
    }
    let ratio = (z as f64 / denominator as f64).clamp(-1.0, 1.0) as f32;
    let angle = ((ratio as f64).acos() as f32 as f64 * 57.29578_f32 as f64) as f32;
    if x <= 0.0 { -angle } else { angle }
}
fn yaw_rotation(yaw: f32) -> [f32; 4] {
    let radians = (yaw as f64 * 0.0174532924_f32 as f64) as f32;
    let angle = (radians * 0.5) as f64;
    [0.0, angle.sin() as f32, 0.0, angle.cos() as f32]
}
pub fn rotate(q: [f32; 4], p: [f32; 3]) -> [f32; 3] {
    let [x, y, z, w] = q.map(|v| v as f64);
    let [a, b, c] = p.map(|v| v as f64);
    [
        ((1.0 - 2.0 * (y * y + z * z)) * a + 2.0 * (x * y - w * z) * b + 2.0 * (x * z + w * y) * c)
            as f32,
        (2.0 * (x * y + w * z) * a + (1.0 - 2.0 * (x * x + z * z)) * b + 2.0 * (y * z - w * x) * c)
            as f32,
        (2.0 * (x * z - w * y) * a + 2.0 * (y * z + w * x) * b + (1.0 - 2.0 * (x * x + y * y)) * c)
            as f32,
    ]
}
pub fn multiply(a: [f32; 4], b: [f32; 4]) -> [f32; 4] {
    let [x, y, z, w] = a.map(|v| v as f64);
    let [i, j, k, l] = b.map(|v| v as f64);
    [
        (w * i + x * l + y * k - z * j) as f32,
        (w * j + y * l + z * i - x * k) as f32,
        (w * k + z * l + x * j - y * i) as f32,
        (w * l - x * i - y * j - z * k) as f32,
    ]
}
pub fn place_point(pose: AbsolutePose, scale: [f32; 3], point: [f32; 3]) -> [f64; 3] {
    let local = rotate(
        pose.rotation,
        std::array::from_fn(|a| (point[a] as f64 * scale[a] as f64) as f32),
    );
    std::array::from_fn(|a| pose.position[a] + local[a] as f64)
}
pub fn place_pose(pose: AbsolutePose, scale: [f32; 3], local: Transform) -> AbsolutePose {
    AbsolutePose {
        position: place_point(pose, scale, local.translation),
        rotation: multiply(pose.rotation, local.rotation),
    }
}
fn extend(bounds: &mut WorldBounds, other: WorldBounds) {
    for a in 0..3 {
        bounds.min[a] = bounds.min[a].min(other.min[a]);
        bounds.max[a] = bounds.max[a].max(other.max[a]);
    }
}
fn intersects(a: WorldBounds, b: WorldBounds, horizontal: bool) -> bool {
    (0..3)
        .filter(|axis| !horizontal || *axis != 1)
        .all(|i| a.min[i] <= b.max[i] && a.max[i] >= b.min[i])
}
fn distance_squared(a: [f64; 3], b: [f64; 3]) -> f64 {
    (0..3).map(|i| (a[i] - b[i]).powi(2)).sum()
}

impl Inputs {
    fn new(reference: &WorldReference, seed: i32) -> Self {
        assert_eq!(
            reference.generation_frame.translation, [0.0; 3],
            "unsupported building generation origin"
        );
        assert_eq!(
            reference.generation_frame.rotation,
            [0.0, 0.0, 0.0, 1.0],
            "unsupported building generation axes"
        );
        assert_eq!(
            reference.generation_frame.scale, [1.0; 3],
            "unsupported building generation scale"
        );
        assert_eq!(
            reference.road.length as f64, SOURCE_END,
            "unsupported building extent"
        );
        assert_eq!(reference.road.aliign_road, 0);
        assert_eq!(reference.road.align_buildings, 0);
        let mut rules = reference.buildings.clone();
        assert!(!rules.groups.is_empty());
        for group in &mut rules.groups {
            assert!(!group.types.is_empty() && group.min_count > 0);
            for selection in &mut group.types {
                if selection.dont_use_default == 0 && selection.sides.is_empty() {
                    selection.sides = rules.default_sides.clone();
                    if selection.use_modifier != 0 {
                        for side in &mut selection.sides {
                            side.min_xpos =
                                (side.min_xpos as f64 * selection.x_dist_modifier as f64) as f32;
                            side.max_xpos =
                                (side.max_xpos as f64 * selection.x_dist_modifier as f64) as f32;
                        }
                    }
                }
                assert!(
                    !selection.sides.is_empty(),
                    "source building selection has no sides"
                );
            }
        }
        let records: HashMap<_, _> = reference.records.iter().map(|r| (&r.source, r)).collect();
        let prototypes = rules
            .types
            .iter()
            .map(|kind| {
                let prototype = reference
                    .prototypes
                    .iter()
                    .find(|p| p.source == kind.prototype)
                    .unwrap();
                copy_prototype(prototype, kind.type_id, &records)
            })
            .collect();
        let minimum = building_spacing(&rules);
        assert!(minimum > 0.0 && rules.min_start_dist >= 0.0);
        let max_buildings = (SOURCE_END / minimum as f64).ceil() as usize + 1;
        let terrain = super::terrain_evaluator(reference, seed);
        let zero_height = terrain.base_height([0.0; 2], false);
        let road_height =
            terrain.base_height([0.0; 2], reference.road.align_edge_of_the_world != 0);
        Self {
            rules,
            prototypes,
            curve: RoadCurve::new(reference, seed),
            terrain,
            width: reference.road.width,
            zero_height,
            road_height,
            seed,
            max_buildings,
        }
    }
}

/// Smallest source z spacing between consecutive buildings.
pub fn building_spacing(rules: &WorldBuildingReference) -> f32 {
    rules
        .groups
        .iter()
        .flat_map(|g| &g.types)
        .map(|s| {
            // Bound with the actual source integer quantization and float spacing store.
            let hundredths = (s.min_dist as f64 * 100.0) as i32;
            ((hundredths as f32 as f64 * 0.01_f32 as f64) as f32 as f64
                * rules.dist_modifier as f64) as f32
        })
        .fold(f32::INFINITY, f32::min)
}
/// Largest lateral building offset from the road curve.
pub fn building_offset(rules: &WorldBuildingReference) -> f64 {
    let extent = |side: &crate::reference::WorldBuildingSide| {
        (side.min_xpos.abs().max(side.max_xpos.abs())) as f64
    };
    let mut offset = rules.default_sides.iter().map(extent).fold(0.0, f64::max);
    for group in &rules.groups {
        for selection in &group.types {
            let sides = if selection.dont_use_default == 0 {
                &rules.default_sides
            } else {
                &selection.sides
            };
            let modifier = if selection.dont_use_default == 0 && selection.use_modifier != 0 {
                selection.x_dist_modifier.abs() as f64
            } else {
                1.0
            };
            for side in sides {
                offset = offset.max(extent(side) * modifier);
            }
        }
    }
    offset
}

type Records<'a> = HashMap<&'a SourceId, &'a WorldSourceRecord>;
fn record<'a>(records: &Records<'a>, id: &SourceId) -> &'a WorldSourceRecord {
    records
        .get(id)
        .copied()
        .unwrap_or_else(|| panic!("missing checked source {}:{}", id.file, id.path_id))
}
fn link(value: &WorldValue) -> &SourceId {
    match value {
        WorldValue::Link(id) => id,
        _ => panic!("expected checked source link"),
    }
}
fn array(value: &WorldValue) -> &[WorldValue] {
    match value {
        WorldValue::Array(v) => v,
        _ => panic!("expected checked source array"),
    }
}
fn fields(value: &WorldValue) -> &crate::reference::WorldFields {
    match value {
        WorldValue::Record(v) => v,
        _ => panic!("expected checked source record"),
    }
}
fn number(value: &WorldValue) -> f32 {
    match value {
        WorldValue::Number(v) => *v as f32,
        WorldValue::Integer(v) => *v as f32,
        WorldValue::Bool(v) => {
            if *v {
                1.0
            } else {
                0.0
            }
        }
        _ => panic!("expected checked source number"),
    }
}
fn vector(value: &WorldValue) -> [f32; 3] {
    let f = fields(value);
    [number(&f["x"]), number(&f["y"]), number(&f["z"])]
}
pub fn active(prototype: &WorldPrototypeReference, index: usize) -> bool {
    let mut node = Some(index);
    while let Some(i) = node {
        let n = &prototype.nodes[i];
        // Instantiate activates the selected root; child activeSelf is preserved.
        if n.parent.is_some() && !n.active {
            return false;
        }
        node = n.parent.map(|p| p as usize);
    }
    true
}
pub fn node_pose(prototype: &WorldPrototypeReference, index: usize) -> Transform {
    let mut result = Transform {
        translation: [0.0; 3],
        rotation: [0.0, 0.0, 0.0, 1.0],
        scale: [1.0; 3],
    };
    let mut node = Some(index);
    while let Some(i) = node {
        let n = &prototype.nodes[i];
        let t = n.transform;
        let moved = rotate(
            t.rotation,
            std::array::from_fn(|a| (result.translation[a] as f64 * t.scale[a] as f64) as f32),
        );
        result.translation =
            std::array::from_fn(|a| (t.translation[a] as f64 + moved[a] as f64) as f32);
        result.rotation = multiply(t.rotation, result.rotation);
        result.scale = std::array::from_fn(|a| (result.scale[a] as f64 * t.scale[a] as f64) as f32);
        node = n.parent.map(|p| p as usize);
    }
    result
}
/// Node `index` in the frame of node `frame` of the same prototype. Scale is dropped.
pub fn node_frame(
    prototype: &WorldPrototypeReference,
    frame: usize,
    index: usize,
) -> crate::vehicle::VehiclePose {
    let pose = |i| {
        let t = node_pose(prototype, i);
        crate::vehicle::to_rapier(crate::vehicle::VehiclePose {
            position: t.translation,
            rotation: t.rotation,
        })
    };
    crate::vehicle::from_rapier(&(pose(frame).inverse() * pose(index)))
}
pub fn node_point(
    prototype: &WorldPrototypeReference,
    index: usize,
    mut point: [f32; 3],
) -> [f32; 3] {
    let mut node = Some(index);
    while let Some(i) = node {
        let n = &prototype.nodes[i];
        let t = n.transform;
        let moved = rotate(
            t.rotation,
            std::array::from_fn(|a| (point[a] as f64 * t.scale[a] as f64) as f32),
        );
        point = std::array::from_fn(|a| (t.translation[a] as f64 + moved[a] as f64) as f32);
        node = n.parent.map(|p| p as usize);
    }
    point
}
fn copy_prototype(
    prototype: &WorldPrototypeReference,
    type_id: i32,
    records: &Records<'_>,
) -> Prototype {
    let mut helpers = Vec::new();
    let mut supplies = Vec::new();
    let mut radius = 0.0_f64;
    let mut components = Vec::new();
    for (index, node) in prototype.nodes.iter().enumerate() {
        let transform = record(records, &node.source);
        let object = record(records, link(&transform.fields["m_GameObject"]));
        for source in object.components() {
            components.push((index, record(records, source)));
        }
    }
    for &(index, component) in &components {
        if component.class_name == "terrainHeightAlignToBuildingScript" && active(prototype, index)
        {
            let coords = array(&component.fields["helpPosCoords"]);
            let ranges = array(&component.fields["range2"]);
            for (i, coord) in coords.iter().enumerate() {
                let source = link(coord);
                let index = prototype
                    .nodes
                    .iter()
                    .position(|n| &n.source == source)
                    .unwrap();
                let scale = prototype.nodes[index].transform.scale[0];
                let factor = if ranges.len() < coords.len() {
                    0.0
                } else {
                    number(&ranges[i])
                };
                let factor = if factor == 0.0 { 4.0 } else { factor.max(0.0) };
                let inner = (scale as f64 * 0.5) as f32;
                let outer = (scale as f64 * factor as f64 * 0.5) as f32;
                helpers.push(Helper {
                    position: node_point(prototype, index, [0.0; 3]),
                    range_squared: [(inner as f64).powi(2) as f32, (outer as f64).powi(2) as f32],
                });
            }
        }
        if component.class_name == "LODGroup" {
            let source = vector(&component.fields["m_LocalReferencePoint"]);
            let center = node_point(prototype, index, [source[0], source[1], -source[2]]);
            // m_Size encloses the group's renderer bounds. Sphere encloses that cube.
            let reach = number(&component.fields["m_Size"]) as f64 * 3.0_f64.sqrt() * 0.5;
            let scale = node_scale_bound(prototype, index);
            radius = radius.max(norm(center) + reach * scale);
        }
        if component.class_name != "tankcapscript"
            || !active(prototype, index)
            || number(&component.fields["m_Enabled"]) == 0.0
        {
            continue;
        }
        let tank_id = link(&component.fields["Tank"]);
        let tank = record(records, tank_id);
        assert_eq!(tank.class_name, "tankscript");
        let tank_object = link(&tank.fields["m_GameObject"]);
        let tank_index = prototype
            .nodes
            .iter()
            .position(|n| link(&record(records, &n.source).fields["m_GameObject"]) == tank_object)
            .unwrap();
        if !active(prototype, tank_index) || number(&tank.fields["m_Enabled"]) == 0.0 {
            continue;
        }
        let container = fields(&tank.fields["F"]);
        let fluids: Vec<_> = array(&container["fluids"])
            .iter()
            .map(|v| {
                let f = fields(v);
                SupplyFluid {
                    fluid_type: number(&f["type"]) as i32,
                    amount: number(&f["amount"]),
                }
            })
            .collect();
        let no_fill_from = number(&tank.fields["noFillFrom"]) != 0.0;
        let no_pour = number(&component.fields["noPour"]) != 0.0;
        if no_fill_from
            || no_pour
            || !fluids
                .iter()
                .any(|f| f.amount > 0.0 && (0..=5).contains(&f.fluid_type))
        {
            continue;
        }
        supplies.push(SupplyDefinition {
            tank: tank.source.clone(),
            cap: component.source.clone(),
            tank_pose: node_pose(prototype, tank_index),
            cap_pose: node_pose(prototype, index),
            fluids,
            capacity: number(&container["maxC"]),
            no_fill_from,
            no_fill_this: number(&tank.fields["noFillThis"]) != 0.0,
            no_pour,
            emission_min_rate: number(&component.fields["minRate"]),
            emission_max_rate: number(&component.fields["maxRate"]),
            explosion: super::TankExplosion::read(&tank.fields, prototype)
                .expect("checked tankscript explosion fields"),
        });
    }
    for collider in &prototype.colliders {
        if let Some(geometry) = &collider.geometry {
            let mut reach = geometry_radius(geometry);
            if let Some(body) = collider.body {
                let pose = prototype.bodies[body as usize].pose;
                reach = norm(pose.translation)
                    + reach
                        * pose
                            .scale
                            .map(|v| v.abs() as f64)
                            .into_iter()
                            .fold(0.0, f64::max);
            }
            radius = radius.max(reach);
        }
    }
    // Include helper and supply anchors even when their renderer is disabled.
    for helper in &helpers {
        radius = radius.max(norm(helper.position));
    }
    for supply in &supplies {
        radius = radius
            .max(norm(supply.cap_pose.translation))
            .max(norm(supply.tank_pose.translation));
    }
    let scale = prototype.prefab_pose.scale;
    radius *= scale
        .map(|v| v.abs() as f64)
        .into_iter()
        .fold(0.0, f64::max);
    // Conservatively cover accumulated float transform rounding without changing placements.
    radius = radius * 1.0001 + 0.01;
    Prototype {
        source: prototype.source.clone(),
        type_id,
        scale,
        radius,
        helpers,
        supplies,
    }
}
fn norm(point: [f32; 3]) -> f64 {
    point
        .into_iter()
        .map(|v| (v as f64).powi(2))
        .sum::<f64>()
        .sqrt()
}
fn node_scale_bound(prototype: &WorldPrototypeReference, index: usize) -> f64 {
    let mut node = Some(index);
    let mut scale = 1.0;
    while let Some(i) = node {
        let n = &prototype.nodes[i];
        scale *= n
            .transform
            .scale
            .into_iter()
            .map(|v| v.abs() as f64)
            .fold(0.0, f64::max)
            * 1.00001;
        node = n.parent.map(|p| p as usize);
    }
    scale
}
fn geometry_radius(geometry: &ColliderGeometry) -> f64 {
    match geometry {
        ColliderGeometry::Box {
            center,
            half_extents,
            ..
        } => norm(*center) + norm(*half_extents),
        ColliderGeometry::Sphere { center, radius } => norm(*center) + *radius as f64,
        ColliderGeometry::Capsule { a, b, radius } => norm(*a).max(norm(*b)) + *radius as f64,
        ColliderGeometry::Convex { points } | ColliderGeometry::Trimesh { points, .. } => {
            points.iter().map(|p| norm(*p)).fold(0.0, f64::max)
        }
    }
}

pub fn prototype_radius(reference: &WorldReference, prototype: &WorldPrototypeReference) -> f64 {
    let records = reference.records.iter().map(|r| (&r.source, r)).collect();
    bound_prototype(prototype, &records)
}

pub fn prototype_radii(reference: &WorldReference) -> std::collections::HashMap<SourceId, f64> {
    let records = reference.records.iter().map(|r| (&r.source, r)).collect();
    reference
        .prototypes
        .iter()
        .map(|p| (p.source.clone(), bound_prototype(p, &records)))
        .collect()
}

fn bound_prototype(prototype: &WorldPrototypeReference, records: &Records<'_>) -> f64 {
    let mut radius = 0.0f64;
    for collider in &prototype.colliders {
        if let Some(geometry) = &collider.geometry {
            let mut reach = geometry_radius(geometry);
            if let Some(body) = collider.body {
                let pose = prototype.bodies[body as usize].pose;
                reach += norm(pose.translation);
            }
            radius = radius.max(reach);
        }
    }
    for (i, node) in prototype.nodes.iter().enumerate() {
        let object = record(
            records,
            link(&record(records, &node.source).fields["m_GameObject"]),
        );
        for component in object.components() {
            let r = record(records, component);
            if r.class_name == "LODGroup" {
                let source = vector(&r.fields["m_LocalReferencePoint"]);
                let center = node_point(prototype, i, [source[0], source[1], -source[2]]);
                radius = radius.max(
                    norm(center)
                        + number(&r.fields["m_Size"]) as f64
                            * 3.0f64.sqrt()
                            * 0.5
                            * node_scale_bound(prototype, i),
                );
            }
        }
    }
    radius
        * prototype
            .prefab_pose
            .scale
            .iter()
            .map(|v| v.abs() as f64)
            .fold(0., f64::max)
        * 1.0001
        + 0.01
}

#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PlacementIndexSnapshot {
    pub state: BuildingCheckpoint,
    blocks: Vec<Block>,
    foundations: Vec<FoundationPoint>,
    supplies: Vec<SupplyPoint>,
}
impl PlacementIndex {
    pub fn coverage(&self) -> u32 {
        self.sequence.state.next_building
    }
    pub fn capture_index(&self) -> PlacementIndexSnapshot {
        PlacementIndexSnapshot {
            state: self.sequence.checkpoint(),
            blocks: self.blocks.clone(),
            foundations: self.foundations.clone(),
            supplies: self.supplies.clone(),
        }
    }
    fn valid_checkpoint(&self, c: &BuildingCheckpoint) -> bool {
        let rules = &self.sequence.inputs.rules;
        c.next_building as usize <= self.sequence.inputs.max_buildings
            && c.next_distance.is_finite()
            && c.next_distance >= 0.
            && c.next_distance <= SOURCE_END * 2.
            && c.draws >= 3
            && c.draws <= 1_000_000_000
            && c.random.next < 56
            && c.random.paired < 56
            && c.random.values.iter().all(|v| *v >= 0 && *v < i32::MAX)
            && c.groups.len() == rules.groups.len()
            && c.current_group < c.groups.len()
            && c.groups.iter().zip(&rules.groups).all(|(g, r)| {
                let pool = |v: &Vec<i32>| {
                    v.len() <= r.types.len()
                        && v.iter().enumerate().all(|(i, x)| {
                            *x >= 0 && (*x as usize) < r.types.len() && !v[..i].contains(x)
                        })
                };
                pool(&g.available)
                    && pool(&g.current_available)
                    && g.available.iter().all(|x| !g.current_available.contains(x))
                    && g.current_count >= 0
                    && g.current_count <= r.max_count + 1
                    && g.current_max_count >= 0
                    && g.current_max_count <= r.max_count
            })
    }
    pub fn validate_index(&self, s: &PlacementIndexSnapshot) -> Result<(), ReferenceError> {
        let invalid = super::generation::invalid;
        let end = s.state.next_building;
        if !self.valid_checkpoint(&s.state)
            || s.blocks.len() != end.div_ceil(BUILDING_CHECKPOINT_INTERVAL) as usize
        {
            return Err(invalid());
        }
        let max_helpers = self
            .sequence
            .inputs
            .prototypes
            .iter()
            .map(|p| p.helpers.len())
            .max()
            .unwrap_or(0);
        let max_supplies = self
            .sequence
            .inputs
            .prototypes
            .iter()
            .map(|p| p.supplies.len())
            .max()
            .unwrap_or(0);
        if s.foundations.len() > end as usize * max_helpers
            || s.supplies.len() > end as usize * max_supplies
        {
            return Err(invalid());
        }
        let initial = BuildingSequence::from_inputs(Arc::clone(&self.sequence.inputs)).checkpoint();
        if s.blocks.first().map(|b| &b.checkpoint).unwrap_or(&s.state) != &initial {
            return Err(invalid());
        }
        let mut helpers = 0;
        let mut supplies = 0;
        let mut distance = 0.;
        let mut draws = 0;
        for (i, b) in s.blocks.iter().enumerate() {
            if !self.valid_checkpoint(&b.checkpoint)
                || b.checkpoint.next_distance >= SOURCE_END
                || b.checkpoint.next_building != i as u32 * BUILDING_CHECKPOINT_INTERVAL
                || b.end != (b.checkpoint.next_building + BUILDING_CHECKPOINT_INTERVAL).min(end)
                || !super::generation::valid_bounds(b.bounds)
                || b.checkpoint.next_distance < distance
                || b.checkpoint.draws < draws
                || b.helpers.start != helpers
                || b.helpers.end < b.helpers.start
                || b.helpers.end > s.foundations.len()
                || b.supplies.start != supplies
                || b.supplies.end < b.supplies.start
                || b.supplies.end > s.supplies.len()
            {
                return Err(invalid());
            }
            for p in &s.foundations[b.helpers.clone()] {
                if !(b.checkpoint.next_building..b.end).contains(&p.building_index)
                    || p.helper_index >= max_helpers
                    || !p.position.iter().all(|x| x.is_finite())
                    || !p.range_squared.iter().all(|x| x.is_finite() && *x >= 0.)
                {
                    return Err(invalid());
                }
            }
            for p in &s.supplies[b.supplies.clone()] {
                if !(b.checkpoint.next_building..b.end).contains(&p.building_index)
                    || p.prototype_index >= self.sequence.inputs.prototypes.len()
                    || p.supply_index
                        >= self.sequence.inputs.prototypes[p.prototype_index]
                            .supplies
                            .len()
                    || ![p.cap_pose, p.tank_pose].iter().all(|p| {
                        p.position.iter().all(|x| x.is_finite())
                            && p.rotation.iter().all(|x| x.is_finite())
                    })
                {
                    return Err(invalid());
                }
            }
            helpers = b.helpers.end;
            supplies = b.supplies.end;
            distance = b.checkpoint.next_distance;
            draws = b.checkpoint.draws;
        }
        if helpers != s.foundations.len()
            || supplies != s.supplies.len()
            || s.state.next_distance < distance
            || s.state.draws < draws
        {
            return Err(invalid());
        }
        if let Some(b) = s.blocks.last() {
            let mut check = Self {
                sequence: BuildingSequence::from_inputs(Arc::clone(&self.sequence.inputs)),
                replay: BuildingSequence::from_inputs(Arc::clone(&self.sequence.inputs)),
                blocks: Vec::new(),
                foundations: Vec::new(),
                supplies: Vec::new(),
                checked_blocks: Vec::new(),
            };
            check.sequence.restore(&b.checkpoint);
            check.prepare((b.end - b.checkpoint.next_building) as usize);
            if check.sequence.checkpoint() != s.state
                || check.blocks.first().is_none_or(|v| v.bounds != b.bounds)
                || check.foundations != s.foundations[b.helpers.clone()]
                || check.supplies != s.supplies[b.supplies.clone()]
            {
                return Err(invalid());
            }
        }
        Ok(())
    }
    pub fn restore_index(&mut self, s: &PlacementIndexSnapshot) -> Result<(), ReferenceError> {
        self.validate_index(s)?;
        self.sequence.restore(&s.state);
        self.blocks.clone_from(&s.blocks);
        self.foundations.clone_from(&s.foundations);
        self.supplies.clone_from(&s.supplies);
        self.checked_blocks.clear();
        self.checked_blocks.resize(s.blocks.len(), false);
        Ok(())
    }
}

impl PlacementIndex {
    fn check_blocks(
        &mut self,
        bounds: Option<WorldBounds>,
        horizontal: bool,
        required: u32,
    ) -> Result<(), ReferenceError> {
        for i in 0..self.blocks.len() {
            let b = &self.blocks[i];
            if self.checked_blocks[i]
                || b.checkpoint.next_building >= required
                || bounds.is_some_and(|q| !intersects(b.bounds, q, horizontal))
            {
                continue;
            }
            self.replay.restore(&b.checkpoint);
            let mut bounds = WorldBounds {
                min: [f64::INFINITY; 3],
                max: [f64::NEG_INFINITY; 3],
            };
            let mut helper = b.helpers.start;
            let mut supply = b.supplies.start;
            let bad = || {
                ReferenceError(format!(
                    "Saved building block {} differs",
                    b.checkpoint.next_building
                ))
            };
            for _ in b.checkpoint.next_building..b.end {
                let building = self.replay.advance().ok_or_else(bad)?;
                let p = &self.replay.inputs.prototypes[building.prototype_index];
                extend(&mut bounds, building.bounds(p.radius));
                for (helper_index, h) in p.helpers.iter().enumerate() {
                    let position = place_point(building.pose, p.scale, h.position);
                    let point = FoundationPoint {
                        building_index: building.index,
                        helper_index,
                        position,
                        range_squared: h.range_squared,
                    };
                    if self.foundations.get(helper) != Some(&point) {
                        return Err(bad());
                    }
                    helper += 1;
                    let radius = h.range_squared[0].max(h.range_squared[1]).sqrt() as f64;
                    extend(
                        &mut bounds,
                        WorldBounds {
                            min: position.map(|v| v - radius),
                            max: position.map(|v| v + radius),
                        },
                    );
                }
                for (supply_index, s) in p.supplies.iter().enumerate() {
                    let point = SupplyPoint {
                        building_index: building.index,
                        supply_index,
                        cap_pose: place_pose(building.pose, p.scale, s.cap_pose),
                        tank_pose: place_pose(building.pose, p.scale, s.tank_pose),
                        prototype_index: building.prototype_index,
                    };
                    if self.supplies.get(supply) != Some(&point) {
                        return Err(bad());
                    }
                    supply += 1;
                }
            }
            let expected = self
                .blocks
                .get(i + 1)
                .map(|b| b.checkpoint.clone())
                .unwrap_or_else(|| self.sequence.checkpoint());
            if bounds != b.bounds
                || helper != b.helpers.end
                || supply != b.supplies.end
                || self.replay.checkpoint() != expected
            {
                return Err(bad());
            }
            self.checked_blocks[i] = true;
        }
        Ok(())
    }
}
