//! Source Tab timing and right-hand release gates.
use crate::{
    reference::ItemId,
    simulation::{FrameInput, keys},
};
#[derive(Clone, Copy)]
pub struct EquipmentContext {
    pub carried_can_equip: bool,
    pub carried: Option<ItemId>,
    pub has_equipment: bool,
    pub seated: bool,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EquipAction {
    Equip,
    Recenter,
}
pub struct EquipmentInput {
    hold_seconds: f32,
    elapsed: f32,
    pressed_item: Option<ItemId>,
    suppressed: bool,
    can_equip: bool,
    can_recenter: bool,
    equipped_now: bool,
}
impl EquipmentInput {
    pub fn new(hold_seconds: f32) -> Self {
        Self {
            hold_seconds,
            elapsed: 0.,
            pressed_item: None,
            suppressed: false,
            can_equip: true,
            can_recenter: false,
            equipped_now: false,
        }
    }
    pub fn begin_frame(
        &mut self,
        input: FrameInput,
        dt: f32,
        context: EquipmentContext,
    ) -> Option<EquipAction> {
        self.equipped_now = false;
        if input.pressed & keys::RECENTER != 0 {
            self.elapsed = 0.;
            self.pressed_item = context.carried;
            self.suppressed = context.carried.is_none();
            self.can_recenter = false;
        } else {
            self.elapsed += dt;
            if self.pressed_item.is_some() && self.pressed_item != context.carried {
                self.pressed_item = None;
                self.suppressed = true;
            }
        }
        if self.suppressed {
            if input.held & keys::RECENTER == 0 && input.pressed & keys::RECENTER == 0 {
                self.suppressed = false;
            }
            return None;
        }
        if self.elapsed > self.hold_seconds {
            if input.held & keys::RECENTER != 0 {
                self.can_recenter = true;
            } else {
                self.can_equip = true;
            }
        }
        let short_release = input.released & keys::RECENTER != 0
            && self.pressed_item.is_some()
            && self.elapsed < self.hold_seconds;
        if input.released & keys::RECENTER != 0 {
            self.pressed_item = None;
        }
        if context.carried.is_none() {
            return None;
        }
        if context.carried_can_equip && !context.has_equipment && self.can_equip && short_release {
            Some(EquipAction::Equip)
        } else if input.held & keys::RECENTER != 0
            && (self.can_recenter || !context.carried_can_equip)
        {
            Some(EquipAction::Recenter)
        } else {
            None
        }
    }
    pub fn record_equipped(&mut self) {
        self.equipped_now = true;
    }
    pub fn record_released(&mut self) {
        self.suppressed = false;
        self.can_equip = false;
        self.can_recenter = false;
        self.pressed_item = None;
    }
    pub fn should_release(&self, input: FrameInput, context: EquipmentContext) -> bool {
        !self.equipped_now
            && context.has_equipment
            && context.carried.is_none()
            && (input.held & keys::ROTATE != 0
                || (!context.seated && input.pressed & keys::RECENTER != 0))
    }
    pub fn finish_frame(&mut self, input: FrameInput) {
        if input.released & keys::RECENTER != 0 {
            self.can_equip = true;
            self.can_recenter = true;
        }
    }
    pub fn cancel_input(&mut self) {
        self.suppressed = false;
        self.elapsed = 0.;
        self.pressed_item = None;
        self.can_equip = true;
        self.can_recenter = false;
        self.equipped_now = false;
    }
}
