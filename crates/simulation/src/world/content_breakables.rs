//! Binds source breakablescripts of active content to `damage::Breakables` targets, steps their hits
//! and removes broken items.
//! Items bind by their world prototype, spawned copies by their item prototype. Lamp triggers bind
//! by their structure lamp control.
use super::WorldStream;
use crate::damage::BreakKind;
use crate::interaction::items::{ItemLocation, ItemStore};
use crate::interaction::storage::StorageLocks;
use crate::reference::{
    BreakableOwnerKind, BreakableReference, ItemId, LightControlId, ReferenceError,
    ReferenceManifest, SourceId,
};
use crate::starting_area::spawned::SpawnedItems;
use crate::vehicle::PhysicsWorld;
use crate::world::GeneratedId;
use rapier3d::prelude::ActiveEvents;

/// The node of the breakablescript in a world prototype.
fn world_node(record: &BreakableReference, prototype: &SourceId) -> Option<u32> {
    record
        .owners
        .iter()
        .find(|o| o.kind == BreakableOwnerKind::World && &o.prototype == prototype)
        .map(|o| o.node)
}

/// The manifest node of the breakablescript in an item prototype.
fn item_node(record: &BreakableReference, prototype: &SourceId) -> Option<u32> {
    record
        .owners
        .iter()
        .find(|o| o.kind == BreakableOwnerKind::Item && &o.prototype == prototype)
        .map(|o| o.node)
}

impl WorldStream {
    /// Run after the physics step. Queues the targets that break this step.
    pub fn step_breakables(
        &mut self,
        reference: &ReferenceManifest,
        items: &ItemStore,
        spawned: &SpawnedItems,
        physics: &mut PhysicsWorld,
        dt: f32,
    ) {
        self.refresh_boxes();
        self.refresh_breakables(reference, items, spawned, physics);
        self.breakables.step_hits(
            &reference.breakables,
            physics,
            &self.trigger_roles,
            |item| items.get(item).and_then(|s| s.body),
            dt,
        );
    }
    /// Next target that broke in `step_breakables`, in break order: its kind and record index.
    pub fn take_break(&mut self) -> Option<(BreakKind, usize)> {
        let target = self.breakables.take_break()?;
        let target = &self.breakables.targets()[target];
        Some((target.kind.clone(), target.record))
    }
    /// The placement pose of a bound prop structure.
    pub fn prop_pose(&self, parent: &GeneratedId) -> Option<crate::world::AbsolutePose> {
        self.structures
            .iter()
            .find(|s| &s.placement.id == parent)
            .map(|s| s.placement.pose)
    }
    /// Source Break: one random clip of the record.
    pub fn pick_break_clip<'a>(&mut self, record: &'a BreakableReference) -> Option<&'a SourceId> {
        self.breakables.pick_clip(record)
    }
    /// Source Break destroys the item unless noDestroy. A box releases its content first.
    /// Items locked in the item unlock at their current pose. The removal is saved.
    pub fn remove_broken_item(
        &mut self,
        reference: &ReferenceManifest,
        record: usize,
        item: ItemId,
        items: &mut ItemStore,
        storage: &mut StorageLocks,
        physics: &mut PhysicsWorld,
    ) -> Result<(), ReferenceError> {
        if !items
            .get(item)
            .is_some_and(|s| !s.removed && s.location == ItemLocation::World && s.body.is_some())
        {
            return Ok(());
        }
        if reference.breakables[record].source == reference.acquisition.r#box.breakable {
            self.break_box(item, items);
        }
        self.remove_item(item, items, storage, physics)
    }
    /// Destroy of a world content item. Items locked in it unlock at their current pose. The
    /// removal is saved.
    pub(super) fn remove_item(
        &mut self,
        item: ItemId,
        items: &mut ItemStore,
        storage: &mut StorageLocks,
        physics: &mut PhysicsWorld,
    ) -> Result<(), ReferenceError> {
        let Some(body) = items.get(item).and_then(|s| s.body) else {
            return Ok(());
        };
        storage.unbind_owner(item, |_, _| false, items, physics)?;
        self.changes.remove(items.object_key(item));
        physics.edit_body(body, |body| body.set_enabled(false));
        physics.refresh_queries();
        items.get_mut(item).expect("removed item").removed = true;
        items.mark_dirty(item);
        Ok(())
    }
    /// Source Break of a lamp trigger: flashlightscript.Break, then Destroy unless noDestroy.
    /// Returns the trigger position for the break clip.
    pub fn break_lamp(
        &mut self,
        reference: &ReferenceManifest,
        physics: &mut PhysicsWorld,
        parent: &GeneratedId,
        control: LightControlId,
        record: usize,
    ) -> Result<Option<[f32; 3]>, ReferenceError> {
        let rules = &reference.breakables[record];
        let lamps = rules
            .lamp
            .as_ref()
            .expect("lamp record")
            .lamps
            .iter()
            .fold(0u64, |bits, i| bits | 1 << i);
        self.break_generated_lamps(parent, control, lamps)?;
        let Some(target) = self.breakables.targets().iter().position(|t| {
            t.record == record
                && matches!(&t.kind, BreakKind::Lamp { parent: p, control: c } if p == parent && *c == control)
        }) else {
            return Ok(None);
        };
        let position = self
            .breakables
            .colliders_of(target)
            .next()
            .map(|c| physics.rapier().colliders[c].translation().to_array());
        if !rules.no_destroy {
            self.breakables.remove(target);
            for collider in self.breakables.colliders_of(target) {
                physics.edit_collider(collider, |c| c.set_enabled(false));
            }
            physics.refresh_queries();
        }
        Ok(position)
    }
    /// Rebinds targets on a content, item or lamp revision change only.
    fn refresh_breakables(
        &mut self,
        reference: &ReferenceManifest,
        items: &ItemStore,
        spawned: &SpawnedItems,
        physics: &mut PhysicsWorld,
    ) {
        let scan = [
            self.request.session,
            self.contents.active_revision(),
            items.membership_revision(),
            self.lamp_binding_revision,
        ];
        if !self.breakables.rescan(scan) {
            return;
        }
        let records = &reference.breakables;
        let active = &self.contents.active;
        let lamps = &self.lamp_bindings;
        let structures = &self.structures;
        let present = |item| items.get(item).is_some_and(|s| !s.removed);
        self.breakables.retain(|t| match &t.kind {
            BreakKind::Item(item) => {
                present(*item)
                    && (spawned.contains(*item) || active.iter().any(|c| c.item == Some(*item)))
            }
            // Lamp bindings can outlive their structure for a step after a far move.
            BreakKind::Lamp { parent, control } => {
                lamps
                    .iter()
                    .any(|b| b.control == *control && &b.parent == parent)
                    && structures.iter().any(|s| &s.placement.id == parent)
            }
            BreakKind::Prop { parent } => structures.iter().any(|s| &s.placement.id == parent),
        });
        // A content item whose world prototype holds a breakablescript without a lamp.
        for content in active {
            let Some(item) = content.item.filter(|i| present(*i)) else {
                continue;
            };
            let prototype = &content.content.placement.prototype;
            if let Some(record) = records
                .iter()
                .position(|r| r.lamp.is_none() && world_node(r, prototype).is_some())
            {
                self.breakables
                    .bind(BreakKind::Item(item), record, &records[record]);
            }
        }
        // A spawned prototype copy whose pickup node holds a breakablescript without a lamp.
        for copy in spawned.iter().filter(|c| present(c.id)) {
            let prototype = &reference.item_prototypes[copy.prototype];
            let node = prototype.pickups[copy.pickup].node;
            if let Some(record) = records
                .iter()
                .position(|r| r.lamp.is_none() && item_node(r, &prototype.id.0) == Some(node.0))
            {
                self.breakables
                    .bind(BreakKind::Item(copy.id), record, &records[record]);
            }
        }
        // Source OnCollisionEnter runs on every contact. Item colliders without a hit sound report
        // no contacts, so bound item colliders report them here.
        for target in self.breakables.targets() {
            let BreakKind::Item(item) = target.kind else {
                continue;
            };
            let Some(body) = items.get(item).and_then(|s| s.body) else {
                continue;
            };
            for index in 0..physics.rapier().bodies[body].colliders().len() {
                let collider = physics.rapier().bodies[body].colliders()[index];
                let events = physics.rapier().colliders[collider].active_events();
                if !events.contains(ActiveEvents::COLLISION_EVENTS) {
                    physics.edit_collider(collider, |c| {
                        c.set_active_events(events | ActiveEvents::COLLISION_EVENTS)
                    });
                }
            }
        }
        // A prop structure whose world prototype holds a breakablescript without a lamp.
        let props = records.iter().enumerate().filter(|(_, r)| r.lamp.is_none());
        let props = props.flat_map(|(i, r)| {
            r.owners
                .iter()
                .filter(|o| o.kind == BreakableOwnerKind::World)
                .map(move |o| (&o.prototype, i))
        });
        // Rescans follow content changes, not frames.
        let props: Vec<(&SourceId, usize)> = props.collect();
        for instance in structures {
            if let Some(&(_, record)) = props
                .iter()
                .find(|(p, _)| *p == &instance.placement.prototype)
            {
                let kind = BreakKind::Prop {
                    parent: instance.placement.id.clone(),
                };
                self.breakables.bind(kind, record, &records[record]);
            }
        }
        // A structure lamp control: each breakablescript in its prototype that breaks its lamps.
        for binding in lamps {
            let Some(instance) = structures.iter().find(|s| s.placement.id == binding.parent)
            else {
                continue;
            };
            for (record, rules) in records.iter().enumerate() {
                if rules
                    .lamp
                    .as_ref()
                    .is_some_and(|l| l.control == binding.source)
                    && world_node(rules, &instance.placement.prototype).is_some()
                {
                    let kind = BreakKind::Lamp {
                        parent: binding.parent.clone(),
                        control: binding.control,
                    };
                    self.breakables.bind(kind, record, rules);
                }
            }
        }
        // Lamp trigger and prop colliders are the structure colliders on the breakablescript
        // node. Prop triggers report OnTriggerEnter.
        for index in 0..self.breakables.targets().len() {
            let target = &self.breakables.targets()[index];
            let (BreakKind::Lamp { parent, .. } | BreakKind::Prop { parent }) = &target.kind else {
                continue;
            };
            let prop = matches!(target.kind, BreakKind::Prop { .. });
            let record = target.record;
            let instance = structures
                .iter()
                .find(|s| &s.placement.id == parent)
                .expect("bound structure");
            let node = world_node(&records[record], &instance.placement.prototype)
                .expect("bound structure owner");
            let prototype = self
                .reference
                .prototypes
                .iter()
                .find(|p| p.source == instance.placement.prototype)
                .expect("structure prototype");
            for (source, handle) in &instance.colliders {
                if prototype
                    .colliders
                    .iter()
                    .any(|c| &c.source == source && c.node == node)
                {
                    self.breakables.bind_collider(*handle, index);
                    let events = physics.rapier().colliders[*handle].active_events();
                    if prop && !events.contains(ActiveEvents::COLLISION_EVENTS) {
                        physics.edit_collider(*handle, |c| {
                            c.set_active_events(events | ActiveEvents::COLLISION_EVENTS)
                        });
                    }
                }
            }
        }
        // A restored lamp trigger whose lamps are all broken was destroyed by its Break.
        let mut removed = false;
        for index in 0..self.breakables.targets().len() {
            let target = &self.breakables.targets()[index];
            let BreakKind::Lamp { parent, control } = &target.kind else {
                continue;
            };
            let rules = &records[target.record];
            let lamps = rules.lamp.as_ref().expect("lamp record");
            let Some(binding) = self
                .lamp_bindings
                .iter()
                .find(|b| b.control == *control && &b.parent == parent)
            else {
                continue;
            };
            let broken = self.contents.broken_lamps(parent, &binding.source);
            if target.removed
                || rules.no_destroy
                || lamps.lamps.is_empty()
                || lamps.lamps.iter().any(|i| broken & (1 << i) == 0)
            {
                continue;
            }
            self.breakables.remove(index);
            for collider in self.breakables.colliders_of(index) {
                physics.edit_collider(collider, |c| c.set_enabled(false));
            }
            removed = true;
        }
        if removed {
            physics.refresh_queries();
        }
    }
}
