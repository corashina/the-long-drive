//! Selected prototype loads and their renderer counts.
use super::{
    preflight::{
        SourcePlan, SourcePlanLoader, load_plan,
        selected::{SelectedMetadata, input_room},
    },
    prototype_loader::{SourceGltf, SourceGltfLoader, SourceGltfSettings},
};
use crate::ownership::{PresentationLedger, RendererCounts};
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
    state: State,
    lods: Option<Arc<super::lod::LodTable>>,
    nodes: Option<Arc<crate::presentation_logic::node_plan::NodePlan>>,
}
#[derive(Resource, Default)]
pub struct SourcePrototypes {
    entries: Vec<Entry>,
    next_request: u64,
    accounted_session: u64,
    retired_sources: Vec<SourceId>,
    io: RendererCounts,
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
        ledger.add_pending(1);
        self.io.pending += 1;
        self.next_request = self.next_request.wrapping_add(1);
        self.entries.push(Entry {
            source: source.clone(),
            session,
            request: self.next_request,
            retired: false,
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
        let prototype = reference.world.prototype(source)?;
        let record = reference
            .assets
            .iter()
            .find(|a| a.id == prototype.asset)
            .ok_or_else(|| ReferenceError("Missing prototype asset".into()))?;
        if !input_room(ledger, false) {
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
        build: impl FnOnce(
            &LoadedPrototype,
        )
            -> Result<crate::presentation_logic::node_plan::NodePlan, ReferenceError>,
    ) -> Result<Option<Arc<crate::presentation_logic::node_plan::NodePlan>>, ReferenceError> {
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
                .find(&e.source)
                .is_some_and(|p| p.deferred_materials.iter().any(|m| m.source == *source))
        })
    }
    pub fn references_asset(
        &self,
        reference: &ReferenceManifest,
        asset: crate::reference::AssetId,
    ) -> bool {
        self.entries.iter().filter(|e| !e.retired).any(|e| {
            reference.world.prototypes.find(&e.source).is_some_and(|p| {
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
            self.io = RendererCounts::default();
            self.accounted_session = session;
        }
        let mut io = RendererCounts::default();
        for entry in &self.entries {
            io.add(entry.state.io());
        }
        io.apply(self.io, ledger)?;
        self.io = io;
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
        uploads: &mut crate::presentation_logic::upload_budget::UploadBudget,
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
                    let prototype = reference.world.prototype(&entry.source)?;
                    entry.retired = true;
                    validate_plan(plan, prototype, reference)?;
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
                    if !assets.contains(asset.id()) {
                        continue;
                    }
                    if entry.retired {
                        completed = Some(index);
                        break;
                    }
                    // Untracked: the move is not a new prototype version.
                    let loaded = assets.get_mut_untracked(asset.id()).unwrap();
                    for mesh in std::mem::take(&mut loaded.pending) {
                        loaded.meshes.push(uploads.add(meshes, mesh));
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
                    entry.state.complete_payload();
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
            if let Some(handle) = asset
                && let Some(asset) = assets.remove(handle.id())
            {
                for mesh in asset.meshes {
                    meshes.remove(mesh.id());
                }
                for skin in asset.inverse_bindposes {
                    skins.remove(skin.id());
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
            let prototype = reference.world.prototype(&entry.source)?;
            let record = reference
                .assets
                .iter()
                .find(|a| a.id == prototype.asset)
                .ok_or_else(|| ReferenceError("Missing prototype asset".into()))?;
            if !input_room(ledger, true) {
                return Ok(());
            }
            facts.push(crate::world::RendererFact::HoldPrototype(
                entry.source.clone(),
            ));
            let handle = load_plan(server, &reference.file_catalog, &record.path)?;
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
