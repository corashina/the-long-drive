//! Loose ammo and magazines: the `ammoscript` of generated items that a gun loads from.
use super::WorldStream;
use crate::interaction::guns::{AmmoItem, GunSnapshot};
use crate::interaction::items::{ItemLocation, ItemStore};
use crate::interaction::spray::Ammo;
use crate::reference::{ItemId, ReferenceError, ReferenceManifest, SourceId};
use crate::vehicle::PhysicsWorld;
use crate::world::GeneratedId;
use crate::world::spawn_rules::*;

pub struct AmmoBinding {
    pub(super) parent: GeneratedId,
    pub(super) item: ItemId,
    pub(super) ammo: AmmoItem,
    pub(super) initial: f32,
}

impl WorldStream {
    /// True when the prototype has an ammoscript of a gun ammo type or of its explosive prop,
    /// and no gun owns it.
    pub(super) fn ammo_prototype(
        &self,
        reference: &ReferenceManifest,
        prototype: &SourceId,
    ) -> Result<bool, ReferenceError> {
        if reference
            .guns
            .weapons
            .iter()
            .any(|w| w.prototype.0 == *prototype)
        {
            return Ok(false);
        }
        let Some(a) = self
            .contents
            .rules
            .component(&self.reference, prototype, "ammoscript")?
        else {
            return Ok(false);
        };
        let kind = number(a.fields, "tipus")? as u32;
        // The rounds of an explosive prop.
        let explosive = self
            .contents
            .rules
            .component(&self.reference, prototype, "explosivescript")?
            .map(|e| optional(e.fields, "ammo"))
            .transpose()?
            .flatten();
        Ok(optional(a.fields, "w")?.is_none()
            && (explosive == Some(a.source)
                || reference.guns.weapons.iter().any(|w| w.ammo.r#type == kind)))
    }
    /// The ammoscript of the ammo content in `slot`.
    pub(super) fn bind_ammo(
        &self,
        slot: usize,
        item: ItemId,
    ) -> Result<AmmoBinding, ReferenceError> {
        let content = &self.contents.active[slot].content;
        let placement = &content.placement;
        let f = self
            .contents
            .rules
            .component(&self.reference, &placement.prototype, "ammoscript")?
            .ok_or_else(|| ReferenceError("Ammo item has no ammoscript".into()))?
            .fields;
        let kind = number(f, "tipus")? as u32;
        let float = flag(f, "floatOrInt")?;
        let prefab = Ammo {
            count: if float {
                number(f, "fammo")?
            } else {
                number(f, "ammo")?
            },
            maximum: number(f, "max")?,
            float,
            kind,
        };
        let initial = content.ammo.unwrap_or(prefab.count);
        let ammo = prefab
            .with_count(initial)
            .ok_or_else(|| ReferenceError("Invalid spawned ammo count".into()))?;
        let saved = self
            .saved_snapshot(&placement.id)
            .and_then(|s| s.ammo)
            .and_then(|count| ammo.with_count(count));
        Ok(AmmoBinding {
            parent: placement.id.clone(),
            item,
            ammo: AmmoItem {
                ammo: saved.unwrap_or(ammo),
                is_mag: flag(f, "isMag")?,
            },
            initial,
        })
    }
    fn saved_snapshot(&self, parent: &GeneratedId) -> Option<&crate::world::ObjectSnapshot> {
        self.changes
            .get(crate::world::ObjectKey::Generated(parent.clone()))
            .and_then(|c| c.snapshot.as_ref())
    }
    /// The loose ammo or magazine of an item.
    pub fn ammo_item(&self, item: ItemId) -> Option<AmmoItem> {
        self.small_items
            .ammo
            .iter()
            .find(|b| b.item == item)
            .map(|b| b.ammo)
    }
    pub fn set_ammo_item(&mut self, item: ItemId, ammo: AmmoItem, items: &mut ItemStore) {
        if let Some(b) = self.small_items.ammo.iter_mut().find(|b| b.item == item) {
            b.ammo = ammo;
            items.mark_dirty(item);
        }
    }
    /// `Destroy` of a magazine that went into a gun. The caller first releases it from the
    /// player. The removal is saved.
    pub fn remove_ammo_item(
        &mut self,
        item: ItemId,
        items: &mut ItemStore,
        physics: &mut PhysicsWorld,
    ) {
        let Some(body) = items.get(item).and_then(|s| s.body) else {
            return;
        };
        self.small_items.ammo.retain(|b| b.item != item);
        self.changes.remove(items.object_key(item));
        physics.edit_body(body, |body| body.set_enabled(false));
        physics.refresh_queries();
        let state = items.get_mut(item).expect("ammo item");
        state.location = ItemLocation::World;
        state.removed = true;
        items.mark_dirty(item);
    }
    /// The saved gun state of a generated or imported item.
    pub fn saved_gun(&self, items: &ItemStore, item: ItemId) -> Option<GunSnapshot> {
        self.changes
            .get(items.object_key(item))
            .and_then(|c| c.snapshot.as_ref())
            .and_then(|s| s.gun)
    }
    /// The `RandomSpawned` ammo count of a generated item.
    pub fn content_ammo(&self, item: ItemId) -> Option<f32> {
        self.contents
            .active
            .iter()
            .find(|c| c.item == Some(item))
            .and_then(|c| c.content.ammo)
    }
}
