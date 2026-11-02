//! Presentation views of the world structures. A view goes out when its state changes.
use super::{GarageRuntime, VehicleRuntime};
use crate::boundary::{Change, MaterialRow, NodeColorRow, NodePoseRow, SimFrame, StructureRow};
use crate::interaction::items::{ItemLocation, ItemState, ItemStore};
use crate::reference::{
    ItemId, ReferenceError, ReferenceManifest, SourceId, Transform, WorldPrototypeReference,
};
use crate::world::stream::content_index::{ContentSlot, InstanceSelection};
use crate::world::{ContentIndex, GeneratedId, NodeAppearance, ObjectKey};
use bevy_platform::collections::HashMap;
use rapier3d::math::Rotation;
use rapier3d::prelude::RigidBodyHandle;
use std::collections::BTreeMap;

/// What a slot was last computed from.
struct Entry {
    id: GeneratedId,
    /// Index in `WorldFrame::structures`.
    structure: usize,
    prototype: usize,
    content: Option<ContentSlot>,
    lamps: Vec<usize>,
    /// The generated item of the structure.
    food: Option<ItemId>,
    /// `VehicleRuntime::presentation_signature` or `CarLights::signature` of the last view.
    signature: u64,
    caps_sent: bool,
    seen: u64,
}

/// Slots of the published structures and the inputs that select a new view.
#[derive(Default)]
pub struct StructureViews {
    slots: HashMap<GeneratedId, u32>,
    entries: Vec<Option<Entry>>,
    free: Vec<u32>,
    stamp: u64,
    membership: Option<((u64, u64, u64, u64, u64), u64)>,
    origin: Option<[f64; 3]>,
    contents: ContentIndex,
    bodies: HashMap<RigidBodyHandle, Vec<u32>>,
    content_slots: HashMap<ContentSlot, Vec<u32>>,
    lamp_slots: HashMap<usize, Vec<u32>>,
    selection: InstanceSelection,
    sources: HashMap<SourceId, u32>,
    /// Local wing pose of each bat prototype node. Reused each frame.
    wing_poses: Vec<Transform>,
    /// Hidden gun parts of the written slot. Reused each slot.
    gun_hidden: Vec<u32>,
    /// `GarageRuntime::other_view` of the written worn flags.
    other_view: bool,
    /// The used binoculars of the written camera hidden flags.
    binoculars: Option<ItemId>,
}

impl GarageRuntime {
    /// The next frame names every pose, structure, condition, screen, plate and followed car again.
    /// A new frame stream calls it: the old stream consumed the sent state.
    pub fn reset_frame_outputs(&mut self) {
        self.structure_views = StructureViews::default();
        self.frame_poses = Default::default();
        self.conditions_sent = false;
        self.home_cable_session = None;
        self.screens_sent = None;
        self.plates_sent.clear();
        self.last_feedback = None;
    }
    /// Writes the views of changed structures. Clears the stream and body presentation queues.
    pub(super) fn fill_structures(&mut self, reference: &ReferenceManifest, frame: &mut SimFrame) {
        frame.structures.clear();
        frame.structure_poses.clear();
        frame.hidden_nodes.clear();
        frame.node_materials.clear();
        frame.node_colors.clear();
        let views = &mut self.structure_views;
        views.select(
            reference,
            &self.stream,
            &self.physics,
            &self.area.items,
            &self.vehicles,
            frame,
        );
        // Radiators with a turned fan.
        for radiator in self.powertrain.radiators_mut() {
            if radiator.presented_angle != radiator.fan_angle
                && let Some(&slot) = views.slots.get(&radiator.parent)
            {
                radiator.presented_angle = radiator.fan_angle;
                views.selection.push(slot as usize);
            }
        }
        // Guns with moved parts.
        for item in self.area.interaction.guns.take_presented() {
            if let ObjectKey::Generated(id) = self.area.items.object_key(item)
                && let Some(&slot) = views.slots.get(&id)
            {
                views.selection.push(slot as usize);
            }
        }
        if views.other_view != self.other_view {
            views.other_view = self.other_view;
            // Rewrite the worn flags of worn items.
            for (slot, entry) in views.entries.iter().enumerate() {
                if entry.as_ref().and_then(|e| e.food).is_some_and(|id| {
                    self.area
                        .items
                        .get(id)
                        .is_some_and(|item| matches!(item.location, ItemLocation::Worn(_)))
                }) {
                    views.selection.push(slot);
                }
            }
        }
        let binoculars = self.area.interaction.binoculars.active().map(|b| b.item);
        if views.binoculars != binoculars {
            for item in [views.binoculars, binoculars].into_iter().flatten() {
                if let ObjectKey::Generated(id) = self.area.items.object_key(item)
                    && let Some(&slot) = views.slots.get(&id)
                {
                    views.selection.push(slot as usize);
                }
            }
            views.binoculars = binoculars;
        }
        let inputs = Inputs {
            reference,
            stream: &self.stream,
            physics: &self.physics,
            items: &self.area.items,
            guns: &self.area.interaction.guns,
            vehicles: &self.vehicles,
            powertrain: &self.powertrain,
            flyers: &self.flyers,
            seconds: self.simulation_seconds,
            other_view: self.other_view,
            binoculars,
        };
        let result = views.write(&inputs, frame);
        self.physics.clear_presentation_bodies();
        self.stream.clear_presentation_changes();
        if let Err(error) = result {
            self.stream.fail(error);
        }
    }
}

struct Inputs<'a> {
    reference: &'a ReferenceManifest,
    stream: &'a crate::world::WorldStream,
    physics: &'a crate::vehicle::PhysicsWorld,
    items: &'a ItemStore,
    guns: &'a crate::interaction::guns::GunState,
    vehicles: &'a BTreeMap<ObjectKey, VehicleRuntime>,
    powertrain: &'a crate::simulation::powertrain::PowertrainParts,
    flyers: &'a crate::flyers::Flyers,
    seconds: f64,
    other_view: bool,
    binoculars: Option<ItemId>,
}

impl StructureViews {
    fn select(
        &mut self,
        reference: &ReferenceManifest,
        stream: &crate::world::WorldStream,
        physics: &crate::vehicle::PhysicsWorld,
        items: &ItemStore,
        vehicles: &BTreeMap<ObjectKey, VehicleRuntime>,
        frame: &mut SimFrame,
    ) {
        let membership = (
            stream.presentation_membership(),
            items.membership_revision(),
        );
        if self.membership != Some(membership) {
            let session_changed = self
                .membership
                .is_none_or(|previous| previous.0.0 != membership.0.0);
            self.update_slots(reference, stream, items, session_changed, frame);
            self.membership = Some(membership);
        }
        if self.origin != Some(stream.origin.offset) {
            self.origin = Some(stream.origin.offset);
            for slot in 0..self.entries.len() {
                self.selection.push(slot);
            }
        }
        let owners = |slots: Option<&Vec<u32>>, selection: &mut InstanceSelection| {
            for &slot in slots.into_iter().flatten() {
                selection.push(slot as usize);
            }
        };
        for body in physics.presentation_bodies() {
            owners(self.bodies.get(&body), &mut self.selection);
        }
        for content in stream.presentation_contents() {
            owners(self.content_slots.get(&content), &mut self.selection);
        }
        for lamp in stream.presentation_lamps() {
            owners(self.lamp_slots.get(&lamp), &mut self.selection);
        }
        // Cobweb changes are rare: a cleaned or start-disabled trigger.
        for id in stream.cobweb_changes() {
            if let Some(&slot) = self.slots.get(id) {
                self.selection.push(slot as usize);
            }
        }
        // World car gauges and lamps change on a still car too.
        let signatures = vehicles
            .iter()
            .map(|(key, v)| (physics.chassis_handle(key), v.presentation_signature()))
            .chain(stream.trailers().map(|(body, l)| (body, l.signature())));
        for (body, signature) in signatures {
            for &slot in self.bodies.get(&body).into_iter().flatten() {
                if self.entries[slot as usize]
                    .as_ref()
                    .is_some_and(|e| e.signature != signature)
                {
                    self.selection.push(slot as usize);
                }
            }
        }
    }
    /// Gives each published structure a slot and selects the slots whose bindings changed.
    fn update_slots(
        &mut self,
        reference: &ReferenceManifest,
        stream: &crate::world::WorldStream,
        items: &ItemStore,
        session_changed: bool,
        frame: &mut SimFrame,
    ) {
        if session_changed {
            for (slot, entry) in self.entries.iter_mut().enumerate() {
                if entry.take().is_some() {
                    frame.changes.push(Change::StructureRemove(slot as u32));
                    self.free.push(slot as u32);
                }
            }
            self.slots.clear();
        }
        stream.index_contents(&mut self.contents);
        self.stamp += 1;
        let mut changed = Vec::new();
        for (index, structure) in stream.frame().structures.iter().enumerate() {
            let id = &structure.placement.id;
            let content = self.contents.slot(id);
            let lamps = self.contents.lamp_slots(id);
            let food = items.generated_item_id(id);
            let slot = match self.slots.get(id) {
                Some(&slot) => slot,
                None => {
                    let Some(prototype) = reference
                        .world
                        .prototypes
                        .index(&structure.placement.prototype)
                    else {
                        continue;
                    };
                    let slot = self.free.pop().unwrap_or_else(|| {
                        self.entries.push(None);
                        self.entries.len() as u32 - 1
                    });
                    self.slots.insert(id.clone(), slot);
                    self.entries[slot as usize] = Some(Entry {
                        id: id.clone(),
                        structure: index,
                        prototype,
                        content,
                        lamps: lamps.to_vec(),
                        food,
                        signature: 0,
                        caps_sent: false,
                        seen: 0,
                    });
                    frame.changes.push(Change::StructureAdd {
                        slot,
                        id: id.clone(),
                    });
                    changed.push(slot);
                    slot
                }
            };
            let entry = self.entries[slot as usize].as_mut().expect("live slot");
            // The first copy of an identity owns the slot.
            if entry.seen == self.stamp {
                continue;
            }
            entry.seen = self.stamp;
            if entry.structure != index
                || entry.content != content
                || entry.lamps != lamps
                || entry.food != food
            {
                entry.structure = index;
                entry.content = content;
                entry.lamps.clear();
                entry.lamps.extend_from_slice(lamps);
                entry.food = food;
                changed.push(slot);
            }
        }
        for (slot, entry) in self.entries.iter_mut().enumerate() {
            if entry.as_ref().is_some_and(|e| e.seen != self.stamp) {
                let gone = entry.take().expect("live slot");
                self.slots.remove(&gone.id);
                frame.changes.push(Change::StructureRemove(slot as u32));
                self.free.push(slot as u32);
            }
        }
        for owners in self
            .bodies
            .values_mut()
            .chain(self.content_slots.values_mut())
            .chain(self.lamp_slots.values_mut())
        {
            owners.clear();
        }
        let structures = stream.frame().structures;
        for (slot, entry) in self.entries.iter().enumerate() {
            let Some(entry) = entry else { continue };
            for body in &structures[entry.structure].bodies {
                self.bodies.entry(*body).or_default().push(slot as u32);
            }
            if let Some(content) = entry.content {
                self.content_slots
                    .entry(content)
                    .or_default()
                    .push(slot as u32);
            }
            for lamp in &entry.lamps {
                self.lamp_slots.entry(*lamp).or_default().push(slot as u32);
            }
        }
        self.bodies.retain(|_, owners| !owners.is_empty());
        self.content_slots.retain(|_, owners| !owners.is_empty());
        self.lamp_slots.retain(|_, owners| !owners.is_empty());
        self.selection.reset(self.entries.len());
        for slot in changed {
            self.selection.push(slot as usize);
        }
    }
    fn write(&mut self, inputs: &Inputs, frame: &mut SimFrame) -> Result<(), ReferenceError> {
        let mut result = Ok(());
        for i in 0..self.selection.len() {
            let slot = self.selection.at(i) as u32;
            result = result.and(self.write_slot(inputs, slot, frame));
        }
        self.selection.clear();
        result
    }
    fn write_slot(
        &mut self,
        inputs: &Inputs,
        slot: u32,
        frame: &mut SimFrame,
    ) -> Result<(), ReferenceError> {
        let Inputs {
            reference,
            stream,
            physics,
            items,
            vehicles,
            powertrain,
            ..
        } = *inputs;
        let Some(entry) = self.entries[slot as usize].as_mut() else {
            return Ok(());
        };
        let structure = &stream.frame().structures[entry.structure];
        let id = &structure.placement.id;
        let prototype = &reference.world.prototypes[entry.prototype];
        let content = stream.content_at(entry.content);
        let mut row = StructureRow {
            slot,
            ..Default::default()
        };
        if !content.bindings_ready() {
            frame.structures.push(row);
            return Ok(());
        }
        row.flags |= StructureRow::READY;
        // The car runtime of a car copy.
        let car = prototype.vehicle.as_ref().and_then(|record| {
            let handle = structure.bodies.get(record.body as usize)?;
            Some((record, vehicles.get(physics.vehicle_of(*handle)?)?))
        });
        if let Some((_, vehicle)) = car {
            entry.signature = vehicle.presentation_signature();
        }
        // The lamp state of a trailer copy.
        let trailer = prototype.trailer.as_ref().and_then(|record| {
            let handle = structure.bodies.get(record.body as usize)?;
            Some((record, stream.trailer_lights(*handle)?))
        });
        if let Some((_, lights)) = trailer {
            entry.signature = lights.signature();
        }
        if !entry.caps_sent {
            entry.caps_sent = true;
            if !content.caps().is_empty() {
                frame.changes.push(Change::StructureCaps {
                    slot,
                    caps: content.caps().to_vec(),
                });
            }
        }
        let appearance = stream.indexed_appearance(&content, id);
        let food = entry
            .food
            .and_then(|id| items.get(id))
            .filter(|item| item.food.is_some());
        if let Some(item) = food {
            check_food(reference, prototype, item)?;
        }
        // Source `wearedRenderer.make = !otherView()`: other views show worn items.
        if !inputs.other_view
            && entry
                .food
                .and_then(|id| items.get(id))
                .is_some_and(|item| matches!(item.location, ItemLocation::Worn(_)))
        {
            row.flags |= StructureRow::WORN;
        }
        // Source `makeinvisible` of the used binoculars: the camera does not draw the item.
        if entry.food.is_some() && entry.food == inputs.binoculars {
            row.flags |= StructureRow::CAMERA_HIDDEN;
        }
        let poses = &mut frame.structure_poses;
        let start = poses.len() as u32;
        let mut local = |node: usize, pose: Transform| {
            poses.push(NodePoseRow {
                node: node as u32,
                kind: NodePoseRow::LOCAL,
                translation: pose.translation,
                rotation: pose.rotation,
                scale: pose.scale,
            });
        };
        if let Some(radio) = stream.radio(id) {
            local(radio.needle_node, radio.needle);
        }
        if let Some(music_box) = stream.music_box(id) {
            for n in &music_box.nodes {
                local(n.node, n.pose);
            }
        }
        if let Some(detector) = stream.detector(id) {
            local(detector.needle.node, detector.needle.pose);
        }
        if let Some(doors) = stream.bus_doors(id) {
            for leaf in doors.leaves() {
                local(leaf.node, leaf.pose);
            }
        }
        if let Some(followers) = stream.followers(id) {
            for (node, pose) in followers {
                local(node, pose);
            }
        }
        if let Some(trailer) = stream.trailer(id) {
            for (node, pose) in trailer.graphics() {
                local(node, pose);
            }
        }
        if let Some(dial) = stream.dial(id) {
            for n in dial.nodes() {
                local(n.node, n.pose);
            }
        }
        // Source `carscript.Transmission`: the lever pose in its parent space.
        if let Some((record, vehicle)) = car
            && let (Some(r), Some(lever)) = (&record.lever, &vehicle.lever)
        {
            local(
                r.node as usize,
                Transform {
                    translation: lever.translation.to_array(),
                    rotation: lever.rotation.to_array(),
                    scale: prototype.nodes[r.node as usize].transform.scale,
                },
            );
        }
        // Source `bikepedalscript.UpdPedals`: turned pedals and level feet holders.
        if let Some((record, vehicle)) = car
            && let Some(r) = &record.pedals
        {
            use crate::simulation::pedals::{feet_holder_rotation, turned};
            let angle = vehicle.pedal_angle;
            let rest = |n: u32| prototype.nodes[n as usize].transform.rotation;
            let pedals = r.pedals.iter().map(|&n| (n, turned(rest(n), angle)));
            let inverse = r
                .pedals_inverse
                .iter()
                .map(|&n| (n, turned(rest(n), -angle)));
            let holders = r
                .feet_holders
                .iter()
                .enumerate()
                .map(|(i, &n)| (n, feet_holder_rotation(&prototype.nodes, r, angle, i)));
            for (n, rotation) in pedals.chain(inverse).chain(holders) {
                let rest = prototype.nodes[n as usize].transform;
                local(
                    n as usize,
                    Transform {
                        rotation: rotation.to_array(),
                        ..rest
                    },
                );
            }
        }
        if let Some(trigger) = stream.spray_trigger(id)
            && let Some(n) = prototype
                .nodes
                .iter()
                .position(|n| n.source == reference.small_items.spray_bottle.trigger)
        {
            local(n, trigger);
        }
        if let Some(level) = stream.level(id) {
            for b in level.bubbles() {
                local(b.node, b.pose);
            }
        }
        // weaponscript, revolverScript, bbgunscript and geppityuscript parts.
        let gun_hidden = &mut self.gun_hidden;
        gun_hidden.clear();
        if let Some(item) = items.generated_item_id(id) {
            inputs.guns.parts(
                item,
                |n, pose| local(n as usize, pose),
                |n| gun_hidden.push(n),
            );
        }
        // The bat Animator loops the wing clip from the flyer start.
        if prototype.source == reference.sky.flyer.prefab {
            let wing = &reference.sky.flyer.wing;
            let born = items
                .generated_item_id(id)
                .and_then(|item| inputs.flyers.flyers().iter().find(|f| f.item == item))
                .map_or(0., |f| f.born);
            let wings = &mut self.wing_poses;
            wings.clear();
            wings.extend(prototype.nodes.iter().map(|n| n.transform));
            let time = crate::animation::clip::loop_time(wing, (inputs.seconds - born) as f32);
            crate::animation::clip::sample_clip(wing, time, |node, part| {
                use crate::animation::clip::LocalPart;
                let pose = &mut wings[node as usize];
                match part {
                    LocalPart::Position(p) => pose.translation = p.to_array(),
                    LocalPart::Rotation(r) => pose.rotation = r.to_array(),
                    LocalPart::Scale(v) => pose.scale = v.to_array(),
                }
            });
            for track in &wing.tracks {
                local(track.node as usize, wings[track.node as usize]);
            }
        }
        for (i, node) in prototype.nodes.iter().enumerate() {
            if let Some((_, pose)) = entry
                .lamps
                .iter()
                .filter_map(|index| stream.lamp_button_at(*index))
                .find(|(button, _)| *button == &node.source)
            {
                local(i, pose);
            }
        }
        // Source `coolantTankscript.Cool` turns the fan around its local Z axis.
        for radiator in powertrain.radiators().filter(|r| &r.parent == id) {
            if let Some(node) = radiator.record.fan_node {
                let rest = prototype.nodes[node as usize].transform;
                let rotation = Rotation::from_array(rest.rotation)
                    * Rotation::from_rotation_z(radiator.fan_angle.to_radians());
                local(
                    node as usize,
                    Transform {
                        rotation: rotation.to_array(),
                        ..rest
                    },
                );
            }
        }
        // Gauge and car instrument needles and counter digits turn in place.
        let gauges = || powertrain.gauges().filter(|g| &g.parent == id);
        for gauge in gauges() {
            let (r, state) = (&gauge.record, &gauge.state);
            let needles = r.needles.iter().zip(&state.needle_angles).map(|(n, &a)| {
                let mut e = n.rest;
                e[n.axis as usize] = a;
                (n.node, e)
            });
            let digits = r
                .counters
                .iter()
                .flat_map(|c| &c.digits)
                .zip(&state.digit_angles)
                .map(|(d, &a)| (d.node, digit_euler(d.axis, d.inv, a)));
            for (node, e) in needles.chain(digits) {
                poses.push(rotation_row(node.0, e));
            }
        }
        if let Some((record, vehicle)) = car
            && let (Some(r), Some(state)) = (&record.instruments, &vehicle.instruments)
        {
            let needles = r.needles.iter().zip(&state.needle_angles).map(|(n, &a)| {
                let mut e = n.rest;
                e[n.axis as usize] = a;
                (n.node, e)
            });
            let digits = r
                .digits()
                .iter()
                .zip(&state.digit_angles)
                .map(|(d, &a)| (d.node, digit_euler(d.axis, d.inv, a)));
            for (node, e) in needles.chain(digits) {
                poses.push(rotation_row(node.0, e));
            }
        }
        for (body, handle) in prototype.bodies.iter().enumerate().zip(&structure.bodies) {
            let pose = physics.rapier().bodies[*handle].position();
            poses.push(NodePoseRow {
                node: body.0 as u32,
                kind: NodePoseRow::BODY,
                translation: pose.translation.to_array(),
                rotation: pose.rotation.to_array(),
                scale: [1.; 3],
            });
        }
        let tool = stream.tool_look(id);
        // drotkefescript: the brush model slides on the contact.
        if let Some((node, pose)) = tool.and_then(|t| t.model) {
            poses.push(NodePoseRow {
                node: node as u32,
                kind: NodePoseRow::CONTENT,
                translation: pose.position,
                rotation: pose.rotation,
                scale: [1.; 3],
            });
        }
        for node in 0..prototype.nodes.len() {
            if let Some(pose) = content.node_pose(node) {
                poses.push(NodePoseRow {
                    node: node as u32,
                    kind: NodePoseRow::CONTENT,
                    translation: pose.position,
                    rotation: pose.rotation,
                    scale: [1.; 3],
                });
            }
        }
        row.poses = [start, poses.len() as u32];
        if let Some(color) = stream.lamp_emission(id) {
            row.flags |= StructureRow::LAMP;
            row.lamp = color;
        }
        // displayscript.Upd: on shows defMaterial with the computer output as _EmissionMap.
        if let Some((on, map)) = stream.display_screen(id) {
            row.flags |= StructureRow::DISPLAY;
            if on {
                row.flags |= StructureRow::DISPLAY_ON;
            }
            row.display_map = map.map_or(0, |item| item.0 + 1);
        }
        let cobwebs = stream.cobweb_hidden(id);
        let start = frame.hidden_nodes.len() as u32;
        // Source washtriggerscript GotDirty and GotClean set the car blood nodes active.
        let blood = |n: usize| car.and_then(|(_, v)| v.blood.as_ref()?.visible(n as u32));
        for i in 0..prototype.nodes.len() {
            let enabled = active_food(prototype, i, appearance, reference, food, blood)
                && !cobwebs.contains(&i)
                && tool.is_none_or(|t| t.hidden != Some(i))
                && !self.gun_hidden.contains(&(i as u32))
                && car
                    .and_then(|(record, v)| v.node_visible(record, i as u32))
                    .or_else(|| {
                        trailer.and_then(|(_, l)| {
                            l.visibility()
                                .iter()
                                .find(|(n, _)| *n == i as u32)
                                .map(|(_, visible)| *visible)
                        })
                    })
                    .unwrap_or(true);
            if !enabled {
                frame.hidden_nodes.push(i as u32);
            }
        }
        row.hidden = [start, frame.hidden_nodes.len() as u32];
        let start = frame.node_materials.len() as u32;
        for (i, node) in prototype.nodes.iter().enumerate() {
            let selected = car
                .and_then(|(record, v)| v.renderer_material(record, i as u32))
                .or_else(|| {
                    let (record, lights) = trailer?;
                    lights
                        .materials()
                        .iter()
                        .find(|m| m.node == i as u32)
                        .and_then(|m| m.choice)
                        .and_then(|c| c.link(&record.lights))
                })
                .or_else(|| gauges().find_map(|g| g.state.renderer_material(&g.record, i as u32)))
                .map(|link| &link.source)
                .or_else(|| {
                    appearance
                        .iter()
                        .rev()
                        .find(|a| a.node == node.source && a.material.is_some())
                        .and_then(|a| a.material.as_ref())
                });
            if let Some(source) = selected {
                let next = self.sources.len() as u32;
                let source = *self.sources.entry(source.clone()).or_insert_with(|| {
                    frame.changes.push(Change::MaterialSource(source.clone()));
                    next
                });
                frame.node_materials.push(MaterialRow {
                    node: i as u32,
                    source,
                });
            }
        }
        row.materials = [start, frame.node_materials.len() as u32];
        // partconditionscript colors. The renderer keeps the construction color without a row.
        let start = frame.node_colors.len() as u32;
        for (i, node) in prototype.nodes.iter().enumerate() {
            let Some(look) = appearance.iter().rev().find(|a| {
                a.node == node.source && (a.color.is_some() || a.color_material.is_some())
            }) else {
                continue;
            };
            let color = look.color.or_else(|| {
                let id = look.color_material.as_ref()?;
                reference
                    .rendering
                    .materials
                    .iter()
                    .find(|m| &m.source == id)
                    .map(|m| m.color)
                    .or_else(|| {
                        prototype
                            .deferred_materials
                            .iter()
                            .find(|m| &m.source == id)
                            .and_then(|m| m.colors.get("_Color").copied())
                    })
            });
            if let Some(color) = color {
                frame.node_colors.push(NodeColorRow {
                    node: i as u32,
                    color,
                });
            }
        }
        // sprayscript.UpdColor: the renderer shows the paint color.
        if let Some((node, color)) = tool.and_then(|t| t.color) {
            frame.node_colors.push(NodeColorRow {
                node: node as u32,
                color,
            });
        }
        row.colors = [start, frame.node_colors.len() as u32];
        frame.structures.push(row);
        Ok(())
    }
}

fn rotation_row(node: u32, euler: [f32; 3]) -> NodePoseRow {
    NodePoseRow {
        node,
        kind: NodePoseRow::ROTATION,
        rotation: crate::simulation::instruments::source_rotation(euler),
        ..Default::default()
    }
}

fn digit_euler(axis: u8, inv: bool, angle: f32) -> [f32; 3] {
    let mut e = [0.; 3];
    e[axis as usize] = angle * if inv { -1. } else { 1. };
    e
}

/// A generated food owns its complete source instance.
fn check_food(
    reference: &ReferenceManifest,
    prototype: &WorldPrototypeReference,
    item: &ItemState,
) -> Result<(), ReferenceError> {
    let recipe = &reference.survival.foods[item.food.unwrap().prototype_index];
    let root = reference
        .nodes
        .iter()
        .find(|n| n.id == recipe.root)
        .ok_or_else(|| ReferenceError("Missing food root".into()))?;
    let mut roots = prototype.nodes.iter().filter(|n| n.parent.is_none());
    match (roots.next(), roots.next()) {
        (Some(only), None) if only.source == root.source => Ok(()),
        _ => Err(ReferenceError(
            "Generated food recipe does not own its complete source instance".into(),
        )),
    }
}

fn active_food(
    prototype: &WorldPrototypeReference,
    mut node: usize,
    appearance: &[NodeAppearance],
    reference: &ReferenceManifest,
    item: Option<&ItemState>,
    set: impl Fn(usize) -> Option<bool>,
) -> bool {
    let Some(item) = item else {
        return active_with(prototype, node, appearance, set);
    };
    let state = item.food.unwrap();
    let food = &reference.survival.foods[state.prototype_index];
    for _ in 0..prototype.nodes.len() {
        let source = &prototype.nodes[node].source;
        let part = food.parts.iter().position(|group| {
            group.iter().any(|id| {
                reference
                    .nodes
                    .iter()
                    .any(|n| n.id == *id && &n.source == source)
            })
        });
        let root = reference
            .nodes
            .iter()
            .any(|n| n.id == food.root && &n.source == source);
        let enabled = if root && item.removed {
            false
        } else if let Some(part) = part {
            !item.removed && part == state.bite as usize
        } else {
            appearance
                .iter()
                .rev()
                .find(|a| &a.node == source && a.active.is_some())
                .and_then(|a| a.active)
                .unwrap_or(prototype.nodes[node].parent.is_none() || prototype.nodes[node].active)
        };
        if !enabled {
            return false;
        }
        let Some(parent) = prototype.nodes[node].parent else {
            return true;
        };
        node = parent as usize;
    }
    false
}

/// The node and its ancestors are active after the appearance overrides.
pub fn active(
    prototype: &WorldPrototypeReference,
    node: usize,
    appearance: &[NodeAppearance],
) -> bool {
    active_with(prototype, node, appearance, |_| None)
}

/// `active` with the runtime `set` override before the appearance overrides.
fn active_with(
    prototype: &WorldPrototypeReference,
    mut node: usize,
    appearance: &[NodeAppearance],
    set: impl Fn(usize) -> Option<bool>,
) -> bool {
    for _ in 0..prototype.nodes.len() {
        if !set(node)
            .or_else(|| {
                appearance
                    .iter()
                    .rev()
                    .find(|a| a.node == prototype.nodes[node].source && a.active.is_some())
                    .and_then(|a| a.active)
            })
            .unwrap_or(prototype.nodes[node].parent.is_none() || prototype.nodes[node].active)
        {
            return false;
        }
        let Some(parent) = prototype.nodes[node].parent else {
            return true;
        };
        node = parent as usize;
    }
    false
}
