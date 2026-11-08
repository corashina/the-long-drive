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

impl QueryAction {
    const ALL: [Self; 7] = [
        Self::Use,
        Self::Grab,
        Self::Removal,
        Self::Install,
        Self::Fill,
        Self::Push,
        Self::Focus,
    ];
}

/// The nearest aim ray hit of one query: collider and distance.
type RayHit = Option<(ColliderHandle, f32)>;

/// The nearest hit of each query on one aim ray.
#[derive(Default)]
struct AimHits {
    light: RayHit,
    help: RayHit,
    actions: [RayHit; QueryAction::ALL.len()],
}

/// Keeps `hit` in `best` when it is nearer and `accepts` holds.
fn keep_nearest(best: &mut RayHit, hit: (ColliderHandle, f32), accepts: impl FnOnce() -> bool) {
    if best.is_none_or(|(_, distance)| hit.1 < distance) && accepts() {
        *best = Some(hit);
    }
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
        let hits = self.cast_aim(physics, &ray, items);
        let action = |action: QueryAction| {
            hits.actions[action as usize]
                .and_then(|hit| self.action_target(physics, &ray, items, action, hit))
        };
        let mut aim = ResolvedAim {
            light_target: hits
                .light
                .and_then(|hit| self.light_target(physics, &ray, hit)),
            use_target: action(QueryAction::Use),
            grab_target: action(QueryAction::Grab),
            installation_target: action(QueryAction::Install),
            filling_target: action(QueryAction::Fill),
            push_target: action(QueryAction::Push),
            focus_target: action(QueryAction::Focus),
            help: hits
                .help
                .map_or(0, |(handle, _)| self.help_texts.get(handle)),
            ..ResolvedAim::default()
        };
        if let Some(removal) = action(QueryAction::Removal)
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

    /// Casts the aim ray once. Each query keeps its nearest hit that passes its own surface test.
    /// This is the hit that a separate cast with that test as its filter gives.
    fn cast_aim(&self, physics: &PhysicsWorld, ray: &Ray, items: &ItemStore) -> AimHits {
        let held_body = self.carry.body();
        let open = |_: ColliderHandle, collider: &rapier3d::prelude::Collider| {
            collider.is_enabled()
                && (held_body.is_none() || collider.parent() != held_body)
                && (self.player_body.is_none() || collider.parent() != self.player_body)
                && !collider
                    .shape()
                    .contains_point(collider.position(), ray.origin)
        };
        let use_mask = self.action_masks.use_mask;
        let mut hits = AimHits::default();
        for (handle, collider, hit) in physics
            .queries(QueryFilter::default().predicate(&open))
            .intersect_ray(*ray, self.reach, true)
        {
            let hit = (handle, hit.time_of_impact);
            let used = collider.collision_groups().memberships.bits() & use_mask != 0;
            keep_nearest(&mut hits.light, hit, || {
                let id = ColliderId((collider.user_data >> 64) as u32);
                used && (!collider.is_sensor() || self.light_controls.contains_key(&id))
            });
            // fpscontroller: the use ray hit with `useLayer`, and its parent `helptextscript`.
            keep_nearest(&mut hits.help, hit, || used && !collider.is_sensor());
            for action in QueryAction::ALL {
                keep_nearest(&mut hits.actions[action as usize], hit, || {
                    self.action_surface(physics, items, action, handle, collider)
                });
            }
        }
        hits
    }

    fn light_target(
        &self,
        physics: &PhysicsWorld,
        ray: &Ray,
        (handle, distance): (ColliderHandle, f32),
    ) -> Option<LightAim> {
        let collider = ColliderId((physics.rapier().colliders[handle].user_data >> 64) as u32);
        Some(LightAim {
            node: *self.light_nodes.get(&collider)?,
            collider,
            control: *self.light_controls.get(&collider)?,
            distance,
            point: ray.point_at(distance).to_array(),
        })
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

    /// `GetComponentInParent<usablescript>` of a collider for `newAiScript`: the usable, its
    /// position, `rotateAble` and `slideAble`.
    pub fn ai_usable(
        &self,
        handle: ColliderHandle,
        physics: &PhysicsWorld,
    ) -> Option<crate::creatures::human_ai::Usable> {
        let item = self.collider_bindings.get(&handle)?.use_owner?;
        if !self.supports_use(item) {
            return None;
        }
        let m = &self.mechanisms;
        let position = m
            .item_node(item)
            .and_then(|node| m.node_pose(node))
            .map(|pose| to_rapier(pose).translation)
            .unwrap_or_else(|| physics.rapier().colliders[handle].translation());
        Some(crate::creatures::human_ai::Usable {
            item,
            position,
            rotate: m.rotation_degrees(item).is_some(),
            slide: m.slideable(item),
        })
    }

    fn supports_use(&self, item: ItemId) -> bool {
        self.usable_items.contains(&item) || self.mechanisms.contains(item)
    }

    fn action_mask(&self, action: QueryAction) -> u32 {
        let masks = self.action_masks;
        match action {
            QueryAction::Grab => masks.pickup_mask,
            QueryAction::Install => masks.attach_mask,
            QueryAction::Push => masks.push_mask,
            _ => masks.use_mask,
        }
    }

    /// The item that `action` takes on a collider.
    fn action_owner(
        &self,
        physics: &PhysicsWorld,
        items: &ItemStore,
        action: QueryAction,
        handle: ColliderHandle,
        collider: &rapier3d::prelude::Collider,
    ) -> Option<ItemId> {
        let binding = self.collider_bindings.get(&handle);
        match action {
            QueryAction::Use => binding
                .and_then(|binding| binding.use_owner)
                .filter(|id| self.supports_use(*id))
                .or_else(|| self.controls.owner(handle, collider)),
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
                                        && items
                                            .get(rim)
                                            .is_some_and(|rim| rim.location == ItemLocation::World)
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
            QueryAction::Install if self.held().is_some_and(|id| self.hitch.is_grip(id)) => self
                .hitch
                .collider_target(ColliderId((collider.user_data >> 64) as u32))
                .and_then(|target| self.hitch.target_owner(target)),
            QueryAction::Install => self.tire_slot_owner(handle, physics),
            QueryAction::Focus => self
                .tire_slot_owner(handle, physics)
                .map(|rim| self.attached_tire(rim, items).unwrap_or(rim))
                .or_else(|| binding.and_then(|binding| binding.use_owner))
                .or_else(|| (collider.user_data as u32).checked_sub(1).map(ItemId)),
        }
    }

    /// Whether `action` may take a collider by its layer.
    fn action_eligible(
        &self,
        action: QueryAction,
        handle: ColliderHandle,
        collider: &rapier3d::prelude::Collider,
    ) -> bool {
        let mask = self.action_mask(action);
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
    }

    /// Whether the ray of `action` stops at a collider: a visible solid, or a collider it takes.
    fn action_surface(
        &self,
        physics: &PhysicsWorld,
        items: &ItemStore,
        action: QueryAction,
        handle: ColliderHandle,
        collider: &rapier3d::prelude::Collider,
    ) -> bool {
        let masks = self.action_masks;
        let visible = masks.use_mask | masks.pickup_mask | masks.attach_mask | masks.push_mask;
        (!collider.is_sensor() && collider.collision_groups().memberships.bits() & visible != 0)
            || (self.action_eligible(action, handle, collider)
                && self
                    .action_owner(physics, items, action, handle, collider)
                    .is_some())
    }

    /// The target of `action` at its nearest ray hit.
    fn action_target(
        &self,
        physics: &PhysicsWorld,
        ray: &Ray,
        items: &ItemStore,
        action: QueryAction,
        (handle, distance): (ColliderHandle, f32),
    ) -> Option<AimTarget> {
        let collider = &physics.rapier().colliders[handle];
        if !self.action_eligible(action, handle, collider) {
            return None;
        }
        let item = self.action_owner(physics, items, action, handle, collider)?;
        // Another player holds or sits on it. The hand can take it from the hands of that player.
        if self.in_use.item(item).is_some()
            && !(matches!(action, QueryAction::Grab) && self.in_use.hand(item).is_some())
        {
            return None;
        }
        Some(AimTarget {
            collider: handle,
            item,
            distance,
            point: ray.point_at(distance).to_array(),
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
