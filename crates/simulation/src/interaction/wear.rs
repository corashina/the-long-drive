//! Source `fpscontroller.weared`: one worn item per `wearableType`.

/// Source `wearableType`: glasses, hat, cap, helmet, mask.
pub const WEAR_SLOTS: usize = 5;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WornItems<T> {
    pub slots: [Option<T>; WEAR_SLOTS],
}

impl<T> Default for WornItems<T> {
    fn default() -> Self {
        Self {
            slots: [const { None }; WEAR_SLOTS],
        }
    }
}

impl<T: Copy + PartialEq> WornItems<T> {
    /// `Wear` runs only when the slot of the item type is empty.
    pub fn wear_if_empty(&mut self, slot: usize, item: T) -> bool {
        if self.slots[slot].is_some() {
            return false;
        }
        self.slots[slot] = Some(item);
        true
    }
    pub fn slot_of(&self, item: T) -> Option<usize> {
        self.slots.iter().position(|s| *s == Some(item))
    }
    /// `removeSunglasses`: the first consumable comes off first, else the first worn slot.
    pub fn take_off_next(&mut self, consumable: impl Fn(T) -> bool) -> Option<T> {
        let slot = self
            .slots
            .iter()
            .position(|s| s.is_some_and(&consumable))
            .or_else(|| self.slots.iter().position(Option::is_some))?;
        self.slots[slot].take()
    }
    /// Death2: every worn item comes off.
    pub fn take_all(&mut self) -> [Option<T>; WEAR_SLOTS] {
        std::mem::take(self).slots
    }
}
