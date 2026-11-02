//! Car prototypes and engine parts of spawned world contents.
//! A car copy is a vehicle body, a car runtime, its wheel slots and the engine and radiator
//! parts on it.
use crate::crash::Blood;
use crate::interaction::InteractionState;
use crate::interaction::cleaning::WashOwner;
use crate::interaction::items::ItemStore;
use crate::interaction::wheels::{WheelAssemblies, WorldWheelSlot};
use crate::reference::{
    Bounds, ColliderId, EnginePartReference, ItemId, RadiatorPartReference, ReferenceError,
    WorldPrototypeReference, WorldReference,
};
use crate::simulation::car_lights::CarLightLinks;
use crate::simulation::powertrain::{EnginePart, GaugePart, PowertrainParts, RadiatorPart};
use crate::starting_area::VehicleRuntime;
use crate::vehicle::PhysicsWorld;
use crate::world::changes::ChangeRecords;
use crate::world::contents::ContentInstance;
use crate::world::spawn_rules::{SourceRules, number, optional, texts};
use crate::world::{GeneratedId, ObjectKey, StructureInstance};
use rapier3d::prelude::{ColliderHandle, RigidBodyHandle};
use std::collections::BTreeMap;

/// Binds the engine and radiator parts and the car of one bound content.
/// Call after slots, parts and wheels bind and before seats bind: seats find the vehicle.
#[allow(clippy::too_many_arguments)]
pub(super) fn bind(
    content: &ContentInstance,
    instance: &StructureInstance,
    prototype: &WorldPrototypeReference,
    world: &WorldReference,
    skid: &crate::reference::SkidReference,
    rules: &SourceRules,
    items: &mut ItemStore,
    interaction: &mut InteractionState,
    physics: &mut PhysicsWorld,
    wheels: &mut WheelAssemblies,
    vehicles: &mut BTreeMap<ObjectKey, VehicleRuntime>,
    powertrain: &mut PowertrainParts,
    changes: &mut ChangeRecords,
) -> Result<(), ReferenceError> {
    let node_item = |node: u32| {
        crate::world::content_mechanisms::node_item(content, prototype, world, rules, node as usize)
    };
    // The car first: parts register only when nothing can fail.
    if let Some(record) = &prototype.vehicle {
        if content.item.is_none() {
            return Err(ReferenceError("Car prototype root has no item".into()));
        }
        let chassis = *instance
            .bodies
            .get(record.body as usize)
            .ok_or_else(|| ReferenceError("Car chassis body is not published".into()))?;
        let colliders = record
            .colliders
            .iter()
            .map(|&index| {
                let source = &prototype.colliders[index as usize].source;
                instance
                    .colliders
                    .iter()
                    .find(|(s, _)| s == source)
                    .map(|(_, handle)| *handle)
                    .ok_or_else(|| ReferenceError("Car chassis collider is not published".into()))
            })
            .collect::<Result<Vec<_>, _>>()?;
        let index = world
            .prototypes
            .iter()
            .position(|p| p.source == prototype.source)
            .ok_or_else(|| ReferenceError("Car prototype missing".into()))?;
        let electronics_of = |node: Option<u32>| -> Result<Option<f32>, ReferenceError> {
            Ok(node
                .map(|node| electronics_output(world, rules, prototype, node))
                .transpose()?
                .flatten())
        };
        // The nearest item that holds a node: a part, a usable or the car root.
        let owner_item = |node: u32| {
            let mut cursor = Some(node);
            while let Some(n) = cursor {
                if let Some(item) = node_item(n) {
                    return Some(item);
                }
                cursor = prototype.nodes.get(n as usize).and_then(|n| n.parent);
            }
            None
        };
        let lights = record
            .lights
            .iter()
            .map(|light| {
                Ok(CarLightLinks {
                    electronics: electronics_of(light.electronics)?,
                    owner: owner_item(light.node).filter(|item| Some(*item) != content.item),
                    doors: light
                        .cabin
                        .iter()
                        .flat_map(|c| &c.door_slots)
                        .map(|&node| owner_item(node))
                        .collect(),
                })
            })
            .collect::<Result<Vec<_>, ReferenceError>>()?;
        let electronics = electronics_of(record.electronics)?;
        let (slot_sources, wheel_slots) = wheel_slots(content, prototype, world, rules, items)?;
        let owner = content.item.unwrap();
        let key = ObjectKey::Generated(content.content.placement.id.clone());
        let blood = record
            .blood
            .as_ref()
            .map(|r| {
                let chassis = prototype.bodies[record.body as usize].node as usize;
                bind_blood(r, chassis, instance, prototype, interaction, physics)
            })
            .transpose()?;
        let ids = spawn(
            CarCopy {
                key: key.clone(),
                prototype: index,
                chassis,
                colliders: &colliders,
                bodies: instance
                    .bodies
                    .iter()
                    .copied()
                    .filter(|body| *body != chassis)
                    .collect(),
                fuel_tank: record.fuel_tank.and_then(node_item),
                electronics,
                lights,
                light_switch: record
                    .light_switch
                    .as_ref()
                    .and_then(|s| node_item(s.usable)),
                handbrake_lever: world
                    .mechanisms
                    .iter()
                    .find(|m| {
                        m.prototype == prototype.source
                            && m.recipe.turn.as_ref().is_some_and(|t| {
                                matches!(t.action, crate::reference::TurnAction::ToggleHandbrake)
                            })
                    })
                    .and_then(|m| node_item(m.node)),
                owner,
                wheel_slots: &wheel_slots,
                // The record mass holds the wheel parts of the prototype.
                wheel_part_mass: content
                    .bindings
                    .iter()
                    .filter_map(|b| wheels.part_mass(b.item))
                    .sum(),
                tanks: std::iter::once(owner)
                    .chain(content.bindings.iter().map(|b| b.item))
                    .filter(|&id| items.get(id).is_some_and(|i| i.fluid.is_some()))
                    .fold(Vec::new(), |mut tanks, id| {
                        if !tanks.contains(&id) {
                            tanks.push(id);
                        }
                        tanks
                    }),
                blood,
            },
            prototype,
            skid,
            physics,
            vehicles,
            wheels,
            items,
            changes,
        )?;
        // carscript.kmhmeters slots: (row, slot) pairs.
        let mut kmh_slots = Vec::new();
        for (row, k) in record.kmh_meters.iter().enumerate() {
            for source in &k.slots {
                let slot = content
                    .slots
                    .iter()
                    .find(|s| &s.source == source)
                    .ok_or_else(|| ReferenceError("Car gauge slot is not bound".into()))?;
                kmh_slots.push((row, slot.slot));
            }
        }
        let fuel_tank_slot = record
            .fuel_tank_slot
            .as_ref()
            .map(|source| {
                content
                    .slots
                    .iter()
                    .find(|s| &s.source == source)
                    .map(|s| s.slot)
                    .ok_or_else(|| ReferenceError("Car tank slot is not bound".into()))
            })
            .transpose()?;
        let vehicle = vehicles.get_mut(&key).unwrap();
        vehicle.kmh_slots = kmh_slots;
        vehicle.fuel_tank_slot = fuel_tank_slot;
        for ((id, source), slot) in ids.into_iter().zip(slot_sources).zip(&wheel_slots) {
            interaction.register_wheel_slot(id, owner, source, slot, wheels.rules());
        }
        for (sensor, rim) in wheels.sensor_rims() {
            interaction.bind_wheel_sensor(sensor, rim, physics);
        }
    }
    for record in &prototype.engines {
        // A random part rule can leave the engine node without an item.
        let Some(item) = node_item(record.node) else {
            continue;
        };
        let oil_tank = record.oil_tank.and_then(node_item);
        register_engine(
            item, record, prototype, oil_tank, items, powertrain, changes,
        )?;
    }
    for record in &prototype.radiators {
        let Some(item) = node_item(record.node) else {
            continue;
        };
        let coolant_tank = record.coolant_tank.and_then(node_item);
        register_radiator(
            item,
            record,
            coolant_tank,
            content.content.placement.id.clone(),
            items,
            powertrain,
            changes,
        );
    }
    for record in &prototype.gauges {
        // The gauge part: the nearest item at or above the meter node.
        let mut cursor = Some(record.node);
        let mut item = None;
        while let Some(n) = cursor {
            item = node_item(n);
            if item.is_some() {
                break;
            }
            cursor = prototype.nodes.get(n as usize).and_then(|n| n.parent);
        }
        let Some(item) = item else {
            continue;
        };
        let mut part = GaugePart::new(item, record.clone(), content.content.placement.id.clone());
        if let Some(saved) = changes.take_gauge(&items.object_key(item))
            && !part.state.restore_save(record, &saved)
        {
            return Err(ReferenceError(
                "Saved gauge values do not fit the gauge".into(),
            ));
        }
        powertrain.register_gauge(part);
    }
    if let Some(owner) = content.item {
        bind_help_texts(owner, instance, prototype, interaction);
    }
    let key = ObjectKey::Generated(content.content.placement.id.clone());
    if let (Some(owner), Some(blood)) = (
        content.item,
        vehicles.get(&key).and_then(|v| v.blood.as_ref()),
    ) {
        // A restored `GotDirty` shows the trigger. Repair progress starts at 0.
        let restored = blood.shown == Some(true);
        if restored {
            blood.set_colliders(physics, true);
        }
        // Source: the sponge washes the trigger while its GameObject is active.
        let appearance = &content.content.appearance;
        if restored
            || crate::world::spawn_appearance::node_active(
                prototype,
                blood.nodes[0] as usize,
                appearance,
            )
        {
            interaction.cleaning.register_generated_wash(
                WashOwner::Car(owner),
                blood.permission,
                blood.never_repair,
                blood.colliders.clone(),
            );
        }
    }
    Ok(())
}

/// The help box colliders of one copy: the colliders at and below each `helptextscript` node.
fn bind_help_texts(
    owner: ItemId,
    instance: &StructureInstance,
    prototype: &WorldPrototypeReference,
    interaction: &mut InteractionState,
) {
    for help in &prototype.help_texts {
        let bits = crate::interaction::help_texts::bits(help);
        for (source, handle) in &instance.colliders {
            let below = prototype
                .colliders
                .iter()
                .find(|c| &c.source == source)
                .is_some_and(|c| {
                    let mut node = Some(c.node);
                    while let Some(n) = node {
                        if n == help.node {
                            return true;
                        }
                        node = prototype.nodes[n as usize].parent;
                    }
                    false
                });
            if below {
                interaction.help_texts.add(*handle, owner, bits);
            }
        }
    }
}

/// The car blood trigger of one car copy. Colliders at and below the trigger node wash it.
fn bind_blood(
    r: &crate::reference::CarBloodReference,
    chassis: usize,
    instance: &StructureInstance,
    prototype: &WorldPrototypeReference,
    interaction: &mut InteractionState,
    physics: &mut PhysicsWorld,
) -> Result<Blood, ReferenceError> {
    let below = |mut i: u32| loop {
        if i == r.node {
            return true;
        }
        match prototype.nodes[i as usize].parent {
            Some(p) => i = p,
            None => return false,
        }
    };
    let mut colliders = Vec::new();
    for (source, handle) in &instance.colliders {
        let on_trigger = prototype
            .colliders
            .iter()
            .find(|c| &c.source == source)
            .is_some_and(|c| c.enabled && below(c.node));
        if !on_trigger {
            continue;
        }
        let mut id = ColliderId((physics.rapier().colliders[*handle].user_data >> 64) as u32);
        if id.0 == 0 {
            id = interaction.allocate_generated_collider()?;
            physics.edit_collider(*handle, |c| c.user_data |= (id.0 as u128) << 64);
        }
        colliders.push((id, *handle));
    }
    Ok(Blood {
        on: r.on,
        shown: None,
        permission: r.permission,
        never_repair: r.never_repair,
        nodes: std::iter::once(r.node)
            .chain(r.objects.iter().copied())
            .collect(),
        colliders,
        point: crate::world::placement::node_frame(prototype, chassis, r.node as usize).position,
    })
}

/// The source slot and the wheel slot of each record wheel. The slot rim is the prototype
/// part: the selected embedded part, or the part that the slot spawns.
fn wheel_slots(
    content: &ContentInstance,
    prototype: &WorldPrototypeReference,
    world: &WorldReference,
    rules: &SourceRules,
    items: &ItemStore,
) -> Result<(Vec<crate::reference::SourceId>, Vec<WorldWheelSlot>), ReferenceError> {
    use rapier3d::math::{Pose, Vector};
    let record = prototype.vehicle.as_ref().unwrap();
    let body = &prototype.bodies[record.body as usize];
    let to_chassis = crate::vehicle::to_rapier(crate::vehicle::VehiclePose {
        position: body.pose.translation,
        rotation: body.pose.rotation,
    })
    .inverse();
    let id = &content.content.placement.id;
    let mut sources = Vec::with_capacity(record.wheels.len());
    let mut slots = Vec::with_capacity(record.wheels.len());
    for wheel in &record.wheels {
        let node = &prototype.nodes[wheel.slot as usize];
        let slot = rules
            .component(world, &node.source, "partslotscript")?
            .ok_or_else(|| ReferenceError("Car wheel slot has no partslotscript".into()))?;
        let f = slot.fields;
        let part = content
            .content
            .inputs
            .slot_parts
            .iter()
            .find(|(parent, s, _)| parent == id && s == slot.source)
            .map(|v| &v.2)
            .or(optional(f, "part")?);
        let rim = if let Some(part) = part {
            content
                .bindings
                .iter()
                .find(|b| &b.source == part)
                .map(|b| items.object_key(b.item))
        } else {
            let mut child = id.clone();
            child.children.push((slot.source.clone(), 0));
            Some(ObjectKey::Generated(child))
        };
        let collider = &wheel.collider;
        let connection = (to_chassis * prototype.root_relative(wheel.node as usize).0)
            * Vector::from_array(collider.center);
        let rest = collider.suspension_distance * (1.0 - collider.target_position);
        let hub = Pose::from_translation(connection - Vector::Y * rest);
        let mount = to_chassis * prototype.root_relative(wheel.mount as usize).0;
        sources.push(slot.source.clone());
        slots.push(WorldWheelSlot {
            rim_main_types: texts(f, "maintipus")?,
            rim_types: texts(f, "tipus")?,
            hub_to_mount: hub.inverse() * mount,
            sensor_radius: collider.radius,
            rim,
        });
    }
    Ok((sources, slots))
}

/// Registers the engine part of `item` with its kept state.
pub fn register_engine(
    item: ItemId,
    record: &EnginePartReference,
    prototype: &WorldPrototypeReference,
    oil_tank: Option<ItemId>,
    items: &ItemStore,
    powertrain: &mut PowertrainParts,
    changes: &mut ChangeRecords,
) -> Result<(), ReferenceError> {
    let mut part = EnginePart::new(item, record.clone(), oil_tank);
    if let Some(g) = &record.gear_stick {
        part.gear_stick = [g.transmission, g.kick_ignition].map(|n| {
            n.map(|n| super::placement::node_frame(prototype, record.node as usize, n as usize))
        });
    }
    if let Some(state) = changes.take_engine(&items.object_key(item)) {
        part.state
            .restore_save(&state)
            .map_err(|e| ReferenceError(format!("Saved engine state: {e}")))?;
    }
    powertrain.register_engine(part);
    Ok(())
}

/// Registers the radiator part of `item` with its kept fan state.
pub fn register_radiator(
    item: ItemId,
    record: &RadiatorPartReference,
    coolant_tank: Option<ItemId>,
    parent: GeneratedId,
    items: &ItemStore,
    powertrain: &mut PowertrainParts,
    changes: &mut ChangeRecords,
) {
    let mut part = RadiatorPart::new(item, record.clone(), coolant_tank, parent);
    if let Some(fan_on) = changes.take_radiator(&items.object_key(item)) {
        part.fan_on = fan_on;
    }
    powertrain.register_radiator(part);
}

/// Source `electronicsscript` output of prototype node `node`: `maxOutput` less the jam share.
/// None: the node has no electronics script.
pub fn electronics_output(
    world: &WorldReference,
    rules: &SourceRules,
    prototype: &WorldPrototypeReference,
    node: u32,
) -> Result<Option<f32>, ReferenceError> {
    let node = prototype
        .nodes
        .get(node as usize)
        .ok_or_else(|| ReferenceError("Car electronics node missing".into()))?;
    let Some(record) = rules.component(world, &node.source, "electronicsscript")? else {
        return Ok(None);
    };
    let f = record.fields;
    Ok(Some(
        number(f, "maxOutput")? * crate::world::spawn_lamps::electronics_factor(f)?,
    ))
}

/// One car copy of a world instance, as the binder spawns it.
pub struct CarCopy<'a> {
    pub key: ObjectKey,
    /// Index of the car prototype in `WorldReference::prototypes`.
    pub prototype: usize,
    pub chassis: RigidBodyHandle,
    /// Live handles of the record chassis colliders.
    pub colliders: &'a [ColliderHandle],
    /// Other bodies of the car instance.
    pub bodies: Vec<RigidBodyHandle>,
    /// Car root `tankscript` item.
    pub fuel_tank: Option<ItemId>,
    /// `carscript.electronics` output. None: the car has no electronics.
    pub electronics: Option<f32>,
    /// Links of each record light controller.
    pub lights: Vec<CarLightLinks>,
    /// The item of the `carscript.InteractSwitchLights` usable.
    pub light_switch: Option<ItemId>,
    /// The item of the `carscript.InteractHandbrake` usable.
    pub handbrake_lever: Option<ItemId>,
    /// The car root item.
    pub owner: ItemId,
    /// One slot per record wheel, in wheel order.
    pub wheel_slots: &'a [WorldWheelSlot],
    /// Mass of the prototype wheel parts. The record mass holds it.
    pub wheel_part_mass: f32,
    /// Prototype `tankscript` items. Their fluid adds to the chassis mass.
    pub tanks: Vec<ItemId>,
    /// Source `carscript.blood`.
    pub blood: Option<Blood>,
}

/// Makes the chassis a vehicle and adds its car runtime and wheel slots with the retired
/// state of its key. Mounts the slot rims that are bound. Returns the wheel slot ids.
#[allow(clippy::too_many_arguments)]
pub fn spawn(
    copy: CarCopy,
    prototype: &WorldPrototypeReference,
    skid: &crate::reference::SkidReference,
    physics: &mut PhysicsWorld,
    vehicles: &mut BTreeMap<ObjectKey, VehicleRuntime>,
    wheels: &mut WheelAssemblies,
    items: &mut ItemStore,
    changes: &mut ChangeRecords,
) -> Result<Vec<crate::interaction::wheels::WheelSlotId>, ReferenceError> {
    let record = prototype
        .vehicle
        .as_ref()
        .ok_or_else(|| ReferenceError("Prototype has no vehicle record".into()))?;
    let key = copy.key;
    if vehicles.contains_key(&key) {
        return Err(ReferenceError("Car runtime already exists".into()));
    }
    // Plate binder: each copy draws its labels from its key. A reload draws the same texts.
    let plates = crate::plates::copy_texts(&record.plates, key.seed())?;
    physics.add_vehicle(key.clone(), copy.chassis, copy.colliders, prototype)?;
    let mut runtime = VehicleRuntime::world(
        copy.prototype,
        record,
        skid,
        crate::starting_area::WorldCarParts {
            seed: key.seed(),
            fuel_tank: copy.fuel_tank,
            recovery_bounds: chassis_bounds(physics, copy.colliders),
            bodies: copy.bodies,
            electronics: copy.electronics,
            lights: copy.lights,
            light_switch: copy.light_switch,
            handbrake_lever: copy.handbrake_lever,
            plates,
            blood: copy.blood,
        },
    );
    if let Some(state) = changes.take_vehicle(&key) {
        if state.gear >= record.gear_ratios.len() {
            return Err(ReferenceError("Saved car gear is out of range".into()));
        }
        runtime.restore(record, &state);
    }
    let saved = changes.take_wheel_slots(&key);
    if saved
        .as_ref()
        .is_some_and(|rims| rims.len() != copy.wheel_slots.len())
    {
        return Err(ReferenceError("Saved car wheel count differs".into()));
    }
    vehicles.insert(key.clone(), runtime);
    let ids = wheels.add_vehicle(
        key,
        copy.owner,
        record.mass_kg - copy.wheel_part_mass,
        copy.tanks,
        copy.wheel_slots,
        saved,
        physics,
    );
    wheels.install_pending(items, physics)?;
    // Empty slots sit on the hub from the first step.
    wheels.sync(items, physics)?;
    Ok(ids)
}

/// Enabled solid chassis colliders in the chassis frame.
fn chassis_bounds(physics: &PhysicsWorld, colliders: &[ColliderHandle]) -> Bounds {
    let mut bounds = Bounds {
        min: [f32::MAX; 3],
        max: [f32::MIN; 3],
    };
    for &handle in colliders {
        let collider = &physics.rapier().colliders[handle];
        if collider.is_sensor() || !collider.is_enabled() {
            continue;
        }
        let local = collider
            .position_wrt_parent()
            .copied()
            .unwrap_or(rapier3d::math::Pose::IDENTITY);
        let aabb = collider.shape().compute_aabb(&local);
        for i in 0..3 {
            bounds.min[i] = bounds.min[i].min(aabb.mins[i]);
            bounds.max[i] = bounds.max[i].max(aabb.maxs[i]);
        }
    }
    bounds
}

/// Keeps the state of a retiring content in the world change records and removes its car
/// and its engine and radiator parts. Call before its items unload.
#[allow(clippy::too_many_arguments)]
pub(super) fn retire(
    content: &ContentInstance,
    items: &ItemStore,
    interaction: &mut InteractionState,
    physics: &mut PhysicsWorld,
    vehicles: &mut BTreeMap<ObjectKey, VehicleRuntime>,
    wheels: &mut WheelAssemblies,
    powertrain: &mut PowertrainParts,
    changes: &mut ChangeRecords,
) {
    for item in content
        .item
        .into_iter()
        .chain(content.bindings.iter().map(|b| b.item))
    {
        retire_parts(item, items, powertrain, changes);
    }
    if let Some(owner) = content.item {
        interaction.help_texts.remove_owner(owner);
        interaction
            .cleaning
            .remove_generated_wash(WashOwner::Car(owner));
        let key = ObjectKey::Generated(content.content.placement.id.clone());
        if vehicles.contains_key(&key) {
            for (id, sensor) in retire_vehicle(&key, physics, vehicles, wheels, changes) {
                interaction.remove_wheel_slot(id, sensor);
            }
        }
    }
}

/// Keeps the engine, radiator and gauge state of `item` and removes its parts.
pub fn retire_parts(
    item: ItemId,
    items: &ItemStore,
    powertrain: &mut PowertrainParts,
    changes: &mut ChangeRecords,
) {
    if let Some(part) = powertrain.engine(item) {
        changes.keep_engine(items.object_key(item), part.state.capture_save());
    }
    if let Some(part) = powertrain.radiator(item) {
        changes.keep_radiator(items.object_key(item), part.fan_on);
    }
    if let Some(part) = powertrain.gauge(item) {
        changes.keep_gauge(items.object_key(item), part.state.capture_save());
    }
    powertrain.remove(item);
}

/// Keeps the car-side state of car `key` and removes its runtime, wheel slots and vehicle
/// body data. Returns the removed slots and their sensors.
pub fn retire_vehicle(
    key: &ObjectKey,
    physics: &mut PhysicsWorld,
    vehicles: &mut BTreeMap<ObjectKey, VehicleRuntime>,
    wheels: &mut WheelAssemblies,
    changes: &mut ChangeRecords,
) -> Vec<(
    crate::interaction::wheels::WheelSlotId,
    Option<rapier3d::prelude::ColliderHandle>,
)> {
    if let Some(runtime) = vehicles.remove(key) {
        changes.keep_vehicle(key.clone(), runtime.state());
    }
    let slots = wheels
        .retire_vehicle(key, physics)
        .map_or(Vec::new(), |(rims, slots)| {
            changes.keep_wheel_slots(key.clone(), rims);
            slots
        });
    physics.remove_vehicle(key);
    slots
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fluids::{FluidContainer, FluidEntry};
    use crate::reference::SourceId;
    use crate::simulation::{DriveInput, FIXED_DT};
    use crate::vehicle::{DriveTorques, VehiclePose, to_rapier};
    use crate::world::{AbsolutePose, GeneratedId, WorldLayer};
    use rapier3d::prelude::{ColliderBuilder, RigidBodyBuilder};

    const START: [f64; 3] = [0., 0.5, 0.];

    /// One car copy as the world binder spawns it: instance body, root item with the
    /// spawn fuel, then the car and its wheel slots. Returns the root item.
    #[allow(clippy::too_many_arguments)]
    fn spawn_copy(
        id: &GeneratedId,
        prototype: &WorldPrototypeReference,
        physics: &mut PhysicsWorld,
        items: &mut ItemStore,
        vehicles: &mut BTreeMap<ObjectKey, VehicleRuntime>,
        wheels: &mut WheelAssemblies,
        wheel_slots: &[WorldWheelSlot],
        wheel_part_mass: f32,
        changes: &mut ChangeRecords,
    ) -> ItemId {
        let pose = AbsolutePose {
            position: START,
            rotation: [0., 0., 0., 1.],
        };
        let (body, collider) = physics.insert(
            RigidBodyBuilder::dynamic()
                .pose(to_rapier(VehiclePose {
                    position: START.map(|v| v as f32),
                    rotation: pose.rotation,
                }))
                .additional_mass(900.),
            ColliderBuilder::cuboid(0.8, 0.25, 2.0).density(0.),
        );
        let item = items.insert_generated(id.clone(), prototype, pose).unwrap();
        let state = items.set_body(item, Some(body)).unwrap();
        state.fluid = Some(FluidContainer::new(
            60.,
            &[FluidEntry {
                fluid_type: 2,
                amount: 40.,
            }],
        ));
        spawn(
            CarCopy {
                key: ObjectKey::Generated(id.clone()),
                prototype: 0,
                chassis: body,
                colliders: &[collider],
                bodies: Vec::new(),
                fuel_tank: Some(item),
                electronics: None,
                lights: Vec::new(),
                light_switch: None,
                handbrake_lever: None,
                owner: item,
                wheel_slots,
                wheel_part_mass,
                tanks: vec![item],
                blood: Some(Blood {
                    on: false,
                    shown: None,
                    permission: 0,
                    never_repair: false,
                    nodes: vec![1],
                    colliders: Vec::new(),
                    point: [0.; 3],
                }),
            },
            prototype,
            &crate::wheel_effects::tests::skid(),
            physics,
            vehicles,
            wheels,
            items,
            changes,
        )
        .unwrap();
        physics.refresh_queries();
        item
    }

    fn fuel(items: &ItemStore, item: ItemId) -> f32 {
        items.get(item).unwrap().fluid.as_ref().unwrap().total()
    }

    #[test]
    fn retired_car_returns_with_pose_gear_and_fuel() {
        let mut prototype = crate::vehicle::car_prototype_fixture();
        prototype.vehicle.as_mut().unwrap().fuel_tank = Some(0);
        let id = GeneratedId {
            layer: WorldLayer::Prop,
            generator: SourceId {
                file: "a".into(),
                path_id: 1,
            },
            unit: [0, 0],
            ordinal: 3,
            children: Vec::new(),
        };
        let key = ObjectKey::Generated(id.clone());
        let mut physics = PhysicsWorld::empty(1, 2);
        physics.insert_box([0., -0.5, 0.], [500., 0.5, 500.]);
        let mut items = ItemStore::from_states(Vec::new());
        let mut vehicles = BTreeMap::new();
        let mut changes = ChangeRecords::default();
        let item = spawn_copy(
            &id,
            &prototype,
            &mut physics,
            &mut items,
            &mut vehicles,
            &mut WheelAssemblies::default(),
            &[],
            0.,
            &mut changes,
        );
        assert_eq!(vehicles[&key].fuel_tank, Some(item));
        assert_eq!(vehicles[&key].drivetrain.gear(), 1, "initial gear");

        // Shift to gear 2, drive, burn fuel, set the handbrake.
        vehicles.get_mut(&key).unwrap().drivetrain.restore_gear(2);
        for _ in 0..(1. / FIXED_DT) as usize {
            physics.apply_vehicle_forces(
                &key,
                DriveInput {
                    throttle: 1.,
                    ..Default::default()
                },
                DriveTorques {
                    motor_nm: 1600.,
                    engine_brake_nm: 0.,
                    pedal_nm: None,
                },
            );
            physics.step_physics();
        }
        vehicles.get_mut(&key).unwrap().handbrake = true;
        items
            .get_mut(item)
            .unwrap()
            .fluid
            .as_mut()
            .unwrap()
            .restore_entries(&[FluidEntry {
                fluid_type: 2,
                amount: 12.5,
            }]);
        items.sync_poses(&physics);
        let moved = items.get(item).unwrap().pose.position;
        assert!(moved[2].abs() > 0.5 || moved[0].abs() > 0.5, "{moved:?}");

        // Retire: the change record path keeps pose and fuel, the binder keeps the car state.
        let body = items.get(item).unwrap().body.unwrap();
        let mut snapshot = items.generated_snapshot(item, None).unwrap();
        snapshot.linear_velocity = physics.rapier().bodies[body].linvel().to_array();
        retire_vehicle(
            &key,
            &mut physics,
            &mut vehicles,
            &mut WheelAssemblies::default(),
            &mut changes,
        );
        assert!(vehicles.is_empty());
        assert_eq!(physics.vehicle_count(), 0);
        items.set_body(item, None);
        assert!(items.remove_generated(item));
        physics.remove_body(body);

        // Respawn at the source placement, then restore the change record.
        let item = spawn_copy(
            &id,
            &prototype,
            &mut physics,
            &mut items,
            &mut vehicles,
            &mut WheelAssemblies::default(),
            &[],
            0.,
            &mut changes,
        );
        assert_eq!(fuel(&items, item), 40., "spawn fuel before the record");
        items.restore_generated(item, &snapshot).unwrap();
        crate::world::changes::restore_motion(item, &snapshot, &items, &mut physics);
        let body = items.get(item).unwrap().body.unwrap();
        assert_eq!(physics.vehicle_of(body), Some(&key));
        let car = &vehicles[&key];
        assert_eq!(car.drivetrain.gear(), 2);
        assert!(car.handbrake);
        assert_eq!(fuel(&items, item), 12.5);
        let position = physics.rapier().bodies[body].translation().to_array();
        for i in 0..3 {
            assert!(
                (position[i] - moved[i]).abs() < 1e-4,
                "{position:?} {moved:?}"
            );
        }
        // The respawned car drives on.
        assert!(physics.snapshot(&key).speed_mps > 1.);
    }

    #[test]
    fn plate_texts_differ_per_copy_and_survive_reload() {
        let mut prototype = crate::vehicle::car_prototype_fixture();
        let identity = [0., 0., 0., 1.];
        let plate = |node: u32, same_as: Option<u32>| {
            serde_json::from_value(serde_json::json!({
                "source": {"file": "a", "path_id": 40 + node}, "node": node, "text_node": node,
                "same_as": same_as, "initial_text": "", "initial_same": false,
                "font_source": {"file": "f", "path_id": 1}, "material_source": {"file": "f", "path_id": 2},
                "sorting": [0, 0],
                "layout": {"bounds": [1.0, 0.2], "pivot": [0.5, 0.5], "anchor_min": [0.5, 0.5],
                    "anchor_max": [0.5, 0.5], "font_size": 10.0, "font_size_base": 10.0, "font_size_min": 1.0,
                    "font_size_max": 20.0, "font_style": 1, "font_weight": 400, "horizontal_alignment": 2,
                    "vertical_alignment": 512, "auto_sizing": false, "kerning": true, "word_wrapping": false,
                    "extra_padding": false, "orthographic": true, "rich_text": false, "overflow_mode": 0,
                    "max_visible_descender": true, "spacing": [0.0, 0.0, 0.0, 0.0],
                    "margin": [0.0, 0.0, 0.0, 0.0], "color": [1.0, 1.0, 1.0, 1.0],
                    "transform": {"translation": [0.0, 0.0, 0.0], "rotation": identity, "scale": [1.0, 1.0, 1.0]}}
            }))
            .unwrap()
        };
        // The front label copies its text to the rear label.
        prototype.vehicle.as_mut().unwrap().plates = vec![plate(1, Some(2)), plate(2, None)];
        let copy = |ordinal| GeneratedId {
            layer: WorldLayer::Prop,
            generator: SourceId {
                file: "a".into(),
                path_id: 1,
            },
            unit: [0, 0],
            ordinal,
            children: Vec::new(),
        };
        // One session: spawn the copy and read its plate texts.
        let texts = |id: &GeneratedId| {
            let mut physics = PhysicsWorld::empty(1, 2);
            let mut items = ItemStore::from_states(Vec::new());
            let mut vehicles = BTreeMap::new();
            let mut changes = ChangeRecords::default();
            spawn_copy(
                id,
                &prototype,
                &mut physics,
                &mut items,
                &mut vehicles,
                &mut WheelAssemblies::default(),
                &[],
                0.,
                &mut changes,
            );
            vehicles
                .remove(&ObjectKey::Generated(id.clone()))
                .unwrap()
                .plates
        };
        let first = texts(&copy(3));
        assert_eq!(first.len(), 2);
        assert_eq!(first[0], first[1], "sameAsThis shares the text");
        let bytes = first[0].as_bytes();
        assert!(
            first[0].len() == 7
                && bytes[..3].iter().all(u8::is_ascii_uppercase)
                && bytes[3] == b'-'
                && bytes[4..].iter().all(u8::is_ascii_digit),
            "{first:?}"
        );
        assert_ne!(texts(&copy(4)), first, "each copy draws its own plate");
        assert_eq!(texts(&copy(3)), first, "a reload draws the same plate");
    }

    #[test]
    fn saved_car_returns_with_engine_gear_handbrake_and_blood() {
        #[derive(serde::Serialize, serde::Deserialize)]
        struct Saved {
            #[serde(with = "crate::save::identity::lossless")]
            changes: ChangeRecords,
        }
        let prototype = crate::vehicle::car_prototype_fixture();
        let engine_record: EnginePartReference =
            serde_json::from_value(crate::reference::engine_part_fixture()).unwrap();
        let radiator_record: RadiatorPartReference = serde_json::from_value(serde_json::json!({
            "source": {"file": "a", "path_id": 15}, "node": 2, "coolant_tank": null,
            "fan_node": 3, "thermostat_range": [80.0, 90.0], "fan_temperature_on": 95.0,
            "fan_temperature_off": 90.0, "fan_on_when_engine_runs": false, "max_value": 1.0,
            "max_fan_value": 1.0, "max_fan_speed": 1.0, "max_fluid_loss": 1.0
        }))
        .unwrap();
        let car = GeneratedId {
            layer: WorldLayer::Prop,
            generator: SourceId {
                file: "a".into(),
                path_id: i64::MIN + 1,
            },
            unit: [i64::MAX - 1, -3],
            ordinal: 3,
            children: Vec::new(),
        };
        let engine = GeneratedId {
            children: vec![(
                SourceId {
                    file: "a".into(),
                    path_id: -9,
                },
                7,
            )],
            ..car.clone()
        };
        let key = ObjectKey::Generated(car.clone());
        let engine_key = ObjectKey::Generated(engine.clone());
        // One session: a car copy with an engine and a radiator part.
        let run = |changes: &mut ChangeRecords| {
            let mut physics = PhysicsWorld::empty(1, 2);
            physics.insert_box([0., -0.5, 0.], [500., 0.5, 500.]);
            let mut items = ItemStore::from_states(Vec::new());
            let mut vehicles = BTreeMap::new();
            let mut powertrain = PowertrainParts::default();
            spawn_copy(
                &car,
                &prototype,
                &mut physics,
                &mut items,
                &mut vehicles,
                &mut WheelAssemblies::default(),
                &[],
                0.,
                changes,
            );
            let pose = AbsolutePose {
                position: START,
                rotation: [0., 0., 0., 1.],
            };
            let part = items
                .insert_generated(engine.clone(), &prototype, pose)
                .unwrap();
            assert_eq!(items.object_key(part), engine_key);
            register_engine(
                part,
                &engine_record,
                &prototype,
                None,
                &items,
                &mut powertrain,
                changes,
            )
            .unwrap();
            register_radiator(
                part,
                &radiator_record,
                None,
                car.clone(),
                &items,
                &mut powertrain,
                changes,
            );
            (vehicles, powertrain, part)
        };

        let mut changes = ChangeRecords::default();
        let (mut vehicles, mut powertrain, part) = run(&mut changes);
        let car_state = vehicles.get_mut(&key).unwrap();
        car_state.drivetrain.restore_gear(2);
        car_state.handbrake = true;
        car_state.ignition.ignition = true;
        car_state.blood.as_mut().unwrap().got_dirty();
        let mut engine_state = powertrain.engine(part).unwrap().state.capture_save();
        engine_state.rpm = 2400.;
        engine_state.temperature = 86.;
        engine_state.elapsed = 12.;
        engine_state.last_run_time = 11.99;
        engine_state.supplied_fuel = 0.3;
        engine_state.start_draw = "5".into();
        powertrain
            .engine_mut(part)
            .unwrap()
            .state
            .restore_save(&engine_state)
            .unwrap();
        powertrain.radiator_mut(part).unwrap().fan_on = true;

        // Save the live states as the game does, through JSON.
        changes.keep_vehicle(key.clone(), vehicles[&key].state());
        changes.keep_engine(
            engine_key.clone(),
            powertrain.engine(part).unwrap().state.capture_save(),
        );
        changes.keep_radiator(engine_key.clone(), true);
        let json = serde_json::to_vec(&Saved { changes }).unwrap();

        // Load into a new session.
        let mut loaded = serde_json::from_slice::<Saved>(&json).unwrap().changes;
        let (vehicles, powertrain, part) = run(&mut loaded);
        let car_state = &vehicles[&key];
        assert_eq!(car_state.drivetrain.gear(), 2);
        assert!(car_state.handbrake);
        assert!(car_state.ignition.ignition);
        let blood = car_state.blood.as_ref().unwrap();
        assert!(
            blood.on && blood.visible(1) == Some(true),
            "load runs GotDirty"
        );
        let restored = &powertrain.engine(part).unwrap().state;
        assert_eq!(restored.capture_save(), engine_state);
        assert!(restored.output().running);
        assert!(powertrain.radiator(part).unwrap().fan_on);
        // Each object took its state back.
        assert!(loaded.vehicle(&key).is_none() && loaded.engine(&engine_key).is_none());
    }
    /// A rim with a tire, as generated wheel parts on their own bodies. Returns the rim.
    fn wheel(
        car: &GeneratedId,
        prototype: &WorldPrototypeReference,
        physics: &mut PhysicsWorld,
        items: &mut ItemStore,
        wheels: &mut WheelAssemblies,
    ) -> ItemId {
        use crate::reference::{ColliderId, WheelPartReference};
        let part = |n: i64, physics: &mut PhysicsWorld, items: &mut ItemStore| {
            let mut id = car.clone();
            id.children.push((
                SourceId {
                    file: "a".into(),
                    path_id: n,
                },
                0,
            ));
            let pose = AbsolutePose {
                position: [3., 0.5, 0.],
                rotation: [0., 0., 0., 1.],
            };
            let (body, collider) = physics.insert(
                RigidBodyBuilder::dynamic().translation(rapier3d::math::Vector::new(3., 0.5, 0.)),
                ColliderBuilder::ball(0.3),
            );
            let item = items.insert_generated(id, prototype, pose).unwrap();
            items.set_body(item, Some(body));
            (item, collider)
        };
        let rule = |item, types: &str, radius| WheelPartReference {
            cover_slot: None,
            item,
            source: SourceId {
                file: "a".into(),
                path_id: item.0 as i64,
            },
            radius,
            main_types: Vec::new(),
            part_types: vec![types.into()],
            colliders: Vec::new(),
            tire_slot_anchor: None,
            tire_mount: None,
            tire_main_types: Vec::new(),
            tire_types: vec!["gumi".into()],
            local_rotation_after_mount: [0., 0., 0., 1.],
            fall_off_category: 7,
        };
        let (rim, rim_collider) = part(40, physics, items);
        let (tire, tire_collider) = part(41, physics, items);
        let mut rim_rule = rule(rim, "felni", 0.25);
        rim_rule.tire_mount = Some(crate::reference::Transform {
            translation: [0.; 3],
            rotation: [0., 0., 0., 1.],
            scale: [1.; 3],
        });
        for (rule, rim, mass, collider) in [
            (rim_rule, true, 10., rim_collider),
            (rule(tire, "gumi", 0.35), false, 15., tire_collider),
        ] {
            wheels
                .register_generated(
                    rule,
                    rim,
                    mass,
                    &[(ColliderId(0), collider)],
                    items,
                    physics,
                )
                .unwrap();
        }
        wheels.assemble(rim, tire, items, physics).unwrap();
        rim
    }

    #[test]
    fn world_car_wheel_slot_removes_refits_and_saves_its_rim() {
        use crate::vehicle::WheelAssembly;
        #[derive(serde::Serialize, serde::Deserialize)]
        struct Saved {
            #[serde(with = "crate::save::identity::lossless")]
            changes: ChangeRecords,
        }
        let prototype = crate::vehicle::car_prototype_fixture();
        let car = GeneratedId {
            layer: WorldLayer::Prop,
            generator: SourceId {
                file: "a".into(),
                path_id: 2,
            },
            unit: [0, 0],
            ordinal: 7,
            children: Vec::new(),
        };
        let key = ObjectKey::Generated(car.clone());
        struct Session {
            physics: PhysicsWorld,
            items: ItemStore,
            wheels: WheelAssemblies,
            vehicles: BTreeMap<ObjectKey, VehicleRuntime>,
            rim: ItemId,
            root: ItemId,
        }
        // One session: the prototype rim of wheel 0, then the car. Wheels 1 to 3 have no rim.
        let run = |changes: &mut ChangeRecords| {
            let mut physics = PhysicsWorld::empty(1, 2);
            physics.insert_box([0., -0.5, 0.], [500., 0.5, 500.]);
            let mut items = ItemStore::from_states(Vec::new());
            let mut wheels = WheelAssemblies::default();
            let mut vehicles = BTreeMap::new();
            let rim = wheel(&car, &prototype, &mut physics, &mut items, &mut wheels);
            let slots: Vec<_> = (0..4)
                .map(|i| WorldWheelSlot {
                    rim_main_types: Vec::new(),
                    rim_types: vec!["felni".into()],
                    hub_to_mount: rapier3d::math::Pose::IDENTITY,
                    sensor_radius: 0.3,
                    rim: (i == 0).then(|| items.object_key(rim)),
                })
                .collect();
            let root = spawn_copy(
                &car,
                &prototype,
                &mut physics,
                &mut items,
                &mut vehicles,
                &mut wheels,
                &slots,
                25.,
                changes,
            );
            Session {
                physics,
                items,
                wheels,
                vehicles,
                rim,
                root,
            }
        };
        let tire = |s: &Session| s.physics.wheel_assembly(&key, 0);
        let mass = |s: &Session| s.physics.rapier().bodies[s.physics.chassis_handle(&key)].mass();
        let save = |s: &Session, mut changes: ChangeRecords| {
            for (key, rims) in s.wheels.world_slots() {
                changes.keep_wheel_slots(key.clone(), rims);
            }
            let json = serde_json::to_vec(&Saved { changes }).unwrap();
            serde_json::from_slice::<Saved>(&json).unwrap().changes
        };

        let mut changes = ChangeRecords::default();
        let mut s = run(&mut changes);
        assert_eq!(
            tire(&s),
            WheelAssembly::Tire {
                radius_m: 0.35,
                mass_kg: 25.,
                quality: 1.
            }
        );
        assert_eq!(s.physics.wheel_assembly(&key, 1), WheelAssembly::Hub);
        assert_eq!(
            s.items.get(s.rim).unwrap().body,
            Some(s.physics.chassis_handle(&key))
        );
        // 820 kg dry chassis with its wheel plus 40 L of fuel.
        assert!((mass(&s) - 860.).abs() < 1e-3, "{}", mass(&s));

        // Remove: the car sits on the hub and loses the wheel mass.
        let slot = s.wheels.slot_for_item(s.rim).unwrap();
        assert_eq!(s.wheels.slot_owner(slot), Some(s.root));
        s.wheels.remove(slot, &mut s.items, &mut s.physics).unwrap();
        assert_eq!(tire(&s), WheelAssembly::Hub);
        assert!((mass(&s) - 835.).abs() < 1e-3, "{}", mass(&s));
        assert_ne!(
            s.items.get(s.rim).unwrap().body,
            Some(s.physics.chassis_handle(&key))
        );

        // Refit rim and tire: the car drives on the tire.
        s.wheels
            .install(s.rim, slot, &mut s.items, &mut s.physics)
            .unwrap();
        assert!(matches!(tire(&s), WheelAssembly::Tire { .. }));
        assert!((mass(&s) - 860.).abs() < 1e-3);

        // Save and load: the slot keeps its rim.
        let mut loaded = save(&s, ChangeRecords::default());
        let mut s = run(&mut loaded);
        assert!(matches!(tire(&s), WheelAssembly::Tire { .. }));
        assert!(
            loaded.take_wheel_slots(&key).is_none(),
            "the car took its record"
        );

        // Remove, save and load: the slot stays empty, the prototype rim stays loose.
        let slot = s.wheels.slot_for_item(s.rim).unwrap();
        s.wheels.remove(slot, &mut s.items, &mut s.physics).unwrap();
        let mut loaded = save(&s, ChangeRecords::default());
        let s = run(&mut loaded);
        assert_eq!(tire(&s), WheelAssembly::Hub);
        assert_eq!(
            s.items.get(s.rim).unwrap().location,
            crate::interaction::items::ItemLocation::World
        );
        assert!((mass(&s) - 835.).abs() < 1e-3);

        // Retire: the change records keep the slot rims for the respawn.
        let mut s = s;
        let mut changes = ChangeRecords::default();
        retire_vehicle(
            &key,
            &mut s.physics,
            &mut s.vehicles,
            &mut s.wheels,
            &mut changes,
        );
        assert_eq!(changes.take_wheel_slots(&key), Some(vec![None; 4]));
    }
}
