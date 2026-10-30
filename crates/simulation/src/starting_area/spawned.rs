//! Runtime copies of exported item prototypes. One item owns each prototype pickup body.
use super::StartingArea;
use crate::interaction::guns::AmmoItem;
use crate::interaction::items::HitSound;
use crate::interaction::spray::Ammo;
use crate::reference::{ItemId, ItemPrototype, ReferenceError, ReferenceManifest};
use crate::vehicle::{PhysicsWorld, VehiclePose, collider_builder, from_rapier, to_rapier};
use crate::world::stream::content_bindings::hit_events;
use rapier3d::prelude::{Group, InteractionGroups, InteractionTestMode, RigidBodyBuilder};

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct SpawnedItem {
    pub id: ItemId,
    /// Save identity of one spawn call. All bodies of that call share it.
    pub serial: u32,
    /// Index in `ReferenceManifest::item_prototypes`.
    pub prototype: usize,
    /// Index in `ItemPrototype::pickups`.
    pub pickup: usize,
}

#[derive(Default)]
pub struct SpawnedItems {
    items: Vec<SpawnedItem>,
    next_serial: u32,
    /// The ammoscript of each spawned magazine.
    ammo: Vec<(ItemId, AmmoItem)>,
}

impl SpawnedItems {
    pub fn get(&self, id: ItemId) -> Option<&SpawnedItem> {
        self.items.iter().find(|item| item.id == id)
    }
    pub fn contains(&self, id: ItemId) -> bool {
        self.get(id).is_some()
    }
    pub fn iter(&self) -> impl Iterator<Item = &SpawnedItem> {
        self.items.iter()
    }
    pub fn len(&self) -> usize {
        self.items.len()
    }
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }
    pub fn ammo(&self, id: ItemId) -> Option<AmmoItem> {
        self.ammo.iter().find(|(i, _)| *i == id).map(|(_, a)| *a)
    }
    pub fn set_ammo(&mut self, id: ItemId, ammo: AmmoItem) {
        if let Some((_, a)) = self.ammo.iter_mut().find(|(i, _)| *i == id) {
            *a = ammo;
        }
    }
    /// Restored serials stay unique: later spawns start after the largest one.
    pub(super) fn record(&mut self, item: SpawnedItem) {
        self.next_serial = self.next_serial.max(item.serial.saturating_add(1));
        self.items.push(item);
    }
    fn allocate_serial(&mut self) -> Result<u32, ReferenceError> {
        let serial = self.next_serial;
        self.next_serial = serial
            .checked_add(1)
            .ok_or_else(|| ReferenceError("Spawn identity space exhausted".into()))?;
        Ok(serial)
    }
}

/// Body pose of one prototype pickup when the prototype root has `root`.
pub fn body_pose(prototype: &ItemPrototype, pickup: usize, root: VehiclePose) -> VehiclePose {
    let body = prototype_body(prototype, pickup);
    from_rapier(
        &(to_rapier(root)
            * to_rapier(VehiclePose {
                position: body.pose.translation,
                rotation: body.pose.rotation,
            })),
    )
}

fn prototype_body(prototype: &ItemPrototype, pickup: usize) -> &crate::reference::BodyRecord {
    let id = prototype.pickups[pickup].body;
    prototype
        .bodies
        .iter()
        .find(|body| body.id == id)
        .expect("validated pickup body")
}

/// Spawn every pickup body of a prototype. `root` is the prototype root pose in physics space.
pub fn spawn_prototype(
    reference: &ReferenceManifest,
    area: &mut StartingArea,
    physics: &mut PhysicsWorld,
    prototype: usize,
    root: VehiclePose,
) -> Result<Vec<ItemId>, ReferenceError> {
    let record = reference
        .item_prototypes
        .get(prototype)
        .ok_or_else(|| ReferenceError("Missing item prototype".into()))?;
    let serial = area.spawned.allocate_serial()?;
    let mut ids = Vec::with_capacity(record.pickups.len());
    for pickup in 0..record.pickups.len() {
        ids.push(spawn_body(
            reference,
            area,
            physics,
            prototype,
            pickup,
            body_pose(record, pickup, root),
            serial,
        )?);
    }
    Ok(ids)
}

/// Spawn one prototype pickup body at `pose`. Save restore uses this with the saved serial.
pub fn spawn_body(
    reference: &ReferenceManifest,
    area: &mut StartingArea,
    physics: &mut PhysicsWorld,
    prototype: usize,
    pickup: usize,
    pose: VehiclePose,
    serial: u32,
) -> Result<ItemId, ReferenceError> {
    let record = reference
        .item_prototypes
        .get(prototype)
        .ok_or_else(|| ReferenceError("Missing item prototype".into()))?;
    let rule = record
        .pickups
        .get(pickup)
        .ok_or_else(|| ReferenceError("Missing prototype pickup".into()))?;
    if !pose
        .position
        .iter()
        .chain(&pose.rotation)
        .all(|v| v.is_finite())
    {
        return Err(ReferenceError("Nonfinite spawned item pose".into()));
    }
    let body = prototype_body(record, pickup);
    let hit = rule
        .collision_clip
        .as_ref()
        .and_then(|clip| reference.clip_id(clip))
        .map(|clip| HitSound {
            clip,
            gain: rule.collision_gain,
        });
    let id = area.items.allocate_item_id()?;
    let builder = if body.dynamic {
        RigidBodyBuilder::dynamic()
            .ccd_enabled(true)
            .additional_mass(body.mass_kg)
            .linear_damping(body.linear_damping)
            .angular_damping(body.angular_damping)
    } else {
        RigidBodyBuilder::kinematic_position_based()
    };
    let handle = physics.insert_body(builder.pose(to_rapier(pose)));
    let mut colliders = Vec::new();
    for c in record.colliders.iter().filter(|c| c.body == body.id) {
        let collider_id = area.interaction.allocate_generated_collider()?;
        let collider = collider_builder(&c.geometry)?
            .friction(c.friction)
            .restitution(c.restitution)
            .sensor(c.sensor)
            .enabled(c.enabled)
            .solver_groups(crate::vehicle::solver_groups(c.enabled))
            .density(0.)
            .active_events(hit_events(hit))
            .collision_groups(InteractionGroups::new(
                Group::from_bits_truncate(c.membership),
                Group::from_bits_truncate(c.filter),
                InteractionTestMode::And,
            ))
            .user_data(
                ((collider_id.0 as u128) << 64)
                    | crate::vehicle::step_surface_bits(c.step_surface)
                    | crate::vehicle::tag_bits(c.tag)
                    | (id.0 as u128 + 1),
            )
            .build();
        colliders.push((
            physics.insert_collider_with_parent(collider, handle),
            collider_id,
            c.membership,
        ));
    }
    area.items.insert_spawned(id, "", pose, handle, hit)?;
    area.interaction
        .register_spawned(id, rule.pickup.clone(), &colliders);
    area.item_render_poses.push((id, pose, pose));
    area.spawned.record(SpawnedItem {
        id,
        serial,
        prototype,
        pickup,
    });
    if let Some(a) = record.behavior.ammo_rules {
        let ammo = Ammo {
            count: a.count,
            maximum: a.maximum,
            float: a.float,
            kind: a.kind,
        };
        area.spawned.ammo.push((
            id,
            AmmoItem {
                ammo,
                is_mag: a.is_mag,
            },
        ));
    }
    physics.refresh_queries();
    Ok(id)
}

/// Remove a spawned item and its body. Held or stored items drop first.
pub fn despawn(
    area: &mut StartingArea,
    physics: &mut PhysicsWorld,
    id: ItemId,
) -> Result<(), ReferenceError> {
    let index = area
        .spawned
        .items
        .iter()
        .position(|item| item.id == id)
        .ok_or_else(|| ReferenceError("Item is not spawned".into()))?;
    if area.interaction.held() == Some(id) {
        area.interaction.drop_held(&mut area.items, physics);
    }
    area.inventory.forget(id);
    let body = area
        .items
        .get(id)
        .and_then(|item| item.body)
        .ok_or_else(|| ReferenceError("Spawned item has no body".into()))?;
    area.interaction.remove_spawned(id, body);
    physics.remove_body(body);
    physics.refresh_queries();
    let state = area.items.set_body(id, None).unwrap();
    state.location = crate::interaction::items::ItemLocation::World;
    area.items.remove_spawned(id);
    area.item_render_poses.retain(|(item, _, _)| *item != id);
    area.spawned.items.swap_remove(index);
    area.spawned.ammo.retain(|(i, _)| *i != id);
    Ok(())
}

impl SpawnedItems {
    /// Every spawned body except gibs is saved in the world at its current pose.
    pub fn capture_save(
        &self,
        reference: &ReferenceManifest,
        items: &crate::interaction::items::ItemStore,
        guns: &crate::interaction::guns::GunState,
        origin: &crate::world::WorldOrigin,
    ) -> Vec<crate::save::SpawnedItemSnapshot> {
        let mut records: Vec<_> = self
            .items
            .iter()
            .filter_map(|spawned| {
                let prototype = &reference.item_prototypes[spawned.prototype];
                // Gib prototypes have no item id. Source gibs are not saved.
                prototype.item_id?;
                let state = items.get(spawned.id)?;
                let pose = state.inactive_pose.unwrap_or(crate::save::AbsolutePose {
                    position: origin.absolute(state.pose.position),
                    rotation: state.pose.rotation,
                });
                Some(crate::save::SpawnedItemSnapshot {
                    serial: spawned.serial,
                    prototype: (&prototype.source).into(),
                    pickup: spawned.pickup as u32,
                    pose,
                    gun: guns.snapshot(spawned.id),
                    // None at the prefab count.
                    ammo: self.ammo(spawned.id).map(|a| a.ammo.count).filter(|count| {
                        prototype
                            .behavior
                            .ammo_rules
                            .is_none_or(|r| r.count != *count)
                    }),
                })
            })
            .collect();
        records.sort_by_key(|r| (r.serial, r.pickup));
        records
    }
}

/// Prototype index and pickup of a validated spawned snapshot.
pub fn saved_prototype(
    reference: &ReferenceManifest,
    saved: &crate::save::SpawnedItemSnapshot,
) -> Result<(usize, usize), crate::save::SaveError> {
    let prototype = reference
        .item_prototypes
        .iter()
        .position(|p| crate::save::SourceKey::from(&p.source) == saved.prototype)
        .ok_or_else(|| crate::save::SaveError::InvalidLinks("Unknown spawned prototype".into()))?;
    let pickup = saved.pickup as usize;
    if pickup >= reference.item_prototypes[prototype].pickups.len() {
        return Err(crate::save::SaveError::InvalidLinks(
            "Unknown spawned pickup".into(),
        ));
    }
    Ok((prototype, pickup))
}

/// Rebuild saved spawned bodies. Call after the area origin matches the save.
pub fn restore_save(
    reference: &ReferenceManifest,
    area: &mut StartingArea,
    physics: &mut PhysicsWorld,
    origin: &crate::world::WorldOrigin,
    saved: &[crate::save::SpawnedItemSnapshot],
) -> Result<Vec<ItemId>, crate::save::SaveError> {
    let mut ids = Vec::with_capacity(saved.len());
    for record in saved {
        let (prototype, pickup) = saved_prototype(reference, record)?;
        ids.push(spawn_body(
            reference,
            area,
            physics,
            prototype,
            pickup,
            VehiclePose {
                position: origin.local(record.pose.position),
                rotation: record.pose.rotation,
            },
            record.serial,
        )?);
        if let Some(count) = record.ammo {
            let id = *ids.last().unwrap();
            let ammo = area
                .spawned
                .ammo(id)
                .and_then(|a| {
                    Some(AmmoItem {
                        ammo: a.ammo.with_count(count)?,
                        ..a
                    })
                })
                .ok_or_else(|| crate::save::SaveError::CorruptData("Spawned ammo".into()))?;
            area.spawned.set_ammo(id, ammo);
        }
    }
    Ok(ids)
}
