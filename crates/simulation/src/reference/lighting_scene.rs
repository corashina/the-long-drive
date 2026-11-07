//! Complete fixed-startup lighting contract and runtime resource links.
use super::postprocessing::{CameraStage, PostprocessingRecord};
#[cfg(feature = "validate")]
use super::render_lighting::{LightKind, valid_pose};
use super::render_lighting::{LightRecord, ProbeRecord};
#[cfg(feature = "validate")]
use super::rendering::safe_path;
use super::rendering::{SamplerRecord, TextureColorSpace};
use super::{FileReceipt, SourceId, Transform};
#[cfg(feature = "validate")]
use super::{NodeRecord, ReferenceError, finite, hash, require};
use serde::{Deserialize, Serialize};
#[cfg(feature = "validate")]
use std::collections::HashSet;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LightingRecord {
    pub project_color_space: String,
    pub output_transform: String,
    pub light_conversion: String,
    pub msaa_samples: u32,
    pub ambient_sh: [[f32; 4]; 7],
    pub global_reflection_intensity: f32,
    pub reflection_bounces: u32,
    pub sample: StartupSample,
    pub lights: Vec<LightRecord>,
    pub probes: Vec<ProbeRecord>,
    pub sky: SkyRecord,
    pub fog: FogRecord,
    pub shadows: ShadowProfile,
    pub cameras: Vec<CameraStage>,
    pub postprocessing: PostprocessingRecord,
    pub textures: Vec<LightingTexture>,
    pub caster_dither: CasterDither,
    pub dependencies: Vec<FileReceipt>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StartupSample {
    pub elapsed_level_seconds: f32,
    pub fixed_updates: u32,
    pub fixed_delta_seconds: f32,
    pub initial_awake_updates: u32,
    pub cloud_phase: f32,
    pub cloud_perlin: f32,
    pub cloud_density: f32,
    pub shader_time: [f32; 4],
    pub sun_euler_degrees: [f32; 3],
    pub sun_intensity: f32,
    pub ambient_gamma: f32,
    pub temperature: f32,
    pub heat_haze_weight: f32,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LightingTexture {
    pub source: SourceId,
    pub color_space: TextureColorSpace,
    pub sampler: SamplerRecord,
    pub width: u32,
    pub height: u32,
    /// Decoded RGBA8 levels in ascending mip order.
    pub file: String,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CasterDither {
    pub file: String,
    pub extent: [u32; 3],
    pub source_engine_sha256: String,
    pub source_format: String,
    pub target_format: String,
    pub filter: String,
    pub wrap: String,
    pub mip_count: u32,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FogRecord {
    pub mode: String,
    pub color_gamma: [f32; 4],
    pub color_linear: [f32; 4],
    pub density: f32,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ShadowProfile {
    pub cascades: u32,
    pub projection: String,
    pub resolution: u32,
    pub distance: f32,
    pub near_offset: f32,
    pub split_fractions: [f32; 3],
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SkyRecord {
    pub source: SourceId,
    pub shader: SourceId,
    pub sun_size: f32,
    pub sun_convergence: f32,
    pub sun_disk: u32,
    pub capture_sun_disk: u32,
    pub exposure: f32,
    pub atmosphere_thickness: f32,
    pub tint: [f32; 4],
    pub ground: [f32; 4],
    pub cloud_root: Transform,
    pub cloud_mesh: CloudMesh,
    pub clouds: Vec<CloudRecord>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CloudMesh {
    pub source: SourceId,
    pub file: String,
    pub buffer: String,
    pub vertex_count: u32,
    pub index_count: u32,
    pub bounds_center: [f32; 3],
    pub bounds_extents: [f32; 3],
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CloudRecord {
    pub source: SourceId,
    pub material: SourceId,
    pub transform: Transform,
    pub color: [f32; 4],
    pub speed: f32,
    pub density: f32,
    pub textures: Vec<CloudTexture>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CloudTexture {
    pub slot: String,
    pub texture: SourceId,
    pub scale: [f32; 2],
    pub offset: [f32; 2],
}
impl LightingRecord {
    #[cfg(feature = "validate")]
    pub fn validate(&self, nodes: &[NodeRecord]) -> Result<(), ReferenceError> {
        require(
            self.project_color_space == "linear"
                && self.output_transform == "neutral_lut_srgb_dither"
                && self.light_conversion == "legacy_gamma_before_linear"
                && self.msaa_samples == 8
                && self.reflection_bounces == 1
                && self.global_reflection_intensity == 0.,
            "lighting source profile",
        )?;
        require(
            self.ambient_sh.iter().all(|row| finite(row)),
            "lighting ambient SH",
        )?;
        let s = &self.sample;
        require(
            s.elapsed_level_seconds == 0.5
                && s.fixed_updates == 25
                && s.initial_awake_updates == 1
                && s.fixed_delta_seconds == 0.02
                && s.shader_time == [0.025, 0.5, 1., 1.5]
                && finite(&[
                    s.cloud_phase,
                    s.cloud_perlin,
                    s.cloud_density,
                    s.sun_intensity,
                    s.ambient_gamma,
                    s.temperature,
                ])
                && finite(&s.sun_euler_degrees)
                && s.sun_intensity >= 0.
                && s.ambient_gamma >= 0.
                && s.heat_haze_weight == 0.,
            "lighting startup schedule",
        )?;
        let mut dependencies = HashSet::new();
        for dependency in &self.dependencies {
            require(
                safe_path(&dependency.file)
                    && hash(&dependency.sha256)
                    && dependencies.insert(dependency.file.as_str()),
                "lighting resource hash",
            )?;
        }
        let mut loaded = HashSet::new();
        let mut textures = HashSet::new();
        for texture in &self.textures {
            require(
                texture.source.is_set() && textures.insert(texture.source.clone()),
                "lighting texture identity",
            )?;
            let sampler = &texture.sampler;
            require(
                sampler.filter <= 2
                    && sampler.wrap_u <= 3
                    && sampler.wrap_v <= 3
                    && sampler.mip_bias.is_finite()
                    && sampler.aniso <= 16
                    && sampler.mip_count > 0
                    && super::fixed_reservation::rgba_payload(
                        texture.width,
                        texture.height,
                        sampler.mip_count,
                    )
                    .is_ok(),
                "lighting texture sampler/mips",
            )?;
            require(
                safe_path(&texture.file)
                    && texture.file.ends_with(".rgba")
                    && dependencies.contains(texture.file.as_str())
                    && loaded.insert(texture.file.as_str()),
                "lighting texture link",
            )?;
        }
        require(
            !self.lights.is_empty() && !self.probes.is_empty(),
            "lighting light/probe inventory",
        )?;
        let mut identities = HashSet::new();
        for light in &self.lights {
            require(
                identities.insert(light.source.clone()),
                "duplicate lighting light",
            )?;
            light.validate(nodes, &textures)?;
        }
        require(
            self.lights[0].kind == LightKind::Directional
                && self.lights[0].enabled
                && self.lights[0].cookie.is_some()
                && self.lights[1..].iter().all(|light| !light.enabled),
            "lighting startup light selection",
        )?;
        for probe in &self.probes {
            require(
                identities.insert(probe.source.clone()),
                "duplicate lighting probe",
            )?;
            probe.validate()?;
        }
        self.postprocessing.validate(&textures)?;
        require(
            CameraStage::validate_stack(&self.cameras),
            "lighting camera stack",
        )?;
        let fog = &self.fog;
        require(
            fog.mode == "exp_squared"
                && finite(&fog.color_gamma)
                && finite(&fog.color_linear)
                && fog.density.is_finite()
                && fog.density >= 0.,
            "lighting source fog",
        )?;
        let shadow = &self.shadows;
        require(
            shadow.cascades == 4
                && shadow.projection == "stable_fit"
                && shadow.resolution == 2
                && shadow.distance.is_finite()
                && shadow.distance > 0.
                && shadow.near_offset.is_finite()
                && shadow.near_offset >= 0.
                && shadow.split_fractions.iter().all(|v| *v > 0. && *v < 1.)
                && shadow
                    .split_fractions
                    .windows(2)
                    .all(|pair| pair[0] < pair[1]),
            "lighting shadow profile",
        )?;
        let sky = &self.sky;
        require(
            sky.source.is_set()
                && sky.shader.is_set()
                && finite(&sky.tint)
                && finite(&sky.ground)
                && finite(&[
                    sky.sun_size,
                    sky.sun_convergence,
                    sky.exposure,
                    sky.atmosphere_thickness,
                ])
                && sky.sun_size > 0.
                && sky.sun_convergence > 0.
                && sky.exposure >= 0.
                && sky.atmosphere_thickness >= 0.
                && sky.sun_disk == 2
                && sky.capture_sun_disk == 0
                && valid_pose(&sky.cloud_root)
                && sky.clouds.len() == 2,
            "lighting procedural sky",
        )?;
        let mesh = &sky.cloud_mesh;
        require(
            mesh.source.is_set()
                && mesh.vertex_count == 829
                && mesh.index_count == 4596
                && finite(&mesh.bounds_center)
                && finite(&mesh.bounds_extents)
                && mesh.bounds_extents.iter().all(|value| *value >= 0.),
            "lighting source cloud geometry",
        )?;
        for path in [&mesh.file, &mesh.buffer, &self.caster_dither.file] {
            require(
                safe_path(path)
                    && dependencies.contains(path.as_str())
                    && loaded.insert(path.as_str()),
                "lighting geometry/dither link",
            )?;
        }
        let mut clouds = HashSet::new();
        for cloud in &sky.clouds {
            require(
                cloud.source.is_set()
                    && clouds.insert(cloud.source.clone())
                    && cloud.material.is_set()
                    && valid_pose(&cloud.transform)
                    && finite(&cloud.color)
                    && cloud.speed.is_finite()
                    && cloud.density.is_finite()
                    && cloud.textures.len() == 4,
                "lighting cloud material",
            )?;
            let slots = cloud
                .textures
                .iter()
                .map(|binding| binding.slot.as_str())
                .collect::<HashSet<_>>();
            require(
                slots
                    == HashSet::from([
                        "_ScatterMap0",
                        "_ScatterMap1",
                        "_DensityMap",
                        "_TextureMap",
                    ])
                    && cloud.textures.iter().all(|binding| {
                        textures.contains(&binding.texture)
                            && finite(&binding.scale)
                            && finite(&binding.offset)
                    }),
                "lighting cloud texture binding",
            )?;
        }
        let dither = &self.caster_dither;
        require(
            dither.extent == [4, 4, 16]
                && hash(&dither.source_engine_sha256)
                && dither.source_format == "a8"
                && dither.target_format == "r8_unorm"
                && dither.filter == "point"
                && dither.wrap == "repeat"
                && dither.mip_count == 1,
            "lighting caster dither",
        )?;
        require(
            loaded == dependencies,
            "unused lighting resource dependency",
        )
    }
}
