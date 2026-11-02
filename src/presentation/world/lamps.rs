//! Shared source pole recipes and absolute stream ownership.
use bevy::prelude::*;

use crate::presentation::lighting::{
    RenderingState, SourceLighting, cookie_ids, cookie_rect, register_road_light,
    register_world_light, remove_road_light,
};
use crate::presentation::materials::SourceMaterial;
use bevy::camera::visibility::NoFrustumCulling;
use bevy::gltf::GltfExtras;
use bevy::world_serialization::{WorldAssetRoot, WorldInstanceReady};

#[derive(Clone)]
pub(crate) struct RoadLampRecipe;

#[derive(Clone)]
struct SourceProxy {
    id: crate::world::GeneratedId,
    node: Entity,
    light: Entity,
    slot: usize,
}
#[derive(Clone)]
pub(crate) struct RoadLamps {
    source_proxies: Vec<SourceProxy>,
    required: Vec<crate::world::GeneratedId>,
    allocation: [Option<usize>; crate::presentation::lighting::MAX_LIGHTS],
    preflight_ready: bool,
    pub capacity: usize,
    pub fixed_lights: usize,
    pub recipe: Option<RoadLampRecipe>,
    prototype: Option<Entity>,
    prototype_ready: bool,
    prototype_bound: bool,
    prototype_nodes: Vec<(crate::reference::NodeId, Entity)>,
    prototype_light: Option<Entity>,
    origin: [f64; 3],
    overlap: usize,
}
impl RoadLamps {
    pub fn clone_bytes(&self) -> usize {
        self.source_proxies.capacity() * std::mem::size_of::<SourceProxy>()
            + self.required.capacity() * std::mem::size_of::<crate::world::GeneratedId>()
            + self.prototype_nodes.capacity()
                * std::mem::size_of::<(crate::reference::NodeId, Entity)>()
    }
    pub fn ready(&self) -> bool {
        self.preflight_ready
            && self.required.len() == self.source_proxies.len()
            && self
                .required
                .iter()
                .all(|id| self.source_proxies.iter().any(|p| &p.id == id))
    }

    pub fn admitted_lights(&self) -> Option<&[crate::world::GeneratedId]> {
        self.preflight_ready.then_some(self.required.as_slice())
    }

    pub fn reset_instances(&mut self, commands: &mut Commands) {
        for proxy in self.source_proxies.drain(..) {
            commands.entity(proxy.node).despawn();
            commands.entity(proxy.light).despawn();
        }
        self.required.clear();
        self.allocation.fill(None);
        self.preflight_ready = false;
        self.origin = [f64::INFINITY; 3];
    }
    pub fn new() -> Self {
        let capacity = crate::presentation::lighting::MAX_LIGHTS;
        Self {
            source_proxies: Vec::with_capacity(crate::presentation::lighting::MAX_LIGHTS),
            required: Vec::with_capacity(crate::presentation::lighting::MAX_LIGHTS),
            allocation: [None; crate::presentation::lighting::MAX_LIGHTS],
            preflight_ready: false,
            capacity,
            fixed_lights: 0,
            recipe: None,
            prototype: None,
            prototype_ready: false,
            prototype_bound: false,
            prototype_nodes: Vec::new(),
            prototype_light: None,
            origin: [f64::INFINITY; 3],
            overlap: 0,
        }
    }
}

/// Prepare the reusable source hierarchy before the loading gate permits Start.
pub(crate) fn prepare_road_lamp(
    mut commands: Commands,
    reference: Res<crate::app::RuntimeReference>,
    visuals: Res<crate::presentation::reference::ReferenceVisuals>,
    gltfs: Res<Assets<bevy::gltf::Gltf>>,
    mut source: ResMut<SourceLighting>,
    materials: Res<crate::presentation::materials::SourceMaterials>,
    extras: Query<(Entity, &GltfExtras)>,
    primitives: Query<(Entity, &ChildOf), With<Mesh3d>>,
    receivers: Query<(
        &Mesh3d,
        &MeshMaterial3d<SourceMaterial>,
        &Transform,
        &Visibility,
        &bevy::camera::visibility::RenderLayers,
        &crate::presentation::materials::SourceShadowMode,
        Option<&bevy::light::NotShadowReceiver>,
        &crate::presentation::renderer_visibility::SourceRenderer,
        &bevy::camera::primitives::Aabb,
        &bevy::gltf::GltfMaterialExtras,
    )>,
) {
    if !source.bound
        || source.road_lamps.recipe.is_some()
        || matches!(source.state, RenderingState::Failed(_))
    {
        return;
    }
    let manifest = &reference.0;
    let road = &manifest.lighting.road_lamp;
    if source.road_lamps.prototype.is_none() {
        let asset = manifest.assets.iter().find(|a| a.id == road.asset).unwrap();
        let handle = &visuals
            .handles
            .iter()
            .find(|(path, _)| *path == asset.path)
            .unwrap()
            .1;
        let Some(gltf) = gltfs.get(handle) else {
            return;
        };
        let root = commands
            .spawn((
                WorldAssetRoot(gltf.scenes[0].clone()),
                Transform::from_translation(Vec3::splat(1_000_000.)),
                Visibility::Hidden,
            ))
            .observe(
                |_: On<WorldInstanceReady>, mut source: ResMut<SourceLighting>| {
                    source.road_lamps.prototype_ready = true;
                },
            )
            .id();
        source.road_lamps.prototype = Some(root);
        return;
    }
    if !source.road_lamps.prototype_ready {
        return;
    }
    if !source.road_lamps.prototype_bound {
        #[derive(serde::Deserialize)]
        struct Identity {
            reference_id: Option<crate::reference::SourceId>,
        }
        for (entity, extra) in &extras {
            let identity: Identity = match serde_json::from_str(&extra.value) {
                Ok(value) => value,
                Err(error) => {
                    source.state = RenderingState::Failed(format!("Road lamp extras: {error}"));
                    return;
                }
            };
            let Some(node) = identity.reference_id.and_then(|id| {
                manifest
                    .nodes
                    .iter()
                    .find(|n| n.source == id && n.asset == road.asset)
            }) else {
                continue;
            };
            if let Err(error) = crate::presentation::reference::bind_node(
                &mut commands,
                node,
                entity,
                extra,
                &primitives,
            ) {
                source.state = RenderingState::Failed(error);
                return;
            }
            if source
                .road_lamps
                .prototype_nodes
                .iter()
                .any(|(id, _)| *id == node.id)
            {
                source.state =
                    RenderingState::Failed(format!("Duplicate road prototype node {}", node.id.0));
                return;
            }
            source.road_lamps.prototype_nodes.push((node.id, entity));
        }
        if source.road_lamps.prototype_nodes.len()
            != manifest
                .nodes
                .iter()
                .filter(|n| n.asset == road.asset)
                .count()
        {
            source.state =
                RenderingState::Failed("Road lamp prototype has missing source nodes".into());
            return;
        }
        let light = manifest
            .lighting
            .lights
            .iter()
            .find(|l| l.id == road.lights[0])
            .unwrap();
        if light.kind != crate::reference::LightKind::Spot || light.cookie.is_some() {
            source.state =
                RenderingState::Failed("Road lamp requires the source default spot recipe".into());
            return;
        }
        let node = source
            .road_lamps
            .prototype_nodes
            .iter()
            .find(|(id, _)| Some(*id) == light.node)
            .unwrap()
            .1;
        let entity = register_road_light(&mut source, &mut commands, &manifest.lighting, 0, node);
        source.road_lamps.prototype_light = Some(entity);
        source.road_lamps.prototype_bound = true;
        return;
    }
    for (entity, parent) in &primitives {
        if !source
            .road_lamps
            .prototype_nodes
            .iter()
            .any(|(_, node)| *node == parent.parent())
        {
            continue;
        }
        if !receivers.contains(entity) {
            return;
        }
    }
    let far = Transform::from_translation(Vec3::splat(1_000_000.));
    for node in manifest
        .nodes
        .iter()
        .filter(|node| node.asset == road.asset)
    {
        let entity = source
            .road_lamps
            .prototype_nodes
            .iter()
            .find(|(id, _)| *id == node.id)
            .unwrap()
            .1;
        for (primitive, owner) in &primitives {
            if owner.parent() != entity {
                continue;
            }
            let (mesh, material, _, _, _, _, _, renderer, _, material_extras) =
                receivers.get(primitive).unwrap();
            let warm = commands
                .spawn((
                    mesh.clone(),
                    material.clone(),
                    crate::presentation::renderer_visibility::SourceMaterialWarmup,
                    far,
                    Visibility::Visible,
                    bevy::light::NotShadowCaster,
                    NoFrustumCulling,
                ))
                .id();
            source.warmups.push(warm);
            #[derive(serde::Deserialize)]
            struct MaterialIdentity {
                asset: crate::reference::AssetId,
                gltf_material: u32,
            }
            #[derive(serde::Deserialize)]
            struct PrimitiveIdentity {
                submesh: u32,
            }
            let identity: MaterialIdentity =
                serde_json::from_str(&material_extras.value).expect("validated road lamp material");
            let primitive: PrimitiveIdentity = serde_json::from_str(
                &extras
                    .get(primitive)
                    .expect("checked road lamp primitive")
                    .1
                    .value,
            )
            .expect("validated road lamp primitive");
            match materials.sort_data(
                (identity.asset, identity.gltf_material),
                &renderer.0,
                primitive.submesh,
            ) {
                Ok(Some(sort)) => {
                    commands
                        .entity(warm)
                        .insert((sort, bevy::render::batching::NoAutomaticBatching));
                }
                Ok(None) => {}
                Err(error) => {
                    source.state = RenderingState::Failed(error.0);
                    return;
                }
            }
        }
    }
    source.road_lamps.recipe = Some(RoadLampRecipe);
}

pub(crate) fn finish_road_lamp_warmup(commands: &mut Commands, source: &mut SourceLighting) {
    if let Some(light) = source.road_lamps.prototype_light.take() {
        remove_road_light(source, commands, light);
    }
    if let Some(root) = source.road_lamps.prototype.take() {
        commands.entity(root).despawn();
    }
    source.road_lamps.prototype_nodes.clear();
}

/// The source frame owns pole geometry. Receiver influence owns independent light proxies.
pub(crate) fn sync_source_lamps(
    frame: &crate::world::RendererFrame<'_>,
    reference: &crate::reference::ReferenceManifest,
    enabled: bool,
    source: &mut SourceLighting,
    commands: &mut Commands,
    roots: &mut super::runtime::WorldRoots,
) -> Result<(), crate::reference::ReferenceError> {
    if source.road_lamps.recipe.is_none() || source.warming {
        return Ok(());
    }
    // Accepted proxies keep their pose and controller output current while membership waits.
    for proxy in &source.road_lamps.source_proxies {
        let light = frame.lights.iter().find(|light| light.id == proxy.id);
        if let Some(bound) = source
            .lights
            .iter_mut()
            .find(|bound| bound.entity == proxy.light)
            .and_then(|bound| bound.world.as_mut())
        {
            if let Some(light) = light {
                bound.output.enabled = light.enabled && (light.biome.is_none() || enabled);
                bound.output.intensity = light.intensity;
                // ginlampscript drives the generated light range.
                bound.record.range_meters = light.range;
            } else {
                // Source retirement can precede the deferred membership replacement.
                bound.output.enabled = false;
            }
        }
        if let Ok(mut transform) = roots.get_mut(proxy.node) {
            if let Some(light) = light {
                transform.set_if_neq(
                    Transform::from_translation(Vec3::from_array(
                        frame.origin.local(light.pose.position),
                    ))
                    .with_rotation(Quat::from_array(light.pose.rotation)),
                );
            } else if source
                .road_lamps
                .origin
                .iter()
                .all(|value| value.is_finite())
            {
                let shift = std::array::from_fn(|i| {
                    (source.road_lamps.origin[i] - frame.origin.offset[i]) as f32
                });
                transform.translation += Vec3::from_array(shift);
            }
        }
    }
    source.road_lamps.origin = frame.origin.offset;
    if !source.road_lamps.preflight_ready
        || source.road_lamps.required.iter().any(|id| {
            frame
                .lights
                .iter()
                .find(|light| &light.id == id)
                .is_none_or(|light| {
                    !frame
                        .structures
                        .iter()
                        .any(|s| light.owned_by(&s.placement.id))
                })
        })
    {
        // Preserve the accepted proxy set until every required owner is published.
        return Ok(());
    }
    for index in (0..source.road_lamps.source_proxies.len()).rev() {
        if source
            .road_lamps
            .required
            .contains(&source.road_lamps.source_proxies[index].id)
        {
            continue;
        }
        let proxy = source.road_lamps.source_proxies.swap_remove(index);
        remove_road_light(source, commands, proxy.light);
        commands.entity(proxy.node).despawn();
    }
    for light in frame.lights {
        if !source.road_lamps.required.contains(&light.id) {
            continue;
        }
        let pose =
            Transform::from_translation(Vec3::from_array(frame.origin.local(light.pose.position)))
                .with_rotation(Quat::from_array(light.pose.rotation));
        if !source
            .road_lamps
            .source_proxies
            .iter()
            .any(|proxy| proxy.id == light.id)
        {
            let index = source
                .road_lamps
                .required
                .iter()
                .position(|id| id == &light.id)
                .unwrap();
            let slot = source.road_lamps.allocation[index].ok_or_else(|| {
                crate::reference::ReferenceError("Source lamp has no proxy admission".into())
            })?;
            let cookie =
                cookie_rect(reference, &cookie_ids(reference), &light.record).ok_or_else(|| {
                    crate::reference::ReferenceError(
                        "Source lamp cookie has no bound texture".into(),
                    )
                })?;
            let node = commands.spawn((pose, Visibility::Inherited)).id();
            let entity = register_world_light(
                source,
                commands,
                light,
                slot,
                node,
                light.enabled && (light.biome.is_none() || enabled),
                cookie,
            );
            source.road_lamps.source_proxies.push(SourceProxy {
                id: light.id.clone(),
                node,
                light: entity,
                slot,
            });
        }
    }
    Ok(())
}

pub(crate) fn source_lamp_admission(
    frame: &crate::world::RendererFrame<'_>,
    selected: &[crate::world::GeneratedId],
    source: &mut SourceLighting,
    reference: &crate::reference::ReferenceManifest,
    light_outputs: &[crate::lighting::LightOutput],
    biome_enabled: bool,
) -> Result<(usize, usize, usize), crate::reference::ReferenceError> {
    source.road_lamps.preflight_ready = false;
    source.road_lamps.allocation.fill(None);
    if source.warming {
        return Ok((0, 0, 0));
    }
    let Some(_recipe) = &source.road_lamps.recipe else {
        return Ok((0, 0, 0));
    };
    let mut count = 0;
    for light in frame.lights.iter().filter(|light| {
        selected.contains(&light.id)
            && light.enabled
            && light.intensity > 0.
            && light.record.culling_mask != 0
            && (light.biome.is_none() || biome_enabled)
    }) {
        if count == source.road_lamps.required.len() {
            source.road_lamps.required.push(light.id.clone());
        } else if source.road_lamps.required[count] != light.id {
            source.road_lamps.required[count] = light.id.clone();
        }
        count += 1;
    }
    source.road_lamps.required.truncate(count);
    let required = source.road_lamps.required.len();
    let light_capacity = crate::presentation::lighting::MAX_LIGHTS - source.road_lamps.fixed_lights;
    if required + source.road_lamps.overlap > light_capacity {
        #[cfg(target_arch = "wasm32")]
        {
            let entries: Vec<_> = source.road_lamps.required.iter().filter_map(|id| {
                frame.lights.iter().find(|light| &light.id == id).map(|light| {
                    serde_json::json!({
                        "id": id,
                        "sourceId": light.record.source,
                        "prototypeId": light.prototype,
                        "controllerOutput": {"enabled": light.enabled && (light.biome.is_none() || biome_enabled), "intensity": light.intensity},
                    })
                })
            }).collect();
            let fixed_entries: Vec<_> = source.lights.iter().take(source.road_lamps.fixed_lights).map(|bound| {
                let record = &reference.lighting.lights[bound.source_index];
                let output = &light_outputs[bound.source_index];
                serde_json::json!({
                    "id": bound.id,
                    "sourceId": record.source,
                    "controllerOutput": {"enabled": output.enabled, "intensity": output.intensity},
                })
            }).collect();
            crate::browser::game_startup_failure_details(&serde_json::json!({
                "kind": "sourceLightAdmission",
                "requiredLightEntries": required,
                "availableLightEntries": light_capacity,
                "availableNewLightEntries": light_capacity.saturating_sub(source.road_lamps.overlap),
                "fixedLightEntries": source.road_lamps.fixed_lights,
                "fixedEntries": fixed_entries,
                "retainedOverlap": {"lightEntries": source.road_lamps.overlap},
                "entries": entries,
            }).to_string());
        }
        return Err(crate::reference::ReferenceError(format!(
            "Required source lights exceed renderer capacity: {required}"
        )));
    }
    // Simulate exact retained slots before any region publishes.
    let mut slots = [false; crate::presentation::lighting::MAX_LIGHTS];
    for (index, id) in source.road_lamps.required.iter().enumerate() {
        if let Some(proxy) = source
            .road_lamps
            .source_proxies
            .iter()
            .find(|p| &p.id == id)
        {
            slots[proxy.slot] = true;
            source.road_lamps.allocation[index] = Some(proxy.slot);
        }
    }
    for index in 0..source.road_lamps.required.len() {
        if source.road_lamps.allocation[index].is_some() {
            continue;
        }
        let slot = (0..light_capacity)
            .find(|slot| !slots[*slot])
            .ok_or_else(|| {
                crate::reference::ReferenceError("Source light slot admission failed".into())
            })?;
        slots[slot] = true;
        source.road_lamps.allocation[index] = Some(slot);
    }
    source.road_lamps.preflight_ready = true;
    let proxies = source.road_lamps.source_proxies.len()
        + source
            .road_lamps
            .required
            .iter()
            .filter(|id| {
                !source
                    .road_lamps
                    .source_proxies
                    .iter()
                    .any(|p| p.id == **id)
            })
            .count();
    let objects = proxies * 2;
    // ECS and GPU transform reservations include old/new overlap until the next frame.
    Ok((
        objects * 2048 + proxies * 1024 + std::mem::size_of_val(&source.road_lamps.allocation),
        objects * 256,
        objects,
    ))
}
