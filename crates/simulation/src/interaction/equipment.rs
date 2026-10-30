//! Source right-hand ownership and retained hand movement.
use super::items::{ItemLocation, ItemStore};
use crate::{
    player::pose::PlayerPoseState,
    reference::{ItemId, NodeId, PickupReference, ReferenceManifest},
    vehicle::{PhysicsWorld, VehiclePose, to_rapier},
};
use rapier3d::{
    math::Pose,
    prelude::{ColliderHandle, RigidBodyType},
};

#[derive(Clone, Copy)]
pub enum EquipmentPlacement {
    Animated,
    Immediate,
}

pub struct EquipmentState {
    item: Option<ItemId>,
    anchor: NodeId,
    colliders: Vec<(ColliderHandle, bool)>,
    hand_pose: Option<Pose>,
    elapsed: Option<f32>,
    interpolation_factor: f32,
    interpolation_seconds: f32,
}
impl EquipmentState {
    pub fn new(r: &ReferenceManifest) -> Self {
        Self {
            item: None,
            anchor: r.clothing.pose.anchors["RH"].node,
            colliders: Vec::with_capacity(r.colliders.len()),
            hand_pose: None,
            elapsed: None,
            interpolation_factor: r.inventory.hand_interpolation_factor,
            interpolation_seconds: r.inventory.hand_interpolation_seconds,
        }
    }
    pub fn item(&self) -> Option<ItemId> {
        self.item
    }
    pub fn equip(
        &mut self,
        id: ItemId,
        placement: EquipmentPlacement,
        items: &mut ItemStore,
        physics: &mut PhysicsWorld,
    ) {
        assert!(self.item.is_none());
        let item = items.get_mut(id).expect("validated equipment item");
        let body = item.body.expect("validated equipment body");
        self.colliders.clear();
        for &handle in physics.rapier().bodies[body].colliders() {
            self.colliders
                .push((handle, physics.rapier().colliders[handle].is_enabled()));
        }
        for &(handle, _) in &self.colliders {
            physics.edit_collider(handle, |collider| collider.set_enabled(false));
        }
        physics.edit_body(body, |rb| {
            rb.set_body_type(RigidBodyType::KinematicPositionBased, true);
            rb.set_linvel(rapier3d::math::Vector::ZERO, false);
            rb.set_angvel(rapier3d::math::Vector::ZERO, false);
        });
        item.location = ItemLocation::Equipped;
        self.item = Some(id);
        self.hand_pose = None;
        self.elapsed = match placement {
            EquipmentPlacement::Animated => Some(0.),
            EquipmentPlacement::Immediate => None,
        };
    }
    pub fn detach(&mut self, items: &mut ItemStore, physics: &mut PhysicsWorld) -> Option<ItemId> {
        let id = self.item.take()?;
        let item = items.get_mut(id).unwrap();
        physics.edit_body(item.body.unwrap(), |body| {
            body.set_body_type(RigidBodyType::Dynamic, true)
        });
        for &(handle, enabled) in &self.colliders {
            physics.edit_collider(handle, |collider| collider.set_enabled(enabled));
        }
        self.colliders.clear();
        self.hand_pose = None;
        self.elapsed = None;
        item.location = ItemLocation::World;
        Some(id)
    }
    fn offset(rule: &PickupReference) -> Pose {
        if rule.disable_rh_fix {
            Pose::IDENTITY
        } else {
            to_rapier(VehiclePose {
                position: rule.right_hand_position,
                rotation: rule.right_hand_rotation,
            })
        }
    }
    pub fn advance_hand(
        &mut self,
        dt: f32,
        pose: &PlayerPoseState,
        items: &ItemStore,
        rule: &PickupReference,
        physics: &mut PhysicsWorld,
    ) {
        if !dt.is_finite() || dt <= 0. || self.item.is_none() {
            return;
        }
        self.sync(pose, items, rule, physics);
        if let Some(elapsed) = &mut self.elapsed {
            *elapsed += dt;
            let target = Self::offset(rule);
            if *elapsed >= self.interpolation_seconds {
                self.hand_pose = Some(target);
                self.elapsed = None;
            } else {
                let current = self.hand_pose.as_mut().unwrap();
                let gain = (dt * self.interpolation_factor).clamp(0., 1.);
                current.translation = current.translation.lerp(target.translation, gain);
                current.rotation = current.rotation.lerp(target.rotation, gain);
            }
            self.sync(pose, items, rule, physics);
        }
    }
    pub fn sync(
        &mut self,
        pose: &PlayerPoseState,
        items: &ItemStore,
        rule: &PickupReference,
        physics: &mut PhysicsWorld,
    ) {
        let Some(id) = self.item else { return };
        let item = items.get(id).unwrap();
        let body = item.body.unwrap();
        let anchor = to_rapier(pose.anchor(self.anchor).expect("retained source hand"));
        let local = *self.hand_pose.get_or_insert_with(|| {
            if self.elapsed.is_some() {
                anchor.inverse()
                    * physics.rapier().bodies[body].position()
                    * to_rapier(item.local_pose)
            } else {
                Self::offset(rule)
            }
        });
        let target = anchor * local * to_rapier(item.local_pose).inverse();
        physics.edit_body(body, |body| {
            body.set_position(target, false);
            body.set_next_kinematic_position(target);
        });
    }
}
