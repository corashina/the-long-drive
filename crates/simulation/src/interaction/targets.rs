//! Collider identity and source action owners.
use super::items::{ItemLocation, ItemStore};
use super::{AimTarget, InteractionState, SLOT_SENSOR_FLAG, SlotTarget};
use crate::reference::{ActionBinding, ColliderId, ItemId, ItemKind, LightControlId, NodeId};
use crate::vehicle::{PhysicsWorld, VehiclePose, to_rapier};
use rapier3d::math::Vector;
use rapier3d::prelude::{ColliderHandle, QueryFilter, Ray};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InteractionBlock {
    Covered,
    Incompatible,
    Occupied,
    Unavailable,
}

impl InteractionBlock {
    pub fn notice(self) -> &'static str {
        match self {
            Self::Covered => "Remove the cover first.",
            Self::Incompatible => "This part does not fit.",
            Self::Occupied => "This slot is occupied.",
            Self::Unavailable => "Cannot use this item.",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LightAim {
    pub node: NodeId,
    pub collider: ColliderId,
    pub distance: f32,
    pub point: [f32; 3],
    pub control: LightControlId,
}
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct ResolvedAim {
    pub light_target: Option<LightAim>,
    pub use_target: Option<AimTarget>,
    pub grab_target: Option<AimTarget>,
    pub installation_target: Option<AimTarget>,
    pub filling_target: Option<AimTarget>,
    pub push_target: Option<AimTarget>,
    pub focus_target: Option<AimTarget>,
    pub grab_block: Option<InteractionBlock>,
    pub installation_block: Option<InteractionBlock>,
    /// `help_texts` bits of the help box that the use ray hits first.
    pub help: u8,
}

impl ResolvedAim {
    pub fn same_actions(&self, other: &Self) -> bool {
        let identity = |target: Option<AimTarget>| {
            target.map(|target| (target.collider, target.item, target.slot))
        };
        self.light_target.map(|t| (t.collider, t.control))
            == other.light_target.map(|t| (t.collider, t.control))
            && identity(self.use_target) == identity(other.use_target)
            && identity(self.grab_target) == identity(other.grab_target)
            && identity(self.installation_target) == identity(other.installation_target)
            && identity(self.filling_target) == identity(other.filling_target)
            && identity(self.push_target) == identity(other.push_target)
            && identity(self.focus_target) == identity(other.focus_target)
            && self.grab_block == other.grab_block
            && self.installation_block == other.installation_block
            && self.help == other.help
    }
    pub fn rebase(&mut self, shift: [f32; 3]) {
        if let Some(target) = &mut self.light_target {
            for (point, shift) in target.point.iter_mut().zip(shift) {
                *point -= shift;
            }
        }
        for target in [
            &mut self.use_target,
            &mut self.grab_target,
            &mut self.installation_target,
            &mut self.filling_target,
            &mut self.push_target,
            &mut self.focus_target,
        ]
        .into_iter()
        .flatten()
        {
            for (point, shift) in target.point.iter_mut().zip(shift) {
                *point -= shift;
            }
        }
    }
}

#[derive(Clone, Copy)]
enum QueryAction {
    Use,
    Grab,
    Removal,
    Install,
    Fill,
    Push,
    Focus,
}

impl InteractionState {
    pub fn bind_collider(&mut self, id: ColliderId, handle: ColliderHandle) {
        self.cleaning.bind_collider(id, handle);
        if let Some(binding) = self.source_bindings.get(&id) {
            self.collider_bindings.insert(handle, *binding);
        }
    }

    /// The use owner bound to a collider, such as a tank cap.
    pub fn use_owner(&self, handle: ColliderHandle) -> Option<ItemId> {
        self.collider_bindings.get(&handle)?.use_owner
    }

    pub fn bind_surface(&mut self, handle: ColliderHandle, binding: ActionBinding) {
        if self.collider_bindings.get(&handle) == Some(&binding) {
            return;
        }
        if binding.collider.0 == 0
            && binding
                .use_owner
                .is_some_and(|item| self.supplies.contains(&item))
        {
            self.collider_bindings.retain(|_, previous| {
                previous.collider.0 != 0
                    || previous.use_owner != binding.use_owner
                    || previous.pickup_owner != binding.pickup_owner
            });
        }
        self.collider_bindings.insert(handle, binding);
    }

    pub fn bind_wheel_sensor(
        &mut self,
        handle: ColliderHandle,
        rim: ItemId,
        physics: &mut PhysicsWorld,
    ) {
        let layers = self.removal_layers.get(&rim).copied().unwrap_or(0);
        let binding = ActionBinding {
            collider: ColliderId(0),
            use_owner: None,
            pickup_owner: None,
            removal_owner: (layers != 0).then_some(rim),
            push_owner: None,
        };
        if self.collider_bindings.get(&handle) == Some(&binding) {
            return;
        }
        physics.edit_collider(handle, |collider| {
            let mut groups = collider.collision_groups();
            groups.memberships = rapier3d::prelude::Group::from_bits_truncate(layers);
            collider.set_collision_groups(groups);
        });
        self.bind_surface(handle, binding);
    }

    pub fn resolve(
        &self,
        physics: &PhysicsWorld,
        view: VehiclePose,
        items: &ItemStore,
    ) -> ResolvedAim {
        if !self.reach.is_finite() || self.reach <= 0.0 {
            return ResolvedAim::default();
        }
        let view = to_rapier(view);
        let ray = Ray::new(view.translation, view.rotation * -Vector::Z);
        let mut aim = ResolvedAim {
            light_target: self.query_light(physics, &ray),
            use_target: self.query_action(physics, &ray, items, QueryAction::Use),
            grab_target: self.query_action(physics, &ray, items, QueryAction::Grab),
            installation_target: self.query_action(physics, &ray, items, QueryAction::Install),
            filling_target: self.query_action(physics, &ray, items, QueryAction::Fill),
            push_target: self.query_action(physics, &ray, items, QueryAction::Push),
            focus_target: self.query_action(physics, &ray, items, QueryAction::Focus),
            help: self.query_help(physics, &ray),
            ..ResolvedAim::default()
        };
        if let Some(removal) = self.query_action(physics, &ray, items, QueryAction::Removal)
            && aim
                .grab_target
                .is_none_or(|pickup| removal.distance <= pickup.distance)
        {
            aim.grab_target = Some(removal);
        }
        if let Some(target) = aim.grab_target {
            let available = items
                .get(target.item)
                .is_some_and(|item| match item.location {
                    ItemLocation::WheelSlot(_) => {
                        self.is_wheel(target.item)
                            && self.carry.allows_pickup(target.item, false, true)
                    }
                    ItemLocation::PartSlot(_) => self.carry.allows_pickup(target.item, false, true),
                    ItemLocation::Attached(parent)
                        if matches!(self.kinds.get(&target.item), Some(ItemKind::Tire))
                            && self.is_wheel(parent)
                            && items
                                .get(parent)
                                .is_some_and(|item| item.location == ItemLocation::World) =>
                    {
                        self.carry.allows_pickup(target.item, false, true)
                    }
                    ItemLocation::Locked { .. } => {
                        self.carry.allows_pickup(target.item, false, false)
                            && self.storage.locked_body(target.item).is_some()
                    }
                    ItemLocation::World => {
                        self.carry.allows_pickup(target.item, false, false)
                            && item.body.is_some_and(|body| {
                                physics.vehicle_of(body).is_none()
                                    && physics
                                        .rapier()
                                        .bodies
                                        .get(body)
                                        .is_some_and(|body| body.is_dynamic())
                            })
                    }
                    _ => false,
                });
            if items.get(target.item).is_some_and(|item| {
                matches!(
                    item.location,
                    ItemLocation::WheelSlot(_) | ItemLocation::PartSlot(_)
                )
            }) && self.parts.covered(target.item)
            {
                aim.grab_target = None;
                aim.grab_block = Some(InteractionBlock::Covered);
            } else if !available {
                aim.grab_target = None;
                aim.grab_block = Some(InteractionBlock::Unavailable);
            }
        }
        if let Some(item) = self.held().filter(|item| self.parts.contains(*item)) {
            aim.installation_target = self.parts.scan(item, items, physics);
        } else {
            aim.installation_block = self.wheel_installation_block(&aim, items);
        }
        if let Some(SlotTarget::Part(slot)) = aim.installation_target.and_then(|t| t.slot)
            && let Some(item) = self.held()
        {
            aim.installation_block = self.parts.can_install(item, slot, items).err();
        }
        if aim.installation_block.is_some() {
            aim.installation_target = None;
        }
        if self.held().is_some()
            || aim.light_target.is_some()
            || aim.use_target.is_some()
            || aim.grab_target.is_some()
            || aim.installation_target.is_some()
            || aim.filling_target.is_some()
        {
            aim.push_target = None;
        }
        aim
    }

    pub(super) fn wheel_installation_block(
        &self,
        aim: &ResolvedAim,
        items: &ItemStore,
    ) -> Option<InteractionBlock> {
        aim.installation_block.or_else(|| {
            let held = self.held()?;
            match aim.installation_target.and_then(|t| t.slot)? {
                SlotTarget::Wheel(slot) => {
                    if !self.is_wheel(held)
                        || !self
                            .wheel_slots
                            .get(&slot)
                            .is_some_and(|rule| rule.fits.contains(&held))
                    {
                        Some(InteractionBlock::Incompatible)
                    } else if self.parts.covered(held) {
                        Some(InteractionBlock::Covered)
                    } else if items
                        .iter()
                        .any(|item| item.location == ItemLocation::WheelSlot(slot))
                    {
                        Some(InteractionBlock::Occupied)
                    } else {
                        None
                    }
                }
                SlotTarget::Tire(rim) => {
                    if !self.tire_fits.contains(&(rim, held)) {
                        Some(InteractionBlock::Incompatible)
                    } else if items
                        .get(rim)
                        .is_none_or(|item| item.location != ItemLocation::World)
                    {
                        Some(InteractionBlock::Unavailable)
                    } else if self.attached_tire(rim, items).is_some() {
                        Some(InteractionBlock::Occupied)
                    } else {
                        None
                    }
                }
                SlotTarget::Part(_) | SlotTarget::Hitch(_) => None,
            }
        })
    }

    pub(super) fn is_wheel(&self, item: ItemId) -> bool {
        matches!(self.kinds.get(&item), Some(ItemKind::Wheel | ItemKind::Rim))
    }

    fn query_light(&self, physics: &PhysicsWorld, ray: &Ray) -> Option<LightAim> {
        let mask = self.action_masks.use_mask;
        let held_body = self.carry.body();
        let surfaces = |_: ColliderHandle, collider: &rapier3d::prelude::Collider| {
            let id = ColliderId((collider.user_data >> 64) as u32);
            collider.is_enabled()
                && collider.collision_groups().memberships.bits() & mask != 0
                && (held_body.is_none() || collider.parent() != held_body)
                && (self.player_body.is_none() || collider.parent() != self.player_body)
                && (!collider.is_sensor() || self.light_controls.contains_key(&id))
                && !collider
                    .shape()
                    .contains_point(collider.position(), ray.origin)
        };
        let (handle, hit) = physics
            .queries(QueryFilter::default().predicate(&surfaces))
            .cast_ray_and_get_normal(ray, self.reach, true)?;
        let collider = ColliderId((physics.rapier().colliders[handle].user_data >> 64) as u32);
        Some(LightAim {
            node: *self.light_nodes.get(&collider)?,
            collider,
            control: *self.light_controls.get(&collider)?,
            distance: hit.time_of_impact,
            point: ray.point_at(hit.time_of_impact).to_array(),
        })
    }

    /// fpscontroller: the use ray hit with `useLayer`, and its parent `helptextscript`.
    fn query_help(&self, physics: &PhysicsWorld, ray: &Ray) -> u8 {
        let held_body = self.carry.body();
        let surfaces = |_: ColliderHandle, collider: &rapier3d::prelude::Collider| {
            collider.is_enabled()
                && !collider.is_sensor()
                && (held_body.is_none() || collider.parent() != held_body)
                && (self.player_body.is_none() || collider.parent() != self.player_body)
                && collider.collision_groups().memberships.bits() & self.action_masks.use_mask != 0
                && !collider
                    .shape()
                    .contains_point(collider.position(), ray.origin)
        };
        physics
            .queries(QueryFilter::default().predicate(&surfaces))
            .cast_ray_and_get_normal(ray, self.reach, true)
            .map_or(0, |(handle, _)| self.help_texts.get(handle))
    }

    fn tire_slot_owner(&self, handle: ColliderHandle, physics: &PhysicsWorld) -> Option<ItemId> {
        let binding = self.collider_bindings.get(&handle)?;
        let rim = binding.pickup_owner?;
        (physics.rapier().colliders[handle].is_sensor()
            && self.is_wheel(rim)
            && binding
                .removal_owner
                .is_some_and(|item| matches!(self.kinds.get(&item), Some(ItemKind::Tire))))
        .then_some(rim)
    }

    fn attached_tire(&self, rim: ItemId, items: &ItemStore) -> Option<ItemId> {
        items
            .iter()
            .find(|item| {
                item.location == ItemLocation::Attached(rim)
                    && matches!(self.kinds.get(&item.id), Some(ItemKind::Tire))
            })
            .map(|item| item.id)
    }

    fn supports_use(&self, item: ItemId) -> bool {
        self.usable_items.contains(&item) || self.mechanisms.contains(item)
    }

    fn query_action(
        &self,
        physics: &PhysicsWorld,
        ray: &Ray,
        items: &ItemStore,
        action: QueryAction,
    ) -> Option<AimTarget> {
        let masks = self.action_masks;
        let mask = match action {
            QueryAction::Grab => masks.pickup_mask,
            QueryAction::Install => masks.attach_mask,
            QueryAction::Push => masks.push_mask,
            _ => masks.use_mask,
        };
        let visible = masks.use_mask | masks.pickup_mask | masks.attach_mask | masks.push_mask;
        let held_body = self.carry.body();
        let owner = |handle: ColliderHandle, collider: &rapier3d::prelude::Collider| {
            let binding = self.collider_bindings.get(&handle);
            match action {
                QueryAction::Use => binding
                    .and_then(|binding| binding.use_owner)
                    .filter(|id| self.supports_use(*id))
                    .or_else(|| self.item_seats.get(&handle).map(|seat| seat.item)),
                QueryAction::Fill => binding.and_then(|binding| binding.use_owner).filter(|id| {
                    self.filling_items.contains(id)
                        && self.held().is_some()
                        && self
                            .openings
                            .iter()
                            .any(|opening| opening.reference.cap == *id && opening.valve > 0.0)
                }),
                QueryAction::Grab | QueryAction::Removal => binding
                    .and_then(|binding| {
                        if matches!(action, QueryAction::Removal) {
                            if let Some(rim) = self.tire_slot_owner(handle, physics) {
                                return self.attached_tire(rim, items);
                            }
                            self.parts
                                .slot_target(binding.collider)
                                .map(|s| self.parts.slot_item(s))
                                .unwrap_or(binding.removal_owner)
                        } else {
                            binding.pickup_owner
                        }
                    })
                    .map(|id| {
                        if matches!(action, QueryAction::Grab)
                            && matches!(self.kinds.get(&id), Some(ItemKind::Tire))
                            && let Some(ItemLocation::Attached(parent)) =
                                items.get(id).map(|item| item.location)
                            && self.is_wheel(parent)
                        {
                            parent
                        } else if matches!(action, QueryAction::Removal)
                            && items.get(id).is_some_and(|item| {
                                matches!(
                                    item.location,
                                    ItemLocation::WheelSlot(_) | ItemLocation::PartSlot(_)
                                )
                            })
                        {
                            self.parts.covering_item(id).unwrap_or(id)
                        } else {
                            id
                        }
                    })
                    .filter(|id| {
                        !matches!(action, QueryAction::Removal)
                            || (matches!(self.kinds.get(id), Some(ItemKind::Tire))
                                && items.get(*id).is_some_and(|item| match item.location {
                                    ItemLocation::Attached(rim) => {
                                        self.is_wheel(rim)
                                            && items.get(rim).is_some_and(|rim| {
                                                rim.location == ItemLocation::World
                                            })
                                    }
                                    _ => false,
                                }))
                            || (self.parts.contains(*id)
                                && matches!(
                                    items.get(*id).map(|i| i.location),
                                    Some(ItemLocation::PartSlot(_))
                                ))
                            || (self.is_wheel(*id)
                                && items.get(*id).is_some_and(|item| {
                                    matches!(item.location, ItemLocation::WheelSlot(_))
                                }))
                    }),
                QueryAction::Push => binding.and_then(|binding| binding.push_owner).filter(|id| {
                    self.push.supports(*id)
                        && self.parts.slot_target(binding.unwrap().collider).is_none()
                        && collider.user_data & SLOT_SENSOR_FLAG == 0
                }),
                QueryAction::Install if collider.user_data & SLOT_SENSOR_FLAG != 0 => {
                    (collider.user_data as u32).checked_sub(1).map(ItemId)
                }
                QueryAction::Install if self.held().is_some_and(|id| self.hitch.is_grip(id)) => {
                    self.hitch
                        .collider_target(ColliderId((collider.user_data >> 64) as u32))
                        .and_then(|target| self.hitch.target_owner(target))
                }
                QueryAction::Install => self.tire_slot_owner(handle, physics),
                QueryAction::Focus => self
                    .tire_slot_owner(handle, physics)
                    .map(|rim| self.attached_tire(rim, items).unwrap_or(rim))
                    .or_else(|| binding.and_then(|binding| binding.use_owner))
                    .or_else(|| (collider.user_data as u32).checked_sub(1).map(ItemId)),
            }
        };
        let eligible = |handle: ColliderHandle, collider: &rapier3d::prelude::Collider| {
            collider.collision_groups().memberships.bits() & mask != 0
                || (matches!(action, QueryAction::Grab)
                    && self
                        .collider_bindings
                        .get(&handle)
                        .and_then(|binding| binding.pickup_owner)
                        .is_some_and(|id| {
                            self.pickup_layers
                                .get(&id)
                                .is_some_and(|layers| layers & mask != 0)
                        }))
        };
        let surfaces = |handle: ColliderHandle, collider: &rapier3d::prelude::Collider| {
            collider.is_enabled()
                && (held_body.is_none() || collider.parent() != held_body)
                && (self.player_body.is_none() || collider.parent() != self.player_body)
                && !collider
                    .shape()
                    .contains_point(collider.position(), ray.origin)
                && ((!collider.is_sensor()
                    && collider.collision_groups().memberships.bits() & visible != 0)
                    || (eligible(handle, collider) && owner(handle, collider).is_some()))
        };
        let (handle, hit) = physics
            .queries(QueryFilter::default().predicate(&surfaces))
            .cast_ray_and_get_normal(ray, self.reach, true)?;
        let collider = &physics.rapier().colliders[handle];
        if !eligible(handle, collider) {
            return None;
        }
        let item = owner(handle, collider)?;
        Some(AimTarget {
            collider: handle,
            item,
            distance: hit.time_of_impact,
            point: ray.point_at(hit.time_of_impact).to_array(),
            slot: (collider.user_data & SLOT_SENSOR_FLAG != 0)
                .then_some(SlotTarget::Wheel(super::wheels::slot_id(
                    collider.user_data,
                )))
                .or_else(|| self.tire_slot_owner(handle, physics).map(SlotTarget::Tire))
                .or_else(|| {
                    self.parts
                        .slot_target(ColliderId((collider.user_data >> 64) as u32))
                        .map(SlotTarget::Part)
                })
                .or_else(|| {
                    self.hitch
                        .collider_target(ColliderId((collider.user_data >> 64) as u32))
                        .filter(|_| {
                            matches!(action, QueryAction::Install)
                                && self.held().is_some_and(|id| self.hitch.is_grip(id))
                        })
                        .map(SlotTarget::Hitch)
                }),
        })
    }
}
