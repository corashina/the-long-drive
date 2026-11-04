//! The one path that forgets a removed item in every interaction map.
use super::InteractionState;
use crate::reference::{ActionBinding, ItemId};
use rapier3d::prelude::RigidBodyHandle;

fn owned_by(binding: &ActionBinding, id: ItemId) -> bool {
    [
        binding.use_owner,
        binding.pickup_owner,
        binding.removal_owner,
        binding.push_owner,
    ]
    .contains(&Some(id))
}

impl InteractionState {
    /// Forget item `id` before it leaves the item store or becomes a removed record.
    /// `body` is the item body when it leaves the world with the item.
    /// Add each new per-item map here, so no removal path keeps a stale entry.
    pub fn forget_item(&mut self, id: ItemId, body: Option<RigidBodyHandle>) {
        self.collider_bindings
            .retain(|_, binding| !owned_by(binding, id));
        self.source_bindings
            .retain(|_, binding| !owned_by(binding, id));
        if let Some(body) = self.mechanism_bodies.remove(&id) {
            self.bodies.remove(&body);
        }
        if let Some(body) = self.push.remove_generated(id) {
            self.bodies.remove(&body);
        }
        if let Some(body) = body {
            self.bodies.retain(|_, handle| *handle != body);
        }
        if self
            .latched_mechanism
            .is_some_and(|(item, _, _, _)| item == id)
        {
            self.latched_mechanism = None;
        }
        self.carry.unregister_generated(id);
        self.unregister_hose(id);
        for slot in self.wheel_slots.values_mut() {
            slot.fits.remove(&id);
        }
        self.tire_fits
            .retain(|(rim, tire)| *rim != id && *tire != id);
        self.kinds.remove(&id);
        self.pickup_layers.remove(&id);
        self.removal_layers.remove(&id);
        self.usable_items.remove(&id);
        self.filling_items.remove(&id);
        self.supplies.remove(&id);
        self.source_supply_emission.remove(&id);
        self.tank_rules.remove(&id);
        self.held_controls.remove(&id);
        self.pending_drink_contacts
            .retain(|(item, _, _)| *item != id);
        self.forget_openings(id);
    }

    /// Forget the holes and openings of a removed tank or cap.
    fn forget_openings(&mut self, id: ItemId) {
        self.holes.forget(id);
        self.openings
            .retain(|opening| opening.reference.tank != id && opening.reference.cap != id);
        for opening in &mut self.openings {
            if opening.borrowed_tank == Some(id) {
                opening.borrowed_tank = None;
            }
        }
        let gone = |opening| !self.openings.iter().any(|s| s.reference.id == opening);
        if self.fill_opening.is_some_and(gone) {
            self.fill_opening = None;
        }
        if self.tuning_opening.is_some_and(gone) {
            self.tuning_opening = None;
        }
    }
}
