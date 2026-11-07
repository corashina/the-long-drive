//! The renderer view of a world stream. The renderer reads only this copy.
//! The simulation keeps the copy that it sent. `WorldStream::sync_renderer` updates it and writes the
//! changes into a `WorldDelta`. The renderer applies the delta to its own copy.
use super::{PublishedSurface, SurfaceKey, WorldRequest, WorldStream};
use crate::reference::{ReferenceError, ReferenceManifest, SourceId, WorldReference};
use crate::world::list_delta::ListDelta;
use crate::world::{
    GeneratedId, IdIndex, NodeAppearance, Placement, StartupTerrain, SupplyDefinition, SupplyPoint,
    WorldBounds, WorldLayer, WorldLight, WorldOrigin, WorldSurface,
};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RendererSurface {
    pub key: SurfaceKey,
    pub surface: Arc<WorldSurface>,
    pub prototype: Option<SourceId>,
    pub bounds: WorldBounds,
}
impl RendererSurface {
    fn of(published: &PublishedSurface) -> Self {
        Self {
            key: published.key,
            surface: published.surface.clone(),
            prototype: published.prototype.clone(),
            bounds: published.bounds,
        }
    }
    fn same(&self, published: &PublishedSurface) -> bool {
        self.key == published.key && Arc::ptr_eq(&self.surface, &published.surface)
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RendererStructure {
    pub fixed_decoration: bool,
    pub placement: Placement,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RendererSupply {
    pub key: GeneratedId,
    pub point: SupplyPoint,
    pub definition: SupplyDefinition,
    pub items: Option<(crate::reference::ItemId, crate::reference::ItemId)>,
}
pub struct RendererFrame<'a> {
    pub request: WorldRequest,
    pub origin: WorldOrigin,
    pub surfaces: &'a [RendererSurface],
    /// Changes when `surfaces` changes.
    pub surface_revision: u64,
    pub lights: &'a [WorldLight],
    pub structures: &'a [RendererStructure],
    pub supplies: &'a [RendererSupply],
    pub startup: Option<&'a StartupTerrain>,
    pub failure: Option<&'a str>,
}
/// Lamp material variants by prototype, then by node.
type LampChoices = HashMap<SourceId, Arc<HashMap<SourceId, Arc<[SourceId]>>>>;
/// Fixed lookups from the world reference. Copies of one world share them.
struct Lookups {
    reference: Arc<WorldReference>,
    rules: crate::world::spawn_rules::SourceRules,
    biome_materials: crate::world::biome_materials::BiomeMaterials,
    /// Lamp material variants by prototype and node, filled by reads.
    lamp_materials: Mutex<LampChoices>,
    /// The variants of a node without lamp materials.
    no_lamp_materials: Arc<[SourceId]>,
}
/// Values of a pending or published structure placement that need the stream to compute.
#[derive(Clone, Debug, Serialize, Deserialize)]
struct PlacementValues {
    /// Biome of the terrain color at the placement position.
    biome: u8,
    prop_material: Option<(SourceId, SourceId)>,
    #[serde(skip)]
    mark: u64,
}
struct ContentValues {
    appearance: Vec<NodeAppearance>,
    /// `Appearance::revision` of the copied appearance. The renderer copy keeps 0.
    revision: u64,
    mark: u64,
}
/// Changes of the renderer copy in one sync. A flag marks a replaced list.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct WorldDelta {
    /// False before the first sync.
    pub synced: bool,
    request: WorldRequest,
    origin: WorldOrigin,
    failure: Option<String>,
    surfaces_changed: bool,
    surface_keys: Vec<SurfaceKey>,
    /// Surfaces that the renderer copy does not have, or has with other data.
    new_surfaces: Vec<RendererSurface>,
    prepared_changed: bool,
    /// The prepared surface. `None` with `prepared_changed`: no prepared surface.
    /// The delta leaves out the surface data when the last list has it.
    prepared: Option<(WorldRequest, SurfaceKey, Option<RendererSurface>)>,
    lights_changed: bool,
    lights: Vec<WorldLight>,
    structures: ListDelta<RendererStructure>,
    pending: ListDelta<Placement>,
    /// Values of new placements. A changed placement list drops the values of missing placements.
    placements: Vec<(GeneratedId, PlacementValues)>,
    initialized_changed: bool,
    initialized: Vec<bool>,
    /// New or changed content appearance.
    contents: Vec<(GeneratedId, Vec<NodeAppearance>)>,
    removed_contents: Vec<GeneratedId>,
    cables_changed: bool,
    cables: Vec<(i64, Vec<NodeAppearance>)>,
    startup_changed: bool,
    startup: Option<StartupTerrain>,
    supplies_changed: bool,
    supplies: Vec<RendererSupply>,
    /// Marks new required structures, required lights and replay lists.
    required_changed: bool,
    required_structures: Vec<GeneratedId>,
    required_lights: Vec<GeneratedId>,
    replay: Vec<GeneratedId>,
    required_ready: bool,
    membership: (u64, u64, u64),
    pick_revision: u64,
    road_required: [bool; 3],
    road_loaded: [bool; 3],
    holds_changed: bool,
    holds: Vec<SourceId>,
}

/// Deltas compare by their encoding. Frame tests use it.
impl PartialEq for WorldDelta {
    fn eq(&self, other: &Self) -> bool {
        postcard::to_allocvec(self).ok() == postcard::to_allocvec(other).ok()
    }
}

impl WorldDelta {
    fn clear(&mut self) {
        self.failure = None;
        self.surfaces_changed = false;
        self.surface_keys.clear();
        self.new_surfaces.clear();
        self.prepared_changed = false;
        self.prepared = None;
        self.lights_changed = false;
        self.lights.clear();
        self.structures.clear();
        self.pending.clear();
        self.placements.clear();
        self.initialized_changed = false;
        self.initialized.clear();
        self.contents.clear();
        self.removed_contents.clear();
        self.cables_changed = false;
        self.cables.clear();
        self.startup_changed = false;
        self.startup = None;
        self.supplies_changed = false;
        self.supplies.clear();
        self.required_changed = false;
        self.required_structures.clear();
        self.required_lights.clear();
        self.replay.clear();
        self.holds_changed = false;
        self.holds.clear();
    }
}

pub struct RendererWorld {
    lookups: Arc<Lookups>,
    request: WorldRequest,
    pub origin: WorldOrigin,
    failure: Option<String>,
    surfaces: Vec<RendererSurface>,
    prepared: Option<(WorldRequest, RendererSurface)>,
    lights: Vec<WorldLight>,
    structures: Vec<RendererStructure>,
    structure_index: IdIndex,
    supplies: Vec<RendererSupply>,
    startup: Option<StartupTerrain>,
    surface_revision: u64,
    pending: Vec<Placement>,
    pending_index: IdIndex,
    /// Content initialization of each pending structure.
    initialized: Vec<bool>,
    placements: HashMap<GeneratedId, PlacementValues>,
    contents: HashMap<GeneratedId, ContentValues>,
    /// Pole rope appearance by road segment index.
    cables: Vec<(i64, Vec<NodeAppearance>)>,
    required_structures: Vec<GeneratedId>,
    required_index: IdIndex,
    required_lights: Vec<GeneratedId>,
    /// Structures that change replay needs.
    replay: Vec<GeneratedId>,
    replay_index: IdIndex,
    required_ready: bool,
    membership: (u64, u64, u64),
    /// Changes when pending, content initialization, required, replay or required_ready changes.
    pick_revision: u64,
    /// Road prototypes by biome that selected roads need.
    road_required: [bool; 3],
    /// Road prototypes by index that the stream has built.
    road_loaded: [bool; 3],
    holds: HashSet<SourceId>,
    mark: u64,
}

impl RendererWorld {
    pub fn new(reference: Arc<WorldReference>) -> Self {
        Self::with_lookups(Arc::new(Lookups {
            rules: crate::world::spawn_rules::SourceRules::new(&reference),
            biome_materials: crate::world::biome_materials::BiomeMaterials::new(
                &reference.biome_material_sets,
            ),
            reference,
            lamp_materials: Mutex::default(),
            no_lamp_materials: Arc::new([]),
        }))
    }
    fn with_lookups(lookups: Arc<Lookups>) -> Self {
        Self {
            lookups,
            request: WorldRequest::default(),
            origin: WorldOrigin::default(),
            failure: None,
            surfaces: Vec::new(),
            prepared: None,
            lights: Vec::new(),
            structures: Vec::new(),
            structure_index: IdIndex::default(),
            supplies: Vec::new(),
            startup: None,
            surface_revision: 0,
            pending: Vec::new(),
            pending_index: IdIndex::default(),
            initialized: Vec::new(),
            placements: HashMap::new(),
            contents: HashMap::new(),
            cables: Vec::new(),
            required_structures: Vec::new(),
            required_index: IdIndex::default(),
            required_lights: Vec::new(),
            replay: Vec::new(),
            replay_index: IdIndex::default(),
            required_ready: false,
            membership: (0, u64::MAX, u64::MAX),
            pick_revision: 0,
            road_required: [false; 3],
            road_loaded: [false; 3],
            holds: HashSet::new(),
            mark: 0,
        }
    }
    pub fn frame(&self) -> RendererFrame<'_> {
        RendererFrame {
            request: self.request,
            origin: self.origin,
            surfaces: &self.surfaces,
            surface_revision: self.surface_revision,
            lights: &self.lights,
            structures: &self.structures,
            supplies: &self.supplies,
            startup: self.startup.as_ref(),
            failure: self.failure.as_deref(),
        }
    }
    pub fn prepared_surface(&self) -> Option<(WorldRequest, &RendererSurface)> {
        self.prepared
            .as_ref()
            .map(|(request, surface)| (*request, surface))
    }
    pub fn pending_structures(&self) -> &[Placement] {
        &self.pending
    }
    fn pending_position(&self, id: &GeneratedId) -> Option<usize> {
        self.pending_index.position(&self.pending, |p| &p.id, id)
    }
    pub fn structure_required(&self, id: &GeneratedId) -> bool {
        self.required_index
            .position(&self.required_structures, |id| id, id)
            .is_some()
    }
    pub fn structure_replay_required(&self, id: &GeneratedId) -> bool {
        self.replay_index
            .position(&self.replay, |id| id, id)
            .is_some()
    }
    /// True while the stream still wants the structure.
    pub fn structure_wanted(&self, request: WorldRequest, id: &GeneratedId) -> bool {
        request.session == self.request.session && self.pending_position(id).is_some()
    }
    /// Session, published structure revision and pending structure revision.
    pub fn structure_membership(&self) -> (u64, u64, u64) {
        self.membership
    }
    /// Changes when an input of the renderer structure pick changes: pending structures,
    /// content initialization, required and replay structures, or `required_ready`.
    pub fn pick_revision(&self) -> u64 {
        self.pick_revision
    }
    pub fn required_lights(&self) -> &[GeneratedId] {
        &self.required_lights
    }
    pub fn required_structure_count(&self) -> usize {
        self.required_structures.len()
    }
    pub fn required_ready(&self) -> bool {
        self.required_ready
    }
    pub fn road_prototype_required(&self, source: &SourceId) -> bool {
        self.lookups
            .reference
            .road
            .prototypes
            .iter()
            .zip(self.road_required)
            .any(|(prototype, required)| required && prototype == source)
    }
    pub fn holds_prototype(&self, source: &SourceId) -> bool {
        self.holds.contains(source)
    }
    pub fn contents_initialized(&self, id: &GeneratedId) -> bool {
        self.pending_position(id)
            .is_none_or(|i| self.initialized[i])
    }
    /// The content of `id` spawned. Startup contents spawn after the renderer accepts the structure.
    pub fn has_contents(&self, id: &GeneratedId) -> bool {
        id.layer == WorldLayer::Lamp || self.contents.contains_key(id)
    }
    pub fn appearance(&self, id: &GeneratedId) -> &[NodeAppearance] {
        if id.layer == WorldLayer::Lamp {
            return self
                .cables
                .iter()
                .find(|(index, _)| *index == id.unit[0])
                .map_or(&[], |(_, cable)| cable);
        }
        self.contents
            .get(id)
            .map_or(&[], |content| &content.appearance)
    }
    /// Source `materialChangeScript.Start` biome for renderer `node`, from the object position, not the surface.
    /// `None` when no active marked node holds the renderer.
    pub fn renderer_biome(
        &self,
        prototype: &crate::reference::WorldPrototypeReference,
        placement: &Placement,
        node: usize,
    ) -> Option<u8> {
        if !crate::world::biome_materials::under_mark(prototype, node)
            || !crate::world::spawn_appearance::node_active(
                prototype,
                node,
                self.appearance(&placement.id),
            )
        {
            return None;
        }
        self.placements.get(&placement.id).map(|p| p.biome)
    }
    /// Biome material for a first material. `None` keeps the material.
    pub fn biome_material(&self, material: &SourceId, biome: u8) -> Option<&SourceId> {
        self.lookups.biome_materials.variant(material, biome)
    }
    pub fn lamp_materials(
        &self,
        prototype: &SourceId,
        node: &SourceId,
    ) -> Result<Arc<[SourceId]>, ReferenceError> {
        let lookups = &*self.lookups;
        let cached = lookups
            .lamp_materials
            .lock()
            .unwrap()
            .get(prototype)
            .cloned();
        let nodes = match cached {
            Some(nodes) => nodes,
            None => {
                let nodes = Arc::new(super::lamp_materials_of(
                    &lookups.rules,
                    &lookups.reference,
                    prototype,
                )?);
                lookups
                    .lamp_materials
                    .lock()
                    .unwrap()
                    .insert(prototype.clone(), nodes.clone());
                nodes
            }
        };
        Ok(nodes
            .get(node)
            .unwrap_or(&lookups.no_lamp_materials)
            .clone())
    }
    pub fn lod_owner(&self, source: &SourceId) -> Option<&SourceId> {
        self.lookups
            .rules
            .full_record_owner(&self.lookups.reference, source)
    }
    pub fn moving_nodes(
        &self,
        reference: &ReferenceManifest,
        prototype: &crate::reference::WorldPrototypeReference,
    ) -> Result<Vec<bool>, ReferenceError> {
        super::moving_nodes::moving_nodes_of(&self.lookups.rules, reference, prototype)
    }
    pub fn prop_material(&self, placement: &Placement) -> Option<(&SourceId, &SourceId)> {
        self.placements
            .get(&placement.id)
            .and_then(|p| p.prop_material.as_ref())
            .map(|(renderer, material)| (renderer, material))
    }
    /// Writes the primary appearance assets of all contents to `assets`.
    pub fn primary_assets(&self, assets: &mut Vec<crate::reference::AssetId>) {
        assets.clear();
        for content in self.contents.values() {
            assets.extend(
                content
                    .appearance
                    .iter()
                    .filter_map(|a| a.primary.map(|p| p.0)),
            );
        }
    }
    /// Structure load order, first is less. See `load_order`.
    pub fn load_order(&self, a: &Placement, b: &Placement) -> std::cmp::Ordering {
        super::load_order(self.request, a, b)
    }
}

impl WorldStream {
    /// Copies the renderer view of the stream into `world` and writes the changes into `delta`.
    /// Allocates only for changed data.
    pub fn sync_renderer(&self, world: &mut RendererWorld, delta: &mut WorldDelta) {
        delta.clear();
        delta.synced = true;
        let session_changed = world.request.session != self.request.session;
        world.request = self.request;
        world.origin = self.origin;
        delta.request = self.request;
        delta.origin = self.origin;
        if world.failure.as_deref() != self.failure.as_deref() {
            world.failure.clone_from(&self.failure);
        }
        delta.failure.clone_from(&world.failure);
        if world.surfaces.len() != self.surfaces.len()
            || world
                .surfaces
                .iter()
                .zip(&self.surfaces)
                .any(|(a, b)| !a.same(b))
        {
            delta.surfaces_changed = true;
            for published in &self.surfaces {
                delta.surface_keys.push(published.key);
                let known = world.surfaces.iter().any(|s| s.same(published))
                    || world
                        .prepared
                        .as_ref()
                        .is_some_and(|(_, s)| s.same(published));
                if !known {
                    delta.new_surfaces.push(RendererSurface::of(published));
                }
            }
            world.surfaces.clear();
            world
                .surfaces
                .extend(self.surfaces.iter().map(RendererSurface::of));
            world.surface_revision = world.surface_revision.wrapping_add(1);
        }
        match self.prepared_surface() {
            None => {
                if world.prepared.take().is_some() {
                    delta.prepared_changed = true;
                }
            }
            Some((request, published)) => {
                if world
                    .prepared
                    .as_ref()
                    .is_none_or(|(r, s)| *r != request || !s.same(published))
                {
                    let known = world.surfaces.iter().any(|s| s.same(published))
                        || world
                            .prepared
                            .as_ref()
                            .is_some_and(|(_, s)| s.same(published));
                    delta.prepared_changed = true;
                    delta.prepared = Some((
                        request,
                        published.key,
                        (!known).then(|| RendererSurface::of(published)),
                    ));
                    world.prepared = Some((request, RendererSurface::of(published)));
                }
            }
        }
        if world.lights.len() != self.lights.len()
            || world
                .lights
                .iter()
                .zip(&self.lights)
                .any(|(a, b)| !same_light(a, b))
        {
            world.lights.clone_from(&self.lights);
            delta.lights_changed = true;
            delta.lights.clone_from(&self.lights);
        }
        let membership = self.structure_membership();
        let structures_changed =
            (membership.0, membership.1) != (world.membership.0, world.membership.1);
        let pending_changed =
            (membership.0, membership.2) != (world.membership.0, world.membership.2);
        world.membership = membership;
        delta.membership = membership;
        if structures_changed {
            let index = &world.structure_index;
            delta.structures.diff(
                &mut world.structures,
                &self.structures,
                |old, s| {
                    index
                        .position(old, |o| &o.placement.id, &s.placement.id)
                        .filter(|&i| {
                            let o = &old[i];
                            o.fixed_decoration == s.fixed_decoration && o.placement == s.placement
                        })
                },
                |s| RendererStructure {
                    fixed_decoration: s.fixed_decoration,
                    placement: s.placement.clone(),
                },
            );
            world
                .structure_index
                .rebuild(&world.structures, |s| &s.placement.id);
        }
        let mut pick_changed = pending_changed;
        if pending_changed {
            let index = &world.pending_index;
            delta.pending.diff(
                &mut world.pending,
                &self.wanted_structures,
                |old, p| {
                    index
                        .position(old, |o| &o.id, &p.id)
                        .filter(|&i| old[i] == *p)
                },
                Placement::clone,
            );
            world.pending_index.rebuild(&world.pending, |p| &p.id);
        }
        if structures_changed || pending_changed {
            world.mark += 1;
            let mark = world.mark;
            for placement in world
                .pending
                .iter()
                .chain(world.structures.iter().map(|s| &s.placement))
            {
                if let Some(values) = world.placements.get_mut(&placement.id) {
                    values.mark = mark;
                    continue;
                }
                let p = placement.pose.position;
                let color = self
                    .evaluator
                    .color([p[0] as f32, -p[2] as f32], p[1] as f32);
                let values = PlacementValues {
                    biome: crate::world::biome(color, self.mode.biome_override()),
                    prop_material: self
                        .props
                        .material(placement)
                        .map(|(r, m)| (r.clone(), m.clone())),
                    mark,
                };
                delta
                    .placements
                    .push((placement.id.clone(), values.clone()));
                world.placements.insert(placement.id.clone(), values);
            }
            world.placements.retain(|_, values| values.mark == mark);
        }
        let mut initialized_changed = world.initialized.len() != world.pending.len();
        world.initialized.resize(world.pending.len(), false);
        for (initialized, placement) in world.initialized.iter_mut().zip(&world.pending) {
            let value = self.contents.initialized(&placement.id);
            initialized_changed |= *initialized != value;
            *initialized = value;
        }
        if initialized_changed || pending_changed {
            pick_changed = true;
            delta.initialized_changed = true;
            delta.initialized.clone_from(&world.initialized);
        }
        self.sync_renderer_contents(world, delta);
        let mut count = 0;
        for road in &self.selected_roads {
            let index = i64::from(road.segment.index);
            match world.cables.get_mut(count) {
                Some(entry) if entry.0 == index && entry.1 == road.cable => {}
                Some(entry) => {
                    entry.0 = index;
                    entry.1.clone_from(&road.cable);
                    delta.cables_changed = true;
                }
                None => {
                    world.cables.push((index, road.cable.clone()));
                    delta.cables_changed = true;
                }
            }
            count += 1;
        }
        if world.cables.len() != count {
            world.cables.truncate(count);
            delta.cables_changed = true;
        }
        if delta.cables_changed {
            delta.cables.clone_from(&world.cables);
        }
        if session_changed || world.startup.is_some() != self.startup.is_some() {
            world.startup = self.startup.clone();
            delta.startup_changed = true;
            delta.startup.clone_from(&world.startup);
        }
        if world.supplies.len() != self.supplies.len()
            || world
                .supplies
                .iter()
                .zip(&self.supplies)
                .any(|(a, b)| a.key != b.key || a.items != b.items)
        {
            world.supplies.clear();
            world
                .supplies
                .extend(self.supplies.iter().map(|s| RendererSupply {
                    key: s.key.clone(),
                    point: s.point,
                    definition: s.definition.clone(),
                    items: s.items,
                }));
            delta.supplies_changed = true;
            delta.supplies.clone_from(&world.supplies);
        }
        let mut required_changed = false;
        if crate::world::required::select(
            &mut world.required_structures,
            self.required.structures.iter(),
        ) {
            world
                .required_index
                .rebuild(&world.required_structures, |id| id);
            required_changed = true;
        }
        required_changed |=
            crate::world::required::select(&mut world.required_lights, self.required.lights.iter());
        if crate::world::required::select(
            &mut world.replay,
            self.replay_roots.iter().map(|root| &root.id).chain(
                self.replay_indices
                    .iter()
                    .filter_map(|&index| self.changes.get_indexed(index).1.instance.as_ref()),
            ),
        ) {
            world.replay_index.rebuild(&world.replay, |id| id);
            required_changed = true;
        }
        if required_changed {
            pick_changed = true;
            delta.required_changed = true;
            delta
                .required_structures
                .clone_from(&world.required_structures);
            delta.required_lights.clone_from(&world.required_lights);
            delta.replay.clone_from(&world.replay);
        }
        let required_ready = self.required_ready();
        pick_changed |= world.required_ready != required_ready;
        world.required_ready = required_ready;
        if pick_changed {
            world.pick_revision = world.pick_revision.wrapping_add(1);
        }
        delta.pick_revision = world.pick_revision;
        world.road_required = [false; 3];
        for road in &self.selected_roads {
            if world.required_ready || self.required.roads.contains(&road.segment.index) {
                world.road_required[road.segment.biome as usize] = true;
            }
        }
        world.road_loaded = self.road_prototypes.each_ref().map(Option::is_some);
        if world.holds.len() != self.prototype_holds.len()
            || self
                .prototype_holds
                .keys()
                .any(|k| !world.holds.contains(k))
        {
            world.holds.clear();
            world.holds.extend(self.prototype_holds.keys().cloned());
            delta.holds_changed = true;
            delta.holds.extend(world.holds.iter().cloned());
        }
        delta.required_ready = world.required_ready;
        delta.road_required = world.road_required;
        delta.road_loaded = world.road_loaded;
    }
    /// Content appearance by placement. Active contents come before cursor roots.
    fn sync_renderer_contents(&self, world: &mut RendererWorld, delta: &mut WorldDelta) {
        world.mark += 1;
        let mark = world.mark;
        let mut count = 0;
        for (id, content) in self
            .contents
            .active()
            .iter()
            .map(|c| (&c.content.placement.id, &c.content))
            .chain(self.contents.cursor_roots())
        {
            match world.contents.get_mut(id) {
                Some(values) if values.mark == mark => continue,
                Some(values) => {
                    values.mark = mark;
                    let revision = content.appearance.revision();
                    if values.revision != revision {
                        values.revision = revision;
                        if values.appearance != *content.appearance {
                            values.appearance.clone_from(&content.appearance);
                            delta
                                .contents
                                .push((id.clone(), content.appearance.to_vec()));
                        }
                    }
                }
                None => {
                    world.contents.insert(
                        id.clone(),
                        ContentValues {
                            appearance: content.appearance.to_vec(),
                            revision: content.appearance.revision(),
                            mark,
                        },
                    );
                    delta
                        .contents
                        .push((id.clone(), content.appearance.to_vec()));
                }
            }
            count += 1;
        }
        if count != world.contents.len() {
            world.contents.retain(|id, values| {
                let keep = values.mark == mark;
                if !keep {
                    delta.removed_contents.push(id.clone());
                }
                keep
            });
        }
    }
}

impl RendererWorld {
    /// Applies the changes of one simulation sync.
    pub fn apply(&mut self, delta: &WorldDelta) {
        if !delta.synced {
            return;
        }
        self.request = delta.request;
        self.origin = delta.origin;
        self.failure.clone_from(&delta.failure);
        if delta.surfaces_changed {
            self.surface_revision = self.surface_revision.wrapping_add(1);
            let old = std::mem::take(&mut self.surfaces);
            let prepared = self.prepared.as_ref().map(|(_, s)| s);
            self.surfaces.extend(delta.surface_keys.iter().map(|key| {
                delta
                    .new_surfaces
                    .iter()
                    .chain(prepared)
                    .chain(&old)
                    .find(|s| s.key == *key)
                    .expect("delta names a known surface")
                    .clone()
            }));
        }
        if delta.prepared_changed {
            self.prepared = delta.prepared.as_ref().map(|(request, key, surface)| {
                let surface = surface.clone().unwrap_or_else(|| {
                    self.prepared
                        .iter()
                        .map(|(_, s)| s)
                        .chain(&self.surfaces)
                        .find(|s| s.key == *key)
                        .expect("delta names a known prepared surface")
                        .clone()
                });
                (*request, surface)
            });
        }
        if delta.lights_changed {
            self.lights.clone_from(&delta.lights);
        }
        self.membership = delta.membership;
        if delta.structures.changed() {
            delta.structures.apply(&mut self.structures);
            self.structure_index
                .rebuild(&self.structures, |s| &s.placement.id);
        }
        if delta.pending.changed() {
            delta.pending.apply(&mut self.pending);
            self.pending_index.rebuild(&self.pending, |p| &p.id);
        }
        if delta.structures.changed() || delta.pending.changed() {
            // Placement values of the new lists. Each list has a placement value.
            self.mark += 1;
            let mark = self.mark;
            for id in self
                .pending
                .iter()
                .map(|p| &p.id)
                .chain(self.structures.iter().map(|s| &s.placement.id))
            {
                if let Some(values) = self.placements.get_mut(id) {
                    values.mark = mark;
                }
            }
            self.placements.retain(|_, values| values.mark == mark);
        }
        for (id, values) in &delta.placements {
            self.placements.insert(id.clone(), values.clone());
        }
        if delta.initialized_changed {
            self.initialized.clone_from(&delta.initialized);
        }
        for (id, appearance) in &delta.contents {
            self.contents.insert(
                id.clone(),
                ContentValues {
                    appearance: appearance.clone(),
                    revision: 0,
                    mark: 0,
                },
            );
        }
        for id in &delta.removed_contents {
            self.contents.remove(id);
        }
        if delta.cables_changed {
            self.cables.clone_from(&delta.cables);
        }
        if delta.startup_changed {
            self.startup.clone_from(&delta.startup);
        }
        if delta.supplies_changed {
            self.supplies.clone_from(&delta.supplies);
        }
        self.pick_revision = delta.pick_revision;
        if delta.required_changed {
            self.required_lights.clone_from(&delta.required_lights);
            self.required_structures
                .clone_from(&delta.required_structures);
            self.required_index
                .rebuild(&self.required_structures, |id| id);
            self.replay.clone_from(&delta.replay);
            self.replay_index.rebuild(&self.replay, |id| id);
        }
        self.required_ready = delta.required_ready;
        self.road_required = delta.road_required;
        self.road_loaded = delta.road_loaded;
        if delta.holds_changed {
            self.holds.clear();
            self.holds.extend(delta.holds.iter().cloned());
        }
    }
}

/// Lights compare by their changing values. Its id fixes the light record.
fn same_light(a: &WorldLight, b: &WorldLight) -> bool {
    a.id == b.id
        && a.pole == b.pole
        && a.pose == b.pose
        && a.biome == b.biome
        && a.prototype == b.prototype
        && a.scale == b.scale
        && a.range == b.range
        && a.enabled == b.enabled
        && a.intensity == b.intensity
        && a.node == b.node
}
