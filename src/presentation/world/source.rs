//! Source surfaces retain the same vertices and triangle ranges as collision publication.
use super::super::materials::{SourceMaterial, SourceShadowMode, terrain::TerrainMaterial};
use super::pipelines::{PipelineTicket, WorldPipelines};
use crate::ownership::renderer_pending::{PipelineContinuation, RendererOwner};
use crate::presentation::fixed_assets::{MeshUpload, SourceFixedAssets};
use crate::presentation::materials::RenderingState;
use crate::world::{RendererFact, RendererFacts, RendererWorld, WorldRequest};
use crate::{
    reference::{ReferenceError, SourceId},
    world::{RendererFrame, SurfaceKey, WorldLayer, WorldSurface},
};
use bevy::ecs as bevy_ecs;
use bevy::{
    asset::RenderAssetUsages,
    camera::visibility::RenderLayers,
    mesh::{Indices, PrimitiveTopology},
    prelude::*,
};
use std::{collections::HashMap, sync::Arc};

pub struct SurfaceMaterial {
    pub renderer: crate::reference::render_lighting::RendererRecord,
    pub material: Handle<SourceMaterial>,
    pub layer: u8,
    pub shadow_mode: u8,
    pub receive_shadows: bool,
}
struct SurfacePreparation {
    request: WorldRequest,
    continuation: PipelineContinuation,
    _upload: Arc<MeshUpload>,
    key: SurfaceKey,
    meshes: Vec<Handle<Mesh>>,
    tickets: Vec<Option<PipelineTicket>>,
    records: Vec<crate::reference::render_lighting::RendererRecord>,
}
struct SurfaceVisual {
    key: SurfaceKey,
    root: Entity,
    meshes: Vec<Handle<Mesh>>,
    _tickets: Vec<Option<PipelineTicket>>,
}
/// Material admission precedes collision publication. A missing binding is an explicit failure.
#[derive(Resource, Default)]
pub struct SourceWorldVisuals {
    session: Option<u64>,
    pending: Option<SurfacePreparation>,
    surfaces: Vec<SurfaceVisual>,
    pub(super) road_materials: HashMap<SourceId, Vec<SurfaceMaterial>>,
    pub(super) terrain: [Option<Handle<TerrainMaterial>>; 3],
    terrain_layers: [u8; 3],
    pub(super) terrain_records: [Option<crate::reference::render_lighting::RendererRecord>; 3],
    terrain_shadow_modes: [u8; 3],
}
impl SourceWorldVisuals {
    #[cfg(target_arch = "wasm32")]
    pub(crate) fn startup_audit(&self) -> serde_json::Value {
        serde_json::json!({
            "road_bindings": self.road_materials.len(),
            "terrain_bindings": self.terrain.each_ref().map(|h| h.is_some()),
            "pending_surface": self.pending.as_ref().map(|p| serde_json::json!({
                "layer": format!("{:?}", p.key.layer), "meshes": p.meshes.len(),
                "tickets": p.tickets.iter().map(|t| t.as_ref().map(|t| format!("{:?}", t.state()))).collect::<Vec<_>>(),
            })),
        })
    }

    pub fn ready(&self, stream: &RendererWorld) -> bool {
        self.pending.is_none() && self.surfaces.len() == stream.frame().surfaces.len()
    }
    pub fn roots(&self) -> impl Iterator<Item = Entity> + '_ {
        self.surfaces.iter().map(|s| s.root)
    }

    pub(crate) fn measurement_meshes(
        &self,
    ) -> impl Iterator<Item = (bevy::asset::AssetId<Mesh>, bool)> + '_ {
        self.surfaces
            .iter()
            .flat_map(|s| s.meshes.iter().map(|m| (m.id(), false)))
            .chain(
                self.pending
                    .iter()
                    .flat_map(|s| s.meshes.iter().map(|m| (m.id(), true))),
            )
    }

    pub fn retire_session(&mut self, commands: &mut Commands, meshes: &mut Assets<Mesh>) {
        if let Some(pending) = &self.pending {
            pending.continuation.stop_flight();
        }
        for visual in self.surfaces.drain(..) {
            commands.entity(visual.root).despawn();
            for mesh in visual.meshes {
                meshes.remove(mesh.id());
            }
        }
        if self
            .pending
            .as_ref()
            .is_some_and(|p| p.continuation.drained())
        {
            let pending = self.pending.take().unwrap();
            for mesh in pending.meshes {
                meshes.remove(mesh.id());
            }
        }
        self.session = None;
    }

    pub fn reset_bindings(&mut self, terrain: &mut Assets<TerrainMaterial>) {
        self.road_materials.clear();
        for handle in &mut self.terrain {
            if let Some(handle) = handle.take() {
                terrain.remove(handle.id());
            }
        }
    }

    pub fn bind_road(
        &mut self,
        source: SourceId,
        materials: Vec<SurfaceMaterial>,
    ) -> Result<(), ReferenceError> {
        if materials.is_empty() {
            return Err(ReferenceError(
                "Source road has no material bindings".into(),
            ));
        }
        self.road_materials.insert(source, materials);
        Ok(())
    }
    pub fn bind_terrain(
        &mut self,
        tier: usize,
        material: Handle<TerrainMaterial>,
        layer: u8,
        shadow_mode: u8,
    ) {
        self.terrain[tier] = Some(material);
        self.terrain_layers[tier] = layer;
        self.terrain_shadow_modes[tier] = shadow_mode;
    }
    pub(crate) fn retire_stale(
        &mut self,
        stream: &RendererWorld,
        facts: &mut RendererFacts,
        meshes: &mut Assets<Mesh>,
    ) -> Result<(), ReferenceError> {
        let matches = |pending: &SurfacePreparation, request: WorldRequest, key: SurfaceKey| {
            pending.request.session == request.session
                && pending.request.revision == request.revision
                && pending.key == key
        };
        if self.pending.as_ref().is_some_and(|p| {
            p.request.session == stream.frame().request.session
                && stream.frame().surfaces.iter().any(|s| s.key == p.key)
        }) {
            return Ok(());
        }
        let current = stream
            .prepared_surface()
            .map(|(request, surface)| (request, surface.key));
        if self.pending.as_ref().is_some_and(|pending| {
            current.is_none_or(|(request, key)| !matches(pending, request, key))
                || pending.request.revision != stream.frame().request.revision
                || pending.request.session != stream.frame().request.session
        }) {
            self.pending.as_ref().unwrap().continuation.stop_flight();
            if !self.pending.as_ref().unwrap().continuation.drained() {
                return Ok(());
            }
            let mut pending = self.pending.take().unwrap();
            pending.continuation.release()?;
            facts.push(RendererFact::DiscardSurface(pending.request, pending.key));
            for mesh in pending.meshes {
                meshes.remove(mesh.id());
            }
        }
        Ok(())
    }
    pub fn prepare_surface(
        &mut self,
        stream: &RendererWorld,
        ledger: &mut crate::ownership::PresentationLedger,
        facts: &mut RendererFacts,
        meshes: &mut Assets<Mesh>,
        pipelines: &mut WorldPipelines,
        fixed: &SourceFixedAssets,
    ) -> Result<(), ReferenceError> {
        self.retire_stale(stream, facts, meshes)?;
        let Some((request, surface)) = stream.prepared_surface() else {
            return Ok(());
        };
        let key = surface.key;
        if request.revision != stream.frame().request.revision
            || request.session != stream.frame().request.session
        {
            return Ok(());
        }
        if let Some(source) = &surface.prototype {
            if !self.road_materials.contains_key(source) {
                return Ok(());
            }
        } else if self.terrain[tier(key.layer)?].is_none() {
            return Ok(());
        }
        if self.pending.is_none() {
            facts.push(RendererFact::ReserveSurface(request, key));
            let mut continuation =
                PipelineContinuation::counted(RendererOwner::Surface(request, key));
            let prepared = stream.prepared_surface().unwrap().1;
            let source = prepared.prototype.as_ref();
            let surface = &prepared.surface;
            let mut handles = Vec::with_capacity(surface.submeshes.len());
            let mut records = Vec::with_capacity(surface.submeshes.len());
            for (index, range) in surface.submeshes.iter().enumerate() {
                let record = if let Some(source) = source {
                    &self.road_materials[source][index].renderer
                } else {
                    self.terrain_records[tier(key.layer)?]
                        .as_ref()
                        .ok_or_else(|| ReferenceError("Missing terrain renderer state".into()))?
                };
                let (mesh, record) = surface_mesh(surface, range.clone(), record);
                handles.push(meshes.add(mesh));
                records.push(record);
            }
            let tickets = (0..handles.len()).map(|_| None).collect();
            let upload = fixed.watch_meshes(handles.iter().cloned());
            continuation.watch_upload(upload.receipt.clone());
            self.pending = Some(SurfacePreparation {
                request,
                continuation,
                _upload: upload,
                key,
                meshes: handles,
                tickets,
                records,
            });
        }
        let pending = self.pending.as_mut().unwrap();
        for index in 0..pending.meshes.len() {
            let source = stream.prepared_surface().unwrap().1.prototype.as_ref();
            PipelineTicket::retry(&mut pending.tickets[index]);
            if pending.tickets[index].is_none() {
                let ticket = if let Some(source) = source {
                    pipelines.request_continuation(
                        stream,
                        ledger,
                        &mut pending.continuation,
                        &pending.meshes[index],
                        &self.road_materials[source][index].material,
                        false,
                    )?
                } else {
                    pipelines.request_continuation(
                        stream,
                        ledger,
                        &mut pending.continuation,
                        &pending.meshes[index],
                        self.terrain[tier(key.layer)?].as_ref().unwrap(),
                        false,
                    )?
                };
                pending.tickets[index] = ticket;
            }
            if pending.tickets[index].is_none() {
                return Ok(());
            }
        }
        for ticket in pending.tickets.iter().flatten() {
            match ticket.state() {
                RenderingState::Loading => return Ok(()),
                RenderingState::Failed(error) => return Err(ReferenceError(error)),
                RenderingState::Ready => {}
            }
        }
        if !pending.continuation.drained() {
            return Ok(());
        }
        facts.push(RendererFact::AcceptSurface(request, key));
        Ok(())
    }
    pub fn sync(
        &mut self,
        frame: &RendererFrame<'_>,
        commands: &mut Commands,
        meshes: &mut Assets<Mesh>,
        roots: &mut super::runtime::WorldRoots,
    ) -> Result<(), ReferenceError> {
        if self.session != Some(frame.request.session) {
            for old in self.surfaces.drain(..) {
                commands.entity(old.root).despawn();
                for mesh in old.meshes {
                    meshes.remove(mesh.id());
                }
            }
            self.session = Some(frame.request.session);
        }
        // Check every new binding before retiring any accepted visual.
        for published in frame.surfaces {
            if self.surfaces.iter().any(|old| old.key == published.key) {
                continue;
            }
            if let Some(prototype) = &published.prototype {
                let bindings = self.road_materials.get(prototype).ok_or_else(|| {
                    ReferenceError(format!(
                        "Unbound source road {}:{}",
                        prototype.file, prototype.path_id
                    ))
                })?;
                if bindings.len() != published.surface.submeshes.len() {
                    return Err(ReferenceError(
                        "Source road submesh material count differs".into(),
                    ));
                }
            } else {
                let tier = tier(published.key.layer)?;
                if self.terrain[tier].is_none() {
                    return Err(ReferenceError(format!(
                        "Unbound source terrain tier {tier}"
                    )));
                }
            }
        }
        for published in frame.surfaces {
            let transform = Transform::from_translation(Vec3::from_array(
                frame.origin.local(published.surface.origin),
            ));
            if let Some(old) = self.surfaces.iter().find(|old| old.key == published.key) {
                if let Ok(mut current) = roots.get_mut(old.root) {
                    current.set_if_neq(transform);
                }
                continue;
            }
            let pending = self
                .pending
                .take()
                .filter(|pending| {
                    pending.key == published.key && pending.request.session == frame.request.session
                })
                .ok_or_else(|| {
                    ReferenceError("Published source surface has no prepared renderer".into())
                })?;
            let root = commands.spawn((transform, Visibility::Inherited)).id();
            for (index, mesh) in pending.meshes.iter().enumerate() {
                let record = &pending.records[index];
                if let Some(prototype) = &published.prototype {
                    let binding = &self.road_materials[prototype][index];
                    let mut entity = commands.spawn((
                        Mesh3d(mesh.clone()),
                        crate::presentation::renderer_visibility::SourceRenderer(record.clone()),
                        MeshMaterial3d(binding.material.clone()),
                        Transform::default(),
                        Visibility::Inherited,
                        RenderLayers::layer(binding.layer as usize),
                        SourceShadowMode(binding.shadow_mode),
                        bevy::light::NotShadowCaster,
                        ChildOf(root),
                    ));
                    if !binding.receive_shadows {
                        entity.insert(bevy::light::NotShadowReceiver);
                    }
                } else {
                    let tier = tier(published.key.layer)?;
                    commands.spawn((
                        Mesh3d(mesh.clone()),
                        crate::presentation::renderer_visibility::SourceRenderer(record.clone()),
                        MeshMaterial3d(self.terrain[tier].as_ref().unwrap().clone()),
                        Transform::default(),
                        Visibility::Inherited,
                        RenderLayers::layer(self.terrain_layers[tier] as usize),
                        SourceShadowMode(self.terrain_shadow_modes[tier]),
                        bevy::light::NotShadowCaster,
                        ChildOf(root),
                    ));
                }
            }
            self.surfaces.push(SurfaceVisual {
                key: published.key,
                root,
                meshes: pending.meshes,
                _tickets: pending.tickets,
            });
        }
        self.surfaces.retain(|old| {
            if frame.surfaces.iter().any(|surface| surface.key == old.key) {
                return true;
            }
            commands.entity(old.root).despawn();
            for mesh in &old.meshes {
                meshes.remove(mesh.id());
            }
            false
        });
        Ok(())
    }
}
fn tier(layer: WorldLayer) -> Result<usize, ReferenceError> {
    match layer {
        WorldLayer::Terrain => Ok(0),
        WorldLayer::DistantTerrain => Ok(1),
        WorldLayer::DistantTerrain2 => Ok(2),
        _ => Err(ReferenceError("Surface is not source terrain".into())),
    }
}
fn surface_mesh(
    surface: &WorldSurface,
    range: std::ops::Range<usize>,
    record: &crate::reference::render_lighting::RendererRecord,
) -> (Mesh, crate::reference::render_lighting::RendererRecord) {
    let mut mesh = Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::RENDER_WORLD,
    );
    mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, surface.vertices.clone());
    mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, surface.normals.clone());
    mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, surface.uv.clone());
    if !surface.colors.is_empty() {
        mesh.insert_attribute(Mesh::ATTRIBUTE_COLOR, surface.colors.clone());
    }
    if !surface.tangents.is_empty() {
        mesh.insert_attribute(Mesh::ATTRIBUTE_TANGENT, surface.tangents.clone());
    }
    mesh.insert_indices(Indices::U32(
        surface.triangles[range].iter().flatten().copied().collect(),
    ));
    let mut record = record.clone();
    let mut min = Vec3::splat(f32::MAX);
    let mut max = Vec3::splat(f32::MIN);
    for position in &surface.vertices {
        let p = Vec3::from_array(*position);
        min = min.min(p);
        max = max.max(p);
    }
    record.bounds_center = ((min + max) * 0.5).to_array();
    record.bounds_extents = ((max - min) * 0.5).to_array();
    let [center, extents] = record.attributes();
    mesh.insert_attribute(
        crate::presentation::renderers::SOURCE_BOUNDS_CENTER,
        vec![center; surface.vertices.len()],
    );
    mesh.insert_attribute(
        crate::presentation::renderers::SOURCE_BOUNDS_EXTENTS,
        vec![extents; surface.vertices.len()],
    );
    (mesh, record)
}
