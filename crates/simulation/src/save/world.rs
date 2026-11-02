use super::*;
use crate::interaction::{
    items::{ItemBodyIndex, ItemLocation},
    wheels::WheelAssemblies,
};
use crate::reference::{BodyId, ItemId, ReferenceManifest};
use crate::starting_area::StartingArea;
use crate::vehicle::{PhysicsWorld, VehiclePose, from_rapier, to_rapier};
use crate::world::{REBASE_LIMIT, WorldOrigin, WorldStream};
use bevy_platform::collections::{HashMap, HashSet};
use rapier3d::math::{Pose, Rotation, Vector};
use rapier3d::prelude::{RigidBodyHandle, RigidBodyType};

#[derive(Clone, Copy, PartialEq, Eq)]
enum Key {
    Source(BodyId),
    Assembly(ItemId),
}
#[derive(Clone, Copy)]
struct BodyState {
    pose: AbsolutePose,
    linear: [f32; 3],
    angular: [f32; 3],
    sleeping: bool,
    enabled: bool,
}
struct Record {
    key: Key,
    root: Option<BodyId>,
    handle: Option<RigidBodyHandle>,
    state: Option<BodyState>,
    /// Collider reach from the body origin and whether the body is not dynamic. `sync` sets them.
    extent: f64,
    fixed: bool,
}

/// Inactive poses use absolute coordinates. Runtime handles never enter a save.
pub struct PersistentWorld {
    records: Vec<Record>,
    processed: HashSet<RigidBodyHandle>,
    order: Vec<usize>,
    reference_items: HashMap<BodyId, Vec<ItemId>>,
    item_bodies: ItemBodyIndex,
    pending_items: Vec<(ItemId, AbsolutePose)>,
    /// Loose bodies made kinematic by distance, as source `itemPlaceRemoveScript`.
    frozen: HashSet<RigidBodyHandle>,
    /// Spawned copies with an assembly record.
    spawned: HashSet<ItemId>,
    important: Vec<[f64; 3]>,
    configured: bool,
}
impl PersistentWorld {
    pub fn new(reference: &ReferenceManifest) -> Self {
        let keys = reference
            .parts
            .iter()
            .map(|part| Key::Assembly(part.item))
            .chain(
                reference
                    .wheel_parts
                    .iter()
                    .map(|part| Key::Assembly(part.item)),
            )
            .chain(reference.bodies.iter().map(|body| Key::Source(body.id)));
        let records: Vec<_> = keys
            .map(|key| Record {
                key,
                root: None,
                handle: None,
                state: None,
                extent: 0.,
                fixed: false,
            })
            .collect();
        let mut result = Self {
            processed: HashSet::with_capacity(records.len()),
            order: Vec::with_capacity(records.len()),
            reference_items: HashMap::new(),
            item_bodies: ItemBodyIndex::default(),
            records,
            pending_items: Vec::new(),
            frozen: HashSet::new(),
            spawned: HashSet::new(),
            important: Vec::new(),
            configured: true,
        };
        result.configure_indexes(reference);
        result
    }
    pub fn restore(snapshot: &SaveSnapshot, identities: &IdentityMap) -> Result<Self, SaveError> {
        let mut records = Vec::with_capacity(identities.bodies.len() + identities.items.len());
        for body in &snapshot.bodies {
            if let BodyIdentity::Assembly { root } = &body.identity {
                records.push(Record {
                    key: Key::Assembly(identities.item(root)?),
                    root: None,
                    handle: None,
                    state: None,
                    extent: 0.,
                    fixed: false,
                });
            }
        }
        for &id in identities.bodies.values() {
            records.push(Record {
                key: Key::Source(id),
                root: None,
                handle: None,
                state: None,
                extent: 0.,
                fixed: false,
            });
        }
        for body in &snapshot.bodies {
            let key = match &body.identity {
                BodyIdentity::Source { source } => Key::Source(
                    *identities
                        .bodies
                        .get(source)
                        .ok_or_else(|| SaveError::InvalidLinks("Unknown body".into()))?,
                ),
                BodyIdentity::Assembly { root } => Key::Assembly(identities.item(root)?),
            };
            records
                .iter_mut()
                .find(|record| record.key == key)
                .unwrap()
                .state = Some(BodyState {
                pose: body.pose,
                linear: body.linear_velocity,
                angular: body.angular_velocity,
                sleeping: body.sleeping,
                enabled: true,
            });
        }
        Ok(Self {
            processed: HashSet::with_capacity(records.len()),
            order: Vec::with_capacity(records.len()),
            reference_items: HashMap::new(),
            item_bodies: ItemBodyIndex::default(),
            records,
            pending_items: snapshot
                .items
                .iter()
                .map(|item| Ok((identities.item(&item.instance)?, item.pose)))
                .collect::<Result<_, SaveError>>()?,
            frozen: HashSet::new(),
            spawned: HashSet::new(),
            important: Vec::new(),
            configured: false,
        })
    }
    fn configure_indexes(&mut self, reference: &ReferenceManifest) {
        let parents = reference
            .bodies
            .iter()
            .map(|body| (body.id, body.parent_body))
            .collect();
        configure_roots(&mut self.records, &parents, &mut self.order);
        self.reference_items.clear();
        for item in &reference.items {
            self.reference_items
                .entry(item.body)
                .or_default()
                .push(item.id);
        }
    }

    pub fn body_snapshot(
        &self,
        handle: RigidBodyHandle,
        identity: BodyIdentity,
    ) -> Option<BodySnapshot> {
        let state = self
            .records
            .iter()
            .find(|r| r.handle == Some(handle) && r.state.is_some())?
            .state?;
        Some(BodySnapshot {
            identity,
            pose: state.pose,
            linear_velocity: state.linear,
            angular_velocity: state.angular,
            sleeping: state.sleeping,
        })
    }
    pub fn recovery_body_pose(
        &self,
        physics: &PhysicsWorld,
        handle: RigidBodyHandle,
        vehicle: &crate::world::ObjectKey,
    ) -> Option<VehiclePose> {
        let Some(car) = self
            .records
            .iter()
            .find(|r| r.handle == Some(physics.chassis_handle(vehicle)))
            .and_then(|r| r.state)
        else {
            return Some(from_rapier(physics.rapier().bodies[handle].position()));
        };
        let state = self
            .records
            .iter()
            .find(|r| r.handle == Some(handle))?
            .state?;
        let delta =
            std::array::from_fn::<_, 3, _>(|i| state.pose.position[i] - car.pose.position[i]);
        if delta.iter().any(|v| v.abs() > f64::from(REBASE_LIMIT)) {
            return None;
        }
        Some(VehiclePose {
            position: delta.map(|v| v as f32),
            rotation: state.pose.rotation,
        })
    }
    pub fn recovery_cargo(
        &self,
        physics: &PhysicsWorld,
        handle: RigidBodyHandle,
        vehicle: &crate::world::ObjectKey,
        bounds: &crate::reference::Bounds,
    ) -> bool {
        let Some(state) = self
            .records
            .iter()
            .find(|r| r.handle == Some(handle))
            .and_then(|r| r.state)
        else {
            return false;
        };
        let Some(car) = self.recovery_body_pose(physics, physics.chassis_handle(vehicle), vehicle)
        else {
            return false;
        };
        let Some(pose) = self.recovery_body_pose(physics, handle, vehicle) else {
            return false;
        };
        let point = to_rapier(car).inverse() * Vector::from_array(pose.position);
        state.enabled
            && physics.rapier().bodies[handle].is_dynamic()
            && physics.vehicle_of(handle).is_none()
            && (0..3).all(|i| point[i] >= bounds.min[i] && point[i] <= bounds.max[i])
    }
    pub fn activate_recovery_bodies(
        &mut self,
        area: &mut StartingArea,
        physics: &mut PhysicsWorld,
        vehicle: &crate::world::ObjectKey,
        bodies: &[RigidBodyHandle],
    ) {
        if physics.body_active(physics.chassis_handle(vehicle)) {
            return;
        }
        // Set the virtual clearance frame only after the target permit succeeds.
        for &handle in bodies {
            let Some(pose) = self.recovery_body_pose(physics, handle, vehicle) else {
                continue;
            };
            let enabled = self
                .records
                .iter()
                .find(|r| r.handle == Some(handle))
                .and_then(|r| r.state)
                .is_some_and(|s| s.enabled);
            physics.edit_body(handle, |body| {
                body.set_position(to_rapier(pose), false);
                body.set_enabled(enabled);
            });
        }
        self.item_bodies.refresh(&area.items);
        for &handle in bodies {
            for record in self.records.iter_mut().filter(|r| r.handle == Some(handle)) {
                record.state = None;
            }
            physics.inactive_bodies.remove(&handle);
            for &id in self.item_bodies.items(handle) {
                area.items.get_mut(id).unwrap().inactive_pose = None;
            }
        }
    }

    pub fn absolute_vehicle_position(
        &self,
        physics: &PhysicsWorld,
        origin: &WorldOrigin,
        vehicle: &crate::world::ObjectKey,
    ) -> [f64; 3] {
        self.records
            .iter()
            .find(|r| r.handle == Some(physics.chassis_handle(vehicle)) && r.state.is_some())
            .and_then(|r| r.state)
            .map_or_else(
                || origin.absolute(physics.snapshot(vehicle).pose.position),
                |s| s.pose.position,
            )
    }
    /// Spawned copies are loose assemblies. They freeze and unload with the world like source items.
    fn track_spawned(&mut self, area: &StartingArea, physics: &mut PhysicsWorld) {
        let mut changed = false;
        if self.spawned.len() > area.spawned.len()
            || area.spawned.iter().any(|s| !self.spawned.contains(&s.id))
        {
            let (spawned, frozen) = (&mut self.spawned, &mut self.frozen);
            self.records.retain(|record| match record.key {
                Key::Assembly(id) if spawned.contains(&id) && !area.spawned.contains(id) => {
                    spawned.remove(&id);
                    if let Some(handle) = record.handle {
                        frozen.remove(&handle);
                        physics.inactive_bodies.remove(&handle);
                    }
                    changed = true;
                    false
                }
                _ => true,
            });
            for item in area.spawned.iter() {
                if self.spawned.insert(item.id) {
                    self.records.push(Record {
                        key: Key::Assembly(item.id),
                        root: None,
                        handle: None,
                        state: None,
                        extent: 0.,
                        fixed: false,
                    });
                    changed = true;
                }
            }
        }
        if changed {
            order_roots(&self.records, &mut self.order);
        }
    }
    pub fn sync(
        &mut self,
        reference: &ReferenceManifest,
        area: &mut StartingArea,
        wheels: &mut WheelAssemblies,
        stream: &WorldStream,
        physics: &mut PhysicsWorld,
    ) -> Result<(), SaveError> {
        if !self.configured {
            for key in reference
                .parts
                .iter()
                .map(|p| Key::Assembly(p.item))
                .chain(reference.wheel_parts.iter().map(|p| Key::Assembly(p.item)))
            {
                if !self.records.iter().any(|r| r.key == key) {
                    self.records.push(Record {
                        key,
                        root: None,
                        handle: None,
                        state: None,
                        extent: 0.,
                        fixed: false,
                    });
                }
            }
            self.records
                .sort_by_key(|r| matches!(r.key, Key::Source(_)));
            for record in &mut self.records {
                // Attachment owners disable source bodies whose shapes moved to another root.
                if let Key::Source(id) = record.key {
                    if let Some(state) = &mut record.state {
                        state.enabled = physics.rapier().bodies[area.bodies[&id]].is_enabled();
                    }
                }
                if record.state.is_some() {
                    continue;
                }
                if let Key::Source(id) = record.key {
                    let source = reference
                        .bodies
                        .iter()
                        .find(|body| body.id == id)
                        .expect("source body identity");
                    if source.parent_body.is_none() {
                        record.state = Some(BodyState {
                            pose: AbsolutePose {
                                position: source.pose.translation.map(f64::from),
                                rotation: source.pose.rotation,
                            },
                            linear: [0.; 3],
                            angular: [0.; 3],
                            sleeping: false,
                            enabled: physics.rapier().bodies[area.bodies[&id]].is_enabled(),
                        });
                    }
                }
            }
            self.configure_indexes(reference);
            self.configured = true;
        }
        self.track_spawned(area, physics);
        self.item_bodies.refresh(&area.items);
        for record in &mut self.records {
            match record.key {
                Key::Assembly(id)
                    if area.items.get(id).is_some_and(|i| {
                        !matches!(i.location, ItemLocation::World | ItemLocation::Held)
                    }) =>
                {
                    record.state = None;
                    record.handle = None;
                }
                Key::Source(id) => {
                    // A destroyed imported item (an exploded tank) has no body.
                    let Some(&handle) = area.bodies.get(&id) else {
                        continue;
                    };
                    if self
                        .reference_items
                        .get(&id)
                        .into_iter()
                        .flatten()
                        .any(|id| {
                            area.items.get(*id).is_some_and(|item| {
                                item.body.is_some() && item.body != Some(handle)
                            })
                        })
                    {
                        if let Some(state) = &mut record.state {
                            state.enabled = false;
                        }
                    }
                }
                _ => (),
            }
        }
        let mut changed = false;
        self.processed.clear();
        let freeze = stream.distances.item_freeze_dist.powi(2) as f64;
        let unfreeze = stream.distances.item_unfreeze_dist.powi(2) as f64;
        // Source important objects: the player and every car.
        let mut important = std::mem::take(&mut self.important);
        important.clear();
        important.push(stream.focus());
        for key in physics.vehicle_keys() {
            important.push(self.absolute_vehicle_position(physics, &stream.origin, key));
        }
        let held = area
            .items
            .iter()
            .find(|item| item.location == ItemLocation::Held)
            .and_then(|item| item.body);
        // Independent bodies decide activity first. Source children follow their physical root.
        for &index in &self.order {
            let key = self.records[index].key;
            let parent = self.records[index].root;
            let handle = match key {
                Key::Source(id) => area.bodies.get(&id).copied(),
                Key::Assembly(id) => area
                    .items
                    .get(id)
                    .filter(|item| {
                        matches!(
                            item.location,
                            ItemLocation::World
                                | ItemLocation::Held
                                | ItemLocation::Stored(_)
                                | ItemLocation::Equipped
                        )
                    })
                    .and_then(|item| item.body),
            };
            let Some(handle) = handle else {
                continue;
            };
            if !self.processed.insert(handle) {
                continue;
            }
            self.records[index].handle = Some(handle);
            let body = &physics.rapier().bodies[handle];
            let absolute = self.records[index].state.map_or_else(
                || stream.origin.absolute(body.translation().to_array()),
                |state| state.pose.position,
            );
            let rotation = self.records[index].state.map_or_else(
                || *body.rotation(),
                |state| Rotation::from_array(state.pose.rotation),
            );
            let body_rotation = Pose::from_parts(Vector::ZERO, rotation);
            let extent = body
                .colliders()
                .iter()
                .map(|h| {
                    let collider = &physics.rapier().colliders[*h];
                    let pose =
                        body_rotation * *collider.position_wrt_parent().expect("body collider");
                    let bounds = collider.shape().compute_aabb(&pose);
                    bounds.mins.abs().max(bounds.maxs.abs()).max_element()
                })
                .fold(0_f32, f32::max) as f64;
            self.records[index].extent = extent;
            self.records[index].fixed = !body.is_dynamic();
            let bounded = [0, 1, 2].iter().all(|&axis| {
                // The focus can lie up to the rebase limit from the origin, so a body near it
                // can lie up to twice the limit away.
                (absolute[axis] - stream.origin.offset[axis]).abs() + extent
                    <= 2. * REBASE_LIMIT as f64
            }) && [0, 2].iter().all(|&axis| {
                (absolute[axis] - stream.focus()[axis]).abs() + extent <= REBASE_LIMIT as f64
            });
            let pending_attachment = self.item_bodies.items(handle).iter().any(|id| {
                area.items
                    .get(*id)
                    .is_some_and(|item| item.pending_attachment)
            });
            let inventory = area.inventory_body(handle);
            let stored = matches!(inventory, Some(ItemLocation::Stored(_)));
            let active = if pending_attachment {
                false
            } else if matches!(
                inventory,
                Some(ItemLocation::Stored(_) | ItemLocation::Equipped | ItemLocation::Held)
            ) {
                true
            } else if let Some(parent) = parent {
                area.bodies
                    .get(&parent)
                    .is_some_and(|h| physics.body_active(*h))
            } else {
                Some(handle) == held
                    || area.player.seat_body() == Some(handle)
                    || (bounded
                        && stream.supports(crate::world::WorldBounds {
                            min: absolute.map(|v| v - extent - 2.),
                            max: absolute.map(|v| v + extent + 2.),
                        }))
            };
            if active {
                if let Some(state) = self.records[index].state.take() {
                    changed = true;
                    physics.edit_body(handle, |body| {
                        let pose = to_rapier(VehiclePose {
                            position: stream.origin.local(state.pose.position),
                            rotation: state.pose.rotation,
                        });
                        body.set_position(pose, false);
                        body.set_next_kinematic_position(pose);
                        body.set_enabled(state.enabled);
                        body.set_linvel(Vector::from_array(state.linear), false);
                        body.set_angvel(Vector::from_array(state.angular), false);
                        if state.sleeping {
                            body.sleep();
                        } else {
                            body.wake_up(true);
                        }
                    });
                    physics.inactive_bodies.remove(&handle);
                    for &id in self.item_bodies.items(handle) {
                        area.items.get_mut(id).unwrap().inactive_pose = None;
                    }
                }
                if stored {
                    physics.edit_body(handle, |body| body.set_enabled(false));
                }
                // Source distances are horizontal.
                let loose = parent.is_none()
                    && inventory.is_none()
                    && Some(handle) != held
                    && physics.vehicle_of(handle).is_none()
                    && !self.item_bodies.items(handle).is_empty();
                let distance = important
                    .iter()
                    .map(|p| (p[0] - absolute[0]).powi(2) + (p[2] - absolute[2]).powi(2))
                    .fold(f64::INFINITY, f64::min);
                let body = &physics.rapier().bodies[handle];
                if loose && distance >= freeze && body.is_dynamic() {
                    self.frozen.insert(handle);
                    physics.edit_body(handle, |body| {
                        body.set_body_type(RigidBodyType::KinematicPositionBased, true)
                    });
                } else if (!loose || distance < unfreeze) && self.frozen.remove(&handle) {
                    physics.edit_body(handle, |body| {
                        body.set_body_type(RigidBodyType::Dynamic, true)
                    });
                }
            } else if physics.body_active(handle) {
                changed = true;
                let pose = from_rapier(body.position());
                let state = self.records[index].state.unwrap_or(BodyState {
                    pose: AbsolutePose {
                        position: absolute,
                        rotation: pose.rotation,
                    },
                    linear: body.linvel().to_array(),
                    angular: body.angvel().to_array(),
                    sleeping: body.is_sleeping(),
                    enabled: body.is_enabled(),
                });
                for &id in self.item_bodies.items(handle) {
                    let item = area.items.get_mut(id).unwrap();
                    let offset = Rotation::from_array(state.pose.rotation)
                        * Vector::from_array(item.local_pose.position);
                    item.inactive_pose = Some(
                        self.pending_items
                            .iter()
                            .find(|(id, _)| *id == item.id)
                            .map_or(
                                AbsolutePose {
                                    position: std::array::from_fn(|axis| {
                                        absolute[axis] + offset[axis] as f64
                                    }),
                                    rotation: (Rotation::from_array(state.pose.rotation)
                                        * Rotation::from_array(item.local_pose.rotation))
                                    .to_array(),
                                },
                                |(_, pose)| *pose,
                            ),
                    );
                }
                self.records[index].state = Some(state);
                physics.inactive_bodies.insert(handle);
                physics.edit_body(handle, |body| {
                    body.set_enabled(false);
                    body.set_translation(Vector::ZERO, false);
                    body.set_linvel(Vector::ZERO, false);
                    body.set_angvel(Vector::ZERO, false);
                });
            }
        }
        self.important = important;
        self.pending_items.clear();
        if changed {
            wheels.sync_activity(physics);
            area.items.sync_poses(physics);
            area.interaction.sync_mechanisms(physics);
            area.sync_attachments(physics, false);
            area.commit_poses(physics, true);
            physics.refresh_queries();
        }
        Ok(())
    }
    /// True when an inactive fixed or kinematic body reaches into `bounds` on X and Z.
    /// Such a body is ground that `sync` has not restored yet, such as the garage after a far move.
    pub fn waits_for(&self, bounds: crate::world::WorldBounds) -> bool {
        self.records.iter().any(|record| {
            record.fixed
                && record.root.is_none()
                && record.state.is_some_and(|state| {
                    state.enabled
                        && [0, 2].into_iter().all(|axis| {
                            state.pose.position[axis] - record.extent <= bounds.max[axis]
                                && state.pose.position[axis] + record.extent >= bounds.min[axis]
                        })
                })
        })
    }
}

/// Preserve independent-record order before source children claim their shared handles.
fn configure_roots(
    records: &mut [Record],
    parents: &HashMap<BodyId, Option<BodyId>>,
    order: &mut Vec<usize>,
) {
    for record in records.iter_mut() {
        let mut parent = match record.key {
            Key::Source(id) => parents.get(&id).copied().flatten(),
            Key::Assembly(_) => None,
        };
        record.root = parent;
        while let Some(id) = parent {
            record.root = Some(id);
            parent = parents.get(&id).copied().flatten();
        }
    }
    order_roots(records, order);
}

/// Independent records first, then source children.
fn order_roots(records: &[Record], order: &mut Vec<usize>) {
    order.clear();
    for children in [false, true] {
        order.extend(
            records
                .iter()
                .enumerate()
                .filter_map(|(index, record)| (record.root.is_some() == children).then_some(index)),
        );
    }
}

#[cfg(test)]
mod world_sync_tests {
    use super::*;

    fn record(key: Key, handle: RigidBodyHandle) -> Record {
        Record {
            key,
            root: None,
            handle: Some(handle),
            state: None,
            extent: 0.,
            fixed: false,
        }
    }

    #[test]
    fn inactive_ground_near_the_player_holds_the_step() {
        let state = BodyState {
            pose: AbsolutePose {
                position: [40., 0., -70.],
                rotation: [0., 0., 0., 1.],
            },
            linear: [0.; 3],
            angular: [0.; 3],
            sleeping: false,
            enabled: true,
        };
        let mut world = PersistentWorld {
            records: vec![Record {
                state: Some(state),
                extent: 12.,
                fixed: true,
                ..record(
                    Key::Source(BodyId(1)),
                    RigidBodyHandle::from_raw_parts(1, 0),
                )
            }],
            processed: HashSet::new(),
            order: Vec::new(),
            reference_items: HashMap::new(),
            item_bodies: ItemBodyIndex::default(),
            pending_items: Vec::new(),
            frozen: HashSet::new(),
            spawned: HashSet::new(),
            important: Vec::new(),
            configured: true,
        };
        let window = |x: f64| crate::world::WorldBounds {
            min: [x - 8., -500., -78.],
            max: [x + 8., 500., -62.],
        };
        assert!(
            world.waits_for(window(30.)),
            "the garage reaches the player"
        );
        assert!(
            !world.waits_for(window(600.)),
            "far bodies do not hold the step"
        );
        world.records[0].state = None;
        assert!(
            !world.waits_for(window(30.)),
            "active bodies do not hold the step"
        );
    }

    #[test]
    fn world_sync_roots_keep_assembly_precedence_and_child_order() {
        let shared = RigidBodyHandle::from_raw_parts(1, 0);
        let other = RigidBodyHandle::from_raw_parts(2, 0);
        let mut records = vec![
            record(Key::Source(BodyId(3)), shared),
            record(Key::Assembly(ItemId(7)), shared),
            record(Key::Source(BodyId(1)), shared),
            record(Key::Source(BodyId(2)), other),
            record(Key::Source(BodyId(4)), other),
        ];
        let parents = [
            (BodyId(1), None),
            (BodyId(2), Some(BodyId(1))),
            (BodyId(3), Some(BodyId(2))),
            (BodyId(4), None),
        ]
        .into_iter()
        .collect();
        let mut order = Vec::new();
        configure_roots(&mut records, &parents, &mut order);
        assert_eq!(order, [1, 2, 4, 0, 3]);
        assert_eq!(records[0].root, Some(BodyId(1)));
        assert_eq!(records[3].root, Some(BodyId(1)));
        assert_eq!(records[1].root, None);
        let mut processed = HashSet::with_capacity(records.len());
        let winners: Vec<_> = order
            .iter()
            .copied()
            .filter(|&i| processed.insert(records[i].handle.unwrap()))
            .collect();
        assert_eq!(winners, [1, 4]);
    }

    #[test]
    fn world_sync_processed_handles_reset_each_pass_and_include_generation() {
        let first = RigidBodyHandle::from_raw_parts(1, 0);
        let reused = RigidBodyHandle::from_raw_parts(1, 1);
        let input = [first, first, reused, reused];
        let mut processed = HashSet::with_capacity(input.len());
        let capacity = processed.capacity();
        for _ in 0..100 {
            processed.clear();
            let mut winners = input.into_iter().filter(|handle| processed.insert(*handle));
            assert_eq!(winners.next(), Some(first));
            assert_eq!(winners.next(), Some(reused));
            assert_eq!(winners.next(), None);
            assert_eq!(processed.capacity(), capacity);
        }
    }

    #[test]
    #[ignore = "requires the installed source export"]
    fn world_sync_activation_follows_rebound_item_membership() {
        use crate::starting_area::GarageRuntime;
        let reference = ReferenceManifest::from_runtime_json(
            &std::fs::read(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../../web/assets/reference/manifest.json"
            ))
            .unwrap(),
        )
        .unwrap();
        let mut run = GarageRuntime::new_default(&reference).unwrap();
        crate::world::stream::tests::kit::prepare_ready(&mut run, &reference);
        assert!(run.startup_aligned);
        run.sync_world_activity(&reference).unwrap();
        let oil = reference
            .items
            .iter()
            .find(|item| item.name == "OilCan")
            .unwrap()
            .id;
        let food = reference.survival.starter_food;
        let oil_body = run.area.items.get(oil).unwrap().body.unwrap();
        let food_body = run.area.items.get(food).unwrap().body.unwrap();
        assert_ne!(oil_body, food_body);
        assert!(run.physics.body_active(oil_body));
        assert!(run.physics.body_active(food_body));
        for (target, previous) in [(food_body, oil_body), (oil_body, food_body)] {
            run.area
                .items
                .set_body(oil, Some(target))
                .unwrap()
                .pending_attachment = true;
            run.sync_world_activity(&reference).unwrap();
            assert!(
                !run.physics.body_active(target),
                "pending item disables its current body"
            );
            assert!(
                run.physics.body_active(previous),
                "previous membership cannot disable a body"
            );
            assert!(run.area.items.get(oil).unwrap().inactive_pose.is_some());
            assert_eq!(
                run.area.items.get(food).unwrap().inactive_pose.is_some(),
                target == food_body
            );
            run.area.items.get_mut(oil).unwrap().pending_attachment = false;
            run.sync_world_activity(&reference).unwrap();
            assert!(
                run.physics.body_active(target),
                "resolved attachment restores body activity"
            );
            assert!(run.physics.body_active(previous));
            assert!(run.area.items.get(oil).unwrap().inactive_pose.is_none());
            assert!(run.area.items.get(food).unwrap().inactive_pose.is_none());
        }
    }

    #[test]
    #[ignore = "requires the installed source export"]
    fn world_sync_unloads_a_spawned_weed_off_the_loaded_world() {
        use crate::starting_area::{GarageRuntime, spawned};
        let reference = ReferenceManifest::from_runtime_json(
            &std::fs::read(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../../web/assets/reference/manifest.json"
            ))
            .unwrap(),
        )
        .unwrap();
        let mut run = GarageRuntime::new_default(&reference).unwrap();
        crate::world::stream::tests::kit::prepare_ready(&mut run, &reference);
        let item = reference.weather.tumbleweeds.item_id;
        let prototype = reference
            .item_prototypes
            .iter()
            .position(|p| p.item_id == Some(item))
            .unwrap();
        let mut pose = run.area.player.body_pose(&run.physics);
        pose.position[1] += 2.;
        let weed =
            spawned::spawn_prototype(&reference, &mut run.area, &mut run.physics, prototype, pose)
                .unwrap()[0];
        let body = run.area.items.get(weed).unwrap().body.unwrap();
        run.sync_world_activity(&reference).unwrap();
        assert!(
            run.physics.body_active(body),
            "a weed near the player stays"
        );
        run.physics.edit_body(body, |b| {
            b.set_translation(
                Vector::new(pose.position[0] + 3000., 0., pose.position[2]),
                true,
            )
        });
        run.sync_world_activity(&reference).unwrap();
        assert!(
            !run.physics.body_active(body),
            "a weed off the loaded world unloads"
        );
        assert!(run.area.items.get(weed).unwrap().inactive_pose.is_some());
        spawned::despawn(&mut run.area, &mut run.physics, weed).unwrap();
        run.sync_world_activity(&reference).unwrap();
        assert!(!run.physics.inactive_bodies.contains(&body));
    }
}
