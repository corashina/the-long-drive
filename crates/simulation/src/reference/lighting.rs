//! Source cycle and lamp controls. Values retain Unity units.
use super::rendering::*;
use super::{
    AssetId, AssetUsage, ColliderId, LightControlId, LightId, NodeId, ReferenceError,
    ReferenceManifest, SourceId, Transform, finite, hash, ids, require, transform,
};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GradientColor {
    pub time: f32,
    pub value: [f32; 4],
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GradientAlpha {
    pub time: f32,
    pub value: f32,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GradientReference {
    pub mode: u8,
    pub colors: Vec<GradientColor>,
    pub alphas: Vec<GradientAlpha>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CloudReference {
    pub asset: AssetId,
    pub node: NodeId,
    pub material: SkyMaterialReference,
    pub day_color: [f32; 4],
    pub transition_color: [f32; 4],
    pub night_color: [f32; 4],
    pub min_speed: f32,
    pub mid_speed: f32,
    pub max_speed: f32,
    pub min_density: f32,
    pub max_density: f32,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CycleReference {
    pub source: SourceId,
    pub day_seconds: f32,
    pub night_seconds: f32,
    pub dawn_seconds: f32,
    pub dusk_seconds: f32,
    pub startup_offset_seconds: f32,
    pub color: GradientReference,
    pub atmosphere_thickness: GradientReference,
    pub day_intensity: f32,
    pub night_intensity: f32,
    pub noon_intensity: f32,
    pub day_ambient: f32,
    pub night_ambient: f32,
    pub min_reflection: f32,
    pub max_reflection: f32,
    pub min_exposure: f32,
    pub max_exposure: f32,
    pub day_angle_min: f32,
    pub day_angle_max: f32,
    pub day_yaw_degrees: f32,
    pub midnight_angle_dawn: f32,
    pub midnight_angle_dusk: f32,
    pub min_moon_angle: f32,
    pub max_moon_angle: f32,
    pub color_delay: f32,
    pub star_speed: f32,
    pub cloud_perlin_spread: f32,
    pub cloud_perlin_speed: f32,
    pub flare_lerp_start: f32,
    pub flare_lerp_end: f32,
    pub biome_refresh_seconds: f32,
    pub desert_color: [f32; 4],
    pub sky_colors: [[f32; 4]; 3],
    pub day_ground_colors: [[f32; 4]; 3],
    pub night_ground_colors: [[f32; 4]; 3],
    pub day_fog_colors: [[f32; 4]; 3],
    pub night_fog_colors: [[f32; 4]; 3],
    pub day_fog_densities: [f32; 3],
    pub night_fog_densities: [f32; 3],
    pub temp_days: [f32; 3],
    pub temp_nights: [f32; 3],
    pub temp_add_noons: [f32; 3],
    /// postprocesscontrol.TemperatureValues: main-view grading temperature per biome.
    pub temperature_values: [f32; 3],
    pub flare_dawn: [f32; 3],
    pub flare_day: [f32; 3],
    pub flare_dusk: [f32; 3],
    pub flare_night: [f32; 3],
    pub day_color: [f32; 4],
    pub night_color: [f32; 4],
    pub dawn_color: [f32; 4],
    pub dusk_color: [f32; 4],
    pub star_color_day: [f32; 4],
    pub star_color_night: [f32; 4],
    pub clouds: Vec<CloudReference>,
    pub star_material: SkyMaterialReference,
    pub sky_resources: Vec<ResourceReference>,
    pub sun_flares: Vec<SunFlareReference>,
    pub stars: Vec<StarReference>,
    pub sky_geometry: SkyGeometryReference,
    pub cloud_shadow_cookie: u32,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FogReference {
    pub enabled: bool,
    pub mode: u8,
    pub density: f32,
    pub color: [f32; 4],
    pub start: f32,
    pub end: f32,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ShadowReference {
    pub kind: u8,
    pub strength: f32,
    pub bias: f32,
    pub normal_bias: f32,
    pub near_plane: f32,
    pub resolution: i32,
    pub custom_resolution: i32,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LightKind {
    Spot,
    Directional,
    Point,
    Area,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LightReference {
    pub id: LightId,
    pub source: SourceId,
    pub node: Option<NodeId>,
    pub pose: Transform,
    pub kind: LightKind,
    pub color: [f32; 4],
    pub serialized_intensity: f32,
    pub startup_intensity: f32,
    pub range_meters: f32,
    pub full_cone_degrees: f32,
    pub inner_cone_degrees: f32,
    pub enabled: bool,
    pub culling_mask: u32,
    pub shadows: ShadowReference,
    pub cookie: Option<u32>,
    pub cookie_size: f32,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LightControllerKind {
    Switch,
    Headlight,
    Cabin,
    Rear,
    Road,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LampSignal {
    Switch,
    Off,
    Low,
    High,
    Cabin,
    Tail,
    Brake,
    Reverse,
    Road,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LampStateReference {
    pub signal: LampSignal,
    pub lights: Vec<LightId>,
    pub intensity: f32,
    pub renderers: Vec<NodeId>,
    pub on_material: Option<LightingMaterialLink>,
    pub off_material: Option<LightingMaterialLink>,
    pub broken: bool,
    pub working_nodes: Vec<NodeId>,
    pub broken_nodes: Vec<NodeId>,
    pub dont_overwrite: bool,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AnimatedLightPose {
    pub node: NodeId,
    pub states: Vec<Transform>,
    pub rule: String,
    pub speed: f32,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LightControllerReference {
    pub id: LightControlId,
    pub source: SourceId,
    pub kind: LightControllerKind,
    pub node: NodeId,
    pub aim_colliders: Vec<ColliderId>,
    pub animated_poses: Vec<AnimatedLightPose>,
    pub startup_on: bool,
    pub current_state: usize,
    pub requested_state: usize,
    pub energy: f32,
    pub energy_move: f32,
    pub enabled: bool,
    pub electronics_source: Option<SourceId>,
    pub electronics_output: f32,
    pub electronics_threshold: f32,
    pub reverse_switch: bool,
    pub states: Vec<LampStateReference>,
    pub usable_source: Option<SourceId>,
    pub usable_event: Option<String>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RoadLampReference {
    pub asset: AssetId,
    pub source: SourceId,
    pub road_source: SourceId,
    pub generator_source: SourceId,
    pub spacing_meters: f32,
    pub road_pose: Transform,
    pub rotation_node: NodeId,
    pub rotation_chances: [f32; 2],
    pub rotation_angles: [[i32; 3]; 2],
    pub controller: LightControlId,
    pub lights: Vec<LightId>,
    pub global_enabled: bool,
    pub startup_on: bool,
    pub electronics_threshold: f32,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LightingRecord {
    pub source: SourceId,
    pub lights_use_linear_intensity: bool,
    pub quality: LightingQualityReference,
    pub cameras: Vec<LightingCameraReference>,
    pub color_space: u8,
    pub ambient_mode: u8,
    pub ambient_intensity: f32,
    pub ambient_sky: [f32; 4],
    pub ambient_equator: [f32; 4],
    pub ambient_ground: [f32; 4],
    pub ambient_probe: Vec<f32>,
    pub startup_ambient: f32,
    pub startup_reflection: f32,
    pub startup_fog_density: f32,
    pub reflection_intensity: f32,
    pub fog: FogReference,
    pub skybox: SkyMaterialReference,
    pub sun: LightId,
    pub default_spot_cookie: u32,
    pub lights: Vec<LightReference>,
    pub cycle: CycleReference,
    pub controllers: Vec<LightControllerReference>,
    pub road_lamp: RoadLampReference,
    pub textures: Vec<LightingTextureRecord>,
    pub materials: Vec<MaterialBindingReference>,
}

impl LightingRecord {
    pub fn validate(&self, manifest: &ReferenceManifest) -> Result<(), ReferenceError> {
        let check = |ok: bool, source: &SourceId, field: &str| {
            require(ok, &format!("{}:{} {field}", source.file, source.path_id))
        };
        let node = |id: NodeId| manifest.nodes.iter().find(|n| n.id == id);
        let valid_source = |s: &SourceId| {
            s.path_id != 0 && manifest.source_hashes.iter().any(|h| h.file == s.file)
        };
        let valid_resource = |s: &SourceId, path: &str, digest: &str| {
            valid_source(s)
                && path.starts_with("reference/")
                && !path.contains("..")
                && hash(digest)
                && manifest.assets.iter().any(|a| {
                    a.dependencies
                        .iter()
                        .any(|d| d.file == path && d.sha256 == digest)
                })
        };
        let texture_ids = ids(
            self.textures.iter().map(|t| t.id),
            "duplicate lighting texture ID",
        )?;
        let light_ids = ids(self.lights.iter().map(|l| l.id), "duplicate light ID")?;
        let control_ids = ids(
            self.controllers.iter().map(|c| c.id),
            "duplicate light control ID",
        )?;
        check(
            valid_source(&self.source)
                && self.color_space <= 1
                && self.ambient_mode <= 3
                && self.ambient_probe.len() == 27
                && finite(&self.ambient_probe),
            &self.source,
            "startup settings",
        )?;
        check(
            finite(&[
                self.ambient_intensity,
                self.reflection_intensity,
                self.fog.density,
                self.fog.start,
                self.fog.end,
            ]) && finite(&self.ambient_sky)
                && finite(&self.ambient_equator)
                && finite(&self.ambient_ground)
                && finite(&self.fog.color)
                && self.fog.mode >= 1
                && self.fog.mode <= 3
                && self.fog.density >= 0.
                && self.fog.end > self.fog.start,
            &self.source,
            "ambient/fog",
        )?;
        for t in &self.textures {
            check(
                t.id > 0
                    && valid_resource(&t.source, &t.path, &t.sha256)
                    && valid_resource(
                        &t.source_data.source,
                        &t.source_data.path,
                        &t.source_data.sha256,
                    )
                    && valid_resource(&t.mip_data.source, &t.mip_data.path, &t.mip_data.sha256)
                    && t.mip_data.source == t.source
                    && t.mip_data.path.ends_with(".smip")
                    && t.mip_count <= 32 - t.width.max(t.height).leading_zeros()
                    && t.width > 0
                    && t.height > 0
                    && t.color_space <= 1
                    && t.filter_mode <= 2
                    && t.wrap_u <= 2
                    && t.wrap_v <= 2
                    && t.mip_count > 0
                    && t.mip_bias.is_finite()
                    && matches!(t.sample_channel.as_str(), "alpha" | "rgba")
                    && (t.source_format != 1 || t.sample_channel == "alpha"),
                &t.source,
                "texture",
            )?;
        }
        let sky = |m: &SkyMaterialReference| -> Result<(), ReferenceError> {
            check(
                valid_resource(
                    &m.source_data.source,
                    &m.source_data.path,
                    &m.source_data.sha256,
                ) && valid_source(&m.source)
                    && valid_resource(&m.shader.source, &m.shader.path, &m.shader.sha256)
                    && !m.shader.name.is_empty()
                    && valid_resource(
                        &m.shader.compiled_data.source,
                        &m.shader.compiled_data.path,
                        &m.shader.compiled_data.sha256,
                    ),
                &m.source,
                "sky shader",
            )?;
            check(
                m.floats.values().all(|v| v.is_finite())
                    && m.colors.values().all(|v| finite(v))
                    && m.textures.iter().all(|t| {
                        texture_ids.contains(&t.texture) && finite(&t.scale) && finite(&t.offset)
                    }),
                &m.source,
                "sky properties",
            )
        };
        check(
            finite(&[
                self.startup_ambient,
                self.startup_reflection,
                self.startup_fog_density,
            ]),
            &self.source,
            "startup overrides",
        )?;
        sky(&self.skybox)?;
        let prototypes = manifest.item_prototypes.iter();
        for particle in manifest
            .particles
            .iter()
            .chain(
                prototypes
                    .clone()
                    .filter_map(|p| p.consumable.as_ref().map(|c| &c.ember.particle)),
            )
            .chain(
                prototypes
                    .filter_map(|p| p.hose.as_ref())
                    .flat_map(|h| h.ends.iter().flat_map(|e| [&e.visual, &e.splash])),
            )
        {
            for material in &particle.materials {
                sky(material)?;
            }
        }
        for property in ["_SunSize", "_SunSizeConvergence"] {
            check(
                self.skybox.floats.contains_key(property),
                &self.skybox.source,
                &format!("floats.{property} missing"),
            )?;
        }
        for l in &self.lights {
            check(
                l.id.0 > 0
                    && valid_source(&l.source)
                    && l.node.is_none_or(|id| node(id).is_some())
                    && transform(&l.pose)
                    && finite(&l.color)
                    && finite(&[
                        l.serialized_intensity,
                        l.startup_intensity,
                        l.range_meters,
                        l.full_cone_degrees,
                        l.inner_cone_degrees,
                        l.cookie_size,
                    ])
                    && l.serialized_intensity >= 0.
                    && l.startup_intensity >= 0.
                    && l.range_meters > 0.
                    && l.full_cone_degrees > 0.
                    && l.full_cone_degrees <= 180.
                    && l.inner_cone_degrees >= 0.
                    && l.inner_cone_degrees <= l.full_cone_degrees
                    && l.culling_mask != 0
                    && l.cookie.is_none_or(|id| texture_ids.contains(&id)),
                &l.source,
                "light pose/cone/cookie",
            )?;
            check(
                l.shadows.kind <= 2
                    && finite(&[
                        l.shadows.strength,
                        l.shadows.bias,
                        l.shadows.normal_bias,
                        l.shadows.near_plane,
                    ])
                    && (0.0..=1.0).contains(&l.shadows.strength)
                    && l.shadows.bias >= 0.
                    && l.shadows.normal_bias >= 0.
                    && l.shadows.near_plane >= 0.,
                &l.source,
                "shadows",
            )?;
        }
        check(
            texture_ids.contains(&self.default_spot_cookie),
            &self.source,
            "default spotlight cookie",
        )?;
        check(light_ids.contains(&self.sun), &self.source, "sun")?;
        let c = &self.cycle;
        check(
            [
                c.day_angle_min,
                c.day_angle_max,
                c.day_yaw_degrees,
                c.midnight_angle_dawn,
                c.midnight_angle_dusk,
                c.min_moon_angle,
                c.max_moon_angle,
            ]
            .iter()
            .all(|v| v.is_finite() && (-360.0..=360.0).contains(v))
                && c.day_angle_min < c.day_angle_max
                && c.min_moon_angle < c.max_moon_angle
                && [
                    c.day_intensity,
                    c.night_intensity,
                    c.noon_intensity,
                    c.day_ambient,
                    c.night_ambient,
                    c.min_reflection,
                    c.max_reflection,
                    c.min_exposure,
                    c.max_exposure,
                ]
                .iter()
                .all(|v| v.is_finite() && *v >= 0.)
                && c.min_exposure <= c.max_exposure
                && c.min_reflection <= c.max_reflection,
            &c.source,
            "cycle bounds",
        )?;
        check(
            valid_source(&c.source)
                && finite(&[
                    c.day_seconds,
                    c.night_seconds,
                    c.dawn_seconds,
                    c.dusk_seconds,
                    c.startup_offset_seconds,
                    c.day_intensity,
                    c.night_intensity,
                    c.noon_intensity,
                    c.day_ambient,
                    c.night_ambient,
                    c.min_reflection,
                    c.max_reflection,
                    c.min_exposure,
                    c.max_exposure,
                    c.day_angle_min,
                    c.day_angle_max,
                    c.day_yaw_degrees,
                    c.midnight_angle_dawn,
                    c.midnight_angle_dusk,
                    c.min_moon_angle,
                    c.max_moon_angle,
                    c.color_delay,
                    c.star_speed,
                    c.cloud_perlin_spread,
                    c.cloud_perlin_speed,
                    c.flare_lerp_start,
                    c.flare_lerp_end,
                    c.biome_refresh_seconds,
                ])
                && c.day_seconds > c.dawn_seconds + c.dusk_seconds
                && c.dawn_seconds > 0.
                && c.dusk_seconds > 0.
                && c.night_seconds > 0.
                && c.startup_offset_seconds >= 0.
                && c.startup_offset_seconds <= c.day_seconds + c.night_seconds
                && c.biome_refresh_seconds > 0.,
            &c.source,
            "cycle durations/angles",
        )?;
        for g in [&c.color, &c.atmosphere_thickness] {
            check(
                g.mode <= 1
                    && !g.colors.is_empty()
                    && !g.alphas.is_empty()
                    && g.colors.iter().all(|k| {
                        k.time.is_finite() && (0.0..=1.0).contains(&k.time) && finite(&k.value)
                    })
                    && g.alphas.iter().all(|k| {
                        k.time.is_finite() && k.value.is_finite() && (0.0..=1.0).contains(&k.time)
                    })
                    && g.colors.windows(2).all(|k| k[0].time <= k[1].time)
                    && g.alphas.windows(2).all(|k| k[0].time <= k[1].time),
                &c.source,
                "gradient",
            )?;
        }
        for values in [
            &c.sky_colors,
            &c.day_ground_colors,
            &c.night_ground_colors,
            &c.day_fog_colors,
            &c.night_fog_colors,
        ] {
            check(values.iter().all(|v| finite(v)), &c.source, "biome colors")?;
        }
        for values in [
            &c.day_fog_densities,
            &c.night_fog_densities,
            &c.flare_dawn,
            &c.flare_day,
            &c.flare_dusk,
            &c.flare_night,
        ] {
            check(
                finite(values) && values.iter().all(|v| *v >= 0.),
                &c.source,
                "biome scalars",
            )?;
        }
        for values in [
            &c.temp_days,
            &c.temp_nights,
            &c.temp_add_noons,
            &c.temperature_values,
        ] {
            check(finite(values), &c.source, "visual temperature arrays")?;
        }
        for values in [
            &c.day_color,
            &c.night_color,
            &c.dawn_color,
            &c.dusk_color,
            &c.star_color_day,
            &c.star_color_night,
            &c.desert_color,
        ] {
            check(finite(values), &c.source, "cycle colors")?;
        }
        sky(&c.star_material)?;
        check(
            c.star_material
                .textures
                .first()
                .is_some_and(|link| link.property == "_MainTex"),
            &c.star_material.source,
            "textures[0]._MainTex missing",
        )?;
        check(
            c.clouds.len() == 2
                && texture_ids.contains(&c.cloud_shadow_cookie)
                && !c.sky_resources.is_empty(),
            &c.source,
            "sky resources",
        )?;
        for resource in &c.sky_resources {
            check(
                valid_resource(&resource.source, &resource.path, &resource.sha256),
                &resource.source,
                "sky resource hash",
            )?;
        }
        for cloud in &c.clouds {
            sky(&cloud.material)?;
            for property in ["_ScatterMap0", "_ScatterMap1", "_DensityMap", "_TextureMap"] {
                check(
                    cloud
                        .material
                        .textures
                        .iter()
                        .any(|link| link.property == property),
                    &cloud.material.source,
                    &format!("textures.{property} missing"),
                )?;
            }
            check(
                node(cloud.node).is_some_and(|n| n.asset == cloud.asset)
                    && cloud.asset == c.sky_geometry.asset,
                &cloud.material.source,
                "cloud geometry",
            )?;
            check(
                finite(&cloud.day_color)
                    && finite(&cloud.transition_color)
                    && finite(&cloud.night_color)
                    && finite(&[
                        cloud.min_speed,
                        cloud.mid_speed,
                        cloud.max_speed,
                        cloud.min_density,
                        cloud.max_density,
                    ]),
                &cloud.material.source,
                "cloud",
            )?;
        }
        let material = |m: &LightingMaterialLink| {
            self.materials.iter().any(|v| {
                v.asset == m.asset && v.gltf_material == m.gltf_material && v.source == m.source
            }) && valid_source(&m.source)
        };
        ids(
            self.materials.iter().map(|m| (m.asset, m.gltf_material)),
            "duplicate lighting material binding",
        )?;
        for m in &self.materials {
            check(
                valid_source(&m.source) && manifest.assets.iter().any(|a| a.id == m.asset),
                &m.source,
                "material asset",
            )?;
        }
        for control in &self.controllers {
            let owner = node(control.node);
            check(
                control.id.0 > 0
                    && valid_source(&control.source)
                    && owner.is_some()
                    && !control.states.is_empty()
                    && control.current_state < control.states.len()
                    && control.requested_state < control.states.len()
                    && finite(&[
                        control.energy,
                        control.energy_move,
                        control.electronics_output,
                        control.electronics_threshold,
                    ])
                    && (0.0..=1.0).contains(&control.energy)
                    && control.energy_move >= 0.
                    && control.electronics_output >= 0.
                    && control.electronics_source.as_ref().is_none_or(valid_source)
                    // Car controllers belong to the car prototypes.
                    && !matches!(
                        control.kind,
                        LightControllerKind::Headlight
                            | LightControllerKind::Rear
                            | LightControllerKind::Cabin
                    ),
                &control.source,
                "controller state/node",
            )?;
            if control.kind == LightControllerKind::Switch {
                check(
                    !control.aim_colliders.is_empty()
                        && control.states.iter().all(|s| {
                            !s.lights.is_empty()
                                && !s.renderers.is_empty()
                                && s.on_material.is_some()
                                && s.off_material.is_some()
                        }),
                    &control.source,
                    "switch lamp bindings",
                )?;
            }
            if control.kind == LightControllerKind::Headlight {
                check(
                    control.states.len() == 3
                        && control.states[1..].iter().all(|s| {
                            !s.lights.is_empty()
                                && s.lights.iter().all(|id| {
                                    self.lights
                                        .iter()
                                        .any(|l| l.id == *id && l.cookie.is_some())
                                })
                        }),
                    &control.source,
                    "required beam cookie",
                )?;
            }
            let asset = owner.unwrap().asset;
            let local = |id: NodeId| node(id).is_some_and(|n| n.asset == asset);
            check(
                control.aim_colliders.iter().all(|id| {
                    manifest
                        .colliders
                        .iter()
                        .any(|c| c.id == *id && local(c.node))
                }),
                &control.source,
                "aim colliders",
            )?;
            for pose in &control.animated_poses {
                check(
                    local(pose.node)
                        && pose.states.len() >= 2
                        && pose.states.iter().all(transform)
                        && pose.rule == "current_pose_lerp"
                        && pose.speed.is_finite()
                        && pose.speed > 0.,
                    &control.source,
                    "animated pose",
                )?;
            }
            for state in &control.states {
                check(
                    state.intensity.is_finite()
                        && state.intensity >= 0.
                        && state.lights.iter().all(|id| {
                            light_ids.contains(id)
                                && self
                                    .lights
                                    .iter()
                                    .any(|l| l.id == *id && l.node.is_some_and(local))
                        })
                        && state
                            .renderers
                            .iter()
                            .chain(&state.working_nodes)
                            .chain(&state.broken_nodes)
                            .all(|id| local(*id))
                        && state
                            .on_material
                            .as_ref()
                            .is_none_or(|m| m.asset == asset && material(m))
                        && state
                            .off_material
                            .as_ref()
                            .is_none_or(|m| m.asset == asset && material(m)),
                    &control.source,
                    "lamp links",
                )?;
                check(
                    state.renderers.is_empty() || state.off_material.is_some(),
                    &control.source,
                    "off material",
                )?;
            }
        }
        let expected_flares = [57088, 57090, 57089];
        check(
            c.sun_flares.len() == expected_flares.len(),
            &c.source,
            "sun flare chain",
        )?;
        for (flare, pid) in c.sun_flares.iter().zip(expected_flares) {
            check(
                flare.source.file == "level1"
                    && flare.source.path_id == pid
                    && flare.component.source == flare.source
                    && c.sky_resources.iter().any(|r| {
                        r.source == flare.component.source
                            && r.path == flare.component.path
                            && r.sha256 == flare.component.sha256
                    })
                    && valid_resource(
                        &flare.component.source,
                        &flare.component.path,
                        &flare.component.sha256,
                    )
                    && flare.flare.source.file == "sharedassets1.assets"
                    && flare.flare.source.path_id == 4516
                    && valid_resource(&flare.flare.source, &flare.flare.path, &flare.flare.sha256)
                    && self.textures.iter().any(|t| {
                        t.id == flare.atlas
                            && t.source.file == "sharedassets1.assets"
                            && t.source.path_id == 744
                    }),
                &flare.source,
                "sun flare resource links",
            )?;
        }
        check(
            manifest
                .assets
                .iter()
                .any(|a| a.id == c.sky_geometry.asset && a.usage == AssetUsage::SkyPrototype)
                && node(c.sky_geometry.root).is_some_and(|n| n.asset == c.sky_geometry.asset)
                && transform(&c.sky_geometry.pose),
            &c.source,
            "sky prototype",
        )?;
        check(self.cycle.stars.len() == 2, &c.source, "star systems")?;
        for star in &c.stars {
            check(
                valid_source(&star.source)
                    && transform(&star.pose)
                    && star.max_particles > 0
                    && finite(&star.shape_scale)
                    && finite(&star.start_color)
                    && finite(&[
                        star.radius,
                        star.radius_thickness,
                        star.size_min,
                        star.size_max,
                        star.speed,
                        star.emission_rate,
                        star.duration_seconds,
                        star.max_screen_size,
                    ])
                    && finite(&star.burst_counts)
                    && star.radius > 0.
                    && star.size_min > 0.
                    && star.size_max >= star.size_min
                    && star.infinite_lifetime == star.lifetime_seconds.is_none()
                    && star
                        .lifetime_seconds
                        .is_none_or(|v| v.is_finite() && v > 0.),
                &star.source,
                "star values",
            )?;
        }
        let q = &self.quality;
        check(
            valid_source(&q.source)
                && q.pixel_light_count > 0
                && q.shadows <= 2
                && q.shadow_resolution <= 3
                && q.shadow_projection <= 1
                && matches!(q.shadow_cascades, 0 | 2 | 4)
                && finite(&[q.shadow_distance, q.shadow_near_plane_offset])
                && q.shadow_distance > 0.
                && finite(&q.shadow_cascade_splits)
                && q.shadow_cascade_splits
                    .iter()
                    .all(|v| (0.0..=1.0).contains(v))
                && q.shadow_cascade_splits.windows(2).all(|v| v[0] < v[1]),
            &q.source,
            "quality shadows",
        )?;
        for role in ["Cam", "farCamera"] {
            check(
                self.cameras.iter().any(|camera| camera.role == role),
                &self.source,
                &format!("cameras.{role} missing"),
            )?;
        }
        check(self.cameras.len() == 4, &self.source, "camera count")?;
        ids(
            self.cameras.iter().map(|v| &v.role),
            "duplicate lighting camera role",
        )?;
        for camera in &self.cameras {
            check(
                valid_source(&camera.source)
                    && finite(&[camera.near_plane, camera.far_plane, camera.field_of_view])
                    && camera.near_plane > 0.
                    && camera.far_plane > camera.near_plane
                    && camera.field_of_view > 0.
                    && camera.field_of_view < 180.
                    && camera.culling_mask != 0,
                &camera.source,
                "camera planes",
            )?;
        }
        for flare in &self.cycle.sun_flares {
            check(
                finite(&flare.color)
                    && flare.fade_speed.is_finite()
                    && flare.fade_speed >= 0.
                    && transform(&flare.pose)
                    && flare.directional
                    && flare.texture_layout == 1
                    && !flare.elements.is_empty()
                    && flare.elements.iter().all(|e| {
                        e.image <= 10
                            && e.position.is_finite()
                            && e.size.is_finite()
                            && e.size >= 0.
                            && finite(&e.color)
                    }),
                &flare.source,
                "flare parameters",
            )?;
        }
        let road = &self.road_lamp;
        check(
            road.lights.len() == 1,
            &road.source,
            "road lights[0] required",
        )?;
        check(
            manifest
                .assets
                .iter()
                .any(|a| a.id == road.asset && a.usage == AssetUsage::RoadLampPrototype)
                && valid_source(&road.source)
                && valid_source(&road.road_source)
                && valid_source(&road.generator_source)
                && transform(&road.road_pose)
                && road.spacing_meters.is_finite()
                && road.spacing_meters > 0.
                && finite(&road.rotation_chances)
                && road.rotation_chances.iter().all(|v| *v >= 0.)
                && road.rotation_angles.iter().flatten().all(|a| *a >= 0)
                && node(road.rotation_node).is_some_and(|n| n.asset == road.asset)
                && control_ids.contains(&road.controller)
                && self.controllers.iter().any(|c| {
                    c.id == road.controller
                        && c.kind == LightControllerKind::Road
                        && node(c.node).is_some_and(|n| n.asset == road.asset)
                })
                && road.lights.iter().all(|id| {
                    self.lights.iter().any(|l| {
                        l.id == *id
                            && l.node
                                .is_some_and(|id| node(id).is_some_and(|n| n.asset == road.asset))
                    })
                })
                && road.electronics_threshold.is_finite(),
            &road.source,
            "road prototype links",
        )?;
        Ok(())
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StarReference {
    pub source: SourceId,
    pub pose: Transform,
    pub max_particles: u32,
    pub radius: f32,
    pub shape_kind: u8,
    pub shape_scale: [f32; 3],
    pub radius_thickness: f32,
    pub size_min: f32,
    pub size_max: f32,
    pub speed: f32,
    pub infinite_lifetime: bool,
    pub lifetime_seconds: Option<f32>,
    pub start_color: [f32; 4],
    pub emission_rate: f32,
    pub burst_counts: Vec<f32>,
    pub auto_random_seed: bool,
    pub random_seed: u32,
    pub looping: bool,
    pub duration_seconds: f32,
    pub simulation_space: u8,
    pub render_mode: u8,
    pub alignment: u8,
    pub max_screen_size: f32,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LightingQualityReference {
    pub source: SourceId,
    pub pixel_light_count: u32,
    pub shadows: u8,
    pub shadow_resolution: u8,
    pub shadow_projection: u8,
    pub shadow_cascades: u8,
    pub shadow_distance: f32,
    pub shadow_near_plane_offset: f32,
    pub shadow_cascade_splits: [f32; 3],
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LightingCameraReference {
    pub source: SourceId,
    pub role: String,
    pub near_plane: f32,
    pub far_plane: f32,
    pub field_of_view: f32,
    pub hdr: bool,
    pub culling_mask: u32,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SkyGeometryReference {
    pub asset: AssetId,
    pub root: NodeId,
    pub pose: Transform,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SunFlareReference {
    pub color: [f32; 4],
    pub fade_speed: f32,
    pub ignore_layers: u32,
    pub directional: bool,
    pub pose: Transform,
    pub use_fog: bool,
    pub texture_layout: u8,
    pub elements: Vec<FlareElementReference>,
    pub source: SourceId,
    pub component: ResourceReference,
    pub flare: ResourceReference,
    pub atlas: u32,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FlareElementReference {
    pub image: u8,
    pub position: f32,
    pub size: f32,
    pub color: [f32; 4],
    pub use_light_color: bool,
    pub rotate: bool,
    pub zoom: bool,
    pub fade: bool,
}
