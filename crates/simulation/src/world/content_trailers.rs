//! Source `utanfutoscript` trailers and `attachTargetscript` hitch targets on spawned world
//! items: hitch registration, trailer wheels, brakes and rear lamps.
use super::WorldStream;
use super::content_kinds::{
    BindContext, Bound, ContentKind, ContentKinds, KindBindings, Ready, ready_unless_removed,
};
use crate::interaction::InteractionState;
use crate::interaction::hitch::TrailerHitch;
use crate::interaction::items::ItemStore;
use crate::reference::{ItemId, ReferenceError, SourceId, Transform, WorldPrototypeReference};
use crate::simulation::FIXED_DT;
use crate::simulation::car_lights::{CarLightInput, CarLightLinks, CarLights};
use crate::vehicle::{
    PhysicsWorld, VehiclePose, WheelMount, WheelSet, WheelTorques, collider_profile, to_rapier,
};
use crate::world::GeneratedId;
use crate::world::wheel_graphics::WheelGraphic;
use rapier3d::math::{Pose, Vector};

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
    /// The `T` pose in its parent frame and its unscaled root position after the last wheel step.
    graphic_poses: Vec<(Transform, Vector)>,
}

impl TrailerBinding {
    /// The `T` position of each wheel in the unscaled root frame.
    pub fn graphic_positions(&self) -> impl Iterator<Item = Vector> + '_ {
        self.graphic_poses.iter().map(|p| p.1)
    }
    /// The `T` node of each wheel and its pose in its parent frame.
    pub fn graphics(&self) -> impl Iterator<Item = (usize, Transform)> + '_ {
        self.graphics
            .iter()
            .zip(&self.graphic_poses)
            .map(|(g, p)| (g.node, p.0))
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

impl WorldStream {
    /// The instance body of collider record `collider` and its pose in the prototype root frame.
    fn record_body(
        &self,
        slot: usize,
        index: usize,
        collider: u32,
    ) -> Result<(usize, Pose), ReferenceError> {
        let placement = &self.contents.active()[slot].content.placement;
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
        let content = &self.contents.active()[slot];
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
        let content = &self.contents.active()[slot];
        let placement = &content.content.placement;
        let prototype = &self.reference.prototypes[index];
        let reference = prototype.trailer.as_ref().unwrap();
        // A saved hitch keeps the grip unrestored until the trailer registers.
        let Some(grip) = self
            .contents
            .active()
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
                switch: None,
                doors: Vec::new(),
            })
            .collect();
        let graphics: Vec<_> = reference
            .wheels
            .iter()
            .zip(&reference.graphics)
            .map(|(w, &node)| {
                WheelGraphic::new(prototype, node as usize, w.node as usize, body_pose, scale)
            })
            .collect();
        let graphic_poses = graphics
            .iter()
            .map(|g| {
                let root = crate::world::placement::node_pose(prototype, g.node).translation;
                (prototype.nodes[g.node].transform, Vector::from_array(root))
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
            graphic_poses,
        }))
    }

    /// utanfutoscript.Update. Applies brake torque while the brake switch is on, then steps the wheels.
    /// Run before the physics step.
    pub fn step_trailers(&mut self, physics: &mut PhysicsWorld) {
        for b in &mut self.kinds.trailers.list {
            if physics.rapier().bodies.get(b.wheels.body()).is_none() {
                continue;
            }
            let on = b.brake.as_ref().is_some_and(|brake| {
                self.contents
                    .active()
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
            for ((g, p), wheel) in b
                .graphics
                .iter()
                .zip(&mut b.graphic_poses)
                .zip(b.wheels.wheels())
            {
                *p = g.pose(wheel);
            }
        }
    }

    /// Source trailer `rearlights`. The lamps follow the car that tows the trailer.
    /// `input` gives the light input of a towing car item.
    pub fn advance_trailer_lights(
        &mut self,
        items: &ItemStore,
        interaction: &InteractionState,
        input: impl Fn(ItemId) -> CarLightInput,
    ) {
        let mut bindings = std::mem::take(&mut self.kinds.trailers.list);
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
                &[],
                |i| i == item || items.descends(i, item),
                |_, _| None,
                |_| None,
                FIXED_DT,
            );
            self.set_car_lights(&b.parent, b.lights.lights());
        }
        self.kinds.trailers.list = bindings;
    }

    /// The lamp state of the trailer on body `body`.
    pub fn trailer_lights(&self, body: rapier3d::prelude::RigidBodyHandle) -> Option<&CarLights> {
        self.kinds
            .trailers
            .list
            .iter()
            .find(|b| b.wheels.body() == body)
            .map(|b| &b.lights)
    }

    pub fn trailer(&self, parent: &GeneratedId) -> Option<&TrailerBinding> {
        self.kinds
            .trailers
            .list
            .iter()
            .find(|b| &b.parent == parent)
    }

    /// Each trailer body and its lamp state.
    pub fn trailers(
        &self,
    ) -> impl Iterator<Item = (rapier3d::prelude::RigidBodyHandle, &CarLights)> {
        self.kinds
            .trailers
            .list
            .iter()
            .map(|b| (b.wheels.body(), &b.lights))
    }
}

impl Bound for TrailerBinding {
    fn parent(&self) -> &GeneratedId {
        &self.parent
    }
    fn item(&self) -> Option<ItemId> {
        Some(self.item)
    }
}

/// A content whose hitch targets are registered.
pub struct HitchOwner {
    parent: GeneratedId,
    item: ItemId,
}
impl Bound for HitchOwner {
    fn parent(&self) -> &GeneratedId {
        &self.parent
    }
    fn item(&self) -> Option<ItemId> {
        Some(self.item)
    }
}

/// The world prototype index of the active content when it is in the trailer prototypes and
/// `with` holds for it.
fn trailer_prototype(
    stream: &WorldStream,
    slot: usize,
    with: fn(&WorldPrototypeReference) -> bool,
) -> Option<usize> {
    let prototype = &stream.contents.active()[slot].content.placement.prototype;
    let prototypes = &stream.reference.prototypes;
    (stream.kinds.trailer_prototypes.iter())
        .copied()
        .find(|&p| &prototypes[p].source == prototype)
        .filter(|&p| with(&prototypes[p]))
}

pub(super) struct HitchKind;
impl ContentKind for HitchKind {
    type Binding = HitchOwner;
    /// The world prototype index.
    type Script = usize;
    fn kind(kinds: &mut ContentKinds) -> &mut KindBindings<HitchOwner> {
        &mut kinds.hitches
    }
    fn script(
        stream: &mut WorldStream,
        _: &BindContext,
        slot: usize,
    ) -> Result<Option<usize>, ReferenceError> {
        Ok(trailer_prototype(stream, slot, |p| !p.hitches.is_empty()))
    }
    fn ready(stream: &WorldStream, items: &ItemStore, slot: usize) -> Ready {
        ready_unless_removed(stream, items, slot)
    }
    fn bind(
        stream: &mut WorldStream,
        cx: &mut BindContext,
        slot: usize,
        item: Option<ItemId>,
        index: usize,
    ) -> Result<Option<HitchOwner>, ReferenceError> {
        stream.bind_hitches(cx.items, cx.interaction, cx.physics, slot, index)?;
        Ok(Some(HitchOwner {
            parent: stream.contents.active()[slot].content.placement.id.clone(),
            item: item.unwrap(),
        }))
    }
}

pub(super) struct TrailerKind;
impl ContentKind for TrailerKind {
    type Binding = TrailerBinding;
    /// The world prototype index.
    type Script = usize;
    fn kind(kinds: &mut ContentKinds) -> &mut KindBindings<TrailerBinding> {
        &mut kinds.trailers
    }
    fn script(
        stream: &mut WorldStream,
        _: &BindContext,
        slot: usize,
    ) -> Result<Option<usize>, ReferenceError> {
        Ok(trailer_prototype(stream, slot, |p| p.trailer.is_some()))
    }
    fn ready(stream: &WorldStream, items: &ItemStore, slot: usize) -> Ready {
        ready_unless_removed(stream, items, slot)
    }
    fn bind(
        stream: &mut WorldStream,
        cx: &mut BindContext,
        slot: usize,
        _: Option<ItemId>,
        index: usize,
    ) -> Result<Option<TrailerBinding>, ReferenceError> {
        stream.bind_trailer(cx.items, cx.interaction, cx.physics, slot, index)
    }
}
