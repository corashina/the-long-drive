//! Source computers (computerscript) and TVs (displayscript) on generated items.
//! Nothing here saves: after a load every computer is off and empty, as in the source.
use super::WorldStream;
use crate::interaction::computer::{Computer, TypingFrame};
use crate::interaction::items::ItemStore;
use crate::interaction::{InteractionState, Knob};
use crate::reference::{ColliderId, ItemId, ReferenceError, ReferenceManifest};
use crate::vehicle::PhysicsWorld;
use crate::world::GeneratedId;

pub struct ComputerBinding {
    parent: GeneratedId,
    pub item: ItemId,
    pub computer: Computer,
    /// computerscript.display.
    pub display: Option<ItemId>,
}
pub struct DisplayBinding {
    parent: GeneratedId,
    /// The active content slot.
    slot: usize,
    /// The screen material changed since the last presentation change.
    dirty: bool,
    pub item: ItemId,
    /// displayscript.ON.
    pub on: bool,
    /// displayscript.computer.
    pub computer: Option<ItemId>,
    /// displayscript.input: the computer whose output it last received.
    input: Option<ItemId>,
    /// The material _EmissionMap: the output of this computer.
    pub map: Option<ItemId>,
    knobs: Vec<ColliderId>,
}
impl DisplayBinding {
    /// displayscript.Connect.
    fn connect(&mut self, computer: ItemId, output: Option<ItemId>) {
        self.computer = Some(computer);
        self.input = output;
        self.map = output;
        self.dirty = true;
    }
    /// displayscript.Disconnect.
    fn disconnect(&mut self) {
        self.computer = None;
        self.map = None;
        self.dirty = true;
    }
}
#[derive(Default)]
pub struct Computers {
    computers: Vec<ComputerBinding>,
    displays: Vec<DisplayBinding>,
    scan: Option<(u64, u64)>,
    /// inputscript.usingComputer.
    pub typing: bool,
    time: f64,
    /// usablescript.Turn clips: clip, volume, absolute position.
    pub sounds: Vec<(u32, f32, [f64; 3])>,
}
impl Computers {
    pub fn heap_bytes(&self) -> usize {
        self.computers.capacity() * std::mem::size_of::<ComputerBinding>()
            + self
                .computers
                .iter()
                .map(|b| crate::world::contents::identity_heap(&b.parent) + b.computer.grid().len())
                .sum::<usize>()
            + self.displays.capacity() * std::mem::size_of::<DisplayBinding>()
            + self
                .displays
                .iter()
                .map(|b| {
                    crate::world::contents::identity_heap(&b.parent)
                        + b.knobs.capacity() * std::mem::size_of::<ColliderId>()
                })
                .sum::<usize>()
            + self.sounds.capacity() * std::mem::size_of::<(u32, f32, [f64; 3])>()
    }
    pub fn computers(&self) -> &[ComputerBinding] {
        &self.computers
    }
    /// Every computer with its screen change since the last call. Presentation draws them.
    pub fn take_screens(&mut self, mut screen: impl FnMut(ItemId, &Computer, bool)) {
        for b in &mut self.computers {
            let changed = b.computer.take_changed();
            screen(b.item, &b.computer, changed);
        }
    }
    pub fn computer(&self, item: ItemId) -> Option<&ComputerBinding> {
        self.computers.iter().find(|b| b.item == item)
    }
    pub fn is_display(&self, item: ItemId) -> bool {
        self.displays.iter().any(|b| b.item == item)
    }
    /// computerscript.Turn. Turned connects the display again with the new output.
    pub fn turn(&mut self, item: ItemId) {
        let Some(b) = self.computers.iter_mut().find(|b| b.item == item) else {
            return;
        };
        b.computer.turn();
        let display = b.display;
        self.connect(item, display);
    }
    /// computerscript.Use: the computer reads the keyboard from now on.
    pub fn use_computer(&mut self, item: ItemId) {
        if let Some(b) = self.computers.iter_mut().find(|b| b.item == item) {
            b.computer.used = true;
            self.typing = true;
        }
    }
    /// computerscript.Connect. `None` only disconnects.
    pub fn connect(&mut self, item: ItemId, display: Option<ItemId>) {
        self.disconnect(item);
        let Some(display) = display else {
            return;
        };
        let Some(b) = self.computers.iter_mut().find(|b| b.item == item) else {
            return;
        };
        let Some(d) = self.displays.iter_mut().find(|d| d.item == display) else {
            return;
        };
        b.display = Some(display);
        d.connect(item, b.computer.on.then_some(item));
    }
    /// computerscript.Disconnect.
    pub fn disconnect(&mut self, item: ItemId) {
        let Some(b) = self.computers.iter_mut().find(|b| b.item == item) else {
            return;
        };
        if let Some(display) = b.display.take()
            && let Some(d) = self.displays.iter_mut().find(|d| d.item == display)
            && d.input == Some(item)
        {
            d.disconnect();
        }
    }
    /// computerscript.Update for every computer. Typing input goes to each used computer.
    pub fn advance(&mut self, dt: f32, typing: Option<TypingFrame>) {
        self.time += f64::from(dt);
        for b in &mut self.computers {
            if let Some(frame) = typing.filter(|_| b.computer.used) {
                b.computer.input(frame, self.time);
            }
            b.computer.update(self.time);
        }
    }
}

impl WorldStream {
    pub(super) fn sync_computers(
        &mut self,
        reference: &ReferenceManifest,
        items: &ItemStore,
        interaction: &mut InteractionState,
        physics: &mut PhysicsWorld,
    ) -> Result<(), ReferenceError> {
        let revision = (self.request.session, self.contents.active_revision());
        if self.computers.scan == Some(revision) {
            return Ok(());
        }
        let active = |parent: &GeneratedId, item: ItemId| {
            self.contents
                .active
                .iter()
                .position(|c| &c.content.placement.id == parent && c.item == Some(item))
        };
        let mut gone = Vec::new();
        self.computers.computers.retain(|b| {
            let keep = active(&b.parent, b.item).is_some();
            if !keep {
                gone.push(b.item);
            }
            keep
        });
        for item in gone.drain(..) {
            // OnDestroy releases the output texture the display may still show.
            for d in &mut self.computers.displays {
                if d.input == Some(item) {
                    d.disconnect();
                    d.input = None;
                }
            }
        }
        let displays = &mut self.computers.displays;
        let mut i = 0;
        while i < displays.len() {
            if let Some(slot) = active(&displays[i].parent, displays[i].item) {
                displays[i].slot = slot;
                i += 1;
                continue;
            }
            for knob in &displays[i].knobs {
                interaction.remove_dial_knob(*knob);
            }
            gone.push(displays.swap_remove(i).item);
        }
        for b in &mut self.computers.computers {
            if b.display.is_some_and(|d| gone.contains(&d)) {
                b.display = None;
            }
        }
        let r = &reference.computer;
        let mut complete = true;
        for slot in 0..self.contents.active.len() {
            let content = &self.contents.active[slot];
            let Some(item) = content.item else {
                continue;
            };
            let prototype = &content.content.placement.prototype;
            let computer = prototype == &r.computer.prototype;
            if (!computer && prototype != &r.display.prototype)
                || self.computers.computer(item).is_some()
                || self.computers.is_display(item)
            {
                continue;
            }
            if !content.restored || items.get(item).and_then(|i| i.body).is_none() {
                complete = false;
                continue;
            }
            let parent = content.content.placement.id.clone();
            if computer {
                self.computers.computers.push(ComputerBinding {
                    parent,
                    item,
                    computer: Computer::new(r.computer.x_size, r.computer.y_size),
                    display: None,
                });
            } else {
                let knobs = self.bind_display_switch(reference, interaction, physics, &parent)?;
                self.presentation_changes
                    .push(super::content_index::ContentSlot::Active(slot));
                self.computers.displays.push(DisplayBinding {
                    parent,
                    slot,
                    dirty: false,
                    item,
                    on: r.display.startup_on,
                    computer: None,
                    input: None,
                    map: None,
                    knobs,
                });
            }
        }
        self.computers.scan = complete.then_some(revision);
        Ok(())
    }
    /// Every collider on the switch usable's own transform turns the display.
    fn bind_display_switch(
        &self,
        reference: &ReferenceManifest,
        interaction: &mut InteractionState,
        physics: &mut PhysicsWorld,
        parent: &GeneratedId,
    ) -> Result<Vec<ColliderId>, ReferenceError> {
        let prototype = self
            .reference
            .prototypes
            .iter()
            .find(|p| p.source == reference.computer.display.prototype)
            .ok_or_else(|| ReferenceError("Display prototype missing".into()))?;
        let switch = &reference.computer.display.switch.node;
        let mut knobs = Vec::new();
        for (source, handle) in self
            .structures
            .iter()
            .filter(|s| &s.placement.id == parent)
            .flat_map(|s| &s.colliders)
        {
            let on_switch = prototype
                .colliders
                .iter()
                .find(|c| &c.source == source)
                .is_some_and(|c| &prototype.nodes[c.node as usize].source == switch);
            if !on_switch {
                continue;
            }
            let mut id = ColliderId((physics.rapier().colliders[*handle].user_data >> 64) as u32);
            if id.0 == 0 {
                id = interaction.allocate_generated_collider()?;
                physics.edit_collider(*handle, |collider| {
                    collider.user_data |= (id.0 as u128) << 64;
                });
            }
            interaction.bind_dial_knob(id, Knob::Switch);
            knobs.push(id);
        }
        if knobs.is_empty() {
            return Err(ReferenceError("Display switch has no collider".into()));
        }
        Ok(knobs)
    }
    /// Queue a presentation change for each display whose screen material changed.
    pub fn publish_display_changes(&mut self) {
        for d in &mut self.computers.displays {
            let slot = super::content_index::ContentSlot::Active(d.slot);
            if std::mem::take(&mut d.dirty) && !self.presentation_changes.contains(&slot) {
                self.presentation_changes.push(slot);
            }
        }
    }
    /// displayscript.R: `on` picks the material, `map` is the computer whose output it shows.
    pub fn display_screen(&self, parent: &GeneratedId) -> Option<(bool, Option<ItemId>)> {
        self.computers
            .displays
            .iter()
            .find(|d| &d.parent == parent)
            .map(|d| (d.on, d.map))
    }
    /// usablescript.Turn on a display switch: displayscript.Turned and the turn clip.
    /// Returns false when the knob is not a display switch.
    pub fn switch_display(
        &mut self,
        knob: ColliderId,
        reference: &ReferenceManifest,
        items: &ItemStore,
    ) -> bool {
        let Some(d) = self
            .computers
            .displays
            .iter_mut()
            .find(|d| d.knobs.contains(&knob))
        else {
            return false;
        };
        d.on = !d.on;
        d.dirty = true;
        let s = &reference.computer.display.switch;
        if let (Some(clip), Some(item)) = (reference.clip_id(&s.clip), items.get(d.item)) {
            let position = self.origin.absolute(item.pose.position);
            self.computers.sounds.push((clip, s.volume, position));
        }
        true
    }
}
