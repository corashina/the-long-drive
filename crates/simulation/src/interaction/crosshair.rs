//! Source `mainscript.CrossHairStuff` (new 2D crosshair). Each layer is one source image.
//! All layers sit at the screen center. Several layers can show at once.
use super::SlotTarget;
use super::feedback::{ActionAvailability, ActionMask, FeedbackContext, ItemPrompt};

/// The adjust icon of a looked usable: `rotateAble` or a sideways or vertical `slideAble`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[repr(u32)]
pub enum AdjustIcon {
    #[default]
    None = 0,
    Rotate = 1,
    Slide = 2,
    SlideVertical = 3,
}

/// Layer bits for the shell. E, adjust and F hold a sprite choice in two or three bits.
pub struct Crosshair;
impl Crosshair {
    /// `GCE`: 1 sit, 2 get out, 3 switch.
    pub const E_SHIFT: u32 = 0;
    /// `GCAdjust`: an `AdjustIcon`.
    pub const ADJUST_SHIFT: u32 = 2;
    /// `GCF`: 1 push, 2 mount, 3 hose, 4 attach.
    pub const F_SHIFT: u32 = 4;
    /// `GCMMB` with the cap image.
    pub const MMB: u32 = 1 << 7;
    /// `GCLMB` with the fill image.
    pub const LMB: u32 = 1 << 8;
    pub const DISMOUNT: u32 = 1 << 9;
    pub const CAN_PICKUP: u32 = 1 << 10;
    /// `GCPickup`. Its two children fade in with the carried weight.
    pub const PICKUP: u32 = 1 << 11;
    pub const TOO_HEAVY: u32 = 1 << 12;
    pub const DEFAULT: u32 = 1 << 13;
    pub const WEAPON: u32 = 1 << 14;
    /// The GCMMB sprite as a `UiReference::sprites` index + 1. 0 shows `ICap`.
    pub const MMB_SPRITE_SHIFT: u32 = 16;
}

/// `weight_m2` is the source `weightM2`: 1 when nothing is carried.
pub fn crosshair(a: &ActionAvailability, carrying: bool, weapon: bool, weight_m2: f32) -> u32 {
    let has = |bits: u32| a.actions.0 & bits != 0;
    let usable = matches!(
        a.context,
        FeedbackContext::Mechanism | FeedbackContext::RadioVolume | FeedbackContext::RadioTuning
    );
    let mut e = 0;
    let mut adjust = AdjustIcon::None;
    let mut mmb = false;
    let mut lmb = false;
    if !weapon {
        if has(ActionMask::ENTER) {
            e = 1;
        }
        if has(ActionMask::EXIT) {
            e = 2;
        }
        if usable {
            adjust = a.adjust;
            e = if has(ActionMask::SWITCH | ActionMask::PRIMARY_SWITCH) {
                3
            } else {
                0
            };
        }
        // A looked usable with tuneAble, or a looked cap with canTune.
        mmb = has(ActionMask::TUNE);
        lmb = has(ActionMask::FILL);
        if a.context == FeedbackContext::LightSwitch && has(ActionMask::USE) {
            e = 3;
        }
    }
    let dismount = has(ActionMask::DISMOUNT) && !carrying && adjust == AdjustIcon::None;
    let mut f = 0;
    if !weapon {
        if (has(ActionMask::PUSH | ActionMask::GRIP) || (has(ActionMask::MOUNT) && carrying))
            && adjust == AdjustIcon::None
            && !dismount
        {
            f = if has(ActionMask::MOUNT) { 2 } else { 1 };
        }
        if has(ActionMask::MOUNT) && a.prompts & ItemPrompt::ATTACH != 0 {
            f = 3;
        }
        if has(ActionMask::MOUNT) && matches!(a.slot, Some(SlotTarget::Hitch(_))) {
            f = 4;
        }
    }
    if f != 0 {
        e = 0;
        adjust = AdjustIcon::None;
    }
    let too_heavy = weight_m2 < 0.5;
    let default = !too_heavy && e == 0 && f == 0 && !lmb && !mmb && !weapon;
    let weapon = weapon && e == 0 && adjust == AdjustIcon::None && !too_heavy && f == 0;
    let pickup = carrying && adjust == AdjustIcon::None && (default || weapon) && !too_heavy;
    let can_pickup =
        adjust == AdjustIcon::None && has(ActionMask::PICKUP) && !has(ActionMask::MOUNT);
    let mut bits = (e << Crosshair::E_SHIFT)
        | ((adjust as u32) << Crosshair::ADJUST_SHIFT)
        | (f << Crosshair::F_SHIFT);
    for (on, bit) in [
        (mmb, Crosshair::MMB),
        (lmb, Crosshair::LMB),
        (dismount, Crosshair::DISMOUNT),
        (can_pickup, Crosshair::CAN_PICKUP),
        (pickup, Crosshair::PICKUP),
        (too_heavy, Crosshair::TOO_HEAVY),
        (default, Crosshair::DEFAULT),
        (weapon, Crosshair::WEAPON),
    ] {
        if on {
            bits |= bit;
        }
    }
    if mmb && let Some(sprite) = a.tune_icon {
        bits |= (sprite + 1) << Crosshair::MMB_SPRITE_SHIFT;
    }
    bits
}

#[cfg(test)]
mod tests {
    use super::*;

    fn availability(actions: u32, context: FeedbackContext) -> ActionAvailability {
        ActionAvailability {
            actions: ActionMask(actions),
            context,
            ..Default::default()
        }
    }

    #[test]
    fn layers_follow_the_source_rules() {
        // A car door: the adjust icon over the default crosshair.
        let mut door = availability(ActionMask::DOOR, FeedbackContext::Mechanism);
        door.adjust = AdjustIcon::Rotate;
        assert_eq!(
            crosshair(&door, false, false, 1.),
            (1 << Crosshair::ADJUST_SHIFT) | Crosshair::DEFAULT
        );
        // A looked usable hides the seat exit icon. A switch shows the E switch icon.
        let switch = availability(
            ActionMask::SWITCH | ActionMask::EXIT,
            FeedbackContext::Mechanism,
        );
        assert_eq!(crosshair(&switch, false, false, 1.), 3);
        // A tuneable cap with a fill shows the wheel and LMB layers.
        let mut cap = availability(ActionMask::TUNE | ActionMask::FILL, FeedbackContext::Cap);
        assert_eq!(
            crosshair(&cap, true, false, 1.),
            Crosshair::MMB | Crosshair::LMB
        );
        // A cap img replaces the ICap sprite.
        cap.tune_icon = Some(4);
        assert_eq!(
            crosshair(&cap, true, false, 1.),
            Crosshair::MMB | Crosshair::LMB | (5 << Crosshair::MMB_SPRITE_SHIFT)
        );
        // Push hides E. Hose attach and hitch attach replace the mount icon.
        let push = availability(ActionMask::PUSH | ActionMask::ENTER, FeedbackContext::Push);
        assert_eq!(crosshair(&push, false, false, 1.), 1 << Crosshair::F_SHIFT);
        let mut hose = availability(ActionMask::MOUNT, FeedbackContext::None);
        hose.prompts = ItemPrompt::ATTACH;
        assert_eq!(crosshair(&hose, true, false, 1.), 3 << Crosshair::F_SHIFT);
        // A carried item shows pickup over the default crosshair until it is too heavy.
        let carry = availability(ActionMask::DROP, FeedbackContext::None);
        assert_eq!(
            crosshair(&carry, true, false, 0.8),
            Crosshair::DEFAULT | Crosshair::PICKUP
        );
        assert_eq!(crosshair(&carry, true, false, 0.4), Crosshair::TOO_HEAVY);
        // An equipped gun replaces the default crosshair. Pick up still shows.
        let item = availability(ActionMask::PICKUP, FeedbackContext::None);
        assert_eq!(
            crosshair(&item, false, true, 1.),
            Crosshair::WEAPON | Crosshair::CAN_PICKUP
        );
    }
}
