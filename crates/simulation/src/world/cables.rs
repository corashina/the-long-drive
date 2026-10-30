//! Source power cable math: rope aim, pole selection and cable recipes.
use super::source_lights::{RoadPole, object_node, record, road_controller, transform_node};
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
/// Keep the pole with the nearest target 0 under 200 m. Then pick its nearest target.
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
    let mut best: Option<(u32, &[[f64; 3]], f64)> = None;
    for (pole, targets) in poles {
        let Some(first) = targets.first() else {
            continue;
        };
        let d = distance(first);
        if d < best.map_or(CABLE_RANGE, |(_, _, best)| best) {
            best = Some((pole, targets, d));
        }
    }
    let (pole, targets, _) = best?;
    let mut index = 0;
    for (i, target) in targets.iter().enumerate().skip(1) {
        if distance(target) < distance(&targets[index]) {
            index = i;
        }
    }
    Some((pole, index))
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
fn road_prototype<'a>(
    world: &'a WorldReference,
    segment: &RoadSegment,
) -> &'a WorldPrototypeReference {
    let source = &world.road.prototypes[segment.biome as usize];
    world
        .prototypes
        .iter()
        .find(|p| &p.source == source)
        .unwrap()
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

#[cfg(test)]
mod tests {
    use super::*;

    fn world_forward(parent: Rotation, local: [f32; 4]) -> Vector {
        (parent * Rotation::from_array(local) * Vector::NEG_Z).normalize()
    }

    #[test]
    fn aim_points_forward_at_target() {
        let (rotation, scale) = aim([0.; 3], Rotation::IDENTITY, [3., 4., -12.]);
        assert!((scale - 0.13).abs() < 1e-6);
        let expected = Vector::new(3., 4., -12.) / 13.;
        assert!((world_forward(Rotation::IDENTITY, rotation) - expected).length() < 1e-5);
    }

    #[test]
    fn aim_removes_parent_rotation() {
        let parent = Rotation::from_rotation_y(std::f32::consts::FRAC_PI_2);
        let (rotation, _) = aim([1., 2., 3.], parent, [4., 6., -9.]);
        let expected = Vector::new(3., 4., -12.) / 13.;
        assert!((world_forward(parent, rotation) - expected).length() < 1e-5);
    }

    #[test]
    fn nearest_target_follows_source_rule() {
        let far = [[201., 0., 0.]];
        let near = [[199., 0., 0.]];
        assert_eq!(nearest_target([0.; 3], [(1, &far[..])].into_iter()), None);
        assert_eq!(
            nearest_target([0.; 3], [(1, &near[..])].into_iter()),
            Some((1, 0))
        );
        let first = [[50., 0., 0.]];
        let second = [[20., 0., 0.], [10., 0., 0.], [15., 0., 0.]];
        let poles = [(7, &first[..]), (8, &second[..])];
        assert_eq!(nearest_target([0.; 3], poles.into_iter()), Some((8, 1)));
    }

    #[test]
    fn building_cable_aims_at_the_target() {
        let id = |path_id| SourceId {
            file: "level1".into(),
            path_id,
        };
        let cable = BuildingCable {
            prototype: id(1),
            root: id(2),
            ropes: vec![Rope {
                node: id(3),
                position: Vector::new(2., 5., 0.),
                parent: Rotation::IDENTITY,
                scale: [0.5, 0.7],
            }],
        };
        let yaw = Rotation::from_rotation_y(std::f32::consts::FRAC_PI_2);
        let pose = AbsolutePose {
            position: [1000., 0., 2000.],
            rotation: yaw.to_array(),
        };
        let scale = [2.; 3];
        let rope0 = cable.rope0(pose, scale);
        assert!((0..3).all(|i| (rope0[i] - [1000., 10., 1996.][i]).abs() < 1e-4));
        let offset = |d: [f64; 3]| std::array::from_fn(|i| rope0[i] + d[i]);
        let mut output = Vec::new();
        cable.connect(pose, scale, Some(offset([30., 0., 40.])), &mut output);
        assert_eq!(output.len(), 2);
        assert_eq!((&output[0].node, output[0].active), (&id(2), Some(true)));
        let rope = &output[1];
        assert_eq!(rope.node, id(3));
        let [x, y, z] = rope.scale.unwrap();
        assert_eq!([x, y], [0.5, 0.7]);
        assert!((z - 0.5).abs() < 1e-5, "scale z is distance / 100");
        let forward = world_forward(yaw, rope.rotation.unwrap());
        assert!((forward - Vector::new(0.6, 0., 0.8)).length() < 1e-4);
        cable.connect(pose, scale, None, &mut output);
        assert!(output.is_empty(), "no target: the root stays inactive");
    }

    #[test]
    fn road_ropes_point_at_next_targets_on_tilted_poles() {
        use crate::reference::{AssetId, Transform, WorldPrototypeNode, WorldPrototypeRole};
        use glam::{Affine3A, Quat, Vec3};
        let id = |path_id| SourceId {
            file: "level1".into(),
            path_id,
        };
        let yaw = |degrees: f32| Rotation::from_rotation_y(degrees.to_radians()).to_array();
        let node = |path_id, parent, translation, rotation, scale| WorldPrototypeNode {
            source: id(path_id),
            name: String::new(),
            parent,
            active: true,
            renderer_enabled: true,
            layer: 0,
            transform: Transform {
                translation,
                rotation,
                scale,
            },
            gltf_node: 0,
            biome_materials: false,
        };
        let identity = [0., 0., 0., 1.];
        let prototype = WorldPrototypeReference {
            source: id(1),
            role: WorldPrototypeRole::Road,
            asset: AssetId(0),
            nodes: vec![
                node(1, None, [0.; 3], identity, [1.; 3]),
                node(2, Some(0), [2., 0., -1.], yaw(20.), [1.2; 3]),
                node(3, Some(1), [0., 5., 0.], yaw(30.), [1.; 3]),
                node(4, Some(2), [-1., 0., 0.], identity, [1.; 3]),
                node(5, Some(2), [1., 0., 0.], identity, [1.; 3]),
                node(6, Some(2), [-1., -0.2, 0.3], identity, [1.; 3]),
                node(7, Some(2), [1., -0.2, 0.3], identity, [1.; 3]),
            ],
            controls: Vec::new(),
            excluded_actions: Vec::new(),
            deferred_materials: Vec::new(),
            skins: Vec::new(),
            bodies: Vec::new(),
            colliders: Vec::new(),
            prefab_pose: Transform {
                translation: [0.; 3],
                rotation: identity,
                scale: [1.; 3],
            },
            interiors: Vec::new(),
            vehicle: None,
            engines: Vec::new(),
            radiators: Vec::new(),
            gauges: Vec::new(),
            sled: None,
            trailer: None,
            followers: None,
            hitches: Vec::new(),
            help_texts: Vec::new(),
            pushes: Vec::new(),
        };
        let pole = RoadPole {
            node: 1,
            scale: Vector::splat(1.2),
            chances: [1., 0.],
            angles: [[15, 10, 15]; 2],
        };
        let seed = 7;
        let segment = |index, source_position| RoadSegment {
            index,
            biome: 0,
            source_position,
            source_origin: [0.; 3],
            source_rotations: [[0.; 3]; super::super::road::ROAD_CONTROLS],
            source_coords: [[0.; 3]; super::super::road::ROAD_CONTROLS],
            bounds: super::super::WorldBounds {
                min: [0.; 3],
                max: [0.; 3],
            },
        };
        let (first, second) = (segment(4, [0.; 3]), segment(5, [3., 1., 20.]));
        for s in [&first, &second] {
            let angles = super::super::lamps::source_angles(
                seed as u64,
                super::super::lamps::RoadLampId(s.index as i64),
                pole.chances,
                pole.angles,
            );
            assert_ne!(angles, [0; 3], "the test pole must tilt");
        }
        let recipes = CableRecipes {
            roads: vec![RoadCable::new(&prototype, pole.clone(), &[3, 4], &[5, 6]).unwrap()],
            buildings: Vec::new(),
            seed,
        };
        let mut poses = Vec::new();
        recipes.road_between([&prototype; 2], &first, &second, &mut poses);
        assert_eq!(poses.len(), 2);
        // Independent Unity TRS chain from the tilted pole.
        let affine = |t: &Transform| {
            Affine3A::from_scale_rotation_translation(
                Vec3::from_array(t.scale),
                Quat::from_array(t.rotation),
                Vec3::from_array(t.translation),
            )
        };
        let tilted = |s: &RoadSegment| {
            let frame = pole.frame(&prototype, s, seed);
            let p = frame.position.map(|v| v as f32);
            Affine3A::from_scale_rotation_translation(
                Vec3::splat(1.2),
                Quat::from_array(frame.rotation.to_array()),
                Vec3::from_array(p),
            ) * affine(&prototype.nodes[2].transform)
        };
        let (root, next_root) = (tilted(&first), tilted(&second));
        let (_, parent, _) = root.to_scale_rotation_translation();
        for (k, pose) in poses.iter().enumerate() {
            assert_eq!(pose.node, id(4 + k as i64));
            let rope = root.transform_point3(Vec3::from_array(
                prototype.nodes[3 + k].transform.translation,
            ));
            let target = next_root.transform_point3(Vec3::from_array(
                prototype.nodes[5 + k].transform.translation,
            ));
            let delta = target - rope;
            let forward = parent * Quat::from_array(pose.rotation) * Vec3::NEG_Z;
            assert!((forward - delta.normalize()).length() < 1e-4);
            assert!((pose.scale[2] - delta.length() / 100.).abs() < 1e-4);
            assert_eq!(&pose.scale[..2], &[1., 1.]);
        }
    }
}
