//! Slot membership is independent of carry and right-hand ownership.
use crate::reference::{InventoryReference, ItemId};
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InventoryCommand {
    Select(u8),
    Swap(u8),
}
#[derive(Clone, Copy, Default)]
pub struct InventoryContext {
    pub carried: Option<ItemId>,
    pub equipped: Option<ItemId>,
    pub ancestor_carry: bool,
    pub seated_clutch: bool,
}
pub struct InventoryState {
    pub slots: [Option<ItemId>; 4],
    pub selected: u8,
    pub swap_progress: f32,
    gain: f32,
    swapped: bool,
}
impl InventoryState {
    pub fn new(r: &InventoryReference) -> Self {
        Self {
            slots: [None; 4],
            selected: r.selected_slot,
            swap_progress: 0.,
            gain: r.swap_gain,
            swapped: false,
        }
    }
    pub fn cancel_input(&mut self) {
        self.swap_progress = 0.;
        self.swapped = false;
    }
    pub fn forget(&mut self, id: ItemId) {
        for slot in &mut self.slots {
            if *slot == Some(id) {
                *slot = None;
            }
        }
    }
    pub fn released_command(
        &self,
        released: u8,
        context: InventoryContext,
    ) -> Option<InventoryCommand> {
        if context.seated_clutch || self.swapped {
            return None;
        }
        (0..4)
            .find(|i| released & (1 << i) != 0)
            .map(InventoryCommand::Select)
    }
    pub fn held_command(
        &mut self,
        held: u8,
        dt: f32,
        context: InventoryContext,
    ) -> Option<InventoryCommand> {
        if context.seated_clutch || !dt.is_finite() || dt <= 0. {
            return None;
        }
        let Some(i) = (0..4).find(|i| held & (1 << i) != 0) else {
            self.swapped = false;
            return None;
        };
        if self.swapped
            || context.ancestor_carry
            || (context.carried.is_some()
                && context.equipped.is_some()
                && self.slots[i as usize].is_some())
        {
            return None;
        }
        if self.swap_progress < 1. {
            self.swap_progress += self.gain * dt;
            None
        } else {
            Some(InventoryCommand::Swap(i))
        }
    }
    pub fn record_swap(&mut self) {
        self.swapped = true;
        self.swap_progress = 0.;
    }
}
