//! Active nested cursors and compact first-call replay boundaries.
use super::spawn_contents::{SpawnEvaluation, SpawnedContent};
use super::spawn_rules::*;
use super::{GeneratedId, Placement, SourceSpawnState, SpawnContext, SpawnPhase};
use crate::reference::{ItemId, ReferenceError, SourceId, WorldReference};
struct DeferredBatch {
    replay_only: bool,
    owner: GeneratedId,
    tasks: Vec<(Placement, SourceId)>,
    inputs: super::spawn_state::SpawnInitialization,
    next: usize,
    eligible: u64,
}
struct Cursor {
    replay_only: bool,
    deferred_batches: usize,
    parent: Placement,
    building: Option<SourceId>,
    initial_eligible: Option<bool>,
    callers: Vec<SourceId>,
    next: usize,
    chance: Option<f32>,
    root: Option<SpawnedContent>,
    inputs: super::spawn_state::SpawnInitialization,
}
pub struct ContentBinding {
    pub render_body: Option<(
        rapier3d::prelude::RigidBodyHandle,
        crate::vehicle::VehiclePose,
    )>,
    pub source: SourceId,
    pub item: ItemId,
    pub baseline: super::ObjectSnapshot,
}
pub struct ContentSlot {
    pub source: SourceId,
    pub slot: crate::reference::PartSlotId,
    pub initialized: bool,
}
pub struct ContentCollider {
    pub handle: crate::vehicle::ColliderHandle,
    pub node: usize,
    pub local: rapier3d::math::Pose,
}
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct GeneratedCapVisual {
    pub opening: crate::reference::OpeningId,
    pub visual: crate::reference::CapVisualReference,
}
pub struct ContentInstance {
    pub visual_revision: u64,
    pub visual_published: u64,
    pub visual_ready: bool,
    pub visual_caps: usize,
    pub visual_food: Option<(crate::interaction::items::FoodState, bool)>,
    pub replay_only: bool,
    pub restored: bool,
    pub suspended_bodies: Vec<(rapier3d::prelude::RigidBodyHandle, bool)>,
    pub origin: Placement,
    pub caps: Vec<GeneratedCapVisual>,
    pub poses: Vec<crate::vehicle::VehiclePose>,
    pub mechanism_colliders: Vec<ContentCollider>,
    pub slots: Vec<ContentSlot>,
    pub bindings: Vec<ContentBinding>,
    pub nodes: Vec<crate::reference::NodeId>,
    pub render_body: Option<(
        rapier3d::prelude::RigidBodyHandle,
        crate::vehicle::VehiclePose,
    )>,
    pub content: SpawnedContent,
    pub item: Option<ItemId>,
    pub baseline: Option<super::ObjectSnapshot>,
    /// Store entries of `content.inputs.conditions`: entry, input index, prototype node.
    pub conditions: Vec<(crate::reference::ConditionId, usize, usize)>,
}
impl ContentInstance {
    pub fn take_visual_change(
        &mut self,
        food: Option<(crate::interaction::items::FoodState, bool)>,
    ) -> bool {
        let ready = self.item.is_none() || self.restored;
        if self.visual_ready != ready
            || self.visual_caps != self.caps.len()
            || self.visual_food != food
        {
            self.visual_revision = self.visual_revision.wrapping_add(1);
            self.visual_ready = ready;
            self.visual_caps = self.caps.len();
            self.visual_food = food;
        }
        if self.visual_revision == self.visual_published {
            return false;
        }
        self.visual_published = self.visual_revision;
        true
    }
    pub fn for_structure(placement: Placement) -> Self {
        Self {
            visual_revision: 0,
            conditions: Vec::new(),
            visual_published: u64::MAX,
            visual_ready: true,
            visual_caps: 0,
            visual_food: None,
            replay_only: false,
            restored: false,
            suspended_bodies: Vec::new(),
            origin: placement.clone(),
            caps: Vec::new(),
            slots: Vec::new(),
            bindings: Vec::new(),
            nodes: Vec::new(),
            poses: Vec::new(),
            mechanism_colliders: Vec::new(),
            render_body: None,
            item: None,
            baseline: None,
            content: SpawnedContent {
                placement,
                inputs: super::spawn_state::SpawnInitialization::default(),
                appearance: Vec::new(),
                cable: Vec::new(),
                lights: Vec::new(),
                fluid: None,
                tanks: Vec::new(),
                buried: false,
                distance_offset: [0.; 3],
                ammo: None,
            },
        }
    }
    pub fn has_external_dependents(
        &self,
        items: &crate::interaction::items::ItemStore,
        parts: &crate::interaction::parts::PartAttachments,
        wheels: &crate::interaction::wheels::WheelAssemblies,
    ) -> bool {
        let owns = |item| self.item == Some(item) || self.bindings.iter().any(|b| b.item == item);
        items.has_external_dependents(owns)
            || parts.has_external_dependents(owns)
            || wheels.has_external_dependents(owns)
    }
}
pub struct SourceContents {
    pub rules: SourceRules,
    pub state: SourceSpawnState,
    cursors: Vec<Cursor>,
    active: Vec<ContentInstance>,
    // It changes on membership and order only. Pose and appearance remain live.
    active_revision: u64,
    /// Bumps when a cursor or its root content changes.
    roots_revision: u64,
    next: usize,
    update: u64,
    selection: Option<(u64, u64)>,
    input_revision: u64,
    deferred: Vec<DeferredBatch>,
    /// Broken boxes whose content may publish.
    pub boxes: super::stream::content_boxes::BoxReleases,
    /// Human monster placements for the creature runtime. A replay places one again.
    humans: Vec<SpawnedContent>,
}
impl SourceContents {
    pub fn discard_replay_siblings(&mut self, wanted: &[Placement]) {
        let previous = self.active.len();
        self.active.retain(|content| {
            content.item.is_some()
                || wanted.iter().any(|p| p.id == content.content.placement.id)
                || !self.cursors.iter().any(|c| {
                    c.replay_only
                        && c.parent.id == content.origin.id
                        && (c.building.is_none()
                            || c.initial_eligible == Some(false)
                            || c.next >= c.callers.len().max(1))
                        && !self.deferred.iter().any(|b| b.owner == c.parent.id)
                })
        });
        if self.active.len() != previous {
            self.active_revision += 1;
        }
    }
    pub fn has_replay(&self) -> bool {
        self.cursors.iter().any(|c| c.replay_only)
    }
    pub fn replaying(&self, id: &GeneratedId) -> bool {
        self.cursors
            .iter()
            .any(|c| c.replay_only && &c.parent.id == id)
    }
    pub fn replay_finished(&self, id: &GeneratedId) -> bool {
        self.cursors
            .iter()
            .find(|c| &c.parent.id == id && c.replay_only)
            .is_some_and(|c| {
                c.root.is_some()
                    && (c.building.is_none() || c.next >= c.callers.len().max(1))
                    && !self
                        .deferred
                        .iter()
                        .any(|b| &b.owner == id && b.replay_only)
            })
    }
    pub fn replay_origin(&self, id: &GeneratedId) -> Option<&Placement> {
        self.cursors
            .iter()
            .find(|c| {
                c.parent.id.layer == id.layer
                    && c.parent.id.generator == id.generator
                    && c.parent.id.unit == id.unit
                    && c.parent.id.ordinal == id.ordinal
            })
            .map(|c| &c.parent)
            .or_else(|| {
                self.active
                    .iter()
                    .find(|c| c.content.placement.id == *id)
                    .map(|c| &c.origin)
            })
    }

    /// `current_car` is the source `carTypeToCarID` of the start car.
    pub fn new(reference: &WorldReference, current_car: i32) -> Self {
        Self {
            rules: SourceRules::new(reference),
            state: SourceSpawnState::supplied_controlled_startup(
                &reference.spawn_context,
                SpawnContext {
                    current_car,
                    peaceful_enemies: 1,
                    chance_bits: None,
                },
            ),
            cursors: Vec::new(),
            active: Vec::new(),
            active_revision: 0,
            roots_revision: 0,
            deferred: Vec::new(),
            update: 0,
            selection: None,
            input_revision: 0,
            next: 0,
            boxes: Default::default(),
            humans: Vec::new(),
        }
    }
    pub fn eligibility_changed(
        &self,
        spawn: f32,
        before: [[f64; 3]; 2],
        after: [[f64; 3]; 2],
    ) -> bool {
        let spawn = spawn.powi(2);
        self.cursors.iter().any(|c| {
            !c.replay_only
                && c.building.is_some()
                && c.initial_eligible == Some(false)
                && before
                    .into_iter()
                    .all(|p| distance(p, c.parent.pose.position) >= spawn)
                && after
                    .into_iter()
                    .any(|p| distance(p, c.parent.pose.position) < spawn)
        })
    }
    pub fn input_revision(&self) -> u64 {
        self.input_revision
    }
    pub fn active_revision(&self) -> u64 {
        self.active_revision
    }
    pub fn roots_revision(&self) -> u64 {
        self.roots_revision
    }
    /// Active contents got their items. Scans that skip contents without an item run again.
    pub(super) fn items_bound(&mut self) {
        self.active_revision += 1;
    }
    pub(super) fn push_active(&mut self, mut content: ContentInstance) {
        if !content.content.inputs.humans.is_empty() {
            self.humans.push(content.content);
            return;
        }
        self.boxes.place(&mut content.content.placement);
        self.active.push(content);
        self.active_revision += 1;
    }
    /// The next human monster placement and its `newAiScript.Start` roll.
    pub fn take_human(&mut self) -> Option<(Placement, super::spawn_state::HumanRoll)> {
        let content = self.humans.pop()?;
        let roll = content.inputs.humans.first()?.1;
        Some((content.placement, roll))
    }
    pub fn remove_active(&mut self, index: usize) -> ContentInstance {
        let content = self.active.swap_remove(index);
        self.active_revision += 1;
        content
    }
    pub fn select_revision(&mut self, session: u64, revision: u64) {
        if self.selection != Some((session, revision)) {
            self.selection = Some((session, revision));
            // A newly selected region performs its first check without simulating timed polling.
            for cursor in &mut self.cursors {
                if cursor.initial_eligible == Some(false) && cursor.building.is_some() {
                    cursor.initial_eligible = None;
                }
            }
        }
    }
    pub fn retain_requests(&mut self, structures: &[Placement], replays: &[Placement]) {
        let previous = self.active.len();
        self.active.retain(|c| {
            !c.replay_only || c.item.is_some() || replays.iter().any(|p| p.id == c.origin.id)
        });
        if self.active.len() != previous {
            self.active_revision += 1;
        }
        let cursors = self.cursors.len();
        self.cursors.retain(|c| {
            let selected = if c.replay_only { replays } else { structures };
            selected.iter().any(|p| p.id == c.parent.id)
        });
        if self.cursors.len() != cursors {
            self.roots_revision = self.roots_revision.wrapping_add(1);
        }
        self.deferred.retain(|batch| {
            self.cursors
                .iter()
                .any(|c| c.parent.id == batch.owner && c.replay_only == batch.replay_only)
        });
    }
    pub fn advance(
        &mut self,
        reference: &WorldReference,
        spawn: f32,
        structures: &[Placement],
        replays: &[Placement],
        cables: &[(GeneratedId, Vec<super::NodeAppearance>)],
        focus: [[f64; 3]; 2],
        limit: usize,
        budget: usize,
        pending: usize,
    ) -> Result<(), ReferenceError> {
        let initial_pending = self.pending_requests();
        self.update = self.update.wrapping_add(1);
        let eligible = self
            .deferred
            .iter()
            .filter(|b| b.eligible <= self.update)
            .map(|b| b.tasks.len() - b.next)
            .sum::<usize>();
        let initial_count = self.active.len();
        for _ in 0..eligible {
            if limit <= self.active.len().saturating_sub(initial_count) {
                return Ok(());
            }
            let cap = budget.saturating_sub(self.active.len().saturating_sub(initial_count));
            if !self.advance_deferred(reference, cap)? {
                return Ok(());
            }
        }
        let limit = limit.saturating_sub(self.active.len().saturating_sub(initial_count));
        if limit == 0 {
            return Ok(());
        }
        for (instance, replay_only) in structures
            .iter()
            .map(|p| (p, false))
            .chain(replays.iter().take(1).map(|p| (p, true)))
        {
            let p = instance;
            if !replay_only && replays.iter().any(|root| root.id == p.id) {
                continue;
            }
            if self
                .cursors
                .iter()
                .any(|c| c.parent.id == p.id && c.replay_only == replay_only)
            {
                continue;
            }
            if replay_only && self.has_replay() {
                continue;
            }
            let building = self
                .rules
                .component(reference, &p.prototype, "buildingscript")?;
            if !p.id.children.is_empty() && building.is_none() {
                continue;
            }
            let existing = self.active.iter().find(|c| c.content.placement.id == p.id);
            // A road building waits for its stream cable.
            let cable = cables.iter().find(|(id, _)| id == &p.id).map(|(_, c)| c);
            if existing.is_none()
                && p.id.layer == super::WorldLayer::Building
                && p.id.children.is_empty()
                && cable.is_none()
            {
                continue;
            }
            let eligible = if building.is_some() {
                eligible_callers(reference, &self.rules, &p.prototype)?
            } else {
                Vec::new()
            };
            self.roots_revision = self.roots_revision.wrapping_add(1);
            self.cursors.push(Cursor {
                replay_only,
                deferred_batches: 0,
                parent: p.clone(),
                building: building.map(|b| b.source.clone()),
                initial_eligible: None,
                callers: eligible,
                next: 0,
                chance: None,
                root: existing.map(|c| c.content.clone()),
                inputs: existing
                    .map(|c| c.content.inputs.for_parent(&p.id))
                    .unwrap_or_default(),
            });
        }
        if let Some(index) = self.cursors.iter().position(|c| c.root.is_none()) {
            if pending.saturating_add(initial_pending) <= self.pending_requests() {
                return Ok(());
            }
            let cursor = &self.cursors[index];
            let parent = &cursor.parent;
            // A fixed startup placement evaluates from a copy of the startup random. It never
            // advances the live random, so it needs no boundary and replays the same contents.
            let startup = parent.id.layer == super::WorldLayer::Startup;
            // A StartSpawnItems copy keeps its scene tank fluids and buried flag.
            let row = reference.startup_row(&parent.id);
            if cursor.replay_only
                && !startup
                && !self.state.has_boundary(
                    &parent.id,
                    cursor.building.as_ref().unwrap_or(&cursor.parent.prototype),
                    0,
                    SpawnPhase::ComponentStart,
                )
            {
                return Err(ReferenceError(
                    "Changed object has no retained root initialization boundary".into(),
                ));
            }
            let cap = budget.saturating_sub(self.active.len().saturating_sub(initial_count));
            let evaluate =
                |random: &mut super::UnityRandom,
                 context: super::SpawnContext,
                 pools: &mut Vec<super::spawn_state::PaintingPool>,
                 initialization: &super::spawn_state::SpawnInitialization| {
                    let mut e =
                        SpawnEvaluation::new(reference, &self.rules, random, context, pools, cap);
                    e.initialization = initialization.clone();
                    let tanks = match row {
                        Some(r) => e.start_tanks(r)?,
                        None => Vec::new(),
                    };
                    e.output.push(SpawnedContent {
                        inputs: super::spawn_state::SpawnInitialization::default(),
                        placement: parent.clone(),
                        appearance: Vec::new(),
                        cable: cables
                            .iter()
                            .find(|(id, _)| id == &parent.id)
                            .map(|(_, c)| c.clone())
                            .unwrap_or_default(),
                        lights: Vec::new(),
                        fluid: None,
                        tanks,
                        buried: row.is_some_and(|r| r.buried),
                        distance_offset: [0.; 3],
                        ammo: None,
                    });
                    e.queue_start(parent.clone(), None);
                    e.finish_starts()?;
                    Ok((e.output, e.deferred, e.initialization))
                };
            let (mut output, deferred, inputs) = if startup {
                let mut random = super::UnityRandom::restore(super::UnityRandomState {
                    words: reference.spawn_context.startup_state,
                });
                evaluate(
                    &mut random,
                    self.state.context,
                    &mut Vec::new(),
                    &cursor.inputs,
                )?
            } else {
                self.state.invoke(
                    &parent.id,
                    cursor.building.as_ref().unwrap_or(&cursor.parent.prototype),
                    0,
                    SpawnPhase::ComponentStart,
                    &cursor.inputs,
                    evaluate,
                )?
            };
            let root = output.remove(0);
            // The startup root is itself a world object.
            if startup {
                output.insert(0, root.clone());
            }
            self.roots_revision = self.roots_revision.wrapping_add(1);
            self.cursors[index].root = Some(root);
            self.cursors[index].initial_eligible = Some(
                self.cursors[index].replay_only
                    || (self.cursors[index].building.is_some()
                        && focus.iter().any(|f| {
                            distance(*f, self.cursors[index].parent.pose.position) < spawn * spawn
                        })),
            );
            self.cursors[index].inputs = inputs.for_parent(&self.cursors[index].parent.id);
            if !deferred.is_empty() {
                self.cursors[index].deferred_batches += 1;
                self.deferred.push(DeferredBatch {
                    replay_only: self.cursors[index].replay_only,
                    owner: self.cursors[index].parent.id.clone(),
                    tasks: deferred,
                    inputs,
                    next: 0,
                    eligible: self.update.wrapping_add(1),
                });
            }
            for content in output {
                if self
                    .active
                    .iter()
                    .any(|c| c.content.placement.id == content.placement.id)
                {
                    continue;
                }
                self.push_active(ContentInstance {
                    visual_revision: 0,
                    conditions: Vec::new(),
                    visual_published: u64::MAX,
                    visual_ready: true,
                    visual_caps: 0,
                    visual_food: None,
                    replay_only: self.cursors[index].replay_only,
                    origin: self
                        .replay_origin(&content.placement.id)
                        .cloned()
                        .unwrap_or_else(|| content.placement.clone()),
                    content,
                    poses: Vec::new(),
                    mechanism_colliders: Vec::new(),
                    restored: false,
                    suspended_bodies: Vec::new(),
                    caps: Vec::new(),
                    slots: Vec::new(),
                    bindings: Vec::new(),
                    nodes: Vec::new(),
                    render_body: None,
                    item: None,
                    baseline: None,
                });
            }
            return Ok(());
        }
        if self.cursors.is_empty() {
            return Ok(());
        }
        let before_count = self.active.len();
        for _ in 0..self.cursors.len() {
            let limit = limit.saturating_sub(self.active.len().saturating_sub(before_count));
            if limit == 0 {
                break;
            }
            let index = self.next % self.cursors.len();
            self.next = (index + 1) % self.cursors.len();
            if self.cursors[index].initial_eligible.is_none() {
                self.cursors[index].initial_eligible = Some(
                    self.cursors[index].replay_only
                        || focus.iter().any(|f| {
                            distance(*f, self.cursors[index].parent.pose.position) < spawn * spawn
                        }),
                );
            }
            let cursor = &self.cursors[index];
            if cursor.building.is_none()
                || cursor.initial_eligible != Some(true)
                || self
                    .deferred
                    .iter()
                    .any(|b| b.owner == cursor.parent.id && b.replay_only == cursor.replay_only)
                || cursor.next >= cursor.callers.len().max(1)
            {
                continue;
            }
            // This synchronous phase can create one new deferred owner.
            if pending.saturating_add(initial_pending) <= self.pending_requests() {
                self.next = index;
                break;
            }
            let cursor = &self.cursors[index];
            let first = cursor.next == 0;
            let source = if first {
                cursor.building.as_ref().unwrap()
            } else {
                &cursor.callers[cursor.next]
            };
            let parent = &cursor.parent;
            let chance = cursor.chance;
            if cursor.replay_only
                && !self.state.has_boundary(
                    &parent.id,
                    source,
                    cursor.next as u32,
                    SpawnPhase::SynchronousAndStarts,
                )
            {
                self.cursors[index].next = self.cursors[index].callers.len().max(1);
                continue;
            }
            if !cursor.replay_only {
                self.state.context.chance_bits = chance.map(f32::to_bits);
            }
            let cap = budget.saturating_sub(self.active.len().saturating_sub(before_count));
            let (output, chance, deferred, initialization) = self.state.invoke(
                &parent.id,
                source,
                cursor.next as u32,
                SpawnPhase::SynchronousAndStarts,
                &cursor.inputs,
                |random, context, pools, initialization| {
                    let mut evaluation =
                        SpawnEvaluation::new(reference, &self.rules, random, context, pools, cap);
                    evaluation.initialization = initialization.clone();
                    if let Some(root) = &cursor.root {
                        evaluation.output.push(root.clone());
                    }
                    let chance = spawn_step(
                        &mut evaluation,
                        parent,
                        cursor.building.as_ref().unwrap(),
                        &cursor.callers,
                        cursor.next,
                        context.chance_bits.map(f32::from_bits),
                    )?;
                    evaluation.finish_starts()?;
                    Ok((
                        evaluation.output,
                        chance,
                        evaluation.deferred,
                        evaluation.initialization,
                    ))
                },
            )?;
            let owner = parent.id.clone();
            let replay_only = cursor.replay_only;
            self.cursors[index].inputs = initialization.for_parent(&self.cursors[index].parent.id);
            if !deferred.is_empty() {
                self.cursors[index].deferred_batches += 1;
                self.deferred.push(DeferredBatch {
                    replay_only,
                    owner,
                    tasks: deferred,
                    inputs: initialization,
                    next: 0,
                    eligible: self.update.wrapping_add(1),
                });
            }
            self.cursors[index].next += 1;
            self.cursors[index].chance = chance;
            for content in output {
                if content.placement.id == self.cursors[index].parent.id {
                    self.roots_revision = self.roots_revision.wrapping_add(1);
                    self.cursors[index].root = Some(content);
                    continue;
                }
                if !self
                    .active
                    .iter()
                    .any(|c| c.content.placement.id == content.placement.id)
                {
                    self.push_active(ContentInstance {
                        visual_revision: 0,
                        conditions: Vec::new(),
                        visual_published: u64::MAX,
                        visual_ready: true,
                        visual_caps: 0,
                        visual_food: None,
                        replay_only,
                        origin: self
                            .replay_origin(&content.placement.id)
                            .cloned()
                            .unwrap_or_else(|| content.placement.clone()),
                        content,
                        poses: Vec::new(),
                        mechanism_colliders: Vec::new(),
                        restored: false,
                        suspended_bodies: Vec::new(),
                        caps: Vec::new(),
                        slots: Vec::new(),
                        bindings: Vec::new(),
                        nodes: Vec::new(),
                        render_body: None,
                        item: None,
                        baseline: None,
                    });
                }
            }
        }
        Ok(())
    }
    /// False: no eligible batch.
    fn advance_deferred(
        &mut self,
        reference: &WorldReference,
        cap: usize,
    ) -> Result<bool, ReferenceError> {
        let Some(index) = self.deferred.iter().position(|b| b.eligible <= self.update) else {
            return Ok(false);
        };
        let final_task = self.deferred[index].next + 1 == self.deferred[index].tasks.len();
        if !self.cursors.iter().any(|c| {
            c.parent.id == self.deferred[index].owner
                && c.replay_only == self.deferred[index].replay_only
                && c.deferred_batches > 0
        }) {
            return Err(ReferenceError("Missing source deferred family".into()));
        }
        let batch = &self.deferred[index];
        let (parent, source) = &batch.tasks[batch.next];
        let replay_only = batch.replay_only;
        if replay_only
            && !self.state.has_boundary(
                &parent.id,
                source,
                batch.next as u32,
                SpawnPhase::ComponentStart,
            )
        {
            return Err(ReferenceError(
                "Changed object has no retained slot continuation boundary".into(),
            ));
        }
        let (output, deferred, initialization) = self.state.invoke(
            &parent.id,
            source,
            batch.next as u32,
            SpawnPhase::ComponentStart,
            &batch.inputs,
            |random, context, pools, initialization| {
                let mut evaluation =
                    SpawnEvaluation::new(reference, &self.rules, random, context, pools, cap);
                evaluation.initialization = initialization.clone();
                // The continuation can change its existing instance's selector and condition.
                if let Some(existing) = self.active.iter().find(|c| {
                    c.content.placement.id == parent.id && (!replay_only || c.item.is_none())
                }) {
                    evaluation.output.push(existing.content.clone());
                } else if let Some(root) = self
                    .cursors
                    .iter()
                    .find(|c| c.parent.id == parent.id && c.replay_only == replay_only)
                    .and_then(|c| c.root.as_ref())
                {
                    evaluation.output.push(root.clone());
                }
                evaluation.spawn_slot(parent, source)?;
                evaluation.finish_starts()?;
                Ok((
                    evaluation.output,
                    evaluation.deferred,
                    evaluation.initialization,
                ))
            },
        )?;
        let owner = &self.deferred[index].owner;
        if let Some(cursor) = self
            .cursors
            .iter_mut()
            .find(|c| &c.parent.id == owner && c.replay_only == replay_only)
        {
            cursor.inputs = initialization.for_parent(&cursor.parent.id);
        }
        self.complete_deferred(index, deferred, initialization, final_task)?;
        for content in output {
            if let Some(cursor) = self
                .cursors
                .iter_mut()
                .find(|c| c.parent.id == content.placement.id && c.replay_only == replay_only)
            {
                self.roots_revision = self.roots_revision.wrapping_add(1);
                cursor.root = Some(content);
            } else if let Some(existing) = self
                .active
                .iter_mut()
                .find(|c| c.content.placement.id == content.placement.id)
            {
                if !replay_only || existing.item.is_none() {
                    existing.content = content;
                }
            } else {
                self.push_active(ContentInstance {
                    visual_revision: 0,
                    conditions: Vec::new(),
                    visual_published: u64::MAX,
                    visual_ready: true,
                    visual_caps: 0,
                    visual_food: None,
                    replay_only,
                    origin: self
                        .replay_origin(&content.placement.id)
                        .cloned()
                        .unwrap_or_else(|| content.placement.clone()),
                    content,
                    poses: Vec::new(),
                    mechanism_colliders: Vec::new(),
                    restored: false,
                    suspended_bodies: Vec::new(),
                    caps: Vec::new(),
                    slots: Vec::new(),
                    bindings: Vec::new(),
                    nodes: Vec::new(),
                    render_body: None,
                    item: None,
                    baseline: None,
                });
            }
        }
        Ok(true)
    }
    fn complete_deferred(
        &mut self,
        index: usize,
        deferred: Vec<(Placement, SourceId)>,
        initialization: super::spawn_state::SpawnInitialization,
        final_task: bool,
    ) -> Result<(), ReferenceError> {
        let owner = &self.deferred[index];
        let cursor = self
            .cursors
            .iter()
            .position(|c| c.parent.id == owner.owner && c.replay_only == owner.replay_only)
            .ok_or_else(|| ReferenceError("Missing source deferred family".into()))?;
        let next = self.cursors[cursor]
            .deferred_batches
            .checked_add(usize::from(!deferred.is_empty()))
            .and_then(|n| n.checked_sub(usize::from(final_task)))
            .ok_or_else(|| ReferenceError("Invalid source deferred family count".into()))?;
        let batch = &mut self.deferred[index];
        batch.next += 1;
        batch.inputs = initialization;
        if !deferred.is_empty() {
            let batch = &self.deferred[index];
            self.deferred.push(DeferredBatch {
                replay_only: batch.replay_only,
                owner: batch.owner.clone(),
                tasks: deferred,
                inputs: batch.inputs.clone(),
                next: 0,
                eligible: self.update.wrapping_add(1),
            });
        }
        if final_task {
            self.deferred.remove(index);
        }
        self.cursors[cursor].deferred_batches = next;
        Ok(())
    }
    pub fn lamp_states(&self) -> impl Iterator<Item = &(GeneratedId, SourceId, bool)> {
        self.instances().flat_map(|c| &c.inputs.lamps)
    }
    /// Source `Lamp.broken` bits set by flashlightscript.Break. Bit i is `Lamps[i]`.
    pub fn broken_lamps(&self, parent: &GeneratedId, source: &SourceId) -> u64 {
        self.instances()
            .flat_map(|c| &c.inputs.broken_lamps)
            .filter(|(p, s, _)| p == parent && s == source)
            .fold(0, |bits, (_, _, b)| bits | b)
    }
    pub fn set_lamp(
        &mut self,
        reference: &WorldReference,
        parent: &GeneratedId,
        source: &SourceId,
        on: bool,
    ) -> Result<(), ReferenceError> {
        let deferred = |i: &super::spawn_state::SpawnInitialization| {
            i.lamps.iter().any(|(p, s, _)| p == parent && s == source)
        };
        self.refresh_lamp(reference, parent, deferred, |inputs| {
            let state = &mut inputs
                .lamps
                .iter_mut()
                .find(|(p, s, _)| p == parent && s == source)
                .expect("generated lamp state")
                .2;
            let changed = *state != on;
            *state = on;
            changed
        })
    }
    /// Source flashlightscript.Break. Sets the lamps in `lamps` broken. Bit i is `Lamps[i]`.
    pub fn break_lamps(
        &mut self,
        reference: &WorldReference,
        parent: &GeneratedId,
        source: &SourceId,
        lamps: u64,
    ) -> Result<(), ReferenceError> {
        let deferred = |i: &super::spawn_state::SpawnInitialization| {
            i.lamps.iter().any(|(p, s, _)| p == parent && s == source)
        };
        self.refresh_lamp(reference, parent, deferred, |inputs| {
            inputs.break_lamps(parent, source, lamps)
        })
    }
    /// Sets the live `jamValue` of electronics `component` on `parent`. Its lamps and lights refresh.
    pub fn set_jam(
        &mut self,
        reference: &WorldReference,
        parent: &GeneratedId,
        component: &SourceId,
        jam: f32,
    ) -> Result<(), ReferenceError> {
        let deferred = |i: &super::spawn_state::SpawnInitialization| {
            i.initialized
                .iter()
                .any(|(p, s)| p == parent && s == component)
        };
        self.refresh_lamp(reference, parent, deferred, |inputs| {
            inputs.set_jam(parent, component, jam);
            true
        })
    }
    /// Source flashlightscript.Refresh after `change` edits the lamp inputs. `change` returns
    /// true when the lamp changed. `deferred` selects the deferred batches that `change` edits.
    fn refresh_lamp(
        &mut self,
        reference: &WorldReference,
        parent: &GeneratedId,
        deferred: impl Fn(&super::spawn_state::SpawnInitialization) -> bool,
        change: impl Fn(&mut super::spawn_state::SpawnInitialization) -> bool,
    ) -> Result<(), ReferenceError> {
        let current = self
            .instances()
            .find(|c| &c.placement.id == parent)
            .ok_or_else(|| ReferenceError("Generated lamp instance missing".into()))?;
        let mut inputs = current.inputs.clone();
        let current = current.clone();
        let lamp_changed = change(&mut inputs);
        let mut random = super::UnityRandom::restore(self.state.live_state());
        let mut pools = Vec::new();
        let mut evaluation = SpawnEvaluation::new(
            reference,
            &self.rules,
            &mut random,
            self.state.context,
            &mut pools,
            1,
        );
        evaluation.initialization = inputs;
        evaluation.output.push(current);
        evaluation.bake_appearance()?;
        evaluation.bake_lamps()?;
        evaluation.bake_cables();
        let content = evaluation.output.pop().unwrap();
        self.input_revision = self.input_revision.wrapping_add(1);
        if let Some(cursor) = self.cursors.iter_mut().find(|c| &c.parent.id == parent) {
            cursor.inputs = evaluation.initialization;
        }
        for batch in &mut self.deferred {
            if deferred(&batch.inputs) {
                change(&mut batch.inputs);
            }
        }
        if let Some(active) = self
            .active
            .iter_mut()
            .find(|c| &c.content.placement.id == parent)
        {
            active.content = content;
            if lamp_changed {
                active.visual_revision = active.visual_revision.wrapping_add(1);
            }
        } else {
            self.cursors
                .iter_mut()
                .find(|c| &c.parent.id == parent)
                .unwrap()
                .root = Some(content);
        }
        Ok(())
    }
    pub fn pending_requests(&self) -> usize {
        self.cursors
            .iter()
            .filter(|c| c.deferred_batches > 0)
            .count()
    }
    pub fn discovery_ready(
        &self,
        reference: &WorldReference,
        selected: &[Placement],
        replays: &[Placement],
    ) -> Result<bool, ReferenceError> {
        for (parent, replay) in selected
            .iter()
            .map(|p| (p, false))
            .chain(replays.iter().map(|p| (p, true)))
        {
            // As in `advance`, a replay root discovers through its replay cursor only.
            if !replay && replays.iter().any(|root| root.id == parent.id) {
                continue;
            }
            if !parent.id.children.is_empty()
                && self
                    .rules
                    .component(reference, &parent.prototype, "buildingscript")?
                    .is_none()
            {
                // Synchronous descendants retain their completed Starts with their selected output.
                if self
                    .active
                    .iter()
                    .any(|c| c.content.placement.id == parent.id)
                {
                    if self
                        .deferred
                        .iter()
                        .any(|b| b.tasks[b.next..].iter().any(|(p, _)| p.id == parent.id))
                    {
                        return Ok(false);
                    }
                    continue;
                }
                // Checked helper placements have no building lifecycle.
                if self
                    .rules
                    .component(reference, &parent.prototype, "buildingscript")?
                    .is_none()
                {
                    continue;
                }
            }
            let Some(cursor) = self
                .cursors
                .iter()
                .find(|c| c.parent.id == parent.id && c.replay_only == replay)
            else {
                return Ok(false);
            };
            if cursor.root.is_none()
                || cursor.initial_eligible.is_none()
                || (cursor.building.is_some()
                    && cursor.initial_eligible == Some(true)
                    && cursor.next < cursor.callers.len().max(1))
                || self
                    .deferred
                    .iter()
                    .any(|b| b.owner == parent.id && b.replay_only == replay)
            {
                return Ok(false);
            }
        }
        Ok(self.deferred.is_empty())
    }
    pub fn initialized(&self, id: &GeneratedId) -> bool {
        self.cursors
            .iter()
            .find(|c| &c.parent.id == id)
            .is_none_or(|c| c.root.is_some())
            && !self
                .deferred
                .iter()
                .any(|b| b.tasks[b.next..].iter().any(|(p, _)| &p.id == id))
    }
    pub fn selected_slot_part<'a>(
        &'a self,
        parent: &GeneratedId,
        source: &SourceId,
        reference: &'a WorldReference,
    ) -> Result<Option<&'a SourceId>, ReferenceError> {
        for content in self.instances() {
            if let Some((_, _, part)) = content
                .inputs
                .slot_parts
                .iter()
                .find(|(p, s, _)| p == parent && s == source)
            {
                return Ok(Some(part));
            }
        }
        optional(self.rules.get(reference, source)?.fields, "part")
    }
    pub fn instances(&self) -> impl Iterator<Item = &SpawnedContent> {
        self.cursors
            .iter()
            .filter_map(|c| c.root.as_ref())
            .chain(self.active.iter().map(|c| &c.content))
    }
    /// The active content or cursor root of placement `id`.
    pub fn instance_mut(&mut self, id: &GeneratedId) -> Option<&mut SpawnedContent> {
        if let Some(i) = self
            .active
            .iter()
            .position(|c| &c.content.placement.id == id)
        {
            return Some(&mut self.active[i].content);
        }
        self.cursors
            .iter_mut()
            .filter_map(|c| c.root.as_mut())
            .find(|c| &c.placement.id == id)
    }
    /// Parent placement and root content of the cursors with a root.
    pub fn cursor_roots(&self) -> impl Iterator<Item = (&GeneratedId, &SpawnedContent)> {
        self.cursors
            .iter()
            .filter_map(|c| c.root.as_ref().map(|root| (&c.parent.id, root)))
    }
    pub fn appearance(&self, id: &GeneratedId) -> &[super::NodeAppearance] {
        self.active
            .iter()
            .find(|c| &c.content.placement.id == id)
            .map(|c| c.content.appearance.as_slice())
            .or_else(|| {
                self.cursors
                    .iter()
                    .find(|c| &c.parent.id == id)
                    .and_then(|c| c.root.as_ref().map(|c| c.appearance.as_slice()))
            })
            .unwrap_or(&[])
    }
    pub fn cursor_appearance(&self, id: &GeneratedId) -> &[super::NodeAppearance] {
        self.cursors
            .iter()
            .find(|c| &c.parent.id == id)
            .and_then(|c| c.root.as_ref().map(|c| c.appearance.as_slice()))
            .unwrap_or(&[])
    }
    pub fn contains(&self, id: &GeneratedId) -> bool {
        self.active.iter().any(|c| c.content.placement.id == *id)
    }
    pub fn active(&self) -> &[ContentInstance] {
        &self.active
    }
    /// Active contents for edit, with the rules and box releases for reading.
    pub fn active_parts(
        &mut self,
    ) -> (
        &mut Vec<ContentInstance>,
        &SourceRules,
        &super::stream::content_boxes::BoxReleases,
    ) {
        (&mut self.active, &self.rules, &self.boxes)
    }
    pub fn active_mut(&mut self) -> &mut Vec<ContentInstance> {
        &mut self.active
    }
}
/// Building callers in source order, without `dontSpawn` callers.
pub(super) fn eligible_callers(
    reference: &WorldReference,
    rules: &SourceRules,
    prototype: &SourceId,
) -> Result<Vec<SourceId>, ReferenceError> {
    let mut callers = Vec::new();
    rules.children(
        reference,
        prototype,
        "newRandomStuffSpawnScript",
        true,
        &mut callers,
    )?;
    let mut eligible = Vec::new();
    for source in callers {
        if !flag(rules.get(reference, source)?.fields, "dontSpawn")? {
            eligible.push(source.clone());
        }
    }
    Ok(eligible)
}
/// One building cursor step: the building entry at step 0, then caller `next` with the
/// `buildingAffectsThis` chance. Returns the building chance for the next steps.
pub(super) fn spawn_step(
    evaluation: &mut SpawnEvaluation,
    parent: &Placement,
    building: &SourceId,
    callers: &[SourceId],
    next: usize,
    chance: Option<f32>,
) -> Result<Option<f32>, ReferenceError> {
    let chance = if next == 0 {
        evaluation.building(parent, building)?
    } else {
        chance
    };
    if let Some(caller) = callers.get(next) {
        let f = evaluation.rules.get(evaluation.reference, caller)?.fields;
        let affected = flag(f, "buildingAffectsThis")?;
        evaluation.spawn(parent, caller, if affected { chance } else { None })?;
    }
    Ok(chance)
}
pub fn distance(a: [f64; 3], b: [f64; 3]) -> f32 {
    let x = (a[0] - b[0]) as f32;
    let z = (a[2] - b[2]) as f32;
    x * x + z * z
}

#[cfg(test)]
mod tests {
    use super::*;
    fn reference() -> &'static WorldReference {
        static REFERENCE: std::sync::OnceLock<crate::reference::ReferenceManifest> =
            std::sync::OnceLock::new();
        &REFERENCE
            .get_or_init(|| {
                crate::reference::ReferenceManifest::from_runtime(
                    &crate::reference::read_manifest(crate::reference::INSTALLED_MANIFEST).unwrap(),
                )
                .unwrap()
            })
            .world
    }

    /// Expectations were recorded at the 500 m source minimum.
    fn spawn(reference: &WorldReference) -> f32 {
        reference.draw_distance.minimum.item_spawn_dist
    }

    fn cursors(reference: &WorldReference) -> SourceContents {
        let mut contents = SourceContents::new(reference, 1);
        let prototype = reference.buildings.types[0].prototype.clone();
        let building = contents
            .rules
            .component(reference, &prototype, "buildingscript")
            .unwrap()
            .unwrap()
            .source
            .clone();
        for ordinal in 0..4 {
            let parent = Placement {
                id: super::super::GeneratedKey {
                    layer: super::super::WorldLayer::Building,
                    generator: reference.buildings.source.clone(),
                    unit: [0; 2],
                    ordinal,
                    children: Vec::new(),
                }
                .into(),
                prototype: prototype.clone(),
                pose: super::super::AbsolutePose {
                    position: [0.; 3],
                    rotation: [0., 0., 0., 1.],
                },
                scale: [1.; 3],
                bounds: super::super::WorldBounds {
                    min: [0.; 3],
                    max: [0.; 3],
                },
                object_seed: 42,
            };
            contents.cursors.push(Cursor {
                root: Some(ContentInstance::for_structure(parent.clone()).content),
                parent,
                building: Some(building.clone()),
                replay_only: false,
                deferred_batches: 0,
                initial_eligible: None,
                callers: Vec::new(),
                next: 0,
                chance: None,
                inputs: Default::default(),
            });
        }
        contents
    }

    #[test]
    #[ignore = "requires the installed source export"]
    fn membership_revisions_cover_push_remove_and_retain() {
        let mut contents = cursors(reference());
        let placements: Vec<_> = contents.cursors.iter().map(|c| c.parent.clone()).collect();
        contents.push_active(ContentInstance::for_structure(placements[0].clone()));
        contents.push_active(ContentInstance::for_structure(placements[1].clone()));
        contents.push_active(ContentInstance::for_structure(placements[2].clone()));
        assert_eq!(contents.active_revision(), 3);
        contents.remove_active(0);
        assert_eq!(contents.active_revision(), 4);
        assert_eq!(contents.active[0].content.placement.id, placements[2].id);
        contents.push_active(ContentInstance::for_structure(placements[3].clone()));
        assert_eq!(contents.active.len(), 3);
        assert_eq!(contents.active_revision(), 5);
        contents.active[0].replay_only = true;
        contents.retain_requests(&placements, &placements);
        assert_eq!(contents.active_revision(), 5);
        contents.retain_requests(&placements, &[]);
        assert_eq!(contents.active_revision(), 6);
        assert_eq!(contents.active.len(), 2);
        contents.cursors[1].replay_only = true;
        contents.cursors[1].building = None;
        contents.discard_replay_siblings(&placements);
        assert_eq!(contents.active_revision(), 6);
        contents.discard_replay_siblings(&[]);
        assert_eq!(contents.active_revision(), 7);
        assert_eq!(contents.active.len(), 1);
        assert_eq!(contents.active[0].content.placement.id, placements[3].id);
        contents.discard_replay_siblings(&[]);
        assert_eq!(contents.active_revision(), 7);
    }

    #[test]
    #[ignore = "requires the installed source export"]
    fn final_cursor_scan_skips_reuse_exact_total() {
        let reference = reference();
        let mut contents = cursors(reference);
        for (index, cursor) in contents.cursors.iter_mut().enumerate() {
            if index == 0 {
                cursor.building = None;
            } else if index == 1 {
                cursor.initial_eligible = Some(false);
            } else {
                cursor.next = 1;
            }
        }
        let history = contents.state.capture_history();
        contents
            .advance(
                reference,
                spawn(reference),
                &[],
                &[],
                &[],
                [[0.; 3]; 2],
                1,
                1,
                1,
            )
            .unwrap();
        assert_eq!(contents.next, 0);
        assert!(contents.active.is_empty());
        assert!(contents.deferred.is_empty());
        assert_eq!(
            contents.cursors.iter().map(|c| c.next).collect::<Vec<_>>(),
            [0, 0, 1, 1]
        );
        assert_eq!(contents.state.capture_history(), history);
    }

    #[test]
    #[ignore = "requires the installed source export"]
    fn final_cursor_scan_refreshes_after_work() {
        let reference = reference();
        let mut contents = cursors(reference);
        for cursor in contents.cursors.iter_mut().skip(1) {
            cursor.building = None;
        }
        let live = contents.state.live_state();
        contents
            .advance(
                reference,
                spawn(reference),
                &[],
                &[],
                &[],
                [[0.; 3]; 2],
                128,
                128,
                128,
            )
            .unwrap();
        assert_eq!(contents.next, 0);
        assert_eq!(
            contents.cursors.iter().map(|c| c.next).collect::<Vec<_>>(),
            [1, 0, 0, 0]
        );
        assert_eq!(contents.state.boundary_count(), 1);
        let history = contents.state.capture_history();
        let boundary = &history.boundaries[0];
        let parent = &contents.cursors[0].parent.id;
        assert_eq!(
            history.sources[boundary.parent.generator as usize],
            parent.generator
        );
        assert_eq!(
            (boundary.parent.unit, boundary.parent.ordinal),
            (parent.unit, parent.ordinal)
        );
        assert_eq!(
            history.sources[boundary.caller as usize],
            *contents.cursors[0].building.as_ref().unwrap()
        );
        assert_eq!(boundary.phase, SpawnPhase::SynchronousAndStarts);
        assert_eq!(boundary.ordinal, 0);
        assert_eq!(boundary.random, live);
        assert_eq!(boundary.initialization, None);
        assert_eq!(
            contents.state.live_state().words,
            [4005824999, 1007970560, 4283702805, 3403211127]
        );
        let expected = contents.cursors[0].parent.id.with_child(
            &SourceId {
                file: "sharedassets1.assets".into(),
                path_id: 154992,
            },
            0,
        );
        assert_eq!(
            contents
                .active
                .iter()
                .map(|c| &c.content.placement.id)
                .collect::<Vec<_>>(),
            [&expected]
        );
        assert!(contents.deferred.is_empty());
        assert_eq!(contents.cursors[0].chance, None);
    }
}
