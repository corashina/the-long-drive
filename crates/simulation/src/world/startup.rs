//! Source startup ownership, alignment, and hole coordinates.
use super::{TerrainContext, TerrainEvaluator, TerrainFoundation, TerrainRoadPoint};
use crate::reference::{BodyId, ReferenceError, ReferenceManifest, WorldReference, WorldValue};
use rapier3d::math::Vector;

/// Startup offsets retain imported ownership; callers apply them before collider insertion.
#[derive(Clone, Debug, Default, serde::Serialize, serde::Deserialize)]
pub struct StartupTerrain {
    pub root_offsets: Vec<(crate::reference::SourceId, f32)>,
    pub player_offset: f32,
    pub foundations: Vec<TerrainFoundation>,
    pub holes: Vec<[[f32; 3]; 4]>,
    pub hole_centers: Vec<[f32; 3]>,
}

impl StartupTerrain {
    /// Sum of the startup root offsets on the Transform chain of `transform`.
    pub fn chain_offset(
        &self,
        reference: &WorldReference,
        transform: &crate::reference::SourceId,
    ) -> Result<f32, ReferenceError> {
        let mut offset = 0.;
        let mut current = Some(transform);
        while let Some(source) = current {
            offset += self
                .root_offsets
                .iter()
                .filter(|(id, _)| id == source)
                .map(|(_, value)| *value)
                .sum::<f32>();
            let record = reference
                .records
                .iter()
                .find(|r| &r.source == source && r.class_name == "Transform")
                .ok_or_else(|| {
                    ReferenceError(format!(
                        "{}:{} startup Transform",
                        source.file, source.path_id
                    ))
                })?;
            current = match record.fields.get("m_Father") {
                Some(WorldValue::Link(parent)) => Some(parent),
                _ => None,
            };
        }
        Ok(offset)
    }
    pub fn body_offsets(&self, reference: &ReferenceManifest) -> Vec<(BodyId, f32)> {
        use std::collections::HashMap;
        let nodes: HashMap<_, _> = reference.nodes.iter().map(|node| (node.id, node)).collect();
        let records: HashMap<_, _> = reference
            .world
            .records
            .iter()
            .map(|r| (&r.source, r))
            .collect();
        reference
            .bodies
            .iter()
            .map(|body| {
                let mut node = &nodes[&body.node];
                let mut offset = 0.0_f32;
                loop {
                    offset += self
                        .root_offsets
                        .iter()
                        .filter(|(id, _)| *id == node.source)
                        .map(|(_, value)| *value)
                        .sum::<f32>();
                    if let Some(parent) = node.parent {
                        node = &nodes[&parent];
                    } else {
                        break;
                    }
                }
                // Asset roots can have source parents outside the exported scene.
                let mut source = &node.source;
                while let Some(WorldValue::Link(parent)) = records
                    .get(source)
                    .and_then(|record| record.fields.get("m_Father"))
                {
                    offset += self
                        .root_offsets
                        .iter()
                        .filter(|(id, _)| id == parent)
                        .map(|(_, value)| *value)
                        .sum::<f32>();
                    source = parent;
                }
                (body.id, offset)
            })
            .collect()
    }

    pub fn new<'a>(
        reference: &'a WorldReference,
        distances: &super::DrawDistances,
        evaluator: &TerrainEvaluator,
        roads: &[TerrainRoadPoint],
    ) -> Result<Self, ReferenceError> {
        use crate::reference::{SourceId, WorldSourceRecord};
        use rapier3d::math::Rotation;
        use std::collections::HashMap;
        let records: HashMap<_, _> = reference.records.iter().map(|r| (&r.source, r)).collect();
        let get = |id: &SourceId| -> Result<&'a WorldSourceRecord, ReferenceError> {
            records
                .get(id)
                .copied()
                .ok_or_else(|| ReferenceError(format!("{}:{} startup record", id.file, id.path_id)))
        };
        fn number(value: &WorldValue) -> f32 {
            match value {
                WorldValue::Number(n) => *n as f32,
                WorldValue::Integer(n) => *n as f32,
                _ => panic!("checked startup scalar"),
            }
        }
        fn xyz(value: &WorldValue) -> [f32; 3] {
            let WorldValue::Record(fields) = value else {
                panic!("checked startup vector")
            };
            [
                number(&fields["x"]),
                number(&fields["y"]),
                number(&fields["z"]),
            ]
        }
        fn link<'a>(record: &'a WorldSourceRecord, name: &str) -> Option<&'a SourceId> {
            match record.fields.get(name) {
                Some(WorldValue::Link(id)) => Some(id),
                _ => None,
            }
        }
        let transform_for =
            |component: &'a WorldSourceRecord| -> Result<&'a WorldSourceRecord, ReferenceError> {
                let owner = link(component, "m_GameObject")
                    .ok_or_else(|| ReferenceError("Startup component owner".into()))?;
                let object = get(owner)?;
                for child in object.components() {
                    let candidate = get(child)?;
                    if candidate.class_name == "Transform" {
                        return Ok(candidate);
                    }
                }
                Err(ReferenceError(format!(
                    "{}:{} startup Transform",
                    owner.file, owner.path_id
                )))
            };
        let belongs =
            |mut node: &'a WorldSourceRecord, root: &SourceId| -> Result<bool, ReferenceError> {
                loop {
                    if &node.source == root {
                        return Ok(true);
                    }
                    let Some(parent) = link(node, "m_Father") else {
                        return Ok(false);
                    };
                    node = get(parent)?;
                }
            };
        let point = |mut node: &'a WorldSourceRecord,
                     mut point: Vector,
                     offsets: &[(SourceId, f32)]|
         -> Result<Vector, ReferenceError> {
            loop {
                let WorldValue::Record(q) = &node.fields["m_LocalRotation"] else {
                    return Err(ReferenceError("Startup local rotation".into()));
                };
                let rotation = Rotation::from_xyzw(
                    number(&q["x"]),
                    number(&q["y"]),
                    number(&q["z"]),
                    number(&q["w"]),
                );
                point = rotation * (point * Vector::from_array(xyz(&node.fields["m_LocalScale"])))
                    + Vector::from_array(xyz(&node.fields["m_LocalPosition"]));
                if let Some((_, offset)) = offsets.iter().find(|(source, _)| source == &node.source)
                {
                    point.y += *offset;
                }
                let Some(parent) = link(node, "m_Father") else {
                    return Ok(point);
                };
                node = get(parent)?;
            }
        };
        let tile = super::terrain_tile(reference, distances, 0)?;
        let mut result = Self::default();
        let mut shared = None;
        // Source runs the shared roots, player root, then each fixed root.
        for group in ["StartObjs", "StartObjsDonTMoveOnLoad", "StartObjsFix"] {
            for root in reference.startup.iter().filter(|r| r.group == group) {
                let transform = get(&root.source)?;
                let position = point(transform, Vector::ZERO, &result.root_offsets)?;
                let context = TerrainContext {
                    evaluator,
                    tile,
                    roads,
                    road_radius_squared: Some(22500.0),
                    foundations: &result.foundations,
                    holes: &[],
                    biome_override: None,
                };
                let offset = if group == "StartObjsFix" || shared.is_none() {
                    (context.height([position.x, position.z]) as f64 - position.y as f64) as f32
                } else {
                    shared.unwrap()
                };
                if shared.is_none() {
                    shared = Some(offset);
                    result.player_offset = offset;
                }
                result.root_offsets.push((root.source.clone(), offset));
                for helper in reference.records.iter().filter(|r| {
                    r.class_name == "terrainHeightAlignToBuildingScript"
                        && r.source.file == "level1"
                        && matches!(r.source.path_id, 79970 | 72657)
                }) {
                    if link(helper, "m_GameObject") != link(transform, "m_GameObject") {
                        continue;
                    }
                    let WorldValue::Array(coords) = &helper.fields["helpPosCoords"] else {
                        return Err(ReferenceError("Startup helper coordinates".into()));
                    };
                    let WorldValue::Array(ranges) = &helper.fields["range2"] else {
                        return Err(ReferenceError("Startup helper ranges".into()));
                    };
                    for (index, coord) in coords.iter().enumerate() {
                        let WorldValue::Link(id) = coord else {
                            return Err(ReferenceError("Startup helper Transform".into()));
                        };
                        let node = get(id)?;
                        let scale = xyz(&node.fields["m_LocalScale"])[0];
                        let factor = if ranges.len() < coords.len() {
                            0.0
                        } else {
                            number(&ranges[index])
                        };
                        let factor = if factor == 0.0 { 4.0 } else { factor.max(0.0) };
                        let inner = (scale as f64 * 0.5) as f32;
                        let outer = (scale as f64 * factor as f64 * 0.5) as f32;
                        result.foundations.push(TerrainFoundation {
                            position: point(node, Vector::ZERO, &result.root_offsets)?.to_array(),
                            range_squared: [
                                (inner as f64 * inner as f64) as f32,
                                (outer as f64 * outer as f64) as f32,
                            ],
                        });
                    }
                }
                for hole in &reference.startup_holes {
                    let component = get(&hole.component)?;
                    let node = transform_for(component)?;
                    if !belongs(node, &root.source)? {
                        continue;
                    }
                    let center = point(node, Vector::ZERO, &result.root_offsets)?;
                    let scale = xyz(&node.fields["m_LocalScale"]);
                    let mut rotation = Rotation::IDENTITY;
                    let mut current = node;
                    loop {
                        let WorldValue::Record(q) = &current.fields["m_LocalRotation"] else {
                            return Err(ReferenceError("Startup hole rotation".into()));
                        };
                        rotation = Rotation::from_xyzw(
                            number(&q["x"]),
                            number(&q["y"]),
                            number(&q["z"]),
                            number(&q["w"]),
                        ) * rotation;
                        let Some(parent) = link(current, "m_Father") else {
                            break;
                        };
                        current = get(parent)?;
                    }
                    // Refresh scales only local X and Z, then applies the world rotation.
                    let vertices = hole.vertices.map(|v| {
                        (rotation * Vector::new(v[0] * scale[0], v[1], v[2] * scale[2])).to_array()
                    });
                    let mut left = [usize::MAX; 2];
                    let mut right = [usize::MAX; 2];
                    for i in 0..4 {
                        if left[0] == usize::MAX || vertices[i][0] < vertices[left[0]][0] {
                            left[0] = i;
                        }
                        if right[0] == usize::MAX || vertices[i][0] > vertices[right[0]][0] {
                            right[0] = i;
                        }
                    }
                    for i in 0..4 {
                        if vertices[i] != vertices[left[0]]
                            && (left[1] == usize::MAX || vertices[i][0] < vertices[left[1]][0])
                        {
                            left[1] = i;
                        }
                        if vertices[i] != vertices[right[0]]
                            && (right[1] == usize::MAX || vertices[i][0] > vertices[right[1]][0])
                        {
                            right[1] = i;
                        }
                    }
                    if left[1] == usize::MAX || right[1] == usize::MAX {
                        return Err(ReferenceError("Degenerate startup hole".into()));
                    }
                    if vertices[left[1]][2] > vertices[left[0]][2] {
                        left.swap(0, 1);
                    }
                    if vertices[right[1]][2] > vertices[right[0]][2] {
                        right.swap(0, 1);
                    }
                    result.hole_centers.push(center.to_array());
                    result.holes.push(
                        [left[0], right[0], left[1], right[1]]
                            .map(|i| (Vector::from_array(vertices[i]) + center).to_array()),
                    );
                }
            }
        }
        Ok(result)
    }
}

/// Pose of a source Transform after startup alignment, in converted world space.
/// `local` is an extra converted local transform below that Transform.
pub fn source_world_pose(
    reference: &WorldReference,
    startup: &StartupTerrain,
    transform: &crate::reference::SourceId,
    local: &[crate::reference::Transform],
) -> Result<crate::vehicle::VehiclePose, ReferenceError> {
    use rapier3d::math::Rotation;
    fn number(value: Option<&WorldValue>) -> Result<f32, ReferenceError> {
        match value {
            Some(WorldValue::Number(n)) => Ok(*n as f32),
            Some(WorldValue::Integer(n)) => Ok(*n as f32),
            _ => Err(ReferenceError("Startup Transform scalar".into())),
        }
    }
    fn fields(
        value: Option<&WorldValue>,
    ) -> Result<&crate::reference::WorldFields, ReferenceError> {
        match value {
            Some(WorldValue::Record(fields)) => Ok(fields),
            _ => Err(ReferenceError("Startup Transform vector".into())),
        }
    }
    let mut chain = Vec::new();
    let mut current = Some(transform.clone());
    while let Some(source) = current {
        let record = reference
            .records
            .iter()
            .find(|r| r.source == source && r.class_name == "Transform")
            .ok_or_else(|| {
                ReferenceError(format!(
                    "{}:{} startup Transform",
                    source.file, source.path_id
                ))
            })?;
        let p = fields(record.fields.get("m_LocalPosition"))?;
        let q = fields(record.fields.get("m_LocalRotation"))?;
        let s = fields(record.fields.get("m_LocalScale"))?;
        // Source left-handed values become converted values: z negates, rotation x and y negate.
        chain.push((
            Vector::new(
                number(p.get("x"))?,
                number(p.get("y"))?,
                -number(p.get("z"))?,
            ),
            Rotation::from_xyzw(
                -number(q.get("x"))?,
                -number(q.get("y"))?,
                number(q.get("z"))?,
                number(q.get("w"))?,
            ),
            Vector::new(
                number(s.get("x"))?,
                number(s.get("y"))?,
                number(s.get("z"))?,
            ),
            startup
                .root_offsets
                .iter()
                .filter(|(id, _)| *id == source)
                .map(|(_, v)| *v)
                .sum::<f32>(),
        ));
        current = match record.fields.get("m_Father") {
            Some(WorldValue::Link(parent)) => Some(parent.clone()),
            _ => None,
        };
    }
    let mut position = Vector::ZERO;
    let mut rotation = Rotation::IDENTITY;
    let mut scale = Vector::ONE;
    let mut offset = 0.;
    for (p, q, s, o) in chain.iter().rev() {
        position += rotation * (scale * *p);
        rotation *= *q;
        scale *= *s;
        offset += o;
    }
    for t in local {
        position += rotation * (scale * Vector::from_array(t.translation));
        rotation *= Rotation::from_array(t.rotation);
        scale *= Vector::from_array(t.scale);
    }
    position.y += offset;
    Ok(crate::vehicle::VehiclePose {
        position: position.to_array(),
        rotation: rotation.normalize().to_array(),
    })
}
