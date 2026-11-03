//! Car prototypes and engine parts of spawned world contents.
//! A car copy is a vehicle body, a car runtime, its wheel slots and the engine and radiator
//! parts on it.
use crate::car::VehicleRuntime;
use crate::crash::Blood;
use crate::electronics::{Electronics, NodeElectronics};
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
use crate::vehicle::PhysicsWorld;
use crate::world::changes::ChangeRecords;
use crate::world::contents::ContentInstance;
use crate::world::spawn_rules::{SourceRules, optional, texts};
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
        // One state per electronicsscript node. Controllers that share a node share its jam.
        let mut electronics: Vec<NodeElectronics> = Vec::new();
        let mut electronics_of = |node: Option<u32>| -> Result<Option<usize>, ReferenceError> {
            let Some(node) = node else {
                return Ok(None);
            };
            if let Some(i) = electronics.iter().position(|e| e.node == node) {
                return Ok(Some(i));
            }
            let Some(e) = node_electronics(world, rules, prototype, node)? else {
                return Ok(None);
            };
            electronics.push(e);
            Ok(Some(electronics.len() - 1))
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
                    switch: light.cabin.as_ref().and_then(|c| owner_item(c.switch_node)),
                    doors: light
                        .cabin
                        .iter()
                        .flat_map(|c| &c.door_slots)
                        .map(|&node| owner_item(node))
                        .collect(),
                })
            })
            .collect::<Result<Vec<_>, ReferenceError>>()?;
        let car_electronics = electronics_of(record.electronics)?;
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
                car_electronics,
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

/// Source `electronicsscript` of prototype node `node`. None: the node has no electronics script.
pub fn node_electronics(
    world: &WorldReference,
    rules: &SourceRules,
    prototype: &WorldPrototypeReference,
    node: u32,
) -> Result<Option<NodeElectronics>, ReferenceError> {
    let n = prototype
        .nodes
        .get(node as usize)
        .ok_or_else(|| ReferenceError("Electronics node missing".into()))?;
    let Some(record) = rules.component(world, &n.source, "electronicsscript")? else {
        return Ok(None);
    };
    Ok(Some(NodeElectronics {
        node,
        layer: u32::from(n.layer),
        state: Electronics::from_fields(record.fields)?,
    }))
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
    /// Electronics nodes of the car.
    pub electronics: Vec<NodeElectronics>,
    /// `carscript.electronics` index in `electronics`. None: the car has no electronics.
    pub car_electronics: Option<usize>,
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
        crate::car::WorldCarParts {
            seed: key.seed(),
            fuel_tank: copy.fuel_tank,
            recovery_bounds: chassis_bounds(physics, copy.colliders),
            bodies: copy.bodies,
            electronics: copy.electronics,
            car_electronics: copy.car_electronics,
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
