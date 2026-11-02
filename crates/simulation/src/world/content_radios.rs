//! Source portable radios: radioscript controls, needle and screen materials on generated items.
use super::WorldStream;
use super::content_index::ContentSlot;
use crate::electronics::{Electronics, Emp, NodeElectronics};
use crate::interaction::items::ItemStore;
use crate::interaction::{InteractionState, Knob};
use crate::reference::{ColliderId, ItemId, ReferenceError, SourceId, Transform, UiReference};
use crate::simulation::radio::{RadioControl, RadioState};
use crate::vehicle::{PhysicsWorld, to_rapier};
use crate::world::spawn_appearance::NodeAppearance;
use crate::world::spawn_rules::*;
use crate::world::{GeneratedId, ObjectKey};
use rapier3d::glamx::{Affine3A, Quat, Vec3};

pub struct RadioBinding {
    pub(super) parent: GeneratedId,
    slot: usize,
    pub item: ItemId,
    /// The radioscript. It selects the browser voice group.
    script: SourceId,
    knobs: Vec<(ColliderId, Knob)>,
    pub state: RadioState,
    /// The radio electronicsscript.
    pub electronics: NodeElectronics,
    /// TMutato prototype node index and local pose.
    #[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
    pub needle_node: usize,
    pub needle: Transform,
    /// ST1 and ST2 positions in TMutato's parent space.
    start: Vec3,
    end: Vec3,
    /// RS and RMutato nodes with their on and off materials.
    screens: [(SourceId, [SourceId; 2]); 2],
    shown: Option<bool>,
}
impl RadioBinding {
    pub fn heap_bytes(&self) -> usize {
        crate::world::contents::identity_heap(&self.parent)
            + self.script.file.capacity()
            + self.knobs.capacity() * std::mem::size_of::<(ColliderId, Knob)>()
            + self
                .screens
                .iter()
                .map(|(n, m)| n.file.capacity() + m[0].file.capacity() + m[1].file.capacity())
                .sum::<usize>()
    }
    fn place_needle(&mut self) -> bool {
        let translation = self
            .start
            .lerp(self.end, self.state.tuning.clamp(0., 1.))
            .to_array();
        let changed = translation != self.needle.translation;
        self.needle.translation = translation;
        changed
    }
}

/// A prototype node's pose in prototype space.
fn prototype_affine(
    prototype: &crate::reference::WorldPrototypeReference,
    node: usize,
) -> Affine3A {
    let mut affine = Affine3A::IDENTITY;
    let mut current = Some(node);
    while let Some(i) = current {
        let t = &prototype.nodes[i].transform;
        affine = Affine3A::from_scale_rotation_translation(
            Vec3::from_array(t.scale),
            Quat::from_array(t.rotation),
            Vec3::from_array(t.translation),
        ) * affine;
        current = prototype.nodes[i].parent.map(|p| p as usize);
    }
    affine
}

/// The radio controls in the change record of world object `parent`: a portable radio or a car.
fn saved_controls(
    changes: &crate::world::changes::ChangeRecords,
    parent: &GeneratedId,
) -> Option<crate::save::RadioSnapshot> {
    changes
        .get(ObjectKey::Generated(parent.clone()))
        .and_then(|c| c.snapshot.as_ref())
        .and_then(|s| s.radio)
}

impl WorldStream {
    /// The radioscript of a prototype, cached because it sits below the item root.
    fn radio_script(&mut self, prototype: &SourceId) -> Result<Option<SourceId>, ReferenceError> {
        if let Some((_, script)) = self.radio_scripts.iter().find(|(p, _)| p == prototype) {
            return Ok(script.clone());
        }
        let mut scripts = Vec::new();
        self.contents.rules.children(
            &self.reference,
            prototype,
            "radioscript",
            true,
            &mut scripts,
        )?;
        let script = match scripts.as_slice() {
            [] => None,
            [script] => Some((*script).clone()),
            _ => return Err(ReferenceError("Item has more than one radio".into())),
        };
        self.radio_scripts.push((prototype.clone(), script.clone()));
        Ok(script)
    }
    pub(super) fn sync_radio_bindings(
        &mut self,
        ui: &UiReference,
        items: &ItemStore,
        interaction: &mut InteractionState,
        physics: &mut PhysicsWorld,
    ) -> Result<(), ReferenceError> {
        let revision = (self.request.session, self.contents.active_revision());
        if self.radio_scan == Some(revision) {
            return Ok(());
        }
        let mut complete = true;
        let mut i = 0;
        while i < self.radio_bindings.len() {
            let b = &mut self.radio_bindings[i];
            if let Some(slot) = self
                .contents
                .active
                .iter()
                .position(|c| c.content.placement.id == b.parent && c.item == Some(b.item))
            {
                b.slot = slot;
                i += 1;
            } else {
                for (knob, _) in &b.knobs {
                    interaction.remove_dial_knob(*knob);
                }
                self.radio_bindings.swap_remove(i);
            }
        }
        for slot in 0..self.contents.active.len() {
            let content = &self.contents.active[slot];
            let Some(item) = content.item else {
                continue;
            };
            if self.radio_bindings.iter().any(|b| b.item == item) {
                continue;
            }
            let prototype = content.content.placement.prototype.clone();
            let restored = content.restored;
            let Some(script) = self.radio_script(&prototype)? else {
                continue;
            };
            if !restored || items.get(item).and_then(|i| i.body).is_none() {
                complete = false;
                continue;
            }
            let mut binding =
                self.bind_radio(ui, items, interaction, physics, slot, item, &script)?;
            binding.place_needle();
            self.radio_bindings.push(binding);
            self.show_radio_power(self.radio_bindings.len() - 1);
        }
        self.radio_scan = complete.then_some(revision);
        Ok(())
    }
    fn bind_radio(
        &self,
        ui: &UiReference,
        items: &ItemStore,
        interaction: &mut InteractionState,
        physics: &mut PhysicsWorld,
        slot: usize,
        item: ItemId,
        script: &SourceId,
    ) -> Result<RadioBinding, ReferenceError> {
        let content = &self.contents.active[slot];
        let parent = content.content.placement.id.clone();
        let rules = &self.contents.rules;
        let reference = &self.reference;
        let prototype = reference
            .prototypes
            .iter()
            .find(|p| p.source == content.content.placement.prototype)
            .ok_or_else(|| ReferenceError("Radio prototype missing".into()))?;
        let index = |link: &SourceId| -> Result<usize, ReferenceError> {
            prototype
                .nodes
                .iter()
                .position(|n| &n.source == link)
                .ok_or_else(|| ReferenceError("Radio node outside its prototype".into()))
        };
        let f = rules.get(reference, script)?.fields;
        let electronics = link(f, "electronics")?;
        let node = index(rules.root(reference, electronics)?)?;
        let electronics = NodeElectronics {
            node: node as u32,
            layer: u32::from(prototype.nodes[node].layer),
            state: Electronics::from_fields(rules.get(reference, electronics)?.fields)?,
        };
        let needle_node = index(link(f, "TMutato")?)?;
        let needle_parent = prototype.nodes[needle_node]
            .parent
            .map_or(Affine3A::IDENTITY, |p| {
                prototype_affine(prototype, p as usize)
            })
            .inverse();
        let local = |name: &str| -> Result<Vec3, ReferenceError> {
            let world = prototype_affine(prototype, index(link(f, name)?)?);
            Ok(needle_parent.transform_point3(world.translation.into()))
        };
        let screen = |renderer: &str, on: &str, off: &str| -> Result<_, ReferenceError> {
            Ok((
                rules.root(reference, link(f, renderer)?)?.clone(),
                [link(f, on)?.clone(), link(f, off)?.clone()],
            ))
        };
        let band = optional(f, "BChangeAMFM")?;
        let mut state = RadioState::new(
            [
                number(f, "AMmin")?,
                number(f, "AMmax")?,
                number(f, "AMStep")?,
            ],
            [
                number(f, "FMmin")?,
                number(f, "FMmax")?,
                number(f, "FMStep")?,
            ],
            number(f, "vSpeed")?,
            band.is_some(),
            crate::save::RadioSnapshot {
                on: flag(f, "ON")?,
                am: flag(f, "amfm")?,
                tuning: number(f, "currentTeker")?,
                volume: number(f, "v")?,
            },
        );
        if let Some(saved) = saved_controls(&self.changes, &parent) {
            state
                .restore_save(&saved)
                .map_err(|_| ReferenceError("Saved radio band without band control".into()))?;
        }
        // Each usable owns the colliders on its own transform.
        let body = items.get(item).and_then(|i| i.body);
        let mut knobs = Vec::new();
        for (field, kind) in [
            (Some(link(f, "Bvolume")?), Knob::Volume),
            (Some(link(f, "Btune")?), Knob::Tuning),
            (band, Knob::Switch),
        ] {
            let Some(usable) = field else {
                continue;
            };
            let u = rules.get(reference, usable)?.fields;
            if !flag(u, "m_Enabled")? || flag(u, "disable")? {
                continue;
            }
            let node = rules.root(reference, usable)?;
            let icon = tune_icon(u, ui)?;
            let colliders = self
                .structures
                .iter()
                .filter(|s| s.placement.id == parent)
                .flat_map(|s| &s.colliders);
            for (source, handle) in colliders {
                let on_usable = prototype
                    .colliders
                    .iter()
                    .find(|c| &c.source == source)
                    .is_some_and(|c| &prototype.nodes[c.node as usize].source == node);
                if !on_usable || physics.rapier().colliders[*handle].parent() != body {
                    continue;
                }
                let mut id =
                    ColliderId((physics.rapier().colliders[*handle].user_data >> 64) as u32);
                if id.0 == 0 {
                    id = interaction.allocate_generated_collider()?;
                    physics.edit_collider(*handle, |collider| {
                        collider.user_data |= (id.0 as u128) << 64;
                    });
                }
                interaction.bind_dial_knob(id, kind);
                if let Some(icon) = icon {
                    interaction.bind_tune_icon(id, icon);
                }
                knobs.push((id, kind));
            }
        }
        Ok(RadioBinding {
            parent,
            slot,
            item,
            script: script.clone(),
            knobs,
            state,
            electronics,
            needle_node,
            needle: prototype.nodes[needle_node].transform,
            start: local("ST1")?,
            end: local("ST2")?,
            screens: [
                screen("RS", "SOn", "SOff")?,
                screen("RMutato", "MutatoOn", "MutatoOff")?,
            ],
            shown: None,
        })
    }
    /// radioscript RS and RMutato materials follow the powered state.
    fn show_radio_power(&mut self, index: usize) {
        let b = &mut self.radio_bindings[index];
        let power = b.state.on && b.electronics.state.output() > self.reference.electronics_cutoff;
        if b.shown == Some(power) {
            return;
        }
        b.shown = Some(power);
        let appearance = &mut self.contents.active[b.slot].content.appearance;
        for (node, [on, off]) in &b.screens {
            let material = if power { on } else { off };
            if let Some(a) = appearance
                .iter_mut()
                .rev()
                .find(|a| &a.node == node && a.material.is_some())
            {
                a.material = Some(material.clone());
            } else {
                appearance.push(NodeAppearance {
                    node: node.clone(),
                    primary: None,
                    scale: None,
                    active: None,
                    material: Some(material.clone()),
                    color: None,
                    color_material: None,
                    rotation: None,
                });
            }
        }
        let slot = ContentSlot::Active(b.slot);
        if !self.presentation_changes.contains(&slot) {
            self.presentation_changes.push(slot);
        }
    }
    /// One radioscript.Update control. Returns the item for TurnRadio's click when the
    /// power or band changed, or None when the knob is not a radio knob.
    pub fn radio_control(
        &mut self,
        knob: ColliderId,
        direction: f32,
        items: &mut ItemStore,
    ) -> Option<(ItemId, bool)> {
        let index = self
            .radio_bindings
            .iter()
            .position(|b| b.knobs.iter().any(|(k, _)| *k == knob))?;
        let b = &mut self.radio_bindings[index];
        let kind = b.knobs.iter().find(|(k, _)| *k == knob)?.1;
        let control = match (kind, direction > 0.) {
            (Knob::Volume, true) => RadioControl::VolumeUp,
            (Knob::Volume, false) => RadioControl::VolumeDown,
            (Knob::Tuning, true) => RadioControl::TuneUp,
            (Knob::Tuning, false) => RadioControl::TuneDown,
            (Knob::Switch, _) => RadioControl::Band,
            (Knob::Dial | Knob::Crank, _) => return None,
        };
        let (on, am) = (b.state.on, b.state.am);
        b.state.control(control);
        let click = on != b.state.on || am != b.state.am;
        let item = b.item;
        if b.place_needle() {
            let slot = ContentSlot::Active(b.slot);
            if !self.presentation_changes.contains(&slot) {
                self.presentation_changes.push(slot);
            }
        }
        self.show_radio_power(index);
        items.mark_dirty(item);
        Some((item, click))
    }
    /// The controls of radio `item`.
    pub fn radio_state(&self, item: ItemId) -> Option<&RadioState> {
        self.radio_bindings
            .iter()
            .find(|b| b.item == item)
            .map(|b| &b.state)
    }
    /// Source `snItemsync.UpdRadio`: the controls of radio `item` from a peer.
    pub fn set_radio(
        &mut self,
        item: ItemId,
        saved: &crate::save::RadioSnapshot,
        items: &mut ItemStore,
    ) {
        let Some(index) = self.radio_bindings.iter().position(|b| b.item == item) else {
            return;
        };
        let b = &mut self.radio_bindings[index];
        if !saved.tuning.is_finite()
            || !saved.volume.is_finite()
            || b.state.restore_save(saved).is_err()
        {
            return;
        }
        if b.place_needle() {
            let slot = ContentSlot::Active(b.slot);
            if !self.presentation_changes.contains(&slot) {
                self.presentation_changes.push(slot);
            }
        }
        self.show_radio_power(index);
        items.mark_dirty(item);
    }
    /// The radio of a knob collider.
    pub fn knob_radio(&self, knob: ColliderId) -> Option<&RadioState> {
        self.radio_bindings
            .iter()
            .find(|b| b.knobs.iter().any(|(k, _)| *k == knob))
            .map(|b| &b.state)
    }
    /// The controls saved with a radio item.
    pub(super) fn radio_value(&self, item: ItemId) -> Option<crate::save::RadioSnapshot> {
        self.radio_bindings
            .iter()
            .find(|b| b.item == item)
            .map(|b| b.state.capture_save())
    }
    /// Bound world radios (portable and car radios) with their radioscript, controls,
    /// electronics output and parent placement. A car radio has the car as parent.
    pub fn portable_radios(
        &self,
    ) -> impl Iterator<Item = (ItemId, &SourceId, &RadioState, f32, &GeneratedId)> + Clone {
        self.radio_bindings.iter().map(|b| {
            let output = b.electronics.state.output();
            (b.item, &b.script, &b.state, output, &b.parent)
        })
    }
    /// EMPs jam each radio at its item position. The screens follow the power.
    pub fn expose_radios(&mut self, emps: &[Emp], items: &ItemStore, dt: f32) {
        for index in 0..self.radio_bindings.len() {
            let b = &mut self.radio_bindings[index];
            let Some(item) = items.get(b.item) else {
                continue;
            };
            let e = &mut b.electronics;
            e.state
                .expose(emps, to_rapier(item.pose).translation, e.layer, dt);
            self.show_radio_power(index);
        }
    }
    pub fn radio(&self, parent: &GeneratedId) -> Option<&RadioBinding> {
        self.radio_bindings.iter().find(|b| &b.parent == parent)
    }
}

#[cfg(test)]
impl WorldStream {
    /// Test probe: the first knob of `kind` on the portable radio `item`.
    pub fn radio_knob(&self, item: ItemId, kind: Knob) -> Option<ColliderId> {
        let b = self.radio_bindings.iter().find(|b| b.item == item)?;
        b.knobs.iter().find(|(_, k)| *k == kind).map(|(c, _)| *c)
    }
}
