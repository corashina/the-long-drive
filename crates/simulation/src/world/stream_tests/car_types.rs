//! V02 start cars: each one starts, drives, saves and loads. The Lada pick keeps the world draws.
use super::drive_out::{enter, frame, frame_with, ground, settle};
use super::*;
use crate::fluids::FluidEntry;
use crate::reference::{DrivingAssists, ItemId};
use crate::simulation::{FrameInput, keys, player_keys};
use crate::world::{GameMode, ObjectKey, StartCar, StartCarChoice};

/// The start cars of V02 PR 1 and PR 2, and the Lada.
const START_CARS: [&str; 15] = [
    "StartCar01",
    "StartCar02",
    "StartCar02k",
    "StartCar03",
    "StartCar03k",
    "StartCar04",
    "StartCar05",
    "StartCar06",
    "StartCar08",
    "StartCar08c",
    "StartCar09",
    "StartCar09S",
    "StartCar09P",
    "StartCar10",
    "StartCar10k",
];
/// Height of the test deck above the start pose. No start area object reaches it.
const DECK_HEIGHT: f32 = 30.;
/// Half size of the test deck.
const DECK: [f32; 3] = [40., 0.5, 40.];
/// Source water. The coolant rule reads water or blood.
const WATER: usize = 3;

fn start_world(reference: &ReferenceManifest, field: &str) -> GarageRuntime {
    let mode = GameMode {
        start_car: StartCarChoice::Field(StartCar::from_field(field).unwrap()),
        ..GameMode::default()
    };
    let mut run =
        GarageRuntime::new(reference, mode, reference.world.draw_distance.default).unwrap();
    prepare_ready(&mut run, reference);
    run
}

/// The start car copy. Only the picked start car spawns on the `Startup` layer.
fn start_key(run: &GarageRuntime) -> Option<ObjectKey> {
    run.vehicles
        .keys()
        .find(|key| matches!(key, ObjectKey::Generated(id) if id.layer == WorldLayer::Startup))
        .cloned()
}

/// Fills `tank` to capacity with the fluid shares of `mix`.
fn fill(run: &mut GarageRuntime, tank: ItemId, mix: [f32; 6]) {
    let sum: f32 = mix.iter().sum();
    let fluid = run
        .area
        .items
        .get_mut(tank)
        .unwrap()
        .fluid
        .as_mut()
        .unwrap();
    let entries: Vec<_> = (0..6)
        .filter(|&i| mix[i] > 0.)
        .map(|i| FluidEntry {
            fluid_type: i as u8,
            amount: fluid.capacity_liters * mix[i] / sum,
        })
        .collect();
    fluid.restore_entries(&entries);
    run.area.items.mark_dirty(tank);
}

/// Fuel of the mix the mounted engine wants (car06 can have the diesel engine), oil and water.
/// A two-stroke gets its oil in the fuel mix (petrol and 2.5% oil) and no sump oil.
fn fill_fluids(run: &mut GarageRuntime, key: &ObjectKey) {
    let mounted = run.powertrain.mounted_on(&run.area.items, key);
    let engine = run
        .powertrain
        .engine(mounted.engine.expect("mounted engine"))
        .unwrap();
    let (fuel, oil_tank, oil) = (
        engine.record.fuel_consumption,
        engine.oil_tank.filter(|_| !engine.record.two_stroke),
        engine.record.oil_fluid as usize,
    );
    let coolant = mounted
        .radiator
        .and_then(|item| run.powertrain.radiator(item))
        .and_then(|radiator| radiator.coolant_tank);
    let one = |index: usize| std::array::from_fn(|i| (i == index) as u8 as f32);
    fill(run, run.vehicles[key].fuel_tank.expect("fuel tank"), fuel);
    if let Some(tank) = oil_tank {
        fill(run, tank, one(oil));
    }
    if let Some(tank) = coolant {
        fill(run, tank, one(WATER));
    }
}

fn absolute(run: &GarageRuntime, key: &ObjectKey) -> ([f64; 3], [f32; 4]) {
    let pose = run.physics.snapshot(key).pose;
    (run.stream.origin.absolute(pose.position), pose.rotation)
}

fn horizontal(a: [f64; 3], b: [f64; 3]) -> f64 {
    ((a[0] - b[0]).powi(2) + (a[2] - b[2]).powi(2)).sqrt()
}

fn running(run: &GarageRuntime, key: &ObjectKey) -> bool {
    run.engine_output(key).running
}

/// Steps frames with `held` and `player_held` keys until `done` or `seconds` pass. `pressed` acts
/// on the first frame.
fn hold(
    run: &mut GarageRuntime,
    reference: &ReferenceManifest,
    held: u32,
    player_held: u16,
    pressed: u32,
    seconds: f32,
    done: impl Fn(&GarageRuntime) -> bool,
) -> bool {
    for step in 0..(seconds / crate::simulation::FIXED_DT) as usize {
        let input = FrameInput {
            held,
            player_held,
            pressed: if step == 0 { pressed } else { 0 },
            ..FrameInput::default()
        };
        frame_with(run, reference, input);
        if done(run) {
            return true;
        }
    }
    false
}

/// Moves the start car onto a deck `DECK_HEIGHT` above its start pose.
/// Returns the absolute deck center.
fn insert_deck(run: &mut GarageRuntime, key: &ObjectKey) -> [f64; 3] {
    let mut pose = run.physics.snapshot(key).pose;
    let clearance = pose.position[1] - ground(run, pose.position).expect("ground under the car");
    pose.position[1] += DECK_HEIGHT;
    let center = [
        pose.position[0],
        pose.position[1] - clearance - DECK[1],
        pose.position[2],
    ];
    run.physics.insert_box(center, DECK);
    run.physics.reset_preserving_wheels(key, pose);
    run.physics.refresh_queries();
    run.stream.origin.absolute(center)
}

/// One start car: fill, start, drive about 10 m in first gear, save, load.
fn start_car_drives_saves_and_loads(reference: &ReferenceManifest, field: &str) {
    let stamp = roadside_stamp(reference);
    let mut run = start_world(reference, field);
    run.started = true;
    run.paused = false;
    // Manual first gear. Automatic clutch permits the shift without the pedal.
    run.driving_assists = DrivingAssists {
        automatic_clutch: true,
        automatic_gears: false,
    };
    for _ in 0..2000 {
        if start_key(&run).is_some() {
            break;
        }
        frame(&mut run, reference);
    }
    let key = start_key(&run).unwrap_or_else(|| panic!("{field}: start car did not spawn"));
    let entry = reference
        .world
        .startup
        .iter()
        .find(|s| s.start_car && s.group == field)
        .unwrap();
    assert_eq!(
        Some(&reference.world.prototypes[run.vehicles[&key].prototype].source),
        entry.prototype.as_ref(),
        "{field}"
    );
    if field == "StartCar06" {
        // The root randomTypeSelector variant sets the fuel tank: 0.5 L diesel for the diesel engine.
        let mounted = run.powertrain.mounted_on(&run.area.items, &key);
        let engine = run.powertrain.engine(mounted.engine.unwrap()).unwrap();
        let diesel = engine.record.fuel_consumption[5] > 0.;
        let tank = run.vehicles[&key].fuel_tank.unwrap();
        let fuel = run.area.items.get(tank).unwrap().fluid.as_ref().unwrap();
        let liters = fuel.mixture().quantities_liters;
        assert_eq!(liters[5] > 0., diesel, "{field}: diesel follows the engine");
        assert!((fuel.total() - 0.5).abs() < 1e-4, "{field}: start fuel");
    }
    let deck = insert_deck(&mut run, &key);
    assert!(enter(&mut run, reference), "{field}: driver seat");
    settle(&mut run, reference, 1.);
    fill_fluids(&mut run, &key);

    // Hold the key until the engine runs.
    let started = hold(
        &mut run,
        reference,
        keys::IGNITION,
        0,
        keys::IGNITION,
        20.,
        |run| running(run, &key),
    );
    assert!(started, "{field}: engine starts");
    frame_with(
        &mut run,
        reference,
        FrameInput {
            released: keys::IGNITION,
            ..FrameInput::default()
        },
    );
    settle(&mut run, reference, 1.);
    assert!(running(&run, &key), "{field}: engine idles");

    // First gear, then throttle for about 10 m.
    hold(&mut run, reference, 0, 0, keys::GEAR_1, 0.2, |_| false);
    let first = run.vehicles[&key].drivetrain.gear();
    let neutral = crate::starting_area::world_vehicle(reference, run.vehicles[&key].prototype)
        .gear_ratios
        .iter()
        .position(|r| *r == 0.)
        .unwrap();
    assert_eq!(first, neutral + 1, "{field}: first gear");
    let start = absolute(&run, &key).0;
    let moved = hold(&mut run, reference, 0, player_keys::GAS, 0, 30., |run| {
        horizontal(absolute(run, &key).0, start) > 10.
    });
    assert!(moved, "{field}: drives 10 m");
    assert_eq!(run.vehicles[&key].drivetrain.gear(), first, "{field}");
    hold(&mut run, reference, 0, player_keys::BRAKE, 0, 4., |_| false);
    assert!(running(&run, &key), "{field}: engine runs after the drive");

    let saved = (
        absolute(&run, &key),
        run.vehicles[&key].drivetrain.gear(),
        run.vehicles[&key].ignition.ignition,
        run.mounted_engine(&key).unwrap().state.capture_save(),
    );
    let snapshot = run
        .capture_save(reference, &stamp, "2026-10-26T12:00:00Z")
        .unwrap();
    let snapshot: crate::save::SaveSnapshot =
        serde_json::from_slice(&serde_json::to_vec(&snapshot).unwrap()).unwrap();
    assert_eq!(snapshot.world.start_car.field(), field);

    // The save sets the start car. The mode start car is ignored.
    let mut loaded = GarageRuntime::from_save(
        reference,
        &stamp,
        &snapshot,
        GameMode::default(),
        reference.world.draw_distance.default,
    )
    .unwrap();
    assert_eq!(loaded.stream.start_car().field(), field);
    let deck = loaded.stream.origin.local(deck);
    loaded.physics.insert_box(deck, DECK);
    for _ in 0..2000 {
        prepare_ready(&mut loaded, reference);
        if loaded.advance_saved_restore(reference).unwrap()
            && !loaded.restore_pending()
            && loaded.vehicles.contains_key(&key)
        {
            break;
        }
    }
    assert!(!loaded.restore_pending(), "{field}");
    loaded.refresh_powertrains();
    let ((position, rotation), gear, ignition, engine) = saved;
    let (now, now_rotation) = absolute(&loaded, &key);
    for i in 0..3 {
        assert!(
            (now[i] - position[i]).abs() < 0.01,
            "{field}: {position:?} -> {now:?}"
        );
    }
    let dot: f32 = (0..4).map(|i| now_rotation[i] * rotation[i]).sum();
    assert!(
        dot.abs() > 0.9999,
        "{field}: {rotation:?} -> {now_rotation:?}"
    );
    assert_eq!(loaded.vehicles[&key].drivetrain.gear(), gear, "{field}");
    assert_eq!(loaded.vehicles[&key].ignition.ignition, ignition, "{field}");
    assert_eq!(
        loaded.mounted_engine(&key).unwrap().state.capture_save(),
        engine,
        "{field}"
    );
    assert!(running(&loaded, &key), "{field}: engine runs after load");
}

#[test]
#[ignore = "Requires exported Steam assets"]
fn start_cars_drive_save_and_load() {
    let reference = reference();
    assert_eq!(reference.build_id, 11140890);
    for field in START_CARS {
        start_car_drives_saves_and_loads(&reference, field);
    }
}

/// The start car pick uses the menu random, so a Lada world draws the same spawn records.
/// The spawn context current car is now 15 for the Lada: source `carTypeToCarID[2]`.
/// Before the start car pick, the loader kept 1.
#[test]
#[ignore = "Requires exported Steam assets"]
fn lada_start_keeps_the_spawn_records() {
    let reference = reference();
    let records = || {
        let run = start_world(&reference, StartCar::LADA.field());
        run.stream.contents.state.capture_history()
    };
    let first = records();
    assert_eq!(first.context.current_car, 15);
    assert!(!first.boundaries.is_empty());
    assert_eq!(first, records());
}

/// The Lada world with a Car07full copy 30 m above `StartCar02`. Returns the copy's startup ordinal.
fn car07_reference() -> (ReferenceManifest, u32) {
    let mut reference = reference();
    let world = std::sync::Arc::make_mut(&mut reference.world);
    let item = world.catalog.items.iter().find(|i| i.name == "car07full");
    let source = item
        .and_then(|i| i.prototype.clone())
        .expect("car07full prototype");
    let car = world.prototypes.iter().find(|p| p.source == source);
    assert!(car.is_some_and(|p| p.vehicle.as_ref().unwrap().driving.force_automatic));
    let start = world
        .startup
        .iter()
        .position(|s| s.start_car && s.group == StartCar::LADA.field())
        .expect("StartCar02 placement");
    let mut copy = world.startup[start].clone();
    copy.pose.translation[1] += 30.;
    copy.prototype = Some(source);
    world.startup.push(copy);
    let ordinal = world.startup.len() as u32 - 1;
    (reference, ordinal)
}

/// Car07 is pool only. With manual assists, its forced automatic still shifts up.
#[test]
#[ignore = "requires the installed source export"]
fn car07_drives_in_forced_automatic() {
    use crate::player::SeatOwner;
    let (reference, ordinal) = car07_reference();
    let mut run = start_world(&reference, StartCar::LADA.field());
    run.started = true;
    run.paused = false;
    run.driving_assists = DrivingAssists {
        automatic_clutch: false,
        automatic_gears: false,
    };
    let copy = |run: &GarageRuntime| {
        run.vehicles
            .keys()
            .find(|key| {
                matches!(key, ObjectKey::Generated(id)
                    if id.layer == WorldLayer::Startup && id.ordinal == ordinal)
            })
            .cloned()
    };
    for _ in 0..2000 {
        if copy(&run).is_some() {
            break;
        }
        frame(&mut run, &reference);
    }
    let key = copy(&run).expect("Car07 copy");
    insert_deck(&mut run, &key);
    let seat = run
        .physics
        .rapier()
        .colliders
        .iter()
        .find_map(|(handle, _)| {
            run.area
                .interaction
                .item_seat(handle)
                .filter(|s| s.owner == SeatOwner::Vehicle(key.clone()) && s.seat.driver)
        })
        .expect("Car07 driver seat");
    assert!(run.area.player.enter_item_seat(&seat, &mut run.physics));
    settle(&mut run, &reference, 1.);
    fill_fluids(&mut run, &key);
    // The dashboard clock binds on the car root item.
    let ObjectKey::Generated(id) = &key else {
        unreachable!()
    };
    assert!(
        run.stream.dial_bindings.iter().any(|b| &b.parent == id),
        "Car07 clock binding"
    );

    let started = hold(
        &mut run,
        &reference,
        keys::IGNITION,
        0,
        keys::IGNITION,
        20.,
        |run| running(run, &key),
    );
    assert!(started, "Car07 engine starts");
    let released = FrameInput {
        released: keys::IGNITION,
        ..FrameInput::default()
    };
    frame_with(&mut run, &reference, released);
    settle(&mut run, &reference, 1.);

    // Gears R, N, 2.5, 1.68, 1.0: throttle alone selects first and shifts up.
    let ratios =
        &crate::starting_area::world_vehicle(&reference, run.vehicles[&key].prototype).gear_ratios;
    let neutral = ratios.iter().position(|r| *r == 0.).unwrap();
    let start = absolute(&run, &key).0;
    let shifted = hold(&mut run, &reference, 0, player_keys::GAS, 0, 30., |run| {
        run.vehicles[&key].drivetrain.gear() > neutral + 1
    });
    assert!(shifted, "Car07 shifts up");
    let moved = hold(&mut run, &reference, 0, player_keys::GAS, 0, 30., |run| {
        horizontal(absolute(run, &key).0, start) > 20.
    });
    assert!(moved, "Car07 drives 20 m");
    assert!(running(&run, &key), "Car07 engine runs after the drive");
}

/// MC2: the garage start car with automatic gears starts in first and drives out of its spot.
#[test]
#[ignore = "requires the installed source export"]
fn garage_start_car_drives_in_automatic() {
    let reference = reference();
    let mut run = start_world(&reference, StartCar::LADA.field());
    run.started = true;
    run.paused = false;
    run.driving_assists = DrivingAssists {
        automatic_clutch: true,
        automatic_gears: true,
    };
    for _ in 0..2000 {
        if start_key(&run).is_some() {
            break;
        }
        frame(&mut run, &reference);
    }
    let key = start_key(&run).expect("start car");
    assert!(enter(&mut run, &reference), "driver seat");
    settle(&mut run, &reference, 1.);
    fill_fluids(&mut run, &key);
    let started = hold(
        &mut run,
        &reference,
        keys::IGNITION,
        0,
        keys::IGNITION,
        20.,
        |run| running(run, &key),
    );
    assert!(started, "engine starts");
    let released = FrameInput {
        released: keys::IGNITION,
        ..FrameInput::default()
    };
    frame_with(&mut run, &reference, released);
    settle(&mut run, &reference, 1.);
    let state = |run: &GarageRuntime| {
        let wheels: Vec<_> = (0..run.physics.wheel_contacts(&key).count())
            .map(|i| run.physics.wheel_motion(&key, i))
            .collect();
        format!(
            "gear {} rpm {:.0} speed {:.2} wheels {wheels:?}",
            run.vehicles[&key].drivetrain.gear(),
            run.engine_output(&key).rpm,
            run.physics.snapshot(&key).speed_mps,
        )
    };
    eprintln!("idle: {}", state(&run));
    let neutral = crate::starting_area::world_vehicle(&reference, run.vehicles[&key].prototype)
        .gear_ratios
        .iter()
        .position(|r| *r == 0.)
        .unwrap();
    hold(&mut run, &reference, 0, player_keys::GAS, 0, 0.1, |_| false);
    eprintln!("gas 0.1 s: {}", state(&run));
    assert_eq!(
        run.vehicles[&key].drivetrain.gear(),
        neutral + 1,
        "first gear at standstill: {}",
        state(&run)
    );
    let start = absolute(&run, &key).0;
    let moved = hold(&mut run, &reference, 0, player_keys::GAS, 0, 10., |run| {
        horizontal(absolute(run, &key).0, start) > 5.
    });
    assert!(moved, "drives 5 m: {}", state(&run));
}
