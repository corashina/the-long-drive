//! Cached source candidate grids and coordinate-seeded object selection.
use super::{
    AbsolutePose, GeneratedId, Placement, RoadCurve, SourceRandom, TerrainContext, WorldBounds,
    WorldLayer, source_perlin,
};
use crate::reference::{
    ReferenceError, SourceId, WorldGeneratorReference, WorldReference, WorldValue,
};
use std::collections::HashMap;

#[derive(Clone, Copy)]
pub struct PropCandidate {
    pub point: [f32; 2],
    pub local: [f32; 2],
    pub ordinal: u32,
    pub selected: usize,
    pub rotation: i32,
    pub random: i32,
}
struct RockRule {
    renderer: Option<SourceId>,
    materials: Vec<SourceId>,
    min: f32,
    max: f32,
    no_scale: bool,
}
struct PrototypeRule {
    source: SourceId,
    scale: [f32; 3],
    radius: f64,
    rock: Option<RockRule>,
}
pub struct PropGenerator {
    pub layer: WorldLayer,
    pub reference: WorldGeneratorReference,
    points: Vec<[f32; 2]>,
    chances: Vec<f32>,
    max_chance: f32,
    prototypes: Vec<Option<PrototypeRule>>,
    curve: RoadCurve,
    width: f32,
    seed: i32,
}
/// Candidate grid side of one chunk.
pub fn candidate_side(reference: &WorldGeneratorReference) -> usize {
    ((reference.chunk_dist / reference.point_dist) as usize).max(2)
}
impl PropGenerator {
    pub fn new(world: &WorldReference, seed: i32, index: usize) -> Result<Self, ReferenceError> {
        let reference = world.generators[index].clone();
        let layer = [
            WorldLayer::Prop,
            WorldLayer::Mountain,
            WorldLayer::Tower,
            WorldLayer::DesertBuilding,
        ][index];
        let side = candidate_side(&reference);
        let half = (side - 1) as f32 * reference.point_dist * 0.5;
        let mut points = Vec::with_capacity(side * side);
        for z in 0..side {
            for x in 0..side {
                points.push([
                    x as f32 * reference.point_dist - half,
                    z as f32 * reference.point_dist - half,
                ]);
            }
        }
        // The source bubble sort is stable for equal distances.
        points.sort_by(|a, b| square(*a).total_cmp(&square(*b)));
        let mut max_chance = 0.0f32;
        let chances = reference
            .types
            .iter()
            .map(|t| {
                max_chance += t.chance;
                max_chance
            })
            .collect();
        max_chance += reference.types.len() as f32 * reference.no_spawn_chance;
        let records: HashMap<_, _> = world.records.iter().map(|r| (&r.source, r)).collect();
        let mut prototypes = Vec::with_capacity(reference.types.len());
        for selection in &reference.types {
            let Some(source) = &selection.prototype else {
                prototypes.push(None);
                continue;
            };
            let prototype = world
                .prototypes
                .iter()
                .find(|p| p.source == *source)
                .ok_or_else(|| ReferenceError("Missing source prop prototype".into()))?;
            let owner = field_link(&records[source].fields, "m_GameObject")?;
            let object = records[owner];
            let component = object
                .components()
                .map(|source| records[source])
                .find(|r| r.class_name == "RandomSpawnedObjScript")
                .ok_or_else(|| {
                    ReferenceError("Missing source prop initialization caller".into())
                })?;
            let rock = match component.fields.get("mountain") {
                Some(WorldValue::Empty) => None,
                Some(WorldValue::Link(link)) => {
                    let r = records[link];
                    Some(RockRule {
                        renderer: optional_link(&r.fields, "r")?.cloned(),
                        materials: value_array(&r.fields, "materials")?
                            .iter()
                            .map(|v| {
                                if let WorldValue::Link(s) = v {
                                    Ok(s.clone())
                                } else {
                                    Err(ReferenceError("Invalid source rock material".into()))
                                }
                            })
                            .collect::<Result<_, _>>()?,
                        min: value_number(&r.fields, "sizeMin")?,
                        max: value_number(&r.fields, "sizeMax")?,
                        no_scale: value_number(&r.fields, "noScale")? != 0.0,
                    })
                }
                _ => {
                    return Err(ReferenceError(
                        "Missing checked source prop mountain field".into(),
                    ));
                }
            };
            prototypes.push(Some(PrototypeRule {
                source: source.clone(),
                scale: prototype.prefab_pose.scale,
                radius: super::placement::prototype_radius(world, prototype),
                rock,
            }));
        }
        Ok(Self {
            layer,
            reference,
            points,
            chances,
            max_chance,
            prototypes,
            curve: RoadCurve::new(world, seed),
            width: world.road.width,
            seed,
        })
    }
    pub fn retained_bytes(&self) -> usize {
        std::mem::size_of::<Self>()
            + self.points.capacity() * 8
            + self.chances.capacity() * 4
            + self.reference.types.capacity()
                * std::mem::size_of::<crate::reference::WorldObjectTypeReference>()
            + self.prototypes.capacity() * std::mem::size_of::<Option<PrototypeRule>>()
            + self
                .prototypes
                .iter()
                .flatten()
                .map(|p| {
                    p.source.file.capacity()
                        + p.rock.as_ref().map_or(0, |r| {
                            r.materials.capacity() * std::mem::size_of::<SourceId>()
                                + r.materials.iter().map(|s| s.file.capacity()).sum::<usize>()
                        })
                })
                .sum::<usize>()
    }
    pub fn candidate_count(&self) -> usize {
        self.points.len()
    }
    pub fn candidate(&self, unit: [i64; 2], ordinal: usize) -> Option<PropCandidate> {
        let offset = self.points[ordinal];
        let p = std::array::from_fn(|a| unit[a] as f32 + offset[a]);
        let seed = self.seed.wrapping_add(self.reference.seedplus);
        let fseed = ((seed as f64 - i32::MIN as f32 as f64)
            / (2147483648.0f64 - i32::MIN as f32 as f64))
            .clamp(0.0, 1.0) as f32;
        let n1 = source_perlin(p[0] * 0.1 + fseed, p[1] * 0.1 + fseed);
        let n2 = source_perlin(p[0] * 0.001 + fseed, p[1] * 0.001 + fseed);
        let n3 = source_perlin(p[0] * 0.00001 + fseed, p[1] * 0.00001 + fseed);
        let n = (((n1 + n2 + n3) * 0.6 - 1.5).clamp(-1.0, 1.0) * 2147483648.0 * 0.5
            + seed as f32 * 0.5)
            .clamp(-2147483648.0, 2147483648.0);
        let mut random = SourceRandom::new(source_int(n));
        let t = (random.next_range(0, 10000001) as f64 / 10000000.0).clamp(0.0, 1.0) as f32;
        let selected = self
            .chances
            .iter()
            .position(|limit| self.max_chance * t <= *limit)?;
        Some(PropCandidate {
            point: p,
            local: offset,
            ordinal: ordinal as u32,
            selected,
            rotation: random.next_range(0, 360),
            random: random.next_range(i32::MIN, i32::MAX),
        })
    }
    /// Selection draws finish before source biome and road rejection.
    pub fn place(
        &self,
        unit: [i64; 2],
        candidate: PropCandidate,
        context: TerrainContext<'_>,
    ) -> Option<Placement> {
        let selection = &self.reference.types[candidate.selected];
        let biome = super::biome(
            context.evaluator.color(candidate.point, 0.0),
            context.biome_override,
        );
        if [selection.in_desert, selection.in_winter, selection.in_grass][biome as usize] == 0 {
            return None;
        }
        if self.reference.affected_by_road != 0 && selection.affected_by_road != 0 {
            let x = self.curve.value(candidate.point[1]) * self.width;
            let distance = (x - candidate.point[0]) * (x - candidate.point[0]);
            if distance < selection.min_dist_from_road * selection.min_dist_from_road
                || (selection.max_dist_from_road != 0.0
                    && distance > selection.max_dist_from_road * selection.max_dist_from_road)
            {
                return None;
            }
        }
        let prototype = self.prototypes[candidate.selected].as_ref()?;
        let context = TerrainContext {
            roads: if self.reference.align_to_road != 0 {
                context.roads
            } else {
                &[]
            },
            foundations: if self.reference.align_to_building != 0 {
                context.foundations
            } else {
                &[]
            },
            ..context
        };
        let position = [
            candidate.point[0] as f64,
            (context.height(candidate.point) + self.reference.height_add) as f64,
            -candidate.point[1] as f64,
        ];
        let mut scale = prototype.scale;
        if let Some(rock) = &prototype.rock {
            let mut random = SourceRandom::new(source_int(candidate.random as f32));
            if rock.renderer.is_some() {
                random.next_range(0, rock.materials.len() as i32);
            }
            if !rock.no_scale {
                let value = random
                    .next_range((rock.min * 1000.0) as i32, (rock.max * 1000.0) as i32)
                    as f32
                    * 0.001;
                scale = [value; 3];
            }
        }
        let radius = prototype.radius * scale.iter().copied().fold(0.0, f32::max) as f64
            / prototype.scale.iter().copied().fold(0.0, f32::max) as f64;
        let yaw = -(candidate.rotation as f32).to_radians() * 0.5;
        Some(Placement {
            id: GeneratedId {
                layer: self.layer,
                generator: self.reference.source.clone(),
                unit,
                ordinal: candidate.ordinal,
                children: Vec::new(),
            },
            prototype: prototype.source.clone(),
            pose: AbsolutePose {
                position,
                rotation: [0.0, yaw.sin(), 0.0, yaw.cos()],
            },
            scale,
            bounds: WorldBounds {
                min: position.map(|v| v - radius),
                max: position.map(|v| v + radius),
            },
            object_seed: candidate.random,
        })
    }
    pub fn material(&self, prototype: &SourceId, seed: i32) -> Option<(&SourceId, &SourceId)> {
        let rock = self
            .prototypes
            .iter()
            .flatten()
            .find(|p| p.source == *prototype)?
            .rock
            .as_ref()?;
        let renderer = rock.renderer.as_ref()?;
        let mut random = SourceRandom::new(source_int(seed as f32));
        let index = random.next_range(0, rock.materials.len() as i32);
        Some((renderer, &rock.materials[index as usize]))
    }
}
fn square(p: [f32; 2]) -> f32 {
    p[0] * p[0] + p[1] * p[1]
}
fn source_int(value: f32) -> i32 {
    if !(-2147483648.0..2147483648.0).contains(&value) {
        i32::MIN
    } else {
        value as i32
    }
}
type Fields = crate::reference::WorldFields;
fn field_link<'a>(fields: &'a Fields, name: &str) -> Result<&'a SourceId, ReferenceError> {
    optional_link(fields, name)?.ok_or_else(|| ReferenceError(format!("Null source field {name}")))
}
fn optional_link<'a>(
    fields: &'a Fields,
    name: &str,
) -> Result<Option<&'a SourceId>, ReferenceError> {
    match fields.get(name) {
        Some(WorldValue::Link(s)) => Ok(Some(s)),
        Some(WorldValue::Empty) => Ok(None),
        _ => Err(ReferenceError(format!("Missing source link {name}"))),
    }
}
fn value_number(fields: &Fields, name: &str) -> Result<f32, ReferenceError> {
    match fields.get(name) {
        Some(WorldValue::Number(v)) => Ok(*v as f32),
        Some(WorldValue::Integer(v)) => Ok(*v as f32),
        Some(WorldValue::Bool(v)) => Ok(u8::from(*v) as f32),
        _ => Err(ReferenceError(format!("Missing source number {name}"))),
    }
}
fn value_array<'a>(fields: &'a Fields, name: &str) -> Result<&'a [WorldValue], ReferenceError> {
    match fields.get(name) {
        Some(WorldValue::Array(v)) => Ok(v),
        _ => Err(ReferenceError(format!("Missing source array {name}"))),
    }
}
