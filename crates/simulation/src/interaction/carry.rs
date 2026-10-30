//! Source carry pose and reusable collision constraint.
use super::items::{ItemLocation, ItemStore};
use crate::player::PlayerState;
use crate::reference::{
    ItemId, PickupReference, PlayerReference, ReferenceError, ReferenceManifest,
};
use crate::simulation::{FrameInput, keys};
use crate::vehicle::{PhysicsWorld, to_rapier};
use rapier3d::math::{Pose, Rotation, Vector};
use rapier3d::parry::query::ShapeCastOptions;
use rapier3d::prelude::{
    ColliderHandle, FixedJointBuilder, ImpulseJointHandle, QueryFilter, RigidBodyBuilder,
    RigidBodyHandle,
};
use std::collections::HashMap;

struct HeldState {
    item: ItemId,
    body: RigidBodyHandle,
    offset: Vector,
    // Relative to the Player root for noRot, otherwise relative to the view.
    rotation: Rotation,
    joint: ImpulseJointHandle,
    local_pose: Pose,
    gravity_scale: f32,
    /// Source `pickedUpPoint` in the item body frame. `None` follows `TpickedUpParent`.
    grip: Option<Pose>,
}

pub struct CarryState {
    reference: PlayerReference,
    pub(super) settings: crate::reference::PlayerSettings,
    rules: HashMap<ItemId, PickupReference>,
    held: Option<HeldState>,
    anchor: Option<RigidBodyHandle>,
    acquisition_held: bool,
    throw_charge: f32,
    throwing: bool,
    charge_falling: bool,
    charge_stopped: bool,
}

// Source 17488 keeps RMB as an override and always excludes MMB rotation.
pub(super) fn wheel_allowed(input: FrameInput, tuning_available: bool, looking: bool) -> bool {
    ((!tuning_available && !looking) || input.held & keys::TRANSLATE != 0)
        && input.held & keys::ROTATE == 0
}

/// Source `TpickedUpParent`: the held item frame the anchor body follows.
fn anchor_pose(
    reference: &PlayerReference,
    held: &HeldState,
    view: Pose,
    rotation_frame: Rotation,
) -> Pose {
    Pose::from_parts(
        view.translation
            + view.rotation
                * (Vector::from_array(reference.carry_anchor_offset) + held.offset)
                    .clamp_length(reference.carry_min_distance, reference.carry_max_distance),
        rotation_frame * held.rotation,
    )
}

impl CarryState {
    pub fn restore_save(
        &mut self,
        saved: &crate::save::CarrySnapshot,
        ids: &crate::save::IdentityMap,
        items: &ItemStore,
        physics: &mut PhysicsWorld,
    ) -> Result<(), crate::save::SaveError> {
        let item = ids.item(&saved.item)?;
        if self.rules.get(&item).is_none_or(|r| r.cant_pickup) {
            return Err(crate::save::SaveError::InvalidLinks(
                "Saved item cannot be held".into(),
            ));
        }
        let body = items.get(item).and_then(|i| i.body).ok_or_else(|| {
            crate::save::SaveError::ReconstructionFailure("Missing held body".into())
        })?;
        if !physics.body_active(body) || !physics.rapier().bodies[body].is_dynamic() {
            return Err(crate::save::SaveError::ReconstructionFailure(
                "Held body is inactive or fixed".into(),
            ));
        }
        let gravity_scale = physics.rapier().bodies[body].gravity_scale();
        physics.set_body_mass_override(body, Some(1.0));
        let pose = *physics.rapier().bodies[body].position();
        let anchor = physics.insert_body(RigidBodyBuilder::kinematic_position_based().pose(pose));
        let joint = physics.insert_impulse_joint(anchor, body, FixedJointBuilder::new(), false);
        self.anchor = Some(anchor);
        self.held = Some(HeldState {
            item,
            body,
            offset: Vector::from_array(saved.offset),
            // Save rotation uses the same authored frame as acquisition and step.
            rotation: Rotation::from_array(saved.rotation),
            joint,
            local_pose: to_rapier(crate::save::local_pose(&saved.local_grip)),
            gravity_scale,
            grip: None,
        });
        physics.edit_body(body, |body| {
            body.enable_ccd(true);
            body.set_gravity_scale(0.0, true);
        });
        self.cancel_input();
        Ok(())
    }
    pub(super) fn register_food(
        &mut self,
        id: ItemId,
        reference: &crate::reference::FoodReference,
    ) {
        self.rules.insert(id, reference.pickup.clone());
    }

    pub(super) fn forget_food(&mut self, id: ItemId) {
        self.rules.remove(&id);
    }
    pub(super) fn retained_binding_bytes(&self) -> usize {
        super::memory::map(&self.rules)
            + self
                .rules
                .values()
                .map(|r| r.source.file.capacity())
                .sum::<usize>()
    }

    pub fn new(reference: &ReferenceManifest) -> Result<Self, ReferenceError> {
        let player = &reference.player;
        if !player.carry_min_weight.is_finite()
            || !player.carry_max_weight.is_finite()
            || player.carry_min_weight >= player.carry_max_weight
        {
            return Err(ReferenceError("Invalid carry weight range".into()));
        }
        Ok(Self {
            reference: player.clone(),
            settings: crate::reference::PlayerSettings::defaults(reference),
            rules: reference
                .items
                .iter()
                .filter_map(|item| item.pickup.clone().map(|rule| (item.id, rule)))
                .collect(),
            held: None,
            anchor: None,
            acquisition_held: false,
            throw_charge: 0.0,
            throwing: false,
            charge_falling: false,
            charge_stopped: false,
        })
    }
    pub(super) fn register_generated(&mut self, id: ItemId, rule: PickupReference) {
        self.rules.insert(id, rule);
    }
    pub(super) fn unregister_generated(&mut self, id: ItemId) {
        self.rules.remove(&id);
    }
    pub(super) fn reference_throw_minimum(&self) -> f32 {
        self.reference.throw_minimum
    }
    pub fn throw_charge(&self) -> f32 {
        self.throw_charge
    }
    pub fn throw_active(&self) -> bool {
        self.throwing && self.held.is_some()
    }
    pub fn cancel_input(&mut self) {
        self.throw_charge = 0.0;
        self.throwing = false;
        self.charge_falling = false;
        self.charge_stopped = false;
        self.acquisition_held = self.held.is_some();
    }
    pub(super) fn begin_throw(&mut self) {
        if self.held.is_some() && !self.acquisition_held {
            self.throwing = true;
            self.throw_charge = 0.0;
            self.charge_falling = false;
            self.charge_stopped = false;
        }
    }
    pub(super) fn charge_throw(&mut self, dt: f32) {
        if !self.throwing || self.charge_stopped {
            return;
        }
        let gain = self.reference.throw_gain * dt;
        if self.charge_falling {
            self.throw_charge -= gain;
            if self.throw_charge <= self.reference.throw_minimum {
                self.charge_stopped = true;
            }
        } else {
            self.throw_charge += gain;
            if self.throw_charge >= 1.0 {
                self.charge_falling = true;
            }
        }
    }
    pub fn release_throw(
        &mut self,
        player: &PlayerState,
        items: &mut ItemStore,
        physics: &mut PhysicsWorld,
    ) -> Option<ItemId> {
        if !self.throwing {
            return None;
        }
        self.throwing = false;
        if self.charge_stopped {
            return None;
        }
        let charge = self.throw_charge;
        let body = self.body()?;
        let direction = to_rapier(player.view(physics).head_pose).rotation * -Vector::Z;
        let item = self.drop(items, physics)?;
        if charge > self.reference.throw_minimum {
            physics.edit_body(body, |body| {
                body.apply_impulse(
                    direction
                        * charge
                        * self.reference.throw_multiplier
                        * self.reference.source_force_interval,
                    true,
                )
            });
        }
        Some(item)
    }
    pub fn capture_save(
        &self,
        ids: &crate::save::IdentityMap,
    ) -> Result<Option<crate::save::CarrySnapshot>, crate::save::SaveError> {
        self.held
            .as_ref()
            .map(|held| {
                Ok(crate::save::CarrySnapshot {
                    item: ids.item_instance(held.item)?,
                    offset: held.offset.to_array(),
                    rotation: held.rotation.to_array(),
                    local_grip: crate::save::LocalPose {
                        position: held.local_pose.translation.to_array(),
                        rotation: held.local_pose.rotation.to_array(),
                    },
                })
            })
            .transpose()
    }
    pub fn rule(&self, id: ItemId) -> Option<&PickupReference> {
        self.rules.get(&id)
    }
    pub fn held(&self) -> Option<ItemId> {
        self.held.as_ref().map(|held| held.item)
    }
    pub fn body(&self) -> Option<RigidBodyHandle> {
        self.held.as_ref().map(|held| held.body)
    }
    /// Source `weightM` and `weightM2` of the carried item. `Drop` sets both to 1.
    pub fn weight_factors(&self, physics: &PhysicsWorld) -> [f32; 2] {
        let Some(held) = &self.held else {
            return [1.; 2];
        };
        let mass = physics.authored_body_mass(held.body);
        let (min, max) = (
            self.reference.carry_min_weight,
            self.reference.carry_max_weight,
        );
        [
            ((mass - max) / (min - max)).clamp(0., 1.),
            ((mass - max * 2.) / (min - max * 2.)).clamp(0., 1.),
        ]
    }
    /// Source `pickedUpPoint` follows `TpickedUpParent`, as after `Drop` or recenter.
    pub fn grip_at_anchor(&mut self) {
        if let Some(held) = self.held.as_mut() {
            held.grip = None;
        }
    }
    /// World `pickedUpPoint`, world `TpickedUpParent`, and the authored mass.
    pub fn hand_frames(
        &self,
        player: &PlayerState,
        physics: &PhysicsWorld,
    ) -> Option<(Pose, Pose, f32)> {
        let held = self.held.as_ref()?;
        let view = to_rapier(player.view(physics).head_pose);
        let rotation_frame = if self.rules[&held.item].no_rotation {
            to_rapier(player.body_pose(physics)).rotation
        } else {
            view.rotation
        };
        let anchor = anchor_pose(&self.reference, held, view, rotation_frame);
        let grab = held.grip.map_or(anchor, |grip| {
            *physics.rapier().bodies[held.body].position() * grip
        });
        Some((grab, anchor, physics.authored_body_mass(held.body)))
    }
    pub fn acquisition_held(&self) -> bool {
        self.acquisition_held
    }
    pub fn release_acquisition(&mut self, input: FrameInput) {
        if input.held & keys::GRAB == 0 || input.released & keys::GRAB != 0 {
            self.acquisition_held = false;
        }
    }
    pub fn allows_pickup(&self, item: ItemId, seated: bool, mounted: bool) -> bool {
        self.rules.get(&item).is_some_and(|rule| {
            !rule.cant_pickup
                && !(seated && rule.cant_pickup_while_seated)
                && !(mounted && rule.cant_pickup_while_mounted)
        })
    }
    pub fn drop(&mut self, items: &mut ItemStore, physics: &mut PhysicsWorld) -> Option<ItemId> {
        let held = self.held.take()?;
        physics.remove_impulse_joint(held.joint, true);
        physics.set_body_mass_override(held.body, None);
        physics.edit_body(held.body, |body| {
            body.set_gravity_scale(held.gravity_scale, true)
        });
        if let Some(item) = items.get_mut(held.item) {
            item.location = ItemLocation::World;
        }
        self.acquisition_held = false;
        self.throwing = false;
        self.throw_charge = 0.0;
        Some(held.item)
    }
    pub fn can_receive_equipment(
        &self,
        id: ItemId,
        player: &PlayerState,
        items: &ItemStore,
        physics: &PhysicsWorld,
    ) -> bool {
        let Some(item) = items.get(id) else {
            return false;
        };
        let Some(body) = item
            .body
            .and_then(|handle| physics.rapier().bodies.get(handle))
        else {
            return false;
        };
        self.held.is_none()
            && item.location == ItemLocation::Equipped
            && !item.removed
            && !item.pending_attachment
            && item.inactive_pose.is_none()
            && self.allows_pickup(id, player.seated(), items.mounted(id))
            && self
                .rules
                .get(&id)
                .is_some_and(|rule| rule.can_hold_right_hand)
            && body.body_type() == rapier3d::prelude::RigidBodyType::KinematicPositionBased
            && body.is_enabled()
            && physics.body_active(item.body.unwrap())
            && physics.vehicle_of(item.body.unwrap()).is_none()
            && player
                .view(physics)
                .head_pose
                .position
                .iter()
                .all(|value| value.is_finite())
    }
    pub fn can_acquire(
        &self,
        id: ItemId,
        point: [f32; 3],
        player: &PlayerState,
        body: RigidBodyHandle,
        physics: &PhysicsWorld,
    ) -> Result<(), ReferenceError> {
        if !physics.body_active(body)
            || self.held.is_some()
            || !point.iter().all(|v| v.is_finite())
            || !self.allows_pickup(id, player.seated(), false)
            || player.item_seat_body() == Some(body)
            || physics.vehicle_of(body).is_some()
            || physics
                .rapier()
                .bodies
                .get(body)
                .is_none_or(|body| !body.is_dynamic())
        {
            return Err(ReferenceError("Cannot carry this item".into()));
        }
        Ok(())
    }
    pub fn pickup(
        &mut self,
        id: ItemId,
        point: [f32; 3],
        player: &PlayerState,
        items: &mut ItemStore,
        physics: &mut PhysicsWorld,
    ) -> Result<(), ReferenceError> {
        let item = items
            .get(id)
            .ok_or_else(|| ReferenceError("Cannot carry this item".into()))?;
        if item.location != ItemLocation::World {
            return Err(ReferenceError("Cannot carry this item".into()));
        }
        let body = item
            .body
            .ok_or_else(|| ReferenceError("Cannot carry this item".into()))?;
        self.can_acquire(id, point, player, body, physics)?;
        let item = items.get_mut(id).unwrap();
        let gravity_scale = physics.rapier().bodies[body].gravity_scale();
        physics.set_body_mass_override(body, Some(1.0));
        let pose = *physics.rapier().bodies[body].position();
        let anchor = *self.anchor.get_or_insert_with(|| {
            physics.insert_body(RigidBodyBuilder::kinematic_position_based().pose(pose))
        });
        physics.edit_body(anchor, |body| body.set_position(pose, false));
        let joint = physics.insert_impulse_joint(anchor, body, FixedJointBuilder::new(), true);
        let view = to_rapier(player.view(physics).head_pose);
        let camera = to_rapier(player.view(physics).camera_pose).rotation;
        let local_pose = to_rapier(item.local_pose);
        let item_pose = pose * local_pose;
        let rotation_frame = if self.rules[&id].no_rotation {
            to_rapier(player.body_pose(physics)).rotation
        } else {
            view.rotation
        };
        physics.edit_body(body, |body| {
            body.enable_ccd(true);
            body.set_gravity_scale(0.0, true);
        });
        self.held = Some(HeldState {
            item: id,
            body,
            offset: (view.rotation.inverse() * (item_pose.translation - view.translation)
                - Vector::from_array(self.reference.carry_anchor_offset))
            .clamp_length_max(self.reference.carry_offset_radius),
            rotation: rotation_frame.inverse() * item_pose.rotation,
            joint,
            local_pose,
            gravity_scale,
            grip: Some(pose.inverse() * Pose::from_parts(Vector::from_array(point), camera)),
        });
        item.location = ItemLocation::Held;
        self.cancel_input();
        Ok(())
    }

    pub fn recenter(&mut self, player: &PlayerState, physics: &PhysicsWorld) {
        let Some(held) = self.held.as_mut() else {
            return;
        };
        let rule = &self.rules[&held.item];
        let view = to_rapier(player.view(physics).head_pose);
        let rotation_frame = if rule.no_rotation {
            to_rapier(player.body_pose(physics)).rotation
        } else {
            view.rotation
        };
        held.grip = None;
        held.offset = if rule.use_recenter_position {
            Vector::from_array(rule.recenter_position)
        } else {
            Vector::from_array(self.reference.carry_anchor_offset) * rule.recenter_distance
        };
        held.rotation = rotation_frame.inverse()
            * view.rotation
            * if rule.use_recenter_rotation {
                Rotation::from_array(rule.recenter_rotation)
            } else {
                Rotation::IDENTITY
            };
    }
    pub fn step(
        &mut self,
        input: FrameInput,
        player: &PlayerState,
        tuning_available: bool,
        physics: &mut PhysicsWorld,
        dt: f32,
    ) -> bool {
        if !dt.is_finite() || dt <= 0.0 {
            return false;
        }
        self.release_acquisition(input);
        let Some(held) = self.held.as_mut() else {
            return false;
        };
        let Some(anchor) = self.anchor else {
            return false;
        };
        let view = to_rapier(player.view(physics).head_pose);
        let rule = &self.rules[&held.item];
        let rotation_frame = if rule.no_rotation {
            to_rapier(player.body_pose(physics)).rotation
        } else {
            view.rotation
        };
        let view_in_frame = rotation_frame.inverse() * view.rotation;
        let mass = physics.authored_body_mass(held.body);
        let movement_gain = ((mass - self.reference.carry_max_weight)
            / (self.reference.carry_min_weight - self.reference.carry_max_weight))
            .clamp(0.0, 1.0);
        let rotation_gain = ((mass - self.reference.carry_max_weight * 2.0)
            / (self.reference.carry_min_weight - self.reference.carry_max_weight * 2.0))
            .clamp(0.0, 1.0);
        let rotating = input.held & keys::ROTATE != 0;
        let moving = input.held & keys::TRANSLATE != 0;
        if (input.held | input.pressed) & keys::RECENTER != 0 {
            held.offset = if rule.use_recenter_position {
                Vector::from_array(rule.recenter_position)
            } else {
                Vector::from_array(self.reference.carry_anchor_offset) * rule.recenter_distance
            };
            held.rotation = rotation_frame.inverse()
                * view.rotation
                * if rule.use_recenter_rotation {
                    Rotation::from_array(rule.recenter_rotation)
                } else {
                    Rotation::IDENTITY
                };
        } else {
            if input.wheel.is_finite()
                && wheel_allowed(input, tuning_available, player.view_owns_look_input())
            {
                let direction = if rule.move_with_rotation {
                    (Vector::from_array(self.reference.carry_anchor_offset) + held.offset)
                        .normalize_or_zero()
                } else {
                    -Vector::Z
                };
                held.offset += direction
                    * input.wheel
                    * self.settings.player_scroll()
                    * dt
                    * self.settings.object()[0];
            }
            if input.mouse.iter().all(|delta| delta.is_finite()) {
                if rotating {
                    let sensitivity = dt * 10.0 * self.settings.object()[1] * rotation_gain;
                    let x = input.mouse[0] * sensitivity;
                    let y = input.mouse[1] * sensitivity;
                    let rotation = if moving {
                        if rule.move_with_rotation {
                            held.rotation * Rotation::from_rotation_z(-x.to_radians())
                        } else {
                            Rotation::from_axis_angle(view_in_frame * Vector::Z, -x.to_radians())
                                * held.rotation
                        }
                    } else {
                        Rotation::from_axis_angle(view_in_frame * Vector::Y, x.to_radians())
                            * held.rotation
                    };
                    held.rotation = if !rule.move_with_rotation || !moving {
                        Rotation::from_axis_angle(view_in_frame * Vector::X, y.to_radians())
                            * rotation
                    } else {
                        rotation
                    };
                } else if moving && !rotating {
                    if rule.move_with_rotation {
                        let sensitivity = dt * 20.0 * self.settings.object()[0];
                        let rotation =
                            Rotation::from_rotation_y((-input.mouse[0] * sensitivity).to_radians())
                                * Rotation::from_rotation_x(
                                    (-input.mouse[1] * sensitivity).to_radians(),
                                );
                        let position =
                            Vector::from_array(self.reference.carry_anchor_offset) + held.offset;
                        held.offset = rotation * position
                            - Vector::from_array(self.reference.carry_anchor_offset);
                    } else {
                        held.offset += Vector::new(input.mouse[0], -input.mouse[1], 0.0)
                            * dt
                            * 0.25
                            * self.settings.object()[0];
                    }
                }
            }
        }
        held.offset = held
            .offset
            .clamp_length_max(self.reference.carry_offset_radius);
        let current = *physics.rapier().bodies[held.body].position();
        let mut desired =
            anchor_pose(&self.reference, held, view, rotation_frame) * held.local_pose.inverse();
        if (desired.translation - current.translation).length()
            >= self.reference.carry_drop_distance
        {
            return true;
        }
        desired.translation = current.translation
            + ((desired.translation - current.translation) * self.reference.carry_force_multiplier)
                .clamp_length_max(self.reference.carry_max_force * movement_gain)
                * physics.rapier().integration_parameters.dt;
        let outside_player = |_: ColliderHandle, collider: &rapier3d::prelude::Collider| {
            collider.parent() != Some(player.body_handle())
        };
        for collider_handle in physics.rapier().bodies[held.body].colliders() {
            let collider = &physics.rapier().colliders[*collider_handle];
            let queries = physics.queries(
                QueryFilter::default()
                    .exclude_sensors()
                    .exclude_rigid_body(held.body)
                    .groups(collider.collision_groups())
                    .predicate(&outside_player),
            );
            let local = collider
                .position_wrt_parent()
                .copied()
                .unwrap_or(Pose::IDENTITY);
            // Slide the rest of the motion along each contact, as a pulled rigidbody does.
            let mut start = current.translation;
            let mut motion = desired.translation - current.translation;
            for _ in 0..3 {
                let Some((_, hit)) = queries.cast_shape(
                    &(Pose::from_parts(start, current.rotation) * local),
                    motion,
                    collider.shape(),
                    ShapeCastOptions {
                        max_time_of_impact: 1.0,
                        stop_at_penetration: false,
                        ..Default::default()
                    },
                ) else {
                    start += motion;
                    break;
                };
                let advance = (hit.time_of_impact - 0.01).max(0.0);
                start += motion * advance;
                let rest = motion * (1.0 - advance);
                motion = rest - hit.normal1 * rest.dot(hit.normal1).min(0.0);
            }
            desired.translation = start;
            if queries
                .intersect_shape(desired * local, collider.shape())
                .next()
                .is_some()
            {
                desired.rotation = current.rotation;
            }
        }
        physics.edit_body(anchor, |body| body.set_next_kinematic_position(desired));
        false
    }
}
