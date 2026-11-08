//! Seats and items that other players use. Net fills it each frame. The player cannot take them,
//! but the player can take an item from another player's hands, as in the source.
use super::InteractionState;
use crate::reference::ItemId;
use crate::vehicle::PhysicsWorld;
use crate::world::stream::content_seats::ItemSeat;
use rapier3d::prelude::RigidBodyType;
use std::sync::Arc;

#[derive(Default)]
pub struct InUse {
    /// Items that another player holds or sits on, with the slot of that player.
    pub items: Vec<(ItemId, u8)>,
    /// Car seats with another player in them, with the slot of that player.
    pub seats: Vec<(Arc<ItemSeat>, u8)>,
    /// Items that another player carries or holds, with the body type to restore on a take.
    pub hands: Vec<(ItemId, RigidBodyType)>,
}

impl InUse {
    pub fn clear(&mut self) {
        self.items.clear();
        self.seats.clear();
        self.hands.clear();
    }
    /// The body type of `item` if another player carries or holds it.
    pub fn hand(&self, item: ItemId) -> Option<RigidBodyType> {
        self.hands.iter().find(|(i, _)| *i == item).map(|(_, t)| *t)
    }
    /// The slot of the player that uses `item`.
    pub fn item(&self, item: ItemId) -> Option<u8> {
        self.items.iter().find(|(i, _)| *i == item).map(|(_, s)| *s)
    }
    /// The slot of the player in `seat`. An item seat is in use when its item is.
    pub fn seat(&self, seat: &ItemSeat) -> Option<u8> {
        self.seats
            .iter()
            .find(|(s, _)| std::ptr::eq(Arc::as_ptr(s), seat))
            .map(|(_, slot)| *slot)
            .or_else(|| self.item(seat.item))
    }
}

impl InteractionState {
    /// Source `BcantSitAngle`, and no other player sits there.
    pub fn can_sit(&self, seat: &ItemSeat, physics: &PhysicsWorld) -> bool {
        self.in_use.seat(seat).is_none() && seat.can_sit(physics)
    }
}
