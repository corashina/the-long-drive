//! Source `tavcsoscript` and the `fpscontroller` binocular use (`Bbinoculars`).
use crate::reference::{BinocularsReference, ItemId};
use rapier3d::math::Pose;

/// Source `Sight()` while the player looks through binoculars.
pub const LOOK_SCALE: f32 = 0.1;
/// Port tuning. The source wheel needs 37 notches for the full focus range. This makes it about 6.
pub const WHEEL_GAIN: f32 = 6.;
/// Port tuning. The source zoom spans `minfov` to `maxfov` (3 to 4 degrees). This makes the wide end 12 degrees.
pub const WIDE_FOV_SCALE: f32 = 3.;

struct Bound {
    item: ItemId,
    /// `CamR` relative to the item pose.
    camera: Pose,
    focus: f32,
}

/// The binoculars in use this frame.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BinocularUse {
    pub item: ItemId,
    pub camera: Pose,
    pub fov_degrees: f32,
    /// `tavcso` DepthOfField focus distance.
    pub focus: f32,
}

pub struct Binoculars {
    reference: BinocularsReference,
    bound: Vec<Bound>,
    active: Option<BinocularUse>,
}

impl Binoculars {
    pub fn new(reference: &BinocularsReference) -> Self {
        Self {
            reference: reference.clone(),
            bound: Vec::new(),
            active: None,
        }
    }
    pub fn bind(&mut self, item: ItemId, camera: Pose) {
        if !self.bound.iter().any(|b| b.item == item) {
            self.bound.push(Bound {
                item,
                camera,
                focus: self.reference.focus_distance,
            });
        }
    }
    pub fn unbind(&mut self, item: ItemId) {
        self.bound.retain(|b| b.item != item);
    }
    /// `noLMBUse`. Carried or equipped binoculars own the LMB.
    pub fn reserves_lmb(&self, equipped: Option<ItemId>, carried: Option<ItemId>) -> bool {
        self.owner(equipped, carried).is_some()
    }
    fn owner(&self, equipped: Option<ItemId>, carried: Option<ItemId>) -> Option<usize> {
        [carried, equipped]
            .into_iter()
            .flatten()
            .find_map(|item| self.bound.iter().position(|b| b.item == item))
    }
    /// `usePickedUpStuff` with LMB held, then `tavcsoscript.Update`. `wheel` is the source `fpscontroller.scroll`: the scroll axis times `FMouseSensZ`.
    pub fn step(
        &mut self,
        equipped: Option<ItemId>,
        carried: Option<ItemId>,
        lmb: bool,
        wheel: f32,
        dt: f32,
    ) -> Option<BinocularUse> {
        let r = &self.reference;
        self.active = self
            .owner(equipped, carried)
            .filter(|_| lmb && dt.is_finite() && dt > 0.)
            .map(|index| {
                let b = &mut self.bound[index];
                if wheel.is_finite() {
                    b.focus += wheel * dt * r.focus_sensitivity * WHEEL_GAIN;
                }
                b.focus = b.focus.clamp(r.min_focus, r.max_focus);
                let t = ((b.focus - r.max_focus) / (r.min_focus - r.max_focus)).clamp(0., 1.);
                BinocularUse {
                    item: b.item,
                    camera: b.camera,
                    fov_degrees: r.min_fov + (r.max_fov * WIDE_FOV_SCALE - r.min_fov) * t,
                    focus: b.focus,
                }
            });
        self.active
    }
    pub fn active(&self) -> Option<BinocularUse> {
        self.active
    }
    pub fn heap_bytes(&self) -> usize {
        self.bound.capacity() * std::mem::size_of::<Bound>()
    }
}
