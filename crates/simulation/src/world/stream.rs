//! Bounded source generation. Publication uses absolute geometry and the current physics origin.
use super::road_worker::{RoadJobId, RoadLayout, RoadPoll, RoadWorker};
use super::terrain::{CompletedTerrain, TerrainExecution, TerrainMetadata};
use super::{
    AbsolutePose, Placement, PlacementIndex, RoadIndex, RoadSegment, RoadSequence, StartupTerrain,
    TerrainContext, TerrainEvaluator, TerrainFoundation, TerrainJob, TerrainRoadPoint, TerrainTile,
    WorldBounds, WorldLayer, WorldMetrics, WorldOrigin, WorldSurface,
};
use crate::reference::{ReferenceError, ReferenceManifest, SourceId, WorldReference};
use crate::vehicle::{ColliderHandle, PhysicsWorld};
use std::{
    collections::HashMap,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
};

const MIB: usize = 1024 * 1024;
const MAX_PENDING: usize = 32;
/// Focus movement in metres before the structure window refreshes.
const STRUCTURE_REFRESH_DISTANCE: f32 = 1.0;
pub const STAGING: usize = 32 * MIB;
pub const CHECKPOINTS: usize = 16 * MIB;
pub const PROTOTYPE_CPU: usize = 512 * MIB;
/// The first part of the world that a supported area still waits for.
#[derive(Clone, Copy, Debug)]
pub enum SupportGap<'a> {
    /// The plan or road list is not complete.
    Plan,
    /// A wanted road segment has no collider. `surface` tells if its surface exists.
    Road { index: u32, surface: bool },
    /// A wanted structure is not built.
    Structure(&'a Placement),
    /// A light influences the area but its structure is not built.
    Light,
    /// The area has more terrain tiles than the limit.
    TileCount,
    /// A terrain tile has no collider.
    Terrain([i64; 2]),
}
pub const PROTOTYPE_GPU: usize = 1024 * MIB;
static NEXT_SESSION: AtomicU64 = AtomicU64::new(1);

#[derive(Clone, Copy, Debug, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct WorldRequest {
    pub session: u64,
    pub revision: u64,
    pub player: [f64; 3],
    pub vehicle: [f64; 3],
}
/// Renderer-owned allocations, including loader staging.
#[derive(Clone, Copy, Debug, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct PresentationAllocation {
    pub grass_worker_wasm_bytes: usize,
    pub grass_worker_cache_bytes: usize,
    pub grass_worker_reserved_bytes: usize,
    pub active_cpu: usize,
    pub active_gpu: usize,
    pub prototype_cpu: usize,
    pub prototype_gpu: usize,
    pub jobs: usize,
    pub pending: usize,
    pub staging_bytes: usize,
    pub dependency_staging_bytes: usize,
    pub objects: usize,
}
impl PresentationAllocation {
    /// Applies `f` to each pair of fields.
    pub fn zip(self, other: Self, f: impl Fn(usize, usize) -> usize) -> Self {
        Self {
            grass_worker_wasm_bytes: f(self.grass_worker_wasm_bytes, other.grass_worker_wasm_bytes),
            grass_worker_cache_bytes: f(
                self.grass_worker_cache_bytes,
                other.grass_worker_cache_bytes,
            ),
            grass_worker_reserved_bytes: f(
                self.grass_worker_reserved_bytes,
                other.grass_worker_reserved_bytes,
            ),
            active_cpu: f(self.active_cpu, other.active_cpu),
            active_gpu: f(self.active_gpu, other.active_gpu),
            prototype_cpu: f(self.prototype_cpu, other.prototype_cpu),
            prototype_gpu: f(self.prototype_gpu, other.prototype_gpu),
            jobs: f(self.jobs, other.jobs),
            pending: f(self.pending, other.pending),
            staging_bytes: f(self.staging_bytes, other.staging_bytes),
            dependency_staging_bytes: f(
                self.dependency_staging_bytes,
                other.dependency_staging_bytes,
            ),
            objects: f(self.objects, other.objects),
        }
    }
}

/// The renderer use of one frame.
#[derive(Clone, Copy, Debug, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct PresentationReport {
    pub used: PresentationAllocation,
    /// Totals the renderer wanted to reach this frame.
    pub want: PresentationAllocation,
    /// Credit the renderer gave back. It no longer spends it.
    pub release: PresentationAllocation,
}
impl PresentationReport {
    /// Adds a later report: its use and wants, and both releases.
    pub fn merge(self, later: Self) -> Self {
        Self {
            used: later.used,
            want: later.want,
            release: self.release.zip(later.release, usize::saturating_add),
        }
    }
}

/// Renderer fields that own stream work waited for this step. The stream grants none of them,
/// and the renderer gives back their unused credit.
#[derive(Clone, Copy, Debug, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Reclaim {
    pub jobs: bool,
    pub pending: bool,
    /// Staging and dependency staging.
    pub staging: bool,
    pub prototype_cpu: bool,
}
impl Reclaim {
    const JOBS: u8 = 1;
    const PENDING: u8 = 2;
    const STAGING: u8 = 4;
    const PROTOTYPE_CPU: u8 = 8;
    fn from_bits(bits: u8) -> Self {
        Self {
            jobs: bits & Self::JOBS != 0,
            pending: bits & Self::PENDING != 0,
            staging: bits & Self::STAGING != 0,
            prototype_cpu: bits & Self::PROTOTYPE_CPU != 0,
        }
    }
    /// `allocation` without the fields own work waited for.
    fn keep(self, mut allocation: PresentationAllocation) -> PresentationAllocation {
        if self.jobs {
            allocation.jobs = 0;
        }
        if self.pending {
            allocation.pending = 0;
        }
        if self.staging {
            allocation.staging_bytes = 0;
            allocation.dependency_staging_bytes = 0;
        }
        if self.prototype_cpu {
            allocation.prototype_cpu = 0;
        }
        allocation
    }
    /// Only the fields of `allocation` that own work waited for.
    pub fn take(self, allocation: PresentationAllocation) -> PresentationAllocation {
        allocation.zip(self.keep(allocation), usize::saturating_sub)
    }
}

/// The stream answer: added credit, and the fields own work waits for.
#[derive(Clone, Copy, Debug, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct PresentationGrant {
    pub credit: PresentationAllocation,
    pub reclaim: Reclaim,
    /// The stream lends its publication slot of this frame.
    pub publication: bool,
}
impl PresentationGrant {
    /// Adds a later grant: both credits. The later step decides reclaim and the lend.
    pub fn merge(self, later: Self) -> Self {
        Self {
            credit: self.credit.zip(later.credit, usize::saturating_add),
            reclaim: later.reclaim,
            publication: later.publication,
        }
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct SurfaceKey {
    pub layer: WorldLayer,
    pub unit: [i64; 2],
}
pub struct PublishedSurface {
    pub key: SurfaceKey,
    pub surface: Arc<WorldSurface>,
    pub prototype: Option<SourceId>,
    pub bounds: WorldBounds,
    collider: Option<ColliderHandle>,
    collision_bytes: usize,
    road: Option<RoadSegment>,
}
pub struct WorldFrame<'a> {
    pub request: WorldRequest,
    pub origin: WorldOrigin,
    pub surfaces: &'a [PublishedSurface],
    pub lights: &'a [super::WorldLight],
    pub structures: &'a [super::StructureInstance],
    pub supplies: &'a [PublishedSupply],
    pub startup: Option<&'a StartupTerrain>,
    pub failure: Option<&'a str>,
}
pub struct PublishedSupply {
    pub key: super::GeneratedId,
    pub point: super::SupplyPoint,
    pub definition: super::SupplyDefinition,
    pub items: Option<(crate::reference::ItemId, crate::reference::ItemId)>,
    baselines: Option<[super::ObjectSnapshot; 2]>,
}
#[derive(Clone, Copy)]
struct PendingRecovery {
    request: WorldRequest,
    target: AbsolutePose,
    supply: Option<(u32, usize)>,
    candidate: usize,
    best: Option<(f64, AbsolutePose)>,
}
pub struct RecoveryPermit {
    request: WorldRequest,
    player: [f64; 3],
    vehicle: [f64; 3],
}
struct PreparedSurface {
    road_claim: Option<(RoadJobId, u64)>,
    pending_bytes: usize,
    renderer_ready: bool,
    /// The renderer may see the surface. Its render staging and memory are charged.
    offered: bool,
    renderer_reserved: bool,
    request: WorldRequest,
    published: PublishedSurface,
}
struct SelectedRoad {
    segment: RoadSegment,
    light: Option<super::WorldLight>,
    /// Pole rope poses toward the next segment pole.
    cable: Vec<super::NodeAppearance>,
}
struct RoadWork {
    pending_bytes: usize,
    surface_bytes: usize,
    request: WorldRequest,
    identity: RoadJobId,
    segment: RoadSegment,
    source_revision: u64,
    completed: bool,
    canceled: bool,
}

/// Source world publication and session ownership.
pub struct WorldStream {
    required: super::required::RequiredSelection,
    static_controls: Vec<super::contents::ContentInstance>,
    // Membership and order only, independent of live control state.
    static_control_revision: u64,
    binding_baseline: Option<usize>,
    binding_bytes: usize,
    binding_owner_bytes: usize,
    binding_objects: usize,
    binding_reserved: usize,
    lamp_bindings: Vec<content_lamps::LampBinding>,
    dial_bindings: Vec<content_dials::DialBinding>,
    pub small_items: content_small_items::SmallItems,
    tools: content_tools::Tools,
    pub computers: content_computers::Computers,
    cobwebs: content_cobwebs::Cobwebs,
    /// Changed world conditions of this frame and the looks of one condition. Reused buffers.
    condition_changes: Vec<crate::reference::ConditionId>,
    condition_looks: Vec<super::NodeAppearance>,
    /// Session and active revision whose dial items are all bound.
    dial_scan: Option<(u64, u64)>,
    level_bindings: Vec<content_levels::LevelBinding>,
    /// Session and active revision whose spirit levels are all bound.
    level_scan: Option<(u64, u64)>,
    radio_bindings: Vec<content_radios::RadioBinding>,
    /// Session and active revision whose radio items are all bound.
    radio_scan: Option<(u64, u64)>,
    /// Prototype radioscript lookups.
    radio_scripts: Vec<(SourceId, Option<SourceId>)>,
    music_box_bindings: Vec<content_musicboxes::MusicBoxBinding>,
    /// Session and active revision whose music box items are all bound.
    music_box_scan: Option<(u64, u64)>,
    seat_bindings: Vec<content_seats::SeatBinding>,
    /// Session and active revision whose seat items are all bound.
    seat_scan: Option<(u64, u64)>,
    /// Seats of world prototypes, read and checked on world load.
    seat_prototypes: Vec<content_seats::PrototypeSeats>,
    sled_bindings: Vec<content_sleds::SledBinding>,
    /// Session and active revision whose sled items are all bound.
    sled_scan: Option<(u64, u64)>,
    /// Indices of world prototypes with a sled.
    sled_prototypes: Vec<usize>,
    trailer_bindings: Vec<content_trailers::TrailerBinding>,
    /// Session and active revision whose trailers and hitch targets are all bound.
    trailer_scan: Option<(u64, u64)>,
    /// Contents whose hitch targets are registered, with their item.
    hitch_owners: Vec<(crate::world::GeneratedId, crate::reference::ItemId)>,
    /// Indices of world prototypes with a trailer or hitch targets.
    trailer_prototypes: Vec<usize>,
    follower_bindings: Vec<content_followers::FollowerBinding>,
    /// Session and active revision whose followers are all bound.
    follower_scan: Option<(u64, u64)>,
    /// Indices of world prototypes with followers.
    follower_prototypes: Vec<usize>,
    detector_bindings: Vec<content_detectors::DetectorBinding>,
    /// Session and active revision whose metal detectors are all bound.
    detector_scan: Option<(u64, u64)>,
    bus_door_bindings: Vec<content_busdoors::BusDoorBinding>,
    /// Session and active revision whose bus doors are all bound.
    bus_door_scan: Option<(u64, u64)>,
    /// `busdoorscripts` and `dinamiccolliderforbus` sources by prototype.
    bus_door_scripts: Vec<(SourceId, Option<SourceId>, Vec<SourceId>)>,
    // Membership and order only, independent of button animation.
    lamp_binding_revision: u64,
    seed: i32,
    /// Session date as year, month, day. It decides date rules for the whole game.
    date: [i32; 3],
    mode: super::GameMode,
    /// The picked start car. Only its start placement spawns.
    start_car: super::StartCar,
    package_id: String,
    generation_revision: u64,
    structure_helpers: super::structure_helpers::StructureHelpers,
    building_components: super::building_extras::BuildingComponents,
    reference: Arc<WorldReference>,
    pub distances: super::DrawDistances,
    limits: super::stream_limits::StreamLimits,
    presentation: PresentationAllocation,
    dependency_staging_reserve: usize,
    dependency_metadata: usize,
    /// Fields that own work waited for this step, as `Reclaim` bits.
    reclaim: std::sync::atomic::AtomicU8,
    changes: super::ChangeRecords,
    replay_roots: Vec<Placement>,
    replay_indices: Vec<super::changes::ChangeRecordIndex>,
    replay_query: Vec<super::changes::ChangeRecordIndex>,
    change_revision: u64,
    change_bounds: Option<[WorldBounds; 2]>,
    imported_baselines: Vec<(crate::reference::ItemId, super::ObjectSnapshot)>,
    road_light: super::source_lights::SourceRoadLight,
    cables: super::cables::CableRecipes,
    /// Road building cables, one per wanted building root. Empty: no cable.
    building_cables: Vec<CableEntry>,
    cable_search: super::cables::BuildingCableSearch,
    home_cable: super::cables::PoleSearch,
    lights: Vec<super::WorldLight>,
    wanted_lights: Vec<u32>,
    pub origin: WorldOrigin,
    request: WorldRequest,
    /// Chassis and absolute position of the start car and the last seated car, as source important objects.
    cars: Vec<(rapier3d::prelude::RigidBodyHandle, [f64; 3])>,
    /// Bodies whose contents never retire: the seat owner and the last seated car.
    kept: [Option<rapier3d::prelude::RigidBodyHandle>; 2],
    recovery: Option<PendingRecovery>,
    recovery_search: Option<([f64; 3], bool)>,
    roads: RoadIndex,
    road_prototypes: [Option<road_mesh::RoadPrototype>; 3],
    road_source_revisions: [u64; 3],
    road_worker: RoadWorker,
    road_worker_reserved_bytes: usize,
    road_work: Option<RoadWork>,
    placements: PlacementIndex,
    props: super::prop_layers::PropLayers,
    contents: super::contents::SourceContents,
    boxes: content_boxes::BoxRuntime,
    /// Source breakablescript targets of active content.
    pub breakables: crate::damage::Breakables,
    evaluator: TerrainEvaluator,
    color_foundations: Vec<super::FoundationPoint>,
    color_helpers: Vec<TerrainFoundation>,
    tiles: [TerrainTile; 3],
    terrain_layer: u8,
    road_layers: [u8; 3],
    initial_generation_ready: bool,
    plan_revision: u64,
    selection_inputs: u64,
    road_focus: [[f64; 3]; 2],
    /// Player and vehicle positions at the last structure refresh.
    structure_focus: [[f64; 3]; 2],
    planned_road_focus: [[f64; 3]; 2],
    terrain_context_dirty: bool,
    selection_changes: u64,
    road_candidates: Vec<u32>,
    next_road: usize,
    selected_roads: Vec<SelectedRoad>,
    next_geometry: usize,
    wanted_roads: Vec<u32>,
    wanted_tiles: Vec<SurfaceKey>,
    road_points: Vec<TerrainRoadPoint>,
    structures: Vec<super::StructureInstance>,
    trigger_roles: super::TriggerRoles,
    structure_revision: u64,
    presentation_changes: Vec<content_index::ContentSlot>,
    presentation_lamp_changes: Vec<usize>,
    saved_item_protection: Vec<super::ObjectKey>,
    saved_locations_pending: bool,
    recipes: HashMap<SourceId, super::StructureRecipe>,
    supplies: Vec<PublishedSupply>,
    wanted_structures: pending_structures::PendingStructures,
    ready_structures: Vec<super::GeneratedId>,
    startup: Option<StartupTerrain>,
    /// Fixed prototype placements of the startup list, set with `startup`.
    startup_placements: Vec<Placement>,
    /// StartSpawnItems world prototype copies (spray bottles, binoculars): prototype and Transform.
    startup_items: Vec<(crate::reference::SourceId, crate::reference::SourceId)>,
    startup_candidates: Vec<u32>,
    startup_next: usize,
    startup_points: Vec<TerrainRoadPoint>,
    surfaces: Vec<PublishedSurface>,
    ready: Option<PreparedSurface>,
    terrain_execution: TerrainExecution,
    /// Prototypes the renderer holds. True when bound. Their bytes are renderer credit.
    prototype_holds: HashMap<SourceId, bool>,
    terrain_ready: [bool; 3],
    shared_cpu_bytes: Option<usize>,
    publication_available: bool,
    /// The renderer holds the publication slot of this frame.
    publication_lent: bool,
    /// Renderer want of the last report above its covered use.
    presentation_want: PresentationAllocation,
    /// Renderer use of the last report above its credit. The grant credits it back.
    presentation_cover: PresentationAllocation,
    /// Renderer use of the last report.
    presentation_used: PresentationAllocation,
    /// A building lookup waits for prototype CPU room. Its ramps are not appended yet.
    ramps_wait: bool,
    failure: Option<String>,
    last_road_work: usize,
}

impl WorldStream {
    pub fn capture_save(
        &self,
        reference: &ReferenceManifest,
        items: &crate::interaction::items::ItemStore,
        interaction: &crate::interaction::InteractionState,
        physics: &PhysicsWorld,
        required: &[super::ObjectKey],
    ) -> crate::save::WorldSnapshot {
        let mut changes = self.changes.generated_records();
        for (key, item) in items.generated_bindings() {
            let object = super::ObjectKey::Generated(key.clone());
            let owner = self
                .contents
                .active
                .iter()
                .chain(&self.static_controls)
                .find(|c| c.item == Some(item.id) || c.bindings.iter().any(|b| b.item == item.id));
            let supply = self
                .supplies
                .iter()
                .find(|s| s.items.is_some_and(|(a, b)| a == item.id || b == item.id));
            let baseline = self
                .generated_baselines()
                .find(|(id, _)| *id == item.id)
                .map(|(_, s)| s)
                .or_else(|| {
                    supply.and_then(|s| {
                        s.baselines.as_ref().map(|b| {
                            if s.items.unwrap().0 == item.id {
                                &b[0]
                            } else {
                                &b[1]
                            }
                        })
                    })
                });
            if let (Some(current), Some(baseline)) = (
                super::changes::capture(item.id, items, interaction, physics, reference),
                baseline,
            ) {
                changes.record(object.clone(), current.clone(), baseline);
                if item.removed {
                    changes.remove(object.clone());
                }
                if let Some(owner) = owner {
                    changes.set_replay(&object, &owner.origin, &owner.content.placement.id);
                } else if let Some(supply) = supply {
                    if let Some(instance) = self
                        .structures
                        .iter()
                        .find(|s| s.placement.id == supply.key)
                    {
                        changes.set_replay(&object, &instance.placement, &supply.key);
                    }
                }
            }
        }
        let mut dependencies: Vec<_> = changes
            .iter()
            .filter_map(|(_, c)| {
                c.snapshot
                    .as_ref()
                    .and_then(|s| s.attachment.as_ref())
                    .and_then(|a| a.owner.clone())
            })
            .collect();
        dependencies.extend_from_slice(required);
        for item in items
            .iter()
            .filter(|i| matches!(items.object_key(i.id), super::ObjectKey::Imported(_)))
        {
            if let Some(snapshot) =
                super::changes::capture(item.id, items, interaction, physics, reference)
            {
                if let Some(owner) = snapshot.attachment.and_then(|a| a.owner) {
                    dependencies.push(owner);
                }
            }
        }
        while let Some(key) = dependencies.pop() {
            if !matches!(key, super::ObjectKey::Generated(_)) || changes.get(&key).is_some() {
                continue;
            }
            let Some(id) = items.resolve_key(&key) else {
                continue;
            };
            let Some(snapshot) =
                super::changes::capture(id, items, interaction, physics, reference)
            else {
                continue;
            };
            if let Some(owner) = snapshot.attachment.as_ref().and_then(|a| a.owner.clone()) {
                dependencies.push(owner);
            }
            changes.retain_dependency(key.clone(), snapshot);
            if let Some(owner) = self
                .contents
                .active
                .iter()
                .chain(&self.static_controls)
                .find(|c| c.item == Some(id) || c.bindings.iter().any(|b| b.item == id))
            {
                changes.set_replay(&key, &owner.origin, &owner.content.placement.id);
            }
        }
        let boxes = self
            .contents
            .boxes
            .released()
            .iter()
            .filter(|r| {
                changes
                    .get(super::ObjectKey::Generated(r.id.clone()))
                    .is_some_and(|c| c.snapshot.is_none())
            })
            .cloned()
            .collect();
        crate::save::WorldSnapshot {
            boxes,
            generation: super::GenerationSnapshot {
                generator_contract: super::GENERATOR_CONTRACT,
                package_id: self.package_id.clone(),
                roads: self.roads.capture_index(),
                placements: self.placements.capture_index(),
            },
            seed: self.seed,
            date: self.date,
            start_car: self.start_car,
            changes,
            spawn: self.contents.state.capture_history(),
        }
    }
    pub fn coverage(&self) -> super::GenerationCoverage {
        super::GenerationCoverage {
            road_end: self.roads.coverage(),
            building_end: self.placements.coverage(),
            revision: self.generation_revision,
        }
    }
    pub fn limits(&self) -> super::StreamLimits {
        self.limits
    }
    /// Largest draw distance whose limits fit this world.
    pub fn largest_fitting(&self) -> Result<f32, ReferenceError> {
        super::StreamLimits::largest_fitting(&self.reference, self.road_light.influence_margin())
    }
    /// Selected counts and active bytes, in the shape of the limits.
    pub fn selected(&self) -> super::StreamLimits {
        let metrics = self.metrics();
        super::StreamLimits {
            roads: self.selected_roads.len(),
            tiles: std::array::from_fn(|tier| {
                self.wanted_tiles
                    .iter()
                    .filter(|k| k.layer == tier_layer(tier))
                    .count()
            }),
            objects: metrics.generated_objects,
            active_cpu: metrics.active_cpu_bytes,
            active_gpu: metrics.active_gpu_bytes,
            terrain_job: 0,
        }
    }
    /// The road and terrain selection of the current request is complete.
    pub fn selection_complete(&self) -> bool {
        self.failure.is_none()
            && self.startup.is_some()
            && self.plan_revision == self.request.revision
            && self.next_road == self.road_candidates.len()
    }
    pub fn restore_save(
        &mut self,
        saved: &crate::save::WorldSnapshot,
    ) -> Result<(), ReferenceError> {
        if self.seed != saved.seed
            || self.start_car != saved.start_car
            || self.initial_generation_ready
            || !self.structures.is_empty()
            || self.roads.coverage() != 0
            || self.placements.coverage() != 0
        {
            return Err(ReferenceError(
                "Source save must enter an unprepared matching stream".into(),
            ));
        }
        saved.generation.validate_identity(&self.package_id)?;
        self.roads.validate_index(&saved.generation.roads)?;
        self.placements
            .validate_index(&saved.generation.placements)?;
        let history = super::SourceSpawnState::restore_history(saved.spawn.clone())?;
        self.roads.restore_index(&saved.generation.roads)?;
        self.placements
            .restore_index(&saved.generation.placements)?;
        self.generation_revision += 1;
        self.contents.state = history;
        self.changes = saved.changes.clone();
        self.contents.boxes.restore(&saved.boxes);
        self.change_revision = u64::MAX;
        self.change_bounds = None;
        Ok(())
    }
    /// The presentation layer must publish and bind the complete target window before Commit.
    pub fn save_owner_ready(
        &self,
        key: &super::ObjectKey,
        items: &crate::interaction::items::ItemStore,
    ) -> bool {
        let Some(item) = items.resolve_key(key) else {
            return false;
        };
        match key {
            super::ObjectKey::Imported(_) => true,
            super::ObjectKey::Generated(_) => self
                .contents
                .active
                .iter()
                .chain(&self.static_controls)
                .find(|c| c.item == Some(item) || c.bindings.iter().any(|b| b.item == item))
                .map(|c| &c.content.placement.id)
                .or_else(|| {
                    self.supplies
                        .iter()
                        .find(|s| s.items.is_some_and(|(a, b)| a == item || b == item))
                        .map(|s| &s.key)
                })
                .is_some_and(|id| self.bindings_ready(id)),
        }
    }
    pub fn save_link_required(&self, position: [f64; 3]) -> bool {
        [self.request.player, self.request.vehicle]
            .iter()
            .any(|focus| {
                source_distance_squared(*focus, position) < self.distances.item_spawn_dist.powi(2)
            })
    }
    pub fn saved_bindings_ready(&self, items: &crate::interaction::items::ItemStore) -> bool {
        self.required_ready()
            && self.change_bounds.is_some()
            && self.change_revision == self.changes.revision()
            && self.replay_indices.iter().all(|&index| {
                let (key, change) = self.changes.get_indexed(index);
                if matches!(key, super::ObjectKey::Imported(_)) {
                    return true;
                }
                change.snapshot.is_none()
                    || ((items.resolve_key(key).is_some()
                        || self.lamp_bindings.iter().any(|b| {
                            let super::ObjectKey::Generated(id) = key else {
                                return false;
                            };
                            id.layer == b.parent.layer
                                && id.generator == b.parent.generator
                                && id.unit == b.parent.unit
                                && id.ordinal == b.parent.ordinal
                                && id.children.len() == b.parent.children.len() + 1
                                && id.children.starts_with(&b.parent.children)
                                && id.children.last().is_some_and(|(source, ordinal)| {
                                    source == &b.source && *ordinal == 0
                                })
                        }))
                        && change
                            .instance
                            .as_ref()
                            .is_some_and(|id| self.bindings_ready(id)))
            })
    }
    /// Saved inventory and imported attachment links protect their generated owners until binding.
    pub fn protect_saved_items(&mut self, keys: &[super::ObjectKey]) {
        for key in keys {
            if !self.saved_item_protection.contains(key) {
                self.saved_item_protection.push(key.clone());
            }
        }
    }
    pub fn hold_saved_item_locations(&mut self) {
        self.saved_locations_pending = true;
    }
    pub fn release_saved_item_locations(
        &mut self,
        items: &crate::interaction::items::ItemStore,
        pending: &[crate::save::ItemSnapshot],
    ) {
        self.saved_item_protection.retain(|key| {
            items.resolve_key(key).is_none() || pending.iter().any(|item| {
                matches!(&item.location, crate::save::ItemLocationSave::GeneratedAttachment { attachment } if attachment.owner.as_ref() == Some(key))
            })
        });
        self.saved_locations_pending = !self.saved_item_protection.is_empty();
    }
    fn fixed_decoration(&self, placement: &Placement) -> bool {
        if !super::instances::fixed_decoration_source(&placement.prototype) {
            return false;
        }
        if let Some(instance) = self
            .structures
            .iter()
            .find(|s| s.placement.id == placement.id)
        {
            return instance.fixed_decoration;
        }
        let owns = |key: &super::ObjectKey| {
            matches!(key, super::ObjectKey::Generated(id) if id == &placement.id)
                || self
                    .changes
                    .get(key)
                    .is_some_and(|c| c.instance.as_ref() == Some(&placement.id))
        };
        !self.saved_item_protection.iter().any(owns)
            && !self.changes.iter().any(|(key, change)| {
                change.snapshot.as_ref().is_some_and(|snapshot| {
                    ((snapshot.held || snapshot.attachment.is_some())
                        && (owns(key) || change.instance.as_ref() == Some(&placement.id)))
                        || snapshot
                            .attachment
                            .as_ref()
                            .and_then(|a| a.owner.as_ref())
                            .is_some_and(owns)
                })
            })
    }
    fn forget_fixed_decoration_changes(&mut self, id: &super::GeneratedId) {
        loop {
            let obsolete = self.changes.iter().find_map(|(key, change)| {
                (change.instance.as_ref() == Some(id)
                    || matches!(key, super::ObjectKey::Generated(key) if key == id))
                .then(|| key.clone())
            });
            let Some(key) = obsolete else {
                break;
            };
            self.changes.forget(&key);
        }
    }
    fn discard_fixed_decoration_changes(&mut self) {
        for index in 0..self.contents.active.len() {
            let content = &self.contents.active[index];
            if content.restored
                || content.item.is_some()
                || !self.fixed_decoration(&content.content.placement)
            {
                continue;
            }
            let id = content.content.placement.id.clone();
            self.forget_fixed_decoration_changes(&id);
            // This instance has no mutable state or bindings to reconstruct.
            self.contents.active[index].restored = true;
        }
        // Deletion compacts records. Refresh indices before any replay consumer runs.
        if self.change_revision != self.changes.revision() {
            self.select_changed_destinations();
        }
    }
    pub fn seed(&self) -> i32 {
        self.seed
    }
    pub fn focus(&self) -> [f64; 3] {
        self.request.player
    }

    /// Spawn context of later source spawns. Spawned content keeps its own context.
    pub fn set_peaceful_enemies(&mut self, value: i32) {
        self.contents.state.context.peaceful_enemies = value;
    }
    pub fn mode(&self) -> super::GameMode {
        self.mode
    }
    pub fn start_car(&self) -> super::StartCar {
        self.start_car
    }

    /// Test stream in the default game mode at the export default draw distance.
    #[cfg(test)]
    pub fn new_default(reference: &ReferenceManifest, seed: i32) -> Result<Self, ReferenceError> {
        let r = &reference.world.draw_distance;
        Self::new(
            reference,
            seed,
            reference.world.spawn_context.date,
            super::GameMode::default(),
            super::DrawDistances::resolve(r, r.default),
        )
    }
    pub fn new(
        reference: &ReferenceManifest,
        seed: i32,
        date: [i32; 3],
        mode: super::GameMode,
        distances: super::DrawDistances,
    ) -> Result<Self, ReferenceError> {
        let world = &reference.world;
        let shared_cpu_bytes = reference.world_retained_cpu_bytes();
        if shared_cpu_bytes > PROTOTYPE_CPU {
            return Err(ReferenceError(format!(
                "Source immutable graph exceeds prototype CPU budget: {shared_cpu_bytes}"
            )));
        }
        let evaluator = super::terrain_evaluator(world, seed);
        let roads = RoadIndex::new(
            RoadSequence::new(world, seed, mode.biome_override())?,
            super::terrain_evaluator(world, seed),
        );
        let terrain_transform = world
            .records
            .iter()
            .find(|record| record.source == world.terrain.prefabs[0])
            .ok_or_else(|| ReferenceError("Missing terrain prefab Transform".into()))?;
        let Some(crate::reference::WorldValue::Link(owner)) =
            terrain_transform.fields.get("m_GameObject")
        else {
            return Err(ReferenceError("Missing terrain prefab owner".into()));
        };
        let object = world
            .records
            .iter()
            .find(|record| &record.source == owner)
            .ok_or_else(|| ReferenceError("Missing terrain prefab GameObject".into()))?;
        let Some(crate::reference::WorldValue::Integer(terrain_layer)) =
            object.fields.get("m_Layer")
        else {
            return Err(ReferenceError("Missing terrain collision layer".into()));
        };
        let mut road_layers = [0; 3];
        for (index, source) in world.road.prototypes.iter().enumerate() {
            let prototype = world
                .prototypes
                .iter()
                .find(|prototype| &prototype.source == source)
                .unwrap();
            let collider = prototype
                .colliders
                .iter()
                .find(|collider| collider.deformed_skin.is_some())
                .ok_or_else(|| ReferenceError("Missing source road skin collider".into()))?;
            if collider.material.is_some() || collider.trigger || !collider.enabled {
                return Err(ReferenceError(
                    "Source road skin requires physical material binding".into(),
                ));
            }
            road_layers[index] = prototype.nodes[collider.node as usize].layer;
        }
        let start_car = mode.start_car.pick(world, &mut super::menu_random());
        let mut contents =
            super::contents::SourceContents::new(world, start_car.current_car(reference)?, date);
        contents.boxes =
            content_boxes::BoxReleases::new(reference.acquisition.r#box.source.clone());
        let structure_helpers =
            super::structure_helpers::StructureHelpers::new(world, &contents.rules)?;
        let seat_prototypes = content_seats::world_seats(world, &contents.rules)?;
        let road_light = super::source_lights::SourceRoadLight::new(reference, seed)?;
        let cables = super::cables::CableRecipes::new(world, seed)?;
        if shared_cpu_bytes
            .saturating_add(road_light.retained_bytes())
            .saturating_add(cables.retained_bytes())
            .saturating_add(contents.rules.retained_bytes())
            .saturating_add(structure_helpers.retained_bytes())
            > PROTOTYPE_CPU
        {
            return Err(ReferenceError(
                "Source rule indices exceed prototype CPU budget".into(),
            ));
        }
        let margin = road_light.influence_margin();
        let limits = super::stream_limits::StreamLimits::new(world, &distances, margin)?;
        if !limits.fits() {
            return Err(ReferenceError(format!(
                "Draw distance {} m does not fit. Largest: {} m",
                distances.draw_distance,
                super::stream_limits::StreamLimits::largest_fitting(world, margin)?
            )));
        }
        Ok(Self {
            required: Default::default(),
            static_controls: Vec::new(),
            static_control_revision: 0,
            binding_baseline: None,
            binding_bytes: 0,
            binding_owner_bytes: 0,
            binding_objects: reference.items.len(),
            binding_reserved: 0,
            lamp_bindings: Vec::new(),
            dial_bindings: Vec::new(),
            small_items: content_small_items::SmallItems::new(reference),
            tools: content_tools::Tools::default(),
            computers: content_computers::Computers::default(),
            cobwebs: content_cobwebs::Cobwebs::new(reference),
            condition_changes: Vec::new(),
            condition_looks: Vec::new(),
            dial_scan: None,
            level_bindings: Vec::new(),
            level_scan: None,
            radio_bindings: Vec::new(),
            radio_scan: None,
            radio_scripts: Vec::new(),
            music_box_bindings: Vec::new(),
            music_box_scan: None,
            seat_bindings: Vec::new(),
            seat_scan: None,
            seat_prototypes,
            sled_bindings: Vec::new(),
            sled_scan: None,
            sled_prototypes: world
                .prototypes
                .iter()
                .enumerate()
                .filter(|(_, p)| p.sled.is_some())
                .map(|(i, _)| i)
                .collect(),
            trailer_bindings: Vec::new(),
            trailer_scan: None,
            hitch_owners: Vec::new(),
            trailer_prototypes: world
                .prototypes
                .iter()
                .enumerate()
                .filter(|(_, p)| p.trailer.is_some() || !p.hitches.is_empty())
                .map(|(i, _)| i)
                .collect(),
            follower_bindings: Vec::new(),
            follower_scan: None,
            follower_prototypes: world
                .prototypes
                .iter()
                .enumerate()
                .filter(|(_, p)| p.followers.is_some())
                .map(|(i, _)| i)
                .collect(),
            detector_bindings: Vec::new(),
            detector_scan: None,
            bus_door_bindings: Vec::new(),
            bus_door_scan: None,
            bus_door_scripts: Vec::new(),
            lamp_binding_revision: 0,
            seed,
            date,
            mode,
            start_car,
            structure_helpers,
            building_components: Default::default(),
            presentation: PresentationAllocation::default(),
            dependency_staging_reserve: 0,
            dependency_metadata: 0,
            reclaim: std::sync::atomic::AtomicU8::new(0),
            terrain_layer: *terrain_layer as u8,
            road_layers,
            reference: Arc::clone(world),
            distances,
            limits,
            package_id: reference.content_base.clone(),
            generation_revision: 0,
            changes: super::ChangeRecords::default(),
            replay_roots: Vec::new(),
            replay_indices: Vec::new(),
            replay_query: Vec::new(),
            change_revision: u64::MAX,
            change_bounds: None,
            imported_baselines: Vec::new(),
            road_light,
            cables,
            building_cables: Vec::new(),
            cable_search: super::cables::BuildingCableSearch::default(),
            home_cable: super::cables::PoleSearch::default(),
            lights: Vec::with_capacity(limits.roads),
            wanted_lights: Vec::with_capacity(limits.roads),
            origin: WorldOrigin::default(),
            request: WorldRequest {
                session: NEXT_SESSION
                    .try_update(Ordering::Relaxed, Ordering::Relaxed, |next| {
                        next.checked_add(1)
                    })
                    .map_err(|_| ReferenceError("World session identity exhausted".into()))?,
                revision: 1,
                player: [0.0; 3],
                vehicle: [0.0; 3],
            },
            cars: Vec::new(),
            kept: [None; 2],
            recovery: None,
            recovery_search: None,
            roads,
            road_prototypes: [const { None }; 3],
            road_source_revisions: [0; 3],
            road_worker: RoadWorker::default(),
            road_worker_reserved_bytes: 0,
            road_work: None,
            placements: PlacementIndex::new(world, seed),
            props: super::prop_layers::PropLayers::new(world, seed, &distances)?,
            contents,
            boxes: Default::default(),
            breakables: Default::default(),
            evaluator,
            color_foundations: Vec::new(),
            color_helpers: Vec::new(),
            tiles: [
                super::terrain_tile(world, &distances, 0)?,
                super::terrain_tile(world, &distances, 1)?,
                super::terrain_tile(world, &distances, 2)?,
            ],
            initial_generation_ready: false,
            plan_revision: 0,
            selection_inputs: 0,
            road_focus: [[f64::INFINITY; 3]; 2],
            structure_focus: [[f64::INFINITY; 3]; 2],
            planned_road_focus: [[f64::INFINITY; 3]; 2],
            terrain_context_dirty: false,
            selection_changes: 0,
            road_candidates: Vec::new(),
            next_road: 0,
            selected_roads: Vec::new(),
            next_geometry: 0,
            wanted_roads: Vec::with_capacity(limits.roads),
            wanted_tiles: Vec::with_capacity(limits.tiles.iter().sum()),
            road_points: Vec::with_capacity(limits.roads * super::ROAD_CONTROLS),
            structures: Vec::new(),
            trigger_roles: super::TriggerRoles::default(),
            structure_revision: 0,
            presentation_changes: Vec::new(),
            presentation_lamp_changes: Vec::new(),
            saved_item_protection: Vec::new(),
            saved_locations_pending: false,
            recipes: HashMap::new(),
            supplies: Vec::new(),
            // Grows with use. Capacity is charged as retained CPU.
            wanted_structures: Default::default(),
            ready_structures: Vec::with_capacity(32),
            startup: None,
            startup_placements: Vec::new(),
            startup_items: {
                let s = &reference.small_items;
                let copies = |prototype: &crate::reference::SourceId, transform| {
                    (prototype.clone(), Clone::clone(transform))
                };
                s.spray_bottle
                    .startup
                    .iter()
                    .map(|v| copies(&s.spray_bottle.prototype, &v.transform))
                    .chain(
                        s.binoculars
                            .startup
                            .iter()
                            .map(|v| copies(&s.binoculars.prototype, &v.transform)),
                    )
                    .collect()
            },
            startup_candidates: Vec::new(),
            startup_next: 0,
            startup_points: Vec::new(),
            surfaces: Vec::new(),
            ready: None,
            terrain_execution: TerrainExecution::default(),
            prototype_holds: HashMap::new(),
            terrain_ready: [false; 3],
            shared_cpu_bytes: Some(shared_cpu_bytes),
            publication_available: false,
            publication_lent: false,
            presentation_want: PresentationAllocation::default(),
            presentation_used: PresentationAllocation::default(),
            ramps_wait: false,
            presentation_cover: PresentationAllocation::default(),
            failure: None,
            last_road_work: 0,
        })
    }

    pub(crate) fn set_presentation_allocation(
        &mut self,
        next: PresentationAllocation,
    ) -> Result<(), ReferenceError> {
        for budget in self.presentation_budgets(next) {
            if budget.used > budget.limit {
                return Err(ReferenceError(format!(
                    "{} budget exceeded: requested {} {}, limit {} {}.",
                    budget.name, budget.used, budget.unit, budget.limit, budget.unit,
                )));
            }
        }
        self.presentation = next;
        Ok(())
    }

    /// Takes the renderer report before own work: given-back credit returns and use is
    /// covered. Use above the credit must fit the limits, or the stream fails as before.
    pub fn take_presentation_report(&mut self, report: PresentationReport) {
        let credit = self.presentation.zip(report.release, usize::saturating_sub);
        let mut covered = credit.zip(report.used, usize::max);
        // Dependency staging is part of the staging credit. Per-field give-backs can leave
        // more dependency credit than staging credit.
        covered.dependency_staging_bytes = covered
            .dependency_staging_bytes
            .min(covered.staging_bytes)
            .max(report.used.dependency_staging_bytes);
        // With no renderer-ready work to publish, the renderer takes this frame's publication slot.
        // A prepared surface without its renderer receipt cannot publish. The renderer work that
        // finishes the receipt can itself wait for the slot.
        self.publication_lent = self.failure.is_none()
            && self.ready.as_ref().is_none_or(|work| !work.renderer_ready)
            && self.ready_structures.is_empty();
        if let Err(error) = self.set_presentation_allocation(covered) {
            self.presentation = credit;
            self.failure.get_or_insert(format!(
                "{} Renderer asset CPU: use {} bytes, credit {} bytes.",
                error.0, report.used.prototype_cpu, credit.prototype_cpu,
            ));
        }
        self.presentation_cover = self.presentation_cover.zip(
            covered.zip(credit, usize::saturating_sub),
            usize::saturating_add,
        );
        self.presentation_want = report.want.zip(covered, usize::saturating_sub);
        self.presentation_used = report.used;
    }
    /// Answers the last report after own work. The renderer gets the credit for its covered
    /// use, and the room that own work left, but no field that own work waited for.
    pub fn grant_presentation(&mut self) -> PresentationGrant {
        let reclaim =
            Reclaim::from_bits(self.reclaim.swap(0, std::sync::atomic::Ordering::Relaxed));
        let want = reclaim.keep(std::mem::take(&mut self.presentation_want));
        if self.failure.is_some() {
            return PresentationGrant {
                reclaim,
                ..Default::default()
            };
        }
        let covered = self.presentation;
        let mut target = covered.zip(want.zip(self.presentation_room(), usize::min), |a, b| {
            a.saturating_add(b)
        });
        // Dependency staging is part of the staging credit.
        target.dependency_staging_bytes = target
            .dependency_staging_bytes
            .min(target.staging_bytes)
            .max(covered.dependency_staging_bytes);
        if self.set_presentation_allocation(target).is_err() {
            target = covered;
            self.presentation = covered;
        }
        let cover = std::mem::take(&mut self.presentation_cover);
        PresentationGrant {
            credit: target
                .zip(covered, usize::saturating_sub)
                .zip(cover, usize::saturating_add),
            reclaim,
            publication: self.publication_lent,
        }
    }
    /// Free room of each renderer field next to the current credit.
    fn presentation_room(&self) -> PresentationAllocation {
        let budgets = self.presentation_budgets(self.presentation);
        let free = |i: usize| (budgets[i].limit as usize).saturating_sub(budgets[i].used as usize);
        let pending = MAX_PENDING.saturating_sub(self.pending_work()).min(free(5));
        let jobs = if self.initial_generation_ready {
            free(9)
        } else {
            0
        };
        PresentationAllocation {
            grass_worker_wasm_bytes: usize::MAX,
            grass_worker_cache_bytes: usize::MAX,
            grass_worker_reserved_bytes: usize::MAX,
            active_cpu: free(0),
            active_gpu: free(1),
            prototype_cpu: free(2),
            prototype_gpu: free(3),
            objects: free(4),
            pending,
            staging_bytes: free(6).min(self.available_dependency_staging_bytes()),
            dependency_staging_bytes: usize::MAX,
            jobs,
        }
    }
    pub fn presentation_budgets(
        &self,
        next: PresentationAllocation,
    ) -> [super::metrics::ResourceBudget; 10] {
        use super::metrics::ResourceBudget;
        let current = self.metrics();
        let old = self.presentation;
        let active_cpu = current
            .active_cpu_bytes
            .saturating_sub(old.active_cpu)
            .saturating_add(next.active_cpu);
        let active_gpu = current
            .active_gpu_bytes
            .saturating_sub(old.active_gpu)
            .saturating_add(next.active_gpu);
        let prototype_cpu = current
            .prototype_cpu_bytes
            .saturating_sub(old.prototype_cpu)
            .saturating_add(next.prototype_cpu);
        let prototype_gpu = current
            .prototype_gpu_bytes
            .saturating_sub(old.prototype_gpu)
            .saturating_add(next.prototype_gpu);
        let objects = current
            .generated_objects
            .saturating_sub(old.objects)
            .saturating_add(next.objects);
        let pending = current
            .pending_work
            .saturating_sub(old.pending)
            .saturating_add(next.pending);
        let staging = next.staging_bytes + self.generation_pending_bytes();
        let jobs = next.jobs
            + usize::from(self.terrain_execution.has_pending())
            + usize::from(self.road_work.is_some())
            + usize::from(self.contents.has_replay());
        [
            ResourceBudget {
                name: super::metrics::BUDGET_LABELS[0].0,
                used: active_cpu,
                limit: self.limits.active_cpu,
                unit: super::metrics::BUDGET_LABELS[0].1,
            },
            ResourceBudget {
                name: super::metrics::BUDGET_LABELS[1].0,
                used: active_gpu,
                limit: self.limits.active_gpu,
                unit: super::metrics::BUDGET_LABELS[1].1,
            },
            ResourceBudget {
                name: super::metrics::BUDGET_LABELS[2].0,
                used: prototype_cpu,
                limit: PROTOTYPE_CPU,
                unit: super::metrics::BUDGET_LABELS[2].1,
            },
            ResourceBudget {
                name: super::metrics::BUDGET_LABELS[3].0,
                used: prototype_gpu,
                limit: PROTOTYPE_GPU,
                unit: super::metrics::BUDGET_LABELS[3].1,
            },
            ResourceBudget {
                name: super::metrics::BUDGET_LABELS[4].0,
                used: objects,
                limit: self.limits.objects,
                unit: super::metrics::BUDGET_LABELS[4].1,
            },
            ResourceBudget {
                name: super::metrics::BUDGET_LABELS[5].0,
                used: pending,
                limit: 32,
                unit: super::metrics::BUDGET_LABELS[5].1,
            },
            ResourceBudget {
                name: super::metrics::BUDGET_LABELS[6].0,
                used: staging,
                limit: STAGING,
                unit: super::metrics::BUDGET_LABELS[6].1,
            },
            ResourceBudget {
                name: super::metrics::BUDGET_LABELS[7].0,
                used: next.dependency_staging_bytes,
                limit: next.staging_bytes,
                unit: super::metrics::BUDGET_LABELS[7].1,
            },
            ResourceBudget {
                name: super::metrics::BUDGET_LABELS[8].0,
                used: self.generation_pending_bytes() - self.binding_reserved,
                limit: STAGING - self.dependency_staging_reserve,
                unit: super::metrics::BUDGET_LABELS[8].1,
            },
            ResourceBudget {
                name: super::metrics::BUDGET_LABELS[9].0,
                used: jobs,
                limit: 2,
                unit: super::metrics::BUDGET_LABELS[9].1,
            },
        ]
    }
    fn generation_pending_bytes(&self) -> usize {
        self.binding_reserved
            + self.contents.pending_bytes()
            + self.props.pending_bytes()
            + self.terrain_execution.pending_bytes()
            + self.road_work.as_ref().map_or(0, |w| w.pending_bytes)
            + self.ready.as_ref().map_or(0, |w| {
                w.pending_bytes
                    + if w.offered {
                        surface_render_cpu(&w.published.surface)
                    } else {
                        0
                    }
            })
    }
    fn admit_generation(&mut self, bytes: usize) -> bool {
        if bytes > STAGING - self.dependency_staging_reserve
            || self.metrics().active_cpu_bytes.saturating_add(bytes) > self.limits.active_cpu
        {
            self.failure = Some(format!(
                "Source generation budget exceeded: CPU requested {} bytes, limit {} bytes; staging requested {bytes} bytes, limit {} bytes",
                self.metrics().active_cpu_bytes.saturating_add(bytes),
                self.limits.active_cpu,
                STAGING - self.dependency_staging_reserve
            ));
            return false;
        }
        self.wait_for_room(bytes <= self.available_staging_bytes(), Reclaim::STAGING)
    }
    // Retained generation may wait for resources. Keep one checked input window free.
    // Grass and synchronous published bindings drain without further resource loads.
    pub(crate) fn available_staging_bytes(&self) -> usize {
        self.available_dependency_staging_bytes().min(
            (STAGING - self.dependency_staging_reserve)
                .saturating_sub(self.generation_pending_bytes() - self.binding_reserved),
        )
    }
    pub(crate) fn available_dependency_staging_bytes(&self) -> usize {
        STAGING.saturating_sub(self.presentation.staging_bytes + self.generation_pending_bytes())
    }
    pub fn dependency_staging_configured(&self) -> bool {
        self.dependency_staging_reserve != 0
    }
    fn configure_dependency_staging(
        &mut self,
        reserve: usize,
        metadata: usize,
    ) -> Result<(), ReferenceError> {
        if reserve == 0 || reserve >= STAGING {
            return Err(ReferenceError(
                "Invalid dependency staging reservation".into(),
            ));
        }
        if self.dependency_staging_configured() {
            return Ok(());
        }
        self.dependency_staging_reserve = reserve;
        self.dependency_metadata = metadata;
        Ok(())
    }
    /// The pending work parts, for the sandbox: replay, contents, props, ready structures,
    /// prepared surface, terrain, road and renderer.
    pub fn pending_parts(&self) -> [usize; 8] {
        [
            self.replay_roots
                .len()
                .max(usize::from(self.contents.has_replay())),
            self.contents.pending_requests(),
            self.props.pending_requests(),
            self.ready_structures.len(),
            usize::from(self.ready.is_some()),
            usize::from(self.terrain_execution.has_pending()),
            usize::from(self.road_work.is_some()),
            self.presentation.pending,
        ]
    }
    fn pending_work(&self) -> usize {
        self.pending_parts().iter().sum()
    }
    pub(crate) fn can_request_independent(&self, claims: usize) -> bool {
        let fits = self
            .pending_work()
            .checked_add(claims)
            .is_some_and(|count| count <= MAX_PENDING);
        self.wait_for_room(fits, Reclaim::PENDING)
    }
    /// Keeps `field` from the next renderer grant when own work does not fit.
    fn wait_for_room(&self, fits: bool, field: u8) -> bool {
        if !fits {
            self.reclaim
                .fetch_or(field, std::sync::atomic::Ordering::Relaxed);
        }
        fits
    }
    pub(crate) fn available_external_jobs(&self) -> usize {
        if !self.initial_generation_ready {
            return 0;
        }
        let free = 2usize.saturating_sub(
            self.presentation.jobs
                + usize::from(self.terrain_execution.has_pending())
                + usize::from(self.road_work.is_some())
                + usize::from(self.contents.has_replay()),
        );
        self.wait_for_room(free > 0, Reclaim::JOBS);
        free
    }
    /// Where a wanted structure waits on its way to publication.
    pub fn structure_audit(&self, placement: &Placement) -> serde_json::Value {
        let source = &placement.prototype;
        serde_json::json!({
            "contents_initialized": self.contents.initialized(&placement.id),
            "required": self.structure_required(&placement.id),
            "held": self.holds_prototype(source),
            "bound": self.prototype_holds.get(source).is_some_and(|bound| *bound),
            "renderer_ready": self.ready_structures.contains(&placement.id),
            "recipe": self.recipes.contains_key(source),
        })
    }
    /// A sandbox world spawn: a startup placement after the source startup list.
    fn added_placement(&self, placement: &Placement) -> bool {
        let base = (self.reference.startup.len() + self.startup_items.len()) as u32;
        placement.id.layer == WorldLayer::Startup && placement.id.ordinal >= base
    }
    /// The progress of each sandbox world spawn.
    pub fn added_placement_audit(&self) -> Vec<(&Placement, serde_json::Value)> {
        self.startup_placements
            .iter()
            .filter(|p| self.added_placement(p))
            .map(|p| {
                let mut audit = self.structure_audit(p);
                audit["wanted"] = self.wanted_structures.iter().any(|w| w.id == p.id).into();
                audit["published"] = self
                    .structures
                    .iter()
                    .any(|s| s.placement.id == p.id)
                    .into();
                audit["content"] = self.contents.contains(&p.id).into();
                (p, audit)
            })
            .collect()
    }
    pub fn startup_job_audit(&self) -> serde_json::Value {
        serde_json::json!({
            "required": {
                "roads": self.required.roads.len(),
                "terrain": self.required.terrain.len(),
                "structures": self.required.structures.len(),
                "lights": self.required.lights.len(),
                "revision": self.required.revision,
                "structure_ids": self.required.structures,
                "light_ids": self.required.lights,
                "road_ids": self.required.roads,
                "terrain_keys": self.required.terrain.iter().map(|key| format!("{key:?}")).collect::<Vec<_>>(),
                "views_ready": self.required.views_ready,
                "discovered_roads": self.next_road,
                "road_candidates": self.road_candidates.len(),
                "published_roads": self.required.roads.iter().filter(|id| self.surfaces.iter().any(|s| s.key.layer == WorldLayer::Road && s.key.unit[0] == i64::from(**id))).count(),
                "published_terrain": self.required.terrain.iter().filter(|key| self.surfaces.iter().any(|s| s.key == **key)).count(),
                "published_structures": self.required.structures.iter().filter(|id| self.structures.iter().any(|s| s.placement.id == **id) && self.bindings_ready(id)).count(),
            },
            "presentation": self.presentation.jobs,
            "pending": {
                "presentation": self.presentation.pending,
                "structures": self.ready_structures.len(),
                "contents": self.contents.pending_requests(),
                "props": self.props.pending_requests(),
                "surface": self.ready.is_some(),
                "terrain": self.terrain_execution.has_pending(),
                "road": self.road_work.is_some(),
                "replays": self.replay_roots.len(),
            },
            "terrain": self.terrain_execution.has_pending(),
            "contents": self.contents.has_replay(),
            "available": self.available_external_jobs(),
        })
    }
    /// The road prototype index to fill and the stream bytes of a new hold.
    fn hold_cost(&self, source: &SourceId) -> (Option<usize>, usize) {
        let road_index = self
            .reference
            .road
            .prototypes
            .iter()
            .position(|id| id == source)
            .filter(|&index| self.road_prototypes[index].is_none());
        let bytes = renderer_world::hold_cost(
            &self.reference,
            &self.road_prototypes.each_ref().map(Option::is_some),
            super::metrics::map_insert_bytes(&self.prototype_holds),
            source,
        );
        (road_index, bytes)
    }
    pub fn holds_prototype(&self, source: &SourceId) -> bool {
        self.prototype_holds.contains_key(source)
    }
    /// True if the stream holds the prototype or has room for the hold.
    pub fn can_hold_prototype(&self, source: &SourceId) -> bool {
        self.holds_prototype(source)
            || self
                .metrics()
                .prototype_cpu_bytes
                .checked_add(self.hold_cost(source).1)
                .is_some_and(|v| v <= PROTOTYPE_CPU)
    }
    /// Holds a prototype for the renderer. True when the hold is new.
    /// The stream charges only its own table entry and the road prototype data.
    fn hold_prototype(&mut self, source: SourceId) -> Result<bool, ReferenceError> {
        if self.holds_prototype(&source) {
            return Ok(false);
        }
        if !self.can_hold_prototype(&source) {
            return Err(ReferenceError(format!(
                "Source prototype budget {}:{}",
                source.file, source.path_id
            )));
        }
        if let Some(index) = self.hold_cost(&source).0 {
            let prototype = self
                .reference
                .prototypes
                .iter()
                .find(|p| p.source == source)
                .expect("validated road prototype");
            self.road_source_revisions[index] = self.road_source_revisions[index]
                .checked_add(1)
                .ok_or_else(|| ReferenceError("Road source revision exhausted".into()))?;
            self.road_prototypes[index] = Some(super::road::road_prototype(prototype));
        }
        self.prototype_holds.insert(source, false);
        Ok(true)
    }
    /// Renderer credit: the renderer share of the budgets.
    pub fn presentation_credit(&self) -> PresentationAllocation {
        self.presentation
    }
    /// Renderer use of the last report.
    pub fn presentation_used(&self) -> PresentationAllocation {
        self.presentation_used
    }
    fn bind_prototype(&mut self, source: &SourceId) -> Result<(), ReferenceError> {
        let bound = self.prototype_holds.get_mut(source).ok_or_else(|| {
            ReferenceError(format!(
                "Unheld source binding {}:{}",
                source.file, source.path_id
            ))
        })?;
        *bound = true;
        for (tier, material) in self.reference.terrain.materials.iter().enumerate() {
            if &material.source == source {
                self.terrain_ready[tier] = true;
            }
        }
        Ok(())
    }
    fn release_prototype(&mut self, source: &SourceId) -> Result<(), ReferenceError> {
        if self
            .surfaces
            .iter()
            .any(|s| s.prototype.as_ref() == Some(source))
            || self
                .structures
                .iter()
                .any(|s| &s.placement.prototype == source)
        {
            return Err(ReferenceError(
                "Cannot release an active source prototype".into(),
            ));
        }
        for (tier, material) in self.reference.terrain.materials.iter().enumerate() {
            if &material.source == source {
                self.terrain_ready[tier] = false;
            }
        }
        if let Some(index) = self
            .reference
            .road
            .prototypes
            .iter()
            .position(|id| id == source)
        {
            if self
                .ready
                .as_ref()
                .is_some_and(|w| w.published.prototype.as_ref() == Some(source))
            {
                if self.ready.as_ref().unwrap().renderer_reserved {
                    return Err(ReferenceError(
                        "Cannot release a reserved road prototype".into(),
                    ));
                }
                self.discard_unreserved_surface();
            }
            // Terminate only this world's actor before releasing any source or job charge.
            self.road_worker.dispose();
            self.road_worker_reserved_bytes = 0;
            if let Some(work) = self.road_work.take() {
                self.rewind_road(SurfaceKey {
                    layer: WorldLayer::Road,
                    unit: [i64::from(work.segment.index), 0],
                });
            }
            self.road_prototypes[index] = None;
        }
        self.prototype_holds.remove(source);
        self.recipes.remove(source);
        Ok(())
    }
    /// Sets the start car and last seated car chassis and position for item freeze and removal.
    pub fn request_cars(
        &mut self,
        cars: impl IntoIterator<Item = (rapier3d::prelude::RigidBodyHandle, [f64; 3])>,
    ) {
        self.cars.clear();
        self.cars.extend(cars);
    }
    /// Sets the seat owner body and the last seated car chassis. Their contents never retire.
    pub fn request_kept(&mut self, kept: [Option<rapier3d::prelude::RigidBodyHandle>; 2]) {
        self.kept = kept;
    }
    pub fn request_at(&mut self, player: [f64; 3], vehicle: [f64; 3]) {
        if self.recovery.is_some() {
            return;
        }
        if !player.into_iter().chain(vehicle).all(f64::is_finite) {
            self.failure = Some("Nonfinite source world request".into());
            return;
        }
        let cell = |p: [f64; 3], size: u32| {
            [
                (p[0] as f32 / size as f32).round_ties_even() as i64,
                (-p[2] as f32 / size as f32).round_ties_even() as i64,
            ]
        };
        // roadGenScript.controlDist is a squared distance of 10000 (Update 39509).
        let road_changed = [player, vehicle]
            .into_iter()
            .zip(self.road_focus)
            .any(|(a, b)| source_distance_squared(a, b) >= 10000.0);
        if road_changed {
            self.road_focus = [player, vehicle];
        }
        let changed = road_changed
            || self.tiles.iter().any(|tile| {
                cell(player, tile.size) != cell(self.request.player, tile.size)
                    || cell(vehicle, tile.size) != cell(self.request.vehicle, tile.size)
            });
        let before = [self.request.player, self.request.vehicle];
        let after = [player, vehicle];
        let moved = before != after;
        let prop_changed = self.props.focus_changed(before, after);
        let eligibility_changed =
            self.contents
                .eligibility_changed(self.distances.item_spawn_dist, before, after);
        let radius = f64::from(self.distances.item_spawn_dist);
        let selected = |points: [[f64; 3]; 2], position: [f64; 3]| {
            points.into_iter().any(|p| {
                (p[0] - position[0]).abs() <= radius && (p[2] - position[2]).abs() <= radius
            })
        };
        let replay_changed = moved
            && self.changes.iter().any(|(_, change)| {
                change.snapshot.as_ref().is_some_and(|s| {
                    selected(before, s.pose.position) != selected(after, s.pose.position)
                })
            });
        self.request.player = player;
        self.request.vehicle = vehicle;
        let structures_due = prop_changed
            || eligibility_changed
            || after
                .into_iter()
                .zip(self.structure_focus)
                .any(|(a, b)| source_distance_squared(a, b) >= STRUCTURE_REFRESH_DISTANCE.powi(2));
        let buildings_changed =
            structures_due && self.initial_generation_ready && self.refresh_structures();
        self.terrain_context_dirty |= road_changed || buildings_changed;
        if changed || prop_changed || buildings_changed || eligibility_changed || replay_changed {
            self.revise_selection();
        }
    }
    fn revise_selection(&mut self) {
        if let Some(revision) = self.request.revision.checked_add(1) {
            self.request.revision = revision;
            self.failure = None;
        } else {
            self.failure = Some("World request identity exhausted".into());
        }
    }
    fn rewind_road(&mut self, key: SurfaceKey) {
        if key.layer == WorldLayer::Road {
            if let Some(index) = self
                .wanted_roads
                .iter()
                .position(|i| *i == key.unit[0] as u32)
            {
                self.next_geometry = self.next_geometry.min(index);
            }
        }
    }
    fn discard_unreserved_surface(&mut self) {
        if self.ready.as_ref().is_some_and(|w| !w.renderer_reserved) {
            let key = self.ready.take().unwrap().published.key;
            if key.layer == WorldLayer::Road {
                self.road_worker.publication_ended(false);
            }
            self.rewind_road(key);
        }
    }
    /// Advance the authoritative road sequence once per ordinary application Update.
    pub fn advance_generation(&mut self) {
        if !self.initial_generation_ready || self.failure.is_some() {
            return;
        }
        let before = self.roads.coverage();
        self.roads
            .prepare(self.reference.road.gen_in_one_frame as usize);
        if self.roads.coverage() == before {
            return;
        }
        self.generation_revision += 1;
        // Frontier changes outside the selected source window do not invalidate readiness.
        let distance = f64::from(self.distances.road_gen_dist) + self.road_light.influence_margin();
        if let Some(segment) = self.roads.latest_segment() {
            if self
                .road_focus
                .into_iter()
                .any(|point| intersects(segment.bounds, window(point, distance)))
            {
                self.planned_road_focus = [[f64::NAN; 3]; 2];
                self.terrain_context_dirty = true;
                self.revise_selection();
            }
        }
    }
    pub fn initial_generation_ready(&self) -> bool {
        self.initial_generation_ready
    }
    /// Prepare source startup without advancing ordinary Update generation.
    fn prepare_descriptors(&mut self, budget_ms: f64) {
        #[cfg(target_arch = "wasm32")]
        let started = crate::host::game_now();
        #[cfg(not(target_arch = "wasm32"))]
        let started = std::time::Instant::now();
        self.publication_available = !self.publication_lent;
        self.last_road_work = 0;
        // Request and source-input changes force structures. Captured item motion only
        // revises replay selection; it must not restart the one-metre baseline.
        let force_structures = self.plan_revision != self.request.revision
            || self.selection_inputs != self.contents.input_revision();
        if self.selection_inputs != self.contents.input_revision()
            || self.selection_changes != self.changes.revision()
        {
            self.revise_selection();
        }
        if self.failure.is_some() {
            return;
        }
        while !self.initial_generation_ready {
            let slots = 2usize.saturating_sub(self.presentation.jobs);
            let before = (
                self.roads.prepared_segments(),
                self.placements.prepared_buildings(),
            );
            let remaining = self.reference.road.gen_at_start.saturating_sub(before.0);
            let road_work = usize::from(remaining > 0);
            self.roads.prepare(if slots > 0 {
                remaining.min(256) as usize
            } else {
                0
            });
            let road_ready = self.roads.coverage() >= self.reference.road.gen_at_start;
            self.last_road_work += (self.roads.prepared_segments() - before.0) as usize;
            let placement_ready = self
                .placements
                .prepare(if slots > road_work { 256 } else { 0 });
            if before != (self.roads.coverage(), self.placements.coverage()) {
                self.generation_revision += 1;
            }
            self.initial_generation_ready = road_ready && placement_ready;
            #[cfg(target_arch = "wasm32")]
            let elapsed = crate::host::game_now() - started;
            #[cfg(not(target_arch = "wasm32"))]
            let elapsed = started.elapsed().as_secs_f64() * 1000.;
            if !self.initial_generation_ready
                && (elapsed >= budget_ms
                    || before
                        == (
                            self.roads.prepared_segments(),
                            self.placements.prepared_buildings(),
                        ))
            {
                return;
            }
        }
        if self.startup.is_none() {
            if self.available_external_jobs() > 0 {
                self.prepare_startup();
            }
            return;
        }
        if self.plan_revision != self.request.revision {
            if let Err(error) = self.plan(force_structures) {
                self.failure = Some(error.0);
            }
            if self.failure.is_some() || self.plan_revision != self.request.revision {
                return;
            }
        }
        if self.next_road < self.road_candidates.len() {
            self.discover_road();
        }
        // Placement needs the complete selected road-point set.
        if self.next_road == self.road_candidates.len() {
            self.prepare_props();
            self.prepare_contents();
        }
    }
    /// Keep payload construction and renderer publication downstream from source selection.
    fn prepare_geometry(&mut self, should_yield: impl FnMut() -> bool) {
        // Completion releases physical ownership even when dispatch and publication are gated.
        self.poll_road_work();
        if self.failure.is_some()
            || !self.initial_generation_ready
            || self.startup.is_none()
            || self.plan_revision != self.request.revision
            || self.next_road != self.road_candidates.len()
        {
            return;
        }
        self.retarget_road_work();
        if self
            .ready
            .as_ref()
            .is_some_and(|work| !self.road_publication_valid(work))
        {
            self.discard_unreserved_surface();
        }
        if self.ready.is_none() {
            self.prepare_surface(should_yield);
        }
        if let Err(error) = self.offer_surface() {
            self.failure = Some(error.0);
        }
    }
    fn prepare_surface(&mut self, should_yield: impl FnMut() -> bool) {
        if self
            .wanted_tiles
            .iter()
            .any(|key| self.local_collision_pending(*key))
        {
            self.prepare_terrain(should_yield);
        } else if self.required.roads.iter().any(|id| {
            !self
                .surfaces
                .iter()
                .any(|s| s.key.layer == WorldLayer::Road && s.key.unit[0] == i64::from(*id))
        }) || self.required_ready() && self.next_geometry < self.wanted_roads.len()
        {
            self.prepare_road();
        } else {
            self.prepare_terrain(should_yield);
        }
    }
    pub fn prepare(&mut self, budget_ms: f64) {
        #[cfg(target_arch = "wasm32")]
        let started = crate::host::game_now();
        #[cfg(not(target_arch = "wasm32"))]
        let started = std::time::Instant::now();
        // Zero requests one driving step. Bound its terrain work to a 1 ms slice.
        // Loading keeps its supplied budget, including time spent on descriptors.
        let limit_ms = if budget_ms == 0. { 1. } else { budget_ms };
        let mut timer =
            super::stream_profile::Timer::new(super::stream_profile::Stage::StreamDescriptors);
        self.poll_road_work();
        self.prepare_descriptors(budget_ms);
        timer.next(super::stream_profile::Stage::StreamGeometry);
        self.prepare_geometry(|| {
            #[cfg(target_arch = "wasm32")]
            let elapsed = crate::host::game_now() - started;
            #[cfg(not(target_arch = "wasm32"))]
            let elapsed = started.elapsed().as_secs_f64() * 1000.;
            elapsed >= limit_ms
        });
    }
    fn prepare_startup(&mut self) {
        if self.startup_candidates.is_empty() && self.startup_next == 0 {
            let bounds = self
                .reference
                .startup
                .iter()
                .map(|root| window(root.pose.translation.map(f64::from), 150.0))
                .reduce(union);
            if let Some(bounds) = bounds {
                assert_eq!(
                    self.roads.segments_in(
                        bounds,
                        self.roads.coverage(),
                        &mut self.startup_candidates
                    ),
                    super::QueryStatus::Complete
                );
            }
        }
        if let Some(&index) = self.startup_candidates.get(self.startup_next) {
            let segment = match self.roads.prepare_segment(index, 8) {
                Ok(segment) => segment,
                Err(error) => {
                    self.failure = Some(error.0);
                    return;
                }
            };
            self.last_road_work = self.roads.replay_work();
            if let Some(segment) = segment {
                append_road_points(
                    &mut self.startup_points,
                    segment,
                    self.reference.road.def_point_range,
                );
                self.startup_next += 1;
            }
            return;
        }
        match StartupTerrain::new(
            &self.reference,
            &self.distances,
            &self.evaluator,
            &self.startup_points,
        ) {
            Ok(startup) => {
                let count = self.placements.foundation_count();
                let helpers = count + startup.foundations.len();
                let bytes = count * std::mem::size_of::<super::FoundationPoint>()
                    + helpers * std::mem::size_of::<TerrainFoundation>();
                if self.metrics().active_cpu_bytes + bytes > self.limits.active_cpu {
                    self.failure =
                        Some("Source color helper admission exceeds active CPU budget".into());
                    return;
                }
                self.color_foundations = Vec::with_capacity(count);
                self.color_helpers = Vec::with_capacity(helpers);
                match self.startup_placements(&startup) {
                    Ok(placements) => self.startup_placements = placements,
                    Err(error) => self.failure = Some(error.0),
                }
                self.startup = Some(startup);
            }
            Err(error) => self.failure = Some(error.0),
        }
        self.startup_points.clear();
        self.startup_candidates.clear();
    }

    /// The startup entries with a prototype become fixed placements. Of the start cars, only
    /// the picked one spawns. The exported pose is the raw
    /// scene pose; the startup root offsets of its Transform chain move it onto the terrain.
    fn startup_placements(
        &self,
        startup: &StartupTerrain,
    ) -> Result<Vec<Placement>, ReferenceError> {
        let mut placements = Vec::new();
        for (ordinal, entry) in self.reference.startup.iter().enumerate() {
            let Some(prototype) = &entry.prototype else {
                continue;
            };
            if entry.start_car && entry.group != self.start_car.field() {
                continue;
            }
            let offset = startup.chain_offset(&self.reference, &entry.source)?;
            let mut position = entry.pose.translation.map(f64::from);
            position[1] += f64::from(offset);
            let radius = self.contents.rules.radius(prototype).unwrap_or(0.);
            placements.push(Placement {
                id: super::GeneratedId {
                    layer: WorldLayer::Startup,
                    generator: entry.source.clone(),
                    unit: [0, 0],
                    ordinal: ordinal as u32,
                    children: Vec::new(),
                },
                prototype: prototype.clone(),
                pose: super::AbsolutePose {
                    position,
                    rotation: entry.pose.rotation,
                },
                scale: entry.pose.scale,
                bounds: WorldBounds {
                    min: position.map(|v| v - radius),
                    max: position.map(|v| v + radius),
                },
                object_seed: 0,
            });
        }
        // StartSpawnItems copies of world prototypes take their aligned scene pose.
        for (index, (prototype, transform)) in self.startup_items.iter().enumerate() {
            let pose = super::source_world_pose(&self.reference, startup, transform, &[])?;
            let scale = self
                .reference
                .prototypes
                .iter()
                .find(|p| &p.source == prototype)
                .ok_or_else(|| ReferenceError("Startup item prototype missing".into()))?
                .prefab_pose
                .scale;
            let position = pose.position.map(f64::from);
            let radius = self.contents.rules.radius(prototype).unwrap_or(0.);
            placements.push(Placement {
                id: super::GeneratedId {
                    layer: WorldLayer::Startup,
                    generator: transform.clone(),
                    unit: [0, 0],
                    ordinal: (self.reference.startup.len() + index) as u32,
                    children: Vec::new(),
                },
                prototype: prototype.clone(),
                pose: super::AbsolutePose {
                    position,
                    rotation: pose.rotation,
                },
                scale,
                bounds: WorldBounds {
                    min: position.map(|v| v - radius),
                    max: position.map(|v| v + radius),
                },
                object_seed: 0,
            });
        }
        Ok(placements)
    }
    /// Adds a fixed startup placement of world prototype `prototype` at `pose`. The sandbox uses it.
    /// It spawns like the startup list when the player is within the item spawn distance.
    pub fn add_startup_placement(
        &mut self,
        prototype: &SourceId,
        pose: super::AbsolutePose,
    ) -> Result<(), ReferenceError> {
        if self.startup.is_none() {
            return Err(ReferenceError("The startup area is not ready".into()));
        }
        let scale = self
            .reference
            .prototypes
            .iter()
            .find(|p| &p.source == prototype)
            .ok_or_else(|| ReferenceError("Missing startup prototype".into()))?
            .prefab_pose
            .scale;
        let radius = self.contents.rules.radius(prototype).unwrap_or(0.);
        let ordinal = (self.reference.startup.len() + self.startup_items.len()) as u32
            + self
                .startup_placements
                .iter()
                .filter(|p| &p.id.generator == prototype)
                .count() as u32;
        self.startup_placements.push(Placement {
            id: super::GeneratedId {
                layer: WorldLayer::Startup,
                generator: prototype.clone(),
                unit: [0, 0],
                ordinal,
                children: Vec::new(),
            },
            prototype: prototype.clone(),
            pose,
            scale,
            bounds: WorldBounds {
                min: pose.position.map(|v| v - radius),
                max: pose.position.map(|v| v + radius),
            },
            object_seed: 0,
        });
        // The next request refreshes the wanted structures.
        self.structure_focus = [[f64::INFINITY; 3]; 2];
        Ok(())
    }
    /// Fixed startup placements request their contents within the item spawn distance.
    /// A changed placement returns through the changed destination replay instead.
    fn refresh_startup_placements(&mut self) {
        let radius = self.distances.item_spawn_dist;
        let points = [self.request.player, self.request.vehicle];
        let near = |p: &Placement| {
            points
                .into_iter()
                .any(|f| source_distance_squared(f, p.pose.position) < radius * radius)
        };
        let changes = &self.changes;
        let wanted = |p: &Placement| {
            near(p)
                && changes
                    .get(super::ObjectKey::Generated(p.id.clone()))
                    .is_none()
        };
        let active = &self.contents.active;
        self.wanted_structures.retain(|p| {
            p.id.layer != WorldLayer::Startup
                || wanted(p)
                || active.iter().any(|c| c.content.placement.id == p.id)
        });
        for placement in &self.startup_placements {
            if wanted(placement) && !self.wanted_structures.iter().any(|p| p.id == placement.id) {
                self.wanted_structures.push(placement.clone());
            }
        }
    }
    fn refresh_structures(&mut self) -> bool {
        let radius = self.distances.building_gen_dist;
        let points = [self.request.player, self.request.vehicle];
        let bounds = union(
            window(points[0], radius as f64),
            window(points[1], radius as f64),
        );
        let numeric =
            |p: &Placement| p.id.layer == WorldLayer::Building && p.id.children.is_empty();
        let count = self.wanted_structures.iter().filter(|p| numeric(p)).count();
        let removed = self.wanted_structures.iter().any(|p| {
            numeric(p)
                && points
                    .into_iter()
                    .all(|f| source_distance_squared(f, p.pose.position) >= radius * radius)
        });
        let placements = &mut self.placements;
        let status = self.wanted_structures.edit(|list| {
            placements.update_buildings_in(bounds, placements.coverage(), list, |position| {
                points
                    .into_iter()
                    .any(|p| source_distance_squared(p, position) < radius * radius)
            })
        });
        if let Err(error) = status {
            self.failure = Some(error.0);
            return false;
        }
        // Planning also forces refreshes. Movement starts from the last completed refresh.
        self.structure_focus = points;
        for placement in self
            .props
            .placements()
            .iter()
            .chain(self.contents.active.iter().map(|c| &c.content.placement))
        {
            if !self.wanted_structures.iter().any(|p| p.id == placement.id) {
                self.wanted_structures.push(placement.clone());
            }
        }
        self.append_ramps();
        self.refresh_poles();
        self.refresh_startup_placements();
        self.ready_structures
            .retain(|id| self.wanted_structures.iter().any(|p| &p.id == id));
        removed || count != self.wanted_structures.iter().filter(|p| numeric(p)).count()
    }
    /// A building lookup without room waits while the renderer holds unused prototype CPU
    /// credit. The renderer gives it back. Without unused credit the stream fails.
    fn append_ramps(&mut self) {
        let used = if self
            .building_components
            .needs_lookup(&self.wanted_structures)
        {
            self.metrics().prototype_cpu_bytes
        } else {
            PROTOTYPE_CPU
        };
        let room = PROTOTYPE_CPU.saturating_sub(used);
        let (reference, rules, components) = (
            &self.reference,
            &self.contents.rules,
            &mut self.building_components,
        );
        let (seed, maximum) = (self.seed, self.limits.objects);
        match self.wanted_structures.append(|list| {
            super::building_extras::append_ramps(
                reference, rules, components, room, seed, list, maximum,
            )
        }) {
            Ok(done) => self.ramps_wait = !done,
            Err(error) => self.failure = Some(error.0),
        }
        if !self.ramps_wait {
            return;
        }
        if self.presentation.prototype_cpu > self.presentation_used.prototype_cpu {
            self.wait_for_room(false, Reclaim::PROTOTYPE_CPU);
            return;
        }
        self.failure = Some(format!(
            "Source building metadata exceeds prototype CPU budget: used {used} of {PROTOTYPE_CPU} bytes, renderer {} bytes",
            self.presentation.prototype_cpu,
        ));
    }
    fn refresh_poles(&mut self) {
        self.wanted_structures
            .retain(|p| p.id.layer != WorldLayer::Lamp || self.lights.iter().any(|l| l.id == p.id));
        for light in &self.lights {
            if light.biome.is_none() || self.wanted_structures.iter().any(|p| p.id == light.id) {
                continue;
            }
            self.wanted_structures.push(Placement {
                id: light.id.clone(),
                prototype: light.prototype.clone(),
                pose: light.pole,
                scale: light.scale,
                bounds: WorldBounds {
                    min: light.pole.position.map(|v| v - light.range as f64),
                    max: light.pole.position.map(|v| v + light.range as f64),
                },
                object_seed: 0,
            });
        }
    }
    fn prepare_props(&mut self) {
        for _ in 0..4 {
            self.prepare_prop_layer();
            if self.failure.is_some() {
                break;
            }
        }
    }
    fn prepare_prop_layer(&mut self) {
        let budget = self.props.allocation_bound();
        if !self.admit_generation(budget) {
            return;
        }
        let metrics = self.metrics();
        let can_request = metrics.pending_work < 32;
        if self.failure.is_some() {
            return;
        }
        let d = &self.distances;
        let helper_radius = d
            .obj_gen_dist
            .max(d.mountain_gen_dist)
            .max(d.building_gen_dist) as f64;
        self.refresh_helper_context(union(
            window(self.request.player, helper_radius),
            window(self.request.vehicle, helper_radius),
        ));
        if self.failure.is_some() {
            return;
        }
        let context = TerrainContext {
            evaluator: &self.evaluator,
            tile: self.tiles[0],
            roads: &self.road_points,
            road_radius_squared: None,
            foundations: &self.color_helpers,
            holes: &[],
            biome_override: self.mode.biome_override(),
        };
        let current = self.props.placements().len();
        let maximum = self
            .limits
            .objects
            .saturating_sub(metrics.generated_objects)
            .saturating_add(current);
        if let Err(error) = self.props.advance(
            [self.request.player, self.request.vehicle],
            context,
            can_request,
            maximum,
        ) {
            self.failure = Some(error.0);
            return;
        }
        self.wanted_structures.retain(|p| {
            !matches!(
                p.id.layer,
                WorldLayer::Prop
                    | WorldLayer::Mountain
                    | WorldLayer::Tower
                    | WorldLayer::DesertBuilding
            ) || self.props.contains(&p.id)
                || (!p.id.children.is_empty()
                    && self.props.placements().iter().any(|parent| {
                        parent.id.layer == p.id.layer
                            && parent.id.unit == p.id.unit
                            && parent.id.ordinal == p.id.ordinal
                    }))
        });
        for p in self.props.placements() {
            if !self.wanted_structures.iter().any(|w| w.id == p.id) {
                self.wanted_structures.push(p.clone());
            }
        }
        self.append_ramps();
        self.ready_structures
            .retain(|id| self.wanted_structures.iter().any(|p| &p.id == id));
    }
    /// Source `buildingscript.FStart(0)`: a road building connects to the nearest road pole.
    /// Contents wait for the cable. The search replays at most 8 road segments per update.
    fn prepare_building_cables(&mut self) {
        self.building_cables.retain(|(id, _)| {
            self.wanted_structures
                .iter()
                .chain(&self.replay_roots)
                .any(|p| &p.id == id)
        });
        let mut budget = 8;
        loop {
            if self.cable_search.placement().is_none() {
                let Some(p) = self
                    .wanted_structures
                    .iter()
                    .chain(&self.replay_roots)
                    .find(|p| {
                        p.id.layer == WorldLayer::Building
                            && p.id.children.is_empty()
                            && !self.building_cables.iter().any(|(id, _)| id == &p.id)
                    })
                else {
                    return;
                };
                match self.cables.building(&p.prototype) {
                    Some(cable) => self.cable_search.start(&self.roads, cable, p.clone()),
                    None => {
                        let entry = (p.id.clone(), Vec::new());
                        if !self.admit_generation(cable_entry_bytes(&entry)) {
                            return;
                        }
                        self.building_cables.push(entry);
                        continue;
                    }
                }
            }
            let before = budget;
            let mut cable = Vec::new();
            let done = self.cable_search.step(
                &self.cables,
                &self.reference,
                &mut self.roads,
                &mut budget,
                &mut cable,
            );
            self.last_road_work += before - budget;
            match done {
                Ok(Some(id)) => {
                    let entry = (id, cable);
                    if !self.admit_generation(cable_entry_bytes(&entry)) {
                        return;
                    }
                    self.building_cables.push(entry);
                }
                Ok(None) => return,
                Err(error) => {
                    self.failure = Some(error.0);
                    return;
                }
            }
        }
    }
    fn prepare_contents(&mut self) {
        if self.failure.is_some() {
            return;
        }
        self.select_changed_destinations();
        if self.failure.is_some() {
            return;
        }
        self.contents
            .select_revision(self.request.session, self.request.revision);
        self.contents
            .retain_requests(&self.wanted_structures, &self.replay_roots);
        let metrics = self.metrics();
        let pending = 32usize.saturating_sub(metrics.pending_work);
        let limit = 32usize.min(
            self.limits
                .objects
                .saturating_sub(metrics.generated_objects),
        );
        if limit == 0 {
            if !self
                .contents
                .discovery_ready(&self.reference, &self.wanted_structures, &self.replay_roots)
                .unwrap_or(false)
            {
                self.failure = Some(format!(
                    "Source content object budget exceeded: requested {}, limit {}",
                    metrics.generated_objects + 1,
                    self.limits.objects
                ));
            }
            return;
        }
        let reservation = self.available_staging_bytes().min(
            self.limits
                .active_cpu
                .saturating_sub(metrics.active_cpu_bytes),
        );
        if !self.admit_generation(reservation) {
            return;
        }
        self.prepare_building_cables();
        if self.failure.is_some() {
            return;
        }
        let replay_admission = self.contents.has_replay() || self.available_external_jobs() > 0;
        if let Err(error) = self.contents.advance(
            &self.reference,
            self.distances.item_spawn_dist,
            &self.wanted_structures,
            if replay_admission {
                &self.replay_roots
            } else {
                &[]
            },
            &self.building_cables,
            [self.request.player, self.request.vehicle],
            limit,
            pending,
            reservation,
        ) {
            self.failure = Some(error.0);
            return;
        }
        self.discard_fixed_decoration_changes();
        self.wanted_structures.retain(|p| {
            self.changes
                .get(super::ObjectKey::Generated(p.id.clone()))
                .is_none_or(|change| {
                    change.snapshot.is_some()
                        && self.replay_indices.iter().any(|&index| {
                            matches!(self.changes.get_indexed(index).0,
                                super::ObjectKey::Generated(id) if id == &p.id)
                        })
                })
        });
        self.wanted_structures.retain(|p| {
            let unbound = self
                .contents
                .active
                .iter()
                .any(|c| c.content.placement.id == p.id && c.item.is_none());
            !unbound
                || [self.request.player, self.request.vehicle]
                    .into_iter()
                    .any(|f| {
                        source_distance_squared(f, p.pose.position)
                            < self.distances.item_spawn_dist.powi(2)
                    })
                || self.replay_indices.iter().any(|&index| {
                    let (_, c) = self.changes.get_indexed(index);
                    c.instance.as_ref() == Some(&p.id)
                        || c.replay.as_ref().is_some_and(|root| root.id == p.id)
                        || matches!(c.snapshot
                                .as_ref()
                                .and_then(|s| s.attachment.as_ref())
                                .and_then(|a| a.owner.as_ref()),
                                Some(super::ObjectKey::Generated(id)) if id == &p.id)
                })
        });
        let missing = |p: &Placement| !self.wanted_structures.iter().any(|w| w.id == p.id);
        let missing_light = |l: &super::WorldLight| !self.lights.iter().any(|w| w.id == l.id);
        let count = self
            .contents
            .instances()
            .filter(|c| missing(&c.placement))
            .count();
        let light_count = self
            .contents
            .instances()
            .flat_map(|c| &c.lights)
            .filter(|l| missing_light(l))
            .count();
        let selected_bytes = self
            .contents
            .instances()
            .filter(|c| missing(&c.placement))
            .map(|c| super::contents::placement_heap(&c.placement))
            .sum::<usize>()
            + self
                .contents
                .instances()
                .flat_map(|c| &c.lights)
                .filter(|l| missing_light(l))
                .map(super::WorldLight::heap_bytes)
                .sum::<usize>();
        let allocation = selected_bytes
            + if self.wanted_structures.len() + count > self.wanted_structures.capacity() {
                (self.wanted_structures.len() + count + self.wanted_structures.capacity())
                    * std::mem::size_of::<Placement>()
            } else {
                0
            }
            + if self.lights.len() + light_count > self.lights.capacity() {
                (self.lights.len() + light_count + self.lights.capacity())
                    * std::mem::size_of::<super::WorldLight>()
            } else {
                0
            };
        if !self.admit_generation(allocation) {
            return;
        }
        if self.wanted_structures.try_reserve_exact(count).is_err()
            || self.lights.try_reserve_exact(light_count).is_err()
        {
            self.failure = Some("Selected content descriptor allocation failed".into());
            return;
        }
        for content in self.contents.instances() {
            if self.box_content_withheld(&content.placement.id) {
                continue;
            }
            let key = super::ObjectKey::Generated(content.placement.id.clone());
            let selected = self.replay_indices.iter().any(|&index| {
                let (_, c) = self.changes.get_indexed(index);
                c.instance.as_ref() == Some(&content.placement.id)
                    || c.replay
                        .as_ref()
                        .is_some_and(|root| root.id == content.placement.id)
                    || matches!(c.snapshot
                            .as_ref()
                            .and_then(|s| s.attachment.as_ref())
                            .and_then(|a| a.owner.as_ref()),
                            Some(super::ObjectKey::Generated(id)) if id == &content.placement.id)
            });
            let changed = self.changes.get(&key);
            if changed.is_some_and(|c| c.snapshot.is_none() || !selected) {
                continue;
            }
            let baseline_near = [self.request.player, self.request.vehicle]
                .into_iter()
                .any(|p| {
                    source_distance_squared(p, content.placement.pose.position)
                        < self.distances.item_spawn_dist.powi(2)
                });
            if !baseline_near
                && !selected
                && !self
                    .structures
                    .iter()
                    .any(|s| s.placement.id == content.placement.id)
            {
                continue;
            }
            if !self
                .wanted_structures
                .iter()
                .any(|p| p.id == content.placement.id)
            {
                self.wanted_structures.push(content.placement.clone());
            }
        }
        self.lights.retain(|light| {
            light.biome.is_some()
                || self
                    .contents
                    .instances()
                    .filter(|c| {
                        self.wanted_structures
                            .iter()
                            .any(|p| p.id == c.placement.id)
                    })
                    .any(|c| c.lights.iter().any(|l| l.id == light.id))
        });
        for content in self.contents.instances().filter(|c| {
            self.wanted_structures
                .iter()
                .any(|p| p.id == c.placement.id)
        }) {
            for light in &content.lights {
                if let Some(current) = self.lights.iter_mut().find(|l| l.id == light.id) {
                    current.enabled = light.enabled;
                    current.intensity = light.intensity;
                    current.pose = light.pose;
                } else {
                    self.lights.push(light.clone());
                }
            }
        }
        self.contents
            .discard_replay_siblings(&self.wanted_structures);
        self.replay_roots.retain(|root| {
            !self.contents.replay_finished(&root.id)
                || self.replay_indices.iter().any(|&index| {
                    let (_, change) = self.changes.get_indexed(index);
                    change.replay.as_ref().is_some_and(|p| p.id == root.id)
                        && change.instance.as_ref().is_some_and(|id| {
                            !self
                                .contents
                                .active
                                .iter()
                                .chain(&self.static_controls)
                                .any(|c| &c.content.placement.id == id && c.restored)
                        })
                })
        });
    }

    fn select_changed_destinations(&mut self) {
        let radius = f64::from(self.distances.item_spawn_dist);
        let bounds = [self.request.player, self.request.vehicle].map(|focus| WorldBounds {
            min: [focus[0] - radius, f64::NEG_INFINITY, focus[2] - radius],
            max: [focus[0] + radius, f64::INFINITY, focus[2] + radius],
        });
        if self.change_revision != self.changes.revision() || self.change_bounds != Some(bounds) {
            self.replay_indices.clear();
            for bounds in bounds {
                let (count, bytes) = self.changes.query_reservation(bounds);
                if count > self.limits.objects
                    || bytes.saturating_mul(4) > self.available_staging_bytes()
                    || self
                        .metrics()
                        .active_cpu_bytes
                        .saturating_add(bytes.saturating_mul(4))
                        > self.limits.active_cpu
                {
                    self.failure =
                        Some("Changed destination selection exceeds staging admission".into());
                    return;
                }
                self.replay_query.reserve(count);
                self.changes.indices_in(bounds, &mut self.replay_query);
                for index in self.replay_query.drain(..) {
                    if !self.replay_indices.contains(&index) {
                        self.replay_indices.push(index);
                    }
                }
            }
            if self.replay_indices.len() > self.limits.objects {
                self.failure = Some("Changed destination replay exceeds active admission".into());
                return;
            }
            self.change_revision = self.changes.revision();
            self.change_bounds = Some(bounds);
        }
        self.replay_roots.retain(|root| {
            self.replay_indices.iter().any(|&index| {
                self.changes
                    .get_indexed(index)
                    .1
                    .replay
                    .as_ref()
                    .is_some_and(|origin| origin.id == root.id)
            })
        });
        // Selected records are not work requests. Admit one caller at a time and
        // leave capacity for its first phase. Retry admission even with cached bounds.
        if !self.replay_roots.is_empty()
            || self.metrics().pending_work >= 31
            || self.available_external_jobs() == 0
        {
            return;
        }
        for &index in &self.replay_indices {
            let (_, change) = self.changes.get_indexed(index);
            let Some(origin) = &change.replay else {
                continue;
            };
            let active = change.instance.as_ref().is_some_and(|id| {
                self.contents
                    .active
                    .iter()
                    .chain(&self.static_controls)
                    .any(|c| &c.content.placement.id == id)
            });
            if active {
                continue;
            }
            let descriptor_bytes = super::contents::placement_heap(origin)
                + usize::from(self.replay_roots.capacity() == 0) * std::mem::size_of::<Placement>();
            if descriptor_bytes > self.available_staging_bytes()
                || self
                    .metrics()
                    .active_cpu_bytes
                    .saturating_add(descriptor_bytes)
                    > self.limits.active_cpu
            {
                return;
            }
            if self.replay_roots.try_reserve_exact(1).is_err() {
                self.failure = Some("Changed destination descriptor allocation failed".into());
                return;
            }
            self.replay_roots.push(origin.clone());
            break;
        }
    }
    /// Source home `buildingscript.FStart`: the road pole target for home rope 0.
    /// The search starts on the startup roads and replays at most 8 road segments per call.
    pub fn home_cable_target(&mut self, rope0: [f64; 3]) -> (super::QueryStatus, Option<[f64; 3]>) {
        if !self.initial_generation_ready || self.failure.is_some() {
            return (super::QueryStatus::Pending, None);
        }
        if self.home_cable.rope0() != Some(rope0) {
            self.home_cable.start(&self.roads, rope0);
        }
        let mut budget = 8;
        let done =
            self.home_cable
                .step(&self.cables, &self.reference, &mut self.roads, &mut budget);
        self.last_road_work += 8 - budget;
        match done {
            Ok(true) => (super::QueryStatus::Complete, self.home_cable.target()),
            Ok(false) => (super::QueryStatus::Pending, None),
            Err(error) => {
                self.failure = Some(error.0);
                (super::QueryStatus::Pending, None)
            }
        }
    }
    pub fn spawn_boundaries(&self) -> &[super::SpawnBoundary] {
        self.contents.state.boundaries()
    }
    pub fn contents_initialized(&self, id: &super::GeneratedId) -> bool {
        self.contents.initialized(id)
    }
    pub fn appearance(&self, id: &super::GeneratedId) -> &[super::NodeAppearance] {
        if id.layer == WorldLayer::Lamp {
            return self
                .selected_roads
                .iter()
                .find(|r| i64::from(r.segment.index) == id.unit[0])
                .map_or(&[], |r| &r.cable);
        }
        self.contents.appearance(id)
    }
    fn plan(&mut self, force_structures: bool) -> Result<(), ReferenceError> {
        let road_changed = self.planned_road_focus != self.road_focus;
        if road_changed {
            let road_distance =
                self.distances.road_gen_dist as f64 + self.road_light.influence_margin();
            // One union query retains both support windows without copying full index arrays.
            let query = union(
                window(self.road_focus[0], road_distance),
                window(self.road_focus[1], road_distance),
            );
            let (status, candidates) = self.roads.candidate_count(query, self.roads.coverage());
            assert_eq!(status, super::QueryStatus::Complete);
            let bytes = candidates.saturating_add(self.road_candidates.capacity())
                * std::mem::size_of::<u32>();
            if !self.admit_generation(bytes) {
                return Ok(());
            }
            self.road_candidates.clear();
            self.next_road = 0;
            self.next_geometry = 0;
            self.wanted_roads.clear();
            self.wanted_lights.clear();
            self.road_points.clear();
            self.road_candidates
                .try_reserve_exact(candidates)
                .map_err(|_| ReferenceError("Selected road index allocation failed".into()))?;
            assert_eq!(
                self.roads
                    .segments_in(query, self.roads.coverage(), &mut self.road_candidates),
                super::QueryStatus::Complete
            );
            let radius = self.distances.road_gen_dist;
            let margin = self.road_light.influence_margin();
            self.selected_roads.retain(|r| {
                self.road_candidates.contains(&r.segment.index)
                    && (self.road_focus.into_iter().any(|p| {
                        source_distance_squared(p, r.segment.position()) <= radius * radius
                    }) || self.road_focus.into_iter().any(|p| {
                        intersects(r.segment.bounds, window(p, f64::from(radius) + margin))
                    }))
            });
            self.planned_road_focus = self.road_focus;
            if self.wanted_structures.len() > self.limits.objects {
                return Err(ReferenceError(format!(
                    "Source object count exceeds {}",
                    self.limits.objects
                )));
            }
        }
        let buildings_changed = (force_structures || road_changed) && self.refresh_structures();
        self.terrain_context_dirty |= road_changed || buildings_changed;
        self.wanted_tiles.clear();
        if self.wanted_structures.len() > self.limits.objects {
            return Err(ReferenceError(format!(
                "Source object count exceeds {}",
                self.limits.objects
            )));
        }
        for (tier, tile) in self.tiles.iter().enumerate() {
            let radius = tile.draw_distance as i64;
            let side = (radius * 2 + 1).max(4);
            let count = super::stream_limits::circle_tiles(*tile);
            let mut offsets = Vec::with_capacity((side * side) as usize);
            for z in 0..side {
                for x in 0..side {
                    offsets.push([x - radius, z - radius]);
                }
            }
            offsets.sort_by_key(|p| p[0] * p[0] + p[1] * p[1]);
            if count > offsets.len() {
                return Err(ReferenceError("Source terrain coordinate count".into()));
            }
            for point in [self.request.player, self.request.vehicle] {
                let center = [
                    (point[0] as f32 / tile.size as f32).round_ties_even() as i64,
                    (-point[2] as f32 / tile.size as f32).round_ties_even() as i64,
                ];
                for offset in &offsets[..count] {
                    let key = SurfaceKey {
                        layer: tier_layer(tier),
                        unit: [center[0] + offset[0], center[1] + offset[1]],
                    };
                    if !self.wanted_tiles.contains(&key) {
                        self.wanted_tiles.push(key);
                    }
                }
            }
            let active = self
                .wanted_tiles
                .iter()
                .filter(|k| k.layer == tier_layer(tier))
                .count();
            if active > self.limits.tiles[tier] {
                return Err(ReferenceError(format!(
                    "Source terrain tier {tier} count budget"
                )));
            }
        }
        let retain_terrain = !self.terrain_context_dirty
            && self
                .terrain_execution
                .key()
                .is_some_and(|key| self.wanted_tiles.contains(&key));
        self.terrain_execution
            .reconcile(self.request, retain_terrain);
        if let Some(work) = &mut self.ready {
            if !work.renderer_reserved {
                let valid = if work.published.key.layer == WorldLayer::Road {
                    work.published.road.is_some_and(|road| {
                        self.selected_roads.iter().any(|r| r.segment == road)
                            && self.road_focus.into_iter().any(|p| {
                                source_distance_squared(p, road.position())
                                    <= self.distances.road_gen_dist.powi(2)
                            })
                    })
                } else {
                    !self.terrain_context_dirty && self.wanted_tiles.contains(&work.published.key)
                };
                if valid {
                    work.request = self.request;
                } else {
                    self.discard_unreserved_surface();
                }
            }
        }
        self.terrain_context_dirty = false;
        self.selection_changes = self.changes.revision();
        self.plan_revision = self.request.revision;
        self.selection_inputs = self.contents.input_revision();
        Ok(())
    }
    fn discover_road(&mut self) {
        if self.available_external_jobs() == 0 {
            return;
        }
        let index = self.road_candidates[self.next_road];
        let existing = self
            .selected_roads
            .iter()
            .position(|r| r.segment.index == index);
        let segment = if let Some(i) = existing {
            self.selected_roads[i].segment
        } else {
            let segment = match self.roads.prepare_segment(index, 8) {
                Ok(segment) => segment,
                Err(error) => {
                    self.failure = Some(error.0);
                    return;
                }
            };
            self.last_road_work = self.roads.replay_work();
            let Some(segment) = segment else {
                return;
            };
            segment
        };
        let radius = self.distances.road_gen_dist;
        let geometry = self
            .road_focus
            .into_iter()
            .any(|p| source_distance_squared(p, segment.position()) <= radius * radius);
        let selected = geometry
            || self.road_focus.into_iter().any(|p| {
                intersects(
                    segment.bounds,
                    window(p, f64::from(radius) + self.road_light.influence_margin()),
                )
            });
        if !selected {
            self.next_road += 1;
            return;
        }
        let old_light = self
            .lights
            .iter()
            .find(|l| l.biome.is_some() && l.id.unit[0] == index as i64);
        let possible_light = existing.map_or(true, |i| self.selected_roads[i].light.is_some());
        let new_light = usize::from(possible_light && old_light.is_none());
        let old_poles = self
            .lights
            .iter()
            .filter(|l| {
                l.biome.is_some()
                    && !self.wanted_structures.iter().any(|p| p.id == l.id)
                    && !self.structures.iter().any(|p| p.placement.id == l.id)
            })
            .count();
        let new_pole = usize::from(
            new_light > 0
                && !self
                    .wanted_structures
                    .iter()
                    .any(|p| p.id.layer == WorldLayer::Lamp && p.id.unit[0] == index as i64)
                && !self.structures.iter().any(|p| {
                    p.placement.id.layer == WorldLayer::Lamp
                        && p.placement.id.unit[0] == index as i64
                }),
        );
        let required = self
            .metrics()
            .generated_objects
            .saturating_add(new_light + old_poles + new_pole);
        if required > self.limits.objects {
            self.failure = Some(format!(
                "Source road {index} object budget exceeded: requested {required}, limit {}",
                self.limits.objects
            ));
            return;
        }
        if existing.is_none() {
            if self.selected_roads.len() >= self.limits.roads {
                self.failure = Some(format!(
                    "Selected road descriptors exceed {} roads",
                    self.limits.roads
                ));
                return;
            }
            // Source roadGenScript links rope i to segment i + 1. The index prepares ahead of selection.
            let next = if index + 1 < super::road::ROAD_SEGMENTS {
                let next = match self.roads.prepare_segment(index + 1, 8) {
                    Ok(next) => next,
                    Err(error) => {
                        self.failure = Some(error.0);
                        return;
                    }
                };
                self.last_road_work += self.roads.replay_work();
                let Some(next) = next else {
                    return;
                };
                Some(next)
            } else {
                None
            };
            // Reserve retained output, vector overlap, the light identity, the cable, and pole placement before selection.
            let prototype = self
                .reference
                .prototypes
                .iter()
                .find(|p| p.source == self.reference.road.prototypes[segment.biome as usize])
                .unwrap();
            let node_heap = prototype
                .nodes
                .iter()
                .map(|n| n.source.file.capacity())
                .sum::<usize>();
            let light_heap = self.road_light.retained_bytes() + node_heap;
            let cable_bytes = self.cables.road_ropes(segment.biome)
                * (std::mem::size_of::<super::NodeAppearance>()
                    + std::mem::size_of::<super::cables::NodePose>())
                + node_heap;
            let bytes = (self.selected_roads.capacity() + 1) * std::mem::size_of::<SelectedRoad>()
                + prototype.nodes.len() * std::mem::size_of::<road_mesh::RoadFrame>()
                + 2 * (std::mem::size_of::<super::WorldLight>() + light_heap)
                + (self.wanted_structures.capacity() + 1) * std::mem::size_of::<Placement>()
                + 2 * light_heap
                + cable_bytes;
            if !self.admit_generation(bytes) {
                return;
            }
            if self.selected_roads.try_reserve_exact(1).is_err() {
                self.failure = Some("Selected road descriptor allocation failed".into());
                return;
            }
            if self.wanted_structures.try_reserve_exact(1).is_err() {
                self.failure = Some("Selected pole placement allocation failed".into());
                return;
            }
            let light = self.road_light.place(&self.reference, &segment);
            let mut cable = Vec::new();
            if let Some(next) = next {
                let mut poses = Vec::new();
                self.cables
                    .road(&self.reference, &segment, &next, &mut poses);
                cable = poses
                    .into_iter()
                    .map(|pose| super::NodeAppearance {
                        node: pose.node,
                        primary: None,
                        scale: Some(pose.scale),
                        rotation: Some(pose.rotation),
                        active: None,
                        material: None,
                        color: None,
                        color_material: None,
                    })
                    .collect();
            }
            self.selected_roads.push(SelectedRoad {
                segment,
                light,
                cable,
            });
        }
        let retained = self
            .selected_roads
            .iter()
            .find(|r| r.segment.index == index)
            .unwrap();
        if let Some(light) = &retained.light {
            if !self.lights.iter().any(|l| l.id == light.id) {
                self.lights.push(light.clone());
            }
            self.wanted_lights.push(index);
        }
        self.refresh_poles();
        if self.metrics().generated_objects > self.limits.objects {
            self.failure = Some(format!(
                "Source object count exceeds {}",
                self.limits.objects
            ));
            return;
        }
        if geometry {
            if self.wanted_roads.len() == self.limits.roads {
                self.failure = Some(format!("Source road count exceeds {}", self.limits.roads));
                return;
            }
            self.wanted_roads.push(index);
            append_road_points(
                &mut self.road_points,
                segment,
                self.reference.road.def_point_range,
            );
        }
        self.next_road += 1;
    }
    fn road_failure(&mut self, error: ReferenceError) {
        self.road_worker.dispose();
        self.road_worker_reserved_bytes = 0;
        self.road_work = None;
        self.failure = Some(error.0);
    }
    fn poll_road_work(&mut self) {
        match self.road_worker.poll() {
            Err(error) => self.road_failure(error),
            Ok(RoadPoll::Idle) => {
                if let Some(work) = self.road_work.take() {
                    if !work.canceled {
                        self.road_failure(ReferenceError("Road worker lost its owned job".into()));
                    }
                }
            }
            Ok(RoadPoll::Ready(id)) => {
                if let Some(work) = self
                    .road_work
                    .as_mut()
                    .filter(|w| w.identity == id && !w.canceled)
                {
                    work.completed = true;
                } else {
                    self.road_worker.discard();
                }
            }
            Ok(RoadPoll::Busy) => {}
        }
    }
    fn retarget_road_work(&mut self) {
        let required_road_missing = self.required.roads.iter().any(|id| {
            !self
                .surfaces
                .iter()
                .any(|s| s.key.layer == WorldLayer::Road && s.key.unit[0] == i64::from(*id))
        });
        if let Some(work) = &mut self.road_work {
            let valid = work.identity.session == self.request.session
                && self.road_source_revisions[work.segment.biome as usize] == work.source_revision
                && self.wanted_roads.contains(&work.segment.index)
                && self
                    .selected_roads
                    .iter()
                    .any(|r| r.segment == work.segment)
                && !self.surfaces.iter().any(|s| {
                    s.key.layer == WorldLayer::Road
                        && s.key.unit[0] == i64::from(work.segment.index)
                })
                && (!required_road_missing || self.required.roads.contains(&work.segment.index));
            if valid && !work.canceled {
                // Keep the submitted identity immutable. Retarget only the main-thread claim.
                work.request = self.request;
            } else if !work.canceled {
                work.canceled = true;
                self.road_worker.discard();
            }
        }
    }
    fn prepare_road(&mut self) {
        let _timer = super::stream_profile::Timer::new(super::stream_profile::Stage::Road);
        if let Some(work) = &self.road_work {
            if !work.completed || work.canceled {
                return;
            }
            if !same_request(work.request, self.request)
                || !(self.required.roads.contains(&work.segment.index) || self.required_ready())
            {
                return;
            }
            let mut surface = WorldSurface::default();
            if let Err(error) = self.road_worker.read(&mut surface) {
                self.road_failure(error);
                return;
            }
            let work = self.road_work.take().unwrap();
            self.next_geometry += 1;
            self.ready = Some(PreparedSurface {
                road_claim: Some((work.identity, work.source_revision)),
                pending_bytes: work.surface_bytes,
                renderer_ready: false,
                offered: false,
                renderer_reserved: false,
                request: self.request,
                published: PublishedSurface {
                    key: SurfaceKey {
                        layer: WorldLayer::Road,
                        unit: [i64::from(work.segment.index), 0],
                    },
                    bounds: surface_bounds(&surface),
                    surface: Arc::new(surface),
                    prototype: Some(
                        self.reference.road.prototypes[work.segment.biome as usize].clone(),
                    ),
                    collider: None,
                    collision_bytes: 0,
                    road: Some(work.segment),
                },
            });
            return;
        }
        if self.available_external_jobs() == 0 {
            return;
        }
        let required_ready = self.required_ready();
        let Some(index) = self.wanted_roads.iter().copied().find(|id| {
            (self.required.roads.contains(id) || required_ready)
                && !self
                    .surfaces
                    .iter()
                    .any(|s| s.key.layer == WorldLayer::Road && s.key.unit[0] == i64::from(*id))
        }) else {
            self.next_geometry = self.wanted_roads.len();
            return;
        };
        let segment = self
            .selected_roads
            .iter()
            .find(|r| r.segment.index == index)
            .unwrap()
            .segment;
        let owner = segment.biome as usize;
        if !self
            .prototype_holds
            .get(&self.reference.road.prototypes[owner])
            .is_some_and(|bound| *bound)
        {
            return;
        }
        let prototype = self.road_prototypes[owner]
            .as_ref()
            .expect("admitted road prototype");
        let source_revision = self.road_source_revisions[owner];
        let install = !self
            .road_worker
            .installed(self.request.session, source_revision, owner);
        let layout = RoadLayout::new(prototype);
        let install_bytes = if install {
            road_mesh::prototype_encoded_len(prototype)
        } else {
            0
        };
        // Old installed storage remains charged while the new source and all copies coexist.
        let pending_bytes = layout.job_bytes() + install_bytes * 3;
        let cache_bytes = if install { layout.cache_bytes } else { 0 };
        if !self.can_request_independent(1) || !self.admit_generation(pending_bytes + cache_bytes) {
            return;
        }
        let capacity = self
            .road_worker
            .cache_bytes()
            .checked_add(cache_bytes)
            .and_then(|v| v.checked_add(pending_bytes))
            .and_then(|v| v.checked_mul(2))
            .and_then(|v| v.checked_add(super::road_worker::INITIAL_WASM_BYTES))
            .and_then(usize::checked_next_power_of_two);
        let Some(capacity) = capacity else {
            self.road_failure(ReferenceError("Road capacity overflow".into()));
            return;
        };
        // This is an admission estimate, separate from the logical operation charge.
        let capacity = capacity.max(self.road_worker_reserved_bytes);
        let delta = capacity - self.road_worker_reserved_bytes;
        if self
            .metrics()
            .active_cpu_bytes
            .saturating_add(pending_bytes + cache_bytes)
            .saturating_add(delta)
            > self.limits.active_cpu
        {
            self.road_failure(ReferenceError(
                "Road worker capacity exceeds CPU budget".into(),
            ));
            return;
        }
        // Retain actor capacity before creation. Wait for initialization before packing or sends.
        self.road_worker_reserved_bytes = capacity;
        match self.road_worker.initialize(capacity) {
            Ok(true) => {}
            Ok(false) => return,
            Err(error) => {
                self.road_failure(error);
                return;
            }
        }
        let identity =
            match self
                .road_worker
                .next_id(self.request.session, self.request.revision, index)
            {
                Ok(id) => id,
                Err(error) => {
                    self.road_failure(error);
                    return;
                }
            };
        // Charge the complete operation before creating or dispatching the actor.
        self.road_work = Some(RoadWork {
            pending_bytes: pending_bytes + cache_bytes,
            surface_bytes: layout.surface_bytes(),
            request: self.request,
            identity,
            segment,
            source_revision,
            completed: false,
            canceled: false,
        });
        if let Err(error) = self.road_worker.packing_started() {
            self.road_failure(error);
            return;
        }
        if install {
            let mut input = Vec::with_capacity(install_bytes);
            {
                let _packing =
                    super::stream_profile::Timer::new(super::stream_profile::Stage::RoadPacking);
                road_mesh::encode_prototype(
                    self.road_prototypes[owner].as_ref().unwrap(),
                    &mut input,
                );
            }
            self.road_worker.packing_ended();
            let source = RoadJobId {
                session: self.request.session,
                revision: source_revision,
                owner: owner as u32,
                sequence: identity.sequence,
            };
            if let Err(error) = self.road_worker.install(source, &input, layout) {
                self.road_failure(error);
                return;
            }
            // Installation now owns the retained cache charge. Keep transient copies reserved.
            self.road_work.as_mut().unwrap().pending_bytes -= cache_bytes;
            if let Err(error) = self.road_worker.packing_started() {
                self.road_failure(error);
                return;
            }
        }
        let packing = super::stream_profile::Timer::new(super::stream_profile::Stage::RoadPacking);
        let mut pose = Vec::with_capacity(144);
        road_mesh::encode_pose(
            &road_mesh::RoadPose {
                origin: segment.position(),
                rotations: segment.source_rotations,
            },
            &mut pose,
        );
        let mut input = [0; 156];
        input[..4].copy_from_slice(&(owner as u32).to_le_bytes());
        input[4..12].copy_from_slice(&source_revision.to_le_bytes());
        input[12..].copy_from_slice(&pose);
        drop(packing);
        self.road_worker.packing_ended();
        if let Err(error) = self.road_worker.submit(identity, &input) {
            self.road_failure(error);
            return;
        }
    }
    fn local_collision_pending(&self, key: SurfaceKey) -> bool {
        if key.layer != WorldLayer::Terrain || self.surfaces.iter().any(|s| s.key == key) {
            return false;
        }
        let size = f64::from(self.tiles[0].size);
        let x = key.unit[0] as f64 * size;
        let z = -(key.unit[1] as f64 * size);
        [self.request.player, self.request.vehicle]
            .into_iter()
            .any(|p| (p[0] - x).abs() <= size * 0.5 + 8. && (p[2] - z).abs() <= size * 0.5 + 8.)
    }
    fn prepare_terrain(&mut self, mut should_yield: impl FnMut() -> bool) {
        let _timer = super::stream_profile::Timer::new(super::stream_profile::Stage::Terrain);
        let had_pending = self.terrain_execution.has_pending();
        match self
            .terrain_execution
            .advance(self.request, &mut should_yield)
        {
            Ok(Some(CompletedTerrain { metadata, surface })) => {
                self.ready = Some(PreparedSurface {
                    road_claim: None,
                    pending_bytes: metadata.pending_bytes,
                    renderer_ready: false,
                    offered: false,
                    renderer_reserved: false,
                    request: metadata.request,
                    published: PublishedSurface {
                        key: metadata.key,
                        bounds: surface_bounds(&surface),
                        surface: Arc::new(surface),
                        prototype: None,
                        collider: None,
                        collision_bytes: 0,
                        road: None,
                    },
                });
                return;
            }
            Ok(None) => {}
            Err(error) => {
                self.failure = Some(error.0);
                return;
            }
        }
        if had_pending {
            return;
        }
        if !self.terrain_execution.has_pending()
            && (self.available_external_jobs() == 0 || !self.can_request_independent(1))
        {
            return;
        }
        if !self.terrain_execution.has_pending() {
            let required_ready = self.required_ready();
            let Some(key) = self
                .wanted_tiles
                .iter()
                .find(|key| self.local_collision_pending(**key))
                .or_else(|| {
                    self.wanted_tiles.iter().find(|key| {
                        (self.required.terrain.contains(key) || required_ready)
                            && !self.surfaces.iter().any(|s| s.key == **key)
                    })
                })
                .copied()
            else {
                return;
            };
            let tier = layer_tier(key.layer);
            let tile = self.tiles[tier];
            if !self.terrain_ready[tier] {
                return;
            }
            // Reserve all output, border and context storage before constructing the job.
            let startup = self.startup.as_ref().map_or(0, |s| {
                s.foundations.len() * std::mem::size_of::<TerrainFoundation>()
                    + s.holes.len() * std::mem::size_of::<[[f32; 3]; 4]>()
            });
            let pending_bytes = super::terrain_output_bound(tile)
                + (self.placements.foundation_count()
                    + self.structure_helpers.count(&self.wanted_structures))
                    * 2
                    * (std::mem::size_of::<super::FoundationPoint>()
                        + std::mem::size_of::<TerrainFoundation>())
                + startup * 2
                + self.road_points.len() * std::mem::size_of::<TerrainRoadPoint>() * 2
                + 512;
            if !self.admit_generation(pending_bytes) {
                return;
            }
            #[cfg(target_arch = "wasm32")]
            let _profile =
                crate::cpu_profile::Timer::new(crate::cpu_profile::Stage::TerrainInputPreparation);
            let mut foundations = Vec::new();
            let mut holes = Vec::new();
            if tier == 0 {
                if let Some(startup) = &self.startup {
                    foundations.extend_from_slice(&startup.foundations);
                    let center = [
                        key.unit[0] as f32 * tile.size as f32,
                        key.unit[1] as f32 * tile.size as f32,
                    ];
                    holes.extend(
                        startup
                            .holes
                            .iter()
                            .zip(&startup.hole_centers)
                            .filter(|(_, position)| {
                                let x = position[0] - center[0];
                                let z = position[2] - center[1];
                                x * x + z * z < tile.size as f32 * tile.size as f32 * 1.25
                            })
                            .map(|(hole, _)| *hole),
                    );
                }
                let mut generated = Vec::new();
                let center = [
                    key.unit[0] as f64 * tile.size as f64,
                    0.0,
                    -key.unit[1] as f64 * tile.size as f64,
                ];
                if let Err(error) = self.placements.foundations_in(
                    window(center, tile.size as f64),
                    self.placements.coverage(),
                    &mut generated,
                ) {
                    self.failure = Some(error.0);
                    return;
                }
                self.structure_helpers
                    .append(&self.wanted_structures, &mut foundations);
                foundations.extend(generated.iter().map(|p| TerrainFoundation {
                    position: [
                        p.position[0] as f32,
                        p.position[1] as f32,
                        -p.position[2] as f32,
                    ],
                    range_squared: p.range_squared,
                }));
            }
            let roads = self
                .road_points
                .iter()
                .filter(|point| {
                    let x = point.segment_origin[0] - key.unit[0] as f32 * tile.size as f32;
                    let z = point.segment_origin[1] - key.unit[1] as f32 * tile.size as f32;
                    x * x + z * z < 1_000_000.0
                })
                .copied()
                .collect::<Vec<_>>();
            let mut input = Vec::new();
            TerrainJob {
                settings: self.evaluator.settings(),
                unit: key.unit,
                tile,
                biome_override: self.mode.biome_override(),
                roads: &roads,
                foundations: &foundations,
                holes: &holes,
            }
            .encode(&mut input);
            self.terrain_execution.enqueue(
                TerrainMetadata {
                    pending_bytes,
                    request: self.request,
                    key,
                    tile,
                },
                input,
            );
            if let Err(error) = self.terrain_execution.advance(self.request, should_yield) {
                self.failure = Some(error.0);
            }
        }
    }
    fn road_publication_valid(&self, work: &PreparedSurface) -> bool {
        let Some(road) = work.published.road else {
            return true;
        };
        work.road_claim.is_some_and(|(identity, source_revision)| {
            identity.session == self.request.session
                && identity.owner == road.index
                && source_revision == self.road_source_revisions[road.biome as usize]
        }) && self.plan_revision == self.request.revision
            && self.next_road == self.road_candidates.len()
            && work.published.key
                == SurfaceKey {
                    layer: WorldLayer::Road,
                    unit: [i64::from(road.index), 0],
                }
            && self.wanted_roads.contains(&road.index)
            && self.selected_roads.iter().any(|r| r.segment == road)
            && !self.surfaces.iter().any(|s| s.key == work.published.key)
            && (self.required.roads.contains(&road.index) || self.required_ready())
    }
    pub fn prepared_surface(&self) -> Option<(WorldRequest, &PublishedSurface)> {
        self.ready
            .as_ref()
            .filter(|work| {
                work.offered
                    && same_request(work.request, self.request)
                    && self.road_publication_valid(work)
            })
            .map(|work| (work.request, &work.published))
    }
    /// Applies the facts of the last renderer step in order.
    /// Facts of an older session are dropped.
    pub fn apply_renderer_facts(
        &mut self,
        facts: &mut super::RendererFacts,
    ) -> Result<(), ReferenceError> {
        use super::RendererFact::*;
        let session = self.request.session;
        for fact in facts.drain() {
            match fact {
                HoldPrototype(source) => {
                    self.hold_prototype(source)?;
                }
                BindPrototype(source) => self.bind_prototype(&source)?,
                ConfigureStaging { reserve, metadata } => {
                    self.configure_dependency_staging(reserve, metadata)?
                }
                ReleasePrototype(source) => self.release_prototype(&source)?,
                BindRecipe(source) => self.bind_structure_recipe(&source)?,
                ReserveSurface(request, key) if request.session == session => {
                    self.reserve_surface_renderer(request, key)?
                }
                AcceptSurface(request, key) if request.session == session => {
                    self.accept_surface_renderer(request, key)?
                }
                DiscardSurface(request, key) if request.session == session => {
                    self.discard_surface_renderer(request, key)?
                }
                AcceptStructure(request, id) if request.session == session => {
                    self.accept_structure_renderer(request, &id)?
                }
                ReserveSurface(..) | AcceptSurface(..) | DiscardSurface(..)
                | AcceptStructure(..) => {}
            }
        }
        Ok(())
    }
    /// The renderer starts the prepared surface. It owns its claim.
    /// The renderer reads an older frame, so the stream can drop the unreserved work first.
    /// The renderer then retires its claim when its frame shows the drop.
    fn reserve_surface_renderer(
        &mut self,
        request: WorldRequest,
        key: SurfaceKey,
    ) -> Result<(), ReferenceError> {
        let Some(work) = self
            .ready
            .as_ref()
            .filter(|work| same_request(work.request, request) && work.published.key == key)
        else {
            return Ok(());
        };
        if work.renderer_reserved {
            return Err(ReferenceError(
                "Source renderer already owns its claim".into(),
            ));
        }
        if !work.offered && !self.offer_surface()? {
            return Err(ReferenceError(
                "Source renderer staging exceeds its resource budget".into(),
            ));
        }
        self.ready.as_mut().unwrap().renderer_reserved = true;
        Ok(())
    }
    /// Charges the render staging and memory of the prepared surface before the renderer sees
    /// it. The renderer reserves from an older frame, so the stream must not grant this room to
    /// other work meanwhile. False while staging waits for renderer credit.
    fn offer_surface(&mut self) -> Result<bool, ReferenceError> {
        let Some(work) = self.ready.as_ref() else {
            return Ok(false);
        };
        if work.offered {
            return Ok(true);
        }
        let cpu = surface_render_cpu(&work.published.surface);
        let gpu = surface_render_bytes(&work.published.surface);
        let metrics = self.metrics();
        let staging = STAGING - self.dependency_staging_reserve;
        if work.pending_bytes.saturating_add(cpu) > staging
            || metrics.active_cpu_bytes.saturating_add(cpu) > self.limits.active_cpu
            || metrics.active_gpu_bytes.saturating_add(gpu) > self.limits.active_gpu
        {
            return Err(ReferenceError(format!(
                "Source renderer allocation exceeds its resource budget: staging {} of {staging} bytes, CPU {} of {} bytes, GPU {} of {} bytes",
                work.pending_bytes.saturating_add(cpu),
                metrics.active_cpu_bytes.saturating_add(cpu),
                self.limits.active_cpu,
                metrics.active_gpu_bytes.saturating_add(gpu),
                self.limits.active_gpu,
            )));
        }
        if !self.wait_for_room(cpu <= self.available_staging_bytes(), Reclaim::STAGING) {
            return Ok(false);
        }
        self.ready.as_mut().unwrap().offered = true;
        Ok(true)
    }
    /// The renderer dropped the prepared surface after its work drained.
    /// Work that the stream dropped before the reservation arrived is already rewound.
    fn discard_surface_renderer(
        &mut self,
        request: WorldRequest,
        key: SurfaceKey,
    ) -> Result<(), ReferenceError> {
        if !self
            .ready
            .as_ref()
            .is_some_and(|w| same_request(w.request, request) && w.published.key == key)
        {
            return Ok(());
        }
        self.ready = None;
        if key.layer == WorldLayer::Road {
            self.road_worker.publication_ended(false);
        }
        self.rewind_road(key);
        Ok(())
    }
    /// The renderer finished the prepared surface. Its work drained.
    /// The renderer repeats the fact until its frame shows the publication, so a repeat
    /// for work that this stream already published or rewound is dropped.
    fn accept_surface_renderer(
        &mut self,
        request: WorldRequest,
        key: SurfaceKey,
    ) -> Result<(), ReferenceError> {
        let Some(work) = self
            .ready
            .as_mut()
            .filter(|work| same_request(work.request, request) && work.published.key == key)
        else {
            return Ok(());
        };
        if !work.renderer_reserved {
            return Err(ReferenceError("Unreserved source renderer".into()));
        }
        work.renderer_ready = true;
        Ok(())
    }
    /// Publish at most one completed unit. Stale work never changes accepted collision.
    pub fn publish(
        &mut self,
        physics: &mut PhysicsWorld,
        required_lights: &[super::GeneratedId],
    ) -> Result<(), ReferenceError> {
        let result = self.publish_unit(physics, required_lights);
        if result.is_ok() && self.failure.is_none() {
            self.retire_surfaces(physics);
        }
        result
    }
    fn publication_light(
        &self,
        light: &super::WorldLight,
        required: &[super::GeneratedId],
    ) -> bool {
        required.contains(&light.id)
            || (light.biome.is_some()
                && self.ready.as_ref().is_some_and(|work| {
                    work.published.key.layer == WorldLayer::Road
                        && work.published.key.unit[0] == light.id.unit[0]
                }))
    }
    fn publish_unit(
        &mut self,
        physics: &mut PhysicsWorld,
        required_lights: &[super::GeneratedId],
    ) -> Result<(), ReferenceError> {
        if !self.publication_available || self.failure.is_some() {
            return Ok(());
        }
        if self.shared_cpu_bytes.is_none() {
            return Ok(());
        }
        if self
            .ready
            .as_ref()
            .is_some_and(|work| !same_request(work.request, self.request))
        {
            self.discard_unreserved_surface();
            return Ok(());
        }
        // Hold the surface until every required owner publishes. Drain individually ready
        // structures meanwhile, so unrelated owners cannot fill the shared pending queue.
        // Each structure receipt already includes geometry, materials and caster pipelines.
        if self
            .lights
            .iter()
            .filter(|light| self.publication_light(light, required_lights))
            .any(|light| {
                !self
                    .structures
                    .iter()
                    .any(|s| light.owned_by(&s.placement.id))
            })
        {
            return self.publish_structure(physics, required_lights);
        }
        let Some(work) = self.ready.as_ref() else {
            return self.publish_structure(physics, required_lights);
        };
        let admitted = match &work.published.prototype {
            Some(source) => self.prototype_holds.get(source).is_some_and(|bound| *bound),
            None => self.terrain_ready[layer_tier(work.published.key.layer)],
        };
        if !admitted || !work.renderer_ready || !self.road_publication_valid(work) {
            return Ok(());
        }
        let metrics = self.metrics();
        // Geometry and renderer staging were reserved before allocation. Rapier adds collision storage.
        let count = match work.published.key.layer {
            WorldLayer::Road => (metrics.road_segments, self.limits.roads),
            WorldLayer::Terrain => (metrics.near_tiles, self.limits.tiles[0]),
            WorldLayer::DistantTerrain => (metrics.distant_tiles[0], self.limits.tiles[1]),
            WorldLayer::DistantTerrain2 => (metrics.distant_tiles[1], self.limits.tiles[2]),
            _ => unreachable!(),
        };
        if count.0 >= count.1
            || metrics.active_cpu_bytes > self.limits.active_cpu
            || metrics.active_gpu_bytes > self.limits.active_gpu
        {
            let error = ReferenceError(format!(
                "Source geometry budget at {:?}",
                work.published.key
            ));
            self.failure = Some(error.0.clone());
            return Err(error);
        }
        self.publication_available = false;
        let mut work = self.ready.take().unwrap();
        let road = work.published.road.is_some();
        let _publication = road.then(|| {
            super::stream_profile::Timer::new(super::stream_profile::Stage::RoadPublication)
        });
        if road {
            self.road_worker.publication_started();
        }
        if matches!(
            work.published.key.layer,
            WorldLayer::Road | WorldLayer::Terrain
        ) {
            let layer = work.published.road.map_or(self.terrain_layer, |road| {
                self.road_layers[road.biome as usize]
            });
            let surface = &work.published.surface;
            let tile = self.tiles[0];
            let grid = (work.published.key.layer == WorldLayer::Terrain
                && ::terrain::regular_grid(tile, &surface.vertices, &surface.triangles))
            .then_some(tile.size as f32);
            match physics.insert_source_mesh(
                surface,
                grid,
                self.origin.local(work.published.surface.origin),
                layer,
                self.reference.collision_rules.layers[layer as usize],
                0.6,
                0.0,
                self.limits
                    .active_cpu
                    .saturating_sub(metrics.active_cpu_bytes),
            ) {
                Ok((collider, collision_bytes)) => {
                    work.published.collider = Some(collider);
                    work.published.collision_bytes = collision_bytes;
                }
                Err(error) => {
                    self.failure = Some(error.0.clone());
                    if road {
                        self.road_worker.publication_ended(false);
                    }
                    return Err(error);
                }
            }
        }
        self.surfaces.push(work.published);
        if road {
            self.road_worker.publication_ended(true);
        }
        Ok(())
    }
    fn retire_surfaces(&mut self, physics: &mut PhysicsWorld) {
        // Keep old support until both new receiver windows have published near terrain.
        if self.recovery.is_none()
            && self.next_road == self.road_candidates.len()
            && self.supports(window(self.request.player, 3.0))
            && self.supports(window(self.request.vehicle, 4.0))
        {
            let distances = &self.distances;
            let focus = [self.request.player, self.request.vehicle];
            let tiles = self.tiles;
            let mut removed = [false; 4];
            self.surfaces.retain(|surface| {
                let tier = if surface.key.layer == WorldLayer::Road {
                    0
                } else {
                    layer_tier(surface.key.layer) + 1
                };
                let inside = if tier == 0 {
                    self.wanted_roads.contains(&(surface.key.unit[0] as u32))
                } else {
                    let size = tiles[tier - 1].size as f32;
                    let radius = [
                        distances.terrain_remove_dist,
                        distances.distant_terrain_remove_dist,
                        distances.distant_terrain2_remove_dist,
                    ][tier - 1];
                    let center = [
                        surface.key.unit[0] as f32 * size,
                        surface.key.unit[1] as f32 * size,
                    ];
                    focus.into_iter().any(|p| {
                        let x = (p[0] as f32 / size).round_ties_even() * size;
                        let z = (-p[2] as f32 / size).round_ties_even() * size;
                        let dx = center[0] - x;
                        let dz = center[1] - z;
                        dx * dx + dz * dz < radius * radius
                    })
                };
                // The source removes at most one object per tier update.
                let keep = inside || removed[tier];
                if !keep {
                    removed[tier] = true;
                }
                if !keep {
                    if let Some(handle) = surface.collider {
                        physics.remove_collider(handle);
                    }
                }
                keep
            });
        }
        if self.recovery.is_none() && self.next_road == self.road_candidates.len() {
            self.lights.retain(|light| {
                light.biome.is_none() || self.wanted_lights.contains(&(light.id.unit[0] as u32))
            });
            self.refresh_poles();
        }
    }
    pub fn supports(&self, swept: WorldBounds) -> bool {
        self.support_gap(swept).is_none()
    }
    /// The first missing part of the world in `swept`, or None when it is loaded.
    pub fn support_gap(&self, swept: WorldBounds) -> Option<SupportGap<'_>> {
        if !self.initial_generation_ready
            || self.plan_revision != self.request.revision
            || self.next_road != self.road_candidates.len()
            || ![swept.min[0], swept.min[2], swept.max[0], swept.max[2]]
                .into_iter()
                .all(f64::is_finite)
        {
            return Some(SupportGap::Plan);
        }
        if let Some(road) = self.selected_roads.iter().find(|r| {
            self.wanted_roads.contains(&r.segment.index)
                && intersects(r.segment.bounds, swept)
                && !self.surfaces.iter().any(|s| {
                    s.key.layer == WorldLayer::Road
                        && s.key.unit[0] == i64::from(r.segment.index)
                        && s.collider.is_some()
                })
        }) {
            let index = road.segment.index;
            let surface = self
                .surfaces
                .iter()
                .any(|s| s.key.layer == WorldLayer::Road && s.key.unit[0] == i64::from(index));
            return Some(SupportGap::Road { index, surface });
        }
        // A sandbox spawn never gates the step: it may load late or not at all.
        if let Some(placement) = self.wanted_structures.iter().find(|p| {
            !self.added_placement(p)
                && [0, 2].into_iter().all(|axis| {
                    p.bounds.min[axis] <= swept.max[axis] && p.bounds.max[axis] >= swept.min[axis]
                })
                && !self.structures.iter().any(|s| s.placement.id == p.id)
        }) {
            return Some(SupportGap::Structure(placement));
        }
        if self.lights.iter().any(|light| {
            light.influences(swept)
                && !self
                    .structures
                    .iter()
                    .any(|s| light.owned_by(&s.placement.id))
        }) {
            return Some(SupportGap::Light);
        }
        let size = self.tiles[0].size as f64;
        let x0 = ((swept.min[0] + size * 0.5) / size).floor() as i64;
        let x1 = ((swept.max[0] + size * 0.5) / size).floor() as i64;
        let z0 = ((-swept.max[2] + size * 0.5) / size).floor() as i64;
        let z1 = ((-swept.min[2] + size * 0.5) / size).floor() as i64;
        if x1 < x0
            || z1 < z0
            || (x1 - x0 + 1).saturating_mul(z1 - z0 + 1) > self.limits.tiles[0] as i64
        {
            return Some(SupportGap::TileCount);
        }
        (x0..=x1)
            .flat_map(|x| (z0..=z1).map(move |z| [x, z]))
            .find(|&unit| {
                !self.surfaces.iter().any(|s| {
                    s.key
                        == SurfaceKey {
                            layer: WorldLayer::Terrain,
                            unit,
                        }
                        && s.collider.is_some()
                })
            })
            .map(SupportGap::Terrain)
    }
    fn refresh_helper_context(&mut self, bounds: WorldBounds) {
        let required = self.placements.foundation_count()
            + self.startup.as_ref().map_or(0, |s| s.foundations.len())
            + self.structure_helpers.count(&self.wanted_structures);
        let extra = required.saturating_sub(self.color_helpers.capacity())
            * std::mem::size_of::<TerrainFoundation>();
        if self.metrics().active_cpu_bytes.saturating_add(extra) > self.limits.active_cpu {
            self.failure = Some("Source helper context exceeds active CPU budget".into());
            return;
        }
        if let Err(error) = self.placements.foundations_in(
            bounds,
            self.placements.coverage(),
            &mut self.color_foundations,
        ) {
            self.failure = Some(error.0);
            return;
        }
        self.color_helpers.clear();
        if let Some(startup) = &self.startup {
            self.color_helpers.extend_from_slice(&startup.foundations);
        }
        self.color_helpers
            .extend(self.color_foundations.iter().map(|p| TerrainFoundation {
                position: [
                    p.position[0] as f32,
                    p.position[1] as f32,
                    -p.position[2] as f32,
                ],
                range_squared: p.range_squared,
            }));
        self.structure_helpers
            .append(&self.wanted_structures, &mut self.color_helpers);
    }
    pub fn terrain_color(&mut self, point: [f64; 3]) -> [f32; 4] {
        self.terrain_sample(point).1
    }
    /// Terrain surface height and color at an absolute point. Source Z is the negated world Z.
    pub fn terrain_sample(&mut self, point: [f64; 3]) -> (f32, [f32; 4]) {
        self.refresh_helper_context(window(point, 1000.));
        let xz = [point[0] as f32, -point[2] as f32];
        let context = TerrainContext {
            evaluator: &self.evaluator,
            tile: self.tiles[0],
            roads: &self.road_points,
            road_radius_squared: Some(1_000_000.0),
            foundations: &self.color_helpers,
            holes: &[],
            biome_override: self.mode.biome_override(),
        };
        let height = context.height(xz);
        let color = self.evaluator.color(xz, height);
        (height, super::forced_color(color, context.biome_override))
    }
    pub fn required_content(&self) -> super::RequiredContent<'_> {
        super::RequiredContent {
            request: self.request,
            coverage: self.coverage(),
            selection_revision: self.required.revision,
            roads: &self.required.roads,
            terrain: &self.required.terrain,
            structures: &self.required.structures,
            lights: &self.required.lights,
            collision_ranges: [
                window(self.request.player, 8.),
                window(self.request.vehicle, 8.),
            ],
        }
    }
    pub fn required_ready(&self) -> bool {
        self.initial_generation_ready
            && self.required.views_ready
            && self.startup.is_some()
            && self.failure.is_none()
            && !self.ramps_wait
            && self.plan_revision == self.request.revision
            && self.next_road == self.road_candidates.len()
            && self.required.roads.iter().all(|id| {
                self.surfaces
                    .iter()
                    .any(|s| s.key.layer == WorldLayer::Road && s.key.unit[0] == i64::from(*id))
            })
            && self
                .required
                .terrain
                .iter()
                .all(|key| self.surfaces.iter().any(|s| s.key == *key))
            && self.required.structures.iter().all(|id| {
                self.structures.iter().any(|s| s.placement.id == *id) && self.bindings_ready(id)
            })
            && self
                .required_content()
                .collision_ranges
                .into_iter()
                .all(|bounds| self.supports(bounds))
            && self.props.selection_complete()
            && self
                .contents
                .discovery_ready(&self.reference, &self.wanted_structures, &self.replay_roots)
                .is_ok_and(|ready| ready)
    }
    pub fn structure_required(&self, id: &super::GeneratedId) -> bool {
        self.required.structures.contains(id)
    }
    pub fn structure_replay_required(&self, id: &super::GeneratedId) -> bool {
        self.replay_roots.iter().any(|root| &root.id == id)
            || self
                .replay_indices
                .iter()
                .any(|&index| self.changes.get_indexed(index).1.instance.as_ref() == Some(id))
    }
    pub fn select_required(&mut self, views: &[super::RequiredView]) {
        let ranges = self.required_content().collision_ranges;
        let required = |bounds, layers| {
            ranges.into_iter().any(|r| intersects(r, bounds))
                || views.iter().any(|view| view.includes(bounds, layers))
        };
        let roads = &self.selected_roads;
        let wanted = &self.wanted_roads;
        let road_layers = self.road_layers;
        let road_needed = |id: &u32| {
            wanted.contains(id)
                && roads
                    .iter()
                    .find(|r| r.segment.index == *id)
                    .is_some_and(|r| {
                        required(r.segment.bounds, 1 << road_layers[r.segment.biome as usize])
                    })
        };
        let mut changed = super::required::select(
            &mut self.required.roads,
            wanted.iter().filter(|id| road_needed(id)),
        );
        // Unbuilt terrain has no checked vertical bound. Refine membership after its geometry exists.
        let tile_needed = |key: &SurfaceKey| {
            self.wanted_tiles.contains(key)
                && self
                    .surfaces
                    .iter()
                    .find(|s| s.key == *key)
                    .map_or(true, |s| required(s.bounds, 1 << self.terrain_layer))
        };
        changed |= super::required::select(
            &mut self.required.terrain,
            self.wanted_tiles.iter().filter(|key| tile_needed(key)),
        );
        let light_needed = |light: &super::WorldLight| {
            light.enabled
                && light.intensity > 0.
                && light.record.culling_mask != 0
                && (ranges.into_iter().any(|r| light.influences(r))
                    || views.iter().any(|view| {
                        view.includes(
                            WorldBounds {
                                min: light.pose.position.map(|v| v - f64::from(light.range)),
                                max: light.pose.position.map(|v| v + f64::from(light.range)),
                            },
                            light.record.culling_mask,
                        )
                    }))
        };
        changed |= super::required::select(
            &mut self.required.lights,
            self.lights
                .iter()
                .filter(|l| light_needed(l))
                .map(|l| &l.id),
        );
        let structure_needed = |p: &Placement| {
            let layers = self
                .reference
                .prototypes
                .iter()
                .find(|v| v.source == p.prototype)
                .map_or(0, |prototype| {
                    prototype
                        .nodes
                        .iter()
                        .filter(|n| n.renderer_enabled)
                        .fold(0, |mask, n| mask | (1 << n.layer))
                });
            ranges.into_iter().any(|r| {
                [0, 2].into_iter().all(|axis| {
                    p.bounds.min[axis] <= r.max[axis] && p.bounds.max[axis] >= r.min[axis]
                })
            }) || required(p.bounds, layers)
                || self.lights.iter().any(|l| {
                    (self.required.lights.contains(&l.id)
                        || ranges.into_iter().any(|r| l.influences(r))
                        || (l.biome.is_some()
                            && self.required.roads.contains(&(l.id.unit[0] as u32))))
                        && l.owned_by(&p.id)
                })
        };
        changed |= super::required::select(
            &mut self.required.structures,
            self.wanted_structures
                .iter()
                .filter(|p| structure_needed(p))
                .map(|p| &p.id),
        );
        self.required.views_ready = !views.is_empty();
        if changed {
            self.required.revision = self.required.revision.wrapping_add(1);
            self.next_geometry = 0;
        }
    }
    pub fn recovery_target(
        &mut self,
        point: [f64; 3],
        prefer_supply: bool,
    ) -> Result<Option<AbsolutePose>, ReferenceError> {
        if let Some(pending) = self.recovery {
            return Ok(Some(pending.target));
        }
        self.recovery_search = Some((point, prefer_supply));
        if !self.initial_generation_ready || (prefer_supply && !self.placements.prepare(0)) {
            return Ok(None);
        }
        if prefer_supply {
            if let (super::QueryStatus::Complete, Some(supply)) = self
                .placements
                .nearest_supply_point(point, self.placements.coverage())?
            {
                let pose = supply.cap_pose;
                let owner = (supply.building_index, supply.supply_index);
                self.request_at(pose.position, pose.position);
                self.recovery = Some(PendingRecovery {
                    request: self.request,
                    target: pose,
                    supply: Some(owner),
                    candidate: 0,
                    best: None,
                });
                self.recovery_search = None;
                return Ok(Some(pose));
            }
        }
        let mut nearest = None;
        for surface in &self.surfaces {
            let Some(road) = surface.road else {
                continue;
            };
            for (control_index, control) in road.source_coords.iter().copied().enumerate() {
                let adjacent = if control_index == 0 {
                    1
                } else {
                    control_index - 1
                };
                let a = if control_index == 0 {
                    control
                } else {
                    road.source_coords[adjacent]
                };
                let b = if control_index == 0 {
                    road.source_coords[adjacent]
                } else {
                    control
                };
                let yaw = (-(b[0] - a[0])).atan2(b[2] - a[2]);
                let rotation = rapier3d::math::Rotation::from_rotation_y(yaw).to_array();
                let position = [
                    road.source_origin[0] + road.source_position[0] + control[0] as f64,
                    road.source_origin[1]
                        + road.source_position[1]
                        + control[1] as f64
                        + self.reference.road.road_above_terrain as f64,
                    -(road.source_origin[2] + road.source_position[2] + control[2] as f64),
                ];
                let distance = (0..3)
                    .map(|a| (position[a] - point[a]).powi(2))
                    .sum::<f64>();
                if nearest.as_ref().is_none_or(|(d, _)| distance < *d) {
                    nearest = Some((distance, AbsolutePose { position, rotation }));
                }
            }
        }
        let Some((_, pose)) = nearest else {
            return Ok(None);
        };
        self.request_at(pose.position, pose.position);
        self.recovery = Some(PendingRecovery {
            request: self.request,
            target: pose,
            supply: None,
            candidate: 0,
            best: None,
        });
        self.recovery_search = None;
        Ok(Some(pose))
    }
    /// The material cache admits the complete visible prototype before this recipe is bound.
    /// Without room the recipe waits while the renderer holds unused prototype CPU credit.
    /// The renderer gives it back and `bind_waiting_recipes` binds it. Without unused credit
    /// the stream fails.
    fn bind_structure_recipe(&mut self, source: &SourceId) -> Result<(), ReferenceError> {
        if self.recipes.contains_key(source) {
            return Ok(());
        }
        if !self.prototype_holds.get(source).is_some_and(|bound| *bound) {
            return Err(ReferenceError(
                "Structure material binding is not ready".into(),
            ));
        }
        let prototype = self
            .reference
            .prototypes
            .iter()
            .find(|prototype| &prototype.source == source)
            .ok_or_else(|| ReferenceError("Missing structure prototype".into()))?;
        let bytes = super::StructureRecipe::reservation(prototype);
        let owner_bytes = super::metrics::map_insert_bytes(&self.recipes) + source.file.len();
        let used = self.metrics().prototype_cpu_bytes;
        if used.saturating_add(bytes).saturating_add(owner_bytes) > PROTOTYPE_CPU {
            if self.presentation.prototype_cpu > self.presentation_used.prototype_cpu {
                self.wait_for_room(false, Reclaim::PROTOTYPE_CPU);
                return Ok(());
            }
            return Err(ReferenceError(format!(
                "Source collision recipe exceeds prototype CPU budget: recipe {} bytes, used {used} of {PROTOTYPE_CPU} bytes, renderer {} bytes",
                bytes + owner_bytes,
                self.presentation.prototype_cpu,
            )));
        }
        let recipe = super::StructureRecipe::new(&self.reference, prototype)?;
        self.recipes.insert(source.clone(), recipe);
        Ok(())
    }

    /// Binds the recipes of renderer-ready structures that waited for prototype CPU room.
    fn bind_waiting_recipes(&mut self) -> Result<(), ReferenceError> {
        for index in 0..self.ready_structures.len() {
            let id = &self.ready_structures[index];
            let Some(source) = self
                .wanted_structures
                .iter()
                .find(|p| &p.id == id)
                .map(|p| &p.prototype)
                .filter(|source| !self.recipes.contains_key(*source))
            else {
                continue;
            };
            self.bind_structure_recipe(&source.clone())?;
        }
        Ok(())
    }
    /// True while the stream still wants the structure.
    pub fn structure_wanted(&self, request: WorldRequest, id: &super::GeneratedId) -> bool {
        request.session == self.request.session
            && self.wanted_structures.iter().any(|p| p.id == *id)
    }
    /// The renderer finished the structure. Its drained claim and credit move to the ready queue.
    fn accept_structure_renderer(
        &mut self,
        request: WorldRequest,
        id: &super::GeneratedId,
    ) -> Result<(), ReferenceError> {
        if !self.structure_wanted(request, id)
            || self.ready_structures.contains(id)
            || self.ready_structures.len() >= 32
        {
            return Err(ReferenceError(
                "Stale or live structure renderer claim".into(),
            ));
        }
        // The claim credit moves to the ready queue in one step.
        self.presentation.pending = self
            .presentation
            .pending
            .checked_sub(1)
            .ok_or_else(|| ReferenceError("Missing instance renderer claim".into()))?;
        self.ready_structures.push(id.clone());
        debug_assert!(self.metrics().pending_work <= 32);
        Ok(())
    }
    fn release_structure_renderer(&mut self, id: &super::GeneratedId) -> bool {
        let count = self.ready_structures.len();
        self.ready_structures.retain(|entry| entry != id);
        self.ready_structures.len() != count
    }
    fn publish_structure(
        &mut self,
        physics: &mut PhysicsWorld,
        required_lights: &[super::GeneratedId],
    ) -> Result<(), ReferenceError> {
        if self.plan_revision != self.request.revision {
            return Ok(());
        }
        self.discard_fixed_decoration_changes();
        self.bind_waiting_recipes()?;
        let request = self.request;
        let ready = |placement: &&Placement| {
            !self
                .structures
                .iter()
                .any(|instance| instance.placement.id == placement.id)
                && self.recipes.contains_key(&placement.prototype)
                && self.ready_structures.contains(&placement.id)
        };
        let Some(placement) = self
            .wanted_structures
            .iter()
            .filter(ready)
            .find(|placement| {
                self.lights.iter().any(|light| {
                    self.publication_light(light, required_lights) && light.owned_by(&placement.id)
                })
            })
            .or_else(|| {
                self.wanted_structures.iter().filter(ready).min_by(|a, b| {
                    focus_distance_squared(request, a.pose.position)
                        .total_cmp(&focus_distance_squared(request, b.pose.position))
                })
            })
        else {
            return Ok(());
        };
        let mut points = Vec::new();
        if placement.id.layer == WorldLayer::Building && placement.id.children.is_empty() {
            if self
                .placements
                .supplies_for_building(placement.id.unit[0] as u32, &mut points)?
                != super::QueryStatus::Complete
            {
                return Err(ReferenceError("Building supply prefix is preparing".into()));
            }
        }
        if self.metrics().generated_objects + 1 + points.len() * 2 > self.limits.objects {
            return Err(ReferenceError(format!(
                "Source object count exceeds {}",
                self.limits.objects
            )));
        }
        if self
            .metrics()
            .active_cpu_bytes
            .saturating_add(self.recipes[&placement.prototype].instance_bytes())
            .saturating_add(super::contents::placement_heap(placement))
            > self.limits.active_cpu
        {
            return Err(ReferenceError(
                "Source structure physics exceeds active CPU budget".into(),
            ));
        }
        let prototype = self
            .reference
            .prototypes
            .iter()
            .find(|p| p.source == placement.prototype)
            .unwrap();
        let fixed_decoration = self.fixed_decoration(placement);
        let (binding_reservation, binding_objects) =
            self.binding_reservation(prototype, &placement.id, fixed_decoration)?;
        // Renderer staging credit is transient. The publication waits for it to come back.
        if !self.wait_for_room(
            binding_reservation <= self.available_dependency_staging_bytes(),
            Reclaim::STAGING,
        ) {
            return Ok(());
        }
        if self
            .metrics()
            .active_cpu_bytes
            .saturating_add(binding_reservation)
            .saturating_add(self.recipes[&placement.prototype].instance_bytes())
            .saturating_add(super::contents::placement_heap(placement))
            > self.limits.active_cpu
            || self
                .metrics()
                .generated_objects
                .saturating_add(binding_objects)
                > self.limits.objects
        {
            return Err(ReferenceError(
                "Source generated bindings exceed admission budget".into(),
            ));
        }
        let appearance = self.appearance(&placement.id);
        let instance = if placement.scale == prototype.prefab_pose.scale
            && !super::instances::moves_colliders(prototype, appearance)
        {
            self.recipes[&placement.prototype].publish(
                placement.clone(),
                self.origin,
                fixed_decoration,
                physics,
                &mut self.trigger_roles,
            )?
        } else {
            let extra = super::StructureRecipe::reservation(prototype);
            if self.metrics().active_cpu_bytes.saturating_add(extra * 2) > self.limits.active_cpu
                || extra + binding_reservation > self.available_dependency_staging_bytes()
            {
                return Err(ReferenceError(
                    "Source scaled collision exceeds allocation budget".into(),
                ));
            }
            let recipe = super::StructureRecipe::with_appearance(
                &self.reference,
                prototype,
                placement.scale,
                appearance,
            )?;
            let mut instance = recipe.publish(
                placement.clone(),
                self.origin,
                fixed_decoration,
                physics,
                &mut self.trigger_roles,
            )?;
            instance.cpu_bytes += extra;
            instance
        };
        let appearance = self.appearance(&placement.id);
        for (index, body) in prototype.bodies.iter().enumerate() {
            physics.edit_body(instance.bodies[index], |state| {
                state.set_enabled(super::spawn_appearance::node_active(
                    prototype,
                    body.node as usize,
                    appearance,
                ))
            });
        }
        for (source, handle) in &instance.colliders {
            let collider = prototype
                .colliders
                .iter()
                .find(|c| &c.source == source)
                .unwrap();
            physics.edit_collider(*handle, |state| {
                state.set_enabled(
                    collider.enabled
                        && super::spawn_appearance::node_active(
                            prototype,
                            collider.node as usize,
                            appearance,
                        ),
                )
            });
        }
        for point in points {
            self.supplies.push(PublishedSupply {
                key: placement.id.clone(),
                point,
                definition: self.placements.supply_definition(&point).clone(),
                items: None,
                baselines: None,
            });
        }
        if fixed_decoration {
            self.forget_fixed_decoration_changes(&instance.placement.id);
            if self.change_revision != self.changes.revision() {
                self.select_changed_destinations();
            }
        }
        self.release_structure_renderer(&instance.placement.id);
        self.structures.push(instance);
        self.structure_revision = self.structure_revision.wrapping_add(1);
        self.binding_reserved = binding_reservation;
        self.publication_available = false;
        Ok(())
    }

    /// Call after publication and before movement or recovery. Repeated calls preserve stock and valves.
    #[allow(clippy::too_many_arguments)]
    pub fn sync_supplies(
        &mut self,
        reference: &ReferenceManifest,
        items: &mut crate::interaction::items::ItemStore,
        interaction: &mut crate::interaction::InteractionState,
        physics: &mut PhysicsWorld,
        wheels: &mut crate::interaction::wheels::WheelAssemblies,
        vehicles: &mut std::collections::BTreeMap<
            super::ObjectKey,
            crate::starting_area::VehicleRuntime,
        >,
        powertrain: &mut crate::simulation::powertrain::PowertrainParts,
    ) -> Result<(), ReferenceError> {
        let owned = items.retained_binding_bytes()
            + interaction.retained_binding_bytes()
            + wheels.retained_binding_bytes()
            + (physics.query_storage_bytes() + physics.presentation_retained_bytes());
        let baseline = *self.binding_baseline.get_or_insert(owned);
        self.binding_bytes = owned.saturating_sub(baseline);
        self.binding_owner_bytes = owned;
        self.binding_objects = items.len();
        if let Err(error) =
            self.sync_static_controls(reference, items, interaction, physics, wheels)
        {
            self.binding_reserved = 0;
            self.binding_owner_bytes = items.retained_binding_bytes()
                + interaction.retained_binding_bytes()
                + wheels.retained_binding_bytes()
                + (physics.query_storage_bytes() + physics.presentation_retained_bytes());
            self.binding_bytes = self.binding_owner_bytes.saturating_sub(baseline);
            return Err(error);
        }
        if let Err(error) = self.sync_contents(
            reference,
            items,
            interaction,
            physics,
            wheels,
            vehicles,
            powertrain,
        ) {
            self.binding_reserved = 0;
            self.binding_owner_bytes = items.retained_binding_bytes()
                + interaction.retained_binding_bytes()
                + wheels.retained_binding_bytes()
                + (physics.query_storage_bytes() + physics.presentation_retained_bytes());
            self.binding_bytes = self.binding_owner_bytes.saturating_sub(baseline);
            return Err(error);
        }
        if let Err(error) = self.sync_lamp_bindings(items, interaction, physics) {
            self.binding_reserved = 0;
            self.binding_owner_bytes = items.retained_binding_bytes()
                + interaction.retained_binding_bytes()
                + wheels.retained_binding_bytes()
                + (physics.query_storage_bytes() + physics.presentation_retained_bytes());
            self.binding_bytes = self.binding_owner_bytes.saturating_sub(baseline);
            return Err(error);
        }
        if let Err(error) = self.sync_dial_bindings(&reference.ui, items, interaction, physics) {
            self.binding_reserved = 0;
            return Err(error);
        }
        if let Err(error) = self.sync_small_items(reference, items, interaction, physics) {
            self.binding_reserved = 0;
            return Err(error);
        }
        if let Err(error) = self.sync_tools(reference, items, interaction) {
            self.binding_reserved = 0;
            return Err(error);
        }
        if let Err(error) = self.sync_cobwebs(reference, interaction, physics) {
            self.binding_reserved = 0;
            return Err(error);
        }
        if let Err(error) = self.sync_condition_appearance(items, interaction) {
            self.binding_reserved = 0;
            return Err(error);
        }
        if let Err(error) = self.sync_computers(reference, items, interaction, physics) {
            self.binding_reserved = 0;
            return Err(error);
        }
        if let Err(error) = self.sync_level_bindings(&reference.small_items.spirit_level, items) {
            self.binding_reserved = 0;
            return Err(error);
        }
        if let Err(error) = self.sync_radio_bindings(&reference.ui, items, interaction, physics) {
            self.binding_reserved = 0;
            return Err(error);
        }
        if let Err(error) = self.sync_music_box_bindings(items, interaction, physics) {
            self.binding_reserved = 0;
            return Err(error);
        }
        if let Err(error) = self.sync_detector_bindings(items, interaction, physics) {
            self.binding_reserved = 0;
            return Err(error);
        }
        if let Err(error) = self.sync_bus_door_bindings(interaction, physics) {
            self.binding_reserved = 0;
            return Err(error);
        }
        if let Err(error) = self.sync_seat_bindings(items, interaction, physics) {
            self.binding_reserved = 0;
            return Err(error);
        }
        if let Err(error) = self.sync_sled_bindings(items, physics) {
            self.binding_reserved = 0;
            return Err(error);
        }
        if let Err(error) = self.sync_trailer_bindings(items, interaction, physics) {
            self.binding_reserved = 0;
            return Err(error);
        }
        if let Err(error) = self.sync_follower_bindings(items) {
            self.binding_reserved = 0;
            return Err(error);
        }
        self.sync_moving_lamps(items);
        self.sync_generated_food(reference, items, physics);
        self.binding_bytes = (items.retained_binding_bytes()
            + interaction.retained_binding_bytes()
            + wheels.retained_binding_bytes()
            + (physics.query_storage_bytes() + physics.presentation_retained_bytes()))
        .saturating_sub(baseline);

        for supply in &mut self.supplies {
            if supply.items.is_some() {
                continue;
            }
            let instance = self
                .structures
                .iter()
                .find(|instance| instance.placement.id == supply.key)
                .ok_or_else(|| ReferenceError("Supply has no published structure".into()))?;
            let prototype = self
                .reference
                .prototypes
                .iter()
                .find(|prototype| prototype.source == instance.placement.prototype)
                .ok_or_else(|| ReferenceError("Supply has no prototype".into()))?;
            let mut tank_key = supply.key.clone();
            tank_key.children.push((supply.definition.tank.clone(), 0));
            let mut cap_key = supply.key.clone();
            cap_key.children.push((supply.definition.cap.clone(), 0));
            let tank =
                items.insert_generated(tank_key.clone(), prototype, supply.point.tank_pose)?;
            let cap = items.insert_generated(cap_key.clone(), prototype, supply.point.cap_pose)?;
            items.initialize_generated_supply(tank, &supply.definition)?;
            interaction.register_generated_supply(
                tank,
                cap,
                &supply.definition,
                &supply.point,
                reference,
            )?;
            supply.baselines = Some([
                items.generated_snapshot(tank, None).unwrap(),
                items
                    .generated_snapshot(cap, interaction.generated_valve(cap))
                    .unwrap(),
            ]);
            for (key, id) in [(tank_key, tank), (cap_key, cap)] {
                if let Some(change) = self
                    .changes
                    .get(super::ObjectKey::Generated(key))
                    .and_then(|c| c.snapshot.as_ref())
                {
                    items.restore_generated(id, change)?;
                    if let Some(valve) = change.valve {
                        interaction.restore_generated_valve(id, valve)?;
                    }
                }
            }
            let cap_record = self
                .reference
                .records
                .iter()
                .find(|record| record.source == supply.definition.cap)
                .ok_or_else(|| ReferenceError("Missing source cap owner".into()))?;
            let Some(crate::reference::WorldValue::Link(owner)) =
                cap_record.fields.get("m_GameObject")
            else {
                return Err(ReferenceError("Missing source cap GameObject".into()));
            };
            for (source, handle) in &instance.colliders {
                let record = self
                    .reference
                    .records
                    .iter()
                    .find(|record| &record.source == source)
                    .ok_or_else(|| ReferenceError("Missing source collider owner".into()))?;
                if matches!(record.fields.get("m_GameObject"), Some(crate::reference::WorldValue::Link(value)) if value == owner)
                {
                    physics.edit_collider(*handle, |collider| {
                        collider.user_data = (collider.user_data
                            & crate::vehicle::STEP_SURFACE_MASK)
                            | (cap.0 as u128 + 1);
                    });
                    interaction.bind_surface(
                        *handle,
                        crate::reference::ActionBinding {
                            collider: crate::reference::ColliderId(0),
                            use_owner: Some(cap),
                            pickup_owner: None,
                            removal_owner: None,
                            push_owner: None,
                        },
                    );
                }
            }
            supply.items = Some((tank, cap));
        }
        physics.refresh_queries();
        self.binding_owner_bytes = items.retained_binding_bytes()
            + interaction.retained_binding_bytes()
            + wheels.retained_binding_bytes()
            + (physics.query_storage_bytes() + physics.presentation_retained_bytes());
        self.binding_bytes = self.binding_owner_bytes.saturating_sub(baseline);
        self.binding_objects = items.len();
        self.binding_reserved = 0;
        // Published instances keep their item policy. Live locations now own retention.
        if !self.saved_locations_pending {
            self.saved_item_protection.retain(|key| {
                items.resolve_key(key).is_none_or(|item| {
                    self.contents
                        .active
                        .iter()
                        .chain(&self.static_controls)
                        .any(|c| {
                            !c.restored
                                && (c.item == Some(item)
                                    || c.bindings.iter().any(|b| b.item == item))
                        })
                })
            });
        }
        Ok(())
    }

    pub fn sync_generated_food(
        &self,
        reference: &ReferenceManifest,
        items: &crate::interaction::items::ItemStore,
        physics: &mut PhysicsWorld,
    ) {
        for content in &self.contents.active {
            let Some(item) = content.item.and_then(|id| items.get(id)) else {
                continue;
            };
            let Some(food) = item.food else { continue };
            let Some(instance) = self
                .structures
                .iter()
                .find(|i| i.placement.id == content.content.placement.id)
            else {
                continue;
            };
            let recipe = &reference.survival.foods[food.prototype_index];
            for (source, handle) in &instance.colliders {
                if let Some(collider) = recipe.colliders.iter().find(|c| &c.source == source) {
                    physics.edit_collider(*handle, |state| {
                        state.set_enabled(
                            content.restored
                                && !item.removed
                                && collider.enabled
                                && crate::starting_area::active_food_part(
                                    reference,
                                    collider.node,
                                    &recipe.parts,
                                    food.bite,
                                ),
                        )
                    });
                }
            }
        }
    }
    pub fn changes(&self) -> &super::ChangeRecords {
        &self.changes
    }

    /// Retire only after the replacement collision windows support both receivers.
    pub fn retire_structures(
        &mut self,
        physics: &mut PhysicsWorld,
        items: &mut crate::interaction::items::ItemStore,
        interaction: &mut crate::interaction::InteractionState,
        wheels: &mut crate::interaction::wheels::WheelAssemblies,
        vehicles: &mut std::collections::BTreeMap<
            super::ObjectKey,
            crate::starting_area::VehicleRuntime,
        >,
        powertrain: &mut crate::simulation::powertrain::PowertrainParts,
    ) -> Result<(), ReferenceError> {
        if self.recovery.is_some()
            || self.plan_revision != self.request.revision
            || !self.supports(window(self.request.player, 3.0))
            || !self.supports(window(self.request.vehicle, 4.0))
        {
            return Ok(());
        }
        self.retire_contents(physics, items, interaction, wheels, vehicles, powertrain)?;
        let mut index = 0;
        while index < self.structures.len() {
            let id = &self.structures[index].placement.id;
            let radius = self.distances.building_gen_dist;
            if self.contents.contains(id)
                || self
                    .static_controls
                    .iter()
                    .filter(|c| &c.content.placement.id == id)
                    .any(|c| c.has_external_dependents(items, &interaction.parts, wheels))
                || (!id.children.is_empty() && self.wanted_structures.iter().any(|p| &p.id == id))
                || (id.children.is_empty()
                    && id.layer != WorldLayer::Building
                    && self.props.contains(id))
                || (id.children.is_empty()
                    && id.layer == WorldLayer::Building
                    && [self.request.player, self.request.vehicle]
                        .into_iter()
                        .any(|p| {
                            source_distance_squared(
                                p,
                                self.structures[index].placement.pose.position,
                            ) < radius * radius
                        }))
                || self.supplies.iter().filter(|s| &s.key == id).any(|s| {
                    s.items.is_some_and(|(tank, cap)| {
                        !items.can_remove_generated(tank)
                            || !items.can_remove_generated(cap)
                            || items.has_external_dependents(|item| item == tank || item == cap)
                            || interaction
                                .parts
                                .has_external_dependents(|item| item == tank || item == cap)
                            || wheels.has_external_dependents(|item| item == tank || item == cap)
                    })
                })
            {
                index += 1;
                continue;
            }
            let instance = self.structures.swap_remove(index);
            self.structure_revision = self.structure_revision.wrapping_add(1);
            if let Some(i) = self
                .static_controls
                .iter()
                .position(|c| c.content.placement.id == instance.placement.id)
            {
                let mut content = self.static_controls.swap_remove(i);
                self.static_control_revision += 1;
                if let Some(item) = content.item {
                    content_bindings::unbind_storage(
                        item,
                        &self.contents.active,
                        items,
                        interaction,
                        physics,
                        wheels,
                    )?;
                }
                content_bindings::rollback(
                    &mut content,
                    &instance,
                    items,
                    interaction,
                    physics,
                    wheels,
                );
            }
            let mut supply_index = 0;
            while supply_index < self.supplies.len() {
                if self.supplies[supply_index].key != instance.placement.id {
                    supply_index += 1;
                    continue;
                }
                let supply = self.supplies.swap_remove(supply_index);
                if let Some((tank, cap)) = supply.items {
                    for item in [tank, cap].into_iter() {
                        for (_, collider) in &instance.colliders {
                            interaction.unbind_generated(item, std::slice::from_ref(collider));
                        }
                        items.remove_generated(item);
                    }
                }
            }
            instance.retire(physics, &mut self.trigger_roles);
            break;
        }
        Ok(())
    }

    /// A recovery target selects a loading window, never a final chassis pose.
    pub fn recovery_ready(&self, pose: AbsolutePose, require_supply: bool) -> bool {
        let Some(recovery) = &self.recovery else {
            return false;
        };
        recovery.target == pose
            && same_request(recovery.request, self.request)
            && self.failure.is_none()
            && self.next_road == self.road_candidates.len()
            && self.plan_revision == self.request.revision
            && self.supports(window(pose.position, 24.0))
            && (!require_supply
                || recovery.supply.as_ref().is_some_and(|owner| {
                    self.supplies.iter().any(|supply| {
                        (supply.point.building_index, supply.point.supply_index) == *owner
                            && supply.items.is_some()
                    })
                }))
    }

    /// Height of published terrain or road at a local point, cast from far above.
    pub fn ground_height(&self, physics: &PhysicsWorld, point: [f32; 3]) -> Option<f32> {
        use rapier3d::math::Vector;
        use rapier3d::prelude::{QueryFilter, Ray};
        let ray = Ray::new(
            Vector::new(point[0], point[1] + 1000., point[2]),
            -Vector::Y,
        );
        let ground = |handle: ColliderHandle, _: &rapier3d::prelude::Collider| {
            self.surfaces
                .iter()
                .any(|surface| surface.collider == Some(handle))
        };
        physics
            .queries(QueryFilter::default().predicate(&ground))
            .cast_ray(&ray, 2000., true)
            .map(|(_, distance)| ray.origin.y - distance)
    }
    /// Search eight published road controls per update. No source index replay occurs here.
    pub fn prepare_recovery_pose(
        &mut self,
        physics: &PhysicsWorld,
        mut clearance: impl FnMut(&PhysicsWorld, AbsolutePose) -> bool,
    ) -> Result<Option<AbsolutePose>, ReferenceError> {
        use rapier3d::math::{Pose, Rotation, Vector};
        use rapier3d::prelude::{QueryFilter, Ray};
        if let Some((point, prefer_supply)) = self.recovery_search {
            self.recovery_target(point, prefer_supply)?;
        }
        let Some(mut pending) = self.recovery else {
            return Ok(None);
        };
        if !self.recovery_ready(pending.target, pending.supply.is_some()) {
            return Ok(None);
        }
        let mut cursor = 0;
        let mut work = 0;
        let ground = |handle: ColliderHandle, _: &rapier3d::prelude::Collider| {
            self.surfaces
                .iter()
                .any(|surface| surface.collider == Some(handle))
        };
        for surface in &self.surfaces {
            let Some(road) = surface.road else {
                continue;
            };
            for (index, control) in road.source_coords.iter().enumerate() {
                cursor += 1;
                if cursor <= pending.candidate {
                    continue;
                }
                if work == 8 {
                    self.recovery = Some(pending);
                    return Ok(None);
                }
                pending.candidate = cursor;
                work += 1;
                let position = [
                    road.source_origin[0] + road.source_position[0] + control[0] as f64,
                    road.source_origin[1] + road.source_position[1] + control[1] as f64,
                    -(road.source_origin[2] + road.source_position[2] + control[2] as f64),
                ];
                if !self.supports(window(position, 7.0)) {
                    continue;
                }
                let distance = (0..3)
                    .map(|axis| (position[axis] - pending.target.position[axis]).powi(2))
                    .sum::<f64>();
                if pending.best.is_some_and(|(best, _)| distance >= best) {
                    continue;
                }
                let previous = road.source_coords[index.saturating_sub(1)];
                let next = road.source_coords[(index + 1).min(road.source_coords.len() - 1)];
                let rotation = Rotation::from_rotation_y(
                    (-(next[0] - previous[0])).atan2(next[2] - previous[2]),
                );
                let local = Vector::from_array(self.origin.local(position));
                let ray = Ray::new(local + Vector::Y * 4.0, -Vector::Y);
                let Some((_, height)) = physics
                    .queries(QueryFilter::default().exclude_sensors().predicate(&ground))
                    .cast_ray(&ray, 8.0, true)
                else {
                    continue;
                };
                let pose =
                    Pose::from_parts(ray.origin + ray.dir * height + Vector::Y * 1.1, rotation);
                let absolute = AbsolutePose {
                    position: self.origin.absolute(pose.translation.to_array()),
                    rotation: pose.rotation.to_array(),
                };
                let clear = clearance(physics, absolute);
                if clear {
                    pending.best = Some((
                        distance,
                        AbsolutePose {
                            position: self.origin.absolute(pose.translation.to_array()),
                            rotation: pose.rotation.to_array(),
                        },
                    ));
                }
            }
        }
        self.recovery = Some(pending);
        pending
            .best
            .map(|(_, pose)| Some(pose))
            .ok_or_else(|| ReferenceError("No clear supported source recovery pose".into()))
    }

    /// Check all destination support before the caller can mutate transported bodies.
    pub fn preflight_recovery(
        &self,
        target: AbsolutePose,
        player: [f64; 3],
        vehicle: [f64; 3],
    ) -> Result<RecoveryPermit, ReferenceError> {
        if self.recovery.as_ref().is_none_or(|pending| {
            pending.best.is_none_or(|(_, pose)| pose != target)
                || !same_request(pending.request, self.request)
        }) {
            return Err(ReferenceError("Stale source recovery destination".into()));
        }
        if !self.supports(window(player, 3.0)) || !self.supports(window(vehicle, 4.0)) {
            return Err(ReferenceError(
                "Source recovery destination is unsupported".into(),
            ));
        }
        Ok(RecoveryPermit {
            request: self.request,
            player,
            vehicle,
        })
    }
    /// Validate the permit, transport once, then release pinned support without another fallible check.
    pub fn finish_recovery(
        &mut self,
        permit: RecoveryPermit,
        transport: impl FnOnce(),
    ) -> Result<(), ReferenceError> {
        if self.recovery.is_none() || !same_request(permit.request, self.request) {
            return Err(ReferenceError("Stale source recovery permit".into()));
        }
        transport();
        self.recovery = None;
        self.request_at(permit.player, permit.vehicle);
        Ok(())
    }

    pub fn fail(&mut self, error: ReferenceError) {
        self.failure = Some(error.0);
    }

    /// Changes when a structure is published or retired.
    pub fn structure_revision(&self) -> u64 {
        self.structure_revision
    }
    /// Session, published structure revision and pending structure revision.
    /// Changes when a structure is added to or removed from either list.
    pub fn structure_membership(&self) -> (u64, u64, u64) {
        (
            self.request.session,
            self.structure_revision,
            self.wanted_structures.revision(),
        )
    }
    pub fn pending_structures(&self) -> &[Placement] {
        &self.wanted_structures
    }

    /// Rebase between physics steps. Pending geometry stays in absolute coordinates.
    pub fn rebase(&mut self, position: [f32; 3], physics: &mut PhysicsWorld) -> Option<[f32; 3]> {
        if position[0].abs().max(position[2].abs()) <= super::REBASE_LIMIT {
            return None;
        }
        let shift = [
            (position[0] / 128.0).round() * 128.0,
            0.0,
            (position[2] / 128.0).round() * 128.0,
        ];
        physics.rebase(shift);
        self.origin.shift(shift);
        Some(shift)
    }

    pub fn trigger_roles(&self) -> &super::TriggerRoles {
        &self.trigger_roles
    }
    pub fn trigger_roles_mut(&mut self) -> &mut super::TriggerRoles {
        &mut self.trigger_roles
    }

    pub fn frame(&self) -> WorldFrame<'_> {
        WorldFrame {
            request: self.request,
            origin: self.origin,
            surfaces: &self.surfaces,
            lights: &self.lights,
            structures: &self.structures,
            supplies: &self.supplies,
            startup: self.startup.as_ref(),
            failure: self.failure.as_deref(),
        }
    }
    /// Bounded live counts. Does not walk spawn history, asset payloads, or change records.
    pub fn performance_counters(&self) -> super::metrics::WorldPerformance {
        let mut counters = super::metrics::WorldPerformance {
            objects: self
                .contents
                .active
                .iter()
                .chain(&self.static_controls)
                .map(|c| c.bindings.len() + usize::from(c.item.is_some()))
                .sum::<usize>()
                + self.structures.len()
                + self.supplies.len() * 2
                + self.lights.len()
                + self.presentation.objects,
            jobs: self.presentation.jobs
                + usize::from(self.terrain_execution.has_pending())
                + usize::from(self.road_work.is_some())
                + usize::from(self.contents.has_replay()),
            pending: self.pending_work(),
            render_cpu_bytes: self.presentation.active_cpu + self.presentation.prototype_cpu,
            render_gpu_bytes: self.presentation.active_gpu + self.presentation.prototype_gpu,
            ..Default::default()
        };
        for surface in &self.surfaces {
            match surface.key.layer {
                WorldLayer::Road => counters.roads += 1,
                WorldLayer::Terrain | WorldLayer::DistantTerrain | WorldLayer::DistantTerrain2 => {
                    counters.terrain += 1;
                }
                _ => {}
            }
            counters.render_cpu_bytes += surface_render_cpu(&surface.surface);
            counters.render_gpu_bytes += surface_render_bytes(&surface.surface);
        }
        counters
    }

    pub fn metrics(&self) -> WorldMetrics {
        let imported_snapshot_bytes = self.imported_baselines.capacity()
            * std::mem::size_of::<(crate::reference::ItemId, super::ObjectSnapshot)>()
            + self
                .imported_baselines
                .iter()
                .map(|(_, s)| s.heap_bytes())
                .sum::<usize>();
        let mut metrics = WorldMetrics {
            road_worker_wasm_bytes: self.road_worker.wasm_bytes(),
            road_worker_reserved_bytes: self.road_worker_reserved_bytes,
            grass_worker_wasm_bytes: self.presentation.grass_worker_wasm_bytes,
            grass_worker_cache_bytes: self.presentation.grass_worker_cache_bytes,
            grass_worker_reserved_bytes: self.presentation.grass_worker_reserved_bytes,
            road_worker_cache_bytes: self.road_worker.cache_bytes(),
            imported_snapshot_bytes,
            road_checkpoints: self.roads.checkpoint_count(),
            building_checkpoints: self.placements.checkpoint_count(),
            road_replay_ms: self.roads.replay_ms(),
            peak_road_replay_ms: self.roads.peak_replay_ms(),
            generation_jobs: self.presentation.jobs
                + usize::from(self.terrain_execution.has_pending())
                + usize::from(self.road_work.is_some())
                + usize::from(self.contents.has_replay()),
            pending_bytes: self.presentation.staging_bytes + self.generation_pending_bytes(),
            pending_work: self.pending_work(),
            spawn_parents: self.contents.state.parent_count(),
            spawn_boundaries: self.contents.state.boundary_count(),
            spawn_boundary_bytes: self.contents.state.retained_bytes(),
            checkpoint_bytes: self.contents.state.retained_bytes()
                + self.roads.retained_index_bytes()
                + self.placements.retained_index_bytes(),
            prototype_cpu_bytes: super::metrics::map_bytes(
                self.prototype_holds.capacity(),
                std::mem::size_of::<(SourceId, bool)>(),
            ) + self
                .prototype_holds
                .keys()
                .map(|id| id.file.capacity())
                .sum::<usize>()
                + super::metrics::map_bytes(
                    self.recipes.capacity(),
                    std::mem::size_of::<(SourceId, super::StructureRecipe)>(),
                )
                + self
                    .recipes
                    .keys()
                    .map(|id| id.file.capacity())
                    .sum::<usize>()
                + self
                    .road_prototypes
                    .iter()
                    .flatten()
                    .map(road_mesh::RoadPrototype::retained_bytes)
                    .sum::<usize>()
                + self.road_light.retained_bytes()
                + self.cables.retained_bytes()
                + self.contents.rules.retained_bytes()
                + self.building_components.retained_bytes()
                + self.structure_helpers.retained_bytes()
                + self.shared_cpu_bytes.unwrap_or(0)
                + self.presentation.prototype_cpu
                + self
                    .recipes
                    .values()
                    .map(|recipe| recipe.cpu_bytes)
                    .sum::<usize>(),
            prototype_gpu_bytes: self.presentation.prototype_gpu,
            active_gpu_bytes: self.presentation.active_gpu,
            changed_record_bytes: self.changes.retained_bytes(),
            changed_objects: self.changes.len(),
            active_cpu_bytes: std::mem::size_of::<Self>()
                + self.road_worker_reserved_bytes
                + self.road_worker.cache_bytes()
                + self.required.roads.capacity() * std::mem::size_of::<u32>()
                + self.required.terrain.capacity() * std::mem::size_of::<SurfaceKey>()
                + (self.required.structures.capacity() + self.required.lights.capacity())
                    * std::mem::size_of::<super::GeneratedId>()
                + self
                    .required
                    .structures
                    .iter()
                    .chain(&self.required.lights)
                    .map(super::contents::identity_heap)
                    .sum::<usize>()
                + imported_snapshot_bytes
                + self.replay_roots.capacity() * std::mem::size_of::<Placement>()
                + self
                    .replay_roots
                    .iter()
                    .map(super::contents::placement_heap)
                    .sum::<usize>()
                + (self.replay_indices.capacity() + self.replay_query.capacity())
                    * std::mem::size_of::<super::changes::ChangeRecordIndex>()
                + self.static_controls.capacity()
                    * std::mem::size_of::<super::contents::ContentInstance>()
                + self
                    .static_controls
                    .iter()
                    .map(super::contents::ContentInstance::control_heap_bytes)
                    .sum::<usize>()
                + self.binding_reserved
                + self.binding_bytes
                + self.presentation_changes.capacity()
                    * std::mem::size_of::<content_index::ContentSlot>()
                + self.presentation_lamp_changes.capacity() * std::mem::size_of::<usize>()
                + self.lamp_bindings.capacity() * std::mem::size_of::<content_lamps::LampBinding>()
                + self
                    .lamp_bindings
                    .iter()
                    .map(content_lamps::LampBinding::heap_bytes)
                    .sum::<usize>()
                + self.dial_bindings.capacity() * std::mem::size_of::<content_dials::DialBinding>()
                + self
                    .dial_bindings
                    .iter()
                    .map(content_dials::DialBinding::heap_bytes)
                    .sum::<usize>()
                + self.small_items.heap_bytes()
                + self.computers.heap_bytes()
                + self.cobwebs.heap_bytes()
                + self.level_bindings.capacity()
                    * std::mem::size_of::<content_levels::LevelBinding>()
                + self
                    .level_bindings
                    .iter()
                    .map(content_levels::LevelBinding::heap_bytes)
                    .sum::<usize>()
                + self.radio_bindings.capacity()
                    * std::mem::size_of::<content_radios::RadioBinding>()
                + self
                    .radio_bindings
                    .iter()
                    .map(content_radios::RadioBinding::heap_bytes)
                    .sum::<usize>()
                + self.radio_scripts.capacity()
                    * std::mem::size_of::<(SourceId, Option<SourceId>)>()
                + self.music_box_bindings.capacity()
                    * std::mem::size_of::<content_musicboxes::MusicBoxBinding>()
                + self
                    .music_box_bindings
                    .iter()
                    .map(content_musicboxes::MusicBoxBinding::heap_bytes)
                    .sum::<usize>()
                + self.seat_bindings.capacity() * std::mem::size_of::<content_seats::SeatBinding>()
                + self
                    .seat_bindings
                    .iter()
                    .map(content_seats::SeatBinding::heap_bytes)
                    .sum::<usize>()
                + self.seat_prototypes.capacity()
                    * std::mem::size_of::<content_seats::PrototypeSeats>()
                + content_seats::prototype_seats_heap(&self.seat_prototypes)
                + self.sled_bindings.capacity() * std::mem::size_of::<content_sleds::SledBinding>()
                + self
                    .sled_bindings
                    .iter()
                    .map(content_sleds::SledBinding::heap_bytes)
                    .sum::<usize>()
                + self.sled_prototypes.capacity() * std::mem::size_of::<usize>()
                + self.trailer_bindings.capacity()
                    * std::mem::size_of::<content_trailers::TrailerBinding>()
                + self
                    .trailer_bindings
                    .iter()
                    .map(content_trailers::TrailerBinding::heap_bytes)
                    .sum::<usize>()
                + self.hitch_owners.capacity()
                    * std::mem::size_of::<(crate::world::GeneratedId, crate::reference::ItemId)>()
                + self
                    .hitch_owners
                    .iter()
                    .map(|(id, _)| crate::world::contents::identity_heap(id))
                    .sum::<usize>()
                + self.trailer_prototypes.capacity() * std::mem::size_of::<usize>()
                + self.follower_bindings.capacity()
                    * std::mem::size_of::<content_followers::FollowerBinding>()
                + self
                    .follower_bindings
                    .iter()
                    .map(content_followers::FollowerBinding::heap_bytes)
                    .sum::<usize>()
                + self.follower_prototypes.capacity() * std::mem::size_of::<usize>()
                + self.tools.heap_bytes()
                + self.detector_bindings.capacity()
                    * std::mem::size_of::<content_detectors::DetectorBinding>()
                + self
                    .detector_bindings
                    .iter()
                    .map(content_detectors::DetectorBinding::heap_bytes)
                    .sum::<usize>()
                + self.bus_door_bindings.capacity()
                    * std::mem::size_of::<content_busdoors::BusDoorBinding>()
                + self
                    .bus_door_bindings
                    .iter()
                    .map(content_busdoors::BusDoorBinding::heap_bytes)
                    .sum::<usize>()
                + self.bus_door_scripts.capacity()
                    * std::mem::size_of::<(SourceId, Option<SourceId>, Vec<SourceId>)>()
                + self.contents.retained_bytes()
                + self.props.retained_bytes()
                + self.presentation.active_cpu
                + self.color_foundations.capacity() * std::mem::size_of::<super::FoundationPoint>()
                + self.color_helpers.capacity() * std::mem::size_of::<TerrainFoundation>()
                + self
                    .structures
                    .iter()
                    .map(|instance| instance.cpu_bytes)
                    .sum::<usize>()
                + self.ready_structures.capacity() * std::mem::size_of::<super::GeneratedId>()
                + self
                    .ready_structures
                    .iter()
                    .map(super::contents::identity_heap)
                    .sum::<usize>()
                + self.selected_roads.capacity() * std::mem::size_of::<SelectedRoad>()
                + self
                    .selected_roads
                    .iter()
                    .filter_map(|r| r.light.as_ref())
                    .map(super::WorldLight::heap_bytes)
                    .sum::<usize>()
                + self
                    .selected_roads
                    .iter()
                    .map(|r| {
                        r.cable.capacity() * std::mem::size_of::<super::NodeAppearance>()
                            + r.cable
                                .iter()
                                .map(super::NodeAppearance::heap_bytes)
                                .sum::<usize>()
                    })
                    .sum::<usize>()
                + self.building_cables.capacity() * std::mem::size_of::<CableEntry>()
                + self
                    .building_cables
                    .iter()
                    .map(cable_entry_heap)
                    .sum::<usize>()
                + self.cable_search.retained_bytes()
                + self.home_cable.retained_bytes()
                + self.road_candidates.capacity() * std::mem::size_of::<u32>()
                + self.wanted_roads.capacity() * std::mem::size_of::<u32>()
                + self.wanted_lights.capacity() * std::mem::size_of::<u32>()
                + self.wanted_tiles.capacity() * std::mem::size_of::<SurfaceKey>()
                + self.road_points.capacity() * std::mem::size_of::<TerrainRoadPoint>()
                + self.startup_candidates.capacity() * std::mem::size_of::<u32>()
                + self.startup_points.capacity() * std::mem::size_of::<TerrainRoadPoint>()
                + self.surfaces.capacity() * std::mem::size_of::<PublishedSurface>()
                + self.lights.capacity() * std::mem::size_of::<super::WorldLight>()
                + self
                    .lights
                    .iter()
                    .map(super::WorldLight::heap_bytes)
                    .sum::<usize>()
                + self.structures.capacity() * std::mem::size_of::<super::StructureInstance>()
                + self.saved_item_protection.capacity() * std::mem::size_of::<super::ObjectKey>()
                + self
                    .saved_item_protection
                    .iter()
                    .map(super::changes::key_heap)
                    .sum::<usize>()
                + self.supplies.capacity() * std::mem::size_of::<PublishedSupply>()
                + self.wanted_structures.capacity() * std::mem::size_of::<Placement>()
                + self
                    .wanted_structures
                    .iter()
                    .map(super::contents::placement_heap)
                    .sum::<usize>(),
            prepared_roads: self.roads.prepared_segments(),
            prepared_buildings: self.placements.prepared_buildings(),

            last_road_work: self.last_road_work,
            generated_objects: self
                .contents
                .active
                .iter()
                .chain(&self.static_controls)
                .map(|c| c.bindings.len() + usize::from(c.item.is_some()))
                .sum::<usize>()
                + self
                    .contents
                    .active
                    .iter()
                    .filter(|c| {
                        c.item.is_none()
                            && !self
                                .wanted_structures
                                .iter()
                                .any(|p| p.id == c.content.placement.id)
                    })
                    .count()
                + self.structures.len()
                + self
                    .wanted_structures
                    .iter()
                    .filter(|p| !self.structures.iter().any(|s| s.placement.id == p.id))
                    .count()
                + self.supplies.len() * 2
                + self.lights.len()
                + self.presentation.objects,
            ..WorldMetrics::default()
        };
        for surface in &self.surfaces {
            match surface.key.layer {
                WorldLayer::Road => metrics.road_segments += 1,
                WorldLayer::Terrain => metrics.near_tiles += 1,
                WorldLayer::DistantTerrain => metrics.distant_tiles[0] += 1,
                WorldLayer::DistantTerrain2 => metrics.distant_tiles[1] += 1,
                _ => {}
            }
            let bytes = surface_bytes(&surface.surface)
                + surface
                    .prototype
                    .as_ref()
                    .map_or(0, |id| id.file.capacity());
            let render_bytes = surface_render_bytes(&surface.surface);
            metrics.active_cpu_bytes +=
                bytes + surface_render_cpu(&surface.surface) + surface.collision_bytes;
            metrics.active_gpu_bytes += render_bytes;
        }
        metrics.active_cpu_bytes += self.failure.as_ref().map_or(0, String::capacity);
        for supply in &self.supplies {
            metrics.active_cpu_bytes += super::contents::identity_heap(&supply.key)
                + supply.definition.tank.file.capacity()
                + supply.definition.cap.file.capacity()
                + supply.definition.fluids.capacity() * std::mem::size_of::<super::SupplyFluid>()
                + supply.baselines.as_ref().map_or(0, |values| {
                    values
                        .iter()
                        .map(super::ObjectSnapshot::heap_bytes)
                        .sum::<usize>()
                });
        }
        if let Some(work) = &self.ready {
            metrics.active_cpu_bytes += work.pending_bytes
                + work
                    .published
                    .prototype
                    .as_ref()
                    .map_or(0, |id| id.file.capacity());
            if work.offered {
                metrics.active_cpu_bytes += surface_render_cpu(&work.published.surface);
                metrics.active_gpu_bytes += surface_render_bytes(&work.published.surface);
            }
        }
        metrics.active_cpu_bytes += self.terrain_execution.pending_bytes();
        if let Some(work) = &self.road_work {
            metrics.active_cpu_bytes += work.pending_bytes;
        }
        if let Some(startup) = &self.startup {
            metrics.active_cpu_bytes += startup.foundations.capacity()
                * std::mem::size_of::<TerrainFoundation>()
                + startup.holes.capacity() * std::mem::size_of::<[[f32; 3]; 4]>()
                + startup.hole_centers.capacity() * std::mem::size_of::<[f32; 3]>()
                + startup.root_offsets.capacity() * std::mem::size_of::<(SourceId, f32)>()
                + startup
                    .root_offsets
                    .iter()
                    .map(|(id, _)| id.file.capacity())
                    .sum::<usize>();
        }
        metrics
    }
}

fn same_request(a: WorldRequest, b: WorldRequest) -> bool {
    a.session == b.session && a.revision == b.revision
}
fn tier_layer(tier: usize) -> WorldLayer {
    match tier {
        0 => WorldLayer::Terrain,
        1 => WorldLayer::DistantTerrain,
        _ => WorldLayer::DistantTerrain2,
    }
}
fn layer_tier(layer: WorldLayer) -> usize {
    match layer {
        WorldLayer::Terrain => 0,
        WorldLayer::DistantTerrain => 1,
        WorldLayer::DistantTerrain2 => 2,
        _ => panic!("terrain layer"),
    }
}
fn window(point: [f64; 3], radius: f64) -> WorldBounds {
    WorldBounds {
        min: [point[0] - radius, f64::NEG_INFINITY, point[2] - radius],
        max: [point[0] + radius, f64::INFINITY, point[2] + radius],
    }
}
fn union(a: WorldBounds, b: WorldBounds) -> WorldBounds {
    WorldBounds {
        min: std::array::from_fn(|i| a.min[i].min(b.min[i])),
        max: std::array::from_fn(|i| a.max[i].max(b.max[i])),
    }
}
fn intersects(a: WorldBounds, b: WorldBounds) -> bool {
    (0..3).all(|i| a.min[i] <= b.max[i] && a.max[i] >= b.min[i])
}
fn surface_bounds(surface: &WorldSurface) -> WorldBounds {
    let mut bounds = WorldBounds {
        min: [f64::INFINITY; 3],
        max: [f64::NEG_INFINITY; 3],
    };
    for p in &surface.vertices {
        for a in 0..3 {
            let v = surface.origin[a] + p[a] as f64;
            bounds.min[a] = bounds.min[a].min(v);
            bounds.max[a] = bounds.max[a].max(v);
        }
    }
    bounds
}
fn surface_bytes(surface: &WorldSurface) -> usize {
    surface.vertices.capacity() * 12
        + surface.normals.capacity() * 12
        + surface.tangents.capacity() * 16
        + surface.uv.capacity() * 8
        + surface.colors.capacity() * 16
        + surface.triangles.capacity() * 12
        + surface.submeshes.capacity() * std::mem::size_of::<std::ops::Range<usize>>()
}

// Bevy retains one vertex buffer for each material range. Casters share those handles.
fn surface_render_cpu(surface: &WorldSurface) -> usize {
    surface_render_bytes(surface) * 2 + surface.submeshes.len() * 4096
}
fn surface_render_bytes(surface: &WorldSurface) -> usize {
    let attributes = surface.vertices.len() * (12 + 32)
        + surface.normals.len() * 12
        + surface.tangents.len() * 16
        + surface.uv.len() * 8
        + surface.colors.len() * 16;
    attributes
        .saturating_mul(surface.submeshes.len())
        .saturating_add(surface.triangles.len() * 12)
        .saturating_add(surface.submeshes.len() * 8)
}

fn append_road_points(output: &mut Vec<TerrainRoadPoint>, segment: RoadSegment, range: f32) {
    for point in segment.source_coords {
        output.push(TerrainRoadPoint {
            position: std::array::from_fn(|axis| {
                (segment.source_origin[axis] + segment.source_position[axis] + point[axis] as f64)
                    as f32
            }),
            segment_origin: [
                segment.source_position[0] as f32,
                segment.source_position[2] as f32,
            ],
            range,
        });
    }
}

fn focus_distance_squared(request: WorldRequest, position: [f64; 3]) -> f32 {
    source_distance_squared(request.player, position)
        .min(source_distance_squared(request.vehicle, position))
}
// Frozen source coordinates, with source float vector boundaries.
fn source_distance_squared(a: [f64; 3], b: [f64; 3]) -> f32 {
    let x = a[0] as f32 - b[0] as f32;
    let z = (-a[2]) as f32 - (-b[2]) as f32;
    x * x + z * z
}

#[path = "content_bindings.rs"]
pub mod content_bindings;

#[path = "content_boxes.rs"]
pub mod content_boxes;
#[path = "content_breakables.rs"]
mod content_breakables;

#[path = "content_index.rs"]
pub mod content_index;

#[path = "content_busdoors.rs"]
mod content_busdoors;
#[path = "content_detectors.rs"]
mod content_detectors;
#[path = "content_dials.rs"]
mod content_dials;
#[path = "content_followers.rs"]
pub mod content_followers;
#[path = "content_musicboxes.rs"]
mod content_musicboxes;
#[path = "content_radios.rs"]
mod content_radios;
#[path = "content_seats.rs"]
pub mod content_seats;
#[path = "content_sleds.rs"]
mod content_sleds;
#[path = "content_trailers.rs"]
pub mod content_trailers;

#[path = "content_small_items.rs"]
pub mod content_small_items;

#[path = "content_tools.rs"]
pub mod content_tools;

#[path = "content_ammo.rs"]
mod content_ammo;

#[path = "content_explosives.rs"]
pub mod content_explosives;

#[path = "content_computers.rs"]
pub mod content_computers;

#[path = "content_lamps.rs"]
mod content_lamps;

#[path = "content_cobwebs.rs"]
mod content_cobwebs;

#[path = "content_conditions.rs"]
pub(super) mod content_conditions;

#[path = "content_levels.rs"]
mod content_levels;

#[path = "moving_nodes.rs"]
mod moving_nodes;
#[path = "renderer_world.rs"]
pub mod renderer_world;

#[path = "pending_structures.rs"]
mod pending_structures;

#[cfg(test)]
#[path = "stream_tests.rs"]
pub mod tests;

impl Drop for WorldStream {
    fn drop(&mut self) {
        // Terminate before Rust drops source caches, job storage, or reservation ownership.
        self.road_worker.dispose();
    }
}
type CableEntry = (super::GeneratedId, Vec<super::NodeAppearance>);
fn cable_entry_bytes(entry: &CableEntry) -> usize {
    std::mem::size_of::<CableEntry>() + cable_entry_heap(entry)
}
fn cable_entry_heap(entry: &CableEntry) -> usize {
    super::contents::identity_heap(&entry.0) + super::contents::appearance_bytes(&entry.1)
}
/// Lamp and radio on/off materials of each node in `prototype`, in one hierarchy walk.
pub(super) fn lamp_materials_of(
    rules: &super::spawn_rules::SourceRules,
    world: &WorldReference,
    prototype: &SourceId,
) -> Result<HashMap<SourceId, Arc<[SourceId]>>, ReferenceError> {
    let mut output: HashMap<SourceId, Vec<SourceId>> = HashMap::new();
    let mut add = |node: &SourceId, material: &SourceId| {
        let list = output.entry(node.clone()).or_default();
        if !list.contains(material) {
            list.push(material.clone());
        }
    };
    use super::spawn_rules::*;
    let mut controllers = Vec::new();
    rules.children(world, prototype, "flashlightscript", true, &mut controllers)?;
    let mut roots = Vec::new();
    for source in controllers {
        let f = rules.get(world, source)?.fields;
        for lamp in array(f, "Lamps")? {
            let lamp = fields(lamp)?;
            roots.clear();
            for r in array(lamp, "R")? {
                if let Some(root) = value_link(r).ok().and_then(|r| rules.root(world, r).ok()) {
                    if !roots.contains(&root) {
                        roots.push(root);
                    }
                }
            }
            if roots.is_empty() {
                continue;
            }
            for field in ["mON", "mOFF"] {
                if let Some(material) = optional(lamp, field)?.or(optional(f, field)?) {
                    for node in &roots {
                        add(node, material);
                    }
                }
            }
        }
    }
    let mut radios = Vec::new();
    rules.children(world, prototype, "radioscript", true, &mut radios)?;
    for source in radios {
        let f = rules.get(world, source)?.fields;
        for (renderer, on, off) in [("RS", "SOn", "SOff"), ("RMutato", "MutatoOn", "MutatoOff")] {
            let node = rules.root(world, link(f, renderer)?)?;
            for material in [link(f, on)?, link(f, off)?] {
                add(node, material);
            }
        }
    }
    if let Some(p) = world.prototypes.iter().find(|p| &p.source == prototype) {
        let mut seen = std::collections::HashSet::new();
        for (index, node) in p.nodes.iter().enumerate() {
            // A repeated node source takes the variants of its first node.
            if !seen.insert(&node.source) {
                continue;
            }
            if let Some(record) = &p.vehicle {
                for link in record.renderer_materials(index as u32) {
                    add(&node.source, &link.source);
                }
            }
            for link in p
                .gauges
                .iter()
                .flat_map(|g| g.renderer_materials(index as u32))
            {
                add(&node.source, &link.source);
            }
        }
    }
    Ok(output
        .into_iter()
        .map(|(node, list)| (node, list.into()))
        .collect())
}
