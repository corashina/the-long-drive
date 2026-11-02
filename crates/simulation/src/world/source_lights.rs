//! Source pole recipes and conservative light influence admission.
use super::spawn_rules::*;
use super::{AbsolutePose, GeneratedId, RoadSegment, WorldBounds, WorldFrame, WorldLayer};
use crate::reference::{
    LightId, LightKind, LightReference, ReferenceError, ReferenceManifest, RoadLampReference,
    ShadowReference, SourceId, Transform, WorldFields, WorldLightCookie, WorldPrototypeReference,
    WorldReference, WorldSourceRecord, WorldValue,
};
use rapier3d::math::{Pose, Rotation, Vector};

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct WorldLight {
    pub id: GeneratedId,
    pub pole: AbsolutePose,
    pub pose: AbsolutePose,
    pub record: LightReference,
    pub biome: Option<u8>,
    pub prototype: SourceId,
    pub scale: [f32; 3],
    pub range: f32,
    pub enabled: bool,
    pub intensity: f32,
    /// Prototype node of a structure light. None: a road light.
    pub node: Option<u32>,
}
impl WorldLight {
    /// Structure lights append their component identity; road lights share the pole identity.
    pub fn owned_by(&self, parent: &GeneratedId) -> bool {
        if self.biome.is_some() {
            return self.id == *parent;
        }
        self.id.layer == parent.layer
            && self.id.generator == parent.generator
            && self.id.unit == parent.unit
            && self.id.ordinal == parent.ordinal
            && self.id.children.len() == parent.children.len() + 1
            && self.id.children.starts_with(&parent.children)
    }
    pub fn influences(&self, bounds: WorldBounds) -> bool {
        (0..3)
            .map(|i| {
                let d = if self.pose.position[i] < bounds.min[i] {
                    bounds.min[i] - self.pose.position[i]
                } else if self.pose.position[i] > bounds.max[i] {
                    self.pose.position[i] - bounds.max[i]
                } else {
                    0.
                };
                d * d
            })
            .sum::<f64>()
            <= (self.range as f64).powi(2)
    }
    pub fn heap_bytes(&self) -> usize {
        super::contents::identity_heap(&self.id)
            + self.record.source.file.capacity()
            + self.prototype.file.capacity()
    }
}
struct RoadLightRecipe {
    reference: RoadLampReference,
    pole: RoadPole,
    local_light: Pose,
    light: LightReference,
    margin: f64,
}
pub struct SourceRoadLight {
    recipes: Vec<RoadLightRecipe>,
    seed: i32,
}
pub fn record<'a>(
    world: &'a WorldReference,
    id: &SourceId,
) -> Result<&'a WorldSourceRecord, ReferenceError> {
    world
        .records
        .iter()
        .find(|r| &r.source == id)
        .ok_or_else(|| ReferenceError(format!("Missing source record {}:{}", id.file, id.path_id)))
}
/// The `roadscript` record of a road prototype.
pub fn road_controller<'a>(
    world: &'a WorldReference,
    prototype: &SourceId,
) -> Result<&'a WorldSourceRecord, ReferenceError> {
    let owner = link(&record(world, prototype)?.fields, "m_GameObject")?;
    world
        .records
        .iter()
        .find(|r| {
            r.class_name == "roadscript"
                && matches!(r.fields.get("m_GameObject"), Some(WorldValue::Link(go)) if go == owner)
        })
        .ok_or_else(|| ReferenceError("Missing biome road controller".into()))
}
/// The prototype node of a source GameObject.
pub fn object_node(
    world: &WorldReference,
    prototype: &WorldPrototypeReference,
    object: &SourceId,
) -> Result<usize, ReferenceError> {
    let transform = world
        .records
        .iter()
        .find(|r| {
            matches!(r.class_name.as_str(), "Transform" | "RectTransform")
                && matches!(r.fields.get("m_GameObject"), Some(WorldValue::Link(go)) if go == object)
        })
        .ok_or_else(|| ReferenceError("Missing source object transform".into()))?;
    transform_node(prototype, &transform.source)
}
/// The prototype node of a source Transform.
pub fn transform_node(
    prototype: &WorldPrototypeReference,
    transform: &SourceId,
) -> Result<usize, ReferenceError> {
    prototype
        .nodes
        .iter()
        .position(|n| &n.source == transform)
        .ok_or_else(|| ReferenceError("Missing source transform node".into()))
}
/// Source `roadscript.RandomPoleRot` inputs of one road prototype.
#[derive(Clone)]
pub struct RoadPole {
    pub node: usize,
    /// Accumulated authored scale of the pole node.
    pub scale: Vector,
    pub chances: [f32; 2],
    pub angles: [[i32; 3]; 2],
}
/// World pose of a tilted road pole.
#[derive(Clone, Copy, Debug)]
pub struct PoleFrame {
    pub position: [f64; 3],
    pub rotation: Rotation,
}
impl PoleFrame {
    /// World position of a point in the pole frame (see `RoadPole::local`).
    pub fn point(&self, local: Vector) -> [f64; 3] {
        let offset = self.rotation * local;
        std::array::from_fn(|i| self.position[i] + offset[i] as f64)
    }
}
impl RoadPole {
    pub fn new(
        prototype: &WorldPrototypeReference,
        controller: &WorldSourceRecord,
    ) -> Result<Self, ReferenceError> {
        let f = &controller.fields;
        let node = transform_node(prototype, link(f, "randomPoleRotTransform")?)?;
        Ok(Self {
            node,
            scale: Vector::from_array(super::placement::node_pose(prototype, node).scale),
            chances: [
                number(f, "randomPoleRotChance1")?,
                number(f, "randomPoleRotChance2")?,
            ],
            angles: [
                [
                    integer(f, "randomPoleRotAngleX1")?,
                    integer(f, "randomPoleRotAngleY1")?,
                    integer(f, "randomPoleRotAngleZ1")?,
                ],
                [
                    integer(f, "randomPoleRotAngleX2")?,
                    integer(f, "randomPoleRotAngleY2")?,
                    integer(f, "randomPoleRotAngleZ2")?,
                ],
            ],
        })
    }
    /// Pose of a pole descendant in the pole frame. The translation includes the pole scale.
    pub fn local(
        &self,
        prototype: &WorldPrototypeReference,
        mut node: usize,
    ) -> Result<Pose, ReferenceError> {
        let mut local = Pose::IDENTITY;
        for _ in 0..prototype.nodes.len() {
            if node == self.node {
                local.translation *= self.scale;
                return Ok(local);
            }
            let n = &prototype.nodes[node];
            let t = &n.transform;
            let ancestor = crate::vehicle::to_rapier(crate::vehicle::VehiclePose {
                position: t.translation,
                rotation: t.rotation,
            });
            local.translation = ancestor.translation
                + ancestor.rotation * (Vector::from_array(t.scale) * local.translation);
            local.rotation = ancestor.rotation * local.rotation;
            node = n
                .parent
                .ok_or_else(|| ReferenceError("Source node is outside its pole".into()))?
                as usize;
        }
        Err(ReferenceError("Source pole ancestor cycle".into()))
    }
    /// Road frame of the pole node with the source tilt.
    pub fn frame(
        &self,
        prototype: &WorldPrototypeReference,
        segment: &RoadSegment,
        seed: i32,
    ) -> PoleFrame {
        let frames = road_mesh::frames(
            prototype.nodes.iter().map(|node| road_mesh::RoadNode {
                parent: node.parent,
                translation: node.transform.translation,
                rotation: node.transform.rotation,
                scale: node.transform.scale,
            }),
            &prototype.controls,
            &segment.source_rotations,
        );
        let frame = frames[self.node];
        let rotation = super::lamps::flatten_then_tilt(
            frame.rotation,
            super::lamps::source_angles(
                seed as u64,
                super::lamps::RoadLampId(segment.index as i64),
                self.chances,
                self.angles,
            ),
        );
        let origin = segment.position();
        PoleFrame {
            position: std::array::from_fn(|i| origin[i] + frame.position[i] as f64),
            rotation,
        }
    }
}
pub fn light_record(
    world: &WorldReference,
    id: &SourceId,
) -> Result<LightReference, ReferenceError> {
    light(&record(world, id)?.fields, id, &world.light_cookies)
}
/// Resolves a source Light component. Only a spot cookie has an exported lighting texture binding.
fn light(
    f: &WorldFields,
    id: &SourceId,
    cookies: &[WorldLightCookie],
) -> Result<LightReference, ReferenceError> {
    let cookie = match optional(f, "m_Cookie")? {
        Some(source) => Some(
            cookies
                .iter()
                .find(|c| &c.source == source)
                .ok_or_else(|| {
                    ReferenceError("World lamp cookie requires its checked texture binding".into())
                })?
                .texture,
        ),
        None => None,
    };
    let kind = match integer(f, "m_Type")? {
        0 => LightKind::Spot,
        2 if cookie.is_none() => LightKind::Point,
        _ => return Err(ReferenceError("Unresolved world lamp type".into())),
    };
    let c = fields(value(f, "m_Color")?)?;
    let shadows = fields(value(f, "m_Shadows")?)?;
    let mask = match value(fields(value(f, "m_CullingMask")?)?, "m_Bits")? {
        WorldValue::Integer(n) => *n as u32,
        _ => return Err(ReferenceError("Invalid source lamp mask".into())),
    };
    Ok(LightReference {
        id: LightId(0),
        source: id.clone(),
        node: None,
        pose: Transform {
            translation: [0.; 3],
            rotation: [0., 0., 0., 1.],
            scale: [1.; 3],
        },
        kind,
        color: [
            number(c, "r")?,
            number(c, "g")?,
            number(c, "b")?,
            number(c, "a")?,
        ],
        serialized_intensity: number(f, "m_Intensity")?,
        startup_intensity: number(f, "m_Intensity")?,
        range_meters: number(f, "m_Range")?,
        full_cone_degrees: number(f, "m_SpotAngle")?,
        inner_cone_degrees: number(f, "m_InnerSpotAngle")?,
        enabled: flag(f, "m_Enabled")?,
        culling_mask: mask,
        shadows: ShadowReference {
            kind: integer(shadows, "m_Type")? as u8,
            strength: number(shadows, "m_Strength")?,
            bias: number(shadows, "m_Bias")?,
            normal_bias: number(shadows, "m_NormalBias")?,
            near_plane: number(shadows, "m_NearPlane")?,
            resolution: integer(shadows, "m_Resolution")?,
            custom_resolution: integer(shadows, "m_CustomResolution")?,
        },
        cookie,
        cookie_size: number(f, "m_CookieSize")?,
    })
}
impl SourceRoadLight {
    pub fn new(reference: &ReferenceManifest, seed: i32) -> Result<Self, ReferenceError> {
        let world = &reference.world;
        let mut recipes = Vec::with_capacity(3);
        for source in &world.road.prototypes {
            let prototype = world
                .prototypes
                .iter()
                .find(|p| &p.source == source)
                .unwrap();
            let controller = road_controller(world, source)?;
            let pole = RoadPole::new(prototype, controller)?;
            let koz = record(world, link(&controller.fields, "koz")?)?;
            let light = light_record(world, link(&koz.fields, "LLight")?)?;
            let owner = link(&record(world, &light.source)?.fields, "m_GameObject")?;
            let local_light = pole.local(prototype, object_node(world, prototype, owner)?)?;
            let mut road = reference.lighting.road_lamp.clone();
            road.source = koz.source.clone();
            road.road_source = controller.source.clone();
            road.asset = prototype.asset;
            road.rotation_chances = pole.chances;
            road.rotation_angles = pole.angles;
            road.road_pose.scale = pole.scale.to_array();
            let radius = super::placement::node_pose(prototype, pole.node)
                .translation
                .iter()
                .map(|v| (*v as f64).powi(2))
                .sum::<f64>()
                .sqrt();
            let margin =
                radius + local_light.translation.length() as f64 + light.range_meters as f64;
            recipes.push(RoadLightRecipe {
                reference: road,
                pole,
                local_light,
                light,
                margin,
            });
        }
        Ok(Self { recipes, seed })
    }
    pub fn influence_margin(&self) -> f64 {
        self.recipes.iter().map(|r| r.margin).fold(0., f64::max)
    }
    pub fn retained_bytes(&self) -> usize {
        std::mem::size_of::<Self>()
            + self.recipes.capacity() * std::mem::size_of::<RoadLightRecipe>()
            + self
                .recipes
                .iter()
                .map(|r| {
                    r.reference.source.file.capacity()
                        + r.reference.road_source.file.capacity()
                        + r.reference.generator_source.file.capacity()
                        + r.reference.lights.capacity() * std::mem::size_of::<LightId>()
                        + r.light.source.file.capacity()
                })
                .sum::<usize>()
    }
    pub fn place(&self, reference: &WorldReference, segment: &RoadSegment) -> Option<WorldLight> {
        let recipe = &self.recipes[segment.biome as usize];
        let prototype = reference
            .prototypes
            .iter()
            .find(|p| p.source == reference.road.prototypes[segment.biome as usize])
            .unwrap();
        let pole = recipe.pole.frame(prototype, segment, self.seed);
        Some(WorldLight {
            id: GeneratedId {
                layer: WorldLayer::Lamp,
                generator: recipe.reference.generator_source.clone(),
                unit: [segment.index as i64, 0],
                ordinal: 0,
                children: vec![(recipe.reference.source.clone(), 0)],
            },
            pole: AbsolutePose {
                position: pole.position,
                rotation: pole.rotation.to_array(),
            },
            pose: AbsolutePose {
                position: pole.point(recipe.local_light.translation),
                rotation: (pole.rotation * recipe.local_light.rotation).to_array(),
            },
            range: recipe.light.range_meters,
            record: recipe.light.clone(),
            biome: Some(segment.biome),
            prototype: prototype.nodes[recipe.pole.node].source.clone(),
            scale: recipe.reference.road_pose.scale,
            enabled: true,
            intensity: recipe.light.startup_intensity,
            node: None,
        })
    }
}
/// Select all influencing source lights. Visibility and pole mesh distance do not reject light influence.
pub fn required_world_lights(
    frame: &WorldFrame<'_>,
    receiver_bounds: &[WorldBounds],
    road_enabled: bool,
    output: &mut Vec<GeneratedId>,
) {
    let mut count = 0;
    for light in frame.lights {
        if !light.enabled
            || light.intensity <= 0.
            || (light.biome.is_some() && !road_enabled)
            || light.record.culling_mask == 0
        {
            continue;
        }
        if receiver_bounds
            .iter()
            .any(|bounds| light.influences(*bounds))
            && !output[..count].contains(&light.id)
        {
            if count == output.len() {
                output.push(light.id.clone());
            } else if output[count] != light.id {
                output[count] = light.id.clone();
            }
            count += 1;
        }
    }
    output.truncate(count);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn world_light_cookie_resolves_to_its_lighting_texture() {
        // Source flashlight Spot Light (sharedassets1.assets:123692) fields.
        let number = |name: &str, v: f64| serde_json::json!({"name": name, "value": {"kind": "number", "value": v}});
        let integer = |name: &str, v: i64| serde_json::json!({"name": name, "value": {"kind": "integer", "value": v}});
        let cookie = SourceId {
            file: "sharedassets1.assets".into(),
            path_id: 990,
        };
        let fields: WorldFields = serde_json::from_value(serde_json::json!([
            {"name": "m_Color", "value": {"kind": "record", "value": [
                number("a", 1.), number("b", 1.), number("g", 1.), number("r", 1.)]}},
            {"name": "m_Cookie", "value": {"kind": "link", "value": cookie}},
            number("m_CookieSize", 10.),
            {"name": "m_CullingMask", "value": {"kind": "record", "value": [
                integer("m_Bits", 4294967295)]}},
            integer("m_Enabled", 1),
            number("m_InnerSpotAngle", 62.188602447509766),
            number("m_Intensity", 1.5),
            number("m_Range", 15.),
            {"name": "m_Shadows", "value": {"kind": "record", "value": [
                number("m_Bias", 0.05), integer("m_CustomResolution", -1),
                number("m_NearPlane", 0.2), number("m_NormalBias", 0.4),
                integer("m_Resolution", -1), number("m_Strength", 1.), integer("m_Type", 1)]}},
            number("m_SpotAngle", 80.),
            integer("m_Type", 0),
        ]))
        .unwrap();
        let id = SourceId {
            file: "sharedassets1.assets".into(),
            path_id: 123692,
        };
        let bindings = [WorldLightCookie {
            source: cookie,
            texture: 4,
        }];
        let record = light(&fields, &id, &bindings).unwrap();
        assert_eq!((record.kind, record.cookie), (LightKind::Spot, Some(4)));
        assert!(light(&fields, &id, &[]).is_err());
    }
}
