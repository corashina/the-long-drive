use crate::reference::{
    ConditionId, ConditionStartup, ConditionSurface, ItemId, ReferenceError, ReferenceManifest,
    WashId,
};
use crate::save::{ConditionSnapshot, SaveError, SourceKey, SurfaceSnapshot, WashSnapshot};
use std::collections::{HashMap, VecDeque};

pub type ConditionState = ConditionStartup;
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RepairResult {
    None,
    Improved,
    Worsened,
}
#[derive(Clone, Copy, Debug)]
pub struct WashState {
    pub active: bool,
    pub on: bool,
    pub destroyed: bool,
    pub repair_progress: f32,
}

impl WashState {
    pub fn visible(&self) -> bool {
        self.active && !self.destroyed
    }
}

/// `partconditionscript` fields that repair, paint, wear and refresh read.
#[derive(Clone, Debug, Default)]
pub struct ConditionRule {
    pub permission_index: u32,
    pub children: Vec<ConditionId>,
    pub is_child: bool,
    pub no_inherit_state: bool,
    pub same_state_as_parent: bool,
    pub random_start_state3: bool,
}
impl ConditionRule {
    fn of(s: &ConditionSurface) -> Self {
        Self {
            permission_index: s.permission_index,
            children: s.children.clone(),
            is_child: s.is_child,
            no_inherit_state: s.no_inherit_state,
            same_state_as_parent: s.same_state_as_parent,
            random_start_state3: s.random_start_state3,
        }
    }
}

/// The save key of a world part condition: its item and source, and its spawn state.
#[derive(Clone, Debug)]
pub struct WorldSave {
    pub item: ItemId,
    pub source: SourceKey,
    pub spawn: ConditionState,
}

/// Condition owners: exported garage surfaces, then world parts.
/// World ids start above the exported ids. Removed world ids are reused.
pub struct ConditionStore {
    states: Vec<ConditionState>,
    rules: Vec<ConditionRule>,
    /// Garage surfaces, in entry order.
    records: Vec<ConditionSurface>,
    indices: HashMap<ConditionId, usize>,
    first_world: u32,
    live: Vec<bool>,
    free: Vec<usize>,
    /// World slots from `records.len()`. None when no item saves the entry.
    saves: Vec<Option<WorldSave>>,
    items: HashMap<ItemId, ConditionId>,
    tables: Vec<[[bool; 7]; 5]>,
    decay_rate: f32,
    pub washes: Vec<WashState>,
    wash_indices: HashMap<WashId, usize>,
    dirty: VecDeque<ConditionId>,
    world_dirty: VecDeque<ConditionId>,
    queued: Vec<bool>,
    /// Entries with non-zero repair progress. Only these decay.
    progress: Vec<usize>,
    in_progress: Vec<bool>,
    traversal: Vec<ConditionId>,
}
impl ConditionStore {
    pub fn new(reference: &ReferenceManifest) -> Result<Self, ReferenceError> {
        reference.conditions.validate(reference)?;
        let r = &reference.conditions;
        let washes = r
            .washes
            .iter()
            .map(|w| {
                (
                    w.id,
                    WashState {
                        active: w.active,
                        on: w.on,
                        destroyed: false,
                        repair_progress: w.repair_progress,
                    },
                )
            })
            .collect();
        Ok(Self::from_parts(
            r.surfaces.clone(),
            r.repair_tables.clone(),
            r.repair_decay_rate,
            washes,
        ))
    }
    fn from_parts(
        records: Vec<ConditionSurface>,
        tables: Vec<[[bool; 7]; 5]>,
        decay_rate: f32,
        washes: Vec<(WashId, WashState)>,
    ) -> Self {
        let n = records.len();
        let mut store = Self {
            states: records.iter().map(|s| s.startup).collect(),
            rules: records.iter().map(ConditionRule::of).collect(),
            indices: records.iter().enumerate().map(|(i, s)| (s.id, i)).collect(),
            first_world: records.iter().map(|s| s.id.0 + 1).max().unwrap_or(1),
            live: vec![true; n],
            free: Vec::new(),
            saves: Vec::new(),
            items: records
                .iter()
                .filter_map(|s| s.item.map(|i| (i, s.id)))
                .collect(),
            tables,
            decay_rate,
            wash_indices: washes
                .iter()
                .enumerate()
                .map(|(i, (id, _))| (*id, i))
                .collect(),
            washes: washes.into_iter().map(|(_, w)| w).collect(),
            dirty: VecDeque::with_capacity(n),
            world_dirty: VecDeque::new(),
            queued: vec![false; n],
            progress: Vec::new(),
            in_progress: vec![false; n],
            traversal: Vec::with_capacity(
                1 + records.iter().map(|s| s.children.len()).sum::<usize>(),
            ),
            records,
        };
        for i in 0..n {
            store.track_progress(i);
        }
        store
    }
    fn index(&self, id: ConditionId) -> usize {
        if id.0 >= self.first_world {
            self.records.len() + (id.0 - self.first_world) as usize
        } else {
            self.indices[&id]
        }
    }
    fn id(&self, index: usize) -> ConditionId {
        if index < self.records.len() {
            self.records[index].id
        } else {
            ConditionId(self.first_world + (index - self.records.len()) as u32)
        }
    }
    pub fn is_world(&self, id: ConditionId) -> bool {
        id.0 >= self.first_world
    }
    /// Adds a world part condition. Set its children with `set_children` once they exist.
    pub fn add_world(
        &mut self,
        mut rule: ConditionRule,
        state: ConditionState,
        item: Option<ItemId>,
        save: Option<(ItemId, SourceKey)>,
    ) -> Result<ConditionId, ReferenceError> {
        if rule.permission_index as usize >= self.tables.len() {
            return Err(ReferenceError("World condition repair property".into()));
        }
        rule.children.clear();
        let index = match self.free.pop() {
            Some(index) => {
                self.states[index] = state;
                self.rules[index] = rule;
                self.live[index] = true;
                index
            }
            None => {
                self.states.push(state);
                self.rules.push(rule);
                self.live.push(true);
                self.queued.push(false);
                self.in_progress.push(false);
                self.states.len() - 1
            }
        };
        let save = save.map(|(item, source)| WorldSave {
            item,
            source,
            spawn: state,
        });
        match self.saves.get_mut(index - self.records.len()) {
            Some(slot) => *slot = save,
            None => self.saves.push(save),
        }
        let id = self.id(index);
        if let Some(item) = item {
            self.items.insert(item, id);
        }
        self.track_progress(index);
        Ok(id)
    }
    pub fn set_children(&mut self, id: ConditionId, children: &[ConditionId]) {
        let i = self.index(id);
        self.rules[i].children.clear();
        self.rules[i].children.extend_from_slice(children);
    }
    pub fn remove_world(&mut self, id: ConditionId) {
        let i = self.index(id);
        if !self.live[i] {
            return;
        }
        self.live[i] = false;
        self.saves[i - self.records.len()] = None;
        self.rules[i].children.clear();
        self.items.retain(|_, v| *v != id);
        if self.in_progress[i] {
            self.in_progress[i] = false;
            self.progress.retain(|v| *v != i);
        }
        self.free.push(i);
    }
    fn track_progress(&mut self, i: usize) {
        if self.states[i].repair_progress != 0. && !self.in_progress[i] {
            self.in_progress[i] = true;
            self.progress.push(i);
        }
    }
    pub fn update(&mut self, delta: f32, active: bool) {
        if !active || !delta.is_finite() || delta <= 0. {
            return;
        }
        let amount = self.decay_rate * delta;
        let mut n = 0;
        while n < self.progress.len() {
            let state = &mut self.states[self.progress[n]];
            state.repair_progress =
                state.repair_progress.signum() * (state.repair_progress.abs() - amount).max(0.);
            if state.repair_progress == 0. {
                self.in_progress[self.progress[n]] = false;
                self.progress.swap_remove(n);
            } else {
                n += 1;
            }
        }
    }
    pub fn get(&self, id: ConditionId) -> &ConditionState {
        &self.states[self.index(id)]
    }
    pub fn quality(&self, id: ConditionId) -> f32 {
        1. - f32::from(self.get(id).state.min(4)) / 4.
    }
    /// `partconditionscript.state` of an item.
    pub fn item_state(&self, item: ItemId) -> Option<u8> {
        self.items.get(&item).map(|id| self.get(*id).state)
    }
    pub fn item_quality(&self, item: ItemId) -> Option<f32> {
        self.items.get(&item).map(|id| self.quality(*id))
    }
    pub fn wash(&self, id: WashId) -> &WashState {
        &self.washes[self.wash_indices[&id]]
    }
    pub fn can_repair(&self, id: ConditionId, tool: u8) -> bool {
        let i = self.index(id);
        self.tables[self.rules[i].permission_index as usize]
            .get(self.states[i].state as usize)
            .and_then(|row| row.get(tool as usize))
            .copied()
            .unwrap_or(false)
    }
    pub fn repair(&mut self, id: ConditionId, tool: u8, gain: f32) -> RepairResult {
        if !gain.is_finite() || gain < 0. {
            return RepairResult::None;
        }
        let i = self.index(id);
        let result = if tool == 1 && self.can_repair(id, 3) && self.states[i].state < 4 {
            self.states[i].repair_progress -= gain;
            if self.states[i].repair_progress <= -1. {
                self.states[i].state += 1;
                RepairResult::Worsened
            } else {
                RepairResult::None
            }
        } else if self.states[i].state > 0 && self.can_repair(id, tool) {
            self.states[i].repair_progress += gain;
            if self.states[i].repair_progress >= 1. {
                self.states[i].state -= 1;
                RepairResult::Improved
            } else {
                RepairResult::None
            }
        } else {
            RepairResult::None
        };
        if result != RepairResult::None {
            self.states[i].repair_progress = 0.;
            self.refresh(id);
        }
        self.track_progress(i);
        result
    }
    pub fn paint(&mut self, id: ConditionId, color: [f32; 4]) -> bool {
        if !color.iter().all(|v| v.is_finite()) || !self.can_repair(id, 0) {
            return false;
        }
        let i = self.index(id);
        if self.states[i].color == color {
            return false;
        }
        self.states[i].color = color;
        self.refresh(id);
        true
    }
    pub fn wear(&mut self, id: ConditionId) -> bool {
        let i = self.index(id);
        if self.states[i].state >= 5 {
            return true;
        }
        self.states[i].state += 1;
        // Source tool disable does not call Refresh with material index five.
        if self.states[i].state < 5 {
            self.refresh(id);
        } else {
            self.mark(id);
        }
        self.states[i].state == 5
    }
    fn mark(&mut self, id: ConditionId) {
        let i = self.index(id);
        if !self.queued[i] {
            self.queued[i] = true;
            if i < self.records.len() {
                self.dirty.push_back(id);
            } else {
                self.world_dirty.push_back(id);
            }
        }
    }
    fn refresh(&mut self, id: ConditionId) {
        self.traversal.clear();
        self.traversal.push(id);
        while let Some(id) = self.traversal.pop() {
            let i = self.index(id);
            self.mark(id);
            let state = self.states[i];
            for child_index in 0..self.rules[i].children.len() {
                let child = self.rules[i].children[child_index];
                let ci = self.index(child);
                let c = &self.rules[ci];
                if !self.live[ci] || (!c.is_child && !c.same_state_as_parent) {
                    continue;
                }
                if !c.is_child || !self.rules[i].no_inherit_state {
                    let random_state3 = c.is_child && c.random_start_state3;
                    self.states[ci].state = state.state;
                    self.states[ci].state2 = state.state2;
                    if !random_state3 {
                        self.states[ci].state3 = state.state3;
                    }
                }
                // Startup already performed the source's one-time child color draw.
                self.traversal.push(child);
            }
        }
    }
    /// Garage surfaces only.
    pub fn iter(&self) -> impl Iterator<Item = (ConditionId, &ConditionState)> {
        self.records.iter().map(|r| r.id).zip(&self.states)
    }
    /// Changed garage surfaces.
    pub fn take_changed(&mut self) -> Option<ConditionId> {
        let id = self.dirty.pop_front()?;
        let i = self.index(id);
        self.queued[i] = false;
        Some(id)
    }
    /// Changed world part conditions that are still live.
    pub fn take_world_changed(&mut self) -> Option<ConditionId> {
        loop {
            let id = self.world_dirty.pop_front()?;
            let i = self.index(id);
            self.queued[i] = false;
            if self.live[i] {
                return Some(id);
            }
        }
    }
    /// The item that saves a world entry.
    pub fn world_owner(&self, id: ConditionId) -> Option<ItemId> {
        let i = self.index(id);
        self.saves
            .get(i.checked_sub(self.records.len())?)?
            .as_ref()
            .map(|s| s.item)
    }
    /// World entries of `item` that differ from their spawn state, in store order.
    pub fn item_surfaces(&self, item: ItemId) -> Vec<SurfaceSnapshot> {
        let first = self.records.len();
        self.saves
            .iter()
            .enumerate()
            .filter_map(|(i, s)| {
                let s = s.as_ref().filter(|s| s.item == item)?;
                let v = &self.states[first + i];
                (*v != s.spawn).then(|| SurfaceSnapshot {
                    source: s.source.clone(),
                    state: v.state,
                    state2: v.state2,
                    state3: v.state3,
                    color: v.color,
                    repair_progress: v.repair_progress,
                })
            })
            .collect()
    }
    /// Applies saved world entries of a bound item over their spawn states.
    pub fn restore_item(
        &mut self,
        item: ItemId,
        saved: &[SurfaceSnapshot],
    ) -> Result<(), ReferenceError> {
        let first = self.records.len();
        for v in saved {
            let i = self
                .saves
                .iter()
                .position(|s| {
                    s.as_ref()
                        .is_some_and(|s| s.item == item && s.source == v.source)
                })
                .ok_or_else(|| ReferenceError("Saved part condition has no owner".into()))?;
            self.states[first + i] = ConditionState {
                state: v.state,
                state2: v.state2,
                state3: v.state3,
                color: v.color,
                repair_progress: v.repair_progress,
            };
            self.track_progress(first + i);
            self.mark(self.id(first + i));
        }
        Ok(())
    }
    pub fn capture_save(
        &self,
        reference: &ReferenceManifest,
    ) -> Result<(Vec<SurfaceSnapshot>, Vec<WashSnapshot>), SaveError> {
        Ok((
            reference
                .conditions
                .surfaces
                .iter()
                .map(|s| {
                    let v = self.get(s.id);
                    SurfaceSnapshot {
                        source: SourceKey::from(&s.source),
                        state: v.state,
                        state2: v.state2,
                        state3: v.state3,
                        color: v.color,
                        repair_progress: v.repair_progress,
                    }
                })
                .collect(),
            reference
                .conditions
                .washes
                .iter()
                .map(|w| {
                    let v = self.wash(w.id);
                    WashSnapshot {
                        source: SourceKey::from(&w.source),
                        active: v.active,
                        on: v.on,
                        destroyed: v.destroyed,
                        repair_progress: v.repair_progress,
                    }
                })
                .collect(),
        ))
    }
    pub fn restore_save(
        &mut self,
        reference: &ReferenceManifest,
        saved: &ConditionSnapshot,
    ) -> Result<(), SaveError> {
        validate_surfaces(reference, saved)?;
        for s in &reference.conditions.surfaces {
            let v = saved
                .surfaces
                .iter()
                .find(|v| v.source == SourceKey::from(&s.source))
                .unwrap();
            let i = self.indices[&s.id];
            self.states[i] = ConditionState {
                state: v.state,
                state2: v.state2,
                state3: v.state3,
                color: v.color,
                repair_progress: v.repair_progress,
            };
            self.track_progress(i);
            self.mark(s.id);
        }
        for w in &reference.conditions.washes {
            let v = saved
                .washes
                .iter()
                .find(|v| v.source == SourceKey::from(&w.source))
                .unwrap();
            self.washes[self.wash_indices[&w.id]] = WashState {
                active: v.active,
                on: v.on,
                destroyed: v.destroyed,
                repair_progress: v.repair_progress,
            };
        }
        Ok(())
    }
}
pub fn validate_surfaces(
    reference: &ReferenceManifest,
    saved: &ConditionSnapshot,
) -> Result<(), SaveError> {
    let invalid = |s: &str| SaveError::CorruptData(s.into());
    if saved.surfaces.len() != reference.conditions.surfaces.len()
        || saved.washes.len() != reference.conditions.washes.len()
    {
        return Err(invalid("Missing condition owner"));
    }
    for r in &reference.conditions.surfaces {
        let mut entries = saved
            .surfaces
            .iter()
            .filter(|v| v.source == SourceKey::from(&r.source));
        let v = entries
            .next()
            .ok_or_else(|| invalid("Missing surface owner"))?;
        let disabled = reference
            .conditions
            .brushes
            .iter()
            .any(|b| b.own_condition == r.id);
        if entries.next().is_some()
            || v.state > if disabled { 5 } else { 4 }
            || !v.color.iter().all(|c| c.is_finite())
            || !v.repair_progress.is_finite()
            || v.repair_progress.abs() >= 1.
            || (r.use_mesh_list
                && r.renderers
                    .iter()
                    .any(|renderer| v.state2 as usize >= renderer.meshes.len()))
            || (r.use_object_list && !r.objects.is_empty() && v.state3 as usize >= r.objects.len())
            || (!r.use_mesh_list && v.state2 != r.startup.state2)
            || (!r.use_object_list && v.state3 != r.startup.state3)
        {
            return Err(invalid("Invalid surface state or duplicate owner"));
        }
    }
    for r in &reference.conditions.washes {
        let mut entries = saved
            .washes
            .iter()
            .filter(|v| v.source == SourceKey::from(&r.source));
        let v = entries
            .next()
            .ok_or_else(|| invalid("Missing wash owner"))?;
        if entries.next().is_some()
            || !v.repair_progress.is_finite()
            || (v.repair_progress < 0. || (!r.never_repair && v.repair_progress > 1.))
            || (v.destroyed && (!r.destroy_on_repair || v.active || v.on))
        {
            return Err(invalid("Invalid wash state or duplicate owner"));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn state(state: u8) -> ConditionState {
        ConditionState {
            state,
            state2: 0,
            state3: 0,
            repair_progress: 0.,
            color: [1.; 4],
        }
    }
    fn store() -> ConditionStore {
        ConditionStore::from_parts(Vec::new(), vec![[[true; 7]; 5]], 0.5, Vec::new())
    }

    #[test]
    fn world_entries_reuse_removed_ids() {
        let mut s = store();
        let a = s
            .add_world(ConditionRule::default(), state(1), Some(ItemId(7)), None)
            .unwrap();
        let b = s
            .add_world(ConditionRule::default(), state(2), None, None)
            .unwrap();
        assert!(s.is_world(a) && a != b);
        assert_eq!(s.item_state(ItemId(7)), Some(1));
        s.remove_world(a);
        assert_eq!(s.item_state(ItemId(7)), None);
        let c = s
            .add_world(ConditionRule::default(), state(3), None, None)
            .unwrap();
        assert_eq!(c, a);
        assert_eq!(s.get(c).state, 3);
        assert_eq!(s.get(b).state, 2);
        assert!(
            s.add_world(
                ConditionRule {
                    permission_index: 1,
                    ..Default::default()
                },
                state(0),
                None,
                None
            )
            .is_err()
        );
    }

    #[test]
    fn world_entries_save_only_changes_of_their_item() {
        let key = |path_id: &str| SourceKey {
            file: "f".into(),
            path_id: path_id.into(),
        };
        let mut s = store();
        let a = s
            .add_world(
                ConditionRule::default(),
                state(2),
                None,
                Some((ItemId(7), key("1"))),
            )
            .unwrap();
        s.add_world(
            ConditionRule::default(),
            state(2),
            None,
            Some((ItemId(7), key("2"))),
        )
        .unwrap();
        assert_eq!(s.world_owner(a), Some(ItemId(7)));
        assert!(s.item_surfaces(ItemId(7)).is_empty());
        assert!(s.paint(a, [1., 0., 0., 1.]));
        let saved = s.item_surfaces(ItemId(7));
        assert_eq!(saved.len(), 1);
        assert_eq!(saved[0].source, key("1"));
        let mut loaded = store();
        let b = loaded
            .add_world(
                ConditionRule::default(),
                state(2),
                None,
                Some((ItemId(9), key("1"))),
            )
            .unwrap();
        loaded.restore_item(ItemId(9), &saved).unwrap();
        assert_eq!(loaded.get(b).color, [1., 0., 0., 1.]);
        assert_eq!(loaded.take_world_changed(), Some(b));
        assert!(loaded.restore_item(ItemId(8), &saved).is_err());
    }

    #[test]
    fn world_repair_refreshes_children_and_decays() {
        let mut s = store();
        let parent = s
            .add_world(ConditionRule::default(), state(2), None, None)
            .unwrap();
        let child = s
            .add_world(
                ConditionRule {
                    is_child: true,
                    ..Default::default()
                },
                state(2),
                None,
                None,
            )
            .unwrap();
        s.set_children(parent, &[child]);
        assert_eq!(s.repair(parent, 2, 0.4), RepairResult::None);
        s.update(0.2, true);
        assert!((s.get(parent).repair_progress - 0.3).abs() < 1e-6);
        s.update(1., true);
        assert_eq!(s.get(parent).repair_progress, 0.);
        assert!(s.progress.is_empty());
        assert_eq!(s.repair(parent, 2, 1.), RepairResult::Improved);
        assert_eq!(s.get(child).state, 1);
        assert_eq!(s.take_changed(), None);
        let mut changed = Vec::new();
        while let Some(id) = s.take_world_changed() {
            changed.push(id);
        }
        assert_eq!(changed, vec![parent, child]);
        s.remove_world(child);
        assert!(s.paint(parent, [0.5; 4]));
        assert_eq!(s.take_world_changed(), Some(parent));
    }
}
