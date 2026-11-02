//! Exact ownership for deferred GPU pipeline creation and retained cache metadata.
use super::gpu_prepare::GpuPreparation;
use crate::reference::ReferenceError;
use bevy::{
    ecs as bevy_ecs,
    prelude::*,
    render::{render_resource::*, renderer::RenderDevice},
};
use std::{
    collections::{HashMap, HashSet},
    sync::Mutex,
};
// Count and retained capacity are separate bounds. Every bank admits the persistent cache reserve.
pub const CACHE_SLOTS: usize = 2048;
pub const DESCRIPTOR_BYTES: usize = 64 * 1024;
pub const CACHE_BYTES: usize = 16 * 1024 * 1024;
struct Entry {
    base: CachedRenderPipelineId,
    owner: Option<GpuPreparation>,
    phase: HeldEntryPhase,
}
#[derive(Resource, Default)]
pub struct PipelineOwners {
    entries: Mutex<HashMap<CachedRenderPipelineId, Entry>>,
    isolated_bases: HashSet<CachedRenderPipelineId>,
}
impl PipelineOwners {
    pub(crate) fn isolated(&self, id: CachedRenderPipelineId) -> bool {
        self.entries.lock().unwrap().contains_key(&id)
    }
    pub(crate) fn base_isolated(&self, base: CachedRenderPipelineId) -> bool {
        self.isolated_bases.contains(&base)
    }
    fn owner(&self, base: CachedRenderPipelineId) -> Option<GpuPreparation> {
        self.entries
            .lock()
            .unwrap()
            .get(&base)
            .and_then(|e| e.owner.clone())
    }
    pub(crate) fn cancelled(&self, base: CachedRenderPipelineId) -> bool {
        self.owner(base).is_some_and(|owner| owner.cancelled())
    }
    pub(crate) fn fail(&self, base: CachedRenderPipelineId, message: String) -> bool {
        let Some(owner) = self.owner(base) else {
            return false;
        };
        owner.fail(message);
        true
    }
    fn companion(
        &self,
        base: CachedRenderPipelineId,
        id: CachedRenderPipelineId,
        owner: GpuPreparation,
    ) {
        let mut entries = self.entries.lock().unwrap();
        let old = entries.insert(
            id,
            Entry {
                base,
                owner: Some(owner),
                phase: HeldEntryPhase::Held,
            },
        );
        entries[&id].owner.as_ref().unwrap().retain_pipeline();
        if let Some(old) = old {
            if old.phase == HeldEntryPhase::Held {
                old.owner.unwrap().release_pipeline();
            }
        }
    }
    pub(crate) fn queue_companion(
        &self,
        cache: &PipelineCache,
        base: CachedRenderPipelineId,
        build: impl FnOnce() -> Option<RenderPipelineDescriptor>,
    ) -> Result<Option<CachedRenderPipelineId>, ReferenceError> {
        let owner = self.owner(base);
        if owner.as_ref().is_some_and(|owner| owner.cancelled()) {
            return Ok(None);
        }
        let _queue = owner
            .as_ref()
            .map(|_| queue_scope(cache))
            .transpose()
            .map_err(|error| ReferenceError(error.into()))?;
        let Some(descriptor) = build() else {
            return Ok(None);
        };
        let id = cache.queue_render_pipeline(descriptor);
        if let Some(owner) = owner {
            cache.hold_render_pipeline(id);
            self.companion(base, id, owner);
        }
        Ok(Some(id))
    }
    fn reclaim_active(&self, cache: &mut PipelineCache, base: CachedRenderPipelineId) -> bool {
        let mut reclaimed = false;
        self.entries.lock().unwrap().retain(|id, entry| {
            if *id != base && entry.base != base {
                return true;
            }
            reclaimed = true;
            cache.release_held_render_pipeline(*id);
            if let Some(owner) = &entry.owner {
                if entry.phase == HeldEntryPhase::Held {
                    owner.release_pipeline();
                }
            }
            false
        });
        reclaimed
    }
}
pub(crate) fn specialize_base(
    world: &mut World,
    specialize: impl FnOnce(&mut World) -> Result<CachedRenderPipelineId, ReferenceError>,
) -> Result<Option<CachedRenderPipelineId>, ReferenceError> {
    let id = specialize(world)?;
    let changed = world.resource_scope(|world, owners: Mut<PipelineOwners>| {
        owners.reclaim_active(&mut world.resource_mut::<PipelineCache>(), id)
    });
    if changed {
        world
            .resource_mut::<super::caster::CasterPasses>()
            .revisit(id);
        world
            .resource_mut::<super::surface_pass::SurfacePasses>()
            .revisit(id);
    }
    Ok(Some(id))
}
fn queue_scope(cache: &PipelineCache) -> Result<OwnedRenderPipelineQueue, &'static str> {
    cache.owned_render_pipeline_queue(CACHE_SLOTS, CACHE_BYTES, DESCRIPTOR_BYTES)
}
pub(crate) fn refresh_isolation(world: &mut World) {
    world.resource_scope(|world, mut owners: Mut<PipelineOwners>| {
        let active = world.resource::<super::world::pipelines::WorldPipelines>();
        owners.isolated_bases.retain(|id| !active.owns(*id));
    });
}
pub(crate) fn prepare(world: &mut World) {
    let device = world.resource::<RenderDevice>().clone();
    world.resource_scope(|world, owners: Mut<PipelineOwners>| {
        let mut entries = owners.entries.lock().unwrap();
        let mut cache = world.resource_mut::<PipelineCache>();
        entries.retain(|id, entry| {
            let Some(owner) = entry.owner.as_ref() else {
                return true;
            };
            let observation = if entry.phase == HeldEntryPhase::Retired {
                HeldEntryObservation {
                    cancelled: false,
                    isolated: false,
                    pipeline_ready: false,
                    preparation_valid: false,
                }
            } else {
                let cancelled = owner.cancelled();
                let isolated = !cancelled && owner.isolated();
                let pipeline_ready = !cancelled && cache.get_render_pipeline(*id).is_some();
                HeldEntryObservation {
                    cancelled,
                    isolated,
                    pipeline_ready,
                    preparation_valid: isolated && !pipeline_ready && owner.ready().is_ok(),
                }
            };
            match held_entry_step(&mut entry.phase, observation) {
                HeldEntryAction::Wait => true,
                HeldEntryAction::Retire => {
                    cache.retire_held_render_pipeline(*id);
                    owner.release_pipeline();
                    true
                }
                HeldEntryAction::Release => {
                    cache.release_held_render_pipeline(*id);
                    owner.release_pipeline();
                    false
                }
                HeldEntryAction::Prepare => {
                    let _scope = owner.scope(&device);
                    cache.prepare_held_render_pipeline(*id);
                    true
                }
            }
        });
    });
}

#[derive(Debug, PartialEq, Eq)]
pub enum HeldEntryPhase {
    Held,
    Retired,
}
#[derive(Debug, PartialEq, Eq)]
pub enum HeldEntryAction {
    Wait,
    Prepare,
    Retire,
    Release,
}
pub struct HeldEntryObservation {
    pub cancelled: bool,
    pub isolated: bool,
    pub pipeline_ready: bool,
    pub preparation_valid: bool,
}
pub fn held_entry_step(
    phase: &mut HeldEntryPhase,
    observation: HeldEntryObservation,
) -> HeldEntryAction {
    if *phase == HeldEntryPhase::Retired {
        return HeldEntryAction::Wait;
    }
    if observation.cancelled {
        *phase = HeldEntryPhase::Retired;
        return HeldEntryAction::Retire;
    }
    if !observation.isolated {
        assert!(observation.pipeline_ready, "Promoted pipeline is not ready");
        return HeldEntryAction::Release;
    }
    if !observation.pipeline_ready && observation.preparation_valid {
        HeldEntryAction::Prepare
    } else {
        HeldEntryAction::Wait
    }
}
