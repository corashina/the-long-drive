//! Live master day-cycle values and rebased local probe poses.
use super::{
    lighting_assets::LightingAssets, readiness::RendererStateHandle, reference::ReferenceVisuals,
    source_lighting::LightingUniform,
};
use crate::{
    app::RuntimeReference,
    reference::reflections::{
        PROBE_DISABLE_DISTANCE_SQUARED, assign_probe_slots, probe_influence_bounds,
    },
};
use bevy::prelude::*;
use bevy::render::extract_resource::ExtractResource;
use bevy::{ecs as bevy_ecs, render as bevy_render};
use simulation::boundary::SimFrame;

pub const PROBE_COUNT: usize = crate::render_readiness::PROBE_COUNT;

/// One car probe in a slot: probe node entity, box size and box offset.
#[derive(Clone, Copy, PartialEq)]
pub struct ProbeSource {
    entity: Entity,
    size: [f32; 3],
    offset: [f32; 3],
}
#[derive(Resource)]
pub struct ProbeActivation {
    pub enabled: [bool; PROBE_COUNT],
    pub checks: u32,
    pub distance_squared: [f32; PROBE_COUNT],
    slots: [Option<ProbeSource>; PROBE_COUNT],
    position: [Vec3; PROBE_COUNT],
    /// Distance check candidates, nearest first. Kept to reuse the allocation.
    candidates: Vec<(f32, ProbeSource)>,
    keys: Vec<ProbeSource>,
    scene: u32,
    instances: u64,
    elapsed: f32,
    last_check: f32,
}
impl Default for ProbeActivation {
    fn default() -> Self {
        Self {
            enabled: [false; PROBE_COUNT],
            checks: 0,
            distance_squared: [0.; PROBE_COUNT],
            slots: [None; PROBE_COUNT],
            position: [Vec3::ZERO; PROBE_COUNT],
            candidates: Vec::new(),
            keys: Vec::new(),
            scene: 0,
            instances: 0,
            elapsed: 0.,
            last_check: 0.,
        }
    }
}
impl ProbeActivation {
    /// ReflectionDisableDistance: the nearest car probes within 50 m take the slots.
    fn check(
        &mut self,
        on: bool,
        player: Vec3,
        clock: &mut super::view_frame::SourceViewClock,
        reference: &RuntimeReference,
        instances: Option<&super::world::instances::SourceInstances>,
        transforms: &Query<&GlobalTransform>,
    ) {
        self.candidates.clear();
        if on && let Some(instances) = instances {
            for (_, _, prototype, nodes) in instances.shown() {
                let prototype = &reference.0.world.prototypes[prototype];
                let Some(probe) = prototype.vehicle.as_ref().and_then(|v| v.probe.as_ref()) else {
                    continue;
                };
                let node = prototype.nodes[probe.node as usize].gltf_node as usize;
                let Some(entity) = nodes.get(node).copied().flatten() else {
                    continue;
                };
                let Ok(transform) = transforms.get(entity) else {
                    continue;
                };
                let distance = player.distance_squared(transform.translation());
                if distance < PROBE_DISABLE_DISTANCE_SQUARED {
                    self.candidates.push((
                        distance,
                        ProbeSource {
                            entity,
                            size: probe.bounds_size,
                            offset: probe.bounds_offset,
                        },
                    ));
                }
            }
        }
        self.candidates.sort_by(|a, b| a.0.total_cmp(&b.0));
        self.keys.clear();
        self.keys
            .extend(self.candidates.iter().map(|(_, source)| *source));
        let changed = assign_probe_slots(&mut self.slots, &self.keys, clock.probe_index());
        for index in 0..PROBE_COUNT {
            if changed & (1 << index) != 0 {
                clock.reset_probe(index);
            }
        }
        self.checks += 1;
    }
}
#[derive(Resource, Default)]
pub struct SourceWorldOrigin(pub [f64; 3]);

#[derive(Clone, Copy)]
pub struct SceneLighting {
    pub ambient_sh: [Vec4; 7],
    pub sky: super::sky::SkyUniform,
    pub cloud_speed: Vec2,
    pub sun_rotation: Quat,
    pub sun_radiance: Vec4,
    pub direction_to_sun: Vec4,
    pub cookie_from_world: Mat4,
    pub fog_color_density: Vec4,
    pub probe_position: [Vec3; PROBE_COUNT],
    pub probe_center: [Vec4; PROBE_COUNT],
    pub probe_extents: [Vec4; PROBE_COUNT],
    pub source_time: Vec4,
}
impl SceneLighting {
    pub fn for_view(
        &self,
        shadow_fade: Vec4,
        shadow_fade_center: Vec4,
        clip_from_world: Mat4,
        near_far: Vec4,
        sun_shadow: super::shadow_collector::SunShadowUniform,
        lamp_shadows: [super::lamp_shadows::LampShadowUniform;
            crate::reference::lamp_shadows::LAMP_SLOTS],
    ) -> LightingUniform {
        LightingUniform {
            ambient_sh: self.ambient_sh,
            sun_radiance: self.sun_radiance,
            direction_to_sun: self.direction_to_sun,
            cookie_from_world: self.cookie_from_world,
            fog_color_density: self.fog_color_density,
            shadow_fade,
            shadow_fade_center,
            probe_center: self.probe_center,
            probe_extents: self.probe_extents,
            source_time: self.source_time,
            clip_from_world,
            near_far,
            sun_shadow,
            lamp_shadows,
        }
    }
}
#[derive(Resource, Default, Clone, ExtractResource)]
pub struct SourceSceneLighting(pub Option<SceneLighting>);

/// Schedule after master sync_lighting and transform propagation.
pub(crate) fn sync_lighting(
    assets: Option<Res<LightingAssets>>,
    visuals: Option<Res<ReferenceVisuals>>,
    master: Option<Res<super::lighting::SourceLighting>>,
    sim: Option<Res<super::sim_frame::SimFrameResource>>,
    reference: Option<Res<RuntimeReference>>,
    reflections: Res<super::reflections::Reflections>,
    mut origin: ResMut<SourceWorldOrigin>,
    mut activation: ResMut<ProbeActivation>,
    mut clock: ResMut<super::view_frame::SourceViewClock>,
    epoch: Res<super::view_frame::SourceSceneEpoch>,
    transforms: Query<&GlobalTransform>,
    mut scene: ResMut<SourceSceneLighting>,
    status: Res<RendererStateHandle>,
    instances: Option<Res<super::world::instances::SourceInstances>>,
) {
    scene.0 = None;
    if status.failure().is_some() {
        return;
    }
    let (Some(assets), Some(visuals), Some(master), Some(sim), Some(reference)) =
        (assets, visuals, master, sim, reference)
    else {
        return;
    };
    if !assets.loaded || !visuals.bound || !master.bound {
        return;
    }
    let sim = &sim.frame;
    origin.0 = sim.state.origin;
    let player = Vec3::from_array(sim.state.body_pose.position);
    let activation = &mut *activation;
    let revision = instances.as_deref().map_or(0, |i| i.revision());
    let empty = activation.slots.iter().all(Option::is_none);
    if activation.scene != epoch.0 {
        activation.scene = epoch.0;
        activation.slots = [None; PROBE_COUNT];
        activation.elapsed = 0.;
        activation.checks = 0;
    }
    activation.elapsed += sim.state.perf.frame_seconds();
    // Checks run every five seconds. Scene start, the setting, and new cars with no probe in a
    // slot check at once.
    if activation.checks == 0
        || reflections.is_changed()
        || activation.elapsed - activation.last_check >= 5.
        || (empty && revision != activation.instances)
    {
        activation.last_check = activation.elapsed;
        activation.instances = revision;
        activation.check(
            reflections.0,
            player,
            &mut clock,
            &reference,
            instances.as_deref(),
            &transforms,
        );
    }
    // The probes follow their cars. A probe without its car turns off until the next check.
    for index in 0..PROBE_COUNT {
        let position = activation.slots[index]
            .and_then(|source| transforms.get(source.entity).ok())
            .map(GlobalTransform::translation);
        if let Some(position) = position {
            activation.position[index] = position;
        }
        activation.enabled[index] = reflections.0 && position.is_some();
        activation.distance_squared[index] = player.distance_squared(activation.position[index]);
    }
    match scene_value(&assets, &master, sim, &reference, &transforms, activation) {
        Ok(mut value) => {
            for index in 0..PROBE_COUNT {
                // The shader samples a slot only after its probe publishes.
                value.probe_extents[index].w =
                    f32::from(activation.enabled[index] && clock.probe_published(index));
            }
            scene.0 = Some(value);
        }
        Err(error) => {
            status.fail(format!("Source lighting pose: {error}"));
        }
    }
}
fn scene_value(
    assets: &LightingAssets,
    master: &super::lighting::SourceLighting,
    sim: &SimFrame,
    reference: &RuntimeReference,
    transforms: &Query<&GlobalTransform>,
    activation: &ProbeActivation,
) -> Result<SceneLighting, String> {
    if !sim.state.origin.iter().all(|value| value.is_finite()) {
        return Err("Nonfinite world origin".into());
    }
    if assets.record.probes.len() != 1 {
        return Err("Shared car reflection probe record is missing".into());
    }
    let mut centers = [Vec4::ZERO; PROBE_COUNT];
    let mut sizes = [Vec4::ZERO; PROBE_COUNT];
    for (index, source) in activation.slots.iter().enumerate() {
        let Some(source) = source else { continue };
        let position = activation.position[index];
        if !position.is_finite() {
            return Err("Nonfinite probe world position".into());
        }
        let (center, extents) =
            probe_influence_bounds(position.to_array(), source.size, source.offset);
        centers[index] = Vec3::from_array(center).extend(0.);
        sizes[index] = Vec3::from_array(extents).extend(0.);
    }
    let bound = master
        .lights
        .iter()
        .find(|bound| {
            !bound.road
                && reference.0.lighting.lights[bound.source_index].kind
                    == crate::reference::LightKind::Directional
        })
        .ok_or("Missing live source sun")?;
    let rotation = transforms
        .get(bound.entity)
        .map_err(|_| "Missing live sun transform")?
        .compute_transform()
        .rotation;
    let sun = &master.uniform.lights[bound.index];
    let cookie_size = reference.0.lighting.lights[bound.source_index].cookie_size;
    let cookie_from_world =
        Mat4::from_translation(Vec3::new(
            sun.cookie_rect.y + 0.5,
            sun.cookie_rect.z + 0.5,
            0.,
        )) * Mat4::from_scale(Vec3::new(1. / cookie_size, 1. / cookie_size, 0.))
            * sun.cookie_from_world;
    let time = sim.lighting.cycle.shader_time_seconds as f32;
    if !cookie_from_world.is_finite() || !time.is_finite() || !rotation.is_finite() {
        return Err("Nonfinite live lighting transform or time".into());
    }
    // Native Flat SH writes the linear ambient RGB into SHAr/Ag/Ab.w.
    let mut ambient_sh = [Vec4::ZERO; 7];
    for (row, channel) in ambient_sh[..3]
        .iter_mut()
        .zip(master.uniform.ambient.to_array())
    {
        row.w = channel;
    }
    Ok(SceneLighting {
        sky: master.sky.uniform,
        cloud_speed: Vec2::new(
            sim.lighting.cycle.clouds[0].speed,
            sim.lighting.cycle.clouds[1].speed,
        ),
        ambient_sh,
        sun_rotation: rotation,
        sun_radiance: sun.color.truncate().extend(0.),
        direction_to_sun: (-sun.direction_kind.truncate()).extend(0.),
        cookie_from_world,
        fog_color_density: master
            .uniform
            .fog
            .truncate()
            .extend(master.uniform.ambient.w),
        probe_position: activation.position,
        probe_center: centers,
        probe_extents: sizes,
        source_time: Vec4::new(time * 0.05, time, time * 2., time * 3.),
    })
}
