use super::{
    items::{ItemLocation, ItemStore},
    parts::PartAttachments,
};
use crate::conditions::{ConditionStore, RepairResult};
use crate::particles::ParticleState;
use crate::reference::{
    BrushNodes, BrushTool, ColliderId, ConditionColliderBinding, ConditionId, ItemId, NodeId,
    PrototypeTool, ReferenceError, ReferenceManifest, SourceId, SprayTool, WashId,
};
use crate::vehicle::{PhysicsWorld, VehiclePose, from_rapier, to_rapier};
use rapier3d::{
    math::{Pose, Rotation, Vector},
    prelude::{ColliderHandle, QueryFilter, Ray},
};
use std::collections::HashMap;

#[derive(Clone, Copy, Debug)]
pub struct ToolFrame {
    pub lmb: bool,
    pub active: bool,
    pub dt: f32,
    pub view: VehiclePose,
    pub origin: [f64; 3],
}
/// The world state a tool ray reads.
#[derive(Clone, Copy)]
pub struct ToolWorld<'a> {
    pub conditions: &'a ConditionStore,
    pub items: &'a ItemStore,
    pub physics: &'a PhysicsWorld,
    pub parts: &'a PartAttachments,
}
#[derive(Clone, Copy, Debug)]
pub enum ToolOwner {
    Equipped(ItemId),
    Carried(ItemId),
}
#[derive(Clone, Copy, Debug)]
pub struct ToolSignals {
    /// None: the player brush.
    pub item: Option<ItemId>,
    pub pose: VehiclePose,
    pub force: f32,
    pub doing_work: bool,
    pub playing: bool,
    pub voice_gain: f32,
    pub emission: [f32; 2],
    pub color: [f32; 4],
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Target {
    Surface(ConditionId),
    Wash(WashId),
    /// A washtriggerscript on generated content.
    Generated(WashOwner),
    Solid(ColliderHandle),
}
/// The owner of a generated washtriggerscript.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WashOwner {
    /// The genie lamp item.
    Item(ItemId),
    /// A building cobweb, by its stream id.
    Cobweb(u32),
    /// The blood trigger of a car, by the car root item.
    Car(ItemId),
}
/// A washtriggerscript on generated content, such as the genie lamp or a cobweb.
struct GeneratedWash {
    owner: WashOwner,
    permission: u8,
    never_repair: bool,
    repair: f32,
    colliders: Vec<(ColliderId, ColliderHandle)>,
}
/// Registered generated wash triggers and the ones that got clean.
#[derive(Default)]
struct GeneratedWashes {
    washes: Vec<GeneratedWash>,
    /// GotClean: the trigger is inactive and no longer registered.
    cleaned: Vec<WashOwner>,
}
impl GeneratedWashes {
    fn register(
        &mut self,
        owner: WashOwner,
        permission: u8,
        never_repair: bool,
        colliders: Vec<(ColliderId, ColliderHandle)>,
    ) {
        self.remove(owner);
        self.washes.push(GeneratedWash {
            owner,
            permission,
            never_repair,
            repair: 0.,
            colliders,
        });
    }
    fn remove(&mut self, owner: WashOwner) {
        self.washes.retain(|w| w.owner != owner);
    }
    /// washtriggerscript.Wash. Returns None when the tool cannot wash, else whether it got clean.
    fn wash(
        &mut self,
        owner: WashOwner,
        tables: &[[bool; 7]],
        tool: u8,
        gain: f32,
    ) -> Option<bool> {
        let i = self.washes.iter().position(|w| w.owner == owner)?;
        let w = &mut self.washes[i];
        if !tables[w.permission as usize][tool as usize] {
            return None;
        }
        w.repair += gain;
        let clean = w.repair > 1. && !w.never_repair;
        if clean {
            // GotClean deactivates the trigger object.
            self.washes.swap_remove(i);
            self.cleaned.push(owner);
        }
        Some(clean)
    }
}
struct WashEffect {
    particles: Vec<ParticleState>,
    durations: Vec<f32>,
    elapsed: Option<f32>,
    emitter: [f64; 3],
}
struct Spray {
    owned: bool,
    firing: bool,
    painted: bool,
    nozzle: Pose,
    emitter: Vector,
    rotation: Rotation,
    particles: Vec<ParticleState>,
}
struct Contact {
    equipped: bool,
    hit: Option<(
        ColliderHandle,
        rapier3d::parry::query::RayIntersection,
        Target,
    )>,
    target: Option<Target>,
    collider: Option<ColliderHandle>,
    relative: Vector,
    ray_relative: Vector,
    motion_ready: bool,
    model: Pose,
    origin: [f64; 3],
    sampled: bool,
    /// The slide moves a model node, else the tool root.
    has_model: bool,
    equip: Pose,
    default: Pose,
    particles: Vec<ParticleState>,
}

use crate::save::{SaveError, SourceKey, ToolSnapshot};
#[derive(Clone, Debug)]
pub struct ToolState {
    pub item: ItemId,
    pub quantity: Option<f32>,
    pub color: Option<[f32; 4]>,
    pub disabled: bool,
    pub valve: f32,
}
/// The reference record of a tool.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ToolParams {
    /// An index in `conditions.brushes` or `conditions.sprays`.
    Fixed(usize),
    /// An index in `conditions.prototype_tools`.
    Prototype(usize),
}
enum ToolBody {
    Brush(Contact),
    Spray(Spray),
}
/// One item tool: a drotkefescript brush or a sprayscript spray.
struct Tool {
    params: ToolParams,
    /// Source `drotkefescript.ownCondition`.
    own_condition: Option<ConditionId>,
    /// The LMB text of a brush with `show_text`.
    text: Option<u32>,
    body: ToolBody,
    signal: ToolSignals,
    state: ToolState,
}
/// Source `fpscontroller.pointKefe`: the player hand brush.
struct PlayerBrush {
    contact: Contact,
    signal: ToolSignals,
    /// The player pose: the brush root.
    pose: Pose,
}
/// Absolute poses that place a brush contact.
pub struct BrushPoses {
    pub root: Pose,
    pub model: Option<Pose>,
    pub equip: Option<Pose>,
    pub default: Option<Pose>,
}
pub struct CleaningTools {
    tools: Vec<Tool>,
    /// Tool index by item.
    index: HashMap<ItemId, usize>,
    player: PlayerBrush,
    bindings: HashMap<ColliderId, ConditionColliderBinding>,
    /// World part colliders and their nearest parent condition (`GetComponentInParent`).
    world_conditions: HashMap<ColliderHandle, ConditionId>,
    item_conditions: HashMap<ItemId, ConditionId>,
    washes: Vec<WashEffect>,
    wash_indices: HashMap<WashId, usize>,
    wash_colliders: Vec<(ColliderHandle, WashId, bool)>,
    collider_enabled: HashMap<ColliderId, bool>,
    generated_washes: GeneratedWashes,
    /// Generated items that reserve LMB while equipped, with their LMB text.
    lmb_items: HashMap<ItemId, u32>,
    /// Loot tools whose state changed: their items need a save capture.
    changed: Vec<ItemId>,
}
/// A contact at its rest pose. Missing positions keep the model at the tool root.
fn new_contact(t: &BrushTool, p: BrushPoses) -> Contact {
    let relative = |pose: Option<Pose>| pose.map_or(Pose::IDENTITY, |n| p.root.inverse() * n);
    Contact {
        equipped: false,
        hit: None,
        target: None,
        collider: None,
        relative: Vector::ZERO,
        ray_relative: Vector::ZERO,
        motion_ready: false,
        has_model: p.model.is_some(),
        model: p.model.unwrap_or(p.root),
        origin: [0.; 3],
        sampled: false,
        equip: relative(p.equip),
        default: relative(p.default),
        particles: t
            .particles
            .iter()
            .enumerate()
            .map(|(i, r)| ParticleState::new(r, t.source.path_id as u64 + i as u64))
            .collect(),
    }
}
fn new_signal(item: Option<ItemId>, pose: Pose, color: [f32; 4]) -> ToolSignals {
    ToolSignals {
        item,
        pose: from_rapier(&pose),
        force: 0.,
        doing_work: false,
        playing: false,
        voice_gain: 0.,
        emission: [0.; 2],
        color,
    }
}
/// A spray at rest. `root` and `nozzle` are absolute.
fn new_spray(t: &SprayTool, root: Pose, nozzle: Pose) -> Spray {
    let local = &t.particles[0].local_pose;
    Spray {
        owned: false,
        firing: false,
        painted: false,
        nozzle: root.inverse() * nozzle,
        emitter: Vector::from_array(local.translation),
        rotation: Rotation::from_xyzw(
            local.rotation[0],
            local.rotation[1],
            local.rotation[2],
            local.rotation[3],
        ),
        particles: t
            .particles
            .iter()
            .enumerate()
            .map(|(i, r)| ParticleState::new(r, t.source.path_id as u64 + i as u64))
            .collect(),
    }
}
impl CleaningTools {
    pub fn new(reference: &ReferenceManifest) -> Result<Self, ReferenceError> {
        let c = &reference.conditions;
        c.validate(reference)?;
        let pose = |n: NodeId| super::items::node_pose(reference, n).map(to_rapier);
        let poses = |n: &BrushNodes<NodeId>, root: NodeId| -> Result<BrushPoses, ReferenceError> {
            Ok(BrushPoses {
                root: pose(root)?,
                model: n.model.map(pose).transpose()?,
                equip: n.model_equip.map(pose).transpose()?,
                default: n.model_default.map(pose).transpose()?,
            })
        };
        let item_node = |item: ItemId| reference.items.iter().find(|i| i.id == item).unwrap().node;
        let player = new_contact(
            &c.player_brush.tool,
            poses(&c.player_brush.nodes, reference.clothing.pose.root)?,
        );
        let mut tools = Self {
            tools: Vec::with_capacity(c.brushes.len() + c.sprays.len()),
            index: HashMap::new(),
            player: PlayerBrush {
                signal: new_signal(None, player.model, [1.; 4]),
                contact: player,
                pose: Pose::IDENTITY,
            },
            generated_washes: GeneratedWashes::default(),
            lmb_items: HashMap::new(),
            changed: Vec::with_capacity(4),
            wash_colliders: Vec::with_capacity(c.washes.iter().map(|w| w.colliders.len()).sum()),
            collider_enabled: reference
                .colliders
                .iter()
                .map(|c| (c.id, c.enabled))
                .collect(),
            washes: c
                .washes
                .iter()
                .map(|w| WashEffect {
                    particles: w
                        .particles
                        .iter()
                        .enumerate()
                        .map(|(i, p)| ParticleState::new(p, w.source.path_id as u64 + i as u64))
                        .collect(),
                    durations: w.particles.iter().map(|p| p.duration).collect(),
                    elapsed: None,
                    emitter: super::items::node_pose(reference, w.node)
                        .expect("checked wash pose")
                        .position
                        .map(f64::from),
                })
                .collect(),
            wash_indices: c
                .washes
                .iter()
                .enumerate()
                .map(|(i, w)| (w.id, i))
                .collect(),
            bindings: c
                .collider_bindings
                .iter()
                .map(|b| (b.collider, b.clone()))
                .collect(),
            world_conditions: HashMap::new(),
            item_conditions: c
                .surfaces
                .iter()
                .filter_map(|s| s.item.map(|i| (i, s.id)))
                .collect(),
        };
        for (i, b) in c.brushes.iter().enumerate() {
            let contact = new_contact(&b.tool, poses(&b.nodes, item_node(b.item))?);
            tools.add_brush(
                b.item,
                ToolParams::Fixed(i),
                Some(b.own_condition),
                contact,
                &b.tool,
            );
        }
        for (i, b) in c.sprays.iter().enumerate() {
            let root = pose(item_node(b.item))?;
            let spray = new_spray(&b.tool, root, pose(b.nozzle)?);
            tools.add_spray(
                b.item,
                ToolParams::Fixed(i),
                spray,
                &b.tool,
                b.tool.startup_quantity,
                b.tool.startup_color,
                root,
            );
        }
        Ok(tools)
    }
    fn push(&mut self, tool: Tool) {
        self.index.insert(tool.state.item, self.tools.len());
        self.tools.push(tool);
    }
    fn add_brush(
        &mut self,
        item: ItemId,
        params: ToolParams,
        own_condition: Option<ConditionId>,
        contact: Contact,
        t: &BrushTool,
    ) {
        self.push(Tool {
            params,
            own_condition,
            text: t.show_text.then_some(t.text),
            signal: new_signal(Some(item), contact.model, [1.; 4]),
            body: ToolBody::Brush(contact),
            state: ToolState {
                item,
                quantity: None,
                color: None,
                disabled: t.disabled,
                valve: 0.,
            },
        });
    }
    #[allow(clippy::too_many_arguments)]
    fn add_spray(
        &mut self,
        item: ItemId,
        params: ToolParams,
        spray: Spray,
        t: &SprayTool,
        quantity: f32,
        color: [f32; 4],
        root: Pose,
    ) {
        self.push(Tool {
            params,
            own_condition: None,
            text: None,
            signal: new_signal(Some(item), root * spray.nozzle, color),
            body: ToolBody::Spray(spray),
            state: ToolState {
                item,
                quantity: Some(quantity),
                color: Some(color),
                disabled: false,
                valve: t.startup_valve,
            },
        });
    }
    /// Adds the brush of loot item `item`. `index` is its `conditions.prototype_tools` index.
    pub fn add_prototype_brush(
        &mut self,
        item: ItemId,
        index: usize,
        own_condition: Option<ConditionId>,
        poses: BrushPoses,
        disabled: bool,
        reference: &ReferenceManifest,
    ) {
        let PrototypeTool::Brush { tool, .. } = &reference.conditions.prototype_tools[index].kind
        else {
            unreachable!("brush of a prototype spray");
        };
        self.add_brush(
            item,
            ToolParams::Prototype(index),
            own_condition,
            new_contact(tool, poses),
            tool,
        );
        self.tools.last_mut().expect("added tool").state.disabled = disabled;
    }
    /// Adds the spray of loot item `item`. `root` and `nozzle` are absolute.
    #[allow(clippy::too_many_arguments)]
    pub fn add_prototype_spray(
        &mut self,
        item: ItemId,
        index: usize,
        root: Pose,
        nozzle: Pose,
        quantity: f32,
        color: [f32; 4],
        reference: &ReferenceManifest,
    ) {
        let PrototypeTool::Spray { tool, .. } = &reference.conditions.prototype_tools[index].kind
        else {
            unreachable!("spray of a prototype brush");
        };
        let spray = new_spray(tool, root, nozzle);
        self.add_spray(
            item,
            ToolParams::Prototype(index),
            spray,
            tool,
            quantity,
            color,
            root,
        );
    }
    fn mark_changed(&mut self, tool: usize) {
        let t = &self.tools[tool];
        if matches!(t.params, ToolParams::Prototype(_)) && !self.changed.contains(&t.state.item) {
            self.changed.push(t.state.item);
        }
    }
    /// Loot tool items whose state changed since the last call.
    pub fn take_changed(&mut self) -> std::vec::Drain<'_, ItemId> {
        self.changed.drain(..)
    }
    /// Removes the tool of `item`. Fixed tools never leave.
    pub fn remove_tool(&mut self, item: ItemId) {
        let Some(i) = self.index.remove(&item) else {
            return;
        };
        self.tools.swap_remove(i);
        if let Some(moved) = self.tools.get(i) {
            self.index.insert(moved.state.item, i);
        }
    }
    fn brush_params<'a>(
        &self,
        tool: Option<usize>,
        reference: &'a ReferenceManifest,
    ) -> &'a BrushTool {
        match tool.map(|i| self.tools[i].params) {
            Some(ToolParams::Fixed(i)) => &reference.conditions.brushes[i].tool,
            Some(ToolParams::Prototype(i)) => match &reference.conditions.prototype_tools[i].kind {
                PrototypeTool::Brush { tool, .. } => tool,
                PrototypeTool::Spray { .. } => unreachable!("brush params of a spray"),
            },
            None => &reference.conditions.player_brush.tool,
        }
    }
    /// The spray record of the tool of `item`.
    pub fn spray_tool<'a>(
        &self,
        item: ItemId,
        reference: &'a ReferenceManifest,
    ) -> Option<&'a SprayTool> {
        let &i = self.index.get(&item)?;
        matches!(self.tools[i].body, ToolBody::Spray(_)).then(|| self.spray_params(i, reference))
    }
    fn spray_params<'a>(&self, tool: usize, reference: &'a ReferenceManifest) -> &'a SprayTool {
        match self.tools[tool].params {
            ToolParams::Fixed(i) => &reference.conditions.sprays[i].tool,
            ToolParams::Prototype(i) => match &reference.conditions.prototype_tools[i].kind {
                PrototypeTool::Spray { tool, .. } => tool,
                PrototypeTool::Brush { .. } => unreachable!("spray params of a brush"),
            },
        }
    }
    pub fn bind_collider(&mut self, id: ColliderId, handle: ColliderHandle) {
        if let Some(wash) = self.bindings.get(&id).and_then(|b| b.wash) {
            self.wash_colliders
                .push((handle, wash, self.collider_enabled[&id]));
        }
    }
    pub fn bind_world_condition(&mut self, handle: ColliderHandle, id: ConditionId) {
        self.world_conditions.insert(handle, id);
    }
    pub fn unbind_world_condition(&mut self, id: ConditionId) {
        self.world_conditions.retain(|_, v| *v != id);
    }
    pub fn sync_wash_colliders(&self, conditions: &ConditionStore, physics: &mut PhysicsWorld) {
        let mut changed = false;
        for &(handle, wash, authored) in &self.wash_colliders {
            let active = physics.rapier().colliders[handle]
                .parent()
                .is_none_or(|body| {
                    physics.body_active(body) && physics.rapier().bodies[body].is_enabled()
                });
            physics.edit_collider(handle, |collider| {
                let enabled = authored && active && conditions.wash(wash).visible();
                if collider.is_enabled() != enabled {
                    crate::vehicle::set_collider_enabled(collider, enabled);
                    changed = true;
                }
            });
        }
        if changed {
            physics.refresh_queries();
        }
    }
    pub fn reserves_lmb(&self, equipped: Option<ItemId>, _carried: Option<ItemId>) -> bool {
        equipped.is_some_and(|item| {
            self.lmb_items.contains_key(&item)
                || self
                    .index
                    .get(&item)
                    .is_some_and(|&i| matches!(self.tools[i].body, ToolBody::Brush(_)))
        })
    }
    pub fn prompt(&self, item: ItemId) -> Option<u32> {
        if let Some(text) = self.lmb_items.get(&item) {
            return Some(*text);
        }
        let t = &self.tools[*self.index.get(&item)?];
        t.text.filter(|_| !t.state.disabled)
    }
    pub fn reserve_lmb(&mut self, item: ItemId, text: Option<u32>) {
        match text {
            Some(text) => {
                self.lmb_items.insert(item, text);
            }
            None => {
                self.lmb_items.remove(&item);
            }
        }
    }
    pub fn register_generated_wash(
        &mut self,
        owner: WashOwner,
        permission: u8,
        never_repair: bool,
        colliders: Vec<(ColliderId, ColliderHandle)>,
    ) {
        self.generated_washes
            .register(owner, permission, never_repair, colliders);
    }
    pub fn remove_generated_wash(&mut self, owner: WashOwner) {
        self.generated_washes.remove(owner);
    }
    /// Generated triggers that got clean since the last take.
    pub fn cleaned_washes(&self) -> &[WashOwner] {
        &self.generated_washes.cleaned
    }
    /// Generated triggers that got clean since the last call.
    pub fn take_cleaned_washes(&mut self) -> std::vec::Drain<'_, WashOwner> {
        self.generated_washes.cleaned.drain(..)
    }
    /// ginlampscript reads a positive repairValue as a rub and resets it to 0.
    pub fn take_generated_rub(&mut self, owner: ItemId) -> bool {
        let Some(w) = self
            .generated_washes
            .washes
            .iter_mut()
            .find(|w| w.owner == WashOwner::Item(owner))
        else {
            return false;
        };
        let rubbed = w.repair > 0.;
        if rubbed {
            w.repair = 0.;
        }
        rubbed
    }
    /// washtriggerscript.Wash on a generated trigger. Returns true when it is clean.
    fn wash_generated(
        &mut self,
        owner: WashOwner,
        tool: u8,
        gain: f32,
        reference: &ReferenceManifest,
    ) -> Option<bool> {
        self.generated_washes
            .wash(owner, &reference.conditions.clean_tables, tool, gain)
    }
    /// washtriggerscript.Wash on a checked trigger. Returns true when it is clean.
    fn wash_fixed(
        &mut self,
        id: WashId,
        tool: u8,
        gain: f32,
        reference: &ReferenceManifest,
        conditions: &mut ConditionStore,
    ) -> Option<bool> {
        let wi = self.wash_indices[&id];
        let w = &reference.conditions.washes[wi];
        if !reference.conditions.clean_tables[w.permission_index as usize][tool as usize] {
            return None;
        }
        let state = &mut conditions.washes[wi];
        state.repair_progress += gain;
        if state.repair_progress > 1. && !w.never_repair {
            state.repair_progress = 0.;
            state.active = false;
            state.on = false;
            state.destroyed = w.destroy_on_repair;
            if w.destroy_on_repair {
                self.washes[wi].elapsed = Some(0.);
            }
            return Some(true);
        }
        Some(false)
    }
    /// Physics.OverlapSphere at `point` for the first active wash trigger on `mask`.
    fn nearby_wash(
        &self,
        point: Vector,
        mask: u32,
        radius: f32,
        conditions: &ConditionStore,
        physics: &PhysicsWorld,
    ) -> Option<Target> {
        let near = |handle: ColliderHandle| {
            let collider = &physics.rapier().colliders[handle];
            if !collider.is_enabled() || collider.collision_groups().memberships.bits() & mask == 0
            {
                return false;
            }
            let projected = collider
                .shape()
                .project_point(collider.position(), point, true);
            projected.is_inside || projected.point.distance_squared(point) <= radius * radius
        };
        // Only checked wash collider links can participate in the nearby trigger search.
        for &(handle, wash, _) in &self.wash_colliders {
            if conditions.wash(wash).visible() && near(handle) {
                return Some(Target::Wash(wash));
            }
        }
        self.generated_washes
            .washes
            .iter()
            .find(|w| {
                w.colliders
                    .iter()
                    .any(|&(_, h)| physics.rapier().colliders.get(h).is_some() && near(h))
            })
            .map(|w| Target::Generated(w.owner))
    }
    /// spricniscript.Pressing target order: hit trigger, nearby trigger, slot part, condition.
    pub fn spray_target(
        &self,
        (handle, point): (ColliderHandle, Vector),
        reference: &ReferenceManifest,
        world: ToolWorld<'_>,
    ) -> Option<Target> {
        let ToolWorld {
            conditions,
            items,
            physics,
            parts,
        } = world;
        let r = &reference.small_items.spray_bottle;
        let collider = ColliderId((physics.rapier().colliders[handle].user_data >> 64) as u32);
        let selected = self.target(
            handle,
            collider,
            r.cleaning_type,
            reference,
            conditions,
            items,
            parts,
        );
        spray_order(selected, || {
            self.nearby_wash(point, r.wash_mask, r.wash_radius, conditions, physics)
        })
    }
    /// spricniscript.Fire on a target. Returns true when the splash spawns.
    pub fn spray(
        &mut self,
        target: Target,
        tool: u8,
        gain: f32,
        reference: &ReferenceManifest,
        conditions: &mut ConditionStore,
    ) -> bool {
        match target {
            Target::Wash(id) => self
                .wash_fixed(id, tool, gain, reference, conditions)
                .is_some(),
            Target::Generated(owner) => self.wash_generated(owner, tool, gain, reference).is_some(),
            Target::Surface(id) if conditions.can_repair(id, tool) => {
                conditions.repair(id, tool, gain);
                true
            }
            Target::Surface(_) | Target::Solid(_) => false,
        }
    }
    /// The signal of the tool of `item`.
    pub fn signal(&self, item: ItemId) -> Option<&ToolSignals> {
        self.index.get(&item).map(|&i| &self.tools[i].signal)
    }
    /// The signal of the player hand brush.
    pub fn player_signal(&self) -> &ToolSignals {
        &self.player.signal
    }
    /// The model pose of the brush of `item`.
    pub fn model_pose(&self, item: ItemId, items: &ItemStore) -> VehiclePose {
        let t = &self.tools[self.index[&item]];
        let ToolBody::Brush(c) = &t.body else {
            unreachable!("model pose of a spray");
        };
        if c.motion_ready {
            t.signal.pose
        } else {
            from_rapier(&(to_rapier(items.get(item).unwrap().pose) * c.default))
        }
    }
    /// Loot tools: item, `conditions.prototype_tools` index and particle system count.
    pub fn prototype_tools(&self) -> impl Iterator<Item = (ItemId, usize, usize)> + '_ {
        self.tools.iter().filter_map(|t| {
            let ToolParams::Prototype(index) = t.params else {
                return None;
            };
            let count = match &t.body {
                ToolBody::Brush(c) => c.particles.len(),
                ToolBody::Spray(s) => s.particles.len(),
            };
            Some((t.state.item, index, count))
        })
    }
    /// The live particles of system `particle` of the tool of `item`.
    pub fn particles(&self, item: ItemId, particle: usize) -> &[crate::particles::ParticleOutput] {
        match &self.tools[self.index[&item]].body {
            ToolBody::Brush(c) => c.particles[particle].particles(),
            ToolBody::Spray(s) => s.particles[particle].particles(),
        }
    }
    pub fn wash_particles(
        &self,
        wash: usize,
        particle: usize,
    ) -> &[crate::particles::ParticleOutput] {
        self.washes[wash].particles[particle].particles()
    }
    pub fn reset_contact(&mut self) {
        for t in &mut self.tools {
            t.state.valve = 0.;
            match &mut t.body {
                ToolBody::Brush(c) => reset_brush(c),
                ToolBody::Spray(spray) => {
                    spray.owned = false;
                    spray.firing = false;
                    spray.painted = false;
                    for p in &mut spray.particles {
                        p.reset_emitter();
                    }
                }
            }
            reset_signal(&mut t.signal);
        }
        reset_brush(&mut self.player.contact);
        reset_signal(&mut self.player.signal);
        for wash in &mut self.washes {
            for p in &mut wash.particles {
                p.reset_emitter();
            }
        }
    }
    fn target(
        &self,
        handle: ColliderHandle,
        collider: ColliderId,
        tool: u8,
        reference: &ReferenceManifest,
        conditions: &ConditionStore,
        items: &ItemStore,
        parts: &PartAttachments,
    ) -> Option<Target> {
        if let Some(w) = self
            .generated_washes
            .washes
            .iter()
            .find(|w| w.colliders.iter().any(|&(id, _)| id == collider))
        {
            return Some(Target::Generated(w.owner));
        }
        if let Some(id) = self.world_conditions.get(&handle) {
            return Some(Target::Surface(*id));
        }
        let b = self.bindings.get(&collider)?;
        if let Some(id) = b.wash.filter(|id| conditions.wash(*id).visible()) {
            return Some(Target::Wash(id));
        }
        let installed = b
            .installed_part_slot
            .and_then(|slot| parts.slot_item(slot))
            .or_else(|| {
                b.installed_tire_slot.and_then(|rim| {
                    items
                        .iter()
                        .find(|i| {
                            i.location == ItemLocation::Attached(rim)
                                && reference.items.iter().any(|p| {
                                    p.id == i.id
                                        && matches!(p.kind, crate::reference::ItemKind::Tire)
                                })
                        })
                        .map(|i| i.id)
                })
            });
        let condition = installed
            .and_then(|i| self.item_conditions.get(&i).copied())
            .filter(|id| conditions.can_repair(*id, tool))
            .or(b.ancestor_condition)
            .or(b.forwarded_condition);
        condition.map(Target::Surface)
    }
    fn paint_target(
        &self,
        handle: ColliderHandle,
        collider: ColliderId,
        items: &ItemStore,
        parts: &PartAttachments,
        reference: &ReferenceManifest,
    ) -> Option<ConditionId> {
        if let Some(id) = self.world_conditions.get(&handle) {
            return Some(*id);
        }
        let b = self.bindings.get(&collider)?;
        b.ancestor_condition
            .or_else(|| {
                if let Some(slot) = b.installed_part_slot {
                    return parts
                        .slot_item(slot)
                        .and_then(|i| self.item_conditions.get(&i).copied());
                }
                if let Some(rim) = b.installed_tire_slot {
                    return items
                        .iter()
                        .find(|i| {
                            i.location == ItemLocation::Attached(rim)
                                && reference.items.iter().any(|p| {
                                    p.id == i.id
                                        && matches!(p.kind, crate::reference::ItemKind::Tire)
                                })
                        })
                        .and_then(|i| self.item_conditions.get(&i.id).copied());
                }
                None
            })
            .or(b.pickup_condition)
    }
    fn brush_disabled(&self, tool: Option<usize>, reference: &ReferenceManifest) -> bool {
        match tool {
            Some(i) => self.tools[i].state.disabled,
            None => reference.conditions.player_brush.tool.disabled,
        }
    }
    fn contact(&self, tool: Option<usize>) -> (&Contact, &ToolSignals) {
        match tool {
            Some(i) => {
                let t = &self.tools[i];
                let ToolBody::Brush(c) = &t.body else {
                    unreachable!("brush contact of a spray");
                };
                (c, &t.signal)
            }
            None => (&self.player.contact, &self.player.signal),
        }
    }
    fn contact_mut(&mut self, tool: Option<usize>) -> (&mut Contact, &mut ToolSignals) {
        match tool {
            Some(i) => {
                let t = &mut self.tools[i];
                let ToolBody::Brush(c) = &mut t.body else {
                    unreachable!("brush contact of a spray");
                };
                (c, &mut t.signal)
            }
            None => (&mut self.player.contact, &mut self.player.signal),
        }
    }
    /// Source `fpscontroller.pointKefe.Use` while the point input is held. `player` is the
    /// `pointAt` pose that carries the brush. The brush has no item: it ignores the item
    /// brush checks.
    #[allow(clippy::too_many_arguments)]
    pub fn advance_player(
        &mut self,
        held: bool,
        player: VehiclePose,
        frame: ToolFrame,
        reference: &ReferenceManifest,
        conditions: &mut ConditionStore,
        items: &ItemStore,
        physics: &PhysicsWorld,
        parts: &PartAttachments,
    ) {
        self.player.pose = to_rapier(player);
        if !frame.active || !frame.dt.is_finite() || frame.dt <= 0. {
            return;
        }
        let frame = ToolFrame { lmb: held, ..frame };
        self.player.contact.equipped = held;
        self.player.contact.hit = self.brush_hit(
            None, true, frame, reference, conditions, items, physics, parts,
        );
        self.use_brush(None, frame, reference, conditions);
    }
    pub fn begin_frame(&mut self, frame: ToolFrame) {
        for t in &mut self.tools {
            t.signal.doing_work = false;
            match &mut t.body {
                ToolBody::Brush(c) => {
                    c.equipped = false;
                    c.hit = None;
                }
                ToolBody::Spray(spray) => {
                    spray.owned = false;
                    spray.firing = false;
                    spray.painted = false;
                }
            }
        }
        self.player.contact.equipped = false;
        self.player.contact.hit = None;
        self.player.signal.doing_work = false;
        if !frame.active || !frame.dt.is_finite() || frame.dt <= 0. {
            self.reset_contact();
        }
    }
    pub fn advance_owner(
        &mut self,
        owner: ToolOwner,
        frame: ToolFrame,
        reference: &ReferenceManifest,
        conditions: &mut ConditionStore,
        items: &ItemStore,
        physics: &PhysicsWorld,
        parts: &PartAttachments,
    ) -> bool {
        let (item, equipped) = match owner {
            ToolOwner::Equipped(item) => (item, true),
            ToolOwner::Carried(item) => (item, false),
        };
        let reserved = self.reserves_lmb(equipped.then_some(item), (!equipped).then_some(item));
        if !frame.active || !frame.dt.is_finite() || frame.dt <= 0. {
            return reserved;
        }
        let Some(&index) = self.index.get(&item) else {
            return reserved;
        };
        if matches!(self.tools[index].body, ToolBody::Brush(_)) {
            let tool = Some(index);
            let hit = self.brush_hit(
                tool, equipped, frame, reference, conditions, items, physics, parts,
            );
            let (c, s) = self.contact_mut(tool);
            c.equipped = equipped;
            c.hit = hit;
            if equipped {
                self.use_brush(tool, frame, reference, conditions);
            } else {
                s.doing_work = false;
            }
        } else {
            self.advance_spray(
                index, equipped, frame, reference, conditions, items, physics, parts,
            );
        }
        reserved
    }
    #[allow(clippy::too_many_arguments)]
    fn brush_hit(
        &self,
        tool: Option<usize>,
        equipped: bool,
        frame: ToolFrame,
        reference: &ReferenceManifest,
        conditions: &ConditionStore,
        items: &ItemStore,
        physics: &PhysicsWorld,
        parts: &PartAttachments,
    ) -> Option<(
        ColliderHandle,
        rapier3d::parry::query::RayIntersection,
        Target,
    )> {
        let view = to_rapier(frame.view);
        let ray = Ray::new(view.translation, view.rotation * -Vector::Z);
        let b = self.brush_params(tool, reference);
        let own_body = tool
            .and_then(|i| items.get(self.tools[i].state.item))
            .and_then(|i| i.body);
        let filter = |_: ColliderHandle, c: &rapier3d::prelude::Collider| {
            c.is_enabled()
                && (own_body.is_none() || c.parent() != own_body)
                && c.collision_groups().memberships.bits() & b.mask != 0
                && !c.shape().contains_point(c.position(), ray.origin)
        };
        let hit = if equipped && frame.lmb {
            physics
                .queries(QueryFilter::default().predicate(&filter))
                .cast_ray_and_get_normal(&ray, b.distance, true)
        } else {
            None
        };
        let mut selected = hit.and_then(|(handle, _)| {
            self.target(
                handle,
                ColliderId((physics.rapier().colliders[handle].user_data >> 64) as u32),
                b.cleaning_type,
                reference,
                conditions,
                items,
                parts,
            )
        });
        if let Some((_, hit)) = hit
            && !matches!(selected, Some(Target::Wash(_) | Target::Generated(_)))
            && !self.brush_disabled(tool, reference)
            && let Some(wash) = self.nearby_wash(
                ray.point_at(hit.time_of_impact),
                b.wash_mask,
                b.wash_radius,
                conditions,
                physics,
            )
        {
            selected = Some(wash);
        }
        hit.map(|(handle, hit)| (handle, hit, selected.unwrap_or(Target::Solid(handle))))
    }
    fn use_brush(
        &mut self,
        tool: Option<usize>,
        frame: ToolFrame,
        reference: &ReferenceManifest,
        conditions: &mut ConditionStore,
    ) {
        let (c, s) = self.contact(tool);
        let Some((handle, _, target)) = c.hit else {
            return;
        };
        let b = self.brush_params(tool, reference);
        let disabled = self.brush_disabled(tool, reference);
        // Use reads the retained force before objSlideOnColliderScript.Update samples this frame.
        if !(c.origin == frame.origin
            && c.sampled
            && c.target == Some(target)
            && c.collider == Some(handle)
            && s.force > b.min_force
            && !disabled)
        {
            return;
        }
        let gain = b.repair_modifier * reference.conditions.repair_rate * frame.dt;
        let kind = b.cleaning_type;
        let mut doing_work = None;
        let mut completed = false;
        match target {
            Target::Surface(id) => {
                doing_work = Some(conditions.can_repair(id, kind));
                completed = conditions.repair(id, kind, gain) != RepairResult::None;
            }
            Target::Wash(id) => {
                if let Some(clean) = self.wash_fixed(id, kind, gain, reference, conditions) {
                    doing_work = Some(true);
                    completed = clean;
                }
            }
            Target::Generated(owner) => {
                if let Some(clean) = self.wash_generated(owner, kind, gain, reference) {
                    doing_work = Some(true);
                    completed = clean;
                }
            }
            Target::Solid(_) => {}
        }
        if let Some(work) = doing_work {
            self.contact_mut(tool).1.doing_work = work;
        }
        if completed
            && b.can_ruin
            && let Some(i) = tool
            && let Some(own) = self.tools[i].own_condition
        {
            self.tools[i].state.disabled = conditions.wear(own);
            self.mark_changed(i);
        }
    }
    fn finish_brush(
        &mut self,
        tool: Option<usize>,
        frame: ToolFrame,
        reference: &ReferenceManifest,
        items: &ItemStore,
        physics: &PhysicsWorld,
    ) {
        let b = self.brush_params(tool, reference);
        let disabled = self.brush_disabled(tool, reference);
        let root = match tool {
            Some(i) => items
                .get(self.tools[i].state.item)
                .map(|i| to_rapier(i.pose))
                .unwrap_or(self.contact(tool).0.model),
            None => self.player.pose,
        };
        let (c, s) = self.contact_mut(tool);
        let equipped = c.equipped;
        let hit = c.hit;
        let view = to_rapier(frame.view);
        let ray = Ray::new(view.translation, view.rotation * -Vector::Z);
        // The player brush works while its own input is held.
        let using = equipped && (frame.lmb || tool.is_none());
        s.emission = [0.; 2];
        if !c.motion_ready {
            c.model = root * c.default;
            c.origin = frame.origin;
        }
        if c.origin != frame.origin {
            c.sampled = false;
            c.target = None;
            c.model.translation -= Vector::from_array(std::array::from_fn(|j| {
                (frame.origin[j] - c.origin[j]) as f32
            }));
            c.origin = frame.origin;
        }
        if let Some((handle, hit, target)) = hit {
            let target_position = physics.rapier().colliders[handle].position().translation;
            let point = ray.point_at(hit.time_of_impact) + hit.normal * b.contact_offset;
            let forward = -hit.normal;
            let head_forward = view.rotation * -Vector::Z;
            let horizontal = Vector::new(head_forward.x, 0., head_forward.z);
            let up = horizontal.normalize_or_zero();
            let right = forward.cross(up).normalize_or_zero();
            let up = right.cross(forward).normalize_or_zero();
            let rotation = if right.length_squared() > 0. {
                Rotation::from_mat3(&rapier3d::math::Matrix::from_cols(right, up, -forward))
            } else {
                Rotation::from_rotation_arc(-Vector::Z, forward)
            };
            let reset = !c.sampled || c.target != Some(target) || c.collider != Some(handle);
            if reset {
                s.force = 0.;
            }
            c.model.translation = c
                .model
                .translation
                .lerp(point, (b.position_lerp * frame.dt).clamp(0., 1.));
            c.model.rotation = c
                .model
                .rotation
                .slerp(rotation, (b.rotation_lerp * frame.dt).clamp(0., 1.));
            let relative = if c.has_model {
                c.model.translation
            } else {
                root.translation
            } - target_position;
            let ray_relative = point - target_position;
            // Port invariant: model settling cannot count as rubbing on a stationary contact ray.
            let force = if reset || ray_relative == c.ray_relative {
                0.
            } else {
                ((relative - c.relative).length() - b.minimum_displacement)
                    / (b.maximum_displacement - b.minimum_displacement)
            }
            .clamp(0., 1.);
            c.target = Some(target);
            c.collider = Some(handle);
            c.relative = relative;
            c.ray_relative = ray_relative;
            c.sampled = true;
            s.force = force;
            s.playing = b.has_sound && (b.sound_when_disabled || !disabled);
            if s.playing {
                s.voice_gain += (force * b.max_volume - s.voice_gain)
                    * (b.volume_lerp * frame.dt).clamp(0., 1.);
            } else {
                s.voice_gain = 0.;
            }
            if !disabled {
                for (pi, p) in b.particles.iter().enumerate() {
                    let first = p.role.ends_with("-particle");
                    s.emission[pi] =
                        if (first && (!s.doing_work || b.both_work)) || (!first && s.doing_work) {
                            force
                        } else {
                            0.
                        };
                }
            }
        } else {
            c.target = None;
            c.collider = None;
            c.sampled = false;
            s.force = 0.;
            let desired = if using {
                Pose::from_parts(
                    view.translation + view.rotation * Vector::new(0., -0.25, -0.75),
                    root.rotation * c.equip.rotation,
                )
            } else {
                Pose::from_parts(
                    (root * c.equip).translation,
                    (root * if equipped { c.equip } else { c.default }).rotation,
                )
            };
            c.model.translation = c.model.translation.lerp(
                desired.translation,
                (b.position_lerp * frame.dt).clamp(0., 1.),
            );
            c.model.rotation = c
                .model
                .rotation
                .slerp(desired.rotation, (b.rotation_lerp * frame.dt).clamp(0., 1.));
            if using {
                s.voice_gain += (0. - s.voice_gain) * (b.volume_lerp * frame.dt).clamp(0., 1.);
            } else {
                s.playing = false;
                s.voice_gain = 0.;
            }
        }
        c.motion_ready = true;
        s.pose = from_rapier(&c.model);
    }
    pub fn finish_frame(
        &mut self,
        frame: ToolFrame,
        reference: &ReferenceManifest,
        _conditions: &mut ConditionStore,
        items: &ItemStore,
        physics: &PhysicsWorld,
        _parts: &PartAttachments,
    ) {
        if !frame.active || !frame.dt.is_finite() || frame.dt <= 0. {
            return;
        }
        // Item brushes, then the player brush.
        for i in 0..=self.tools.len() {
            let tool = (i < self.tools.len()).then_some(i);
            if tool.is_some_and(|i| !matches!(self.tools[i].body, ToolBody::Brush(_))) {
                continue;
            }
            self.finish_brush(tool, frame, reference, items, physics);
            let (c, s) = self.contact_mut(tool);
            for (pi, p) in c.particles.iter_mut().enumerate() {
                p.rotation = c.model.rotation;
                p.set_origin(frame.origin);
                p.advance(
                    frame.dt,
                    std::array::from_fn(|j| frame.origin[j] + f64::from(c.model.translation[j])),
                    s.emission[pi],
                    physics,
                );
            }
        }
        self.finish_sprays(frame, reference, items, physics);
        for effect in &mut self.washes {
            if let Some(elapsed) = effect.elapsed.as_mut() {
                for (index, p) in effect.particles.iter_mut().enumerate() {
                    p.set_origin(frame.origin);
                    p.advance(
                        frame.dt,
                        effect.emitter,
                        if *elapsed < effect.durations[index] {
                            1.
                        } else {
                            0.
                        },
                        physics,
                    );
                }
                *elapsed += frame.dt;
            }
        }
    }
    #[allow(clippy::too_many_arguments)]
    fn advance_spray(
        &mut self,
        index: usize,
        equipped: bool,
        frame: ToolFrame,
        reference: &ReferenceManifest,
        conditions: &mut ConditionStore,
        items: &ItemStore,
        physics: &PhysicsWorld,
        parts: &PartAttachments,
    ) {
        let b = self.spray_params(index, reference);
        let item = self.tools[index].state.item;
        let root = to_rapier(items.get(item).expect("checked spray owner").pose);
        let (nozzle, emitter, local_rotation) = match &self.tools[index].body {
            ToolBody::Spray(s) => (root * s.nozzle, root * s.emitter, s.rotation),
            ToolBody::Brush(_) => unreachable!("spray of a brush"),
        };
        let firing = frame.lmb;
        let color = self.tools[index].state.color.expect("spray color");
        let mut rotation = root.rotation * local_rotation;
        let mut painted = false;
        if firing {
            let state = &mut self.tools[index].state;
            state.valve += (1. - state.valve) * (b.valve_speed * frame.dt).clamp(0., 1.);
            let valve = state.valve;
            let quantity = state.quantity.as_mut().expect("spray quantity");
            if *quantity > 0. {
                // Fire checks ammo, consumes it, then paints even on the last shot.
                *quantity -= (b.paint_loss * valve * frame.dt).clamp(0., *quantity);
                self.mark_changed(index);
                rotation = nozzle.rotation;
                let direction = if equipped {
                    to_rapier(frame.view)
                } else {
                    nozzle
                };
                let ray = Ray::new(direction.translation, direction.rotation * -Vector::Z);
                let own = items.get(item).and_then(|i| i.body);
                let filter = |_: ColliderHandle, c: &rapier3d::prelude::Collider| {
                    c.is_enabled()
                        && (own.is_none() || c.parent() != own)
                        && c.collision_groups().memberships.bits() & b.mask != 0
                        && !c.shape().contains_point(c.position(), ray.origin)
                };
                if let Some((handle, hit)) = physics
                    .queries(QueryFilter::default().predicate(&filter))
                    .cast_ray_and_get_normal(&ray, b.distance, true)
                {
                    let id =
                        ColliderId((physics.rapier().colliders[handle].user_data >> 64) as u32);
                    if let Some(target) = self.paint_target(handle, id, items, parts, reference) {
                        let previous = conditions.get(target).color;
                        let falloff =
                            ((hit.time_of_impact - b.distance) / (1. - b.distance)).clamp(0., 1.);
                        let factor = (b.color_lerp * frame.dt * falloff).clamp(0., 1.);
                        painted = conditions.paint(
                            target,
                            std::array::from_fn(|j| {
                                previous[j] + (color[j] - previous[j]) * factor
                            }),
                        );
                    }
                    let forward = (ray.point_at(hit.time_of_impact) - emitter).normalize_or_zero();
                    let right = forward
                        .cross(nozzle.rotation * Vector::Y)
                        .normalize_or_zero();
                    if right.length_squared() > 0. {
                        rotation = Rotation::from_mat3(&rapier3d::math::Matrix::from_cols(
                            right,
                            right.cross(forward),
                            -forward,
                        ));
                    }
                }
            }
        }
        let ToolBody::Spray(s) = &mut self.tools[index].body else {
            unreachable!("spray of a brush");
        };
        s.owned = true;
        s.firing |= firing;
        s.rotation = root.rotation.inverse() * rotation;
        s.painted |= painted;
    }
    fn finish_sprays(
        &mut self,
        frame: ToolFrame,
        reference: &ReferenceManifest,
        items: &ItemStore,
        physics: &PhysicsWorld,
    ) {
        for index in 0..self.tools.len() {
            if !matches!(self.tools[index].body, ToolBody::Spray(_)) {
                continue;
            }
            let b = self.spray_params(index, reference);
            let t = &mut self.tools[index];
            let ToolBody::Spray(spray) = &mut t.body else {
                unreachable!("checked spray");
            };
            let root = to_rapier(items.get(t.state.item).expect("checked spray owner").pose);
            let emitter = root * spray.emitter;
            let rotation = root.rotation * spray.rotation;
            let state = &mut t.state;
            let color = state.color.expect("spray color");
            if !spray.owned {
                state.valve = 0.;
            }
            let signal = &mut t.signal;
            // Update reads the open valve before it performs release interpolation.
            let emitting = state.quantity.unwrap() > 0. && state.valve > 0.;
            signal.pose = from_rapier(&Pose::from_parts(emitter, rotation));
            signal.color = color;
            signal.doing_work = spray.painted;
            // A fixed spray plays its voice. A loot spray plays its prototype `sound` loop.
            signal.playing = emitting;
            signal.voice_gain = if emitting {
                state.valve * b.max_volume
            } else {
                0.
            };
            signal.emission = [if emitting { state.valve } else { 0. }, 0.];
            for particle in &mut spray.particles {
                particle.rotation = root.rotation;
                particle.emission_rotation = Some(rotation);
                particle.set_origin(frame.origin);
                particle.start_color = Some(color);
                particle.advance(
                    frame.dt,
                    std::array::from_fn(|j| frame.origin[j] + f64::from(root.translation[j])),
                    signal.emission[0],
                    physics,
                );
            }
            if !spray.firing {
                state.valve += (0. - state.valve) * (b.valve_speed * frame.dt).clamp(0., 1.);
            }
        }
    }
    pub fn get(&self, item: ItemId) -> Option<&ToolState> {
        self.index.get(&item).map(|&i| &self.tools[i].state)
    }
    #[cfg(test)]
    pub fn get_mut(&mut self, item: ItemId) -> Option<&mut ToolState> {
        self.index.get(&item).map(|&i| &mut self.tools[i].state)
    }
    /// The garage tools in reference order, with their save source.
    fn fixed<'a>(
        &'a self,
        reference: &'a ReferenceManifest,
    ) -> impl Iterator<Item = (usize, &'a SourceId)> + 'a {
        self.tools.iter().enumerate().filter_map(|(i, t)| {
            let ToolParams::Fixed(r) = t.params else {
                return None;
            };
            Some((
                i,
                match t.body {
                    ToolBody::Brush(_) => &reference.conditions.brushes[r].tool.source,
                    ToolBody::Spray(_) => &reference.conditions.sprays[r].tool.source,
                },
            ))
        })
    }
    pub fn capture_save(
        &self,
        reference: &ReferenceManifest,
    ) -> Result<Vec<ToolSnapshot>, SaveError> {
        Ok(self
            .fixed(reference)
            .map(|(i, source)| {
                let t = &self.tools[i].state;
                ToolSnapshot {
                    source: SourceKey::from(source),
                    quantity: t.quantity,
                    color: t.color,
                    disabled: t.disabled,
                }
            })
            .collect())
    }
    pub fn restore_save(
        &mut self,
        reference: &ReferenceManifest,
        saved: &[ToolSnapshot],
    ) -> Result<(), SaveError> {
        validate_tools(reference, saved)?;
        self.reset_contact();
        for t in &mut self.tools {
            if let ToolBody::Brush(c) = &mut t.body {
                c.motion_ready = false;
            }
        }
        self.player.contact.motion_ready = false;
        let restored: Vec<(usize, &ToolSnapshot)> = self
            .fixed(reference)
            .map(|(i, source)| {
                let key = SourceKey::from(source);
                (i, saved.iter().find(|v| v.source == key).unwrap())
            })
            .collect();
        for (i, v) in restored {
            let t = &mut self.tools[i];
            t.state.quantity = v.quantity;
            t.state.color = v.color;
            t.state.disabled = v.disabled;
            t.state.valve = 0.;
            if let Some(color) = v.color {
                t.signal.color = color;
            }
        }
        Ok(())
    }
}
/// The hit trigger wins, then a nearby trigger, then the hit part or condition.
fn spray_order(hit: Option<Target>, nearby: impl FnOnce() -> Option<Target>) -> Option<Target> {
    if matches!(hit, Some(Target::Wash(_) | Target::Generated(_))) {
        return hit;
    }
    nearby().or(hit)
}
fn reset_brush(c: &mut Contact) {
    c.equipped = false;
    c.hit = None;
    c.target = None;
    c.collider = None;
    c.sampled = false;
    for p in &mut c.particles {
        p.reset_emitter();
    }
}
fn reset_signal(s: &mut ToolSignals) {
    s.force = 0.;
    s.doing_work = false;
    s.playing = false;
    s.voice_gain = 0.;
    s.emission = [0.; 2];
}
/// A brush state has no paint. A spray state has paint in range and a finite color.
pub fn valid_tool_state(
    spray: Option<&SprayTool>,
    quantity: Option<f32>,
    color: Option<[f32; 4]>,
    disabled: bool,
) -> bool {
    match spray {
        None => quantity.is_none() && color.is_none(),
        Some(t) => {
            !disabled
                && quantity
                    .is_some_and(|q| q.is_finite() && (0. ..=t.maximum_quantity).contains(&q))
                && color.is_some_and(|c| c.iter().all(|v| v.is_finite()))
        }
    }
}
pub fn validate_tools(
    reference: &ReferenceManifest,
    saved: &[ToolSnapshot],
) -> Result<(), SaveError> {
    let c = &reference.conditions;
    if saved.len() != c.brushes.len() + c.sprays.len() {
        return Err(SaveError::CorruptData("Invalid tool state or owner".into()));
    }
    let brushes = c.brushes.iter().map(|b| (&b.tool.source, None));
    for (source, spray) in brushes.chain(c.sprays.iter().map(|s| (&s.tool.source, Some(&s.tool)))) {
        let mut matches = saved.iter().filter(|v| v.source == SourceKey::from(source));
        let valid = matches
            .next()
            .is_some_and(|v| valid_tool_state(spray, v.quantity, v.color, v.disabled))
            && matches.next().is_none();
        if !valid {
            return Err(SaveError::CorruptData("Invalid tool state or owner".into()));
        }
    }
    Ok(())
}
