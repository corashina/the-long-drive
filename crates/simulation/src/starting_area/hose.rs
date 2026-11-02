//! Garage siphon hose: one spawned `item_siphon_hose` and its runtime hose.
use super::StartingArea;
use super::spawned::SpawnedItems;
use crate::reference::{HOSE_SEGMENTS, ItemId, ItemPrototype, ReferenceError, ReferenceManifest};
use crate::vehicle::{PhysicsWorld, VehiclePose, from_rapier, to_rapier};
use rapier3d::math::{Pose, Rotation, Vector};
use rapier3d::prelude::{QueryFilter, Ray};

/// Side distance from the 20 L fuel can to the hose root.
const CAN_SIDE: f32 = 0.6;
/// Height of the lowest segment center above the floor.
const FLOOR_CLEARANCE: f32 = 0.05;

/// Index of the hose prototype in `ReferenceManifest::item_prototypes`.
pub(super) fn prototype(reference: &ReferenceManifest) -> Result<usize, ReferenceError> {
    reference
        .item_prototypes
        .iter()
        .position(|p| p.hose.is_some())
        .ok_or_else(|| ReferenceError("Missing hose prototype".into()))
}

/// Pickup index of each hose segment, in `hose.segments` order.
fn segment_pickups(record: &ItemPrototype) -> Result<[usize; HOSE_SEGMENTS], ReferenceError> {
    let hose = record
        .hose
        .as_ref()
        .ok_or_else(|| ReferenceError("Prototype has no hose".into()))?;
    let mut pickups = [0; HOSE_SEGMENTS];
    for (pickup, body) in pickups.iter_mut().zip(hose.segments) {
        *pickup = record
            .pickups
            .iter()
            .position(|p| p.body == body)
            .ok_or_else(|| ReferenceError("Hose segment has no pickup".into()))?;
    }
    Ok(pickups)
}

/// Items of one spawned hose serial, in segment order. Every segment must exist.
fn segment_items(
    spawned: &SpawnedItems,
    prototype: usize,
    serial: u32,
    pickups: [usize; HOSE_SEGMENTS],
) -> Result<[ItemId; HOSE_SEGMENTS], ReferenceError> {
    let mut items = [ItemId(0); HOSE_SEGMENTS];
    for (item, pickup) in items.iter_mut().zip(pickups) {
        *item = spawned
            .iter()
            .find(|s| s.prototype == prototype && s.serial == serial && s.pickup == pickup)
            .ok_or_else(|| ReferenceError("Spawned hose segment is missing".into()))?
            .id;
    }
    Ok(items)
}

/// Bind the runtime hose of one spawned hose serial.
fn register(
    reference: &ReferenceManifest,
    area: &mut StartingArea,
    physics: &mut PhysicsWorld,
    prototype: usize,
    serial: u32,
) -> Result<(), ReferenceError> {
    let record = &reference.item_prototypes[prototype];
    let segments = segment_items(&area.spawned, prototype, serial, segment_pickups(record)?)?;
    area.interaction
        .register_hose(record, segments, &area.items, physics)
}

/// Save restore: rebuild one runtime hose per restored hose serial.
pub(super) fn restore(
    reference: &ReferenceManifest,
    area: &mut StartingArea,
    physics: &mut PhysicsWorld,
) -> Result<(), ReferenceError> {
    let prototype = prototype(reference)?;
    let mut serials: Vec<u32> = area
        .spawned
        .iter()
        .filter(|s| s.prototype == prototype)
        .map(|s| s.serial)
        .collect();
    serials.sort_unstable();
    serials.dedup();
    for serial in serials {
        register(reference, area, physics, prototype, serial)?;
    }
    Ok(())
}

/// Save validation: each saved hose serial holds all its segments.
pub fn validate_save(
    reference: &ReferenceManifest,
    saved: &[crate::save::SpawnedItemSnapshot],
) -> Result<(), crate::save::SaveError> {
    let prototype = prototype(reference)?;
    let key = crate::save::SourceKey::from(&reference.item_prototypes[prototype].source);
    let hose = |r: &&crate::save::SpawnedItemSnapshot| r.prototype == key;
    // Pickups are unique and in range per serial, so the count proves completeness.
    for record in saved.iter().filter(hose) {
        let count = saved
            .iter()
            .filter(hose)
            .filter(|r| r.serial == record.serial)
            .count();
        if count != HOSE_SEGMENTS {
            return Err(crate::save::SaveError::InvalidLinks(
                "Incomplete spawned hose".into(),
            ));
        }
    }
    Ok(())
}

/// New game: the hose lies flat on the garage floor, beside the 20 L fuel can.
pub(super) fn spawn_start(
    reference: &ReferenceManifest,
    area: &mut StartingArea,
    physics: &mut PhysicsWorld,
) -> Result<(), ReferenceError> {
    let prototype = prototype(reference)?;
    let can = reference
        .tanks
        .iter()
        .find(|t| {
            t.role == crate::reference::TankRole::Portable
                && (t.capacity_liters - 20.0).abs() < 0.5
                && t.fluids.iter().any(|f| f.fluid_type == 0)
        })
        .ok_or_else(|| ReferenceError("Missing 20 L fuel can".into()))?;
    let can = area
        .items
        .get(can.item)
        .ok_or_else(|| ReferenceError("Missing 20 L fuel can item".into()))?;
    let can_body = can
        .body
        .ok_or_else(|| ReferenceError("20 L fuel can has no body".into()))?;
    let can_pose = *physics.rapier().bodies[can_body].position() * to_rapier(can.local_pose);
    // Yaw only: the hose lies flat whatever the can tilt.
    let side = (can_pose.rotation * Vector::X)
        .with_y(0.0)
        .normalize_or(Vector::X);
    let rotation = Rotation::from_rotation_y((-side.z).atan2(side.x));
    let target = can_pose.translation + side * CAN_SIDE;
    let ray = Ray::new(target + Vector::Y * 0.5, -Vector::Y);
    let filter = QueryFilter::exclude_dynamic().exclude_sensors();
    let (_, distance) = physics
        .queries(filter)
        .cast_ray(&ray, 2.0, true)
        .ok_or_else(|| ReferenceError("No garage floor beside the fuel can".into()))?;
    let floor = ray.origin.y - distance;
    let record = &reference.item_prototypes[prototype];
    let lowest = record
        .pickups
        .iter()
        .filter_map(|p| record.bodies.iter().find(|b| b.id == p.body))
        .map(|b| (rotation * Vector::from_array(b.pose.translation)).y)
        .fold(f32::INFINITY, f32::min);
    let root = from_rapier(&Pose::from_parts(
        target.with_y(floor + FLOOR_CLEARANCE - lowest),
        rotation,
    ));
    spawn(reference, area, physics, prototype, root)
}

fn spawn(
    reference: &ReferenceManifest,
    area: &mut StartingArea,
    physics: &mut PhysicsWorld,
    prototype: usize,
    root: VehiclePose,
) -> Result<(), ReferenceError> {
    let id = super::spawned::spawn_prototype(reference, area, physics, prototype, root)?[0];
    let serial = area.spawned.get(id).unwrap().serial;
    register(reference, area, physics, prototype, serial)
}
