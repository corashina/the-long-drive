use super::InteractionState;
use crate::reference::{ActionBinding, ColliderId, FoodReference, ItemId, ItemKind};
use rapier3d::prelude::{ColliderHandle, RigidBodyHandle};

impl InteractionState {
    pub fn register_consumable(
        &mut self,
        id: ItemId,
        reference: &FoodReference,
        colliders: &[ColliderHandle],
    ) {
        self.kinds.insert(id, ItemKind::Food);
        self.carry.register_food(id, reference);
        self.pickup_layers.insert(
            id,
            reference
                .colliders
                .iter()
                .fold(0, |mask, collider| mask | collider.membership),
        );
        for &handle in colliders {
            self.bind_surface(
                handle,
                ActionBinding {
                    collider: ColliderId(0),
                    use_owner: None,
                    pickup_owner: Some(id),
                    removal_owner: None,
                    push_owner: None,
                },
            );
        }
    }

    /// Forget the bindings of a removed item: an eaten food, an exploded tank or its cap.
    pub fn forget_removed(&mut self, id: ItemId, body: RigidBodyHandle) {
        self.collider_bindings.retain(|_, binding| {
            binding.use_owner != Some(id)
                && binding.pickup_owner != Some(id)
                && binding.removal_owner != Some(id)
                && binding.push_owner != Some(id)
        });
        self.source_bindings.retain(|_, binding| {
            binding.use_owner != Some(id)
                && binding.pickup_owner != Some(id)
                && binding.removal_owner != Some(id)
                && binding.push_owner != Some(id)
        });
        self.kinds.remove(&id);
        self.pickup_layers.remove(&id);
        self.removal_layers.remove(&id);
        self.usable_items.remove(&id);
        self.filling_items.remove(&id);
        self.tank_rules.remove(&id);
        self.forget_openings(id);
        self.carry.forget_food(id);
        self.bodies.retain(|_, handle| *handle != body);
        self.mechanism_bodies.remove(&id);
    }
}

impl InteractionState {
    /// Bind a spawned prototype body as an ordinary pickup.
    pub fn register_spawned(
        &mut self,
        id: ItemId,
        rule: crate::reference::PickupReference,
        colliders: &[(ColliderHandle, ColliderId, u32)],
    ) {
        let layers = colliders.iter().fold(0, |mask, c| mask | c.2);
        self.register_generated_pickup(id, rule, layers, false);
        for &(handle, collider, _) in colliders {
            self.bind_surface(
                handle,
                ActionBinding {
                    collider,
                    use_owner: None,
                    pickup_owner: Some(id),
                    removal_owner: None,
                    push_owner: None,
                },
            );
        }
    }

    pub fn remove_spawned(&mut self, id: ItemId, body: RigidBodyHandle) {
        self.collider_bindings
            .retain(|_, binding| binding.pickup_owner != Some(id));
        self.kinds.remove(&id);
        self.pickup_layers.remove(&id);
        self.carry.unregister_generated(id);
        self.unregister_hose(id);
        self.bodies.retain(|_, handle| *handle != body);
    }
}
