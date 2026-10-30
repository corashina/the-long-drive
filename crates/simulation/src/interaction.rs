pub mod binoculars;
pub mod buried;
pub mod carry;
pub mod cigarettes;
pub mod cleaning;
pub mod computer;
pub mod consumption;
pub mod crosshair;
pub mod decals;
pub mod equipment;
pub mod equipment_input;
pub mod explosions;
pub mod feedback;
pub mod fluid_contact;
mod fluid_receipts;
pub mod genie_lamp;
pub mod guns;
pub mod help_texts;
pub mod hitch;
pub mod holes;
mod hose;
mod input;
pub use input::InteractionOutcome;
pub mod items;
pub mod labels;
pub mod level;
pub mod mechanisms;
pub mod melee;
pub mod memory;
pub mod parts;
pub mod push;
pub mod spray;
pub mod storage;
pub mod targets;
pub mod wear;
mod wheel_actions;
pub mod wheels;
use feedback::ActionMask;
pub use targets::{InteractionBlock, LightAim, ResolvedAim};
pub use wheel_actions::WheelAction;

use crate::fluids::{spill, transfer};
use crate::player::PlayerState;
use crate::reference::{
    ActionBinding, ActionMasks, BodyId, ColliderGeometry, ColliderId, FluidParticleReference,
    FluidPresentation, ItemId, ItemKind, LightControlId, LightControllerKind, NodeId, OpeningId,
    OpeningRecord, PartSlotId, ReferenceError, ReferenceManifest, TankRole,
};
use crate::simulation::{FrameInput, keys};
use crate::survival::NeedsOutput;
use crate::vehicle::{PhysicsWorld, VehiclePose, to_rapier};
use items::{ItemLocation, ItemStore};
use rapier3d::math::{Pose, Rotation, Vector};
use rapier3d::prelude::{ColliderHandle, RigidBodyHandle};
use std::collections::{HashMap, HashSet};
use wheels::WheelSlotId;

pub use crate::vehicle::SLOT_SENSOR_FLAG;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct AimTarget {
    pub collider: ColliderHandle,
    pub item: ItemId,
    pub distance: f32,
    pub point: [f32; 3],
    pub slot: Option<SlotTarget>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SlotTarget {
    Wheel(WheelSlotId),
    Tire(ItemId),
    Part(PartSlotId),
    Hitch(hitch::HitchTarget),
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum InteractionAction {
    LightControl(LightControlId),
    Pickup(ItemId),
    Drop(ItemId),
    Throw(ItemId),
    Push(ItemId),
    Cap(OpeningId),
    Mechanism(ItemId),
    /// Enter or exit a seat.
    Seat,
    Fill {
        source: ItemId,
        destination: ItemId,
        liters: f32,
    },
    Consume(ItemId),
    Drink {
        source: ItemId,
        requested_liters: f32,
    },
    DrinkContact {
        source: ItemId,
        poured_liters: f32,
        fractions: [f32; 6],
    },
    Wheel(WheelAction),
    PartRemove(ItemId),
    PartInstall(ItemId),
    /// A held trailer grip drops onto a hitch target.
    HitchAttach(ItemId),
    /// One source tune step (+1 or -1) on a generated dial or radio knob, or 0 for a
    /// radio band switch.
    DialTune(ColliderId, f32),
    /// Look input (source X right, Y up) on a generated music box crank while used.
    Crank(ColliderId, [f32; 2]),
}

/// A generated usablescript knob bound to a dial or a portable radio.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Knob {
    Dial,
    Volume,
    Tuning,
    /// usablescript turned with use: radio BChangeAMFM, metal detector onOffButton.
    Switch,
    /// musicboxscript use: rotateAble, turned with the mouse.
    Crank,
}

#[derive(Clone, Copy, Debug)]
pub enum InteractionSound {
    Action(InteractionAction),
    Cap {
        id: OpeningId,
        previous: f32,
        current: f32,
        tune: Option<f32>,
    },
}

struct OpeningState {
    flow: Option<(bool, f32)>,
    fill_called: bool,
    empty_pour: bool,
    reference: OpeningRules,
    local_pose: Pose,
    tilt_axis: Vector,
    valve: f32,
    particles: Option<FluidParticleReference>,
    contact: Option<fluid_contact::FluidContact>,
    /// Fluid owner when it differs from `reference.tank`, the pose owner (hose ends).
    borrowed_tank: Option<ItemId>,
}
impl OpeningState {
    fn fluid_owner(&self) -> ItemId {
        self.borrowed_tank.unwrap_or(self.reference.tank)
    }
}
struct OpeningRules {
    id: OpeningId,
    tank: ItemId,
    cap: ItemId,
    flow_liters_per_second: f32,
    flow_radius: f32,
    tilt_degrees: f32,
    angle_disabled: bool,
    spill_disabled: bool,
    tube_connected: bool,
    cant_tube: bool,
    can_toggle: bool,
    can_tune: bool,
    tune_rate: f32,
    /// `UiReference::sprites` index of the cap `img`.
    tune_icon: Option<u32>,
    presentation: FluidPresentation,
}
impl OpeningRules {
    fn new(opening: &OpeningRecord, ui: &crate::reference::UiReference) -> Self {
        Self {
            id: opening.id,
            tank: opening.tank,
            cap: opening.cap,
            flow_liters_per_second: opening.flow_liters_per_second,
            flow_radius: opening.flow_radius,
            tilt_degrees: opening.tilt_degrees,
            angle_disabled: opening.angle_disabled,
            spill_disabled: opening.spill_disabled,
            tube_connected: opening.tube_connected,
            cant_tube: opening.cant_tube,
            can_toggle: opening.can_toggle,
            can_tune: opening.can_tune,
            tune_rate: opening.tune_rate,
            tune_icon: opening.tune_icon.as_ref().and_then(|icon| ui.sprite(icon)),
            presentation: opening.presentation.clone(),
        }
    }
}
#[derive(Clone, Copy)]
struct SeatPress {
    usable: Option<ItemId>,
    duration: f32,
}
impl SeatPress {
    fn matches(self, usable: Option<ItemId>) -> bool {
        self.usable == usable
            && (usable.is_none() || crate::player::source_short_hold(self.duration, 0.2))
    }
}
/// Install rules and identity of one wheel slot.
struct WheelSlotRule {
    /// The car root item.
    owner: Option<ItemId>,
    source: crate::reference::SourceId,
    rim_main_types: Vec<String>,
    rim_types: Vec<String>,
    fits: HashSet<ItemId>,
}

pub struct InteractionState {
    /// Garage surfaces and world part conditions.
    pub conditions: crate::conditions::ConditionStore,
    pub cleaning: cleaning::CleaningTools,
    pub binoculars: binoculars::Binoculars,
    pub buried: buried::BuriedItems,
    pub melee: melee::MeleeState,
    pub guns: guns::GunState,
    pub explosions: explosions::ExplosionState,
    pub holes: holes::Holes,
    pub decals: decals::DecalStore,
    sounds: [Option<InteractionSound>; 64],
    sound_count: usize,
    next_generated_collider: u32,
    next_generated_light: u32,
    light_controls: HashMap<ColliderId, LightControlId>,
    held_controls: HashMap<ItemId, LightControlId>,
    pub storage: storage::StorageLocks,
    light_nodes: HashMap<ColliderId, NodeId>,
    seat_press: Option<SeatPress>,
    seat_action_latched: bool,
    cancelled_input: u32,
    usable_light_controls: HashSet<LightControlId>,
    /// Generated source dial knobs (meterscript tune usables).
    dial_knobs: HashMap<ColliderId, Knob>,
    tune_icons: HashMap<ColliderId, u32>,
    fill_opening: Option<OpeningId>,
    kinds: HashMap<ItemId, ItemKind>,
    wheel_slots: HashMap<WheelSlotId, WheelSlotRule>,
    tire_fits: HashSet<(ItemId, ItemId)>,
    f_gesture_consumed: bool,
    source_bindings: HashMap<ColliderId, ActionBinding>,
    pickup_layers: HashMap<ItemId, u32>,
    removal_layers: HashMap<ItemId, u32>,
    usable_items: HashSet<ItemId>,
    filling_items: HashSet<ItemId>,
    collider_bindings: HashMap<ColliderHandle, ActionBinding>,
    bodies: HashMap<BodyId, RigidBodyHandle>,
    pub mechanisms: mechanisms::MechanismState,
    pub parts: parts::PartAttachments,
    pub hitch: hitch::Hitches,
    pub help_texts: help_texts::HelpTexts,
    mechanism_bodies: HashMap<ItemId, BodyId>,
    openings: Vec<OpeningState>,
    hoses: hose::HoseState,
    receipt_tanks: Vec<fluid_receipts::ReceiptTank>,
    callback_nodes: Vec<NodeId>,
    /// Hit colliders of world item seats.
    item_seats:
        HashMap<ColliderHandle, std::sync::Arc<crate::world::stream::content_seats::ItemSeat>>,
    pub carry: carry::CarryState,
    pub equipment: equipment::EquipmentState,
    pub equipment_input: equipment_input::EquipmentInput,
    push: push::PushState,
    player_body: Option<RigidBodyHandle>,
    reach: f32,
    action_masks: ActionMasks,
    /// Source `FMouseSensZ`.
    scroll_sensitivity: f32,
    supplies: HashSet<ItemId>,
    source_supply_emission: HashMap<ItemId, [f32; 2]>,
    tank_rules: HashMap<ItemId, TankRules>,
    slow_oil_modifier: f32,
    drink_flow_liters_per_second: f32,
    drink_contact_radius: f32,
    drink_collider_radius: f32,
    drink_collider_center: Vector,
    pending_drink_contacts: Vec<(ItemId, f32, [f32; 6])>,
    latched_mechanism: Option<(ItemId, BodyId, Vector, ColliderHandle)>,
    use_consumed: bool,
    mechanism_release_distance_squared: f32,
    tuning_opening: Option<OpeningId>,
}

#[derive(Clone, Copy)]
struct TankRules {
    no_drink: bool,
    no_fill_this: bool,
    no_fill_from: bool,
    slow_oil: bool,
    explosion: crate::world::TankExplosion,
}

impl InteractionState {
    pub fn set_settings(&mut self, settings: crate::reference::PlayerSettings) {
        self.scroll_sensitivity = settings.scroll;
        self.mechanisms.scroll_sensitivity = settings.scroll;
        self.carry.settings = settings;
    }
    pub fn restore_save(
        &mut self,
        saved: &crate::save::MechanismsSnapshot,
        ids: &crate::save::IdentityMap,
    ) {
        self.mechanisms.restore_save(ids, &saved.controls);
        for record in &saved.openings {
            self.openings
                .iter_mut()
                .find(|o| o.reference.id == ids.openings[&record.source])
                .unwrap()
                .valve = record.valve;
        }
        self.cancel_input();
    }
    pub fn restore_carry_save(
        &mut self,
        saved: &crate::save::CarrySnapshot,
        ids: &crate::save::IdentityMap,
        items: &ItemStore,
        physics: &mut PhysicsWorld,
    ) -> Result<(), crate::save::SaveError> {
        self.carry.restore_save(saved, ids, items, physics)
    }
    pub fn capture_save(
        &self,
        ids: &crate::save::IdentityMap,
    ) -> Result<crate::save::MechanismsSnapshot, crate::save::SaveError> {
        let mut openings = ids
            .openings
            .iter()
            .map(|(source, id)| {
                let opening = self
                    .openings
                    .iter()
                    .find(|o| o.reference.id == *id)
                    .ok_or_else(|| {
                        crate::save::SaveError::InvalidLinks("Missing source opening".into())
                    })?;
                Ok(crate::save::OpeningSnapshot {
                    source: source.clone(),
                    valve: opening.valve,
                })
            })
            .collect::<Result<Vec<_>, crate::save::SaveError>>()?;
        openings.sort_by(|a, b| a.source.cmp(&b.source));
        Ok(crate::save::MechanismsSnapshot {
            controls: self.mechanisms.capture_save(ids)?,
            openings,
        })
    }
    pub fn capture_carry_save(
        &self,
        ids: &crate::save::IdentityMap,
    ) -> Result<Option<crate::save::CarrySnapshot>, crate::save::SaveError> {
        self.carry.capture_save(ids)
    }
    pub fn retained_binding_bytes(&self) -> usize {
        use memory::{map, set, vector};
        map(&self.light_controls)
            + self.binoculars.heap_bytes()
            + map(&self.light_nodes)
            + set(&self.usable_light_controls)
            + map(&self.dial_knobs)
            + map(&self.tune_icons)
            + map(&self.item_seats)
            + map(&self.kinds)
            + map(&self.wheel_slots)
            + self
                .wheel_slots
                .values()
                .map(|r| {
                    set(&r.fits)
                        + r.source.file.capacity()
                        + memory::strings(&r.rim_main_types)
                        + memory::strings(&r.rim_types)
                })
                .sum::<usize>()
            + set(&self.tire_fits)
            + map(&self.pickup_layers)
            + map(&self.removal_layers)
            + set(&self.usable_items)
            + set(&self.filling_items)
            + map(&self.collider_bindings)
            + map(&self.mechanism_bodies)
            + vector(&self.openings)
            + vector(&self.receipt_tanks)
            + self
                .receipt_tanks
                .iter()
                .map(|tank| tank.retained_cap_bytes())
                .sum::<usize>()
            + vector(&self.callback_nodes)
            + set(&self.supplies)
            + map(&self.source_supply_emission)
            + map(&self.tank_rules)
            + self.carry.retained_binding_bytes()
            + self.parts.retained_binding_bytes()
            + self.mechanisms.retained_binding_bytes()
    }

    pub fn new(reference: &ReferenceManifest) -> Result<Self, ReferenceError> {
        let mut mechanisms = mechanisms::MechanismState::new(reference)?;
        let parts = parts::PartAttachments::new(reference)?;
        parts.mount_initial(&mut mechanisms);
        let mut openings = Vec::with_capacity(reference.openings.len());
        for opening in &reference.openings {
            let anchor = reference
                .anchors
                .iter()
                .find(|anchor| anchor.id == opening.anchor)
                .ok_or_else(|| ReferenceError("Missing opening anchor".into()))?;
            let item = reference
                .items
                .iter()
                .find(|item| item.id == opening.tank)
                .ok_or_else(|| ReferenceError("Missing opening tank".into()))?;
            let body = reference
                .bodies
                .iter()
                .find(|body| body.id == item.body)
                .ok_or_else(|| ReferenceError("Missing tank body".into()))?;
            let item_pose = to_rapier(items::node_pose(reference, item.node)?);
            let local_pose = item_pose.inverse()
                * transform_pose(&body.pose)
                * transform_pose(&anchor.transform);
            openings.push(OpeningState {
                reference: OpeningRules::new(opening, &reference.ui),
                local_pose,
                tilt_axis: Vector::from_array(opening.tilt_axis),
                valve: opening.valve,
                flow: None,
                fill_called: false,
                empty_pour: false,
                particles: Some(opening.particles.clone()),
                contact: Some(fluid_contact::FluidContact::new(
                    &opening.particles,
                    reference
                        .particles
                        .iter()
                        .find(|p| p.role == format!("outlet_{}", opening.id.0))
                        .unwrap(),
                    reference
                        .particles
                        .iter()
                        .find(|p| p.role == format!("splash_{}", opening.id.0))
                        .unwrap(),
                    opening.id.0,
                )),
                borrowed_tank: None,
            });
        }
        let mut pickup_layers = HashMap::new();
        for binding in &reference.action_bindings {
            if let Some(owner) = binding.pickup_owner {
                let collider = reference
                    .colliders
                    .iter()
                    .find(|collider| collider.id == binding.collider)
                    .ok_or_else(|| ReferenceError("Missing action collider".into()))?;
                *pickup_layers.entry(owner).or_insert(0) |= collider.membership;
            }
        }
        let usable_items = reference
            .openings
            .iter()
            .filter(|opening| opening.can_toggle || opening.can_tune)
            .map(|opening| opening.cap)
            .collect();
        let filling_items = reference
            .openings
            .iter()
            .map(|opening| opening.cap)
            .collect();
        let mut removal_layers = HashMap::new();
        for binding in &reference.action_bindings {
            if let Some(owner) = binding.removal_owner {
                let collider = reference
                    .colliders
                    .iter()
                    .find(|collider| collider.id == binding.collider)
                    .ok_or_else(|| ReferenceError("Missing action collider".into()))?;
                *removal_layers.entry(owner).or_insert(0) |= collider.membership;
            }
        }
        let is_rim = |id: ItemId| {
            reference
                .items
                .iter()
                .any(|item| item.id == id && matches!(item.kind, ItemKind::Rim | ItemKind::Wheel))
        };
        let mut tire_fits = HashSet::new();
        for rim in &reference.wheel_parts {
            if is_rim(rim.item) {
                for tire in &reference.wheel_parts {
                    if !is_rim(tire.item)
                        && wheels::fits(
                            &rim.tire_main_types,
                            &rim.tire_types,
                            &tire.main_types,
                            &tire.part_types,
                        )
                    {
                        tire_fits.insert((rim.item, tire.item));
                    }
                }
            }
        }
        let mut light_controls = HashMap::new();
        for control in &reference.lighting.controllers {
            for &collider in &control.aim_colliders {
                if let Some(previous) = light_controls.get(&collider) {
                    let previous = reference
                        .lighting
                        .controllers
                        .iter()
                        .find(|c| c.id == *previous)
                        .unwrap();
                    if control.kind != LightControllerKind::Headlight
                        || previous.kind != control.kind
                        || previous.usable_source != control.usable_source
                        || previous.usable_event != control.usable_event
                    {
                        return Err(ReferenceError("Conflicting light control collider".into()));
                    }
                    if previous.id.0 < control.id.0 {
                        continue;
                    }
                }
                light_controls.insert(collider, control.id);
            }
        }
        Ok(Self {
            conditions: crate::conditions::ConditionStore::new(reference)?,
            cleaning: cleaning::CleaningTools::new(reference)?,
            binoculars: binoculars::Binoculars::new(&reference.small_items.binoculars),
            buried: buried::BuriedItems::default(),
            melee: melee::MeleeState::new(reference)?,
            guns: guns::GunState::new(reference)?,
            explosions: explosions::ExplosionState::new(reference)?,
            holes: holes::Holes::new(reference),
            decals: decals::DecalStore::new(reference),
            source_supply_emission: HashMap::new(),
            light_nodes: reference.colliders.iter().map(|c| (c.id, c.node)).collect(),
            light_controls,
            held_controls: reference
                .flashlights
                .instances
                .iter()
                .filter(|f| f.startup_active)
                .map(|f| (f.item, f.controller))
                .collect(),
            storage: storage::StorageLocks::new(reference)?,
            seat_press: None,
            seat_action_latched: false,
            cancelled_input: 0,
            next_generated_light: reference
                .lighting
                .controllers
                .iter()
                .map(|c| c.id.0)
                .max()
                .unwrap_or(0)
                + 1,
            next_generated_collider: reference
                .colliders
                .iter()
                .map(|c| c.id.0)
                .max()
                .unwrap_or(0)
                + 1,
            fill_opening: None,
            dial_knobs: HashMap::new(),
            tune_icons: HashMap::new(),
            usable_light_controls: reference
                .lighting
                .controllers
                .iter()
                .filter(|c| {
                    matches!(
                        c.kind,
                        LightControllerKind::Cabin | LightControllerKind::Headlight
                    )
                })
                .map(|c| c.id)
                .collect(),
            wheel_slots: HashMap::new(),
            tire_fits,
            f_gesture_consumed: false,
            pickup_layers,
            removal_layers,
            usable_items,
            filling_items,
            source_bindings: reference
                .action_bindings
                .iter()
                .map(|binding| (binding.collider, *binding))
                .collect(),
            collider_bindings: HashMap::with_capacity(reference.colliders.len()),
            kinds: reference
                .items
                .iter()
                .map(|item| (item.id, item.kind.clone()))
                .collect(),
            bodies: HashMap::with_capacity(reference.bodies.len()),
            mechanisms,
            parts,
            hitch: hitch::Hitches::default(),
            help_texts: help_texts::HelpTexts::default(),
            mechanism_bodies: reference.items.iter().map(|i| (i.id, i.body)).collect(),
            receipt_tanks: fluid_receipts::tank_nodes(reference),
            callback_nodes: Vec::with_capacity(
                reference
                    .openings
                    .iter()
                    .map(|o| o.particles.capacity as usize)
                    .max()
                    .unwrap_or(0),
            ),
            openings,
            hoses: hose::HoseState::new(
                reference.fluid_rules.min_tube_vertical_height,
                reference.fluid_rules.max_tube_vertical_height,
            ),
            sounds: [None; 64],
            sound_count: 0,
            item_seats: HashMap::new(),
            carry: carry::CarryState::new(reference)?,
            equipment: equipment::EquipmentState::new(reference),
            equipment_input: equipment_input::EquipmentInput::new(
                reference.inventory.equip_hold_seconds,
            ),
            push: push::PushState::new(reference),
            player_body: None,
            reach: reference.player.aim_reach,
            action_masks: reference.player.action_masks,
            scroll_sensitivity: reference.settings.sliders.scroll.value(),
            supplies: reference
                .tanks
                .iter()
                .filter(|tank| tank.role == TankRole::Supply)
                .map(|tank| tank.item)
                .collect(),
            tank_rules: reference
                .tanks
                .iter()
                .map(|tank| {
                    (
                        tank.item,
                        TankRules {
                            no_drink: tank.no_drink,
                            no_fill_this: tank.no_fill_this,
                            no_fill_from: tank.no_fill_from,
                            slow_oil: tank.slow_oil,
                            explosion: tank.explosion,
                        },
                    )
                })
                .collect(),
            slow_oil_modifier: reference.fluid_rules.slow_oil_modifier,
            drink_flow_liters_per_second: reference.fluid_rules.pour_speed
                * reference.survival.drinking.valve
                * reference.survival.drinking.radius.powi(2)
                * std::f32::consts::PI
                * 0.025
                * reference.survival.drink_modifier,
            drink_contact_radius: reference.survival.drinking.radius
                * reference
                    .survival
                    .drinking
                    .particle_contact_radius_multiplier,
            drink_collider_radius: match &reference.survival.drinking.geometry {
                ColliderGeometry::Sphere { radius, .. } => *radius,
                _ => return Err(ReferenceError("Drink collider is not a sphere".into())),
            },
            drink_collider_center: match &reference.survival.drinking.geometry {
                ColliderGeometry::Sphere { center, .. } => {
                    Vector::from_array(reference.survival.drinking.local_position)
                        + Vector::from_array(*center)
                }
                _ => unreachable!(),
            },
            pending_drink_contacts: Vec::with_capacity(reference.openings.len()),
            latched_mechanism: None,
            use_consumed: false,
            mechanism_release_distance_squared: (reference.player.aim_reach * 1.5).powi(2),
            tuning_opening: None,
        })
    }

    pub fn bind_body(&mut self, id: BodyId, handle: RigidBodyHandle) {
        self.bodies.insert(id, handle);
        self.mechanisms.bind_body(id, handle);
    }
    pub fn player_body(&self) -> Option<RigidBodyHandle> {
        self.player_body
    }
    pub fn bind_player_body(&mut self, handle: RigidBodyHandle) {
        self.player_body = Some(handle);
    }
    pub fn pickup_rule(&self, id: ItemId) -> Option<&crate::reference::PickupReference> {
        self.carry.rule(id)
    }
    pub fn reach(&self) -> f32 {
        self.reach
    }
    pub fn held(&self) -> Option<ItemId> {
        self.carry.held()
    }
    /// fpscontroller `noLMBUse` of a carried or equipped tool.
    pub fn reserves_lmb(&self) -> bool {
        let (equipped, carried) = (self.equipment.item(), self.held());
        self.cleaning.reserves_lmb(equipped, carried)
            || self.binoculars.reserves_lmb(equipped, carried)
    }
    pub fn can_equip_carried(
        &self,
        player: &PlayerState,
        items: &ItemStore,
        physics: &PhysicsWorld,
    ) -> bool {
        let Some(id) = self.held() else {
            return false;
        };
        let Some(item) = items.get(id) else {
            return false;
        };
        let Some(body) = item
            .body
            .and_then(|handle| physics.rapier().bodies.get(handle))
        else {
            return false;
        };
        self.equipment.item().is_none()
            && item.location == ItemLocation::Held
            && !item.removed
            && !item.pending_attachment
            && item.inactive_pose.is_none()
            && body.is_enabled()
            && body.is_dynamic()
            && self.carry.body() == item.body
            && self
                .pickup_rule(id)
                .is_some_and(|rule| rule.can_hold_right_hand)
            && self
                .carry
                .allows_pickup(id, player.seated(), items.mounted(id))
            && physics.body_active(item.body.unwrap())
            && physics.vehicle_of(item.body.unwrap()).is_none()
    }
    pub fn owned_light_control(&self, item: ItemId) -> Option<LightControlId> {
        (self.equipment.item() == Some(item) || self.held() == Some(item))
            .then(|| self.held_controls.get(&item).copied())
            .flatten()
    }
    pub fn held_light_control(&self) -> Option<LightControlId> {
        self.equipment
            .item()
            .and_then(|item| self.owned_light_control(item))
            .or_else(|| self.held().and_then(|item| self.owned_light_control(item)))
    }
    pub fn pickup_stored_item(
        &mut self,
        item: ItemId,
        point: [f32; 3],
        player: &PlayerState,
        items: &mut ItemStore,
        physics: &mut PhysicsWorld,
    ) -> Result<ItemId, ReferenceError> {
        let body = self
            .storage
            .locked_body(item)
            .ok_or_else(|| ReferenceError("Item is not stored".into()))?;
        if !self
            .carry
            .allows_pickup(item, player.seated(), items.mounted(item))
        {
            return Err(ReferenceError("Cannot carry this item".into()));
        }
        self.carry.can_acquire(item, point, player, body, physics)?;
        self.storage.unlock(item, items, physics)?;
        self.pickup_item(item, point, player, items, physics)?;
        Ok(item)
    }
    pub fn allocate_generated_light(&mut self) -> Result<LightControlId, ReferenceError> {
        let id = self.next_generated_light;
        self.next_generated_light = id
            .checked_add(1)
            .ok_or_else(|| ReferenceError("Generated light identity exhausted".into()))?;
        Ok(LightControlId(id))
    }
    pub fn bind_generated_light(&mut self, control: LightControlId, collider: ColliderId) {
        self.light_controls.insert(collider, control);
        self.light_nodes.insert(collider, NodeId(collider.0));
        self.usable_light_controls.insert(control);
    }
    /// Portable source flashlights switch while held or equipped.
    pub fn bind_held_light(&mut self, item: ItemId, control: LightControlId) {
        self.held_controls.insert(item, control);
    }
    pub fn remove_generated_light(&mut self, control: LightControlId) {
        self.light_nodes
            .retain(|id, _| self.light_controls.get(id) != Some(&control));
        self.light_controls.retain(|_, id| *id != control);
        self.held_controls.retain(|_, id| *id != control);
        self.usable_light_controls.remove(&control);
    }
    pub fn bind_item_seat(
        &mut self,
        collider: ColliderHandle,
        seat: std::sync::Arc<crate::world::stream::content_seats::ItemSeat>,
    ) {
        self.item_seats.insert(collider, seat);
    }
    pub fn remove_item_seat(&mut self, collider: ColliderHandle) {
        self.item_seats.remove(&collider);
    }
    /// The bound item seat of a world item by its main seatscript. Car seats are not saved here.
    pub fn find_item_seat(
        &self,
        owner: &crate::player::SeatOwner,
        source: &crate::reference::SourceId,
    ) -> Option<std::sync::Arc<crate::world::stream::content_seats::ItemSeat>> {
        self.item_seats
            .values()
            .find(|seat| &seat.owner == owner && &seat.seat.source == source)
            .cloned()
    }
    #[cfg(test)]
    pub fn item_seat(
        &self,
        collider: ColliderHandle,
    ) -> Option<std::sync::Arc<crate::world::stream::content_seats::ItemSeat>> {
        self.item_seats.get(&collider).cloned()
    }
    pub fn bind_dial_knob(&mut self, collider: ColliderId, kind: Knob) {
        self.dial_knobs.insert(collider, kind);
    }
    /// `img` of a `tuneAble` knob as a `UiReference::sprites` index.
    pub fn bind_tune_icon(&mut self, collider: ColliderId, sprite: u32) {
        self.tune_icons.insert(collider, sprite);
    }
    pub fn remove_dial_knob(&mut self, collider: ColliderId) {
        self.dial_knobs.remove(&collider);
        self.tune_icons.remove(&collider);
    }
    pub fn dial_knob(
        &self,
        target: &AimTarget,
        physics: &PhysicsWorld,
    ) -> Option<(ColliderId, Knob)> {
        let id =
            ColliderId((physics.rapier().colliders.get(target.collider)?.user_data >> 64) as u32);
        self.dial_knobs.get(&id).map(|kind| (id, *kind))
    }
    pub fn allocate_generated_collider(&mut self) -> Result<ColliderId, ReferenceError> {
        let id = self.next_generated_collider;
        self.next_generated_collider = id
            .checked_add(1)
            .ok_or_else(|| ReferenceError("Generated collider identity exhausted".into()))?;
        Ok(ColliderId(id))
    }
    pub fn register_generated_part_kind(&mut self, item: ItemId, layers: u32) {
        self.removal_layers.insert(item, layers);
        for binding in self.collider_bindings.values_mut() {
            if binding.pickup_owner == Some(item) {
                binding.removal_owner = Some(item);
            }
        }
    }
    /// Adds the install rules of a world car wheel slot. `owner` is the car root item.
    pub fn register_wheel_slot(
        &mut self,
        id: WheelSlotId,
        owner: ItemId,
        source: crate::reference::SourceId,
        slot: &wheels::WorldWheelSlot,
        rules: &[crate::reference::WheelPartReference],
    ) {
        let fits = rules
            .iter()
            .filter(|rule| {
                self.is_wheel(rule.item)
                    && wheels::fits(
                        &slot.rim_main_types,
                        &slot.rim_types,
                        &rule.main_types,
                        &rule.part_types,
                    )
            })
            .map(|rule| rule.item)
            .collect();
        self.wheel_slots.insert(
            id,
            WheelSlotRule {
                owner: Some(owner),
                source,
                rim_main_types: slot.rim_main_types.clone(),
                rim_types: slot.rim_types.clone(),
                fits,
            },
        );
    }
    /// Removes a world car wheel slot and the binding of its sensor.
    pub fn remove_wheel_slot(&mut self, id: WheelSlotId, sensor: Option<ColliderHandle>) {
        self.wheel_slots.remove(&id);
        if let Some(sensor) = sensor {
            self.collider_bindings.remove(&sensor);
        }
    }
    /// The car root item and the source slot of a wheel slot.
    pub fn wheel_slot_identity(
        &self,
        id: WheelSlotId,
    ) -> Option<(Option<ItemId>, &crate::reference::SourceId)> {
        self.wheel_slots.get(&id).map(|r| (r.owner, &r.source))
    }
    /// The world car wheel slot of car root item `owner` and source slot `source`.
    pub fn resolve_wheel_slot(
        &self,
        owner: Option<ItemId>,
        source: &crate::reference::SourceId,
    ) -> Option<WheelSlotId> {
        let owner = owner?;
        self.wheel_slots
            .iter()
            .find(|(_, r)| r.owner == Some(owner) && &r.source == source)
            .map(|(id, _)| *id)
    }
    pub fn register_generated_wheel(
        &mut self,
        rule: &crate::reference::WheelPartReference,
        rim: bool,
        rules: &[crate::reference::WheelPartReference],
    ) {
        self.kinds
            .insert(rule.item, if rim { ItemKind::Rim } else { ItemKind::Tire });
        for slot in self.wheel_slots.values_mut() {
            if rim
                && wheels::fits(
                    &slot.rim_main_types,
                    &slot.rim_types,
                    &rule.main_types,
                    &rule.part_types,
                )
            {
                slot.fits.insert(rule.item);
            }
        }
        for old in rules {
            if old.item == rule.item {
                continue;
            }
            let old_rim = self
                .kinds
                .get(&old.item)
                .is_some_and(|kind| matches!(kind, ItemKind::Rim | ItemKind::Wheel));
            if rim
                && !old_rim
                && wheels::fits(
                    &rule.tire_main_types,
                    &rule.tire_types,
                    &old.main_types,
                    &old.part_types,
                )
            {
                self.tire_fits.insert((rule.item, old.item));
            }
            if !rim
                && old_rim
                && wheels::fits(
                    &old.tire_main_types,
                    &old.tire_types,
                    &rule.main_types,
                    &rule.part_types,
                )
            {
                self.tire_fits.insert((old.item, rule.item));
            }
        }
    }
    /// Remove only bindings owned by this generated instance.
    pub fn unbind_generated(&mut self, id: ItemId, colliders: &[ColliderHandle]) {
        for collider in colliders {
            if self.collider_bindings.get(collider).is_some_and(|binding| {
                [
                    binding.use_owner,
                    binding.pickup_owner,
                    binding.removal_owner,
                    binding.push_owner,
                ]
                .contains(&Some(id))
            }) {
                self.collider_bindings.remove(collider);
            }
        }
        if let Some(body) = self.mechanism_bodies.remove(&id) {
            self.bodies.remove(&body);
        }
        if let Some(body) = self.push.remove_generated(id) {
            self.bodies.remove(&body);
        }
        if self
            .latched_mechanism
            .is_some_and(|(item, _, _, _)| item == id)
        {
            self.latched_mechanism = None;
        }
        self.carry.unregister_generated(id);
        self.unregister_hose(id);
        for slot in self.wheel_slots.values_mut() {
            slot.fits.remove(&id);
        }
        self.tire_fits
            .retain(|(rim, tire)| *rim != id && *tire != id);
        self.kinds.remove(&id);
        self.pickup_layers.remove(&id);
        self.removal_layers.remove(&id);
        self.usable_items.remove(&id);
        self.filling_items.remove(&id);
        self.supplies.remove(&id);
        self.source_supply_emission.remove(&id);
        self.tank_rules.remove(&id);
        self.forget_openings(id);
    }
    /// Forget the holes and openings of a removed tank or cap.
    fn forget_openings(&mut self, id: ItemId) {
        self.holes.forget(id);
        self.openings
            .retain(|opening| opening.reference.tank != id && opening.reference.cap != id);
        for opening in &mut self.openings {
            if opening.borrowed_tank == Some(id) {
                opening.borrowed_tank = None;
            }
        }
        if self.fill_opening.is_some_and(|opening| {
            !self
                .openings
                .iter()
                .any(|state| state.reference.id == opening)
        }) {
            self.fill_opening = None;
        }
        if self.tuning_opening.is_some_and(|opening| {
            !self
                .openings
                .iter()
                .any(|state| state.reference.id == opening)
        }) {
            self.tuning_opening = None;
        }
    }

    fn insert_generated_body(&mut self, body: RigidBodyHandle) -> Result<BodyId, ReferenceError> {
        let id = BodyId(
            self.bodies
                .keys()
                .map(|b| b.0)
                .max()
                .unwrap_or(0)
                .checked_add(1)
                .ok_or_else(|| ReferenceError("Generated body identity exhausted".into()))?,
        );
        self.bodies.insert(id, body);
        Ok(id)
    }
    /// Binds the source `pushablescript` of a world copy to `item` and its body.
    /// `colliders` are the copy colliders on that body, with their prototype collider index.
    pub fn register_generated_push(
        &mut self,
        item: ItemId,
        body: RigidBodyHandle,
        reference: &crate::reference::WorldPushReference,
        colliders: &[(u32, ColliderHandle)],
        grip_hand: Option<VehiclePose>,
        physics: &mut PhysicsWorld,
    ) -> Result<(), ReferenceError> {
        let mut grip = Vec::new();
        for &(index, handle) in colliders {
            let mut id = ColliderId((physics.rapier().colliders[handle].user_data >> 64) as u32);
            if id.0 == 0 {
                id = self.allocate_generated_collider()?;
                physics.edit_collider(handle, |collider| {
                    collider.user_data |= (id.0 as u128) << 64;
                });
            }
            self.collider_bindings
                .entry(handle)
                .or_insert(ActionBinding {
                    collider: id,
                    use_owner: None,
                    pickup_owner: None,
                    removal_owner: None,
                    push_owner: None,
                })
                .push_owner = Some(item);
            if reference
                .grip
                .as_ref()
                .is_some_and(|g| g.colliders.contains(&index))
            {
                grip.push(id);
            }
        }
        if reference.grip.is_some() && grip.is_empty() {
            return Err(ReferenceError("Push grip collider is not published".into()));
        }
        let id = self.insert_generated_body(body)?;
        self.push
            .register_generated(item, id, reference, grip, grip_hand);
        Ok(())
    }

    pub fn register_generated_mechanism(
        &mut self,
        item: ItemId,
        body: RigidBodyHandle,
        colliders: &[(crate::reference::SourceId, ColliderHandle)],
    ) -> Result<(), ReferenceError> {
        let id = self.insert_generated_body(body)?;
        self.mechanism_bodies.insert(item, id);
        self.usable_items.insert(item);
        for (_, handle) in colliders {
            if let Some(binding) = self.collider_bindings.get_mut(handle) {
                binding.use_owner = Some(item);
            }
        }
        Ok(())
    }
    pub fn bind_generated_mechanism_surface(
        &mut self,
        item: ItemId,
        handle: ColliderHandle,
        collider: ColliderId,
    ) {
        let binding = self
            .collider_bindings
            .entry(handle)
            .or_insert(ActionBinding {
                collider,
                use_owner: None,
                pickup_owner: None,
                removal_owner: None,
                push_owner: None,
            });
        binding.use_owner = Some(item);
    }
    pub fn register_generated_pickup(
        &mut self,
        id: ItemId,
        rule: crate::reference::PickupReference,
        layers: u32,
        container: bool,
    ) {
        self.carry.register_generated(id, rule);
        self.pickup_layers.insert(id, layers);
        self.kinds.insert(
            id,
            if container {
                ItemKind::Container
            } else {
                ItemKind::Decoration
            },
        );
    }
    /// A generated supply or container cap. Hose ends are not saved.
    fn generated_cap(&self, cap: ItemId) -> Option<&OpeningState> {
        if !self.source_supply_emission.contains_key(&cap) {
            return None;
        }
        self.openings.iter().find(|o| o.reference.cap == cap)
    }
    pub fn generated_opening(&self, cap: ItemId) -> Option<OpeningId> {
        self.generated_cap(cap).map(|o| o.reference.id)
    }
    pub fn generated_valve(&self, cap: ItemId) -> Option<f32> {
        self.generated_cap(cap).map(|opening| opening.valve)
    }
    pub fn restore_generated_valve(
        &mut self,
        cap: ItemId,
        valve: f32,
    ) -> Result<(), ReferenceError> {
        if !valve.is_finite() || !(0.0..=1.0).contains(&valve) {
            return Err(ReferenceError("Invalid generated valve snapshot".into()));
        }
        let opening = self
            .openings
            .iter_mut()
            .find(|opening| opening.reference.cap == cap)
            .ok_or_else(|| ReferenceError("Missing generated valve".into()))?;
        opening.valve = valve;
        Ok(())
    }

    /// Register a hose end opening. The segment owns the pose; it has no fluid.
    fn register_hose_end(
        &mut self,
        segment: ItemId,
        end: &crate::reference::HoseEnd,
    ) -> Result<OpeningId, ReferenceError> {
        let id = self.next_opening_id()?;
        let o = &end.opening;
        self.openings.push(OpeningState {
            reference: OpeningRules {
                id,
                tank: segment,
                cap: segment,
                flow_liters_per_second: o.flow_liters_per_second,
                flow_radius: o.flow_radius,
                tilt_degrees: o.tilt_degrees,
                angle_disabled: o.angle_disabled,
                spill_disabled: o.spill_disabled,
                tube_connected: o.tube_connected,
                cant_tube: o.cant_tube,
                can_toggle: false,
                can_tune: false,
                tune_rate: 0.0,
                tune_icon: None,
                presentation: o.presentation.clone(),
            },
            local_pose: transform_pose(&end.pose),
            tilt_axis: Vector::from_array(o.tilt_axis),
            valve: 0.0,
            flow: None,
            fill_called: false,
            empty_pour: false,
            particles: Some(o.particles.clone()),
            contact: Some(fluid_contact::FluidContact::new(
                &o.particles,
                &end.visual,
                &end.splash,
                id.0,
            )),
            borrowed_tank: None,
        });
        Ok(id)
    }
    fn next_opening_id(&self) -> Result<OpeningId, ReferenceError> {
        self.openings
            .iter()
            .map(|opening| opening.reference.id.0)
            .max()
            .unwrap_or(0)
            .checked_add(1)
            .map(OpeningId)
            .ok_or_else(|| ReferenceError("Opening identity space exhausted".into()))
    }

    /// Bind checked source contents and opening rules once per generated instance.
    pub fn register_generated_supply(
        &mut self,
        tank: ItemId,
        cap: ItemId,
        definition: &crate::world::SupplyDefinition,
        point: &crate::world::SupplyPoint,
        reference: &ReferenceManifest,
    ) -> Result<(), ReferenceError> {
        if self
            .openings
            .iter()
            .any(|opening| opening.reference.cap == cap)
        {
            return if self.source_supply_emission.contains_key(&cap) {
                Ok(())
            } else {
                Err(ReferenceError(
                    "Generated cap overlaps an imported opening".into(),
                ))
            };
        }
        let get = |source: &crate::reference::SourceId| {
            reference
                .world
                .records
                .iter()
                .find(|record| &record.source == source)
                .ok_or_else(|| {
                    ReferenceError(format!(
                        "Missing source supply {}:{}",
                        source.file, source.path_id
                    ))
                })
        };
        let cap_record = get(&definition.cap)?;
        let tank_record = get(&definition.tank)?;
        let number = |record: &crate::reference::WorldSourceRecord,
                      name: &str|
         -> Result<f32, ReferenceError> {
            use crate::reference::WorldValue;
            let value = match record.fields.get(name) {
                Some(WorldValue::Number(value)) => *value as f32,
                Some(WorldValue::Integer(value)) => *value as f32,
                Some(WorldValue::Bool(value)) => u8::from(*value) as f32,
                _ => {
                    return Err(ReferenceError(format!(
                        "Missing source supply field {name}"
                    )));
                }
            };
            if !value.is_finite() {
                return Err(ReferenceError(format!(
                    "Nonfinite source supply field {name}"
                )));
            }
            Ok(value)
        };
        let radius = number(cap_record, "radius")?;
        let flow =
            radius * radius * std::f32::consts::PI * 0.025 * reference.fluid_rules.pour_speed;
        let valve = number(cap_record, "valve")?;
        let can_toggle = number(cap_record, "canToggle")? != 0.0;
        let can_tune = number(cap_record, "canTune")? != 0.0;
        let rules = OpeningRules {
            id: self.next_opening_id()?,
            tank,
            cap,
            flow_liters_per_second: flow,
            flow_radius: radius,
            tilt_degrees: number(cap_record, "defPourAngle")?,
            angle_disabled: number(cap_record, "noAngle")? != 0.0,
            spill_disabled: definition.no_pour,
            tube_connected: number(cap_record, "tube")? != 0.0,
            cant_tube: number(cap_record, "canttube")? != 0.0,
            can_toggle,
            can_tune,
            tune_rate: number(cap_record, "tuneSpeed")? * reference.fluid_rules.fluid_tune_speed,
            tune_icon: match cap_record.fields.get("img") {
                Some(crate::reference::WorldValue::Link(icon)) if can_tune => {
                    reference.ui.sprite(icon)
                }
                _ => None,
            },
            presentation: generated_presentation(
                number(cap_record, "hasName")? != 0.0,
                number(cap_record, "nameText")?,
                number(cap_record, "noText")? != 0.0,
                reference.language.names.len(),
            )?,
        };
        let tank_rules = TankRules {
            no_drink: number(tank_record, "noDrink")? != 0.0,
            no_fill_this: definition.no_fill_this,
            no_fill_from: definition.no_fill_from,
            slow_oil: number(tank_record, "slowOil")? != 0.0,
            explosion: definition.explosion,
        };
        // Subtract in f64 before conversion, so distant bindings retain local precision.
        let relative = VehiclePose {
            position: std::array::from_fn(|a| {
                (point.cap_pose.position[a] - point.tank_pose.position[a]) as f32
            }),
            rotation: point.cap_pose.rotation,
        };
        let tank_rotation = to_rapier(VehiclePose {
            position: [0.0; 3],
            rotation: point.tank_pose.rotation,
        });
        self.openings.push(OpeningState {
            reference: rules,
            local_pose: tank_rotation.inverse() * to_rapier(relative),
            tilt_axis: -Vector::Z,
            valve,
            flow: None,
            fill_called: false,
            empty_pour: false,
            particles: None,
            contact: None,
            borrowed_tank: None,
        });
        self.source_supply_emission.insert(
            cap,
            [definition.emission_min_rate, definition.emission_max_rate],
        );
        self.tank_rules.insert(tank, tank_rules);
        self.kinds.insert(cap, ItemKind::Cap);
        if can_toggle || can_tune {
            self.usable_items.insert(cap);
        }
        self.filling_items.insert(cap);
        self.supplies.insert(tank);
        Ok(())
    }

    /// A movable container shares source opening rules but is not a fixed supply nozzle.
    pub fn register_generated_container(
        &mut self,
        tank: ItemId,
        cap: ItemId,
        definition: &crate::world::SupplyDefinition,
        point: &crate::world::SupplyPoint,
        reference: &ReferenceManifest,
    ) -> Result<(), ReferenceError> {
        self.register_generated_supply(tank, cap, definition, point, reference)?;
        self.supplies.remove(&tank);
        self.kinds.insert(tank, ItemKind::Container);
        Ok(())
    }
    /// tankscript explosion fields of a tank.
    pub fn tank_explosion(&self, tank: ItemId) -> Option<&crate::world::TankExplosion> {
        self.tank_rules.get(&tank).map(|r| &r.explosion)
    }
    pub fn bind_generated_cap(&mut self, handle: ColliderHandle, cap: ItemId) {
        let binding = self
            .collider_bindings
            .entry(handle)
            .or_insert(ActionBinding {
                collider: ColliderId(0),
                use_owner: None,
                pickup_owner: None,
                removal_owner: None,
                push_owner: None,
            });
        binding.use_owner = Some(cap);
    }
    fn sound(&mut self, sound: InteractionSound) {
        if self.sound_count < self.sounds.len() {
            self.sounds[self.sound_count] = Some(sound);
            self.sound_count += 1;
        }
    }

    fn complete_carry_release(
        &mut self,
        action: &mut Option<InteractionAction>,
        release: InteractionAction,
    ) {
        if matches!(*action, Some(InteractionAction::LightControl(_))) {
            // The runtime must still execute this external owner.
            self.completed_action(release);
        } else {
            *action = Some(release);
        }
    }
    pub fn completed_action(&mut self, action: InteractionAction) {
        self.sound(InteractionSound::Action(action));
    }

    pub fn pop_sound(&mut self) -> Option<InteractionSound> {
        if self.sound_count == 0 {
            return None;
        }
        let sound = self.sounds[0];
        self.sound_count -= 1;
        self.sounds.copy_within(1..=self.sound_count, 0);
        self.sounds[self.sound_count] = None;
        sound
    }

    fn fill_pair(&self, opening: &OpeningState, items: &ItemStore) -> Option<(ItemId, ItemId)> {
        let source = self.held()?;
        let destination = opening.reference.tank;
        if source == destination
            || opening.valve <= 0.
            || self.tank_rules.get(&source)?.no_fill_from
            || self.tank_rules.get(&destination)?.no_fill_this
        {
            return None;
        }
        let source_fluid = items.get(source)?.fluid.as_ref()?;
        let destination_fluid = items.get(destination)?.fluid.as_ref()?;
        (source_fluid.total() > 0. && destination_fluid.total() < destination_fluid.capacity_liters)
            .then_some((source, destination))
    }

    pub fn opening_flow(&self, id: OpeningId) -> Option<(bool, f32)> {
        self.openings
            .iter()
            .find(|o| o.reference.id == id)
            .and_then(|o| o.flow)
    }

    pub fn opening_audio_flags(&self, id: OpeningId) -> (bool, bool) {
        self.openings
            .iter()
            .find(|o| o.reference.id == id)
            .map_or((false, false), |o| (o.fill_called, o.empty_pour))
    }
    pub fn opening_valves(&self) -> impl Iterator<Item = (OpeningId, f32)> + '_ {
        self.openings.iter().map(|o| (o.reference.id, o.valve))
    }
    pub fn opening_valve(&self, id: OpeningId) -> f32 {
        self.openings
            .iter()
            .find(|opening| opening.reference.id == id)
            .map_or(0.0, |opening| opening.valve)
    }
    pub fn tuning_opening(&self) -> Option<OpeningId> {
        self.tuning_opening
    }
    pub fn dragging_mechanism(&self) -> Option<ItemId> {
        self.latched_mechanism.map(|(item, _, _, _)| item)
    }
    pub fn throw_charge(&self) -> f32 {
        self.carry.throw_charge()
    }
    pub fn throw_active(&self) -> bool {
        self.carry.throw_active()
    }
    /// Returns the pushed body and its steering input.
    pub fn push_steering(&self) -> Option<(RigidBodyHandle, f32)> {
        self.push.steering()
    }
    pub fn push_gripping(&self) -> bool {
        self.push.gripping()
    }
    pub fn cancel_input(&mut self) {
        self.equipment_input.cancel_input();
        self.cancelled_input = keys::GRAB
            | keys::RECENTER
            | keys::TRANSLATE
            | keys::ROTATE
            | keys::USE
            | keys::PRIMARY;
        self.cleaning.reset_contact();
        self.seat_press = None;
        self.seat_action_latched = false;
        self.fill_opening = None;
        self.f_gesture_consumed = false;
        self.carry.cancel_input();
        self.push.cancel_input();
        self.latched_mechanism = None;
        self.use_consumed = false;
        self.tuning_opening = None;
        self.mechanisms.cancel_input();
    }
    pub fn finish_cancelled_input(&mut self, player: &mut PlayerState, physics: &mut PhysicsWorld) {
        self.push.finish_cancelled_input(player, physics);
    }
    pub fn drop_held(
        &mut self,
        items: &mut ItemStore,
        physics: &mut PhysicsWorld,
    ) -> Option<ItemId> {
        let item = self.carry.drop(items, physics)?;
        self.cleaning.reset_contact();
        self.storage.mark_dropped(item);
        Some(item)
    }
    pub fn pickup_item(
        &mut self,
        item: ItemId,
        point: [f32; 3],
        player: &PlayerState,
        items: &mut ItemStore,
        physics: &mut PhysicsWorld,
    ) -> Result<(), ReferenceError> {
        // Source pickupable.CanPickup refuses IsBuried items.
        if self
            .buried
            .is_buried(item, items, physics, self.player_body)
        {
            return Err(ReferenceError("Item is buried".into()));
        }
        // Source pickupable.Pick on an attached grip detaches it first.
        if self.hitch.is_hitched(item) {
            self.hitch.detach(item, items, physics)?;
        }
        self.carry.pickup(item, point, player, items, physics)?;
        self.storage.picked_up(item);
        Ok(())
    }

    pub fn fluid_particles(
        &self,
    ) -> impl Iterator<Item = (OpeningId, &fluid_contact::FluidContact)> {
        self.openings
            .iter()
            .filter_map(|o| o.contact.as_ref().map(|c| (o.reference.id, c)))
    }
    pub fn rebase(&mut self, shift: [f32; 3]) {
        self.cleaning.reset_contact();
        self.melee.rebase(shift);
        self.decals.rebase(shift);
        self.mechanisms.rebase(shift);
        self.parts.rebase(shift);
        for opening in &mut self.openings {
            if let Some(contact) = opening.contact.as_mut() {
                contact.rebase(Vector::from_array(shift));
            }
        }
    }

    pub fn take_drink_contact(&mut self) -> Option<(ItemId, f32, [f32; 6])> {
        self.pending_drink_contacts.pop()
    }

    pub fn step(
        &mut self,
        mut input: FrameInput,
        mut aim: ResolvedAim,
        player: &mut PlayerState,
        body: &mut crate::player::body::BodyState,
        items: &mut ItemStore,
        physics: &mut PhysicsWorld,
        dt: f32,
        needs: NeedsOutput,
        fluid_scale: f32,
    ) -> InteractionOutcome {
        self.finish_cancelled_input(player, physics);
        let binoculars = self.binoculars.step(
            self.equipment.item(),
            self.held(),
            input.held & keys::PRIMARY != 0,
            input.wheel,
            dt,
        );
        if binoculars.is_some() {
            // fpscontroller: BcanTune. The wheel focuses and does not move the carried item.
            input.wheel = 0.;
        }
        if self.reserves_lmb() {
            input.held &= !keys::PRIMARY;
            input.pressed &= !keys::PRIMARY;
            input.released &= !keys::PRIMARY;
        }
        if !dt.is_finite() || dt <= 0.0 {
            return InteractionOutcome {
                action: None,
                look_mouse: input.mouse,
            };
        }
        self.buried.tick(dt);
        for opening in &mut self.openings {
            opening.flow = None;
            opening.fill_called = false;
            opening.empty_pour = false;
            if let Some(contact) = opening.contact.as_mut() {
                contact.clear_outputs();
            }
        }
        self.mechanisms.begin_audio_step();
        let mut cap_sounds = [None; 2];
        self.pending_drink_contacts.clear();
        let acquisition_release = self.carry.acquisition_held();
        let reach = self.reach;
        let valid = |target: AimTarget| {
            target.distance.is_finite() && target.distance >= 0.0 && target.distance <= reach
        };
        if input.held & keys::USE == 0 {
            self.seat_action_latched = false;
        }
        let ray_actions_blocked = player.view(physics).ray_actions_blocked;
        if ray_actions_blocked {
            aim = ResolvedAim::default();
            self.latched_mechanism = None;
        }
        if input.pressed & keys::USE != 0 {
            let usable = aim
                .use_target
                .and_then(|target| self.mechanisms.contains(target.item).then_some(target.item));
            self.seat_press = Some(SeatPress {
                usable,
                duration: 0.0,
            });
        }
        if input.held & keys::USE != 0
            && let Some(press) = &mut self.seat_press
        {
            press.duration += dt;
        }
        let seat_release = if input.released & keys::USE != 0 {
            self.seat_press.take()
        } else {
            None
        };
        if self.active_mechanism_target(player, physics).is_none() {
            self.latched_mechanism = None;
        }
        let mut availability = self.available_actions(&aim, player, items, physics, needs);
        if availability.active_drag && !self.drag_input(input, &availability) {
            self.latched_mechanism = None;
            availability = self.available_actions(&aim, player, items, physics, needs);
        }
        let resolved = self.resolve_input(input, &availability);
        self.carry.release_acquisition(input);
        let grab_input = resolved.grab_input;
        let use_input = resolved.use_input;
        let light_action = availability.light_control.filter(|_| {
            use_input.pressed & keys::USE != 0
                || (use_input.pressed & keys::PRIMARY != 0 && availability.lmb_use)
        });
        let target = aim
            .use_target
            .filter(|target| valid(*target) && availability.light_control.is_none());
        let grab_target = aim.grab_target.filter(|target| valid(*target));
        let filling_target = aim.filling_target.filter(|target| valid(*target));
        self.mechanisms.sync(physics);
        let released_mechanism = use_input.released & keys::USE != 0 && self.use_consumed;
        let primary_use = availability.lmb_use && self.held_light_control().is_none();
        let mechanism_held = use_input.held & keys::USE != 0
            || (use_input.held & keys::PRIMARY != 0 && primary_use)
            || (use_input.wheel != 0.
                && availability.use_item.is_some_and(|item| {
                    self.mechanisms.drag_eligible(item) && self.mechanisms.scroll_eligible(item)
                }));
        if !mechanism_held {
            self.latched_mechanism = None;
            self.use_consumed = false;
        } else if self.latched_mechanism.is_none()
            && let Some(target) = target
            && self.mechanisms.contains(target.item)
            && (use_input.held & keys::USE != 0 || self.mechanisms.drag_eligible(target.item))
            && let Some(&body_id) = self.mechanism_bodies.get(&target.item)
            && let Some(body) = self
                .bodies
                .get(&body_id)
                .and_then(|handle| physics.rapier().bodies.get(*handle))
        {
            self.latched_mechanism = Some((
                target.item,
                body_id,
                body.position().inverse() * Vector::from_array(target.point),
                target.collider,
            ));
            self.use_consumed = true;
        }
        let use_target = self.active_mechanism_target(player, physics).or(target);
        let mut action = None;
        let available = availability;
        if grab_input.pressed & keys::GRAB != 0 {
            self.f_gesture_consumed = false;
        }
        let grip_release = self.push.gripping() && grab_input.pressed & keys::GRAB != 0;
        if grip_release {
            self.push.stop(player, physics);
            self.f_gesture_consumed = true;
        }
        if grab_input.released & keys::GRAB != 0
            && !self.f_gesture_consumed
            && !acquisition_release
            && available.actions.0 & ActionMask::MOUNT != 0
            && self.carry.throw_charge() < self.carry.reference_throw_minimum()
        {
            if let Some(slot) = available.slot {
                let held = self.held().expect("mount held item");
                match slot {
                    SlotTarget::Wheel(slot) => {
                        self.carry.cancel_input();
                        action = Some(InteractionAction::Wheel(WheelAction::Install {
                            item: held,
                            target: slot,
                        }));
                    }
                    SlotTarget::Tire(rim) => {
                        self.carry.cancel_input();
                        action = Some(InteractionAction::Wheel(WheelAction::Join {
                            rim,
                            tire: held,
                        }));
                    }
                    SlotTarget::Hitch(target) => {
                        self.drop_held(items, physics);
                        self.hitch
                            .attach(held, target, items, physics)
                            .expect("validated grip attach");
                        action = Some(InteractionAction::HitchAttach(held));
                    }
                    SlotTarget::Part(slot) => {
                        self.drop_held(items, physics);
                        self.parts
                            .install(held, slot, items, physics)
                            .expect("validated part installation");
                        self.parts.mount_mechanism(held, &mut self.mechanisms);
                        self.sync_parts(items, physics).expect("bound parts");
                        action = Some(InteractionAction::PartInstall(held));
                        self.sound(InteractionSound::Action(InteractionAction::PartInstall(
                            held,
                        )));
                    }
                }
            } else if let Some(cap) = available.hose_cap
                && let Some(held) = self.attach_held_hose(cap, items, physics)
            {
                action = Some(InteractionAction::Drop(held));
            }
        }
        if grab_input.pressed & keys::GRAB != 0
            && !self.carry.acquisition_held()
            && !grip_release
            && self.held().is_none()
        {
            if available.actions.0 & ActionMask::DISMOUNT != 0 {
                if let Some(rim) = available.rim {
                    action = Some(InteractionAction::Wheel(WheelAction::Split(rim)));
                } else if let Some(target) = grab_target {
                    if self.parts.contains(target.item) {
                        self.parts
                            .remove(target.item, items, physics)
                            .expect("validated removal");
                        self.parts
                            .detach_mechanism(target.item, &mut self.mechanisms, physics);
                        self.sync_parts(items, physics).expect("bound parts");
                        self.carry
                            .pickup(target.item, target.point, player, items, physics)
                            .expect("validated carry acquisition");
                        action = Some(InteractionAction::PartRemove(target.item));
                        self.completed_action(InteractionAction::PartRemove(target.item));
                    } else {
                        action = Some(InteractionAction::Wheel(WheelAction::Remove(target.item)));
                    }
                }
            }
            if action.is_some() {
                self.f_gesture_consumed = true;
            }
        }

        let carry_input = resolved.carry_input;
        self.tuning_opening = None;
        let can_use_target = availability.light_control.is_some()
            || use_target.is_some_and(|target| {
                self.dial_knob(&target, physics).is_some()
                    || self.mechanisms.contains(target.item)
                    || self.openings.iter().any(|opening| {
                        opening.reference.cap == target.item
                            && (opening.reference.can_toggle || opening.reference.can_tune)
                    })
            });
        if action.is_none() {
            action = light_action.map(InteractionAction::LightControl);
            if !ray_actions_blocked
                && use_input.pressed & keys::USE != 0
                && player.seated()
                && !can_use_target
            {
                if player.try_exit(physics) {
                    self.seat_action_latched = true;
                    action = Some(InteractionAction::Seat);
                }
            } else if let Some(target) = use_target {
                if use_input.held & keys::USE != 0
                    || use_input.pressed & (keys::USE | keys::PRIMARY) != 0
                    || use_input.wheel != 0.
                {
                    items.mark_dirty(target.item);
                }
                if use_input.pressed & keys::USE != 0
                    || (use_input.pressed & keys::PRIMARY != 0 && availability.lmb_use)
                {
                    for opening in &mut self.openings {
                        if opening.reference.cap == target.item && opening.reference.can_toggle {
                            let previous = opening.valve;
                            items.mark_dirty(target.item);
                            opening.valve = if opening.valve > 0.0 { 0.0 } else { 1.0 };
                            cap_sounds[0] = Some(InteractionSound::Cap {
                                id: opening.reference.id,
                                previous,
                                current: opening.valve,
                                tune: None,
                            });
                            action = Some(InteractionAction::Cap(opening.reference.id));
                            break;
                        }
                    }
                }
                if let Some((knob, Knob::Crank)) = self.dial_knob(&target, physics) {
                    if use_input.held & keys::USE != 0
                        || (primary_use && use_input.held & keys::PRIMARY != 0)
                    {
                        // Browser movementY points down. Source Mouse Y points up.
                        let look = [use_input.mouse[0], -use_input.mouse[1]];
                        if look.iter().all(|v| v.is_finite()) {
                            action = Some(InteractionAction::Crank(knob, look));
                        }
                    }
                } else if let Some((knob, Knob::Switch)) = self.dial_knob(&target, physics) {
                    if use_input.pressed & keys::USE != 0
                        || (use_input.pressed & keys::PRIMARY != 0 && availability.lmb_use)
                    {
                        action = Some(InteractionAction::DialTune(knob, 0.));
                    }
                } else if let Some((knob, _)) = self.dial_knob(&target, physics) {
                    let mut knob_input = use_input;
                    if !availability.lmb_use {
                        knob_input.held &= !keys::PRIMARY;
                    }
                    let direction = self.mechanisms.tune_direction(&knob_input);
                    if direction.is_finite() && direction != 0. {
                        action = Some(InteractionAction::DialTune(knob, direction.signum()));
                    }
                }
                if let Some(opening) = self.openings.iter_mut().find(|opening| {
                    opening.reference.cap == target.item && opening.reference.can_tune
                }) {
                    let delta = if use_input.wheel.is_finite() && use_input.wheel != 0.0 {
                        use_input.wheel * self.scroll_sensitivity
                    } else if use_input.held & keys::USE != 0
                        || (primary_use && use_input.held & keys::PRIMARY != 0)
                    {
                        use_input.mouse[1]
                    } else {
                        0.0
                    };
                    if use_input.held & keys::USE != 0
                        || (primary_use && use_input.held & keys::PRIMARY != 0)
                        || delta != 0.0
                    {
                        self.tuning_opening = Some(opening.reference.id);
                    }
                    if delta.is_finite() && delta != 0.0 {
                        items.mark_dirty(target.item);
                        let previous = opening.valve;
                        let requested = delta * dt * opening.reference.tune_rate;
                        opening.valve = (opening.valve + requested).clamp(0.0, 1.0);
                        cap_sounds[1] = Some(InteractionSound::Cap {
                            id: opening.reference.id,
                            previous,
                            current: opening.valve,
                            tune: Some(requested),
                        });
                        if opening.valve < 1e-6 {
                            opening.valve = 0.0;
                        }
                        action = Some(InteractionAction::Cap(opening.reference.id));
                    }
                }
                if use_input.released & keys::USE != 0
                    && !released_mechanism
                    && let Some(seat) = self.item_seats.get(&target.collider)
                    && seat.can_sit(physics)
                    && seat_release.is_some_and(|press| {
                        press.matches(self.mechanisms.contains(target.item).then_some(target.item))
                    })
                    && player.enter_item_seat(seat, physics)
                {
                    self.seat_action_latched = true;
                    action = Some(InteractionAction::Seat);
                }
                if self.mechanisms.contains(target.item)
                    && (use_input.pressed & keys::USE != 0
                        || (use_input.pressed & keys::PRIMARY != 0
                            && self.mechanisms.primary_switch(target.item)
                            && availability.lmb_use)
                        || use_input.held & keys::USE != 0
                        || (use_input.held & keys::PRIMARY != 0
                            && self.mechanisms.drag_eligible(target.item)
                            && primary_use)
                        || (use_input.wheel != 0.0 && self.mechanisms.scroll_eligible(target.item)))
                {
                    let mut mechanism_input = use_input;
                    if !availability.lmb_use {
                        mechanism_input.held &= !keys::PRIMARY;
                        mechanism_input.pressed &= !keys::PRIMARY;
                    }
                    let camera = player.view(physics).camera_pose;
                    // Source: a new use moves the hand. A held usable only refreshes it.
                    if use_input.pressed & (keys::USE | keys::PRIMARY) != 0
                        || use_input.wheel != 0.0
                    {
                        if let Some((reach, rule)) = self.mechanisms.using_hand(
                            target.item,
                            target.point,
                            Rotation::from_array(camera.rotation),
                            target.collider,
                            physics,
                        ) {
                            body.move_using_hand(reach, rule);
                        }
                    } else {
                        body.refresh_using_hand();
                    }
                    self.mechanisms
                        .step(target.item, mechanism_input, camera, dt);
                    // A usable can slide and be a switch knob (bus door button): both react.
                    if !matches!(action, Some(InteractionAction::DialTune(..))) {
                        action = Some(InteractionAction::Mechanism(target.item));
                    }
                }
            }
        }
        for sound in cap_sounds.into_iter().flatten() {
            self.sound(sound);
        }
        if action.is_none()
            && !grip_release
            && !self.carry.acquisition_held()
            && self.held().is_none()
            && (grab_input.held | grab_input.pressed) & keys::GRAB != 0
            && available.actions.0 & ActionMask::PICKUP != 0
            && let Some(target) = grab_target
            && (if self.storage.stored(target.item) {
                self.pickup_stored_item(target.item, target.point, player, items, physics)
                    .is_ok()
            } else {
                self.pickup_item(target.item, target.point, player, items, physics)
                    .is_ok()
            })
        {
            action = Some(InteractionAction::Pickup(target.item));
        }
        if matches!(action, Some(InteractionAction::Pickup(_)))
            && (grab_input.held | grab_input.pressed) & keys::GRAB != 0
        {
            self.f_gesture_consumed = true;
        }
        let push_target = aim.push_target.filter(|target| valid(*target));
        let pushing =
            if (action.is_none() || self.push.gripping()) && self.held().is_none() && !grip_release
            {
                self.push.step(
                    resolved.push_input,
                    push_target,
                    &self.bodies,
                    player,
                    body,
                    physics,
                    dt,
                )
            } else {
                self.push.stop(player, physics);
                None
            };
        if action.is_none()
            && let Some(item) = pushing
        {
            action = Some(InteractionAction::Push(item));
        }
        if !acquisition_release && !self.f_gesture_consumed && self.held().is_some() {
            if grab_input.pressed & keys::GRAB != 0 {
                self.carry.begin_throw();
            }
            if grab_input.held & keys::GRAB != 0 {
                self.carry.charge_throw(dt);
            }
            if grab_input.released & keys::GRAB != 0 {
                let throwing = self.carry.throw_charge() > self.carry.reference_throw_minimum();
                if let Some(item) = self.carry.release_throw(player, items, physics) {
                    self.storage.mark_dropped(item);
                    let release = if throwing {
                        InteractionAction::Throw(item)
                    } else {
                        InteractionAction::Drop(item)
                    };
                    self.complete_carry_release(&mut action, release);
                }
            }
        }
        if self.carry.step(
            carry_input,
            player,
            self.tuning_opening.is_some(),
            physics,
            dt,
        ) && let Some(item) = self.drop_held(items, physics)
        {
            self.complete_carry_release(&mut action, InteractionAction::Drop(item));
        }
        self.mechanisms.sync(physics);
        items.sync_poses(physics);
        let hose_input = hose::HoseInput {
            held: self.held(),
            coil: input.held & keys::RELOAD != 0,
            drain: input.held & keys::PRIMARY != 0,
        };
        self.step_hoses(hose_input, dt, fluid_scale, items, physics);
        let mut filled_destination = None;
        if input.held & keys::PRIMARY != 0
            && available.actions.0 & ActionMask::FILL != 0
            && let Some(target) = filling_target
        {
            if let Some(opening) = self
                .openings
                .iter()
                .find(|opening| opening.reference.cap == target.item)
                && let Some((source_id, destination)) = self.fill_pair(opening, items)
            {
                let flow = opening.reference.flow_liters_per_second * opening.valve;
                let destination_opening = opening.reference.id;
                let destination_rules = self.tank_rules.get(&destination);
                if let Some((source, destination_state)) =
                    items.get_pair_mut(source_id, destination)
                    && let (Some(source), Some(destination_state)) =
                        (source.fluid.as_mut(), destination_state.fluid.as_mut())
                {
                    let mut amount = flow * dt;
                    let source_total = source.total();
                    let destination_total = destination_state.total();
                    if destination_rules.is_some_and(|rules| rules.slow_oil)
                        && source_total > 0.0
                        && source.mixture().amount(1) / source_total > 0.25
                        && destination_total > 0.05
                        && destination_state.mixture().amount(1) / destination_total < 1.0
                    {
                        amount = slow_oil_amount(amount, destination_total, self.slow_oil_modifier);
                    }
                    let liters = transfer(source, destination_state, amount * fluid_scale);
                    if liters > 0.0 {
                        filled_destination = Some(destination_opening);
                        action = Some(InteractionAction::Fill {
                            source: source_id,
                            destination,
                            liters,
                        });
                    }
                }
            }
        }
        for opening in &mut self.openings {
            if filled_destination == Some(opening.reference.id) {
                opening.flow = Some((true, 1.));
                opening.fill_called = true;
            }
            if opening.particles.is_none() {
                continue;
            }
            let Some((item_pose, item)) = pour_source(opening, items) else {
                continue;
            };
            let cap_pose = item_pose * opening.local_pose;
            let forward = cap_pose.rotation * opening.tilt_axis;
            let angle = forward.dot(Vector::Y).clamp(-1.0, 1.0).acos().to_degrees();
            let (emission_rate, emission_speed, poured, mut fractions) =
                spill_step(opening, item.fluid.as_mut(), angle, dt, fluid_scale);
            let particles = opening.particles.as_ref().unwrap();
            let contact = opening.contact.as_mut().unwrap();
            contact.update_response(particles);
            let view = to_rapier(player.view(physics).head_pose);
            let cap_center = view.translation + view.rotation * self.drink_collider_center;
            if poured.is_some() {
                contact.set_color(fractions);
            }
            contact.advance(
                particles,
                cap_pose,
                opening.tilt_axis,
                self.player_body,
                cap_center,
                self.drink_collider_radius,
                self.drink_contact_radius,
                physics,
                dt,
                emission_rate,
                emission_speed,
                poured,
            );
            if contact.player_hit() && contact.last_poured > 0. {
                if poured.is_none() {
                    let Some(fluid) = item.fluid.as_ref() else {
                        continue;
                    };
                    let total = fluid.total();
                    if total <= 0. {
                        continue;
                    }
                    fractions = fluid
                        .mixture()
                        .quantities_liters
                        .map(|amount| amount / total);
                }
                if action.is_none() {
                    action = Some(InteractionAction::DrinkContact {
                        source: item.id,
                        poured_liters: contact.last_poured * fluid_scale,
                        fractions,
                    });
                } else {
                    self.pending_drink_contacts.push((
                        item.id,
                        contact.last_poured * fluid_scale,
                        fractions,
                    ));
                }
            }
        }
        self.holes.step(items, dt, fluid_scale);
        self.apply_fluid_receipts(items, physics, fluid_scale);
        if input.released & keys::GRAB != 0 {
            self.f_gesture_consumed = false;
        }
        if action.is_none()
            && input.pressed & keys::PRIMARY != 0
            && available.actions.0 & ActionMask::CONSUME != 0
            && let Some(item) = self.held()
        {
            action = Some(InteractionAction::Consume(item));
        }
        if action.is_none()
            && input.held & keys::PRIMARY != 0
            && available.actions.0 & ActionMask::DRINK != 0
            && let Some(source) = self.held()
        {
            action = Some(InteractionAction::Drink {
                source,
                requested_liters: self.drink_flow_liters_per_second * dt * fluid_scale,
            });
        }
        self.fill_opening = availability.fill_reservation;
        InteractionOutcome {
            action,
            look_mouse: resolved.look_mouse,
        }
    }

    pub fn sync_parts(
        &mut self,
        items: &mut ItemStore,
        physics: &mut PhysicsWorld,
    ) -> Result<(), ReferenceError> {
        self.parts.prepare_mechanisms(items, &mut self.mechanisms);
        self.mechanisms.sync(physics);
        self.parts
            .capture_mechanisms(items, &self.mechanisms, physics)?;
        self.parts.sync(items, physics)?;
        self.hitch.sync(items, physics);
        Ok(())
    }
    pub fn sync_mechanisms(&mut self, physics: &mut PhysicsWorld) {
        self.mechanisms.sync(physics);
    }
}

fn transform_pose(transform: &crate::reference::Transform) -> Pose {
    to_rapier(VehiclePose {
        position: transform.translation,
        rotation: transform.rotation,
    })
}
fn inverse_lerp(start: f32, end: f32, value: f32) -> f32 {
    if (end - start).abs() <= f32::EPSILON {
        0.0
    } else {
        ((value - start) / (end - start)).clamp(0.0, 1.0)
    }
}

/// Source `GetCapString` reads `ls.l.g(nameText)`, not the serialized `nameString`.
/// Pose of the opening's pose owner and the state of its fluid owner.
/// An inactive pose owner does not pour.
fn pour_source<'a>(
    opening: &OpeningState,
    items: &'a mut ItemStore,
) -> Option<(Pose, &'a mut items::ItemState)> {
    let pose_owner = items.get(opening.reference.tank)?;
    if pose_owner.inactive_pose.is_some() {
        return None;
    }
    let pose = to_rapier(pose_owner.pose);
    Some((pose, items.get_mut(opening.fluid_owner())?))
}

/// `tankcapscript.Update` pour factor at `angle` degrees from up. None below the pour angle,
/// which rises from `tilt_degrees` at full to 100 when empty.
fn pour_factor(tilt_degrees: f32, angle_disabled: bool, fill: f32, angle: f32) -> Option<f32> {
    if angle_disabled {
        return Some(1.0);
    }
    let threshold = 100.0 + (tilt_degrees - 100.0) * fill;
    (angle > threshold).then(|| {
        inverse_lerp(threshold, tilt_degrees + 90.0, angle).max(inverse_lerp(90.0, 180.0, angle))
    })
}

/// Spill from `fluid` through `opening` at `angle` degrees from up.
/// Returns emission rate, emission speed, poured liters, and fluid fractions.
fn spill_step(
    opening: &mut OpeningState,
    fluid: Option<&mut crate::fluids::FluidContainer>,
    angle: f32,
    dt: f32,
    fluid_scale: f32,
) -> (f32, f32, Option<f32>, [f32; 6]) {
    let mut result = (0., 0., None, [0.; 6]);
    let (Some(fluid), Some(particles)) = (fluid, opening.particles.as_ref()) else {
        return result;
    };
    let fill = (fluid.total() / fluid.capacity_liters).clamp(0.0, 1.0);
    let rules = &opening.reference;
    let factor = pour_factor(rules.tilt_degrees, rules.angle_disabled, fill, angle);
    opening.empty_pour = fluid.total() <= 0.;
    if let Some(factor) = factor
        && opening.valve > 0.
        && !rules.spill_disabled
        && !rules.tube_connected
        && fluid.total() > 0.
    {
        let speed = factor * opening.valve * opening.reference.flow_liters_per_second;
        let requested_liters = speed * dt * fluid_scale;
        let total = fluid.total();
        result.3 = fluid
            .mixture()
            .quantities_liters
            .map(|amount| amount / total);
        let debited = spill(fluid, requested_liters);
        if debited > 0. {
            opening.flow = Some((false, speed));
        }
        result.2 = Some(speed * dt);
        result.0 =
            particles.min_rate + (particles.max_rate - particles.min_rate) * speed.clamp(0., 1.);
        result.1 = particles.max_speed * fill * factor;
    }
    result
}

fn generated_presentation(
    has_name: bool,
    name_text: f32,
    no_text: bool,
    words: usize,
) -> Result<FluidPresentation, ReferenceError> {
    if has_name && !(name_text >= 0.0 && (name_text as usize) < words) {
        return Err(ReferenceError(
            "Cap name word outside language table".into(),
        ));
    }
    Ok(FluidPresentation {
        has_name,
        word: if has_name { name_text as u32 } else { 0 },
        no_text,
    })
}

/// Source `DrinkFill` slow oil: `Mathf.Clamp(num * amount * slowOilModifier, 0.0001f, num)`.
/// Unity tests the minimum first, so a result below 0.0001 stays 0.0001 even above `num`.
fn slow_oil_amount(amount: f32, destination_total: f32, modifier: f32) -> f32 {
    let value = amount * destination_total * modifier;
    if value < 0.0001 {
        0.0001
    } else if value > amount {
        amount
    } else {
        value
    }
}
