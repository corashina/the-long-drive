//! Source push force and a reusable steering grip.
use super::AimTarget;
use crate::player::PlayerState;
use crate::player::body::{BodyState, ReachFrame, ReachTarget};
use crate::reference::{
    BodyId, ColliderId, ItemId, PlayerReference, ReferenceManifest, Transform, WorldPushReference,
};
use crate::simulation::{FrameInput, keys};
use crate::vehicle::{PhysicsWorld, VehiclePose, from_rapier, to_rapier};
use rapier3d::math::{Pose, Rotation, Vector};
use rapier3d::prelude::{GenericJointBuilder, ImpulseJointHandle, JointAxesMask, RigidBodyHandle};
use std::collections::HashMap;

/// Push values of one item. `body` keys the interaction body map.
#[derive(Clone)]
struct Rule {
    body: BodyId,
    modifier: f32,
    only_projected: bool,
    both: bool,
    push_even_if_same_parent: bool,
    grip: Option<GripRule>,
}
#[derive(Clone)]
struct GripRule {
    colliders: Vec<ColliderId>,
    /// Grip pose in the body frame.
    local_pose: Transform,
    /// `HT`, else the grip, in the body frame.
    hand: VehiclePose,
    in_use: bool,
}

fn pose(t: &Transform) -> Pose {
    Pose::from_parts(
        Vector::from_array(t.translation),
        Rotation::from_array(t.rotation),
    )
}

/// `pushgripable.HT` in the body frame. `grip` and `hand` share one root frame.
pub fn grip_hand(local: &Transform, grip: &Transform, hand: Option<&Transform>) -> VehiclePose {
    let local = pose(local);
    from_rapier(&hand.map_or(local, |hand| local * pose(grip).inverse() * pose(hand)))
}
struct Grip {
    owner: ItemId,
    body: RigidBodyHandle,
    collider: rapier3d::prelude::ColliderHandle,
    joint: ImpulseJointHandle,
    previous_pose: Pose,
}
pub struct PushState {
    rules: HashMap<ItemId, Rule>,
    player: PlayerReference,
    /// Maximum steering angle per vehicle, in radians.
    grip: Option<Grip>,
    cancelled: bool,
    steer: Option<f32>,
}
impl PushState {
    pub fn new(reference: &ReferenceManifest) -> Self {
        Self {
            rules: reference
                .pushables
                .iter()
                .map(|r| {
                    let rule = Rule {
                        body: r.body,
                        modifier: r.modifier,
                        only_projected: r.only_projected,
                        both: r.both,
                        push_even_if_same_parent: r.push_even_if_same_parent,
                        grip: r.grip.as_ref().map(|g| {
                            let node = |id| {
                                let p = super::items::node_pose(reference, id)
                                    .expect("validated push grip node");
                                Transform {
                                    translation: p.position,
                                    rotation: p.rotation,
                                    scale: [1.; 3],
                                }
                            };
                            GripRule {
                                colliders: g.colliders.clone(),
                                local_pose: g.local_pose,
                                hand: grip_hand(
                                    &g.local_pose,
                                    &node(g.node),
                                    g.hand_node.map(node).as_ref(),
                                ),
                                in_use: g.in_use,
                            }
                        }),
                    };
                    (r.owner, rule)
                })
                .collect(),
            player: reference.player.clone(),
            grip: None,
            cancelled: false,
            steer: None,
        }
    }
    /// Adds the push rule of a world copy. `grip` holds the generated ids of the grip colliders.
    pub fn register_generated(
        &mut self,
        item: ItemId,
        body: BodyId,
        reference: &WorldPushReference,
        grip: Vec<ColliderId>,
        hand: Option<VehiclePose>,
    ) {
        self.rules.insert(
            item,
            Rule {
                body,
                modifier: reference.modifier,
                only_projected: reference.only_projected,
                both: reference.both,
                push_even_if_same_parent: reference.push_even_if_same_parent,
                grip: reference.grip.as_ref().zip(hand).map(|(g, hand)| GripRule {
                    colliders: grip,
                    local_pose: g.local_pose,
                    hand,
                    in_use: g.in_use,
                }),
            },
        );
    }
    /// Removes the rule of a world copy. Returns its body id.
    pub fn remove_generated(&mut self, item: ItemId) -> Option<BodyId> {
        self.rules.remove(&item).map(|rule| rule.body)
    }
    pub fn is_grip(&self, item: ItemId, collider: ColliderId) -> bool {
        self.rules
            .get(&item)
            .and_then(|rule| rule.grip.as_ref())
            .is_some_and(|grip| grip.colliders.contains(&collider))
    }
    pub fn supports(&self, item: ItemId) -> bool {
        self.rules.contains_key(&item)
    }
    pub(super) fn can_start(
        &self,
        target: AimTarget,
        bodies: &HashMap<BodyId, RigidBodyHandle>,
        player: &PlayerState,
        physics: &PhysicsWorld,
    ) -> bool {
        let Some(rule) = self.rules.get(&target.item) else {
            return false;
        };
        let Some(&body) = bodies.get(&rule.body) else {
            return false;
        };
        let Some(collider) = physics.rapier().colliders.get(target.collider) else {
            return false;
        };
        if target.slot.is_some()
            || player.seated()
            || !target.point.iter().all(|v| v.is_finite())
            || physics
                .rapier()
                .bodies
                .get(body)
                .is_none_or(|b| !b.is_dynamic() || !b.is_enabled())
            || !collider.is_enabled()
        {
            return false;
        }
        let id = crate::reference::ColliderId((collider.user_data >> 64) as u32);
        if self.is_grip(target.item, id) {
            let grip = rule.grip.as_ref().unwrap();
            player.grounded()
                && !grip.in_use
                && player.support_body() != Some(body)
                && self.grip_in_range(grip, body, player, physics)
        } else {
            rule.push_even_if_same_parent
                || !player.grounded()
                || player.support_body() != Some(body)
        }
    }
    fn grip_in_range(
        &self,
        rule: &GripRule,
        body: RigidBodyHandle,
        player: &PlayerState,
        physics: &PhysicsWorld,
    ) -> bool {
        let point = *physics.rapier().bodies[body].position()
            * Vector::from_array(rule.local_pose.translation);
        Vector::from_array(player.body_pose(physics).position).distance_squared(point)
            <= self.player.grip_distance.powi(2)
    }
    pub fn cancel_input(&mut self) {
        self.cancelled = true;
        self.steer = None;
    }
    pub fn steering(&self) -> Option<(RigidBodyHandle, f32)> {
        Some((self.grip.as_ref()?.body, self.steer?))
    }
    pub fn gripping(&self) -> bool {
        self.grip.is_some() && !self.cancelled
    }
    pub fn stop(&mut self, player: &mut PlayerState, physics: &mut PhysicsWorld) {
        if let Some(grip) = self.grip.take() {
            physics.remove_impulse_joint(grip.joint, true);
        }
        player.set_push_gripping(false, physics);
        self.steer = None;
        self.cancelled = false;
    }
    pub(super) fn finish_cancelled_input(
        &mut self,
        player: &mut PlayerState,
        physics: &mut PhysicsWorld,
    ) {
        if self.cancelled {
            self.stop(player, physics);
        }
    }
    pub fn step(
        &mut self,
        input: FrameInput,
        target: Option<AimTarget>,
        bodies: &HashMap<BodyId, RigidBodyHandle>,
        player: &mut PlayerState,
        body: &mut BodyState,
        physics: &mut PhysicsWorld,
        dt: f32,
    ) -> Option<ItemId> {
        if self.grip.is_none() && (input.held & keys::GRAB == 0 || input.released & keys::GRAB != 0)
        {
            self.stop(player, physics);
            return None;
        }
        let target = if let Some(grip) = &self.grip {
            if physics
                .rapier()
                .bodies
                .get(grip.body)
                .is_none_or(|body| !body.is_dynamic() || !body.is_enabled())
                || physics
                    .rapier()
                    .colliders
                    .get(grip.collider)
                    .is_none_or(|collider| {
                        !collider.is_enabled() || collider.parent() != Some(grip.body)
                    })
            {
                self.stop(player, physics);
                return None;
            }
            Some(AimTarget {
                item: grip.owner,
                collider: grip.collider,
                distance: 0.0,
                point: physics.rapier().colliders[grip.collider]
                    .position()
                    .translation
                    .to_array(),
                slot: None,
            })
        } else {
            target
        };
        let Some(target) = target else {
            self.stop(player, physics);
            return None;
        };
        if input.pressed & keys::GRAB != 0 && self.grip.is_some() {
            self.stop(player, physics);
            return None;
        }
        if self.grip.is_none() && !self.can_start(target, bodies, player, physics) {
            self.stop(player, physics);
            return None;
        }
        let Some(rule) = self.rules.get(&target.item) else {
            self.stop(player, physics);
            return None;
        };
        let Some(&body_handle) = bodies.get(&rule.body) else {
            self.stop(player, physics);
            return None;
        };
        if target.slot.is_some() || player.seated() || !target.point.iter().all(|v| v.is_finite()) {
            self.stop(player, physics);
            return None;
        }
        let is_grip = rule.grip.as_ref().is_some_and(|grip| {
            let id = crate::reference::ColliderId(
                (physics
                    .rapier()
                    .colliders
                    .get(target.collider)
                    .map_or(0, |c| c.user_data)
                    >> 64) as u32,
            );
            grip.colliders.contains(&id)
        });
        if is_grip {
            let grip_rule = rule.grip.as_ref().unwrap();
            let car_pose = *physics.rapier().bodies[body_handle].position();
            let position = Vector::from_array(player.body_pose(physics).position);
            if !self.grip_in_range(grip_rule, body_handle, player, physics)
                || self
                    .grip
                    .as_ref()
                    .is_some_and(|g| g.owner != target.item || g.collider != target.collider)
            {
                self.stop(player, physics);
                return None;
            }
            if self.grip.is_none() {
                if input.pressed & keys::GRAB == 0 {
                    return None;
                }
                let player_pose = *physics.rapier().bodies[player.body_handle()].position();
                let joint = GenericJointBuilder::new(JointAxesMask::LIN_X | JointAxesMask::LIN_Z)
                    .local_frame1(Pose::IDENTITY)
                    .local_frame2(car_pose.inverse() * player_pose);
                let joint =
                    physics.insert_impulse_joint(player.body_handle(), body_handle, joint, true);
                self.grip = Some(Grip {
                    owner: target.item,
                    body: body_handle,
                    collider: target.collider,
                    joint,
                    previous_pose: car_pose,
                });
            }
            let grip = self.grip.as_mut().unwrap();
            debug_assert_eq!(grip.body, body_handle);
            // Source: `MoveNearestHand(pushgrip.HT)`, else the grip, every gripping frame.
            body.move_nearest_hand(
                ReachTarget {
                    frame: ReachFrame::Body(body_handle),
                    pose: grip_rule.hand,
                },
                None,
            );
            let before = grip.previous_pose.rotation * -Vector::Z;
            let after = car_pose.rotation * -Vector::Z;
            let turn = before.angle_between(after).to_degrees() * before.cross(after).y.signum();
            player.follow_push_grip(turn);
            grip.previous_pose = car_pose;
            player.set_push_gripping(true, physics);
            self.steer = Some(
                ((input.held & keys::LEFT != 0) as u8 as f32)
                    - ((input.held & keys::RIGHT != 0) as u8 as f32),
            );
            let forward = ((input.held & keys::FORWARD != 0) as u8 as f32)
                - ((input.held & keys::BACK != 0) as u8 as f32);
            let angle = physics.max_steer(body_handle).unwrap_or(0.0);
            let direction = Rotation::from_rotation_y(self.steer.unwrap() * angle)
                * (car_pose.rotation * -Vector::Z)
                * forward;
            let up = car_pose.rotation * Vector::Y;
            let projected = (direction - up * direction.dot(up)).normalize_or_zero();
            physics.edit_body(body_handle, |body| {
                body.apply_impulse_at_point(
                    projected
                        * self.player.grip_speed
                        * self.player.grip_force
                        * self.player.source_force_interval,
                    position,
                    true,
                )
            });
            return Some(target.item);
        }
        if self.grip.is_some() {
            self.stop(player, physics);
            return None;
        }
        let view = to_rapier(player.view(physics).camera_pose);
        let forward = view.rotation * -Vector::Z;
        // Source `MoveNearestLegHand(hit.point)` at the world `nearestHand`/`nearestLeg` Transform.
        // Their rotation is `CamParent.rotation`.
        body.move_nearest_leg_hand(ReachTarget {
            frame: ReachFrame::World,
            pose: VehiclePose {
                position: target.point,
                rotation: view.rotation.to_array(),
            },
        });
        let mut direction = forward;
        let projected = Vector::new(direction.x, 0.0, direction.z);
        if (rule.both && projected.angle_between(direction) < 45.0_f32.to_radians())
            || (!rule.both && rule.only_projected)
        {
            direction = projected;
        }
        let mass = physics.rapier().bodies[body_handle].mass();
        physics.edit_body(body_handle, |body| {
            body.apply_impulse_at_point(
                direction
                    * mass
                    * self.player.push_force
                    * dt
                    * rule.modifier
                    * self.player.source_force_interval,
                Vector::from_array(target.point),
                true,
            )
        });
        if !player.grounded() {
            player.apply_recoil(
                -forward * self.player.airborne_push_force * self.player.source_force_interval,
            );
        }
        Some(target.item)
    }
}
