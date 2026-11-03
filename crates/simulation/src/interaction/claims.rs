//! fpscontroller `noLMBUse`: the held tool that owns the LMB.
//! The render frame, the fixed step, the HUD and the player step ask here.
use super::InteractionState;
use crate::simulation::{FrameInput, keys};

/// The held tool that owns the LMB.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LmbOwner {
    /// A gun in the hand fires on LMB.
    Gun,
    /// An equipped cleaning tool washes or scrubs on LMB.
    CleaningTool,
    /// Carried or equipped binoculars look on LMB.
    Binoculars,
}

impl LmbOwner {
    /// The owner reads LMB in the render frame, so the frame removes it from the fixed input.
    /// Binoculars read LMB in the fixed step, so the fixed step removes it after them.
    pub fn frame_owned(self) -> bool {
        !matches!(self, Self::Binoculars)
    }
}

impl InteractionState {
    /// The held tool that owns the LMB. With None, LMB uses the aimed item.
    pub fn lmb_owner(&self) -> Option<LmbOwner> {
        let (equipped, carried) = (self.equipment.item(), self.held());
        if self.hand_gun() {
            Some(LmbOwner::Gun)
        } else if self.cleaning.reserves_lmb(equipped) {
            Some(LmbOwner::CleaningTool)
        } else if self.binoculars.reserves_lmb(equipped, carried) {
            Some(LmbOwner::Binoculars)
        } else {
            None
        }
    }
    /// fpscontroller `scrubbing` from the hand: `drotkefe.Use` with LMB held.
    pub fn scrubbing(&self, input: &FrameInput) -> bool {
        input.held & keys::PRIMARY != 0
            && self
                .equipment
                .item()
                .is_some_and(|id| self.cleaning.is_brush(id))
    }
    /// pP.weapon in the hand: an equipped gun with a weapon record.
    fn hand_gun(&self) -> bool {
        self.equipment.item().is_some_and(|item| {
            self.guns.runtime(item).is_some()
                && self
                    .carry
                    .rule(item)
                    .and_then(|rule| self.guns.weapon_for(&rule.source))
                    .is_some()
        })
    }
}
