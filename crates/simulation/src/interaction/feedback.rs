use super::*;
use crate::reference::FluidPresentation;

#[derive(Clone, Copy, Debug, Default)]
pub struct ActionMask(pub u32);
impl ActionMask {
    pub const USE: u32 = 1;
    pub const DOOR: u32 = 2;
    pub const PICKUP: u32 = 4;
    pub const DROP: u32 = 8;
    pub const MOUNT: u32 = 16;
    pub const DISMOUNT: u32 = 32;
    pub const FILL: u32 = 64;
    pub const TUNE: u32 = 128;
    pub const ENTER: u32 = 256;
    pub const EXIT: u32 = 512;
    pub const SWITCH: u32 = 1024;
    pub const PUSH: u32 = 2048;
    pub const GRIP: u32 = 4096;
    pub const THROW: u32 = 8192;
    pub const ROTATE: u32 = 32768;
    pub const RELEASE_GRIP: u32 = 65536;
    pub const PRIMARY_SWITCH: u32 = 131072;
    pub const SCROLL: u32 = 262144;
    pub const CONSUME: u32 = 524288;
    pub const DRINK: u32 = 1048576;
    pub const HELD_SWITCH: u32 = 2097152;
    pub const WASH: u32 = 4194304;
    pub const SCRUB: u32 = 8388608;
    pub const EQUIP: u32 = 1 << 24;
    pub const RECENTER: u32 = 1 << 25;
    pub const RELEASE_EQUIPMENT: u32 = 1 << 26;
    pub const RELEASE_EQUIPMENT_SEATED: u32 = 1 << 27;
    pub const TAKE_ONE: u32 = 1 << 28;
    pub const SMOKE: u32 = 1 << 29;
    pub const PUT_ON: u32 = 1 << 30;
    pub const CRANK: u32 = 1 << 31;
}
/// Item prompt bits. Hose: attach on the mount key, coil up on R, drain on LMB.
/// Ammo: reload on R, unload on E. Binoculars: use on LMB, focus on MMB.
pub struct ItemPrompt;
impl ItemPrompt {
    pub const ATTACH: u8 = 1;
    pub const COIL: u8 = 2;
    pub const DRAIN: u8 = 4;
    pub const RELOAD: u8 = 8;
    pub const UNLOAD: u8 = 16;
    /// Binoculars: LMB use.
    pub const LOOK: u8 = 32;
    /// Binoculars in use: MMB focus.
    pub const FOCUS: u8 = 64;
}
/// Computer prompt bits: R turns on or off, LMB uses, E connects or disconnects a held one.
pub struct ComputerPrompt;
impl ComputerPrompt {
    pub const TURN_ON: u8 = 1;
    pub const TURN_OFF: u8 = 2;
    pub const USE: u8 = 4;
    pub const CONNECT: u8 = 8;
    pub const DISCONNECT: u8 = 16;
    /// The player types on a computer (inputscript.usingComputer).
    pub const TYPING: u8 = 32;
}
#[derive(Clone, Copy, Debug, Default, PartialEq)]
#[repr(u32)]
pub enum FeedbackContext {
    #[default]
    None = 0,
    Door = 1,
    Cap = 2,
    Container = 3,
    Seat = 4,
    Wheel = 5,
    Pump = 6,
    Mechanism = 7,
    Part = 8,
    Push = 9,
    LightSwitch = 10,
    RadioVolume = 11,
    RadioTuning = 12,
    Food = 13,
    Paint = 14,
}
#[derive(Clone, Copy, Debug, Default)]
pub struct ActionAvailability {
    pub actions: ActionMask,
    pub context: FeedbackContext,
    pub item: Option<ItemId>,
    pub part: Option<ItemId>,
    pub rim: Option<ItemId>,
    pub opening: Option<OpeningId>,
    pub slot: Option<SlotTarget>,
    pub(super) use_item: Option<ItemId>,
    pub(super) active_drag: bool,
    pub light_control: Option<LightControlId>,
    pub lmb_use: bool,
    pub fill_reservation: Option<OpeningId>,
    /// `ItemPrompt` bits.
    pub prompts: u8,
    pub(super) hose_cap: Option<OpeningId>,
    /// The adjust icon of the looked usable.
    pub adjust: super::crosshair::AdjustIcon,
    /// GCMMB sprite of the looked knob or cap, as a `UiReference::sprites` index.
    pub tune_icon: Option<u32>,
}
pub struct FluidFeedback<'a> {
    pub amount: f32,
    pub capacity: f32,
    pub quantities: [f32; 6],
    /// Source list order: fluid type + 1 per 4 bits, low bits first, 0 ends.
    pub order: u32,
    pub presentation: &'a FluidPresentation,
}
pub struct InteractionFeedback<'a> {
    pub actions: ActionMask,
    pub context: FeedbackContext,
    pub name: &'a str,
    pub carrying: bool,
    pub fluid: Option<FluidFeedback<'a>>,
}
impl InteractionState {
    pub fn available_actions(
        &self,
        aim: &ResolvedAim,
        player: &PlayerState,
        items: &ItemStore,
        physics: &PhysicsWorld,
        needs: crate::survival::NeedsOutput,
    ) -> ActionAvailability {
        if needs.dead || player.view(physics).ray_actions_blocked {
            return ActionAvailability::default();
        }
        let valid = |target: Option<AimTarget>| {
            target.filter(|target| {
                target.distance.is_finite()
                    && target.distance >= 0.
                    && target.distance <= self.reach
            })
        };
        let active_target = self.active_mechanism_target(player, physics);
        let use_target = active_target.or_else(|| valid(aim.use_target));
        let grab_target = valid(aim.grab_target);
        let installation_target = valid(aim.installation_target);
        let filling_target = valid(aim.filling_target);
        let focus = valid(aim.focus_target);
        let mut a = ActionAvailability::default();
        a.use_item = use_target.map(|t| t.item);
        a.active_drag = active_target.is_some_and(|t| self.mechanisms.drag_eligible(t.item));
        a.item = use_target
            .or(grab_target)
            .or(filling_target)
            .or(installation_target)
            .or(valid(aim.push_target))
            .or(focus)
            .map(|t| t.item);
        if self.held().is_none()
            && let Some(id) = self.equipment.item()
            && self.held_controls.contains_key(&id)
        {
            a.actions.0 |= ActionMask::HELD_SWITCH;
            a.context = FeedbackContext::LightSwitch;
            a.item = Some(id);
        }
        if let Some(held) = self.held() {
            a.actions.0 |=
                ActionMask::DROP | ActionMask::THROW | ActionMask::ROTATE | ActionMask::RECENTER;
            if self.can_equip_carried(player, items, physics) {
                a.actions.0 |= ActionMask::EQUIP;
            }
            if items
                .get(held)
                .is_some_and(|item| item.food.is_some() && !item.removed)
                && needs.can_eat
            {
                a.actions.0 |= ActionMask::CONSUME;
                a.context = FeedbackContext::Food;
                a.item = Some(held);
            } else if self
                .tank_rules
                .get(&held)
                .is_some_and(|tank| !tank.no_drink)
                && items
                    .get(held)
                    .and_then(|item| item.fluid.as_ref())
                    .is_some_and(|fluid| fluid.total() > 0.)
                && a.fill_reservation.is_none()
            {
                a.actions.0 |= ActionMask::DRINK;
                a.item = Some(held);
            }
            if self.held_controls.contains_key(&held) {
                a.actions.0 |= ActionMask::HELD_SWITCH;
                a.context = FeedbackContext::LightSwitch;
            }
        }
        if self
            .equipment
            .item()
            .is_some_and(|id| self.carry.can_receive_equipment(id, player, items, physics))
        {
            a.actions.0 |= if player.seated() {
                ActionMask::RELEASE_EQUIPMENT_SEATED
            } else {
                ActionMask::RELEASE_EQUIPMENT
            };
        }
        if let Some(t) = use_target {
            if let Some((id, kind)) = self.dial_knob(&t, physics) {
                use super::Knob;
                a.tune_icon = self.tune_icons.get(&id).copied();
                a.context = match kind {
                    Knob::Volume => FeedbackContext::RadioVolume,
                    Knob::Tuning => FeedbackContext::RadioTuning,
                    Knob::Dial | Knob::Switch | Knob::Crank => FeedbackContext::Mechanism,
                };
                a.actions.0 |= match kind {
                    Knob::Switch => ActionMask::SWITCH,
                    Knob::Crank => ActionMask::CRANK,
                    _ => ActionMask::TUNE | ActionMask::SCROLL,
                };
                if kind == Knob::Crank {
                    a.adjust = super::crosshair::AdjustIcon::Rotate;
                }
            }
            if self.mechanisms.contains(t.item) {
                a.context = FeedbackContext::Mechanism;
                if self.mechanisms.turn_state(t.item).is_some() {
                    a.actions.0 |= ActionMask::SWITCH;
                    if self.mechanisms.primary_switch(t.item) {
                        a.actions.0 |= ActionMask::PRIMARY_SWITCH;
                    }
                } else {
                    a.actions.0 |= ActionMask::DOOR;
                    a.adjust = self.mechanisms.adjust_icon(t.item);
                    if self.mechanisms.scroll_eligible(t.item) {
                        a.actions.0 |= ActionMask::SCROLL;
                    }
                }
            }
            if let Some(o) = self.openings.iter().find(|o| o.reference.cap == t.item) {
                a.opening = Some(o.reference.id);
                a.context = if self.supplies.contains(&o.reference.tank) {
                    FeedbackContext::Pump
                } else {
                    FeedbackContext::Cap
                };
                if o.reference.can_toggle {
                    a.actions.0 |= ActionMask::USE;
                }
                if o.reference.can_tune {
                    a.actions.0 |= ActionMask::TUNE;
                    a.tune_icon = o.reference.tune_icon;
                }
            }
            if self
                .item_seats
                .get(&t.collider)
                .is_some_and(|seat| seat.can_sit(physics))
                && !player.seated()
            {
                a.actions.0 |= ActionMask::ENTER;
                a.context = FeedbackContext::Seat;
            }
        }
        if let Some(t) = filling_target
            && let Some(o) = self.openings.iter().find(|o| o.reference.cap == t.item)
        {
            a.opening = Some(o.reference.id);
            if self.fill_pair(o, items).is_some() {
                a.fill_reservation = Some(o.reference.id);
                a.actions.0 |= ActionMask::FILL;
            }
        }
        if a.fill_reservation.is_some() {
            a.actions.0 &= !ActionMask::DRINK;
        }
        if self.held().is_none() {
            if let Some(t) = grab_target
                && matches!(self.kinds.get(&t.item), Some(ItemKind::Tire))
                && let Some(ItemLocation::Attached(rim)) = items.get(t.item).map(|i| i.location)
                && self.is_wheel(rim)
                && items
                    .get(rim)
                    .is_some_and(|item| item.location == ItemLocation::World)
                && self.carry.allows_pickup(t.item, player.seated(), true)
                && items
                    .get(rim)
                    .and_then(|item| item.body)
                    .is_some_and(|body| {
                        self.carry
                            .can_acquire(t.item, t.point, player, body, physics)
                            .is_ok()
                    })
            {
                a.actions.0 |= ActionMask::DISMOUNT;
                a.rim = Some(rim);
                a.part = Some(t.item);
                a.context = FeedbackContext::Wheel;
            } else if let Some(t) = grab_target
                && aim.grab_block.is_none()
                && let Some(item) = items.get(t.item)
            {
                a.part = Some(t.item);
                if self.is_wheel(t.item) || matches!(self.kinds.get(&t.item), Some(ItemKind::Tire))
                {
                    a.context = FeedbackContext::Wheel;
                }
                match item.location {
                    ItemLocation::Locked { .. }
                        if self.carry.allows_pickup(t.item, player.seated(), false)
                            && self.storage.locked_body(t.item).is_some_and(|body| {
                                self.carry
                                    .can_acquire(t.item, t.point, player, body, physics)
                                    .is_ok()
                            }) =>
                    {
                        a.actions.0 |= ActionMask::PICKUP;
                    }
                    ItemLocation::PartSlot(_)
                        if self.parts.can_remove(t.item, player, items).is_ok()
                            && self.parts.loose_body(t.item).is_some_and(|body| {
                                self.carry
                                    .can_acquire(t.item, t.point, player, body, physics)
                                    .is_ok()
                            }) =>
                    {
                        a.actions.0 |= ActionMask::DISMOUNT;
                        if use_target.is_none() {
                            a.context = FeedbackContext::Part;
                        }
                    }
                    ItemLocation::WheelSlot(_)
                        if self.is_wheel(t.item)
                            && self.carry.allows_pickup(t.item, player.seated(), true)
                            && !self.parts.covered(t.item) =>
                    {
                        a.actions.0 |= ActionMask::DISMOUNT;
                    }
                    // A hitched grip body sleeps; picking it up detaches it first.
                    ItemLocation::World
                        if t.slot.is_none()
                            && ((self.hitch.is_hitched(t.item)
                                && self.carry.allows_pickup(t.item, player.seated(), false))
                                || item.body.is_some_and(|body| {
                                    self.carry
                                        .can_acquire(t.item, t.point, player, body, physics)
                                        .is_ok()
                                })) =>
                    {
                        a.actions.0 |= ActionMask::PICKUP;
                    }
                    _ => {}
                }
            }
        } else if let Some(held) = self.held() {
            (a.prompts, a.hose_cap) = self.hose_actions(held, filling_target, items, physics);
            if let Some(cap) = a.hose_cap {
                a.actions.0 |= ActionMask::MOUNT;
                a.opening = Some(cap);
            }
            if let Some(t) = installation_target
                && aim.installation_block.is_none()
                && let Some(slot) = t.slot
            {
                let fits = match slot {
                    SlotTarget::Part(slot) => self.parts.can_install(held, slot, items).is_ok(),
                    SlotTarget::Hitch(target) => self.hitch.can_attach(held, target, items).is_ok(),
                    SlotTarget::Wheel(_) | SlotTarget::Tire(_) => {
                        self.wheel_installation_block(aim, items).is_none()
                    }
                };
                if fits {
                    a.actions.0 |= ActionMask::MOUNT;
                    a.slot = Some(slot);
                    a.part = Some(held);
                    a.rim = self.is_wheel(held).then_some(held);
                }
            }
        }
        if self.push.gripping() {
            a.actions.0 |= ActionMask::RELEASE_GRIP;
            a.context = FeedbackContext::Push;
        } else if self.held().is_none()
            && let Some(t) = valid(aim.push_target)
            && self.push.can_start(t, &self.bodies, player, physics)
        {
            a.actions.0 |= if self
                .collider_bindings
                .get(&t.collider)
                .is_some_and(|b| self.push.is_grip(t.item, b.collider))
            {
                ActionMask::GRIP
            } else {
                ActionMask::PUSH
            };
            a.context = FeedbackContext::Push;
        }
        let drink_guard = self.held().is_some_and(|held| {
            self.tank_rules
                .get(&held)
                .is_some_and(|tank| !tank.no_drink)
        });
        let latched_fill = self
            .fill_opening
            .and_then(|id| self.openings.iter().find(|o| o.reference.id == id))
            .is_some_and(|opening| self.fill_pair(opening, items).is_some());
        let food_guard = self.held().is_some_and(|held| {
            items
                .get(held)
                .is_some_and(|item| item.food.is_some() && !item.removed)
        });
        a.lmb_use = a.fill_reservation.is_none()
            && !latched_fill
            && !drink_guard
            && !food_guard
            && !self.reserves_lmb();
        if self.latched_mechanism.is_none()
            && let Some(t) = aim.light_target
            && t.distance.is_finite()
            && t.distance >= 0.
            && t.distance <= self.reach
            && self.light_controls.get(&t.collider) == Some(&t.control)
            && self.light_nodes.get(&t.collider) == Some(&t.node)
            && !(self.seat_action_latched && self.usable_light_controls.contains(&t.control))
        {
            a.light_control = Some(t.control);
            a.use_item = None;
            a.actions.0 |= ActionMask::USE;
            a.actions.0 &= !(ActionMask::DOOR
                | ActionMask::SWITCH
                | ActionMask::PRIMARY_SWITCH
                | ActionMask::SCROLL);
            a.adjust = super::crosshair::AdjustIcon::None;
            a.context = FeedbackContext::LightSwitch;
        }
        if player.seated()
            && a.actions.0
                & (ActionMask::USE | ActionMask::DOOR | ActionMask::TUNE | ActionMask::SWITCH)
                == 0
            && player.can_exit(physics)
        {
            a.actions.0 |= ActionMask::EXIT;
        }
        if self.carry.throw_charge() >= self.carry.reference_throw_minimum() {
            a.actions.0 &= !ActionMask::MOUNT;
        }
        if !a.active_drag && a.actions.0 & ActionMask::MOUNT != 0 {
            a.actions.0 &= !(ActionMask::DROP | ActionMask::THROW);
        }
        if let Some(held) = self.equipment.item().filter(|_| {
            self.cleaning
                .reserves_lmb(self.equipment.item(), self.held())
        }) {
            a.item = Some(held);
            a.actions.0 &= !(ActionMask::DOOR
                | ActionMask::FILL
                | ActionMask::PRIMARY_SWITCH
                | ActionMask::HELD_SWITCH);
            match self.cleaning.prompt(held) {
                Some(185) => a.actions.0 |= ActionMask::WASH,
                Some(186) => a.actions.0 |= ActionMask::SCRUB,
                _ => {}
            }
        }
        if self
            .binoculars
            .reserves_lmb(self.equipment.item(), self.held())
        {
            a.prompts |= ItemPrompt::LOOK;
            if self.binoculars.active().is_some() {
                a.prompts |= ItemPrompt::FOCUS;
            }
        }
        if a.active_drag {
            a.actions.0 &= !(ActionMask::PICKUP
                | ActionMask::MOUNT
                | ActionMask::DISMOUNT
                | ActionMask::PUSH
                | ActionMask::GRIP);
        }
        a
    }
    pub fn paint_feedback(
        &self,
        aim: &ResolvedAim,
        reference: &ReferenceManifest,
    ) -> Option<(f32, f32)> {
        let item = aim.focus_target?.item;
        let tool = self.cleaning.spray_tool(item, reference)?;
        if tool.hide_look_at_count || (self.held() == Some(item) && tool.hide_carried_count) {
            return None;
        }
        Some((self.cleaning.get(item)?.quantity?, tool.maximum_quantity))
    }
    pub fn feedback<'a>(
        &'a self,
        a: &ActionAvailability,
        items: &'a ItemStore,
        reference: &'a ReferenceManifest,
    ) -> InteractionFeedback<'a> {
        let name = a
            .item
            .map_or("", |id| labels::display_name(reference, items, id));
        // Generated world caps have no manifest opening record, so read the live opening.
        let fluid = a
            .opening
            .and_then(|id| self.openings.iter().find(|o| o.reference.id == id))
            .and_then(|o| {
                items
                    .get(o.reference.tank)
                    .and_then(|i| i.fluid.as_ref())
                    .map(|f| FluidFeedback {
                        amount: f.total(),
                        capacity: f.capacity_liters,
                        quantities: f.mixture().quantities_liters,
                        order: fluid_order(f),
                        presentation: &o.reference.presentation,
                    })
            });
        InteractionFeedback {
            actions: a.actions,
            context: if a
                .item
                .is_some_and(|id| self.cleaning.spray_tool(id, reference).is_some())
            {
                FeedbackContext::Paint
            } else {
                a.context
            },
            name,
            carrying: self.held().is_some(),
            fluid,
        }
    }
}

/// Source `GetCapString` lists percentages in the tank's fluid list order.
fn fluid_order(fluid: &crate::fluids::FluidContainer) -> u32 {
    fluid
        .entries()
        .enumerate()
        .fold(0, |order, (position, entry)| {
            order | (u32::from(entry.fluid_type) + 1) << (4 * position)
        })
}
