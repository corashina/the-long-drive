//! Source `utanfutoscript` trailers and `attachTargetscript` hitch targets on spawned world
//! items: hitch registration, trailer wheels, brakes and rear lamps.
use super::WorldStream;
use crate::interaction::InteractionState;
use crate::interaction::hitch::TrailerHitch;
use crate::interaction::items::ItemStore;
use crate::reference::{ItemId, ReferenceError, SourceId, Transform};
use crate::simulation::FIXED_DT;
use crate::simulation::car_lights::{CarLightInput, CarLightLinks, CarLights};
use crate::vehicle::{
    PhysicsWorld, VehiclePose, WheelMount, WheelSet, WheelTorques, collider_profile, to_rapier,
};
use crate::world::GeneratedId;
use rapier3d::math::{Pose, Rotation, Vector};

/// The trailer of one spawned world item.
pub struct TrailerBinding {
    pub(super) parent: GeneratedId,
    prototype: usize,
    item: ItemId,
    /// `utanfutoscript.brake`: the flashlightscript that sets the brake. None: no brake.
    brake: Option<SourceId>,
    brake_force: f32,
    wheels: WheelSet,
    torques: Vec<WheelTorques>,
    steer: Vec<f32>,
    lights: CarLights,
    graphics: Vec<WheelGraphic>,
}

/// Source `wheelgraphicsscript.Graphics` of one trailer wheel.
struct WheelGraphic {
    /// The `T` node.
    node: usize,
    /// Root frame of the body, scaled.
    body_pose: Pose,
    /// Unscaled root frame of the `T` parent, inverted.
    parent_inverse: Pose,
    /// Rotation of the wheel collider node in the body frame.
    collider_rotation: Rotation,
    scale: [f32; 3],
    /// The `T` pose in its parent frame after the last wheel step.
    pose: Transform,
    /// The `T` position in the unscaled root frame after the last wheel step.
    root: Vector,
}
impl TrailerBinding {
    pub fn heap_bytes(&self) -> usize {
        crate::world::contents::identity_heap(&self.parent)
            + self.brake.as_ref().map_or(0, |s| s.file.capacity())
            + self.wheels.heap_bytes()
            + self.torques.capacity() * std::mem::size_of::<WheelTorques>()
            + self.steer.capacity() * std::mem::size_of::<f32>()
            + self.graphics.capacity() * std::mem::size_of::<WheelGraphic>()
    }
    /// The `T` position of each wheel in the unscaled root frame.
    pub fn graphic_positions(&self) -> impl Iterator<Item = Vector> + '_ {
        self.graphics.iter().map(|g| g.root)
    }
    /// The `T` node of each wheel and its pose in its parent frame.
    pub fn graphics(&self) -> impl Iterator<Item = (usize, Transform)> + '_ {
        self.graphics.iter().map(|g| (g.node, g.pose))
    }
}

/// True if `child` is the content that `parent` spawned from component `source`.
fn spawned_child(child: &GeneratedId, parent: &GeneratedId, source: &SourceId) -> bool {
    child.layer == parent.layer
        && child.generator == parent.generator
        && child.unit == parent.unit
        && child.ordinal == parent.ordinal
        && child.children.len() == parent.children.len() + 1
        && child.children.starts_with(&parent.children)
        && child
            .children
            .last()
            .is_some_and(|(s, o)| s == source && *o == 0)
}

/// WheelCollider.GetWorldPose in the parent frame of `T`: the wheel center below its connection,
/// turned by steer, then by the roll angle.
fn graphic_pose(g: &WheelGraphic, wheel: &crate::vehicle::WheelState) -> (Transform, Vector) {
    let center = wheel.connection - Vector::Y * wheel.suspension_length;
    let rotation = g.collider_rotation
        * Rotation::from_rotation_y(wheel.steering)
        * Rotation::from_rotation_x(-wheel.angle);
    let root = g.body_pose * Pose::from_parts(center, rotation);
    // Prototype node frames are unscaled.
    let unscaled = Vector::from_array(std::array::from_fn(|i| root.translation[i] / g.scale[i]));
    let local = g.parent_inverse * Pose::from_parts(unscaled, root.rotation);
    let pose = Transform {
        translation: local.translation.to_array(),
        rotation: local.rotation.to_array(),
        scale: g.pose.scale,
    };
    (pose, unscaled)
}

impl WorldStream {
    pub(super) fn sync_trailer_bindings(
        &mut self,
        items: &mut ItemStore,
        interaction: &mut InteractionState,
        physics: &mut PhysicsWorld,
    ) -> Result<(), ReferenceError> {
        let revision = (self.request.session, self.contents.active_revision());
        if self.trailer_scan == Some(revision) {
            return Ok(());
        }
        let mut complete = true;
        let active = &self.contents.active;
        self.trailer_bindings.retain(|b| {
            active
                .iter()
                .any(|c| c.content.placement.id == b.parent && c.item == Some(b.item))
        });
        self.hitch_owners.retain(|(parent, item)| {
            active
                .iter()
                .any(|c| c.content.placement.id == *parent && c.item == Some(*item))
        });
        for slot in 0..self.contents.active.len() {
            let content = &self.contents.active[slot];
            let Some(item) = content.item else {
                continue;
            };
            let prototype = &content.content.placement.prototype;
            let Some(index) = self
                .trailer_prototypes
                .iter()
                .copied()
                .find(|&p| &self.reference.prototypes[p].source == prototype)
            else {
                continue;
            };
            let registered = self.hitch_owners.iter().any(|(_, i)| *i == item);
            let bound = self.trailer_bindings.iter().any(|b| b.item == item);
            let record = &self.reference.prototypes[index];
            if (record.hitches.is_empty() || registered) && (record.trailer.is_none() || bound) {
                continue;
            }
            if !content.restored {
                complete = false;
                continue;
            }
            // A removed item has no body and no hitch to bind.
            if items.get(item).and_then(|i| i.body).is_none() {
                continue;
            }
            if !record.hitches.is_empty() && !registered {
                self.bind_hitches(items, interaction, physics, slot, index)?;
                let parent = self.contents.active[slot].content.placement.id.clone();
                self.hitch_owners.push((parent, item));
            }
            if record.trailer.is_some() && !bound {
                match self.bind_trailer(items, interaction, physics, slot, index)? {
                    Some(binding) => self.trailer_bindings.push(binding),
                    None => complete = false,
                }
            }
        }
        self.trailer_scan = complete.then_some(revision);
        Ok(())
    }

    /// The instance body of collider record `collider` and its pose in the prototype root frame.
    fn record_body(
        &self,
        slot: usize,
        index: usize,
        collider: u32,
    ) -> Result<(usize, Pose), ReferenceError> {
        let placement = &self.contents.active[slot].content.placement;
        let prototype = &self.reference.prototypes[index];
        let body = prototype.colliders[collider as usize]
            .body
            .ok_or_else(|| ReferenceError("Hitch collider has no body".into()))?
            as usize;
        let t = prototype.bodies[body].pose;
        let scale = placement.scale;
        Ok((
            body,
            to_rapier(VehiclePose {
                position: std::array::from_fn(|i| t.translation[i] * scale[i]),
                rotation: t.rotation,
            }),
        ))
    }

    fn bind_hitches(
        &self,
        items: &ItemStore,
        interaction: &mut InteractionState,
        physics: &PhysicsWorld,
        slot: usize,
        index: usize,
    ) -> Result<(), ReferenceError> {
        let content = &self.contents.active[slot];
        let placement = &content.content.placement;
        let prototype = &self.reference.prototypes[index];
        let instance = self
            .structures
            .iter()
            .find(|s| s.placement.id == placement.id)
            .ok_or_else(|| ReferenceError("Hitch item has no published structure".into()))?;
        let scale = placement.scale;
        for hitch in &prototype.hitches {
            let (body, body_pose) = self.record_body(slot, index, hitch.collider)?;
            let handle = instance.bodies[body];
            let source = &prototype.colliders[hitch.collider as usize].source;
            let collider = instance
                .colliders
                .iter()
                .find(|(s, _)| s == source)
                .map(|(_, h)| *h)
                .ok_or_else(|| ReferenceError("Hitch collider is not published".into()))?;
            let id = crate::reference::ColliderId(
                (physics.rapier().colliders[collider].user_data >> 64) as u32,
            );
            // The target belongs to the content item or binding on the collider body.
            let owner = content
                .item
                .into_iter()
                .chain(content.bindings.iter().map(|b| b.item))
                .find(|i| items.get(*i).is_some_and(|s| s.body == Some(handle)))
                .ok_or_else(|| ReferenceError("Hitch target body has no item".into()))?;
            let t = crate::world::placement::node_pose(prototype, hitch.target as usize);
            let target = to_rapier(VehiclePose {
                position: std::array::from_fn(|i| t.translation[i] * scale[i]),
                rotation: t.rotation,
            });
            let local = to_rapier(items.get(owner).unwrap().local_pose).inverse()
                * body_pose.inverse()
                * target;
            interaction.hitch.add_target(
                owner,
                hitch.source.clone(),
                id,
                hitch.types.clone(),
                local,
            );
        }
        Ok(())
    }

    /// Binds a trailer once its grip item has a body. None: the grip is not bound yet.
    fn bind_trailer(
        &self,
        items: &mut ItemStore,
        interaction: &mut InteractionState,
        physics: &mut PhysicsWorld,
        slot: usize,
        index: usize,
    ) -> Result<Option<TrailerBinding>, ReferenceError> {
        let content = &self.contents.active[slot];
        let placement = &content.content.placement;
        let prototype = &self.reference.prototypes[index];
        let reference = prototype.trailer.as_ref().unwrap();
        // A saved hitch keeps the grip unrestored until the trailer registers.
        let Some(grip) = self
            .contents
            .active
            .iter()
            .find(|c| {
                spawned_child(
                    &c.content.placement.id,
                    &placement.id,
                    &reference.grip.source,
                )
            })
            .and_then(|c| c.item)
        else {
            return Ok(None);
        };
        let Some(grip_body) = items.get(grip).and_then(|i| i.body) else {
            return Ok(None);
        };
        let item = content.item.unwrap();
        let body = items.get(item).and_then(|i| i.body).unwrap();
        let scale = placement.scale;
        let scaled = |p: [f32; 3]| Vector::from_array(std::array::from_fn(|i| p[i] * scale[i]));
        let body_pose = {
            let t: Transform = prototype.bodies[reference.body as usize].pose;
            to_rapier(VehiclePose {
                position: std::array::from_fn(|i| t.translation[i] * scale[i]),
                rotation: t.rotation,
            })
        };
        let mounts: Vec<WheelMount> = reference
            .wheels
            .iter()
            .map(|w| {
                let point = crate::world::placement::node_point(
                    prototype,
                    w.node as usize,
                    w.collider.center,
                );
                WheelMount {
                    connection: body_pose.inverse() * scaled(point),
                    collider: &w.collider,
                    profile: collider_profile(&w.collider),
                }
            })
            .collect();
        interaction.hitch.register(
            TrailerHitch {
                item,
                body,
                grip,
                grip_body,
                grip_position: body_pose.inverse() * scaled(reference.grip.position),
                types: reference.grip.types.clone(),
                center_of_mass: body_pose.inverse() * scaled(reference.center_of_mass),
                center_of_mass_hitched: body_pose.inverse()
                    * scaled(reference.center_of_mass_hitched),
            },
            items,
            physics,
        )?;
        let links = reference
            .lights
            .iter()
            .map(|light| CarLightLinks {
                electronics: None,
                owner: crate::world::content_mechanisms::node_item(
                    content,
                    prototype,
                    &self.reference,
                    &self.contents.rules,
                    light.node as usize,
                )
                .filter(|i| *i != item),
                doors: Vec::new(),
            })
            .collect();
        let frame = |i: usize| to_rapier(crate::world::placement::node_frame(prototype, 0, i));
        let graphics = reference
            .wheels
            .iter()
            .zip(&reference.graphics)
            .map(|(w, &node)| {
                let node = node as usize;
                let parent = prototype.nodes[node].parent.unwrap_or(0) as usize;
                WheelGraphic {
                    node,
                    body_pose,
                    parent_inverse: frame(parent).inverse(),
                    collider_rotation: body_pose.rotation.inverse()
                        * frame(w.node as usize).rotation,
                    scale,
                    pose: prototype.nodes[node].transform,
                    root: Vector::from_array(
                        crate::world::placement::node_pose(prototype, node).translation,
                    ),
                }
            })
            .collect();
        let count = mounts.len();
        Ok(Some(TrailerBinding {
            parent: placement.id.clone(),
            prototype: index,
            item,
            brake: reference.brake.clone(),
            brake_force: reference.brake_force,
            wheels: WheelSet::new(body, &mounts),
            torques: vec![
                WheelTorques {
                    motor_nm: 0.,
                    brake_nm: 0.,
                };
                count
            ],
            steer: vec![0.; count],
            lights: CarLights::new(&reference.lights, links),
            graphics,
        }))
    }

    /// utanfutoscript.Update: brake torque while the brake switch is on, then the wheels.
    /// Run before the physics step.
    pub fn step_trailers(&mut self, physics: &mut PhysicsWorld) {
        for b in &mut self.trailer_bindings {
            if physics.rapier().bodies.get(b.wheels.body()).is_none() {
                continue;
            }
            let on = b.brake.as_ref().is_some_and(|brake| {
                self.contents
                    .active
                    .iter()
                    .find(|c| c.content.placement.id == b.parent)
                    .is_some_and(|c| {
                        c.content
                            .inputs
                            .lamps
                            .iter()
                            .any(|(p, s, on)| *on && *p == b.parent && s == brake)
                    })
            });
            let torque = if on { b.brake_force } else { 0. };
            for t in &mut b.torques {
                t.brake_nm = torque;
            }
            physics.step_wheel_set(&mut b.wheels, &b.torques, &b.steer);
            for (g, wheel) in b.graphics.iter_mut().zip(b.wheels.wheels()) {
                (g.pose, g.root) = graphic_pose(g, wheel);
            }
        }
    }

    /// Source trailer `rearlights`: the lamps follow the car that tows the trailer.
    /// `input` gives the light input of a towing car item.
    pub fn advance_trailer_lights(
        &mut self,
        items: &ItemStore,
        interaction: &InteractionState,
        input: impl Fn(ItemId) -> CarLightInput,
    ) {
        let mut bindings = std::mem::take(&mut self.trailer_bindings);
        for b in &mut bindings {
            let input = interaction
                .hitch
                .towing_car(b.item)
                .map_or_else(CarLightInput::default, &input);
            let records = &self.reference.prototypes[b.prototype]
                .trailer
                .as_ref()
                .unwrap()
                .lights;
            let item = b.item;
            b.lights.advance(
                records,
                input,
                |i| i == item || items.descends(i, item),
                |i| interaction.mechanisms.value(i, false).unwrap_or(0.),
                FIXED_DT,
            );
            self.set_car_lights(&b.parent, b.lights.lights());
        }
        self.trailer_bindings = bindings;
    }

    /// The lamp state of the trailer on body `body`.
    pub fn trailer_lights(&self, body: rapier3d::prelude::RigidBodyHandle) -> Option<&CarLights> {
        self.trailer_bindings
            .iter()
            .find(|b| b.wheels.body() == body)
            .map(|b| &b.lights)
    }

    pub fn trailer(&self, parent: &GeneratedId) -> Option<&TrailerBinding> {
        self.trailer_bindings.iter().find(|b| &b.parent == parent)
    }

    /// Each trailer body and its lamp state.
    pub fn trailers(
        &self,
    ) -> impl Iterator<Item = (rapier3d::prelude::RigidBodyHandle, &CarLights)> {
        self.trailer_bindings
            .iter()
            .map(|b| (b.wheels.body(), &b.lights))
    }
}
