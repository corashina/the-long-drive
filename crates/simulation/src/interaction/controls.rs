//! Usable world controls by collider: generated knobs, item seats and UFO controls.
//! A collider has at most one control.
use super::{InteractionState, Knob};
use crate::ground_ufo::UfoControlTarget;
use crate::reference::{ColliderId, ItemId, ReferenceError};
use crate::vehicle::PhysicsWorld;
use crate::world::stream::content_seats::ItemSeat;
use rapier3d::prelude::{Collider, ColliderHandle};
use std::collections::HashMap;
use std::sync::Arc;

pub enum UseControl {
    /// A generated usablescript knob. `tune_icon` is a `UiReference::sprites` index.
    Knob {
        id: ColliderId,
        kind: Knob,
        tune_icon: Option<u32>,
    },
    Seat(Arc<ItemSeat>),
    Ufo(UfoControlTarget),
}

#[derive(Default)]
pub struct UseControls {
    controls: HashMap<ColliderHandle, UseControl>,
}

impl UseControls {
    pub fn bind(&mut self, collider: ColliderHandle, control: UseControl) {
        self.controls.insert(collider, control);
    }
    pub fn remove(&mut self, collider: ColliderHandle) {
        self.controls.remove(&collider);
    }
    /// Removes the knob with this collider identity.
    pub fn remove_knob(&mut self, knob: ColliderId) {
        self.controls
            .retain(|_, c| !matches!(c, UseControl::Knob { id, .. } if *id == knob));
    }
    pub fn knob(&self, collider: ColliderHandle) -> Option<(ColliderId, Knob, Option<u32>)> {
        match self.controls.get(&collider)? {
            UseControl::Knob {
                id,
                kind,
                tune_icon,
            } => Some((*id, *kind, *tune_icon)),
            _ => None,
        }
    }
    pub fn seat(&self, collider: ColliderHandle) -> Option<&Arc<ItemSeat>> {
        match self.controls.get(&collider)? {
            UseControl::Seat(seat) => Some(seat),
            _ => None,
        }
    }
    pub fn ufo(&self, collider: ColliderHandle) -> Option<UfoControlTarget> {
        match self.controls.get(&collider)? {
            UseControl::Ufo(target) => Some(*target),
            _ => None,
        }
    }
    pub fn seats(&self) -> impl Iterator<Item = &Arc<ItemSeat>> {
        self.controls.values().filter_map(|c| match c {
            UseControl::Seat(seat) => Some(seat),
            _ => None,
        })
    }
    /// The item that a use on the control acts on. A knob has no use owner: its body item owns it.
    pub fn owner(&self, handle: ColliderHandle, collider: &Collider) -> Option<ItemId> {
        match self.controls.get(&handle)? {
            UseControl::Seat(seat) => Some(seat.item),
            UseControl::Ufo(target) => Some(target.item),
            UseControl::Knob { .. } => (collider.user_data as u32).checked_sub(1).map(ItemId),
        }
    }
    /// The bound item seat of a world item by its main seatscript. Car seats are not here.
    pub fn find_seat(
        &self,
        owner: &crate::player::SeatOwner,
        source: &crate::reference::SourceId,
    ) -> Option<Arc<ItemSeat>> {
        self.seats()
            .find(|seat| &seat.owner == owner && &seat.seat.source == source)
            .cloned()
    }
    pub fn heap_bytes(&self) -> usize {
        super::memory::map(&self.controls)
    }
}

impl InteractionState {
    /// Binds a generated knob on `collider`. The collider gets a generated identity when it has
    /// none. Returns the identity.
    pub fn bind_knob(
        &mut self,
        physics: &mut PhysicsWorld,
        collider: ColliderHandle,
        kind: Knob,
        tune_icon: Option<u32>,
    ) -> Result<ColliderId, ReferenceError> {
        let mut id = ColliderId((physics.rapier().colliders[collider].user_data >> 64) as u32);
        if id.0 == 0 {
            id = self.allocate_generated_collider()?;
            physics.edit_collider(collider, |c| c.user_data |= (id.0 as u128) << 64);
        }
        self.controls.bind(
            collider,
            UseControl::Knob {
                id,
                kind,
                tune_icon,
            },
        );
        Ok(id)
    }
}
