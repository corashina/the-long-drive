//! Source compasses and clocks: meterscript needles and digit wheels on generated items.
use super::WorldStream;
use super::content_index::ContentSlot;
use crate::interaction::InteractionState;
use crate::interaction::items::ItemStore;
use crate::reference::{ColliderId, ItemId, ReferenceError, SourceId, Transform, UiReference};
use crate::simulation::dials::*;
use crate::vehicle::PhysicsWorld;
use crate::world::spawn_rules::*;
use crate::world::{GeneratedId, ObjectKey};
use rapier3d::math::{Rotation, Vector};

/// A source transform whose local rotation is rewritten as Euler angles.
pub struct DialNode {
    /// Prototype node index.
    #[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
    pub node: usize,
    pub pose: Transform,
    axis: usize,
    inv: bool,
    angle: f32,
}
impl DialNode {
    fn set(&mut self, euler: [f32; 3]) -> bool {
        let rotation = unity_euler(euler);
        let changed = rotation != self.pose.rotation;
        self.pose.rotation = rotation;
        changed
    }
}
/// forgoszamlap digit.
struct Digit {
    node: DialNode,
    continuous: bool,
    direct: bool,
    place: f32,
}
/// mutatoscript with lerpAngle.
struct Needle {
    node: DialNode,
    rest: [f32; 3],
    value: f32,
    min_angle: f32,
    max_angle: f32,
    min_value: f32,
    max_value: f32,
    offset: f32,
    speed: f32,
}
pub struct DialBinding {
    pub(super) parent: GeneratedId,
    slot: usize,
    item: ItemId,
    knobs: Vec<ColliderId>,
    affect: f32,
    /// meterscript szamlap value: compass bearing or clock offset.
    value: f32,
    base: f32,
    multiplier: Option<f32>,
    digits: Vec<Digit>,
    values: Vec<f32>,
    /// clockscript hour, minute and second digit indices.
    clock: Option<[Option<usize>; 3]>,
    needle: Option<Needle>,
}
impl DialBinding {
    pub fn heap_bytes(&self) -> usize {
        crate::world::contents::identity_heap(&self.parent)
            + self.digits.capacity() * std::mem::size_of::<Digit>()
            + self.values.capacity() * std::mem::size_of::<f32>()
            + self.knobs.capacity() * std::mem::size_of::<ColliderId>()
    }
    pub fn nodes(&self) -> impl Iterator<Item = &DialNode> {
        self.digits
            .iter()
            .map(|d| &d.node)
            .chain(self.needle.as_ref().map(|n| &n.node))
    }
}

impl WorldStream {
    pub(super) fn sync_dial_bindings(
        &mut self,
        ui: &UiReference,
        items: &ItemStore,
        interaction: &mut InteractionState,
        physics: &mut PhysicsWorld,
    ) -> Result<(), ReferenceError> {
        let revision = (self.request.session, self.contents.active_revision());
        if self.dial_scan == Some(revision) {
            return Ok(());
        }
        let mut complete = true;
        let mut i = 0;
        while i < self.dial_bindings.len() {
            let b = &mut self.dial_bindings[i];
            if let Some(slot) = self
                .contents
                .active
                .iter()
                .position(|c| c.content.placement.id == b.parent && c.item == Some(b.item))
            {
                b.slot = slot;
                i += 1;
            } else {
                for knob in &b.knobs {
                    interaction.remove_dial_knob(*knob);
                }
                self.dial_bindings.swap_remove(i);
            }
        }
        for slot in 0..self.contents.active.len() {
            let content = &self.contents.active[slot];
            let Some(item) = content.item else {
                continue;
            };
            if self.dial_bindings.iter().any(|b| b.item == item) {
                continue;
            }
            let root = &content.content.placement.prototype;
            let rules = &self.contents.rules;
            let script = |class| rules.component(&self.reference, root, class).ok().flatten();
            let script = match script("compassscript").or_else(|| script("clockscript")) {
                Some(script) => script.source.clone(),
                // A car dashboard clock (Car07) is on an active child node of the car root item.
                None if self
                    .reference
                    .prototypes
                    .iter()
                    .any(|p| &p.source == root && p.vehicle.is_some()) =>
                {
                    let mut clocks = Vec::new();
                    rules.children(&self.reference, root, "clockscript", false, &mut clocks)?;
                    match clocks[..] {
                        [] => continue,
                        [clock] => clock.clone(),
                        _ => return Err(ReferenceError("Car has more than one clock".into())),
                    }
                }
                None => continue,
            };
            if !content.restored || items.get(item).and_then(|i| i.body).is_none() {
                complete = false;
                continue;
            }
            let binding = self.bind_dial(ui, items, interaction, physics, slot, item, &script)?;
            self.dial_bindings.push(binding);
        }
        self.dial_scan = complete.then_some(revision);
        Ok(())
    }
    fn bind_dial(
        &self,
        ui: &UiReference,
        items: &ItemStore,
        interaction: &mut InteractionState,
        physics: &mut PhysicsWorld,
        slot: usize,
        item: ItemId,
        script: &SourceId,
    ) -> Result<DialBinding, ReferenceError> {
        let content = &self.contents.active[slot];
        let parent = content.content.placement.id.clone();
        let rules = &self.contents.rules;
        let reference = &self.reference;
        let prototype = reference
            .prototypes
            .iter()
            .find(|p| p.source == content.content.placement.prototype)
            .ok_or_else(|| ReferenceError("Dial prototype missing".into()))?;
        let node = |link: &SourceId, axis: usize, inv: bool| -> Result<DialNode, ReferenceError> {
            Ok(DialNode {
                node: prototype
                    .nodes
                    .iter()
                    .position(|n| &n.source == link)
                    .ok_or_else(|| ReferenceError("Dial node outside its prototype".into()))?,
                pose: rules.node(reference, link)?.pose,
                axis,
                inv,
                angle: 0.,
            })
        };
        let axis = |f: Fields, names: [&str; 3]| -> Result<usize, ReferenceError> {
            // Source checks X, then Y, then Z; none falls back to X.
            for (i, n) in names.iter().enumerate() {
                if flag(f, n)? {
                    return Ok(i);
                }
            }
            Ok(0)
        };
        let s = rules.get(reference, script)?;
        let meter = rules.get(reference, link(s.fields, "meter")?)?.fields;
        let mut needle = None;
        if s.class_name == "compassscript" {
            for m in array(meter, "mutatok")? {
                let m = fields(m)?;
                if text(m, "tipus")? != "compassnorth" {
                    continue;
                }
                let n = rules.get(reference, link(m, "mutato")?)?.fields;
                if flag(n, "noValueLerp")? || !flag(n, "lerpAngle")? {
                    return Err(ReferenceError("Unsupported compass needle".into()));
                }
                let multiplier = number(n, "lerpSpeedMultiplier")?;
                needle = Some(Needle {
                    node: node(
                        link(n, "mutato")?,
                        axis(n, ["X", "Y", "Z"])?,
                        flag(n, "inv")?,
                    )?,
                    rest: [number(n, "x")?, number(n, "y")?, number(n, "z")?],
                    value: 0.,
                    min_angle: number(n, "minAngle")?,
                    max_angle: number(n, "maxAngle")?,
                    min_value: number(n, "minValue")?,
                    max_value: number(n, "maxValue")?,
                    offset: number(n, "inputValueOffset")?,
                    speed: 8. * if multiplier == 0. { 1. } else { multiplier },
                });
            }
        }
        // A clock reads only its "time" face. A car dashboard meter also has the odometer face.
        let mut faces = Vec::new();
        for f in array(meter, "szamlapok")? {
            let f = fields(f)?;
            if s.class_name != "clockscript" || text(f, "tipus")? == "time" {
                faces.push(f);
            }
        }
        let [face] = faces[..] else {
            return Err(ReferenceError("Dial needs one source face".into()));
        };
        let [usable] = array(face, "usables")? else {
            return Err(ReferenceError("Dial needs one source knob".into()));
        };
        let usable = fields(usable)?;
        if !flag(usable, "useTuneUp")?
            || !flag(usable, "useTuneDown")?
            || flag(usable, "usefix")?
            || flag(usable, "useTunedFloat")?
            || flag(usable, "useUpperLimit")?
            || flag(usable, "useLowerLimit")?
        {
            return Err(ReferenceError("Unsupported dial knob".into()));
        }
        let forgo = rules.get(reference, link(face, "forgo")?)?.fields;
        let base = integer(forgo, "szamrendszer")? as f32;
        let decimals = number(forgo, "tizedesjegyek")?;
        let mut digits = Vec::new();
        for d in array(forgo, "szamok")? {
            let d = fields(d)?;
            digits.push(Digit {
                node: node(link(d, "T")?, axis(d, ["x", "y", "z"])?, flag(d, "inv")?)?,
                continuous: flag(d, "continous")?,
                direct: flag(d, "direct")?,
                place: base.powf(integer(d, "helyiertekHatvany")? as f32 - decimals + 1.),
            });
        }
        let clock = if s.class_name == "clockscript" {
            let hand = |on: &str, index: &str| -> Result<Option<usize>, ReferenceError> {
                if !flag(s.fields, on)? {
                    return Ok(None);
                }
                let i = integer(s.fields, index)? as usize;
                (i < digits.len())
                    .then_some(Some(i))
                    .ok_or_else(|| ReferenceError("Clock hand outside its dial".into()))
            };
            Some([
                hand("hour", "hourindex")?,
                hand("minute", "minuteindex")?,
                hand("second", "secondindex")?,
            ])
        } else {
            None
        };
        // The knob is every collider on the usable's own transform.
        let knob_node = rules.root(reference, link(usable, "usable")?)?;
        let icon = tune_icon(rules.get(reference, link(usable, "usable")?)?.fields, ui)?;
        let body = items.get(item).and_then(|i| i.body);
        let mut knobs = Vec::new();
        let colliders = self
            .structures
            .iter()
            .filter(|s| s.placement.id == parent)
            .flat_map(|s| &s.colliders);
        for (source, handle) in colliders {
            let on_knob = prototype
                .colliders
                .iter()
                .find(|c| &c.source == source)
                .is_some_and(|c| &prototype.nodes[c.node as usize].source == knob_node);
            if !on_knob || physics.rapier().colliders[*handle].parent() != body {
                continue;
            }
            let mut id = ColliderId((physics.rapier().colliders[*handle].user_data >> 64) as u32);
            if id.0 == 0 {
                id = interaction.allocate_generated_collider()?;
                physics.edit_collider(*handle, |collider| {
                    collider.user_data |= (id.0 as u128) << 64;
                });
            }
            interaction.bind_dial_knob(id, crate::interaction::Knob::Dial);
            if let Some(icon) = icon {
                interaction.bind_tune_icon(id, icon);
            }
            knobs.push(id);
        }
        let value = self
            .changes
            .get(ObjectKey::Generated(parent.clone()))
            .and_then(|c| c.snapshot.as_ref())
            .and_then(|s| s.dial)
            .unwrap_or(0.);
        Ok(DialBinding {
            parent,
            slot,
            item,
            knobs,
            affect: number(usable, "affect")?,
            value,
            base,
            multiplier: flag(forgo, "useMultiplier")?
                .then(|| number(forgo, "multiplier"))
                .transpose()?,
            values: vec![0.; digits.len()],
            digits,
            clock,
            needle,
        })
    }
    /// One meterscript tune step. Returns false when the knob is not a dial.
    pub fn tune_dial(&mut self, knob: ColliderId, direction: f32, items: &mut ItemStore) -> bool {
        let Some(b) = self
            .dial_bindings
            .iter_mut()
            .find(|b| b.knobs.contains(&knob))
        else {
            return false;
        };
        b.value += b.affect * direction.signum();
        items.mark_dirty(b.item);
        true
    }
    /// The tuned value saved with a dial item; None at the source default.
    pub(super) fn dial_value(&self, item: ItemId) -> Option<Option<f32>> {
        self.dial_bindings
            .iter()
            .find(|b| b.item == item)
            .map(|b| (b.value != 0.).then_some(b.value))
    }
    /// compassscript, clockscript, meterscript, mutatoscript and forgoszamlap updates.
    pub fn advance_dials(&mut self, dt: f32, cycle_fraction: f32, items: &ItemStore) {
        if !dt.is_finite() || dt < 0. {
            return;
        }
        let north = Rotation::from_array(self.reference.magnetic_field) * Vector::NEG_Z;
        for b in &mut self.dial_bindings {
            let mut changed = false;
            if let Some(n) = &mut b.needle
                && let Some(item) = items.get(b.item)
            {
                let forward = Rotation::from_array(item.pose.rotation) * Vector::NEG_Z;
                n.value = lerp_angle(n.value, compass_north(forward, north), dt * n.speed);
                if n.value > n.max_angle {
                    n.value -= n.max_angle - n.min_angle;
                }
                if n.value < n.min_angle {
                    n.value += n.max_angle - n.min_angle;
                }
                let mut t = inverse_lerp(n.min_value, n.max_value, n.value + n.offset);
                if n.node.inv {
                    t = 1. - t;
                }
                let mut euler = n.rest;
                euler[n.node.axis] = lerp(n.min_angle, n.max_angle, t);
                changed |= n.node.set(euler);
            }
            if let Some([hour, minute, second]) = b.clock {
                let t = clock_degrees(b.value, cycle_fraction);
                for (index, scale) in [(second, 1440.), (minute, 24.), (hour, 2.)] {
                    if let Some(i) = index {
                        b.values[i] = t * scale;
                    }
                }
            }
            let value = b.value * b.multiplier.unwrap_or(1.);
            for (i, d) in b.digits.iter().enumerate() {
                if !d.direct {
                    b.values[i] = if d.place == 0. {
                        0.
                    } else {
                        value % d.place / d.place * b.base
                    };
                }
            }
            for i in 0..b.digits.len() {
                let target = digit_angle(&b.values, i, b.digits[i].continuous, b.base);
                let n = &mut b.digits[i].node;
                n.angle = lerp_angle(n.angle, target, dt * 20.);
                let mut euler = [0.; 3];
                euler[n.axis] = if n.inv { -n.angle } else { n.angle };
                changed |= n.set(euler);
            }
            let slot = ContentSlot::Active(b.slot);
            if changed && !self.presentation_changes.contains(&slot) {
                self.presentation_changes.push(slot);
            }
        }
    }
    pub fn dial(&self, parent: &GeneratedId) -> Option<&DialBinding> {
        self.dial_bindings.iter().find(|b| &b.parent == parent)
    }
}
