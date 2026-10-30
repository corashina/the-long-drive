//! Source trailer hitch: `attachgripscript` grip, `attachablescript` onlyToPoint attach to a
//! `utanfuto` target, and `utanfutoscript` center of mass. One ball joint links car and trailer.
//! A hitched grip keeps `ItemLocation::World`. Its body sleeps disabled and its shapes ride on
//! the car without contacts or mass.
use super::{
    InteractionBlock,
    items::{ItemLocation, ItemStore},
};
use crate::reference::{ColliderId, ItemId, ReferenceError, SourceId};
use crate::vehicle::{PhysicsWorld, from_rapier, solver_groups, to_rapier};
use rapier3d::math::{Pose, Vector};
use rapier3d::prelude::{
    ColliderHandle, ImpulseJointHandle, RigidBodyHandle, SphericalJointBuilder,
};

/// Slot of one hitch target.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HitchTarget(pub u32);

struct Target {
    /// Source `attachTargetscript`.
    source: SourceId,
    /// The item that owns the target: a car or a trailer root.
    owner: ItemId,
    collider: ColliderId,
    types: Vec<String>,
    /// Target pose in the owner item frame.
    local: Pose,
}

enum Link {
    Loose(ImpulseJointHandle),
    Hitched {
        target: usize,
        joint: ImpulseJointHandle,
    },
}

struct GripShape {
    handle: ColliderHandle,
    /// Pose in the grip body frame.
    local: Pose,
}

/// The source values of one trailer, in its body frame.
#[derive(Clone, Debug)]
pub struct TrailerHitch {
    pub item: ItemId,
    pub body: RigidBodyHandle,
    pub grip: ItemId,
    pub grip_body: RigidBodyHandle,
    /// `VgripPos`.
    pub grip_position: Vector,
    /// Grip `attachablescript.tipus`.
    pub types: Vec<String>,
    pub center_of_mass: Vector,
    pub center_of_mass_hitched: Vector,
}

struct Trailer {
    spec: TrailerHitch,
    shapes: Vec<GripShape>,
    link: Link,
}

#[derive(Default)]
pub struct Hitches {
    targets: Vec<Option<Target>>,
    trailers: Vec<Trailer>,
}

fn error(message: &str) -> ReferenceError {
    ReferenceError(message.into())
}

impl Hitches {
    pub fn add_target(
        &mut self,
        owner: ItemId,
        source: SourceId,
        collider: ColliderId,
        types: Vec<String>,
        local: Pose,
    ) -> HitchTarget {
        let target = Some(Target {
            source,
            owner,
            collider,
            types,
            local,
        });
        let index = match self.targets.iter().position(Option::is_none) {
            Some(index) => {
                self.targets[index] = target;
                index
            }
            None => {
                self.targets.push(target);
                self.targets.len() - 1
            }
        };
        HitchTarget(index as u32)
    }

    /// Removes the targets of `owner`. A grip on them goes loose.
    pub fn remove_targets(
        &mut self,
        owner: ItemId,
        items: &mut ItemStore,
        physics: &mut PhysicsWorld,
    ) {
        for index in 0..self.targets.len() {
            if self.targets[index]
                .as_ref()
                .is_some_and(|t| t.owner == owner)
            {
                if let Some(trailer) = self
                    .trailers
                    .iter()
                    .position(|t| matches!(t.link, Link::Hitched { target, .. } if target == index))
                {
                    self.release(trailer, items, physics);
                }
                self.targets[index] = None;
            }
        }
    }

    /// Adds a bound trailer and joins its grip to the drawbar.
    pub fn register(
        &mut self,
        spec: TrailerHitch,
        items: &mut ItemStore,
        physics: &mut PhysicsWorld,
    ) -> Result<(), ReferenceError> {
        if self
            .trailers
            .iter()
            .any(|t| t.spec.item == spec.item || t.spec.grip == spec.grip)
        {
            return Err(error("Trailer is already registered"));
        }
        let shapes = physics.rapier().bodies[spec.grip_body]
            .colliders()
            .iter()
            .map(|h| GripShape {
                handle: *h,
                local: *physics.rapier().colliders[*h]
                    .position_wrt_parent()
                    .unwrap(),
            })
            .collect::<Vec<GripShape>>();
        // Grip shapes move to the car while hitched, so the grip mass is additional mass.
        physics.recompute_body_mass(spec.grip_body);
        let mass = physics.rapier().bodies[spec.grip_body].mass();
        for s in &shapes {
            physics.edit_collider(s.handle, |c| c.set_density(0.));
        }
        physics.set_body_mass(spec.grip_body, mass);
        // Source attachgripscript: the grip spawns at the grip point. Source grip Rigidbody:
        // useGravity false.
        let pose = *physics.rapier().bodies[spec.body].position()
            * Pose::from_translation(spec.grip_position);
        physics.edit_body(spec.grip_body, |b| {
            b.set_gravity_scale(0., true);
            b.set_translation(pose.translation, true);
        });
        if let Some(grip) = items.get_mut(spec.grip) {
            grip.pose.position = pose.translation.to_array();
        }
        let joint = Self::loose_joint(&spec, physics);
        physics.set_center_of_mass(spec.body, spec.center_of_mass)?;
        self.trailers.push(Trailer {
            spec,
            shapes,
            link: Link::Loose(joint),
        });
        Ok(())
    }

    /// Removes a trailer. A hitched grip goes loose first.
    pub fn remove_trailer(
        &mut self,
        item: ItemId,
        items: &mut ItemStore,
        physics: &mut PhysicsWorld,
    ) {
        let Some(index) = self.trailers.iter().position(|t| t.spec.item == item) else {
            return;
        };
        self.release(index, items, physics);
        if let Link::Loose(joint) = self.trailers[index].link {
            physics.remove_impulse_joint(joint, true);
        }
        let spec = &self.trailers[index].spec;
        if physics.rapier().bodies.contains(spec.body) {
            physics.clear_center_of_mass(spec.body);
        }
        self.trailers.swap_remove(index);
    }

    /// Removes the targets of `item` and the trailer that `item` is or grips.
    pub fn remove_item(&mut self, item: ItemId, items: &mut ItemStore, physics: &mut PhysicsWorld) {
        self.remove_targets(item, items, physics);
        if let Some(trailer) = self
            .trailers
            .iter()
            .find(|t| t.spec.item == item || t.spec.grip == item)
            .map(|t| t.spec.item)
        {
            self.remove_trailer(trailer, items, physics);
        }
    }

    fn loose_joint(spec: &TrailerHitch, physics: &mut PhysicsWorld) -> ImpulseJointHandle {
        let joint = SphericalJointBuilder::new()
            .local_anchor1(spec.grip_position)
            .local_anchor2(Vector::ZERO)
            .contacts_enabled(false);
        physics.insert_impulse_joint(spec.body, spec.grip_body, joint, true)
    }

    pub fn collider_target(&self, id: ColliderId) -> Option<HitchTarget> {
        self.targets
            .iter()
            .position(|t| t.as_ref().is_some_and(|t| t.collider == id))
            .map(|i| HitchTarget(i as u32))
    }

    pub fn target_owner(&self, target: HitchTarget) -> Option<ItemId> {
        self.target(target.0 as usize).map(|t| t.owner)
    }

    fn target(&self, index: usize) -> Option<&Target> {
        self.targets.get(index).and_then(Option::as_ref)
    }

    /// The target of `owner` with source `source`.
    pub fn resolve_target(&self, owner: ItemId, source: &SourceId) -> Option<HitchTarget> {
        self.targets
            .iter()
            .position(|t| {
                t.as_ref()
                    .is_some_and(|t| t.owner == owner && &t.source == source)
            })
            .map(|i| HitchTarget(i as u32))
    }

    pub fn is_grip(&self, item: ItemId) -> bool {
        self.trailers.iter().any(|t| t.spec.grip == item)
    }

    pub fn is_hitched(&self, grip: ItemId) -> bool {
        self.trailers
            .iter()
            .any(|t| t.spec.grip == grip && matches!(t.link, Link::Hitched { .. }))
    }

    /// The target owner and source of a hitched grip.
    pub fn hitched_target(&self, grip: ItemId) -> Option<(ItemId, &SourceId)> {
        self.trailers
            .iter()
            .find(|t| t.spec.grip == grip)
            .and_then(|t| match t.link {
                Link::Hitched { target, .. } => self.target(target).map(|t| (t.owner, &t.source)),
                Link::Loose(_) => None,
            })
    }

    fn target_used(&self, target: usize) -> bool {
        self.trailers
            .iter()
            .any(|t| matches!(t.link, Link::Hitched { target: used, .. } if used == target))
    }

    /// Source `attachTargetscript.CanAttach` on the onlyToPoint branch.
    pub fn can_attach(
        &self,
        grip: ItemId,
        target: HitchTarget,
        items: &ItemStore,
    ) -> Result<(), InteractionBlock> {
        let trailer = self
            .trailers
            .iter()
            .find(|t| t.spec.grip == grip)
            .ok_or(InteractionBlock::Unavailable)?;
        let index = target.0 as usize;
        let t = self.target(index).ok_or(InteractionBlock::Unavailable)?;
        let shared = t.types.iter().any(|kind| trailer.spec.types.contains(kind));
        if !shared
            || t.owner == trailer.spec.item
            || matches!(trailer.link, Link::Hitched { .. })
            || self.target_used(index)
            || !items
                .get(grip)
                .is_some_and(|s| matches!(s.location, ItemLocation::World | ItemLocation::Held))
            || items.get(t.owner).is_none_or(|s| s.body.is_none())
        {
            return Err(InteractionBlock::Unavailable);
        }
        Ok(())
    }

    /// Body, world pose and body-frame pose of a target.
    fn target_frame(
        &self,
        target: usize,
        items: &ItemStore,
        physics: &PhysicsWorld,
    ) -> Option<(RigidBodyHandle, Pose, Pose)> {
        let t = self.target(target)?;
        let owner = items.get(t.owner)?;
        let body = owner.body?;
        let pose = to_rapier(owner.pose) * t.local;
        let local = physics.rapier().bodies.get(body)?.position().inverse() * pose;
        Some((body, pose, local))
    }

    /// Source `attachablescript.Attach`: the grip snaps to the target, the car pulls the
    /// trailer at the grip point, and the trailer uses its hitched center of mass.
    /// The grip must lie in the world, not in the hand.
    pub fn attach(
        &mut self,
        grip: ItemId,
        target: HitchTarget,
        items: &mut ItemStore,
        physics: &mut PhysicsWorld,
    ) -> Result<(), ReferenceError> {
        self.can_attach(grip, target, items)
            .map_err(|_| error("Cannot attach grip"))?;
        if items.get(grip).unwrap().location != ItemLocation::World {
            return Err(error("Release carry before grip attach"));
        }
        let index = target.0 as usize;
        let (car, _, local) = self
            .target_frame(index, items, physics)
            .ok_or_else(|| error("Missing hitch target body"))?;
        let trailer = self
            .trailers
            .iter_mut()
            .find(|t| t.spec.grip == grip)
            .unwrap();
        if let Link::Loose(joint) = trailer.link {
            physics.remove_impulse_joint(joint, true);
        }
        let joint = SphericalJointBuilder::new()
            .local_anchor1(local.translation)
            .local_anchor2(trailer.spec.grip_position)
            .contacts_enabled(false);
        let joint = physics.insert_impulse_joint(car, trailer.spec.body, joint, true);
        trailer.link = Link::Hitched {
            target: index,
            joint,
        };
        physics.edit_body(trailer.spec.grip_body, |b| b.set_enabled(false));
        Self::move_shapes(&trailer.shapes, car, local, false, physics);
        physics.set_center_of_mass(trailer.spec.body, trailer.spec.center_of_mass_hitched)?;
        self.sync(items, physics);
        physics.refresh_queries();
        Ok(())
    }

    /// Source `pickupable.Pick` on an attached grip: `attachablescript.Detach`.
    pub fn detach(
        &mut self,
        grip: ItemId,
        items: &mut ItemStore,
        physics: &mut PhysicsWorld,
    ) -> Result<(), ReferenceError> {
        let index = self
            .trailers
            .iter()
            .position(|t| t.spec.grip == grip)
            .ok_or_else(|| error("Unknown grip"))?;
        if !matches!(self.trailers[index].link, Link::Hitched { .. }) {
            return Err(error("Grip is not attached"));
        }
        self.release(index, items, physics);
        Ok(())
    }

    /// Ends the hitch of trailer `index`: the grip leaves the target with the target speed.
    fn release(&mut self, index: usize, items: &mut ItemStore, physics: &mut PhysicsWorld) {
        let Link::Hitched { target, joint } = self.trailers[index].link else {
            return;
        };
        let frame = self.target_frame(target, items, physics);
        let trailer = &mut self.trailers[index];
        physics.remove_impulse_joint(joint, true);
        let spec = trailer.spec.clone();
        if !physics.rapier().bodies.contains(spec.grip_body) {
            trailer.link = Link::Loose(ImpulseJointHandle::invalid());
            return;
        }
        let (pose, velocity, angular) = match frame {
            Some((car, pose, _)) => {
                let car = &physics.rapier().bodies[car];
                (pose, car.velocity_at_point(pose.translation), car.angvel())
            }
            None => (
                *physics.rapier().bodies[spec.grip_body].position(),
                Vector::ZERO,
                Vector::ZERO,
            ),
        };
        physics.edit_body(spec.grip_body, |b| {
            b.set_position(pose, true);
            b.set_enabled(true);
            b.set_linvel(velocity, true);
            b.set_angvel(angular, true);
        });
        Self::move_shapes(
            &trailer.shapes,
            spec.grip_body,
            Pose::IDENTITY,
            true,
            physics,
        );
        if physics.rapier().bodies.contains(spec.body) {
            trailer.link = Link::Loose(Self::loose_joint(&spec, physics));
            // The trailer body keeps its mass, so the override cannot fail.
            let _ = physics.set_center_of_mass(spec.body, spec.center_of_mass);
        } else {
            trailer.link = Link::Loose(ImpulseJointHandle::invalid());
        }
        if let Some(s) = items.get_mut(spec.grip) {
            s.pose = from_rapier(&pose);
        }
        physics.refresh_queries();
    }

    /// Moves the grip shapes to `body` at `pose`. Hitched shapes have no contacts and no mass.
    fn move_shapes(
        shapes: &[GripShape],
        body: RigidBodyHandle,
        pose: Pose,
        loose: bool,
        physics: &mut PhysicsWorld,
    ) {
        for s in shapes {
            if physics.rapier().colliders.get(s.handle).is_none() {
                continue;
            }
            if physics.rapier().colliders[s.handle].parent() != Some(body) {
                physics.set_collider_parent(s.handle, Some(body));
            }
            physics.edit_collider(s.handle, |c| {
                c.set_position_wrt_parent(pose * s.local);
                c.set_solver_groups(solver_groups(loose));
            });
        }
        physics.recompute_body_mass(body);
    }

    /// Keeps each hitched grip item and its sleeping body on its target.
    pub fn sync(&self, items: &mut ItemStore, physics: &mut PhysicsWorld) {
        for t in &self.trailers {
            let Link::Hitched { target, .. } = t.link else {
                continue;
            };
            let Some((_, pose, _)) = self.target_frame(target, items, physics) else {
                continue;
            };
            physics.try_edit_body(t.spec.grip_body, |b| b.set_position(pose, false));
            if let Some(s) = items.get_mut(t.spec.grip) {
                s.pose = from_rapier(&pose);
            }
        }
    }

    /// The trailer item of `item`, its grip item, and the target owner it hangs on.
    /// `item` is a trailer, a grip or an owner of a used target.
    pub fn groups(&self) -> impl Iterator<Item = [Option<ItemId>; 3]> + '_ {
        self.trailers.iter().map(|t| {
            let owner = match t.link {
                Link::Hitched { target, .. } => self.target(target).map(|t| t.owner),
                Link::Loose(_) => None,
            };
            [Some(t.spec.item), Some(t.spec.grip), owner]
        })
    }

    /// The car item that pulls trailer `item`, through any chain of trailers.
    pub fn towing_car(&self, item: ItemId) -> Option<ItemId> {
        let mut current = item;
        for _ in 0..=self.trailers.len() {
            let Some(trailer) = self.trailers.iter().find(|t| t.spec.item == current) else {
                return (current != item).then_some(current);
            };
            let Link::Hitched { target, .. } = trailer.link else {
                return None;
            };
            current = self.target(target)?.owner;
        }
        None
    }
}
