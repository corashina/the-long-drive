//! Source power cable math: rope aim, pole selection and cable recipes.
use super::source_lights::{
    RoadPole, object_node, record, road_controller, road_prototype, transform_node,
};
use super::spawn_rules::{array, link, optional, value_link};
use super::{AbsolutePose, NodeAppearance, Placement, RoadIndex, RoadSegment, WorldBounds};
use crate::reference::{
    ReferenceError, SourceId, WorldPrototypeReference, WorldReference, WorldValue,
};
use rapier3d::math::{Pose, Rotation, Vector};
use std::ops::Range;

/// Source `roadGenScript.CheckCableConnections` range in meters.
pub const CABLE_RANGE: f64 = 200.;

/// Local pose of one rope node.
#[derive(Clone, Debug, PartialEq)]
pub struct NodePose {
    pub node: SourceId,
    pub rotation: [f32; 4],
    pub scale: [f32; 3],
}

/// Source `ropeScript.Connect`: LookAt the target, localScale z = distance / 100.
/// Port forward is -Z and up is +Y. Returns the local rotation and scale z.
pub fn aim(rope: [f64; 3], parent_rotation: Rotation, target: [f64; 3]) -> ([f32; 4], f32) {
    let delta = [0, 1, 2].map(|i| target[i] - rope[i]);
    let distance = delta.iter().map(|d| d * d).sum::<f64>().sqrt();
    if distance == 0. {
        return (Rotation::IDENTITY.to_array(), 0.);
    }
    let forward = Vector::from_array(delta.map(|d| (d / distance) as f32));
    let right = forward.cross(Vector::Y);
    // Straight up or down has no right axis. Use the shortest arc.
    let look = if right.length_squared() < 1e-12 {
        Rotation::from_rotation_arc(Vector::NEG_Z, forward)
    } else {
        let right = right.normalize();
        Rotation::from_rotation_axes(right, right.cross(forward), -forward)
    };
    let local = (parent_rotation.inverse() * look).normalize();
    (local.to_array(), (distance / 100.) as f32)
}

/// Source `roadGenScript.CheckCableConnections`.
/// A pole with target 0 under 200 m and nearer than the best distance is kept.
/// Its nearest target distance becomes the best distance.
pub fn nearest_target<'a>(
    rope0: [f64; 3],
    poles: impl Iterator<Item = (u32, &'a [[f64; 3]])>,
) -> Option<(u32, usize)> {
    let distance = |target: &[f64; 3]| {
        (0..3)
            .map(|i| (target[i] - rope0[i]).powi(2))
            .sum::<f64>()
            .sqrt()
    };
    let mut best: Option<(u32, usize, f64)> = None;
    for (pole, targets) in poles {
        let Some(first) = targets.first() else {
            continue;
        };
        let d = distance(first);
        if d < best.map_or(CABLE_RANGE, |(_, _, best)| best) {
            let mut nearest = (0, d);
            for (i, target) in targets.iter().enumerate().skip(1) {
                let d = distance(target);
                if d < nearest.1 {
                    nearest = (i, d);
                }
            }
            best = Some((pole, nearest.0, nearest.1));
        }
    }
    best.map(|(pole, index, _)| (pole, index))
}

/// One rope in its pole frame (road) or below the prototype root (building).
struct Rope {
    node: SourceId,
    position: Vector,
    parent: Rotation,
    scale: [f32; 2],
}
impl Rope {
    fn appearance(&self, rotation: [f32; 4], scale_z: f32) -> NodeAppearance {
        NodeAppearance {
            node: self.node.clone(),
            primary: None,
            scale: Some([self.scale[0], self.scale[1], scale_z]),
            rotation: Some(rotation),
            active: None,
            material: None,
            color: None,
            color_material: None,
        }
    }
}
/// Source `ropeScript` of one road prototype, in its pole frame.
struct RoadCable {
    pole: RoadPole,
    ropes: Vec<Rope>,
    targets: Vec<Vector>,
}
impl RoadCable {
    fn new(
        prototype: &WorldPrototypeReference,
        pole: RoadPole,
        ropes: &[usize],
        targets: &[usize],
    ) -> Result<Self, ReferenceError> {
        let ropes = ropes
            .iter()
            .map(|&node| {
                let parent = prototype.nodes[node]
                    .parent
                    .ok_or_else(|| ReferenceError("Source rope is outside its pole".into()))?;
                let scale = prototype.nodes[node].transform.scale;
                Ok(Rope {
                    node: prototype.nodes[node].source.clone(),
                    position: pole.local(prototype, node)?.translation,
                    parent: pole.local(prototype, parent as usize)?.rotation,
                    scale: [scale[0], scale[1]],
                })
            })
            .collect::<Result<_, ReferenceError>>()?;
        let targets = targets
            .iter()
            .map(|&node| Ok(pole.local(prototype, node)?.translation))
            .collect::<Result<_, ReferenceError>>()?;
        Ok(Self {
            pole,
            ropes,
            targets,
        })
    }
}
/// Pose of a node below the prototype root. The placement replaces the root transform.
fn below_root(
    prototype: &WorldPrototypeReference,
    mut node: usize,
) -> Result<Pose, ReferenceError> {
    let mut local = Pose::IDENTITY;
    for _ in 0..prototype.nodes.len() {
        let n = &prototype.nodes[node];
        let Some(parent) = n.parent else {
            return Ok(local);
        };
        let t = &n.transform;
        local.translation = Vector::from_array(t.translation)
            + Rotation::from_array(t.rotation) * (Vector::from_array(t.scale) * local.translation);
        local.rotation = Rotation::from_array(t.rotation) * local.rotation;
        node = parent as usize;
    }
    Err(ReferenceError("Source building ancestor cycle".into()))
}
/// Source `buildingscript.connectToPole`: the inactive rope root and its ropes.
pub struct BuildingCable {
    prototype: SourceId,
    root: SourceId,
    ropes: Vec<Rope>,
}
impl BuildingCable {
    fn new(
        prototype: &WorldPrototypeReference,
        root: usize,
        ropes: &[usize],
    ) -> Result<Self, ReferenceError> {
        if ropes.is_empty() {
            return Err(ReferenceError("Source building cable has no ropes".into()));
        }
        let ropes = ropes
            .iter()
            .map(|&node| {
                let parent = prototype.nodes[node]
                    .parent
                    .ok_or_else(|| ReferenceError("Source rope is outside its building".into()))?;
                let scale = prototype.nodes[node].transform.scale;
                Ok(Rope {
                    node: prototype.nodes[node].source.clone(),
                    position: below_root(prototype, node)?.translation,
                    parent: below_root(prototype, parent as usize)?.rotation,
                    scale: [scale[0], scale[1]],
                })
            })
            .collect::<Result<_, ReferenceError>>()?;
        Ok(Self {
            prototype: prototype.source.clone(),
            root: prototype.nodes[root].source.clone(),
            ropes,
        })
    }
    /// World position and parent world rotation of a rope.
    fn world(&self, rope: &Rope, pose: AbsolutePose, scale: [f32; 3]) -> ([f64; 3], Rotation) {
        let rotation = Rotation::from_array(pose.rotation);
        let offset = rotation * (Vector::from_array(scale) * rope.position);
        (
            std::array::from_fn(|i| pose.position[i] + offset[i] as f64),
            rotation * rope.parent,
        )
    }
    /// World position of rope 0, the source search origin.
    pub fn rope0(&self, pose: AbsolutePose, scale: [f32; 3]) -> [f64; 3] {
        self.world(&self.ropes[0], pose, scale).0
    }
    /// Source `ropeScript.Connect(target, k)` after `CheckCableConnections`.
    /// No target: no output, the root stays inactive.
    pub fn connect(
        &self,
        pose: AbsolutePose,
        scale: [f32; 3],
        target: Option<[f64; 3]>,
        output: &mut Vec<NodeAppearance>,
    ) {
        output.clear();
        let Some(target) = target else {
            return;
        };
        output.push(NodeAppearance {
            node: self.root.clone(),
            primary: None,
            scale: None,
            rotation: None,
            active: Some(true),
            material: None,
            color: None,
            color_material: None,
        });
        for rope in &self.ropes {
            let (position, parent) = self.world(rope, pose, scale);
            let (rotation, scale_z) = aim(position, parent, target);
            output.push(rope.appearance(rotation, scale_z));
        }
    }
}
/// Road and building cables, resolved once per stream.
pub struct CableRecipes {
    roads: Vec<RoadCable>,
    buildings: Vec<BuildingCable>,
    seed: i32,
}
fn nodes(
    prototype: &WorldPrototypeReference,
    links: &[WorldValue],
) -> Result<Vec<usize>, ReferenceError> {
    links
        .iter()
        .map(|v| transform_node(prototype, value_link(v)?))
        .collect()
}
fn prototype<'a>(
    world: &'a WorldReference,
    source: &SourceId,
) -> Result<&'a WorldPrototypeReference, ReferenceError> {
    world
        .prototypes
        .iter()
        .find(|p| &p.source == source)
        .ok_or_else(|| ReferenceError("Missing cable prototype".into()))
}
fn ropes_bytes(ropes: &Vec<Rope>) -> usize {
    ropes.capacity() * std::mem::size_of::<Rope>()
        + ropes.iter().map(|r| r.node.file.capacity()).sum::<usize>()
}
impl CableRecipes {
    pub fn new(world: &WorldReference, seed: i32) -> Result<Self, ReferenceError> {
        let mut roads = Vec::with_capacity(world.road.prototypes.len());
        for source in &world.road.prototypes {
            let prototype = prototype(world, source)?;
            let controller = road_controller(world, source)?;
            let rope = &record(world, link(&controller.fields, "rope")?)?.fields;
            roads.push(RoadCable::new(
                prototype,
                RoadPole::new(prototype, controller)?,
                &nodes(prototype, array(rope, "ropes")?)?,
                &nodes(prototype, array(rope, "ropeTargets")?)?,
            )?);
        }
        let mut buildings: Vec<BuildingCable> = Vec::new();
        for kind in &world.buildings.types {
            if buildings.iter().any(|b| b.prototype == kind.prototype) {
                continue;
            }
            let prototype = prototype(world, &kind.prototype)?;
            let owner = link(&record(world, &prototype.source)?.fields, "m_GameObject")?;
            let Some(script) = world.records.iter().find(|r| {
                r.class_name == "buildingscript"
                    && matches!(r.fields.get("m_GameObject"), Some(WorldValue::Link(go)) if go == owner)
            }) else {
                continue;
            };
            let Some(rope) = optional(&script.fields, "connectToPole")? else {
                continue;
            };
            let rope = &record(world, rope)?.fields;
            buildings.push(BuildingCable::new(
                prototype,
                object_node(world, prototype, link(rope, "m_GameObject")?)?,
                &nodes(prototype, array(rope, "ropes")?)?,
            )?);
        }
        Ok(Self {
            roads,
            buildings,
            seed,
        })
    }
    /// Upper bound of `road` output poses for a road biome.
    pub fn road_ropes(&self, biome: u8) -> usize {
        self.roads[biome as usize].ropes.len()
    }
    pub fn retained_bytes(&self) -> usize {
        std::mem::size_of::<Self>()
            + self.roads.capacity() * std::mem::size_of::<RoadCable>()
            + self
                .roads
                .iter()
                .map(|r| {
                    ropes_bytes(&r.ropes) + r.targets.capacity() * std::mem::size_of::<Vector>()
                })
                .sum::<usize>()
            + self.buildings.capacity() * std::mem::size_of::<BuildingCable>()
            + self
                .buildings
                .iter()
                .map(|b| {
                    b.prototype.file.capacity() + b.root.file.capacity() + ropes_bytes(&b.ropes)
                })
                .sum::<usize>()
    }
    pub fn building(&self, prototype: &SourceId) -> Option<&BuildingCable> {
        self.buildings.iter().find(|b| &b.prototype == prototype)
    }
    /// Source `roadGenScript.PlaceOneRoad`: rope k of `segment` aims at target k of `next`.
    pub fn road(
        &self,
        world: &WorldReference,
        segment: &RoadSegment,
        next: &RoadSegment,
        output: &mut Vec<NodePose>,
    ) {
        self.road_between(
            [road_prototype(world, segment), road_prototype(world, next)],
            segment,
            next,
            output,
        );
    }
    /// Append the world positions of the pole rope targets of `segment`.
    pub fn targets(
        &self,
        world: &WorldReference,
        segment: &RoadSegment,
        output: &mut Vec<[f64; 3]>,
    ) {
        let cable = &self.roads[segment.biome as usize];
        let frame = cable
            .pole
            .frame(road_prototype(world, segment), segment, self.seed);
        output.extend(cable.targets.iter().map(|t| frame.point(*t)));
    }
    fn road_between(
        &self,
        prototypes: [&WorldPrototypeReference; 2],
        segment: &RoadSegment,
        next: &RoadSegment,
        output: &mut Vec<NodePose>,
    ) {
        output.clear();
        let cable = &self.roads[segment.biome as usize];
        let target = &self.roads[next.biome as usize];
        let from = cable.pole.frame(prototypes[0], segment, self.seed);
        let to = target.pole.frame(prototypes[1], next, self.seed);
        for (rope, point) in cable.ropes.iter().zip(&target.targets) {
            let (rotation, scale) = aim(
                from.point(rope.position),
                from.rotation * rope.parent,
                to.point(*point),
            );
            output.push(NodePose {
                node: rope.node.clone(),
                rotation,
                scale: [rope.scale[0], rope.scale[1], scale],
            });
        }
    }
}

/// Resumable road pole search around one rope 0. The buffers are reused.
#[derive(Default)]
pub struct PoleSearch {
    rope0: Option<[f64; 3]>,
    segments: Vec<u32>,
    next: usize,
    targets: Vec<[f64; 3]>,
    poles: Vec<Range<usize>>,
}
impl PoleSearch {
    pub fn rope0(&self) -> Option<[f64; 3]> {
        self.rope0
    }
    /// Source `CheckCableConnections` reads the roads that exist at `FStart`: use the prepared prefix.
    pub fn start(&mut self, roads: &RoadIndex, rope0: [f64; 3]) {
        assert_eq!(
            roads.segments_in(range(rope0), roads.coverage(), &mut self.segments),
            super::QueryStatus::Complete
        );
        self.next = 0;
        self.targets.clear();
        self.poles.clear();
        self.rope0 = Some(rope0);
    }
    /// Replay at most `budget` road segments. True when every candidate is read.
    pub fn step(
        &mut self,
        recipes: &CableRecipes,
        world: &WorldReference,
        roads: &mut RoadIndex,
        budget: &mut usize,
    ) -> Result<bool, ReferenceError> {
        let Some(rope0) = self.rope0 else {
            return Ok(false);
        };
        let bounds = range(rope0);
        while let Some(&index) = self.segments.get(self.next) {
            if *budget == 0 {
                return Ok(false);
            }
            let segment = roads.prepare_segment(index, *budget)?;
            *budget = budget.saturating_sub(roads.replay_work());
            let Some(segment) = segment else {
                return Ok(false);
            };
            self.next += 1;
            if (0..3).all(|a| {
                segment.bounds.min[a] <= bounds.max[a] && segment.bounds.max[a] >= bounds.min[a]
            }) {
                let start = self.targets.len();
                recipes.targets(world, &segment, &mut self.targets);
                self.poles.push(start..self.targets.len());
            }
        }
        Ok(true)
    }
    /// The selected pole target, after `step` returns true.
    pub fn target(&self) -> Option<[f64; 3]> {
        let (pole, k) = nearest_target(
            self.rope0?,
            self.poles
                .iter()
                .enumerate()
                .map(|(i, r)| (i as u32, &self.targets[r.clone()])),
        )?;
        Some(self.targets[self.poles[pole as usize].start + k])
    }
    pub fn retained_bytes(&self) -> usize {
        self.segments.capacity() * std::mem::size_of::<u32>()
            + self.targets.capacity() * std::mem::size_of::<[f64; 3]>()
            + self.poles.capacity() * std::mem::size_of::<Range<usize>>()
    }
}
fn range(rope0: [f64; 3]) -> WorldBounds {
    WorldBounds {
        min: rope0.map(|v| v - CABLE_RANGE),
        max: rope0.map(|v| v + CABLE_RANGE),
    }
}

/// Resumable road pole search for one building cable.
#[derive(Default)]
pub struct BuildingCableSearch {
    placement: Option<Placement>,
    search: PoleSearch,
}
impl BuildingCableSearch {
    pub fn placement(&self) -> Option<&Placement> {
        self.placement.as_ref()
    }
    pub fn start(&mut self, roads: &RoadIndex, cable: &BuildingCable, placement: Placement) {
        self.search
            .start(roads, cable.rope0(placement.pose, placement.scale));
        self.placement = Some(placement);
    }
    /// Replay at most `budget` road segments.
    /// When done, `output` holds the cable and the result is the building identity.
    pub fn step(
        &mut self,
        recipes: &CableRecipes,
        world: &WorldReference,
        roads: &mut RoadIndex,
        budget: &mut usize,
        output: &mut Vec<NodeAppearance>,
    ) -> Result<Option<super::GeneratedId>, ReferenceError> {
        let Some(placement) = &self.placement else {
            return Ok(None);
        };
        if !self.search.step(recipes, world, roads, budget)? {
            return Ok(None);
        }
        let cable = recipes
            .building(&placement.prototype)
            .ok_or_else(|| ReferenceError("Missing source building cable".into()))?;
        cable.connect(
            placement.pose,
            placement.scale,
            self.search.target(),
            output,
        );
        Ok(self.placement.take().map(|p| p.id))
    }
    pub fn retained_bytes(&self) -> usize {
        self.placement
            .as_ref()
            .map_or(0, super::contents::placement_heap)
            + self.search.retained_bytes()
    }
}
