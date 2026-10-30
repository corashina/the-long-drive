//! Source bus doors on world prototypes: `busdoorscripts` buttons and lamps,
//! `busdooropenscript` leaves and `dinamiccolliderforbus` boxes.
use super::WorldStream;
use super::content_index::ContentSlot;
use crate::interaction::{InteractionState, Knob};
use crate::reference::{
    CarShotReference, ColliderId, ReferenceError, SourceId, Transform, WorldPrototypeReference,
};
use crate::simulation::dials::{lerp, unity_euler};
use crate::vehicle::PhysicsWorld;
use crate::world::spawn_appearance::NodeAppearance;
use crate::world::spawn_rules::*;
use crate::world::unity_random::UnityRandom;
use crate::world::{GeneratedId, ObjectKey};
use rapier3d::glamx::{Affine3A, Quat, Vec3};
use rapier3d::math::{Pose, Rotation, Vector};
use rapier3d::prelude::ColliderHandle;

/// The lamps of one door: green `Rbs` and red `Rbc` renderers.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DoorLamps {
    /// Green off, red off.
    Off,
    /// `Bsigned`: green on, red off.
    Signed,
    /// `Bopened`: green off, red on.
    Open,
}

/// One `busdooropenscript` leaf.
pub struct DoorLeaf {
    /// Prototype node index.
    #[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
    pub node: usize,
    /// Local pose. The rotation follows `current`.
    pub pose: Transform,
    closed: [f32; 3],
    opened: [f32; 3],
    current: [f32; 3],
    speed: f32,
    open: bool,
    min_time: f32,
    last_used: f32,
    sounds: usize,
    /// Index in the car audio `doors` list.
    audio: Option<usize>,
}
impl DoorLeaf {
    /// `Open` and `Close`: the sound index to play, if the cooldown allows one.
    fn set(&mut self, open: bool, time: f32, random: &mut UnityRandom) -> Option<usize> {
        let sound = (self.sounds > 0 && time - self.last_used >= self.min_time)
            .then(|| random.range_int(0, self.sounds as i32) as usize);
        self.open = open;
        self.last_used = time;
        sound
    }
    /// `Update`: euler lerp to the target. Returns true when the rotation changed.
    fn advance(&mut self, dt: f32) -> bool {
        let target = if self.open { self.opened } else { self.closed };
        let t = dt * self.speed;
        self.current = std::array::from_fn(|i| lerp(self.current[i], target[i], t));
        let rotation = unity_euler(self.current);
        let changed = rotation != self.pose.rotation;
        self.pose.rotation = rotation;
        changed
    }
    /// Restored state: the leaf starts at its target.
    fn snap(&mut self, open: bool) {
        self.open = open;
        self.current = if open { self.opened } else { self.closed };
        self.pose.rotation = unity_euler(self.current);
    }
}

/// One `busdoor` entry.
pub struct Door {
    pub opened: bool,
    pub signed: bool,
    leaves: std::ops::Range<usize>,
    green: Vec<SourceId>,
    red: Vec<SourceId>,
}
impl Door {
    pub fn lamps(&self) -> DoorLamps {
        if self.opened {
            DoorLamps::Open
        } else if self.signed {
            DoorLamps::Signed
        } else {
            DoorLamps::Off
        }
    }
}

/// Door and leaf state of one `busdoorscripts`.
pub struct BusDoors {
    pub doors: Vec<Door>,
    pub leaves: Vec<DoorLeaf>,
    /// `Electronics()`: no electronics, or output above `mainscript.lightTurnOffValue`.
    powered: bool,
    time: f32,
    random: UnityRandom,
}
impl BusDoors {
    /// One turned usable of door `door`: `Uopen` when `open`, else `Usign`. Calls `play`
    /// with the leaf audio index and the sound index of each leaf sound. Returns false
    /// without power.
    fn press(
        &mut self,
        door: usize,
        open: bool,
        mut play: impl FnMut(Option<usize>, usize),
    ) -> bool {
        if !self.powered {
            return false;
        }
        let d = &mut self.doors[door];
        if !open {
            d.signed = true;
            return true;
        }
        d.opened = !d.opened;
        d.signed = false;
        for index in d.leaves.clone() {
            let leaf = &mut self.leaves[index];
            if let Some(sound) = leaf.set(d.opened, self.time, &mut self.random) {
                play(leaf.audio, sound);
            }
        }
        true
    }
    /// `Bopened` and `Bsigned` of each door.
    pub fn capture(&self) -> Vec<[bool; 2]> {
        self.doors.iter().map(|d| [d.opened, d.signed]).collect()
    }
    fn restore(&mut self, saved: &[[bool; 2]]) -> Result<(), ReferenceError> {
        if saved.len() != self.doors.len() {
            return Err(ReferenceError("Saved bus door count differs".into()));
        }
        for (d, [opened, signed]) in self.doors.iter_mut().zip(saved) {
            (d.opened, d.signed) = (*opened, *signed);
            for leaf in &mut self.leaves[d.leaves.clone()] {
                leaf.snap(*opened);
            }
        }
        Ok(())
    }
}

/// A collider below a leaf. It turns about the leaf pivot.
struct LeafCollider {
    handle: ColliderHandle,
    leaf: usize,
    /// Pose in the root body frame at bind.
    base: Pose,
    /// Inverse root frame rotation of the leaf at bind.
    base_rotation: Quat,
}

/// One `dinamiccolliderforbus` column: box size z follows the `t1`-`t2` distance.
struct DynamicBox {
    handle: ColliderHandle,
    ends: [usize; 2],
    /// Body half extent per source half size, with the placement scale.
    factor: f32,
}

pub struct BusDoorBinding {
    pub(super) parent: GeneratedId,
    slot: usize,
    prototype: usize,
    /// Button colliders: door index and `Uopen` (true) or `Usign`.
    buttons: Vec<(ColliderId, usize, bool)>,
    /// `redON`, `redOFF`, `greenON`, `greenOFF`.
    materials: [SourceId; 4],
    pub state: BusDoors,
    colliders: Vec<LeafCollider>,
    boxes: Vec<DynamicBox>,
    /// Placement scale: body units per root frame unit.
    scale: f32,
}
impl BusDoorBinding {
    pub fn heap_bytes(&self) -> usize {
        let ids = |v: &Vec<SourceId>| {
            v.capacity() * std::mem::size_of::<SourceId>()
                + v.iter().map(|s| s.file.capacity()).sum::<usize>()
        };
        crate::world::contents::identity_heap(&self.parent)
            + self.buttons.capacity() * std::mem::size_of::<(ColliderId, usize, bool)>()
            + self
                .materials
                .iter()
                .map(|m| m.file.capacity())
                .sum::<usize>()
            + self.state.doors.capacity() * std::mem::size_of::<Door>()
            + self
                .state
                .doors
                .iter()
                .map(|d| ids(&d.green) + ids(&d.red))
                .sum::<usize>()
            + self.state.leaves.capacity() * std::mem::size_of::<DoorLeaf>()
            + self.colliders.capacity() * std::mem::size_of::<LeafCollider>()
            + self.boxes.capacity() * std::mem::size_of::<DynamicBox>()
    }
    #[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
    pub fn leaves(&self) -> &[DoorLeaf] {
        &self.state.leaves
    }
}

/// Pose of prototype node `node` in the root frame, with the rotations of `leaves`.
fn root_affine(prototype: &WorldPrototypeReference, leaves: &[DoorLeaf], node: usize) -> Affine3A {
    let mut affine = Affine3A::IDENTITY;
    let mut current = node;
    while let Some(parent) = prototype.nodes[current].parent {
        let t = &prototype.nodes[current].transform;
        let rotation = leaves
            .iter()
            .find(|l| l.node == current)
            .map_or(t.rotation, |l| l.pose.rotation);
        affine = Affine3A::from_scale_rotation_translation(
            Vec3::from_array(t.scale),
            Quat::from_array(rotation),
            Vec3::from_array(t.translation),
        ) * affine;
        current = parent as usize;
    }
    affine
}

/// Root frame rotation of a node, with the rotations of `leaves`.
fn root_rotation(prototype: &WorldPrototypeReference, leaves: &[DoorLeaf], node: usize) -> Quat {
    root_affine(prototype, leaves, node)
        .to_scale_rotation_translation()
        .1
}

/// Sets the material of renderer node `node` in a content appearance.
fn set_material(appearance: &mut Vec<NodeAppearance>, node: &SourceId, material: &SourceId) {
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

impl WorldStream {
    /// The `busdoorscripts` and `dinamiccolliderforbus` sources of a prototype, cached.
    fn bus_door_scripts(
        &mut self,
        prototype: &SourceId,
    ) -> Result<(Option<SourceId>, Vec<SourceId>), ReferenceError> {
        if let Some((_, doors, boxes)) = self.bus_door_scripts.iter().find(|(p, ..)| p == prototype)
        {
            return Ok((doors.clone(), boxes.clone()));
        }
        let mut found = Vec::new();
        self.contents.rules.children(
            &self.reference,
            prototype,
            "busdoorscripts",
            true,
            &mut found,
        )?;
        let doors = match found.as_slice() {
            [] => None,
            [script] => Some((*script).clone()),
            _ => {
                return Err(ReferenceError(
                    "Prototype has more than one busdoorscripts".into(),
                ));
            }
        };
        found.clear();
        self.contents.rules.children(
            &self.reference,
            prototype,
            "dinamiccolliderforbus",
            true,
            &mut found,
        )?;
        let boxes: Vec<SourceId> = found.into_iter().cloned().collect();
        self.bus_door_scripts
            .push((prototype.clone(), doors.clone(), boxes.clone()));
        Ok((doors, boxes))
    }
    pub(super) fn sync_bus_door_bindings(
        &mut self,
        interaction: &mut InteractionState,
        physics: &mut PhysicsWorld,
    ) -> Result<(), ReferenceError> {
        let revision = (self.request.session, self.contents.active_revision());
        if self.bus_door_scan == Some(revision) {
            return Ok(());
        }
        let mut complete = true;
        let mut i = 0;
        while i < self.bus_door_bindings.len() {
            let b = &mut self.bus_door_bindings[i];
            if let Some(slot) = self
                .contents
                .active
                .iter()
                .position(|c| c.content.placement.id == b.parent)
            {
                b.slot = slot;
                i += 1;
            } else {
                for (knob, ..) in &b.buttons {
                    interaction.remove_dial_knob(*knob);
                }
                self.bus_door_bindings.swap_remove(i);
            }
        }
        for slot in 0..self.contents.active.len() {
            let content = &self.contents.active[slot];
            if self
                .bus_door_bindings
                .iter()
                .any(|b| b.parent == content.content.placement.id)
            {
                continue;
            }
            let parent = content.content.placement.id.clone();
            let prototype = content.content.placement.prototype.clone();
            let restored = content.restored;
            let (doors, boxes) = self.bus_door_scripts(&prototype)?;
            if doors.is_none() && boxes.is_empty() {
                continue;
            }
            if !restored || !self.structures.iter().any(|s| s.placement.id == parent) {
                complete = false;
                continue;
            }
            let binding =
                self.bind_bus_doors(interaction, physics, slot, doors.as_ref(), &boxes)?;
            self.bus_door_bindings.push(binding);
            let index = self.bus_door_bindings.len() - 1;
            if self
                .changes
                .bus_doors(&ObjectKey::Generated(parent))
                .is_some()
            {
                for door in 0..self.bus_door_bindings[index].state.doors.len() {
                    self.show_door_lamps(index, door);
                }
            }
            self.place_bus_doors(index, physics);
        }
        self.bus_door_scan = complete.then_some(revision);
        Ok(())
    }
    fn bind_bus_doors(
        &self,
        interaction: &mut InteractionState,
        physics: &mut PhysicsWorld,
        slot: usize,
        script: Option<&SourceId>,
        boxes: &[SourceId],
    ) -> Result<BusDoorBinding, ReferenceError> {
        let content = &self.contents.active[slot];
        let parent = content.content.placement.id.clone();
        let rules = &self.contents.rules;
        let reference = &*self.reference;
        let prototype_index = reference
            .prototypes
            .iter()
            .position(|p| p.source == content.content.placement.prototype)
            .ok_or_else(|| ReferenceError("Bus door prototype missing".into()))?;
        let prototype = &reference.prototypes[prototype_index];
        let instance = self
            .structures
            .iter()
            .find(|s| s.placement.id == parent)
            .unwrap();
        let index = |link: &SourceId| -> Result<Option<usize>, ReferenceError> {
            let node = rules.root(reference, link)?;
            Ok(prototype.nodes.iter().position(|n| &n.source == node))
        };
        let required = |link: &SourceId| -> Result<usize, ReferenceError> {
            index(link)?.ok_or_else(|| ReferenceError("Bus door node outside its prototype".into()))
        };
        let handle = |collider: &SourceId| {
            instance
                .colliders
                .iter()
                .find(|(s, _)| s == collider)
                .map(|(_, h)| *h)
        };
        let audio = prototype
            .vehicle
            .as_ref()
            .and_then(|v| v.audio.as_ref())
            .map_or(&[][..], |a| &a.doors[..]);
        let mut doors = Vec::new();
        let mut leaves = Vec::new();
        let mut buttons = Vec::new();
        let mut materials = None;
        let mut powered = true;
        if let Some(script) = script {
            let f = rules.get(reference, script)?.fields;
            materials = Some([
                link(f, "redON")?.clone(),
                link(f, "redOFF")?.clone(),
                link(f, "greenON")?.clone(),
                link(f, "greenOFF")?.clone(),
            ]);
            if let Some(electronics) = optional(f, "electronics")? {
                let node = required(electronics)?;
                powered = super::super::content_vehicles::electronics_output(
                    reference,
                    rules,
                    prototype,
                    node as u32,
                )?
                .is_none_or(|output| output > reference.electronics_cutoff);
            }
            for (door, d) in array(f, "doors")?.iter().enumerate() {
                let d = fields(d)?;
                // `detachIfOpen` is null on every bus in the source build.
                if optional(d, "detachIfOpen")?.is_some() {
                    return Err(ReferenceError("Unsupported bus door detachIfOpen".into()));
                }
                for (field, open) in [("Usign", false), ("Uopen", true)] {
                    for usable in array(d, field)? {
                        let node = rules.root(reference, value_link(usable)?)?;
                        for (source, h) in &instance.colliders {
                            let on_usable = prototype
                                .colliders
                                .iter()
                                .find(|c| &c.source == source)
                                .is_some_and(|c| &prototype.nodes[c.node as usize].source == node);
                            if !on_usable {
                                continue;
                            }
                            let mut id =
                                ColliderId((physics.rapier().colliders[*h].user_data >> 64) as u32);
                            if id.0 == 0 {
                                id = interaction.allocate_generated_collider()?;
                                physics.edit_collider(*h, |collider| {
                                    collider.user_data |= (id.0 as u128) << 64;
                                });
                            }
                            interaction.bind_dial_knob(id, Knob::Switch);
                            buttons.push((id, door, open));
                        }
                    }
                }
                let renderers = |field: &str| -> Result<Vec<SourceId>, ReferenceError> {
                    array(d, field)?
                        .iter()
                        .map(|r| Ok(rules.root(reference, value_link(r)?)?.clone()))
                        .collect()
                };
                let first = leaves.len();
                for leaf in array(d, "doors")? {
                    let leaf = value_link(leaf)?;
                    // A leaf on a part outside the prototype is not bound.
                    let Some(node) = index(leaf)? else {
                        continue;
                    };
                    let l = rules.get(reference, leaf)?.fields;
                    let speed = content
                        .content
                        .inputs
                        .door_speeds
                        .iter()
                        .find(|(p, s, _)| p == &parent && s == leaf)
                        .map_or_else(|| number(l, "speed"), |v| Ok(v.2))?;
                    let closed = vector(l, "closed")?;
                    leaves.push(DoorLeaf {
                        node,
                        pose: Transform {
                            rotation: unity_euler(closed),
                            ..prototype.nodes[node].transform
                        },
                        closed,
                        opened: vector(l, "opened")?,
                        current: closed,
                        speed,
                        open: flag(l, "open")?,
                        min_time: number(l, "minTime")?,
                        last_used: f32::NEG_INFINITY,
                        sounds: array(l, "sounds")?.len(),
                        audio: audio.iter().position(|a| &a.source == leaf),
                    });
                }
                doors.push(Door {
                    opened: flag(d, "Bopened")?,
                    signed: flag(d, "Bsigned")?,
                    leaves: first..leaves.len(),
                    green: renderers("Rbs")?,
                    red: renderers("Rbc")?,
                });
            }
        }
        let key = ObjectKey::Generated(parent.clone());
        let mut state = BusDoors {
            doors,
            leaves,
            powered,
            time: 0.,
            random: UnityRandom::new(key.seed() as i32),
        };
        if let Some(saved) = self.changes.bus_doors(&key) {
            state.restore(saved)?;
        }
        let scale = content.content.placement.scale[0];
        // Colliders below a leaf turn with it. Only the root body is supported.
        let root_body = prototype
            .bodies
            .iter()
            .position(|b| b.node == 0)
            .map(|i| instance.bodies[i]);
        let mut colliders = Vec::new();
        for (source, h) in &instance.colliders {
            let Some(c) = prototype.colliders.iter().find(|c| &c.source == source) else {
                continue;
            };
            let mut node = Some(c.node);
            let leaf = loop {
                let Some(n) = node else { break None };
                if let Some(leaf) = state.leaves.iter().position(|l| l.node == n as usize) {
                    break Some(leaf);
                }
                node = prototype.nodes[n as usize].parent;
            };
            let Some(leaf) = leaf else { continue };
            let collider = &physics.rapier().colliders[*h];
            if collider.parent().is_none() || collider.parent() != root_body {
                continue;
            }
            colliders.push(LeafCollider {
                handle: *h,
                leaf,
                base: *collider.position_wrt_parent().unwrap(),
                base_rotation: root_rotation(prototype, &[], state.leaves[leaf].node).inverse(),
            });
        }
        let mut dynamic = Vec::new();
        for script in boxes {
            for col in array(rules.get(reference, script)?.fields, "cols")? {
                let col = fields(col)?;
                let source = link(col, "collider")?;
                let Some(h) = handle(source) else {
                    continue;
                };
                let size = vector(rules.get(reference, source)?.fields, "m_Size")?[2];
                let Some(cuboid) = physics.rapier().colliders[h].shape().as_cuboid() else {
                    return Err(ReferenceError("Bus dynamic collider is not a box".into()));
                };
                if size <= 0. {
                    return Err(ReferenceError("Bus dynamic collider has no length".into()));
                }
                dynamic.push(DynamicBox {
                    handle: h,
                    ends: [required(link(col, "t1")?)?, required(link(col, "t2")?)?],
                    factor: cuboid.half_extents.z / (size * 0.5) * scale,
                });
            }
        }
        Ok(BusDoorBinding {
            parent,
            slot,
            prototype: prototype_index,
            buttons,
            materials: materials.unwrap_or_else(|| {
                let none = SourceId {
                    file: String::new(),
                    path_id: 0,
                };
                [none.clone(), none.clone(), none.clone(), none]
            }),
            state,
            colliders,
            boxes: dynamic,
            scale,
        })
    }
    /// `Refresh` materials of one door.
    fn show_door_lamps(&mut self, index: usize, door: usize) {
        let b = &self.bus_door_bindings[index];
        let d = &b.state.doors[door];
        let [red_on, red_off, green_on, green_off] = &b.materials;
        let (green, red) = match d.lamps() {
            DoorLamps::Open => (green_off, red_on),
            DoorLamps::Signed => (green_on, red_off),
            DoorLamps::Off => (green_off, red_off),
        };
        let appearance = &mut self.contents.active[b.slot].content.appearance;
        for node in &d.green {
            set_material(appearance, node, green);
        }
        for node in &d.red {
            set_material(appearance, node, red);
        }
        let slot = ContentSlot::Active(b.slot);
        if !self.presentation_changes.contains(&slot) {
            self.presentation_changes.push(slot);
        }
    }
    /// One bus door button. Each door that lists the usable reacts. Calls `play` with the
    /// bus and shot of each leaf sound. Returns false when the knob is not a bus door button.
    pub fn use_bus_door_button(
        &mut self,
        knob: ColliderId,
        mut play: impl FnMut(&ObjectKey, &CarShotReference),
    ) -> bool {
        let Some(index) = self
            .bus_door_bindings
            .iter()
            .position(|b| b.buttons.iter().any(|(k, ..)| *k == knob))
        else {
            return false;
        };
        let reference = self.reference.clone();
        let audio = reference.prototypes[self.bus_door_bindings[index].prototype]
            .vehicle
            .as_ref()
            .and_then(|v| v.audio.as_ref());
        let key = ObjectKey::Generated(self.bus_door_bindings[index].parent.clone());
        for button in 0..self.bus_door_bindings[index].buttons.len() {
            let b = &mut self.bus_door_bindings[index];
            let (k, door, open) = b.buttons[button];
            if k != knob {
                continue;
            }
            let changed = b.state.press(door, open, |leaf, sound| {
                if let Some(shot) = leaf.and_then(|a| audio?.doors[a].sounds.get(sound)) {
                    play(&key, shot);
                }
            });
            if changed {
                let saved = b.state.capture();
                self.changes.set_bus_doors(key.clone(), saved);
                self.show_door_lamps(index, door);
            }
        }
        true
    }
    /// `busdooropenscript.Update` and `dinamiccolliderforbus.Update` of every bound bus.
    pub fn advance_bus_doors(&mut self, dt: f32, physics: &mut PhysicsWorld) {
        if !dt.is_finite() || dt < 0. {
            return;
        }
        for index in 0..self.bus_door_bindings.len() {
            let b = &mut self.bus_door_bindings[index];
            b.state.time += dt;
            let mut moved = false;
            for leaf in &mut b.state.leaves {
                moved |= leaf.advance(dt);
            }
            if moved {
                let slot = ContentSlot::Active(b.slot);
                if !self.presentation_changes.contains(&slot) {
                    self.presentation_changes.push(slot);
                }
                self.place_bus_doors(index, physics);
            }
        }
    }
    /// Turns the leaf colliders and sets the dynamic box lengths after a leaf moved.
    fn place_bus_doors(&self, index: usize, physics: &mut PhysicsWorld) {
        let b = &self.bus_door_bindings[index];
        let prototype = &self.reference.prototypes[b.prototype];
        for c in &b.colliders {
            let node = b.state.leaves[c.leaf].node;
            let leaf = root_affine(prototype, &b.state.leaves, node);
            let (_, rotation, pivot) = leaf.to_scale_rotation_translation();
            let turn = Rotation::from_array((rotation * c.base_rotation).to_array());
            let pivot = Vector::from_array((pivot * b.scale).to_array());
            let local = Pose::from_parts(
                pivot + turn * (c.base.translation - pivot),
                turn * c.base.rotation,
            );
            if physics.rapier().colliders[c.handle].position_wrt_parent() != Some(&local) {
                physics.edit_collider(c.handle, |collider| collider.set_position_wrt_parent(local));
            }
        }
        for d in &b.boxes {
            let [a, z] = d
                .ends
                .map(|n| root_affine(prototype, &b.state.leaves, n).translation);
            let half = a.distance(z) * 0.5 * d.factor;
            let current = physics.rapier().colliders[d.handle]
                .shape()
                .as_cuboid()
                .map_or(half, |c| c.half_extents.z);
            if current != half {
                physics.edit_collider(d.handle, |collider| {
                    if let Some(cuboid) = collider.shape_mut().as_cuboid_mut() {
                        cuboid.half_extents.z = half;
                    }
                });
            }
        }
    }
    pub fn bus_doors(&self, parent: &GeneratedId) -> Option<&BusDoorBinding> {
        self.bus_door_bindings.iter().find(|b| &b.parent == parent)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::world::WorldLayer;
    use crate::world::changes::ChangeRecords;

    fn leaf(node: usize) -> DoorLeaf {
        DoorLeaf {
            node,
            pose: Transform {
                translation: [0.; 3],
                rotation: [0., 0., 0., 1.],
                scale: [1.; 3],
            },
            closed: [0.; 3],
            opened: [0., 90., 0.],
            current: [0.; 3],
            speed: 2.,
            open: false,
            min_time: 0.5,
            last_used: f32::NEG_INFINITY,
            sounds: 2,
            audio: Some(node),
        }
    }

    /// Two doors, one leaf each.
    fn bus(powered: bool) -> BusDoors {
        let door = |i: usize| Door {
            opened: false,
            signed: false,
            leaves: i..i + 1,
            green: Vec::new(),
            red: Vec::new(),
        };
        BusDoors {
            doors: vec![door(0), door(1)],
            leaves: vec![leaf(1), leaf(2)],
            powered,
            time: 10.,
            random: UnityRandom::new(7),
        }
    }

    #[test]
    fn bus_door_open_signal_and_power_off_lamps() {
        let mut doors = bus(true);
        let mut sounds = Vec::new();
        assert_eq!(doors.doors[0].lamps(), DoorLamps::Off);
        assert!(doors.press(0, false, |l, s| sounds.push((l, s))));
        assert_eq!(doors.doors[0].lamps(), DoorLamps::Signed);
        assert!(sounds.is_empty());
        // Open clears the signal and swings the leaf with one sound.
        assert!(doors.press(0, true, |l, s| sounds.push((l, s))));
        assert_eq!(doors.doors[0].lamps(), DoorLamps::Open);
        assert!(!doors.doors[0].signed);
        assert_eq!(sounds.len(), 1);
        assert_eq!(sounds[0].0, Some(1));
        assert!(sounds[0].1 < 2);
        assert!(doors.leaves[0].advance(0.1));
        assert!(doors.leaves[0].current[1] > 0.);
        // The cooldown holds the close sound back.
        assert!(doors.press(0, true, |l, s| sounds.push((l, s))));
        assert_eq!(doors.doors[0].lamps(), DoorLamps::Off);
        assert_eq!(sounds.len(), 1);
        // The other door stays as it was.
        assert_eq!(doors.doors[1].lamps(), DoorLamps::Off);

        let mut off = bus(false);
        assert!(!off.press(0, false, |_, _| unreachable!()));
        assert!(!off.press(1, true, |_, _| unreachable!()));
        assert_eq!(off.doors[0].lamps(), DoorLamps::Off);
        assert_eq!(off.doors[1].lamps(), DoorLamps::Off);
        assert!(!off.leaves[1].open);
    }

    #[test]
    fn bus_door_state_saves_and_restores() {
        #[derive(serde::Serialize, serde::Deserialize)]
        struct Saved {
            #[serde(with = "crate::save::identity::lossless")]
            changes: ChangeRecords,
        }
        let key = ObjectKey::Generated(GeneratedId {
            layer: WorldLayer::Startup,
            generator: SourceId {
                file: "sharedassets1.assets".into(),
                path_id: 9,
            },
            unit: [0, 0],
            ordinal: 0,
            children: Vec::new(),
        });
        let mut doors = bus(true);
        doors.press(0, true, |_, _| {});
        doors.press(1, false, |_, _| {});
        let mut changes = ChangeRecords::default();
        changes.set_bus_doors(key.clone(), doors.capture());
        let json = serde_json::to_vec(&Saved { changes }).unwrap();
        let loaded = serde_json::from_slice::<Saved>(&json).unwrap().changes;

        let mut restored = bus(true);
        restored.restore(loaded.bus_doors(&key).unwrap()).unwrap();
        assert_eq!(restored.doors[0].lamps(), DoorLamps::Open);
        assert_eq!(restored.doors[1].lamps(), DoorLamps::Signed);
        // Leaves restart at their target.
        assert_eq!(restored.leaves[0].current, restored.leaves[0].opened);
        assert_eq!(restored.leaves[1].current, restored.leaves[1].closed);
        assert!(restored.restore(&[[true, false]]).is_err());
    }
}
