//! ROADMAP test pass: Car parts and repair (CP rows). Each test cites its source method.
use crate::reference::ReferenceManifest;
use crate::starting_area::GarageRuntime;
use crate::world::stream::tests::kit::*;
use crate::world::{GameMode, StartCar, StartCarChoice};

/// A new game with the start car `field`, or the default car.
fn new_game(reference: &ReferenceManifest, field: Option<&str>) -> GarageRuntime {
    let mode = GameMode {
        start_car: field.map_or(StartCarChoice::default(), |f| {
            StartCarChoice::Field(StartCar::from_field(f).unwrap())
        }),
        ..GameMode::default()
    };
    let mut run =
        GarageRuntime::new(reference, mode, reference.world.draw_distance.default).unwrap();
    prepare_ready(&mut run, reference);
    start_vehicle(&mut run);
    run
}

/// (prototype node, state) of each start car part condition, by node.
fn start_states(run: &GarageRuntime) -> Vec<(usize, u8)> {
    let c = &run.area.interaction.conditions;
    let mut states: Vec<_> = start_car_conditions(run)
        .iter()
        .map(|&(id, _, node)| (node, c.get(id).state))
        .collect();
    states.sort();
    states
}

/// CP18, CP19: source `partconditionscript.Start` draws the start state with the fixed
/// startup random (`mainscript.startupRandomState`), so each new game draws the same wear.
#[test]
#[ignore = "requires the installed source export"]
fn tp_cp18_start_car_starts_worn_the_same_each_game() {
    let reference = reference();
    for field in [None, Some("StartCar01"), Some("StartCar05")] {
        let first = start_states(&new_game(&reference, field));
        let second = start_states(&new_game(&reference, field));
        assert_eq!(first, second, "{field:?}: the same wear each new game");
        // The body is the root node condition.
        let body = first[0].1;
        assert!(
            (1..=3).contains(&body),
            "{field:?}: body state {body} is Used to Old"
        );
        assert!(
            first.iter().any(|s| s.1 == 4),
            "{field:?}: a part shows rust"
        );
    }
}

/// The aim of a use ray that hits `item` on its use collider.
fn aim_on(run: &GarageRuntime, item: crate::reference::ItemId) -> crate::interaction::ResolvedAim {
    let (collider, c) = run
        .physics
        .rapier()
        .colliders
        .iter()
        .find(|(h, _)| run.area.interaction.use_owner(*h) == Some(item))
        .expect("item use collider");
    crate::interaction::ResolvedAim {
        use_target: Some(crate::interaction::AimTarget {
            collider,
            item,
            distance: 0.5,
            point: c.compute_aabb().center().to_array(),
            slot: None,
        }),
        ..Default::default()
    }
}

/// One interaction step with `aim`, as `GarageRuntime::step` runs it.
fn interact(
    run: &mut GarageRuntime,
    aim: crate::interaction::ResolvedAim,
    input: crate::simulation::FrameInput,
) {
    let needs = run.survival.output();
    run.area.interaction.step(
        input,
        aim,
        &mut run.area.player,
        &mut run.area.body,
        &mut run.area.items,
        &mut run.physics,
        crate::simulation::FIXED_DT,
        needs,
        1.,
    );
}

/// CP20: source `fpscontroller.RayStuff` with `tankcapscript` (`GetCapString`, `canToggle`,
/// `USE`): the oil and coolant caps show their name and level, and E opens and closes them.
#[test]
#[ignore = "requires the installed source export"]
fn tp_cp20_oil_and_coolant_caps_show_level_and_toggle() {
    use crate::interaction::feedback::{ActionMask, FeedbackContext};
    use crate::simulation::{FrameInput, keys};
    let (reference, mut run) = load_garage();
    seconds(&mut run, &reference, 1.);
    let car = run
        .physics
        .snapshot(run.vehicle.as_ref().unwrap())
        .pose
        .position;
    let caps: Vec<_> = run
        .area
        .items
        .iter()
        .filter(|i| !i.removed && run.area.interaction.generated_opening(i.id).is_some())
        .filter(|i| {
            let p = i.pose.position;
            (p[0] - car[0]).hypot(p[2] - car[2]) < 4.
        })
        .map(|i| i.id)
        .collect();
    let mut fluids = Vec::new();
    for cap in caps {
        let aim = aim_on(&run, cap);
        let available = run.area.interaction.available_actions(
            &aim,
            &run.area.player,
            &run.area.items,
            &run.physics,
            run.survival.output(),
        );
        let info = run
            .area
            .interaction
            .feedback(&available, &run.area.items, &reference);
        assert_eq!(available.context, FeedbackContext::Cap, "{cap:?}");
        let f = info.fluid.as_ref().expect("cap level");
        let main = (0..6)
            .max_by(|&a, &b| f.quantities[a].total_cmp(&f.quantities[b]))
            .unwrap();
        assert!(
            available.actions.0 & ActionMask::USE != 0,
            "{cap:?}: E toggles"
        );
        assert!(f.presentation.has_name && !f.presentation.no_text && f.capacity > 0.);
        fluids.push(main);
        {
            let press = FrameInput {
                pressed: keys::USE,
                held: keys::USE,
                ..FrameInput::default()
            };
            let before = run.area.interaction.generated_valve(cap).unwrap();
            interact(&mut run, aim, press);
            interact(
                &mut run,
                aim,
                FrameInput {
                    released: keys::USE,
                    ..FrameInput::default()
                },
            );
            let open = run.area.interaction.generated_valve(cap).unwrap();
            interact(&mut run, aim, press);
            interact(
                &mut run,
                aim,
                FrameInput {
                    released: keys::USE,
                    ..FrameInput::default()
                },
            );
            let closed = run.area.interaction.generated_valve(cap).unwrap();
            assert_eq!(
                (before, open, closed),
                (0., 1., 0.),
                "{cap:?}: E opens and closes"
            );
        }
    }
    // Source fluid types: 1 oil, 3 water.
    assert!(
        fluids.contains(&1) && fluids.contains(&3),
        "oil and coolant caps: {fluids:?}"
    );
}

/// Seats the player in the start car, fills petrol and holds the key until the engine runs.
fn start_engine(run: &mut GarageRuntime, reference: &ReferenceManifest) {
    use crate::simulation::{FrameInput, keys};
    act(run, reference, r#"{"action":"seat","seat":"driver"}"#);
    act(run, reference, r#"{"action":"fill","fluid":0,"liters":20}"#);
    let key = run.vehicle.clone().unwrap();
    for step in 0..1000 {
        let pressed = if step == 0 { keys::IGNITION } else { 0 };
        frame_with(
            run,
            reference,
            FrameInput {
                held: keys::IGNITION,
                pressed,
                ..FrameInput::default()
            },
        );
        if run.engine_output(&key).running {
            break;
        }
    }
    frame_with(
        run,
        reference,
        FrameInput {
            released: keys::IGNITION,
            ..FrameInput::default()
        },
    );
    seconds(run, reference, 0.5);
    assert!(run.engine_output(&key).running, "the start car engine runs");
}

/// CP2: source `coolantTankscript.Cool` turns the fan by `selectedFanSpeed` (`maxFanSpeed`
/// times 1, 0.8, 0.6, 0.4, 0.2 per condition) while the engine runs and the fan is on.
/// CP3: source `carscript` smoke `rateOverTime = InverseLerp(0, maxRpm, rpm) * defSmokeRate`,
/// start color `GetBurnColor` every 2 s, and a 5 x 5 sheet.
#[test]
#[ignore = "requires the installed source export"]
fn tp_cp2_cp3_running_engine_spins_the_fan_and_smokes() {
    use crate::simulation::{FrameInput, keys};
    let (reference, mut run) = load_garage();
    seconds(&mut run, &reference, 1.);
    let key = run.vehicle.clone().unwrap();
    let vehicle = crate::starting_area::world_vehicle(&reference, run.vehicles[&key].prototype);
    for system in &vehicle.exhaust {
        assert_eq!(
            (
                system.scalars["UVModule.tilesX"],
                system.scalars["UVModule.tilesY"]
            ),
            (5., 5.),
            "smoke sheet"
        );
    }
    start_engine(&mut run, &reference);
    let mounted = run.powertrain.mounted_on(&run.area.items, &key);
    let radiator = mounted.radiator.expect("start car radiator");
    let engine = mounted.engine.unwrap();
    // A hot engine turns the fan on.
    let hot = run
        .powertrain
        .radiator(radiator)
        .unwrap()
        .record
        .fan_temperature_on
        + 5.;
    run.powertrain.engine_mut(engine).unwrap().state.temperature = hot;
    let angle = |run: &GarageRuntime| run.powertrain.radiator(radiator).unwrap().fan_angle;
    let before = angle(&run);
    seconds(&mut run, &reference, 1.);
    assert_ne!(angle(&run), before, "the fan spins while the engine runs");
    // Idle smoke, then more smoke at high rpm.
    let smoke = |run: &GarageRuntime| run.vehicles[&key].exhaust.particles(0).len();
    let idle = smoke(&run);
    assert!(idle > 0, "idle smoke");
    for _ in 0..150 {
        frame_with(
            &mut run,
            &reference,
            FrameInput {
                player_held: crate::simulation::player_keys::GAS,
                ..FrameInput::default()
            },
        );
    }
    assert!(
        smoke(&run) > idle,
        "smoke rate follows the rpm: {} > {idle}",
        smoke(&run)
    );
    // A fuel mix change changes the smoke color within 2 s.
    let color = |run: &GarageRuntime| {
        run.vehicles[&key]
            .exhaust
            .particles(0)
            .last()
            .unwrap()
            .color
    };
    let petrol = color(&run);
    let tank = run.vehicles[&key].fuel_tank.unwrap();
    let fluid = run
        .area
        .items
        .get_mut(tank)
        .unwrap()
        .fluid
        .as_mut()
        .unwrap();
    let total = fluid.total();
    fluid.restore_entries(&[
        crate::fluids::FluidEntry {
            fluid_type: 0,
            amount: total * 0.7,
        },
        crate::fluids::FluidEntry {
            fluid_type: 1,
            amount: total * 0.3,
        },
    ]);
    seconds(&mut run, &reference, 3.);
    assert_ne!(
        color(&run),
        petrol,
        "oil in the fuel changes the smoke color"
    );
    // The engine stops: the fan and the smoke stop.
    for _ in 0..200 {
        frame_with(
            &mut run,
            &reference,
            FrameInput {
                held: keys::IGNITION,
                pressed: keys::IGNITION,
                ..FrameInput::default()
            },
        );
        frame_with(
            &mut run,
            &reference,
            FrameInput {
                released: keys::IGNITION,
                ..FrameInput::default()
            },
        );
        if !run.engine_output(&key).running {
            break;
        }
    }
    assert!(!run.engine_output(&key).running, "the key stops the engine");
    let stopped = angle(&run);
    seconds(&mut run, &reference, 1.);
    assert_eq!(angle(&run), stopped, "the fan stops with the engine");
}

/// CP2: source `partslotscript.UnCraft` and `Craft` swap the start Lada radiator for a loot
/// `Car02Coolant`. Source `coolantTankscript.Cool` then turns the loot fan while the engine runs.
#[test]
#[ignore = "requires the installed source export"]
fn tp_cp2_loot_radiator_fan_spins_in_the_lada() {
    use crate::interaction::items::ItemLocation;
    let (reference, mut run) = load_garage();
    seconds(&mut run, &reference, 1.);
    let key = run.vehicle.clone().unwrap();
    let old = run
        .powertrain
        .mounted_on(&run.area.items, &key)
        .radiator
        .expect("start car radiator");
    let old_fan = old;
    // The radiator item can sit below its part: climb to the item in the part slot.
    let mut old = old;
    let slot = loop {
        match run.area.items.get(old).unwrap().location {
            ItemLocation::PartSlot(slot) => break slot,
            location => {
                old = run
                    .area
                    .items
                    .parent(old)
                    .unwrap_or_else(|| panic!("the radiator part is in a slot: {location:?}"))
            }
        }
    };
    let before: Vec<_> = run.powertrain.radiators().map(|r| r.item).collect();
    super::weapons::spawn(&mut run, &reference, "Car02Coolant");
    let loot = run
        .powertrain
        .radiators()
        .map(|r| r.item)
        .find(|item| !before.contains(item))
        .expect("the loot radiator binds as a radiator");
    let fan = loot;
    let mut loot = loot;
    while let Some(parent) = run.area.items.parent(loot) {
        loot = parent;
    }
    // Take the start radiator out and fit the loot one.
    let i = &mut run.area.interaction;
    i.parts
        .remove(old, &mut run.area.items, &mut run.physics)
        .expect("the start radiator comes off");
    i.parts
        .detach_mechanism(old, &mut i.mechanisms, &mut run.physics);
    i.sync_parts(&mut run.area.items, &mut run.physics).unwrap();
    seconds(&mut run, &reference, 0.5);
    let i = &mut run.area.interaction;
    i.parts
        .install(loot, slot, &mut run.area.items, &mut run.physics)
        .expect("the loot radiator fits the Lada slot");
    i.parts.mount_mechanism(loot, &mut i.mechanisms);
    i.sync_parts(&mut run.area.items, &mut run.physics).unwrap();
    seconds(&mut run, &reference, 0.5);
    let mounted = run.powertrain.mounted_on(&run.area.items, &key);
    assert_eq!(
        mounted.radiator,
        Some(fan),
        "the Lada uses the loot radiator"
    );
    start_engine(&mut run, &reference);
    // A hot engine turns the fan on.
    let engine = mounted.engine.unwrap();
    let hot = run
        .powertrain
        .radiator(fan)
        .unwrap()
        .record
        .fan_temperature_on
        + 5.;
    run.powertrain.engine_mut(engine).unwrap().state.temperature = hot;
    let angle = |run: &GarageRuntime| run.powertrain.radiator(fan).unwrap().fan_angle;
    let still = angle(&run);
    seconds(&mut run, &reference, 1.);
    assert_ne!(
        angle(&run),
        still,
        "the loot fan spins while the engine runs"
    );
    let old_angle = run.powertrain.radiator(old_fan).unwrap().fan_angle;
    seconds(&mut run, &reference, 1.);
    assert_eq!(
        run.powertrain.radiator(old_fan).unwrap().fan_angle,
        old_angle,
        "the removed radiator fan stays still"
    );
}

/// CP7: source `carscript.steer` saves with the car (`savedata.steer`) and the wheels keep
/// their angle when nobody drives.
#[test]
#[ignore = "requires the installed source export"]
fn tp_cp7_turned_wheels_stay_after_save_and_load() {
    use crate::simulation::{FrameInput, keys};
    let (reference, mut run) = load_garage();
    seconds(&mut run, &reference, 1.);
    let on_foot = run.area.player.body_pose(&run.physics);
    act(&mut run, &reference, r#"{"action":"seat","seat":"driver"}"#);
    let key = run.vehicle.clone().unwrap();
    for _ in 0..100 {
        frame_with(
            &mut run,
            &reference,
            FrameInput {
                held: keys::LEFT,
                player_held: crate::simulation::player_keys::STEER_LEFT,
                ..FrameInput::default()
            },
        );
    }
    let steer = run.vehicles[&key].steer;
    assert!(steer.abs() > 0.5, "wheels turned: {steer}");
    // A seated driver without steer keys centers the wheels. Nobody drives now: the steer stays.
    // The closed doors block the exit (BL), so the player goes back to the spot on foot.
    run.area.player.recover(on_foot, &mut run.physics);
    seconds(&mut run, &reference, 1.);
    assert_eq!(run.vehicles[&key].steer, steer, "wheels stay turned");
    let mut loaded = save_load(&run, &reference);
    seconds(&mut loaded, &reference, 1.);
    let key = loaded
        .vehicles
        .keys()
        .find(|k| **k == key)
        .cloned()
        .expect("start car after load");
    assert!(
        (loaded.vehicles[&key].steer - steer).abs() < 1e-3,
        "{} != {steer}",
        loaded.vehicles[&key].steer
    );
}

/// CP1: source `partslotscript.UnCraft` and `Craft`: the hood and the trunk lid come off and
/// go back on. `partconditionscript` repair: the wire brush takes off rust, the sponge dirt.
#[test]
#[ignore = "requires the installed source export"]
fn tp_cp1_hood_and_trunk_lid_refit_and_repair() {
    use crate::interaction::items::ItemLocation;
    let (reference, mut run) = load_garage();
    seconds(&mut run, &reference, 1.);
    let key = run.vehicle.clone().unwrap();
    let car = run.physics.snapshot(&key).pose;
    let root = run.area.items.resolve_key(&key).unwrap();
    let rotation = rapier3d::math::Rotation::from_array(car.rotation);
    // Hinged parts of the car: doors, the hood and the trunk lid. The hood and the lid sit on
    // the car axis, at its two ends.
    let mut hinged: Vec<_> = run
        .area
        .interaction
        .parts
        .mounted()
        .map(|(item, _)| item)
        .filter(|&item| run.area.items.descends(item, root))
        .map(|item| {
            let p =
                rapier3d::math::Vector::from_array(run.area.items.get(item).unwrap().pose.position)
                    - rapier3d::math::Vector::from_array(car.position);
            (rotation.inverse() * p, item)
        })
        .collect();
    let all = hinged.clone();
    hinged.retain(|(local, _)| local.x.abs() < 0.3 && local.y > 0.3);
    hinged.sort_by(|a, b| a.0.z.total_cmp(&b.0.z));
    assert!(hinged.len() >= 2, "hood and trunk lid: {all:?}");
    for (_, part) in [hinged[0], hinged[hinged.len() - 1]] {
        let ItemLocation::PartSlot(slot) = run.area.items.get(part).unwrap().location else {
            panic!("{part:?} is mounted");
        };
        let i = &mut run.area.interaction;
        i.parts
            .remove(part, &mut run.area.items, &mut run.physics)
            .expect("part comes off");
        i.parts
            .detach_mechanism(part, &mut i.mechanisms, &mut run.physics);
        i.sync_parts(&mut run.area.items, &mut run.physics).unwrap();
        assert!(!i.parts.installed(part));
        seconds(&mut run, &reference, 0.5);
        let i = &mut run.area.interaction;
        i.parts
            .install(part, slot, &mut run.area.items, &mut run.physics)
            .expect("part goes back on");
        i.parts.mount_mechanism(part, &mut i.mechanisms);
        i.sync_parts(&mut run.area.items, &mut run.physics).unwrap();
        assert!(i.parts.installed(part));
        seconds(&mut run, &reference, 0.5);
        assert_eq!(
            run.area.items.get(part).unwrap().location,
            ItemLocation::PartSlot(slot)
        );
    }
    // Repair: each tool that the source permission table allows improves the part state.
    let mut improved = [false; 3];
    for (id, ..) in start_car_conditions(&run) {
        for tool in [1u8, 2] {
            let c = &mut run.area.interaction.conditions;
            let before = c.get(id).state;
            if before > 0 && before < 4 && c.can_repair(id, tool) && !c.can_repair(id, 3) {
                assert_eq!(
                    c.repair(id, tool, 1.),
                    crate::conditions::RepairResult::Improved
                );
                assert_eq!(c.get(id).state, before - 1);
                improved[tool as usize] = true;
            }
        }
    }
    assert!(
        improved[1] && improved[2],
        "wire brush and sponge repair: {improved:?}"
    );
}

/// Holds LMB with the tool in the hand for `frames` inventory frames. Returns the frames with
/// work, with sound, with particles, and the tool model travel.
fn use_tool(
    run: &mut GarageRuntime,
    reference: &ReferenceManifest,
    item: crate::reference::ItemId,
    frames: usize,
) -> (usize, usize, usize, f32) {
    use crate::simulation::{FrameInput, keys};
    let (mut work, mut sound, mut particles, mut travel) = (0, 0, 0, 0f32);
    let rest = rapier3d::math::Vector::from_array(
        run.area
            .interaction
            .cleaning
            .signal(item)
            .unwrap()
            .pose
            .position,
    );
    for i in 0..frames {
        let input = FrameInput {
            held: keys::PRIMARY,
            pressed: if i == 0 { keys::PRIMARY } else { 0 },
            // Scrub: the mouse moves the view left and right.
            mouse: [if (i / 8) % 2 == 0 { 4. } else { -4. }, 0.],
            ..FrameInput::default()
        };
        super::weapons::input(run, reference, input);
        let cleaning = &run.area.interaction.cleaning;
        let s = cleaning.signal(item).unwrap();
        work += usize::from(s.doing_work);
        sound += usize::from(s.playing && s.voice_gain > 0.);
        particles += usize::from(!cleaning.particles(item, 0).is_empty());
        let at = rapier3d::math::Vector::from_array(s.pose.position);
        travel = travel.max((at - rest).length());
    }
    super::weapons::input(
        run,
        reference,
        FrameInput {
            released: keys::PRIMARY,
            ..FrameInput::default()
        },
    );
    (work, sound, particles, travel)
}

/// Stands 1.8 m out from the start car center line, on the player side, and aims at the body.
fn face_the_car(run: &mut GarageRuntime, reference: &ReferenceManifest) {
    use rapier3d::math::Vector;
    let key = run.vehicle.clone().unwrap();
    let chassis = *run.physics.rapier().bodies[run.physics.chassis_handle(&key)].position();
    let player = Vector::from_array(run.area.player.body_pose(&run.physics).position);
    let sign = (chassis.inverse() * player).x.signum();
    let stand = chassis * Vector::new(sign * 1.8, 0., 0.);
    stand_near(run, reference, stand.to_array(), 0.);
    let target = chassis * Vector::new(sign * 0.5, 0.3, 0.);
    run.area.player.look_at(target.to_array(), &run.physics);
    frame(run, reference);
}

/// Rust and dirt on the start car, so the brush and the sponge have work.
fn rust(run: &mut GarageRuntime) {
    for (id, ..) in start_car_conditions(run) {
        let c = &mut run.area.interaction.conditions;
        while c.get(id).state < 2 {
            c.wear(id);
        }
    }
}

/// CP13-CP16. Source `drotkefescript.Use` and `sprayscript.Update` on loot tools: in the hand,
/// LMB on a car part works. The brush model moves on the surface with particles and sound. The
/// spray takes its color, sprays and runs out. A worn brush is disabled.
#[test]
#[ignore = "requires the installed source export"]
fn tp_cp13_cp16_loot_tools_work_from_the_hand() {
    let (reference, mut run) = load_garage();
    settle(&mut run, &reference);
    rust(&mut run);
    let start = run.area.player.body_pose(&run.physics);
    for name in ["drotkefe01", "sponge01", "spraypaint01"] {
        // The sandbox places the tool in front of the player, clear of the car.
        run.area.player.recover(start, &mut run.physics);
        seconds(&mut run, &reference, 0.5);
        let item = super::weapons::take(&mut run, &reference, "hand", name);
        assert_eq!(
            run.area.interaction.equipment.item(),
            Some(item),
            "{name} in the hand"
        );
        face_the_car(&mut run, &reference);
        let quantity = run.area.interaction.cleaning.get(item).unwrap().quantity;
        let used = use_tool(&mut run, &reference, item, 150);
        let state = run.area.interaction.cleaning.get(item).unwrap();
        eprintln!("{name}: work/sound/particles/travel {used:?}, {quantity:?} -> {state:?}");
        let (work, sound, particles, travel) = used;
        assert!(work > 0, "{name} works on the car");
        assert!(particles > 0, "{name} shows particles");
        if name == "spraypaint01" {
            assert!(state.quantity < quantity, "the spray uses paint");
            let color = state.color.expect("the can has a color");
            let s = run.area.interaction.cleaning.signal(item).unwrap();
            assert_eq!(s.color, color, "the can shows its color");
            // Nearly empty: it runs out and stops.
            run.area
                .interaction
                .cleaning
                .get_mut(item)
                .unwrap()
                .quantity = Some(0.01);
            use_tool(&mut run, &reference, item, 50);
            let (work, ..) = use_tool(&mut run, &reference, item, 50);
            assert_eq!(work, 0, "an empty can does not spray");
        } else {
            assert!(travel > 0.01, "{name}: the model moves on the surface");
            if name == "drotkefe01" {
                // Wear the brush out.
                let own = run.area.interaction.conditions.item_condition(item);
                let own = own.expect("the brush has its own condition");
                // One state from worn out: the next finished repair wears it out.
                while run.area.interaction.conditions.get(own).state < 4 {
                    run.area.interaction.conditions.wear(own);
                }
                rust(&mut run);
                face_the_car(&mut run, &reference);
                use_tool(&mut run, &reference, item, 400);
                face_the_car(&mut run, &reference);
                let (work, ..) = use_tool(&mut run, &reference, item, 50);
                let state = run.area.interaction.cleaning.get(item).unwrap();
                assert!(state.disabled, "the worn brush is disabled");
                assert_eq!(work, 0, "a worn brush stops working");
            }
        }
        assert!(sound > 0, "{name} plays its sound");
        // Drop the tool.
        super::weapons::drop_hand(&mut run);
        seconds(&mut run, &reference, 0.5);
    }
}

/// The bound volume of `owner` from its prototype record of `storage_type` with the largest box.
fn storage_volume(
    run: &GarageRuntime,
    reference: &ReferenceManifest,
    owner: crate::reference::ItemId,
    prototype: &crate::reference::SourceId,
    storage_type: u8,
) -> u16 {
    let record = reference
        .storage
        .prototype_volumes
        .iter()
        .filter(|v| &v.prototype == prototype && v.storage_type == storage_type)
        .max_by(|a, b| {
            let size = |v: &crate::reference::storage::PrototypeVolumeReference| {
                v.half_extents.iter().product::<f32>()
            };
            size(a).total_cmp(&size(b))
        })
        .unwrap_or_else(|| panic!("storage type {storage_type} volume"));
    run.area
        .interaction
        .storage
        .owner_volume(owner, &record.source)
        .unwrap_or_else(|| panic!("bound storage type {storage_type} volume"))
}

/// Moves loose `item` to the box centre of bound volume `volume`, at rest.
fn put_in(run: &mut GarageRuntime, item: crate::reference::ItemId, volume: u16) {
    use rapier3d::math::Vector;
    let pose = run
        .area
        .interaction
        .storage
        .volume_pose(volume, &run.area.items, &run.physics)
        .expect("volume pose");
    let body = run.area.items.get(item).unwrap().body.unwrap();
    run.physics.edit_body(body, |b| {
        b.set_position(pose, true);
        b.set_linvel(Vector::ZERO, true);
        b.set_angvel(Vector::ZERO, true);
    });
    run.area.items.get_mut(item).unwrap().pose.position = pose.translation.to_array();
    run.physics.refresh_queries();
}

/// The owner that `item` is locked to.
fn locked_in(
    run: &GarageRuntime,
    item: crate::reference::ItemId,
) -> Option<crate::reference::ItemId> {
    match run.area.items.get(item).unwrap().location {
        crate::interaction::items::ItemLocation::Locked { owner, .. } => Some(owner),
        _ => None,
    }
}

/// Frames until `item` locks to `owner`, at most `limit` seconds. Returns the seconds taken.
/// Volumes of one owner can overlap: any of them counts.
fn lock_time(
    run: &mut GarageRuntime,
    reference: &ReferenceManifest,
    item: crate::reference::ItemId,
    owner: crate::reference::ItemId,
    limit: f32,
) -> Option<f32> {
    let frames = (limit / crate::simulation::FIXED_DT) as usize;
    for i in 0..frames {
        if locked_in(run, item) == Some(owner) {
            return Some(i as f32 * crate::simulation::FIXED_DT);
        }
        frame(run, reference);
    }
    (locked_in(run, item) == Some(owner)).then_some(limit)
}

/// Holds E and drags the mouse on usable `item` until it opens. Returns its closed snapshot.
fn open_usable(
    run: &mut GarageRuntime,
    item: crate::reference::ItemId,
) -> crate::world::MechanismSnapshot {
    use crate::simulation::{FrameInput, keys};
    let m = &mut run.area.interaction.mechanisms;
    let closed = m.snapshot(item).expect("usable mechanism");
    for mouse in [[0., 1e4], [0., -1e4], [1e4, 0.], [-1e4, 0.]] {
        for _ in 0..5 {
            m.step(
                item,
                FrameInput {
                    held: keys::USE,
                    mouse,
                    ..FrameInput::default()
                },
                crate::vehicle::VehiclePose {
                    position: [0.; 3],
                    rotation: [0., 0., 0., 1.],
                },
                crate::simulation::FIXED_DT,
            );
        }
        if m.value(item, false).unwrap() > 0.1 {
            m.sync(&mut run.physics);
            return closed;
        }
    }
    panic!("usable {item:?} does not open");
}

fn close_usable(
    run: &mut GarageRuntime,
    item: crate::reference::ItemId,
    closed: crate::world::MechanismSnapshot,
) {
    let m = &mut run.area.interaction.mechanisms;
    m.restore(item, closed).unwrap();
    m.sync(&mut run.physics);
}

/// Spawns world prototype `name` on open ground ahead of the player. Returns the volume owner,
/// right after its area loads.
fn spawn_storage(
    run: &mut GarageRuntime,
    reference: &ReferenceManifest,
    name: &str,
) -> (crate::reference::ItemId, crate::reference::SourceId) {
    let prototype = reference
        .world
        .prototypes
        .iter()
        .find(|p| {
            p.nodes
                .iter()
                .find(|n| n.parent.is_none())
                .is_some_and(|n| n.name == name)
                && reference
                    .storage
                    .prototype_volumes
                    .iter()
                    .any(|v| v.prototype == p.source)
        })
        .unwrap_or_else(|| panic!("{name} storage prototype"))
        .source
        .clone();
    let record = reference
        .storage
        .prototype_volumes
        .iter()
        .find(|v| v.prototype == prototype)
        .unwrap();
    let before: Vec<_> = run.area.items.iter().map(|i| i.id).collect();
    act(
        run,
        reference,
        &format!(r#"{{"action":"spawn","name":"{name}","forward":3}}"#),
    );
    load_placements(run, reference);
    let owner = run
        .area
        .items
        .iter()
        .map(|i| i.id)
        .filter(|id| !before.contains(id))
        .find(|id| {
            run.area
                .interaction
                .storage
                .owner_volume(*id, &record.source)
                .is_some()
        })
        .unwrap_or_else(|| {
            let new: Vec<_> = run
                .area
                .items
                .iter()
                .filter(|i| !before.contains(&i.id))
                .map(|i| (i.id, run.area.items.name(i.id), i.location))
                .collect();
            panic!("{name} volumes bind: {new:?}")
        });
    (owner, prototype)
}

/// CP5, CP6. Source `mountStuff` at physics lock setting 2: an item that two 1.5 s scans see in
/// a closed volume locks to the owner. An open gate (`usablescript`, or a `partslotscript` part
/// with an open usable) unlocks it. `StartFreeze` runs `QuickFreeze` in the first frames after
/// the owner loads, also for an `onlyManual` volume, whose `Upd` scan never runs. Locks save
/// with the owner (`physLockParentID`). Source `tubescript` has no save: a loaded hose lies
/// loose.
#[test]
#[ignore = "requires the installed source export"]
fn tp_cp5_cp6_cargo_locks_opens_and_saves() {
    use crate::interaction::items::ItemLocation;
    use crate::simulation::{FrameInput, keys};
    let (reference, mut run) = load_garage();
    seconds(&mut run, &reference, 1.);
    let key = run.vehicle.clone().unwrap();
    let car = run.area.items.resolve_key(&key).unwrap();
    let lada = reference
        .world
        .startup
        .iter()
        .find_map(|s| s.prototype.clone().filter(|_| s.group == "StartCar02"))
        .expect("start Lada prototype");
    // Glove box (0), trunk (3) and engine bay (4).
    let mut cargo = Vec::new();
    for storage_type in [0, 3, 4] {
        let volume = storage_volume(&run, &reference, car, &lada, storage_type);
        let item = super::weapons::spawn(&mut run, &reference, "cigi");
        put_in(&mut run, item, volume);
        cargo.push((storage_type, item, volume));
    }
    for &(storage_type, item, volume) in &cargo {
        let time = lock_time(&mut run, &reference, item, car, 4.);
        assert!(
            time.is_some(),
            "type {storage_type}: the item locks in the Lada"
        );
        assert_eq!(
            run.area.items.get(item).unwrap().location,
            ItemLocation::Locked { owner: car, volume },
            "type {storage_type}: in its own volume"
        );
    }
    // The trunk lid and the hood gate the trunk and the engine bay. Opening each lid
    // releases its own volume only.
    for gated in [3, 4] {
        let &(_, _, volume) = cargo.iter().find(|c| c.0 == gated).unwrap();
        let slots = run.area.interaction.storage.gate_slots(volume).to_vec();
        assert_eq!(slots.len(), 1, "type {gated}: one gate slot");
        let lid = run
            .area
            .interaction
            .parts
            .slot_item(slots[0])
            .expect("lid on");
        let usable = run
            .area
            .interaction
            .mechanisms
            .part_usable(lid)
            .expect("the lid has a usable");
        let closed = open_usable(&mut run, usable);
        seconds(&mut run, &reference, 2.);
        let loose: Vec<_> = cargo
            .iter()
            .filter(|(_, item, _)| locked_in(&run, *item).is_none())
            .map(|(storage_type, ..)| *storage_type)
            .collect();
        assert_eq!(loose, [gated], "an open lid releases its own volume");
        close_usable(&mut run, usable, closed);
        // Back in the closed volume, it locks again.
        let &(_, item, volume) = cargo.iter().find(|c| c.0 == gated).unwrap();
        put_in(&mut run, item, volume);
        assert!(
            lock_time(&mut run, &reference, item, car, 4.).is_some(),
            "type {gated}: locks again after the lid closes"
        );
    }

    // CP6: the hose on the Lada fuel cap.
    let hose = reference
        .item_prototypes
        .iter()
        .position(|p| p.hose.is_some())
        .expect("hose prototype");
    let car_pose = run.physics.snapshot(&key).pose.position;
    let fuel_cap = run
        .area
        .items
        .iter()
        .filter(|i| !i.removed && run.area.interaction.generated_opening(i.id).is_some())
        .filter(|i| {
            let p = i.pose.position;
            (p[0] - car_pose[0]).hypot(p[2] - car_pose[2]) < 4.
        })
        .map(|i| i.id)
        .max_by(|&a, &b| {
            let capacity = |cap| {
                let aim = aim_on(&run, cap);
                let available = run.area.interaction.available_actions(
                    &aim,
                    &run.area.player,
                    &run.area.items,
                    &run.physics,
                    run.survival.output(),
                );
                run.area
                    .interaction
                    .feedback(&available, &run.area.items, &reference)
                    .fluid
                    .map_or(0., |f| f.capacity)
            };
            capacity(a).total_cmp(&capacity(b))
        })
        .expect("Lada fuel cap");
    let cap_aim = aim_on(&run, fuel_cap);
    let tap = |run: &mut GarageRuntime, aim, key: u32| {
        interact(
            run,
            aim,
            FrameInput {
                pressed: key,
                held: key,
                ..FrameInput::default()
            },
        );
        interact(
            run,
            aim,
            FrameInput {
                released: key,
                ..FrameInput::default()
            },
        );
    };
    tap(&mut run, cap_aim, keys::USE);
    assert_eq!(
        run.area.interaction.generated_valve(fuel_cap),
        Some(1.),
        "the fuel cap opens"
    );
    // Lay the hose on the floor beside the cap, as a player drags it there.
    let record = &reference.item_prototypes[hose];
    let end_body = record.hose.as_ref().unwrap().ends[0].body;
    let end_pickup = record
        .pickups
        .iter()
        .position(|p| p.body == end_body)
        .unwrap();
    let segments: Vec<_> = run
        .area
        .spawned
        .iter()
        .filter(|s| s.prototype == hose)
        .map(|s| (s.id, s.pickup))
        .collect();
    let segment = segments.iter().find(|s| s.1 == end_pickup).unwrap().0;
    {
        use rapier3d::math::Vector;
        let cap = Vector::from_array(run.area.items.get(fuel_cap).unwrap().pose.position);
        let center = Vector::from_array(car_pose);
        let out = (cap - center).with_y(0.).normalize();
        // Turn the hose about its end so it points straight out from the car side.
        let at = |run: &GarageRuntime, id| {
            Vector::from_array(run.area.items.get(id).unwrap().pose.position)
        };
        let end = at(&run, segment);
        let far = segments
            .iter()
            .map(|s| at(&run, s.0))
            .max_by(|a, b| (*a - end).length().total_cmp(&(*b - end).length()))
            .unwrap();
        let turn =
            rapier3d::math::Rotation::from_rotation_arc((far - end).with_y(0.).normalize(), out);
        let target = (cap + out * 1.5).with_y(end.y);
        for &(id, _) in &segments {
            let body = run.area.items.get(id).unwrap().body.unwrap();
            run.physics.edit_body(body, |b| {
                let pose = *b.position();
                let pose = rapier3d::math::Pose::from_parts(
                    target + turn * (pose.translation - end),
                    turn * pose.rotation,
                );
                b.set_position(pose, true);
                b.set_linvel(Vector::ZERO, true);
                b.set_angvel(Vector::ZERO, true);
            });
        }
        run.physics.refresh_queries();
        seconds(&mut run, &reference, 1.);
        // Stand out beside the cap, clear of the car.
        let mut pose = run.area.player.body_pose(&run.physics);
        let at = cap + out * 1.;
        pose.position = [at.x, pose.position[1], at.z];
        run.area.player.recover(pose, &mut run.physics);
        seconds(&mut run, &reference, 0.5);
    }
    take(&mut run, &reference, segment);
    let fill_aim = crate::interaction::ResolvedAim {
        filling_target: cap_aim.use_target,
        ..Default::default()
    };
    tap(&mut run, fill_aim, keys::GRAB);
    assert_ne!(
        run.area.interaction.held(),
        Some(segment),
        "F attaches the hose to the open cap"
    );
    let near_cap = |run: &GarageRuntime| {
        let cap_at =
            rapier3d::math::Vector::from_array(run.area.items.get(fuel_cap).unwrap().pose.position);
        run.area
            .spawned
            .iter()
            .filter(|s| s.prototype == hose)
            .map(|s| {
                (rapier3d::math::Vector::from_array(
                    run.area.items.get(s.id).unwrap().pose.position,
                ) - cap_at)
                    .length()
            })
            .fold(f32::INFINITY, f32::min)
    };
    // The joint holding the end on the cap is CP4. Here the save runs while it is on.
    let near = near_cap(&run);
    assert!(near < 0.05, "a hose end sits on the cap: {near} m");

    // Save and load: the cargo stays locked in its volume. The hose comes back whole.
    let saved: Vec<_> = cargo
        .iter()
        .map(|&(storage_type, item, volume)| {
            let source = run
                .area
                .interaction
                .storage
                .volume_source(volume)
                .unwrap()
                .clone();
            let serial = run
                .area
                .spawned
                .iter()
                .find(|s| s.id == item)
                .unwrap()
                .serial;
            (storage_type, serial, source)
        })
        .collect();
    let mut loaded = save_load(&run, &reference);
    seconds(&mut loaded, &reference, 2.);
    let car = loaded
        .area
        .items
        .resolve_key(&key)
        .expect("Lada after load");
    for (storage_type, serial, source) in &saved {
        let item = loaded
            .area
            .spawned
            .iter()
            .find(|s| s.serial == *serial)
            .unwrap_or_else(|| panic!("type {storage_type}: cargo after load"))
            .id;
        let volume = loaded
            .area
            .interaction
            .storage
            .owner_volume(car, source)
            .unwrap();
        assert_eq!(
            loaded.area.items.get(item).unwrap().location,
            ItemLocation::Locked { owner: car, volume },
            "type {storage_type}: cargo stays locked after load"
        );
    }
    let segments = loaded
        .area
        .spawned
        .iter()
        .filter(|s| s.prototype == hose)
        .count();
    assert_eq!(
        segments,
        crate::reference::HOSE_SEGMENTS,
        "the hose loads whole"
    );

    // A fridge and a coffin on open ground. Items in them when their area loads lock at once.
    // The manual box locks only those: an item put in later stays loose. The pallet (`raklap`)
    // is outside the exported spawn pools, so no pallet appears in the game.
    let spawn = reference.area.player_spawn.translation.map(f64::from);
    super::blocked::teleport(&mut loaded, &reference, spawn[0] + 80., spawn[2] + 80.);
    let mut run = loaded;
    for (name, storage_type) in [("Fridge", 0), ("Coffin", 0), ("Box", 9)] {
        let item = super::weapons::spawn(&mut run, &reference, "cigi");
        let (owner, prototype) = spawn_storage(&mut run, &reference, name);
        let volume = storage_volume(&run, &reference, owner, &prototype, storage_type);
        put_in(&mut run, item, volume);
        let time = lock_time(&mut run, &reference, item, owner, 4.);
        eprintln!("{name}: locked after {time:?} s");
        assert!(
            time.is_some_and(|t| t < 0.2),
            "{name}: an item inside at load locks at once: {time:?}"
        );
        // Past the load passes.
        seconds(&mut run, &reference, 1.);
        let later = super::weapons::spawn(&mut run, &reference, "cigi");
        put_in(&mut run, later, volume);
        let time = lock_time(&mut run, &reference, later, owner, 4.);
        if name == "Box" {
            assert_eq!(time, None, "Box: an item put in later stays loose");
        } else {
            assert!(time.is_some(), "{name}: an item put in later locks");
        }
        if name == "Coffin" {
            let usable = run
                .area
                .items
                .iter()
                .map(|i| i.id)
                .find(|&id| {
                    run.area.interaction.mechanisms.contains(id)
                        && run.area.items.descends(id, owner)
                })
                .expect("coffin lid");
            open_usable(&mut run, usable);
            seconds(&mut run, &reference, 2.);
            assert_eq!(locked_in(&run, item), None, "Coffin: the open lid releases");
        }
        // Clear the spot for the next one.
        let player = run.area.player.body_pose(&run.physics);
        let mut pose = player;
        pose.position[0] += 6.;
        run.area.player.recover(pose, &mut run.physics);
        seconds(&mut run, &reference, 0.5);
    }
}
