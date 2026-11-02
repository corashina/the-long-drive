//! Explosive props and tanks: raw `explosivescript` and `tankscript` explosion fields, and the
//! removal of exploded items.
use super::WorldStream;
use crate::interaction::items::ItemStore;
use crate::interaction::storage::StorageLocks;
use crate::reference::{ItemId, NodeId, ReferenceError, SourceId, WorldPrototypeReference};
use crate::vehicle::PhysicsWorld;
use crate::world::GeneratedId;
use crate::world::spawn_rules::*;

/// tankscript explosion fields.
#[derive(Clone, Copy, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct TankExplosion {
    pub never_explode: bool,
    pub always_explode: bool,
    pub always_explode_force: f32,
    /// explosionForceNeededToExplode.
    pub force_needed: f32,
    pub explosion_type: i32,
    pub no_shoot_hole: bool,
    pub no_shoot_explode: bool,
    /// parentDest: its node in the owner prototype. None when null or outside it.
    pub parent_dest: Option<NodeId>,
}

impl TankExplosion {
    pub fn read<'a>(
        f: impl FieldAccess<'a>,
        prototype: &WorldPrototypeReference,
    ) -> Result<Self, ReferenceError> {
        Ok(Self {
            never_explode: flag(f, "neverExplode")?,
            always_explode: flag(f, "alwaysExplode")?,
            always_explode_force: number(f, "alwaysExplodeForce")?,
            force_needed: number(f, "explosionForceNeededToExplode")?,
            explosion_type: integer(f, "explosionType")?,
            no_shoot_hole: flag(f, "noShootHole")?,
            no_shoot_explode: flag(f, "noShootExplode")?,
            parent_dest: optional(f, "parentDest")?.and_then(|s| {
                prototype
                    .nodes
                    .iter()
                    .position(|n| &n.source == s)
                    .map(|i| NodeId(i as u32))
            }),
        })
    }
}

/// explosivescript fields.
#[derive(Clone, Debug, PartialEq)]
pub struct Explosive {
    pub force_needed: f32,
    pub explode_force: f32,
    pub explosion_type: i32,
    pub min_ammo: f32,
    /// The item's own ammoscript: its rounds are the item's ammo binding.
    pub ammo: Option<SourceId>,
    pub color: [f32; 4],
    /// ccolor: the colorscript whose color replaces `color`.
    pub ccolor: Option<SourceId>,
}

pub struct ExplosiveBinding {
    pub(super) parent: GeneratedId,
    pub(super) item: ItemId,
    pub(super) explosive: Explosive,
}

impl WorldStream {
    /// Binds the explosivescript of each active generated item once.
    pub(super) fn sync_explosives(&mut self) -> Result<(), ReferenceError> {
        let active = &self.contents.active;
        self.small_items.explosives.retain(|b| {
            active
                .iter()
                .any(|c| c.content.placement.id == b.parent && c.item == Some(b.item))
        });
        for slot in 0..self.contents.active.len() {
            let content = &self.contents.active[slot];
            let Some(item) = content.item else {
                continue;
            };
            let prototype = &content.content.placement.prototype;
            if self.small_items.explosive_prototypes.get(prototype) == Some(&false)
                || self.small_items.explosives.iter().any(|b| b.item == item)
            {
                continue;
            }
            let prototype = prototype.clone();
            let binding = self.bind_explosive(slot, item)?;
            self.small_items
                .explosive_prototypes
                .insert(prototype, binding.is_some());
            self.small_items.explosives.extend(binding);
        }
        Ok(())
    }
    /// The explosivescript of the content in `slot`, if it has one.
    pub(super) fn bind_explosive(
        &self,
        slot: usize,
        item: ItemId,
    ) -> Result<Option<ExplosiveBinding>, ReferenceError> {
        let placement = &self.contents.active[slot].content.placement;
        let rules = &self.contents.rules;
        let Some(e) = rules.component(&self.reference, &placement.prototype, "explosivescript")?
        else {
            return Ok(None);
        };
        let f = e.fields;
        let ammo = optional(f, "ammo")?.cloned();
        if let Some(ammo) = &ammo {
            let own = rules.component(&self.reference, &placement.prototype, "ammoscript")?;
            if own.is_none_or(|a| a.source != ammo) {
                return Err(ReferenceError(
                    "Explosive ammo is not the item's ammoscript".into(),
                ));
            }
        }
        let color = fields(value(f, "color")?)?;
        Ok(Some(ExplosiveBinding {
            parent: placement.id.clone(),
            item,
            explosive: Explosive {
                force_needed: number(f, "forceNeeded")?,
                explode_force: number(f, "explodeForce")?,
                explosion_type: integer(f, "explosionType")?,
                min_ammo: number(f, "minAmmo")?,
                ammo,
                color: [
                    number(color, "r")?,
                    number(color, "g")?,
                    number(color, "b")?,
                    number(color, "a")?,
                ],
                ccolor: optional(f, "ccolor")?.cloned(),
            },
        }))
    }
    /// The explosive prop of an item.
    pub fn explosive_item(&self, item: ItemId) -> Option<&Explosive> {
        self.small_items
            .explosives
            .iter()
            .find(|b| b.item == item)
            .map(|b| &b.explosive)
    }
    /// `Destroy` and `removeFromMemory` of an exploded content item. The caller first releases
    /// it from the player. The removal is saved.
    pub fn remove_exploded_item(
        &mut self,
        item: ItemId,
        items: &mut ItemStore,
        storage: &mut StorageLocks,
        physics: &mut PhysicsWorld,
    ) -> Result<(), ReferenceError> {
        self.small_items.explosives.retain(|b| b.item != item);
        self.small_items.ammo.retain(|b| b.item != item);
        self.remove_item(item, items, storage, physics)
    }
}
