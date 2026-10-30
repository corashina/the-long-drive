//! Test sandbox. The `?sandbox` page sends these commands; Claude runs the ROADMAP test pass with them.
//! A bad command changes nothing and answers `ok: false`. It never stops the game.
use crate::interaction::items::ItemLocation;
use crate::player::SeatOwner;
use crate::reference::{ItemId, ReferenceManifest, SourceId, WorldPrototypeReference};
use crate::starting_area::GarageRuntime;
use crate::vehicle::{VehiclePose, to_rapier};
use crate::world::stream::content_seats::ItemSeat;
use crate::world::{AbsolutePose, ObjectKey, WorldLayer, WorldStream, stream::SupportGap};
use rapier3d::math::{Rotation, Vector};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::BTreeMap;

/// Buildings that `building` checks from the start of the road.
const BUILDING_SEARCH: usize = 4000;
/// Metres between the cars that come along on a move.
const CAR_SPACING: f32 = 8.;
/// `spawn` names of a normal and an evil rabbit.
const RABBITS: [&str; 2] = ["rabbit", "evil rabbit"];
/// World prototype names that a failed `spawn` lists.
const NAME_LIST: usize = 30;

#[derive(Deserialize)]
#[serde(tag = "action", rename_all = "lowercase", deny_unknown_fields)]
enum Action {
    /// Moves the player on foot to the ground at absolute `x`, `z`.
    Teleport { x: f64, z: f64 },
    /// Moves the player to the start spawn. Held cars go back to their pose before the first move.
    Garage,
    /// Moves the player to the first building on the road whose name contains `name`.
    Building { name: String },
    /// Spawns an item prototype (catalog name) or a world prototype (root name, such as a car or a gun)
    /// `forward` metres in front of the player. A world prototype loads with the stream.
    Spawn { name: String, forward: f32 },
    /// Spawns an item prototype and puts it in the hand.
    Hand { name: String },
    /// Replaces the fuel of the seated car, or the nearest car within 10 m, with `liters` of `fluid` (0 to 5).
    Fill { fluid: u8, liters: f32 },
    /// Sits on the nearest loaded seat. `car` limits it to cars whose name contains `car`.
    /// `seat` limits it to the driver or a passenger seat.
    Seat {
        car: Option<String>,
        seat: Option<SeatKind>,
    },
    /// Gets out of the seat to the first clear exit.
    Stand,
    /// Spawns a wearable item prototype (catalog or root name) and puts it on.
    Wear { name: String },
    /// Places a gun (world prototype root name) and puts it in the hand with full ammo.
    Equip { name: String },
    /// Turns the head toward absolute `x`, `y`, `z`, within the look limits.
    Aim { x: f64, y: f64, z: f64 },
    /// Spawns a sandstorm `distance` metres in front of the player.
    Storm { distance: f32 },
    /// Sets the source day cycle seconds.
    Cycle { seconds: f64 },
    /// Answers the player, the cycle, the cars and the items within `radius` metres.
    State { radius: f32 },
}

#[derive(Clone, Copy, Deserialize)]
#[serde(rename_all = "lowercase")]
enum SeatKind {
    Driver,
    Passenger,
}

/// Sandbox state that lives across commands.
#[derive(Default)]
pub struct Sandbox {
    /// JSON arrays of commands that wait for a command without hold.
    pub queued: Vec<String>,
    /// Replies that the next frame sends.
    pub replies: Vec<String>,
    /// Absolute pose of each held car before its first move. `garage` puts the car back there.
    homes: BTreeMap<ObjectKey, AbsolutePose>,
}

#[derive(Serialize)]
struct Reply {
    id: u32,
    ok: bool,
    message: String,
    data: Value,
}

/// Runs the queued JSON arrays. Each command adds one JSON reply.
pub fn run(runtime: &mut GarageRuntime, reference: &ReferenceManifest, sandbox: &mut Sandbox) {
    for json in std::mem::take(&mut sandbox.queued) {
        run_batch(runtime, reference, &json, sandbox);
    }
}

fn run_batch(
    runtime: &mut GarageRuntime,
    reference: &ReferenceManifest,
    json: &str,
    sandbox: &mut Sandbox,
) {
    let replies = &mut sandbox.replies;
    let entries = match serde_json::from_str::<Vec<Value>>(json) {
        Ok(entries) => entries,
        Err(error) => {
            replies.push(reply(0, Err(format!("Bad sandbox JSON: {error}"))));
            return;
        }
    };
    for mut entry in entries {
        let id = entry
            .as_object_mut()
            .and_then(|e| e.remove("id"))
            .and_then(|id| id.as_u64())
            .unwrap_or(0) as u32;
        let result = serde_json::from_value::<Action>(entry)
            .map_err(|e| format!("Bad sandbox command: {e}"))
            .and_then(|action| apply(runtime, reference, &mut sandbox.homes, action));
        replies.push(reply(id, result));
    }
}

fn reply(id: u32, result: Result<(String, Value), String>) -> String {
    let reply = match result {
        Ok((message, data)) => Reply {
            id,
            ok: true,
            message,
            data,
        },
        Err(message) => Reply {
            id,
            ok: false,
            message,
            data: Value::Null,
        },
    };
    serde_json::to_string(&reply).expect("sandbox reply")
}

fn apply(
    runtime: &mut GarageRuntime,
    reference: &ReferenceManifest,
    homes: &mut BTreeMap<ObjectKey, AbsolutePose>,
    action: Action,
) -> Result<(String, Value), String> {
    match action {
        Action::Teleport { x, z } => {
            let (height, _) = runtime.stream.terrain_sample([x, 0., z]);
            place_player(runtime, homes, [x, f64::from(height) + 2., z], true)?;
            Ok((
                "Moved. The world loads around the player.".into(),
                json!([x, z]),
            ))
        }
        Action::Garage => {
            let spawn = reference.area.player_spawn.translation.map(f64::from);
            place_player(runtime, homes, spawn, false)?;
            Ok(("Moved to the start spawn.".into(), Value::Null))
        }
        Action::Building { name } => {
            let (found, position) = building(reference, runtime.stream.seed(), &name)?;
            place_player(
                runtime,
                homes,
                [position[0], position[1] + 2., position[2]],
                true,
            )?;
            Ok((format!("Moved to {found}."), json!(position)))
        }
        Action::Spawn { name, forward } if RABBITS.contains(&name.to_lowercase().as_str()) => {
            // Source `nyulscript`: a rabbit above the evil size gets evil unless peaceful.
            let size = if name.eq_ignore_ascii_case("rabbit") {
                0.5
            } else {
                1.
            };
            let pose = to_rapier(ahead(runtime, forward));
            let id = runtime
                .creatures
                .spawn_rabbit(
                    &reference.creatures.rabbit,
                    &mut runtime.physics,
                    pose,
                    size,
                    runtime.simulation_seconds,
                )
                .map_err(|e| e.to_string())?;
            Ok((format!("Spawned {name}."), json!(id)))
        }
        Action::Spawn { name, forward } => match item_prototype(reference, &name) {
            Ok(prototype) => {
                let items = spawn_item(runtime, reference, prototype, forward)?;
                Ok((format!("Spawned {name}."), json!(items)))
            }
            Err(_) => {
                let position = spawn_world(runtime, reference, &name, forward)?;
                Ok((
                    format!("Placed {name}. It spawns when the stream loads it."),
                    json!(position),
                ))
            }
        },
        Action::Hand { name } => hand(runtime, reference, &name),
        Action::Seat { car, seat } => {
            let name = sit(runtime, reference, car.as_deref(), seat)?;
            Ok((format!("Seated in {name}."), Value::Null))
        }
        Action::Stand => {
            if !runtime.area.player.seated() {
                return Err("Not seated.".into());
            }
            if !runtime.area.player.try_exit(&mut runtime.physics) {
                return Err("No clear exit.".into());
            }
            Ok(("Standing.".into(), Value::Null))
        }
        Action::Wear { name } => {
            let item = wear(runtime, reference, &name)?;
            Ok((format!("Wearing {name}."), json!(item)))
        }
        Action::Equip { name } => equip(runtime, reference, &name),
        Action::Aim { x, y, z } => {
            let target = runtime.stream.origin.local([x, y, z]);
            runtime.area.player.look_at(target, &runtime.physics);
            Ok(("Aimed.".into(), Value::Null))
        }
        Action::Storm { distance } => {
            let position = runtime
                .stream
                .origin
                .absolute(ahead(runtime, distance).position);
            let id = runtime.weather.spawn_at(&reference.weather, position);
            Ok((
                "Storm spawned.".into(),
                json!({"id": id.0, "position": position}),
            ))
        }
        Action::Fill { fluid, liters } => fill(runtime, fluid, liters),
        Action::Cycle { seconds } => {
            let cycle = &reference.lighting.cycle;
            let length = f64::from(cycle.day_seconds + cycle.night_seconds);
            if !(0. ..=length).contains(&seconds) {
                return Err(format!("Cycle seconds must be 0 to {length}."));
            }
            runtime.lighting.cycle.set_cycle_seconds(seconds);
            Ok(("Cycle set.".into(), Value::Null))
        }
        Action::State { radius } => Ok(("State.".into(), state(runtime, reference, radius))),
    }
}

/// Moves the player on foot, with the cars that hold an area, and asks the stream for the new area.
/// `row` puts the cars in a row next to the target. Else each car with a home goes back there,
/// because a row next to the start spawn is inside the garage walls.
fn place_player(
    runtime: &mut GarageRuntime,
    homes: &mut BTreeMap<ObjectKey, AbsolutePose>,
    absolute: [f64; 3],
    row: bool,
) -> Result<(), String> {
    if runtime.area.player.seated() {
        return Err("Get out of the seat first.".into());
    }
    let target = runtime.stream.origin.local(absolute);
    let old = runtime.area.player.body_pose(&runtime.physics).position;
    runtime
        .area
        .player
        .rebase(std::array::from_fn(|i| old[i] - target[i]));
    let pose = runtime.area.player.body_pose(&runtime.physics);
    let body = runtime.area.player.body_handle();
    runtime.physics.edit_body(body, |body| {
        body.set_position(to_rapier(pose), true);
        body.set_linvel(Vector::ZERO, true);
    });
    // Cars that hold an area come along: the step waits until each active car's area is loaded.
    let held: Vec<_> = runtime
        .vehicles
        .keys()
        .filter(|key| {
            Some(*key) == runtime.vehicle.as_ref()
                || matches!(key, ObjectKey::Generated(id) if id.layer == WorldLayer::Startup)
        })
        .cloned()
        .collect();
    for (index, key) in held.iter().enumerate() {
        let pose = runtime.physics.snapshot(key).pose;
        let target = if row {
            let origin = &runtime.stream.origin;
            homes.entry(key.clone()).or_insert(AbsolutePose {
                position: origin.absolute(pose.position),
                rotation: pose.rotation,
            });
            let x = absolute[0] + CAR_SPACING as f64 * (index + 1) as f64;
            let (height, _) = runtime.stream.terrain_sample([x, 0., absolute[2]]);
            AbsolutePose {
                position: [x, f64::from(height) + 1.5, absolute[2]],
                rotation: pose.rotation,
            }
        } else if let Some(home) = homes.remove(key) {
            home
        } else {
            // The car did not move. It is still at its home.
            continue;
        };
        let position = runtime.stream.origin.local(target.position);
        runtime.physics.reset_preserving_wheels(
            key,
            VehiclePose {
                position,
                rotation: target.rotation,
            },
        );
    }
    runtime.physics.refresh_queries();
    // The step waits for a supported world, so the stream must ask for the new area here.
    runtime.request_world(absolute);
    let focus = runtime.area.player.focus_position(&runtime.physics);
    runtime.rebase_to(focus);
    Ok(())
}

fn fill(runtime: &mut GarageRuntime, fluid: u8, liters: f32) -> Result<(String, Value), String> {
    if fluid >= 6 || !liters.is_finite() || liters < 0. {
        return Err("Fluid must be 0 to 5 and liters at least 0.".into());
    }
    let player = runtime.area.player.body_pose(&runtime.physics).position;
    let distance = |key| {
        let p = runtime.physics.snapshot(key).pose.position;
        (0..3).map(|i| (p[i] - player[i]).powi(2)).sum::<f32>()
    };
    let key = match &runtime.vehicle {
        Some(key) if runtime.area.player.seated() => key.clone(),
        _ => runtime
            .vehicles
            .keys()
            .filter(|key| distance(key) <= 100.)
            .min_by(|a, b| distance(a).total_cmp(&distance(b)))
            .ok_or("No car within 10 m.")?
            .clone(),
    };
    let tank = runtime.vehicles[&key]
        .fuel_tank
        .ok_or("The car has no fuel tank.")?;
    let container = runtime
        .area
        .items
        .get_mut(tank)
        .and_then(|item| item.fluid.as_mut())
        .ok_or("The fuel tank has no fluid.")?;
    let amount = liters.min(container.capacity_liters);
    *container = crate::fluids::FluidContainer::new(
        container.capacity_liters,
        &[crate::fluids::FluidEntry {
            fluid_type: fluid,
            amount,
        }],
    );
    Ok((format!("Filled {amount} L."), json!(format!("{key:?}"))))
}

/// Sits on the nearest seat that `car` and `kind` allow, as release E on the seat does.
/// Returns the owner name.
fn sit(
    runtime: &mut GarageRuntime,
    reference: &ReferenceManifest,
    car: Option<&str>,
    kind: Option<SeatKind>,
) -> Result<String, String> {
    if runtime.area.player.seated() {
        return Err("Get out of the seat first.".into());
    }
    let player = Vector::from_array(runtime.area.player.body_pose(&runtime.physics).position);
    let wanted = car.map(str::to_lowercase);
    let bodies = &runtime.physics.rapier().bodies;
    let name = |seat: &ItemSeat| match &seat.owner {
        SeatOwner::Vehicle(key) => runtime.vehicles.get(key).map_or("", |v| {
            prototype_name(&reference.world.prototypes[v.prototype])
        }),
        SeatOwner::Item(_) => "",
    };
    let seat = runtime
        .area
        .interaction
        .item_seats()
        .filter(|seat| {
            wanted.as_ref().is_none_or(|w| car_matches(name(seat), w))
                && kind.is_none_or(|k| seat.seat.driver == matches!(k, SeatKind::Driver))
                && seat.can_sit(&runtime.physics)
        })
        .map(|seat| {
            let at = *bodies[seat.body].position() * to_rapier(seat.pose);
            (seat, at.translation.distance_squared(player))
        })
        .min_by(|a, b| a.1.total_cmp(&b.1))
        .map(|(seat, _)| seat.clone())
        .ok_or("No loaded seat matches. Spawn or teleport to the car first.")?;
    let found = name(&seat).to_string();
    if !runtime
        .area
        .player
        .enter_item_seat(&seat, &mut runtime.physics)
    {
        return Err("Cannot sit there.".into());
    }
    Ok(found)
}

/// The root name (Car02Full), the game name (Volna) or, for the Volna, "Lada" contains `wanted`.
fn car_matches(root: &str, wanted: &str) -> bool {
    let game = crate::interaction::labels::vehicle_name(root).unwrap_or("");
    let lada = if game == "Volna" { "lada" } else { "" };
    !root.is_empty()
        && [root, game, lada]
            .iter()
            .any(|n| !n.is_empty() && n.to_lowercase().contains(wanted))
}

/// Spawns a wearable in front of the player and puts it on, as the short Tab release does.
fn wear(
    runtime: &mut GarageRuntime,
    reference: &ReferenceManifest,
    name: &str,
) -> Result<u32, String> {
    let cigarette = runtime.area.cigarettes.cigarette_prototype;
    let wearables: Vec<usize> = reference
        .clothing
        .wearables
        .iter()
        .filter_map(|w| {
            reference
                .item_prototypes
                .iter()
                .position(|p| p.id == w.prototype)
        })
        .chain([cigarette])
        .collect();
    // A root name can repeat across prototypes, so the match stays inside the wearables.
    let catalog = item_prototype(reference, name).ok();
    let prototype = wearables
        .iter()
        .copied()
        .find(|&p| {
            catalog == Some(p)
                || root_name(reference, reference.item_prototypes[p].root)
                    .eq_ignore_ascii_case(name)
        })
        .ok_or_else(|| {
            format!(
                "No wearable {name}. Names: {}",
                names(reference, wearables.iter().copied())
            )
        })?;
    let root = ahead(runtime, 1.);
    let item = if prototype == cigarette {
        runtime.spawn_cigarette(reference, root)
    } else {
        crate::starting_area::spawned::spawn_prototype(
            reference,
            &mut runtime.area,
            &mut runtime.physics,
            prototype,
            root,
        )
        .map(|items| items[0])
    }
    .map_err(|e| e.to_string())?;
    if !runtime.area.put_on(&mut runtime.physics, item) {
        return Err("Its slot is full. It lies in front of the player.".into());
    }
    runtime.area.update_glasses(&runtime.stream);
    Ok(item.0)
}

/// Places gun `name` (a world prototype root name), then puts the loaded gun in the hand with
/// full ammo. The reply has `pending` until the stream loads it.
fn equip(
    runtime: &mut GarageRuntime,
    reference: &ReferenceManifest,
    name: &str,
) -> Result<(String, Value), String> {
    let guns = reference.guns.weapons.iter().filter_map(|g| {
        reference
            .world
            .prototypes
            .iter()
            .find(|w| w.source == g.prototype.0)
    });
    let Some(prototype) = guns
        .clone()
        .find(|w| prototype_name(w).eq_ignore_ascii_case(name))
    else {
        let mut names: Vec<_> = guns.map(prototype_name).collect();
        names.sort_unstable();
        names.dedup();
        return Err(format!("No gun {name}. Names: {}", names.join(", ")));
    };
    let bound = |item| runtime.area.interaction.guns.runtime(item).is_some();
    let Some(item) = loaded_world_item(runtime, &prototype.source, bound) else {
        return place_pending(runtime, prototype, name);
    };
    if !take_in_hand(runtime, item)? {
        return Err("Carried, but the hand is not free.".into());
    }
    runtime.area.interaction.guns.fill(item);
    Ok((format!("{name} in the hand."), json!([item.0])))
}

/// Places world prototype `prototype` once. The item command answers `pending` until it loads.
fn place_pending(
    runtime: &mut GarageRuntime,
    prototype: &WorldPrototypeReference,
    name: &str,
) -> Result<(String, Value), String> {
    let placed = runtime
        .stream
        .added_placement_audit()
        .iter()
        .any(|(p, _)| p.prototype == prototype.source);
    if !placed {
        place_world(runtime, prototype, 1.)?;
    }
    Ok((
        format!("Placed {name}. Call again when it loads."),
        json!({"pending": true}),
    ))
}

/// Picks up `item` and puts it in the hand. False: it stays carried.
fn take_in_hand(runtime: &mut GarageRuntime, item: ItemId) -> Result<bool, String> {
    let area = &mut runtime.area;
    let point = area
        .items
        .get(item)
        .ok_or("The item is missing")?
        .pose
        .position;
    area.interaction
        .pickup_item(
            item,
            point,
            &area.player,
            &mut area.items,
            &mut runtime.physics,
        )
        .map_err(|e| e.to_string())?;
    area.sync_inventory();
    area.equip_carried(&mut runtime.physics)
        .map_err(|e| e.to_string())
}

/// Spawns item `name` and puts it in the hand. A catalog item of a world prototype, such as the
/// binoculars, is placed first and taken once the stream loads it: the reply then has `pending`.
fn hand(
    runtime: &mut GarageRuntime,
    reference: &ReferenceManifest,
    name: &str,
) -> Result<(String, Value), String> {
    let item = match item_prototype(reference, name) {
        Ok(prototype) => {
            let items = spawn_item(runtime, reference, prototype, 1.)?;
            ItemId(*items.first().ok_or("The prototype has no pickup.")?)
        }
        Err(error) => {
            let prototype = reference
                .world
                .catalog
                .items
                .iter()
                .find(|i| i.name.eq_ignore_ascii_case(name))
                .and_then(|i| i.prototype.as_ref())
                .and_then(|p| reference.world.prototypes.iter().find(|w| &w.source == p))
                .ok_or(error)?;
            match loaded_world_item(runtime, &prototype.source, |_| true) {
                Some(item) => item,
                None => return place_pending(runtime, prototype, name),
            }
        }
    };
    let message = if take_in_hand(runtime, item)? {
        "In the hand."
    } else {
        "Carried."
    };
    Ok((message.into(), json!([item.0])))
}

/// The nearest loose item of a sandbox or startup placement of world prototype `source`.
fn loaded_world_item(
    runtime: &GarageRuntime,
    source: &SourceId,
    keep: impl Fn(ItemId) -> bool,
) -> Option<ItemId> {
    let player = Vector::from_array(runtime.area.player.body_pose(&runtime.physics).position);
    // The prototype root comes first, then the nearest copy.
    runtime
        .area
        .items
        .generated_bindings()
        .filter(|(id, item)| {
            id.layer == WorldLayer::Startup
                && &id.generator == source
                && !item.removed
                && item.location == ItemLocation::World
                && keep(item.id)
        })
        .map(|(id, item)| {
            let at = Vector::from_array(item.pose.position);
            (item.id, id.children.len(), at.distance_squared(player))
        })
        .min_by(|a, b| a.1.cmp(&b.1).then(a.2.total_cmp(&b.2)))
        .map(|(id, ..)| id)
}

fn root_name(reference: &ReferenceManifest, root: crate::reference::NodeId) -> &str {
    reference
        .nodes
        .iter()
        .find(|n| n.id == root)
        .map_or("", |n| n.name.as_str())
}

/// The sorted root names of item prototypes.
fn names(reference: &ReferenceManifest, prototypes: impl Iterator<Item = usize>) -> String {
    let mut names: Vec<_> = prototypes
        .map(|p| root_name(reference, reference.item_prototypes[p].root))
        .collect();
    names.sort_unstable();
    names.dedup();
    names.join(", ")
}

/// The first building of the road sequence whose name contains `name`. Unknown: lists the names.
fn building(
    reference: &ReferenceManifest,
    seed: i32,
    name: &str,
) -> Result<(String, [f64; 3]), String> {
    let wanted = name.to_lowercase();
    let mut sequence = crate::world::BuildingSequence::new(&reference.world, seed);
    let mut names = Vec::new();
    for placement in std::iter::from_fn(|| sequence.next()).take(BUILDING_SEARCH) {
        let Some(prototype) = reference
            .world
            .prototypes
            .iter()
            .find(|p| p.source == placement.prototype)
        else {
            continue;
        };
        let found = prototype_name(prototype);
        if found.to_lowercase().contains(&wanted) {
            return Ok((found.to_string(), placement.pose.position));
        }
        if !names.contains(&found) {
            names.push(found);
        }
    }
    names.sort_unstable();
    Err(format!("No building {name}. Names: {}", names.join(", ")))
}

/// The physics pose `forward` metres in front of the player and 0.5 m up.
fn ahead(runtime: &GarageRuntime, forward: f32) -> VehiclePose {
    let body = runtime.area.player.body_pose(&runtime.physics);
    let ahead = Rotation::from_array(body.rotation) * -Vector::Z * forward;
    VehiclePose {
        position: std::array::from_fn(|i| body.position[i] + ahead[i] + [0., 0.5, 0.][i]),
        rotation: body.rotation,
    }
}

/// Spawns item prototype `prototype` in front of the player. Returns the item ids.
fn spawn_item(
    runtime: &mut GarageRuntime,
    reference: &ReferenceManifest,
    prototype: usize,
    forward: f32,
) -> Result<Vec<u32>, String> {
    let root = ahead(runtime, forward);
    let items = crate::starting_area::spawned::spawn_prototype(
        reference,
        &mut runtime.area,
        &mut runtime.physics,
        prototype,
        root,
    )
    .map_err(|e| e.to_string())?;
    Ok(items.iter().map(|i| i.0).collect())
}

/// Places world prototype `name` (a root name) in front of the player. Returns the absolute position.
fn spawn_world(
    runtime: &mut GarageRuntime,
    reference: &ReferenceManifest,
    name: &str,
    forward: f32,
) -> Result<[f64; 3], String> {
    let prototypes = &reference.world.prototypes;
    let Some(prototype) = prototypes
        .iter()
        .find(|p| prototype_name(p).eq_ignore_ascii_case(name))
    else {
        let wanted = name.to_lowercase();
        let mut names: Vec<_> = prototypes
            .iter()
            .map(prototype_name)
            .filter(|n| n.to_lowercase().contains(&wanted))
            .collect();
        names.sort_unstable();
        names.dedup();
        names.truncate(NAME_LIST);
        return Err(format!(
            "No item or world prototype {name}. Similar: {}",
            names.join(", ")
        ));
    };
    place_world(runtime, prototype, forward)
}

/// Places world prototype `prototype` in front of the player. Returns the absolute position.
fn place_world(
    runtime: &mut GarageRuntime,
    prototype: &WorldPrototypeReference,
    forward: f32,
) -> Result<[f64; 3], String> {
    let root = ahead(runtime, forward);
    let position = runtime.stream.origin.absolute(root.position);
    runtime
        .stream
        .add_startup_placement(
            &prototype.source,
            crate::world::AbsolutePose {
                position,
                rotation: root.rotation,
            },
        )
        .map_err(|e| e.to_string())?;
    Ok(position)
}

fn item_prototype(reference: &ReferenceManifest, name: &str) -> Result<usize, String> {
    let catalog = &reference.world.catalog.items;
    let item = catalog
        .iter()
        .find(|i| i.name.eq_ignore_ascii_case(name))
        .ok_or_else(|| format!("No catalog item {name}."))?;
    reference
        .item_prototypes
        .iter()
        .position(|p| p.item_id == Some(item.index) || item.prototype.as_ref() == Some(&p.source))
        .ok_or_else(|| format!("{name} has no item prototype."))
}

fn state(runtime: &GarageRuntime, reference: &ReferenceManifest, radius: f32) -> Value {
    let origin = runtime.stream.origin;
    let player = runtime.area.player.body_pose(&runtime.physics).position;
    let near =
        |p: [f32; 3]| (0..3).map(|i| (p[i] - player[i]).powi(2)).sum::<f32>() <= radius * radius;
    // The step waits while the player or an active car has a gap: the same 8 m test as the step.
    let gap = |p: [f32; 3]| {
        let center = origin.absolute(p);
        let gap = runtime.stream.support_gap(crate::world::WorldBounds {
            min: center.map(|v| v - 8.),
            max: center.map(|v| v + 8.),
        });
        gap.map_or(Value::Null, |gap| gap_json(reference, &runtime.stream, gap))
    };
    let cars: Vec<_> = runtime
        .vehicles
        .iter()
        .map(|(key, car)| {
            let position = runtime.physics.snapshot(key).pose.position;
            let active = runtime
                .physics
                .body_active(runtime.physics.chassis_handle(key));
            json!({
                "key": format!("{key:?}"),
                "name": prototype_name(&reference.world.prototypes[car.prototype]),
                "position": origin.absolute(position),
                "near": near(position),
                "gap": if active { gap(position) } else { Value::Null },
            })
        })
        .collect();
    let items: Vec<_> = runtime
        .area
        .items
        .iter()
        .filter(|i| !i.removed && near(i.pose.position))
        .map(|i| {
            // Only sandbox and other spawned items know their prototype.
            let name = runtime.area.spawned.get(i.id).and_then(|s| {
                let id = reference.item_prototypes[s.prototype].item_id?;
                let catalog = &reference.world.catalog.items;
                catalog
                    .iter()
                    .find(|c| c.index == id)
                    .map(|c| c.name.as_str())
            });
            json!({"id": i.id.0, "name": name, "position": origin.absolute(i.pose.position)})
        })
        .collect();
    let cycle = &reference.lighting.cycle;
    json!({
        "player": origin.absolute(player),
        "supported": runtime.world_supported(),
        "pending": runtime.stream.pending_parts(),
        "required_ready": runtime.stream.required_ready(),
        // Shared budgets with the renderer credit: [used, limit].
        "budgets": runtime
            .stream
            .presentation_budgets(runtime.stream.presentation_credit())
            .iter()
            .map(|b| (b.name.to_owned(), json!([b.used, b.limit])))
            .collect::<serde_json::Map<_, _>>(),
        // Renderer asset CPU: [used, credit]. Credit above use goes back on a reclaim.
        "renderer_asset_cpu": [
            runtime.stream.presentation_used().prototype_cpu,
            runtime.stream.presentation_credit().prototype_cpu,
        ],
        // Sandbox world spawns and where each one waits.
        "world_spawns": runtime
            .stream
            .added_placement_audit()
            .into_iter()
            .map(|(p, progress)| {
                json!({
                    "name": reference
                        .world
                        .prototypes
                        .iter()
                        .find(|w| w.source == p.prototype)
                        .map_or("", prototype_name),
                    "position": p.pose.position,
                    "radius": p.bounds.max[0] - p.pose.position[0],
                    "progress": progress,
                })
            })
            .collect::<Vec<_>>(),
        "player_gap": gap(runtime.area.player.focus_position(&runtime.physics)),
        "seated": runtime.area.player.seated(),
        "cycle": runtime.lighting.cycle.capture_save().cycle_seconds,
        "day_seconds": cycle.day_seconds,
        "night_seconds": cycle.night_seconds,
        "cars": cars,
        "items": items,
        "creatures": creatures(runtime, reference),
        "storms": runtime
            .weather
            .storms()
            .iter()
            .map(|s| json!({
                "id": s.id.0,
                "position": s.absolute_position,
                "size": s.size,
                "life": s.life,
                "strength": s.strength,
            }))
            .collect::<Vec<_>>(),
        "storm_influence": runtime.weather.influence().strength,
    })
}

/// Rabbits, bats and planes. Positions are absolute.
fn creatures(runtime: &GarageRuntime, reference: &ReferenceManifest) -> Vec<Value> {
    let origin = runtime.stream.origin;
    let bodies = &runtime.physics.rapier().bodies;
    let rabbits = runtime
        .creatures
        .rabbits()
        .zip(runtime.creatures.views(&runtime.physics))
        .map(|(r, view)| {
            let state = if !r.alive {
                "dead"
            } else if r.evil.attacking {
                "attack"
            } else if r.evil.roaring {
                "roar"
            } else if r.brain.fleeing {
                "flee"
            } else {
                "roam"
            };
            json!({
                "kind": "rabbit",
                "id": r.id,
                "position": origin.absolute(view.pose.translation.to_array()),
                "size": r.size.size,
                "big": crate::creatures::evil_size(r.size.size),
                "evil": r.evil.kind.map(|k| format!("{k:?}").to_lowercase()),
                "alive": r.alive,
                "active": r.active,
                "eyes": r.senses.has_eyes,
                "ears": r.senses.has_ears,
                "state": state,
            })
        });
    let bats = runtime.flyers.flyers().iter().map(|f| {
        json!({
            "kind": "bat",
            "id": f.item.0,
            "position": origin.absolute(bodies[f.body].translation().to_array()),
        })
    });
    let anchor = runtime.sky_anchor(reference);
    let planes = runtime.sky().planes().iter().map(|p| {
        json!({
            "kind": "plane",
            "id": p.id,
            "position": std::array::from_fn::<f64, 3, _>(|i| anchor[i] + f64::from(p.offset.to_array()[i])),
        })
    });
    rabbits.chain(bats).chain(planes).collect()
}

fn gap_json(reference: &ReferenceManifest, stream: &WorldStream, gap: SupportGap) -> Value {
    match gap {
        SupportGap::Plan => json!("plan"),
        SupportGap::Road { index, surface } => json!({"road": index, "surface": surface}),
        SupportGap::Structure(p) => json!({
            "structure": reference
                .world
                .prototypes
                .iter()
                .find(|w| w.source == p.prototype)
                .map_or("", prototype_name),
            "position": p.pose.position,
            "scale": p.scale,
            "progress": stream.structure_audit(p),
        }),
        SupportGap::Light => json!("light"),
        SupportGap::TileCount => json!("tile count"),
        SupportGap::Terrain(unit) => json!({"terrain": unit}),
    }
}

/// The name of the prototype root node.
fn prototype_name(prototype: &WorldPrototypeReference) -> &str {
    prototype
        .nodes
        .iter()
        .find(|n| n.parent.is_none())
        .map_or("", |n| n.name.as_str())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bad_command_answers_not_ok() {
        let value: Value =
            serde_json::from_str(&reply(3, Err("No catalog item x.".into()))).unwrap();
        assert_eq!(value["ok"], false);
        assert_eq!(value["id"], 3);
        let error = serde_json::from_value::<Action>(json!({"action": "fly"}));
        assert!(error.is_err());
        let action = serde_json::from_value::<Action>(
            json!({"action": "spawn", "name": "beer", "forward": 3.0}),
        );
        assert!(matches!(action, Ok(Action::Spawn { forward, .. }) if forward == 3.));
        let action = serde_json::from_value::<Action>(
            json!({"action": "seat", "car": null, "seat": "passenger"}),
        );
        assert!(car_matches("Car02Full", "volna") && car_matches("Car02Full", "lada"));
        assert!(car_matches("Car02Full", "car02") && !car_matches("Car07Full", "lada"));
        assert!(matches!(
            action,
            Ok(Action::Seat {
                car: None,
                seat: Some(SeatKind::Passenger)
            })
        ));
    }
}
