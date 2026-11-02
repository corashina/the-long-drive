//! Road and terrain material admission through the shared source cache.
use super::{SourceWorldVisuals, SurfaceMaterial, prototypes::SourcePrototypes};
use crate::ownership::PresentationLedger;
use crate::{
    presentation::materials::{
        SourceMaterial, SourceMaterials, authored_material, terrain::TerrainMaterial,
    },
    reference::{ReferenceError, ReferenceManifest, WorldValue},
    world::RendererWorld,
};
use bevy::prelude::*;
impl SourceWorldVisuals {
    pub fn request_descriptors(
        &self,
        stream: &RendererWorld,
        ledger: &mut PresentationLedger,
        reference: &ReferenceManifest,
        prototypes: &mut SourcePrototypes,
    ) -> Result<(), ReferenceError> {
        // Request every selected road descriptor before any material admission.
        for source in &reference.world.road.prototypes {
            if !stream.road_prototype_required(source) {
                continue;
            }
            prototypes.request(stream, ledger, source)?;
        }
        Ok(())
    }
    pub fn prepare_bindings(
        &mut self,
        stream: &RendererWorld,
        ledger: &mut PresentationLedger,
        facts: &mut crate::world::RendererFacts,
        reference: &ReferenceManifest,
        prototypes: &mut SourcePrototypes,
        server: &AssetServer,
        cache: &mut SourceMaterials,
        materials: &mut Assets<SourceMaterial>,
        terrain: &mut Assets<TerrainMaterial>,
    ) -> Result<(), ReferenceError> {
        for source in &reference.world.road.prototypes {
            if !stream.road_prototype_required(source) {
                continue;
            }
            if self.road_materials.contains_key(source) {
                continue;
            }
            if prototypes
                .plan(stream.frame().request.session, source)
                .is_none()
            {
                continue;
            }
            let prototype = reference
                .world
                .prototypes
                .iter()
                .find(|p| p.source == *source)
                .ok_or_else(|| ReferenceError("Missing road prototype".into()))?;
            let plan = prototypes
                .plan(stream.frame().request.session, source)
                .unwrap();
            let mut ready = true;
            for skin in &prototype.skins {
                let node = &plan.nodes[prototype.nodes[skin.node as usize].gltf_node as usize];
                for primitive in plan.primitives.iter().filter(|p| Some(p.mesh) == node.mesh) {
                    let material = primitive
                        .material
                        .as_ref()
                        .ok_or_else(|| ReferenceError("Missing road material identity".into()))?;
                    let identity = (material.asset, material.gltf_material);
                    cache.admit_world_primary(identity, reference, ledger)?;
                    ready &= cache.world_primary_ready(identity);
                }
            }
            if !ready || !prototypes.admit_payload(stream, ledger, reference, server, source)? {
                continue;
            }
            let Some(loaded) = prototypes.loaded(stream.frame().request.session, source) else {
                continue;
            };
            // Road bindings persist for this bank. Reserve before allocating any binding.
            let count = prototype
                .skins
                .iter()
                .map(|skin| skin.triangles.len())
                .sum::<usize>();
            let mut allocation = ledger.allocation();
            allocation.prototype_cpu +=
                count * (8192 + crate::presentation::caster_material::CPU_BYTES);
            allocation.prototype_gpu +=
                count * (2048 + crate::presentation::caster_material::gpu_bytes());
            ledger.set_allocation(allocation)?;
            let mut bindings = Vec::with_capacity(
                prototype
                    .skins
                    .iter()
                    .map(|skin| skin.triangles.len())
                    .sum(),
            );
            for skin in &prototype.skins {
                let node =
                    &loaded.plan.nodes[prototype.nodes[skin.node as usize].gltf_node as usize];
                let extras = node
                    .extras
                    .as_ref()
                    .ok_or_else(|| ReferenceError("Missing road renderer metadata".into()))?;
                let layer = extras
                    .layer
                    .ok_or_else(|| ReferenceError("Missing road renderer layer".into()))?;
                let shadow_mode = extras
                    .cast_shadows
                    .ok_or_else(|| ReferenceError("Missing road shadow mode".into()))?;
                let receive_shadows = extras
                    .receive_shadows
                    .ok_or_else(|| ReferenceError("Missing road receiver state".into()))?;
                let before = bindings.len();
                for primitive in loaded
                    .plan
                    .primitives
                    .iter()
                    .filter(|p| Some(p.mesh) == node.mesh)
                {
                    let identity = primitive
                        .material
                        .as_ref()
                        .filter(|_| !primitive.has_source_material)
                        .ok_or_else(|| {
                            ReferenceError("Unresolved source road material family".into())
                        })?;
                    let key = (
                        identity.asset,
                        identity.gltf_material,
                        false,
                        if receive_shadows { 1u32 << layer } else { 0 },
                    );
                    let material = if let Some(handle) = cache.handles.get(&key) {
                        handle.clone()
                    } else {
                        let handle =
                            materials.add(authored_material((key.0, key.1), key.2, key.3, cache));
                        cache.handles.insert(key, handle.clone());
                        handle
                    };
                    bindings.push(SurfaceMaterial {
                        renderer: extras
                            .source_renderer
                            .clone()
                            .ok_or_else(|| ReferenceError("Missing road renderer record".into()))?,
                        material,
                        layer,
                        shadow_mode,
                        receive_shadows,
                    });
                }
                if bindings.len() - before != skin.triangles.len() {
                    return Err(ReferenceError("Road material/submesh order differs".into()));
                }
            }
            self.bind_road(source.clone(), bindings)?;
            facts.push(crate::world::RendererFact::BindPrototype(source.clone()));
        }
        for (tier, record) in reference.world.terrain.materials.iter().enumerate() {
            if self.terrain[tier].is_some() {
                continue;
            }
            // The terrain material lives as long as its stream hold.
            let hold = crate::world::RendererFact::HoldPrototype(record.source.clone());
            if !stream.holds_prototype(&record.source) && !facts.contains(&hold) {
                // A full stream table waits like a full credit.
                if !stream.can_hold_prototype(&record.source) {
                    ledger.wait_for_hold();
                    continue;
                }
                facts.push(hold);
                let mut allocation = ledger.allocation();
                allocation.prototype_cpu += std::mem::size_of::<TerrainMaterial>() + 256 + 8192;
                allocation.prototype_gpu += 2048;
                ledger.set_allocation(allocation)?;
            }
            cache.admit_world_material(record, reference, ledger)?;
            if !cache.world_textures.ready(&record.source) {
                continue;
            }
            let (layer, renderer) = terrain_renderer(reference, tier)?;
            let shadow_mode = renderer.cast_shadows;
            let receive = renderer.receive_shadows;
            self.terrain_records[tier] = Some(renderer);
            let material = terrain.add(TerrainMaterial::from_source(
                record,
                &cache.world_textures,
                1u32 << layer,
                receive,
            )?);
            terrain.get_mut(&material).unwrap().preparation = cache.preparation.clone();
            self.bind_terrain(tier, material, layer, shadow_mode);
            facts.push(crate::world::RendererFact::BindPrototype(
                record.source.clone(),
            ));
        }
        Ok(())
    }
}
fn terrain_renderer(
    reference: &ReferenceManifest,
    tier: usize,
) -> Result<(u8, crate::reference::render_lighting::RendererRecord), ReferenceError> {
    let records = &reference.world.records;
    let transform = records
        .iter()
        .find(|r| r.source == reference.world.terrain.prefabs[tier])
        .ok_or_else(|| ReferenceError("Missing terrain transform".into()))?;
    let owner = match transform.fields.get("m_GameObject") {
        Some(WorldValue::Link(id)) => id,
        _ => return Err(ReferenceError("Missing terrain owner".into())),
    };
    let object = records
        .iter()
        .find(|r| r.source == *owner)
        .ok_or_else(|| ReferenceError("Missing terrain GameObject".into()))?;
    let layer = match object.fields.get("m_Layer") {
        Some(WorldValue::Integer(layer)) => *layer as u8,
        _ => return Err(ReferenceError("Missing terrain layer".into())),
    };
    let renderer = records
        .iter()
        .find(|r| {
            r.class_name == "MeshRenderer"
                && matches!(r.fields.get("m_GameObject"),Some(WorldValue::Link(id))if id==owner)
        })
        .ok_or_else(|| ReferenceError("Missing terrain renderer".into()))?;
    let shadows = match renderer.fields.get("m_CastShadows") {
        Some(WorldValue::Integer(value)) => *value as u8,
        _ => return Err(ReferenceError("Missing terrain shadow state".into())),
    };
    let receive = match renderer.fields.get("m_ReceiveShadows") {
        Some(WorldValue::Bool(value)) => *value,
        Some(WorldValue::Integer(value)) => *value != 0,
        _ => return Err(ReferenceError("Missing terrain receiver state".into())),
    };
    let integer = |name: &str| match renderer.fields.get(name) {
        Some(WorldValue::Integer(value)) => Ok(*value),
        _ => Err(ReferenceError(format!("Missing terrain renderer {name}"))),
    };
    if integer("m_SortingLayerID")? != 0 {
        return Err(ReferenceError(
            "Unsupported terrain sorting layer ID".into(),
        ));
    }
    let record = crate::reference::render_lighting::RendererRecord {
        source: renderer.source.clone(),
        bounds_center: [0.; 3],
        bounds_extents: [0.; 3],
        cast_shadows: shadows,
        receive_shadows: receive,
        reflection_probe_usage: u8::try_from(integer("m_ReflectionProbeUsage")?)
            .map_err(|_| ReferenceError("Terrain probe usage".into()))?,
        motion_vectors: u8::try_from(integer("m_MotionVectors")?)
            .map_err(|_| ReferenceError("Terrain motion mode".into()))?,
        sorting_layer: i16::try_from(integer("m_SortingLayer")?)
            .map_err(|_| ReferenceError("Terrain sorting layer".into()))?,
        sorting_order: i16::try_from(integer("m_SortingOrder")?)
            .map_err(|_| ReferenceError("Terrain sorting order".into()))?,
    };
    record.validate()?;
    Ok((layer, record))
}
