//! Selected prototype ownership. Reservations precede every accessor or scene allocation.
use super::{
    preflight::{
        SourcePlan, SourcePlanLoader, load_plan,
        selected::{IoLedger, Reservation, SelectedMetadata, control_bytes, retained_plan_bytes},
    },
    prototype_loader::{SourceGltf, SourceGltfLoader, SourceGltfSettings},
};
use crate::ownership::PresentationLedger;
use crate::{
    reference::{ReferenceError, ReferenceManifest, SourceId, prototype_plan::PrototypePlan},
    world::RendererWorld,
};
use bevy::{
    asset::LoadState, ecs as bevy_ecs, mesh::skinning::SkinnedMeshInverseBindposes, prelude::*,
};
use std::sync::Arc;

pub type LoadedPrototype =
    super::preflight::selected::LoadedPrototype<Handle<SourcePlan>, Handle<SourceGltf>>;
type State = SelectedMetadata<Handle<SourcePlan>, Handle<SourceGltf>>;
struct Entry {
    source: SourceId,
    session: u64,
    request: u64,
    retired: bool,
    cpu: usize,
    gpu: usize,
    staging: usize,
    state: State,
    lods: Option<Arc<super::lod::LodTable>>,
    nodes: Option<Arc<super::node_plan::NodePlan>>,
}
#[derive(Resource, Default)]
pub struct SourcePrototypes {
    entries: Vec<Entry>,
    next_request: u64,
    accounted_session: u64,
    /// Prototype bytes of all entries, as charged to the ledger.
    entry_cpu: usize,
    entry_gpu: usize,
    retired_sources: Vec<SourceId>,
    io: IoLedger,
    control_cpu: usize,
    /// Inputs of the last `retire_unused` pass.
    unused_key: Option<[u64; 8]>,
    /// Prototype indices with an instance or a pending or published structure.
    used: Vec<bool>,
    /// Prototype indices whose queued request may stay. See `retire_unused`.
    queued: Vec<bool>,
}
pub struct SourcePrototypePlugin;
impl Plugin for SourcePrototypePlugin {
    fn build(&self, app: &mut App) {
        app.init_asset::<SourceGltf>()
            .init_asset_loader::<SourceGltfLoader>()
            .init_asset::<SourcePlan>()
            .init_asset_loader::<SourcePlanLoader>()
            .init_resource::<SourcePrototypes>();
    }
}
impl SourcePrototypes {
    pub fn cancel(&mut self) {
        for e in &mut self.entries {
            e.retired = true;
        }
    }
    pub fn drain_ready(
        &mut self,
        server: &AssetServer,
        assets: &Assets<SourceGltf>,
        plans: &Assets<SourcePlan>,
    ) -> bool {
        for e in &mut self.entries {
            let terminal = match &e.state {
                State::PlanLoading(job) => super::preflight::plan_terminal(server, plans, job),
                State::Loading { asset, .. } => {
                    assets.contains(asset.id())
                        || matches!(server.load_state(asset.id()), LoadState::Failed(_))
                }
                _ => true,
            };
            if !e.state.can_retire(terminal) {
                return false;
            }
        }
        true
    }
    pub fn release_all(
        self,
        assets: &mut Assets<SourceGltf>,
        _plans: &mut Assets<SourcePlan>,
        meshes: &mut Assets<Mesh>,
        skins: &mut Assets<SkinnedMeshInverseBindposes>,
    ) {
        for e in self.entries {
            let asset = match e.state {
                State::Ready(loaded) if Arc::strong_count(&loaded) == 1 => {
                    Some(loaded.asset.clone())
                }
                State::Loading { asset, .. } => Some(asset),
                _ => None,
            };
            if let Some(asset) = asset.and_then(|h| assets.remove(h.id())) {
                for m in asset.meshes {
                    meshes.remove(m.id());
                }
                for s in asset.inverse_bindposes {
                    skins.remove(s.id());
                }
            }
        }
    }

    pub fn settled(&self) -> bool {
        self.entries
            .iter()
            .all(|e| matches!(e.state, State::Ready(_)))
    }

    pub fn request(
        &mut self,
        stream: &RendererWorld,
        ledger: &mut PresentationLedger,
        source: &SourceId,
    ) -> Result<(), ReferenceError> {
        let session = stream.frame().request.session;
        self.account(stream, ledger)?;
        if self.entries.iter().any(|e| e.source == *source) {
            return Ok(());
        }
        if !ledger.can_request_independent(1) {
            return Ok(());
        }
        if self.entries.len() >= 8192 {
            return Err(ReferenceError("Source prototype owner limit".into()));
        }
        // Reserve container storage before extending the selected owner list.
        let retired_capacity = self.entries.len() + self.retired_sources.len() + 1;
        let extra = usize::from(self.entries.len() == self.entries.capacity())
            * std::mem::size_of::<Entry>()
            + source.file.len()
            + retired_capacity.saturating_sub(self.retired_sources.capacity())
                * std::mem::size_of::<SourceId>();
        let mut allocation = ledger.allocation();
        allocation.prototype_cpu += extra;
        allocation.pending += 1;
        ledger.set_allocation(allocation)?;
        self.io.pending += 1;
        self.next_request = self.next_request.wrapping_add(1);
        self.control_cpu += extra;
        self.entries.reserve_exact(1);
        if self.retired_sources.capacity() < retired_capacity {
            self.retired_sources
                .reserve_exact(retired_capacity - self.retired_sources.len());
        }
        self.entries.push(Entry {
            source: source.clone(),
            session,
            request: self.next_request,
            retired: false,
            cpu: 0,
            gpu: 0,
            staging: 0,
            state: State::Queued,
            lods: None,
            nodes: None,
        });
        Ok(())
    }
    pub fn plan(&self, session: u64, source: &SourceId) -> Option<&PrototypePlan> {
        self.entries
            .iter()
            .find(|e| e.session == session && e.source == *source && !e.retired)
            .and_then(|e| e.state.plan())
    }
    pub fn admit_payload(
        &mut self,
        stream: &RendererWorld,
        ledger: &mut PresentationLedger,
        reference: &ReferenceManifest,
        server: &AssetServer,
        source: &SourceId,
    ) -> Result<bool, ReferenceError> {
        self.account(stream, ledger)?;
        let session = stream.frame().request.session;
        let Some(entry) = self
            .entries
            .iter_mut()
            .find(|e| e.session == session && e.source == *source && !e.retired)
        else {
            return Ok(false);
        };
        let State::Planned(plan) = &entry.state else {
            return Ok(matches!(
                entry.state,
                State::Loading { .. } | State::Ready(_)
            ));
        };
        let prototype = reference
            .world
            .prototypes
            .get(source)
            .ok_or_else(|| ReferenceError("Missing checked source prototype".into()))?;
        let record = reference
            .assets
            .iter()
            .find(|a| a.id == prototype.asset)
            .ok_or_else(|| ReferenceError("Missing prototype asset".into()))?;
        let reservation = entry.state.payload_reservation()?;
        if !reservation.fits(ledger, entry.cpu, entry.gpu, false) {
            return Ok(false);
        }
        let settings = SourceGltfSettings {
            json_bytes: plan.json_bytes,
            buffer_bytes: plan.buffer_bytes,
            mesh_bytes: plan.mesh_bytes,
        };
        let asset = server
            .load_builder()
            .with_settings(move |target: &mut SourceGltfSettings| {
                *target = SourceGltfSettings {
                    json_bytes: settings.json_bytes,
                    buffer_bytes: settings.buffer_bytes,
                    mesh_bytes: settings.mesh_bytes,
                };
            })
            .load::<SourceGltf>(record.path.clone());
        entry.state.start_payload(asset);
        entry.cpu = reservation.cpu;
        entry.gpu = reservation.gpu;
        entry.staging = reservation.staging;
        self.account(stream, ledger)?;
        Ok(true)
    }
    pub fn loaded(&self, session: u64, source: &SourceId) -> Option<&LoadedPrototype> {
        self.entries.iter().find_map(|entry| {
            if entry.session == session && entry.source == *source && !entry.retired {
                if let State::Ready(loaded) = &entry.state {
                    Some(loaded.as_ref())
                } else {
                    None
                }
            } else {
                None
            }
        })
    }
    /// LOD table of a loaded prototype. The first call builds it.
    pub fn lods(
        &mut self,
        session: u64,
        source: &SourceId,
        build: impl FnOnce() -> Result<super::lod::LodTable, ReferenceError>,
    ) -> Result<Option<Arc<super::lod::LodTable>>, ReferenceError> {
        let Some(entry) = self.entries.iter_mut().find(|e| {
            e.session == session
                && e.source == *source
                && !e.retired
                && matches!(e.state, State::Ready(_))
        }) else {
            return Ok(None);
        };
        if entry.lods.is_none() {
            entry.lods = Some(Arc::new(build()?));
        }
        Ok(entry.lods.clone())
    }
    /// Node plan of a loaded prototype. The first call builds it.
    pub fn nodes(
        &mut self,
        session: u64,
        source: &SourceId,
        build: impl FnOnce(&LoadedPrototype) -> Result<super::node_plan::NodePlan, ReferenceError>,
    ) -> Result<Option<Arc<super::node_plan::NodePlan>>, ReferenceError> {
        let Some(entry) = self
            .entries
            .iter_mut()
            .find(|e| e.session == session && e.source == *source && !e.retired)
        else {
            return Ok(None);
        };
        let State::Ready(loaded) = &entry.state else {
            return Ok(None);
        };
        if entry.nodes.is_none() {
            entry.nodes = Some(Arc::new(build(loaded)?));
        }
        Ok(entry.nodes.clone())
    }
    /// Retirement keeps loader reservations until cancellation or loading has completed.
    pub fn retire(&mut self, source: &SourceId) {
        for entry in &mut self.entries {
            if entry.source == *source {
                entry.retired = true;
            }
        }
    }
    pub fn take_retired(&mut self) -> Option<SourceId> {
        self.retired_sources.pop()
    }
    pub fn references_material(&self, reference: &ReferenceManifest, source: &SourceId) -> bool {
        self.entries.iter().filter(|e| !e.retired).any(|e| {
            reference
                .world
                .prototypes
                .get(&e.source)
                .is_some_and(|p| p.deferred_materials.iter().any(|m| m.source == *source))
        })
    }
    pub fn references_asset(
        &self,
        reference: &ReferenceManifest,
        asset: crate::reference::AssetId,
    ) -> bool {
        self.entries.iter().filter(|e| !e.retired).any(|e| {
            reference.world.prototypes.get(&e.source).is_some_and(|p| {
                p.asset == asset
                    || p.nodes.iter().any(|n| {
                        super::preflight::initial_material(reference, &n.source)
                            .is_some_and(|m| m.0 == asset)
                    })
            })
        })
    }
    pub fn requested(&self, source: &SourceId) -> bool {
        self.entries
            .iter()
            .any(|e| !e.retired && e.source == *source)
    }
    pub fn retire_unused(
        &mut self,
        stream: &RendererWorld,
        reference: &ReferenceManifest,
        instances: &super::instances::SourceInstances,
    ) {
        let held =
            |e: &Entry| matches!(&e.state, State::Ready(loaded) if Arc::strong_count(loaded)>1);
        // Request numbers identify entries. The sums change when an entry is added, removed,
        // retired, restored, or shared with a fork.
        let (active, shared) = self.entries.iter().fold((0u64, 0u64), |(a, s), e| {
            (
                a.wrapping_add(if e.retired { 0 } else { e.request }),
                s.wrapping_add(if held(e) { e.request } else { 0 }),
            )
        });
        let (session, structures, pending) = stream.structure_membership();
        let required_ready = stream.required_ready();
        let key = [
            session,
            structures,
            pending,
            instances.revision(),
            active,
            shared,
            u64::from(required_ready),
            stream.required_structure_count() as u64,
        ];
        if self.unused_key == Some(key) {
            return;
        }
        self.unused_key = Some(key);
        let prototypes = &reference.world.prototypes;
        self.used.clear();
        self.used.resize(prototypes.len(), false);
        self.queued.clear();
        self.queued.resize(prototypes.len(), false);
        for index in instances.prototypes() {
            self.used[index] = true;
            self.queued[index] = true;
        }
        let frame = stream.frame();
        // While the world is not ready, only required structures request descriptors. A queued
        // request of another structure holds a pending claim that a required request needs.
        let queued_wanted = |id: &crate::world::GeneratedId| {
            required_ready || stream.structure_required(id) || stream.structure_replay_required(id)
        };
        for placement in stream.pending_structures() {
            if let Some(index) = prototypes.index(&placement.prototype) {
                self.used[index] = true;
                self.queued[index] |= queued_wanted(&placement.id);
            }
        }
        for structure in frame.structures {
            if let Some(index) = prototypes.index(&structure.placement.prototype) {
                self.used[index] = true;
                self.queued[index] = true;
            }
        }
        for entry in &mut self.entries {
            let queued = matches!(entry.state, State::Queued);
            if held(entry)
                || reference.world.road.prototypes.contains(&entry.source)
                || prototypes
                    .index(&entry.source)
                    .is_some_and(|index| self.used[index] && (!queued || self.queued[index]))
            {
                continue;
            }
            entry.retired = true;
        }
    }
    fn account(
        &mut self,
        stream: &RendererWorld,
        ledger: &mut PresentationLedger,
    ) -> Result<(), ReferenceError> {
        let session = stream.frame().request.session;
        if self.accounted_session != session {
            self.io = IoLedger::default();
            self.control_cpu = 0;
            self.entry_cpu = 0;
            self.entry_gpu = 0;
            self.accounted_session = session;
        }
        let entry_cpu = self.entries.iter().map(|e| e.cpu).sum::<usize>();
        let entry_gpu = self.entries.iter().map(|e| e.gpu).sum::<usize>();
        let mut io = IoLedger::default();
        for entry in &self.entries {
            io.add(entry.state.io(entry.staging));
        }
        let control_cpu = control_bytes::<Entry>(
            self.entries.capacity(),
            self.entries.iter().map(|e| e.source.file.capacity()).sum(),
            self.retired_sources.capacity(),
            self.retired_sources.iter().map(|s| s.file.capacity()).sum(),
        ) + self.used.capacity()
            + self.queued.capacity();
        let mut allocation = ledger.allocation();
        allocation.prototype_cpu = allocation
            .prototype_cpu
            .saturating_sub(self.entry_cpu + self.control_cpu)
            + entry_cpu
            + control_cpu;
        allocation.prototype_gpu =
            allocation.prototype_gpu.saturating_sub(self.entry_gpu) + entry_gpu;
        allocation = io.replace(self.io, allocation);
        ledger.set_allocation(allocation)?;
        self.entry_cpu = entry_cpu;
        self.entry_gpu = entry_gpu;
        self.io = io;
        self.control_cpu = control_cpu;
        Ok(())
    }
    pub fn prepare(
        &mut self,
        stream: &RendererWorld,
        ledger: &mut PresentationLedger,
        facts: &mut crate::world::RendererFacts,
        reference: &ReferenceManifest,
        server: &AssetServer,
        assets: &mut Assets<SourceGltf>,
        plans: &mut Assets<SourcePlan>,
        meshes: &mut Assets<Mesh>,
        skins: &mut Assets<SkinnedMeshInverseBindposes>,
    ) -> Result<(), ReferenceError> {
        let session = stream.frame().request.session;
        for entry in &mut self.entries {
            if entry.session != session {
                entry.retired = true;
            }
        }
        let mut completed = None;
        for index in 0..self.entries.len() {
            let entry = &mut self.entries[index];
            match &mut entry.state {
                State::Queued | State::Planned(_) => {
                    if entry.retired {
                        completed = Some(index);
                        break;
                    }
                }
                State::PlanLoading(job) => {
                    let result = super::preflight::poll_plan(server, plans, job);
                    if let Err(error) = result {
                        if entry.retired {
                            completed = Some(index);
                            break;
                        }
                        entry.retired = true;
                        return Err(error);
                    }
                    let Some(plan) = result.unwrap() else {
                        continue;
                    };
                    if entry.retired {
                        completed = Some(index);
                        break;
                    }
                    entry.state.complete_metadata(plan);
                    let plan = entry.state.plan().unwrap();
                    let prototype = reference
                        .world
                        .prototypes
                        .get(&entry.source)
                        .ok_or_else(|| ReferenceError("Missing checked source prototype".into()))?;
                    entry.retired = true;
                    validate_plan(plan, prototype, reference)?;
                    let cpu = retained_plan_bytes(plan);
                    entry.cpu = cpu;
                    entry.gpu = 0;
                    entry.staging = 0;
                    entry.retired = false;
                }
                State::Loading { plan, asset } => {
                    if let LoadState::Failed(error) = server.load_state(asset.id()) {
                        if entry.retired {
                            completed = Some(index);
                            break;
                        }
                        entry.retired = true;
                        return Err(ReferenceError(format!(
                            "Source prototype {}:{}: {error}",
                            entry.source.file, entry.source.path_id
                        )));
                    }
                    let Some(loaded) = assets.get(asset) else {
                        continue;
                    };
                    if entry.retired {
                        completed = Some(index);
                        break;
                    }
                    if !loaded.meshes.iter().all(|mesh| meshes.contains(mesh.id()))
                        || !loaded
                            .inverse_bindposes
                            .iter()
                            .all(|skin| skins.contains(skin.id()))
                    {
                        continue;
                    }
                    entry.retired = true;
                    if loaded.primitives.len()
                        != plan
                            .nodes
                            .iter()
                            .map(|n| {
                                plan.primitives
                                    .iter()
                                    .filter(|p| Some(p.mesh) == n.mesh)
                                    .count()
                            })
                            .sum::<usize>()
                    {
                        return Err(ReferenceError(
                            "Source primitive count changed after preflight".into(),
                        ));
                    }
                    // Keep both main-world data and render extraction/upload copies charged.
                    let cpu = loaded.mesh_bytes * 2
                        + loaded.skin_bytes * 2
                        + retained_plan_bytes(plan)
                        + State::ready_control_bytes()
                        + plan.scene_bytes
                        + loaded.meshes.len() * 4096;
                    let gpu = loaded.mesh_bytes + loaded.skin_bytes + loaded.meshes.len() * 8;
                    entry.cpu = cpu;
                    entry.gpu = gpu;
                    entry.state.complete_payload();
                    entry.staging = 0;
                    entry.retired = false;
                }
                State::Ready(_) => {
                    if entry.retired {
                        completed = Some(index);
                        break;
                    }
                }
            }
        }
        if let Some(index) = completed {
            let entry = &self.entries[index];
            // Active geometry must release its owners before prototype resources retire.
            facts.push(crate::world::RendererFact::ReleasePrototype(
                entry.source.clone(),
            ));
            let entry = self.entries.remove(index);
            self.retired_sources.push(entry.source);
            let asset = match entry.state {
                State::Ready(loaded) => {
                    if Arc::strong_count(&loaded) > 1 {
                        None
                    } else {
                        Some(loaded.asset.clone())
                    }
                }
                State::Loading { asset, .. } => Some(asset),
                _ => None,
            };
            if let Some(handle) = asset {
                if let Some(asset) = assets.remove(handle.id()) {
                    for mesh in asset.meshes {
                        meshes.remove(mesh.id());
                    }
                    for skin in asset.inverse_bindposes {
                        skins.remove(skin.id());
                    }
                }
            }
        }
        self.account(stream, ledger)?;
        if let Some(entry) = self
            .entries
            .iter_mut()
            .find(|entry| !entry.retired && matches!(entry.state, State::Queued))
        {
            if ledger.available_external_jobs() == 0 {
                return Ok(());
            }
            let prototype = reference
                .world
                .prototypes
                .get(&entry.source)
                .ok_or_else(|| ReferenceError("Missing checked source prototype".into()))?;
            let record = reference
                .assets
                .iter()
                .find(|a| a.id == prototype.asset)
                .ok_or_else(|| ReferenceError("Missing prototype asset".into()))?;
            let reservation = Reservation::metadata(&reference.file_catalog, &record.path)?;
            // A full stream table waits like a full credit.
            if !stream.can_hold_prototype(&entry.source) {
                ledger.wait_for_hold();
                return Ok(());
            }
            if !reservation.fits(ledger, entry.cpu, entry.gpu, true) {
                return Ok(());
            }
            facts.push(crate::world::RendererFact::HoldPrototype(
                entry.source.clone(),
            ));
            let handle = load_plan(
                server,
                &reference.file_catalog,
                &record.path,
                reservation.cpu,
                reservation.staging,
            )?;
            entry.cpu = reservation.cpu;
            entry.gpu = 0;
            entry.staging = reservation.staging;
            entry.state = State::PlanLoading(handle);
            self.account(stream, ledger)?;
        }
        Ok(())
    }
}
fn validate_plan(
    plan: &PrototypePlan,
    prototype: &crate::reference::WorldPrototypeReference,
    reference: &ReferenceManifest,
) -> Result<(), ReferenceError> {
    for source in &prototype.nodes {
        let node = plan
            .nodes
            .get(source.gltf_node as usize)
            .ok_or_else(|| ReferenceError("Missing checked prototype node".into()))?;
        if node.index != source.gltf_node
            || node.extras.as_ref().and_then(|e| e.reference_id.as_ref()) != Some(&source.source)
        {
            return Err(ReferenceError(
                "Prototype node ownership differs from checked contract".into(),
            ));
        }
    }
    for primitive in &plan.primitives {
        let null = primitive.source_material.is_none()
            && (primitive.has_source_material || primitive.material.is_none());
        if null {
            let mut owners = plan.nodes.iter().filter(|n| n.mesh == Some(primitive.mesh));
            if !owners.clone().any(|_| true)
                || !owners.all(|node| {
                    node.extras
                        .as_ref()
                        .and_then(|e| e.reference_id.as_ref())
                        .and_then(|id| super::preflight::initial_material(reference, id))
                        .is_some()
                })
            {
                return Err(ReferenceError(
                    "Unresolved checked startup material slot".into(),
                ));
            }
            continue;
        }
        if let Some(source) = &primitive.source_material {
            if !prototype
                .deferred_materials
                .iter()
                .any(|m| m.source == *source)
            {
                return Err(ReferenceError(
                    "Unresolved deferred prototype material".into(),
                ));
            }
        } else if !primitive.has_source_material
            && primitive
                .material
                .as_ref()
                .is_none_or(|material| material.asset != prototype.asset)
        {
            return Err(ReferenceError(
                "Unresolved primary prototype material".into(),
            ));
        }
    }
    Ok(())
}
