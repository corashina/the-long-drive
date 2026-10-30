//! Source `helptextscript` boxes on world cars. fpscontroller shows the help set of the box that the use ray hits.
use crate::reference::{ItemId, WorldHelpTextReference};
use rapier3d::prelude::ColliderHandle;
use std::collections::HashMap;

/// Help bits sent to the browser. 0: the use ray hits no help box.
pub const SHOWN: u8 = 1;
pub const FORCE_AUTO: u8 = 2;
pub const MOTORCYCLE: u8 = 4;
pub const BIKE: u8 = 8;
pub const ALWAYS: u8 = 16;

pub fn bits(r: &WorldHelpTextReference) -> u8 {
    SHOWN
        | if r.force_auto { FORCE_AUTO } else { 0 }
        | if r.motorcycle { MOTORCYCLE } else { 0 }
        | if r.bike { BIKE } else { 0 }
        | if r.always { ALWAYS } else { 0 }
}

/// Help bits of each help box collider, with the item that owns the box.
#[derive(Default)]
pub struct HelpTexts {
    boxes: HashMap<ColliderHandle, (ItemId, u8)>,
}

impl HelpTexts {
    pub fn add(&mut self, handle: ColliderHandle, owner: ItemId, bits: u8) {
        self.boxes.insert(handle, (owner, bits));
    }
    pub fn remove_owner(&mut self, owner: ItemId) {
        self.boxes.retain(|_, (o, _)| *o != owner);
    }
    pub fn get(&self, handle: ColliderHandle) -> u8 {
        self.boxes.get(&handle).map_or(0, |(_, bits)| *bits)
    }
}
