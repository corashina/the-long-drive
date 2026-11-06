//! Seats and items that other players use. Net fills it each frame. The player cannot take them.
//! The source lets a player take an item from another player's hands. The port does not.
use super::InteractionState;
use crate::reference::ItemId;
use crate::vehicle::PhysicsWorld;
use crate::world::stream::content_seats::ItemSeat;
use std::sync::Arc;

#[derive(Default)]
pub struct InUse {
    /// Items that another player holds or sits on, with the slot of that player.
    pub items: Vec<(ItemId, u8)>,
    /// Car seats with another player in them, with the slot of that player.
    pub seats: Vec<(Arc<ItemSeat>, u8)>,
}

impl InUse {
    pub fn clear(&mut self) {
        self.items.clear();
        self.seats.clear();
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
