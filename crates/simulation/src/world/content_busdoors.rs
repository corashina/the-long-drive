//! Source bus doors on world prototypes: `busdoorscripts` buttons and lamps,
//! `busdooropenscript` leaves and `dinamiccolliderforbus` boxes.
use super::WorldStream;
use super::content_index::ContentSlot;
use super::content_kinds::{BindContext, Bound, ContentKind, ContentKinds, KindBindings, Ready};
use crate::electronics::{Emp, NodeElectronics};
use crate::interaction::items::ItemStore;
use crate::interaction::{InteractionState, Knob};
use crate::reference::{
    CarShotReference, ColliderId, ItemId, ReferenceError, SourceId, Transform,
    WorldPrototypeReference,
};
use crate::simulation::dials::{lerp, unity_euler};
use crate::vehicle::{PhysicsWorld, to_rapier};
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
    /// `Update`. Lerps the euler angles to the target. Returns true when the rotation changed.
    fn advance(&mut self, dt: f32) -> bool {
        let target = if self.open { self.opened } else { self.closed };
        let t = dt * self.speed;
        self.current = std::array::from_fn(|i| lerp(self.current[i], target[i], t));
        let rotation = unity_euler(self.current);
        let changed = rotation != self.pose.rotation;
        self.pose.rotation = rotation;
        changed
    }
    /// Sets the restored state. The leaf starts at its target.
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
    /// `electronics`. None: always powered.
    electronics: Option<NodeElectronics>,
    /// `mainscript.lightTurnOffValue`.
    cutoff: f32,
    time: f32,
    random: UnityRandom,
}
impl BusDoors {
    /// `Electronics()`: no electronics, or output above `mainscript.lightTurnOffValue`.
    fn powered(&self) -> bool {
        self.electronics
            .as_ref()
            .is_none_or(|e| e.state.output() > self.cutoff)
    }
    /// One turned usable of door `door`: `Uopen` when `open`, else `Usign`. Calls `play`
    /// with the leaf audio index and the sound index of each leaf sound. Returns false
    /// without power.
    fn press(
        &mut self,
        door: usize,
        open: bool,
        mut play: impl FnMut(Option<usize>, usize),
    ) -> bool {
        if !self.powered() {
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

/// One `dinamiccolliderforbus` column. The box size z follows the distance from `t1` to `t2`.
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
        if let Some((_, doors, boxes)) = self
            .kinds
            .bus_door_scripts
            .iter()
            .find(|(p, ..)| p == prototype)
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
        self.kinds
            .bus_door_scripts
            .push((prototype.clone(), doors.clone(), boxes.clone()));
        Ok((doors, boxes))
    }
    fn bind_bus_doors(
        &self,
        interaction: &mut InteractionState,
        physics: &mut PhysicsWorld,
        slot: usize,
        script: Option<&SourceId>,
        boxes: &[SourceId],
    ) -> Result<BusDoorBinding, ReferenceError> {
        let content = &self.contents.active()[slot];
        let parent = content.content.placement.id.clone();
        let rules = &self.contents.rules;
        let reference = &*self.reference;
        let prototype_index = reference.prototype_index(&content.content.placement.prototype)?;
        let prototype = &reference.prototypes[prototype_index];
        let instance = self
            .structures
            .iter()
            .find(|s| s.placement.id == parent)
            .unwrap();
        let index = |link: &SourceId| -> Result<Option<usize>, ReferenceError> {
            let node = rules.root(reference, link)?;
            Ok(prototype.nodes.index(node))
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
        let mut door_electronics = None;
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
                door_electronics = super::super::content_vehicles::node_electronics(
                    reference,
                    rules,
                    prototype,
                    node as u32,
                )?;
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
                            let id = interaction.bind_knob(physics, *h, Knob::Switch, None)?;
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
            electronics: door_electronics,
            cutoff: reference.electronics_cutoff,
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
        let b = &self.kinds.bus_doors.list[index];
        let d = &b.state.doors[door];
        let [red_on, red_off, green_on, green_off] = &b.materials;
        let (green, red) = match d.lamps() {
            DoorLamps::Open => (green_off, red_on),
            DoorLamps::Signed => (green_on, red_off),
            DoorLamps::Off => (green_off, red_off),
        };
        let appearance = &mut self.contents.active_mut()[b.slot].content.appearance;
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
            .kinds
            .bus_doors
            .list
            .iter()
            .position(|b| b.buttons.iter().any(|(k, ..)| *k == knob))
        else {
            return false;
        };
        let reference = self.reference.clone();
        let audio = reference.prototypes[self.kinds.bus_doors.list[index].prototype]
            .vehicle
            .as_ref()
            .and_then(|v| v.audio.as_ref());
        let key = ObjectKey::Generated(self.kinds.bus_doors.list[index].parent.clone());
        for button in 0..self.kinds.bus_doors.list[index].buttons.len() {
            let b = &mut self.kinds.bus_doors.list[index];
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
    /// EMPs jam the bus door electronics at the bus item position.
    pub fn expose_bus_doors(&mut self, emps: &[Emp], items: &ItemStore, dt: f32) {
        for b in &mut self.kinds.bus_doors.list {
            let Some(e) = &mut b.state.electronics else {
                continue;
            };
            let Some(item) = self.contents.active()[b.slot]
                .item
                .and_then(|i| items.get(i))
            else {
                continue;
            };
            e.state
                .expose(emps, to_rapier(item.pose).translation, e.layer, dt);
        }
    }
    /// `busdooropenscript.Update` and `dinamiccolliderforbus.Update` of every bound bus.
    pub fn advance_bus_doors(&mut self, dt: f32, physics: &mut PhysicsWorld) {
        if !dt.is_finite() || dt < 0. {
            return;
        }
        for index in 0..self.kinds.bus_doors.list.len() {
            let b = &mut self.kinds.bus_doors.list[index];
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
        let b = &self.kinds.bus_doors.list[index];
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
    /// The parent seed and the doors of each bound bus.
    pub fn bus_door_states(&self) -> impl Iterator<Item = (u64, &[Door])> {
        self.kinds
            .bus_doors
            .list
            .iter()
            .map(|b| (b.parent.seed(), &b.state.doors[..]))
    }
    /// Source `UpdBus`: bit i of `opened` and `signed` is door i. False if no bus has seed `seed`.
    pub fn set_bus_doors(&mut self, seed: u64, opened: u32, signed: u32) -> bool {
        let Some(index) = self
            .kinds
            .bus_doors
            .list
            .iter()
            .position(|b| b.parent.seed() == seed)
        else {
            return false;
        };
        let b = &mut self.kinds.bus_doors.list[index];
        let mut changed = 0u32;
        for (i, d) in b.state.doors.iter_mut().enumerate().take(32) {
            let open = opened & (1 << i) != 0;
            let sign = signed & (1 << i) != 0;
            if (d.opened, d.signed) == (open, sign) {
                continue;
            }
            if d.opened != open {
                for leaf in &mut b.state.leaves[d.leaves.clone()] {
                    leaf.set(open, b.state.time, &mut b.state.random);
                }
            }
            (d.opened, d.signed) = (open, sign);
            changed |= 1 << i;
        }
        if changed != 0 {
            let key = ObjectKey::Generated(b.parent.clone());
            self.changes.set_bus_doors(key, b.state.capture());
            for door in (0..32).filter(|i| changed & (1 << i) != 0) {
                self.show_door_lamps(index, door);
            }
        }
        true
    }
    pub fn bus_doors(&self, parent: &GeneratedId) -> Option<&BusDoorBinding> {
        self.kinds
            .bus_doors
            .list
            .iter()
            .find(|b| &b.parent == parent)
    }
}

impl Bound for BusDoorBinding {
    fn parent(&self) -> &GeneratedId {
        &self.parent
    }
    fn item(&self) -> Option<ItemId> {
        None
    }
    fn set_slot(&mut self, slot: usize) {
        self.slot = slot;
    }
}

pub(super) struct BusDoorKind;
impl ContentKind for BusDoorKind {
    type Binding = BusDoorBinding;
    type Script = (Option<SourceId>, Vec<SourceId>);
    const BY_ITEM: bool = false;
    fn kind(kinds: &mut ContentKinds) -> &mut KindBindings<BusDoorBinding> {
        &mut kinds.bus_doors
    }
    fn script(
        stream: &mut WorldStream,
        _: &BindContext,
        slot: usize,
    ) -> Result<Option<Self::Script>, ReferenceError> {
        let prototype = stream.contents.active()[slot]
            .content
            .placement
            .prototype
            .clone();
        let (doors, boxes) = stream.bus_door_scripts(&prototype)?;
        Ok((doors.is_some() || !boxes.is_empty()).then_some((doors, boxes)))
    }
    fn ready(stream: &WorldStream, _: &ItemStore, slot: usize) -> Ready {
        let content = &stream.contents.active()[slot];
        let parent = &content.content.placement.id;
        if content.restored && stream.structures.contains(parent) {
            Ready::Bind
        } else {
            Ready::Wait
        }
    }
    fn bind(
        stream: &mut WorldStream,
        cx: &mut BindContext,
        slot: usize,
        _: Option<ItemId>,
        (doors, boxes): Self::Script,
    ) -> Result<Option<BusDoorBinding>, ReferenceError> {
        stream
            .bind_bus_doors(cx.interaction, cx.physics, slot, doors.as_ref(), &boxes)
            .map(Some)
    }
    fn bound(stream: &mut WorldStream, cx: &mut BindContext, index: usize) {
        let parent = ObjectKey::Generated(stream.kinds.bus_doors.list[index].parent.clone());
        if stream.changes.bus_doors(&parent).is_some() {
            for door in 0..stream.kinds.bus_doors.list[index].state.doors.len() {
                stream.show_door_lamps(index, door);
            }
        }
        stream.place_bus_doors(index, cx.physics);
    }
    fn unbind(binding: &BusDoorBinding, interaction: &mut InteractionState) {
        for (knob, ..) in &binding.buttons {
            interaction.controls.remove_knob(*knob);
        }
    }
}
