//! ROADMAP test pass: the "Blocked checks" rows. Each test checks the part of its row that its
//! blocking bug does not hold. MC2 holds only the garage Lada: drive tests move it to the road.
use crate::reference::ReferenceManifest;
use crate::simulation::{FIXED_DT, FrameInput, keys, player_keys};
use crate::starting_area::GarageRuntime;
use crate::vehicle::to_rapier;
use crate::world::ObjectKey;
use crate::world::stream::tests::car_types::{
    absolute, fill_fluids, hold, release_handbrake, running,
};
use crate::world::stream::tests::drive_out::{enter, park, place_car, road_points};
use crate::world::stream::tests::garage_exit::open_door_and_look_out;
use crate::world::stream::tests::kit::*;
use rapier3d::math::Vector;

fn horizontal(a: [f64; 3], b: [f64; 3]) -> f64 {
    ((a[0] - b[0]).powi(2) + (a[2] - b[2]).powi(2)).sqrt()
}

/// The start Lada with the player in the driver seat, on the road about 60 m from the garage,
/// with fuel, oil and water, and the engine running.
pub(super) fn road_lada(run: &mut GarageRuntime, reference: &ReferenceManifest) -> ObjectKey {
    seconds(run, reference, 2.);
    let key = run.vehicle.clone().expect("start car");
    assert!(enter(run, reference), "driver seat");
    let start = absolute(run, &key).0;
    let points = road_points(run, reference);
    let i = points
        .iter()
        .position(|p| horizontal(*p, start) > 60.)
        .expect("road 60 m out");
    place_car(run, reference, points[i], points[i + 1], 0.8);
    park(run, reference, points[i]);
    fill_fluids(run, &key);
    let started = hold(
        run,
        reference,
        keys::IGNITION,
        0,
        keys::IGNITION,
        20.,
        |run| running(run, &key),
    );
    assert!(started, "the engine starts");
    frame_with(
        run,
        reference,
        FrameInput {
            released: keys::IGNITION,
            ..FrameInput::default()
        },
    );
    seconds(run, reference, 1.);
    assert!(running(run, &key), "the engine idles");
    release_handbrake(run, reference, &key);
    key
}

/// First gear, then gas until the car moves `metres`. Then brake to a stop.
fn drive(run: &mut GarageRuntime, reference: &ReferenceManifest, key: &ObjectKey, metres: f64) {
    run.driving_assists.automatic_clutch = true;
    hold(run, reference, 0, 0, keys::GEAR_1, 0.2, |_| false);
    let start = absolute(run, key).0;
    let moved = hold(run, reference, 0, player_keys::GAS, 0, 30., |run| {
        horizontal(absolute(run, key).0, start) > metres
    });
    let now = absolute(run, key).0;
    assert!(
        moved,
        "the Lada drives {metres} m on the road: moved {:.2} m, gear {} rpm {:.0} {:?}",
        horizontal(now, start),
        run.vehicles[key].drivetrain.gear(),
        run.engine_output(key).rpm,
        run.physics.snapshot(key)
    );
    hold(run, reference, 0, player_keys::BRAKE, 0, 4., |_| false);
    hold(run, reference, 0, 0, keys::NEUTRAL, 0.2, |_| false);
}

/// SA1. Source `carscript` ignition, gears and throttle; `savescript`. The Lada with fluids
/// starts, drives, returns to the garage, and keeps its pose and fluids after save and load.
#[test]
#[ignore = "requires the installed source export"]
fn tp_sa1_fill_start_drive_return_save_reload() {
    let (reference, mut run) = load_garage();
    let key = run.vehicle.clone().unwrap();
    seconds(&mut run, &reference, 2.);
    let home = run.physics.snapshot(&key).pose;
    let home_absolute = run.stream.origin.absolute(home.position);
    let key = road_lada(&mut run, &reference);
    drive(&mut run, &reference, &key, 20.);
    assert!(running(&run, &key), "the engine runs after the drive");
    // Return: the car parks at its garage pose.
    let mut pose = home;
    pose.position = run.stream.origin.local(home_absolute);
    pose.position[1] += 0.3;
    run.physics.reset_preserving_wheels(&key, pose);
    run.physics.refresh_queries();
    park(&mut run, &reference, home_absolute);
    // The driver parks on the handbrake.
    frame_with(
        &mut run,
        &reference,
        FrameInput {
            player_pressed: player_keys::HANDBRAKE,
            ..FrameInput::default()
        },
    );
    assert!(run.vehicles[&key].handbrake, "the handbrake is on");
    seconds(&mut run, &reference, 3.);
    let (position, _) = absolute(&run, &key);
    assert!(
        horizontal(position, home_absolute) < 0.5,
        "back in the garage"
    );
    let fuel = |run: &GarageRuntime| {
        let tank = run.vehicles[&key].fuel_tank.unwrap();
        run.area
            .items
            .get(tank)
            .unwrap()
            .fluid
            .as_ref()
            .unwrap()
            .total()
    };
    let before = fuel(&run);
    assert!(before > 1., "fuel left: {before}");
    let speed = run.physics.snapshot(&key).speed_mps;
    let mut loaded = save_load(&run, &reference);
    let (first, _) = absolute(&loaded, &key);
    seconds(&mut loaded, &reference, 2.);
    let (now, _) = absolute(&loaded, &key);
    assert!(
        horizontal(now, position) < 0.1,
        "the car pose reloads: moved {:.2} m, {:.2} m at load, speed at save {speed:.3} m/s, now {:.3} m/s",
        horizontal(now, position),
        horizontal(first, position),
        loaded.physics.snapshot(&key).speed_mps
    );
    assert!((fuel(&loaded) - before).abs() < 0.01, "the fuel reloads");
}

/// SA10. Source rigidbody sleep. The Lada drives on the road and parks: engine off, handbrake
/// on. The driver gets out and drops the petrol can. The car and the can settle and sleep.
#[test]
#[ignore = "requires the installed source export"]
fn tp_sa10_driven_lada_and_a_dropped_can_settle_and_sleep() {
    let (reference, mut run) = load_garage();
    let key = road_lada(&mut run, &reference);
    drive(&mut run, &reference, &key, 20.);
    // Short key taps until the engine stops. The first tap after the drive does not always turn
    // the ignition off.
    for _ in 0..5 {
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
        if hold(&mut run, &reference, 0, 0, 0, 1., |run| !running(run, &key)) {
            break;
        }
    }
    assert!(!running(&run, &key), "the engine stops");
    frame_with(
        &mut run,
        &reference,
        FrameInput {
            player_pressed: player_keys::HANDBRAKE,
            ..FrameInput::default()
        },
    );
    assert!(run.vehicles[&key].handbrake, "the handbrake is on");
    assert!(
        open_door_and_look_out(&mut run, &reference).is_some(),
        "clear exit"
    );
    assert!(
        run.area.player.try_exit(&mut run.physics),
        "the driver gets out"
    );
    seconds(&mut run, &reference, 1.);
    let can = reference
        .tanks
        .iter()
        .find(|t| {
            t.role == crate::reference::TankRole::Portable
                && t.fluids.iter().any(|f| f.fluid_type == 0)
        })
        .expect("petrol can")
        .item;
    take(&mut run, &reference, can);
    assert_eq!(
        run.area
            .interaction
            .drop_held(&mut run.area.items, &mut run.physics),
        Some(can),
        "the can drops"
    );
    // The player walks away. A kinematic player body that touches the car keeps it awake.
    let mut pose = run.area.player.body_pose(&run.physics);
    let car = run.physics.snapshot(&key).pose.position;
    let away = (Vector::from_array(pose.position) - Vector::from_array(car))
        .with_y(0.)
        .normalize_or_zero();
    let at = Vector::from_array(car) + away * 6.;
    pose.position = [at.x, pose.position[1] + 0.5, at.z];
    run.area.player.recover(pose, &mut run.physics);
    seconds(&mut run, &reference, 10.);
    let bodies = &run.physics.rapier().bodies;
    let chassis = &bodies[run.physics.chassis_handle(&key)];
    assert!(
        chassis.is_sleeping(),
        "the parked Lada sleeps: velocity {:?} {:?}",
        chassis.linvel(),
        chassis.activation()
    );
    let body = &bodies[run.area.items.get(can).unwrap().body.unwrap()];
    assert!(
        body.is_sleeping(),
        "the dropped can sleeps: speed {:.3} m/s",
        body.linvel().length()
    );
}

/// Positions of the limb targets that the body sends, relative to the player.
fn limbs(run: &GarageRuntime) -> [Vector; 4] {
    let t = run.area.body.sent_targets();
    [1, 2, 3, 4].map(|i| Vector::new(t[i][0], t[i][1], t[i][2]))
}

/// Car prototype node `node` of `key`, relative to the player.
fn node_local(
    run: &GarageRuntime,
    reference: &ReferenceManifest,
    key: &ObjectKey,
    node: u32,
) -> Vector {
    let prototype = &reference.world.prototypes[run.vehicles[key].prototype];
    let chassis = *run.physics.rapier().bodies[run.physics.chassis_handle(key)].position();
    let body = to_rapier(run.area.player.body_pose(&run.physics)).inverse();
    (body * chassis * to_rapier(crate::world::node_frame(prototype, 0, node as usize))).translation
}

/// The nearest distance of the `of` limb targets (0, 1 hands; 2, 3 legs) to `node` while
/// `input(frame)` runs for `seconds`.
fn reach(
    run: &mut GarageRuntime,
    reference: &ReferenceManifest,
    key: &ObjectKey,
    node: u32,
    of: [usize; 2],
    seconds: f32,
    input: impl Fn(usize) -> FrameInput,
) -> f32 {
    let mut best = f32::MAX;
    for i in 0..(seconds / FIXED_DT) as usize {
        frame_with(run, reference, input(i));
        let target = node_local(run, reference, key, node);
        let l = limbs(run);
        for j in of {
            best = best.min((l[j] - target).length());
        }
    }
    // Let the limbs go back.
    crate::world::stream::tests::kit::seconds(run, reference, 1.);
    best
}

/// PB5. Source `carscript.Update` (horn, ignition, pedals), `InteractSwitchLights`, handbrake,
/// `Shifted`; `fpscontroller.MoveNearestHand` and `MoveNearestLeg`. Each car control moves a
/// hand or a foot to its node.
#[test]
#[ignore = "requires the installed source export"]
fn tp_pb5_hands_and_feet_reach_the_car_controls() {
    let (reference, mut run) = load_garage();
    seconds(&mut run, &reference, 1.);
    act(&mut run, &reference, r#"{"action":"seat","seat":"driver"}"#);
    seconds(&mut run, &reference, 1.);
    let key = run.vehicle.clone().unwrap();
    // A running engine and manual gears: the stick shifts and the pedals work.
    fill_fluids(&mut run, &key);
    run.driving_assists = crate::reference::DrivingAssists {
        automatic_clutch: true,
        automatic_gears: false,
    };
    let r = crate::starting_area::world_vehicle(&reference, run.vehicles[&key].prototype)
        .reach
        .clone();
    eprintln!("Lada reach: {r:?}");
    const HANDS: [usize; 2] = [0, 1];
    const LEGS: [usize; 2] = [2, 3];
    let near = 0.1;
    let pressed = |pressed: u32, held: u32| {
        move |i: usize| FrameInput {
            pressed: if i == 0 { pressed } else { 0 },
            held,
            ..FrameInput::default()
        }
    };
    let player = |pressed: u16, held: u16| {
        move |i: usize| FrameInput {
            player_pressed: if i == 0 { pressed } else { 0 },
            player_held: held,
            ..FrameInput::default()
        }
    };
    let mut checks = Vec::new();
    let horn = r.horn.expect("horn node");
    checks.push((
        "horn",
        reach(
            &mut run,
            &reference,
            &key,
            horn,
            HANDS,
            0.5,
            pressed(keys::HORN, keys::HORN),
        ),
    ));
    let lights = r.lights.expect("lights node");
    checks.push((
        "lights",
        reach(
            &mut run,
            &reference,
            &key,
            lights,
            HANDS,
            0.5,
            pressed(keys::LIGHTS, 0),
        ),
    ));
    let ignition = r.ignition.expect("ignition node");
    let limbs_of = |leg: bool| if leg { LEGS } else { HANDS };
    checks.push((
        "ignition tap",
        reach(
            &mut run,
            &reference,
            &key,
            ignition,
            limbs_of(r.leg_ignition),
            0.5,
            |i| FrameInput {
                pressed: if i == 0 { keys::IGNITION } else { 0 },
                held: if i < 3 { keys::IGNITION } else { 0 },
                released: if i == 3 { keys::IGNITION } else { 0 },
                ..FrameInput::default()
            },
        ),
    ));
    checks.push((
        "ignition hold",
        reach(
            &mut run,
            &reference,
            &key,
            ignition,
            limbs_of(r.leg_ignition),
            1.,
            pressed(keys::IGNITION, keys::IGNITION),
        ),
    ));
    frame_with(
        &mut run,
        &reference,
        FrameInput {
            released: keys::IGNITION,
            ..FrameInput::default()
        },
    );
    assert!(running(&run, &key), "the held ignition starts the engine");
    let handbrake = r.handbrake.expect("handbrake node");
    checks.push((
        "handbrake",
        reach(
            &mut run,
            &reference,
            &key,
            handbrake,
            limbs_of(r.leg_handbrake),
            0.5,
            player(player_keys::HANDBRAKE, 0),
        ),
    ));
    if let Some(gear) = r.transmission {
        checks.push((
            "gear stick",
            reach(
                &mut run,
                &reference,
                &key,
                gear,
                limbs_of(r.leg_shift),
                0.5,
                pressed(keys::GEAR_1, 0),
            ),
        ));
    }
    let gas = r.gas.expect("gas node");
    checks.push((
        "gas",
        reach(
            &mut run,
            &reference,
            &key,
            gas,
            LEGS,
            0.5,
            player(0, player_keys::GAS),
        ),
    ));
    let brake = r.brake.expect("brake node");
    checks.push((
        "brake",
        reach(
            &mut run,
            &reference,
            &key,
            brake,
            LEGS,
            0.5,
            player(0, player_keys::BRAKE),
        ),
    ));
    let clutch = r.clutch.expect("clutch node");
    checks.push((
        "clutch",
        reach(
            &mut run,
            &reference,
            &key,
            clutch,
            LEGS,
            0.5,
            pressed(0, keys::CLUTCH),
        ),
    ));
    eprintln!(
        "nearest limb distances: {checks:?}, gear {}",
        run.vehicles[&key].drivetrain.gear()
    );
    for (name, d) in checks {
        assert!(d < near, "{name}: the limb stays {d} m from its node");
    }
}

/// Prop targets with the cactus gib, nearest to the player first, with their parent and center.
fn cacti(
    run: &GarageRuntime,
    reference: &ReferenceManifest,
) -> Vec<(usize, crate::world::GeneratedId, Vector)> {
    use crate::damage::BreakKind;
    let player = Vector::from_array(run.area.player.body_pose(&run.physics).position);
    let breakables = &run.stream.breakables;
    let mut found: Vec<_> = breakables
        .targets()
        .iter()
        .enumerate()
        .filter_map(|(i, t)| {
            let BreakKind::Prop { parent } = &t.kind else {
                return None;
            };
            reference.breakables[t.record].gib.as_ref()?;
            let collider = breakables.colliders_of(i).next()?;
            let center = run.physics.rapier().colliders[collider]
                .compute_aabb()
                .center();
            Some((i, parent.clone(), center))
        })
        .collect();
    found.sort_by(|a, b| (a.2 - player).length().total_cmp(&(b.2 - player).length()));
    found
}

/// The item prototype index of the gib of break target `target`.
fn gib_prototype(run: &GarageRuntime, reference: &ReferenceManifest, target: usize) -> usize {
    let record = run.stream.breakables.targets()[target].record;
    let gib = reference.breakables[record].gib.as_ref().unwrap();
    reference
        .item_prototypes
        .iter()
        .position(|p| &p.id == gib)
        .expect("gib prototype")
}

/// Gib spawns: one serial per spawned prototype copy.
fn gibs(run: &GarageRuntime, prototype: usize) -> usize {
    let mut serials: Vec<_> = run
        .area
        .spawned
        .iter()
        .filter(|s| s.prototype == prototype)
        .map(|s| s.serial)
        .collect();
    serials.sort_unstable();
    serials.dedup();
    serials.len()
}

/// WD19. Source `breakablescript.TryBreak(Rigidbody)`, `TryBreakShoot`, `TryBreakExplosion` and
/// `Break` on Cacti props: each break spawns the gib. Cacti has no `tosaveitemscript`, so Break
/// throws after the gib spawns: the cactus stays and does not break again. A new stream-in of its area binds a new breakablescript.
#[test]
#[ignore = "requires the installed source export"]
fn tp_wd19_cactus_breaks_once_and_stays() {
    use crate::damage::BreakKind;
    let (reference, mut run) = load_garage();
    seconds(&mut run, &reference, 1.);
    let spawn = reference.area.player_spawn.translation.map(f64::from);
    let mut at = None;
    for k in 1..20 {
        let x = spawn[0] + 150. * k as f64;
        let z = spawn[2] + 90. * k as f64;
        act(
            &mut run,
            &reference,
            &format!(r#"{{"action":"teleport","x":{x},"z":{z}}}"#),
        );
        super::world::wait_loaded(&mut run, &reference);
        seconds(&mut run, &reference, 1.);
        if cacti(&run, &reference).len() >= 3 {
            at = Some([x, z]);
            break;
        }
    }
    let at = at.expect("three cacti near a teleport point");
    let found = cacti(&run, &reference);
    let records = reference.breakables.clone();
    let gib = gib_prototype(&run, &reference, found[0].0);

    // A car-sized body runs into the first cactus trigger.
    let (hit, _, center) = found[0].clone();
    let body = run.physics.insert_body(
        rapier3d::prelude::RigidBodyBuilder::dynamic()
            .translation(center + Vector::new(-3., 0.5, 0.))
            .linvel(Vector::new(8., 0., 0.))
            .gravity_scale(0.),
    );
    run.physics.insert_collider_with_parent(
        rapier3d::prelude::ColliderBuilder::ball(0.5).mass(1000.),
        body,
    );
    run.physics.refresh_queries();
    let before = gibs(&run, gib);
    seconds(&mut run, &reference, 1.);
    assert!(
        run.stream.breakables.targets()[hit].destroyed,
        "the car hit breaks the cactus"
    );
    // A shot and a blast break the next two.
    let (shot, _, _) = found[1].clone();
    let (blast, _, _) = found[2].clone();
    for _ in 0..100 {
        run.stream.breakables.try_break_shoot(&records, shot, 50.);
    }
    run.stream
        .breakables
        .try_break_explosion(&records, blast, 1e4);
    seconds(&mut run, &reference, 1.);
    for (name, target) in [("shot", shot), ("blast", blast)] {
        assert!(
            run.stream.breakables.targets()[target].destroyed,
            "the {name} breaks a cactus"
        );
    }
    assert_eq!(
        gibs(&run, gib),
        before + 3,
        "each break spawns the cactus gib once"
    );
    // The cacti stay and do not break again.
    for target in [hit, shot, blast] {
        let t = &run.stream.breakables.targets()[target];
        assert!(!t.removed, "the cactus stays");
        for c in run.stream.breakables.colliders_of(target) {
            assert!(run.physics.rapier().colliders[c].is_enabled());
        }
        run.stream.breakables.try_break(&records, target, 1e6);
    }
    seconds(&mut run, &reference, 1.);
    assert_eq!(
        gibs(&run, gib),
        before + 3,
        "a broken cactus does not break again"
    );

    // A new stream-in of the area binds the cactus again.
    let parent = found[0].1.clone();
    act(
        &mut run,
        &reference,
        &format!(
            r#"{{"action":"teleport","x":{},"z":{}}}"#,
            at[0] + 8000.,
            at[1]
        ),
    );
    super::world::wait_loaded(&mut run, &reference);
    let bound = |run: &GarageRuntime| {
        run.stream
            .breakables
            .targets()
            .iter()
            .filter(|t| matches!(&t.kind, BreakKind::Prop { parent: p } if *p == parent))
            .map(|t| t.destroyed)
            .collect::<Vec<_>>()
    };
    for _ in 0..1000 {
        if bound(&run).is_empty() {
            break;
        }
        frame(&mut run, &reference);
    }
    assert_eq!(bound(&run), [false; 0], "the cactus area unloads");
    act(
        &mut run,
        &reference,
        &format!(r#"{{"action":"teleport","x":{},"z":{}}}"#, at[0], at[1]),
    );
    super::world::wait_loaded(&mut run, &reference);
    seconds(&mut run, &reference, 1.);
    let again = run
        .stream
        .breakables
        .targets()
        .iter()
        .position(|t| matches!(&t.kind, BreakKind::Prop { parent: p } if *p == parent))
        .expect("the cactus binds again");
    assert!(
        !run.stream.breakables.targets()[again].destroyed,
        "the cactus can break again: {:?}",
        bound(&run)
    );
    run.stream.breakables.try_break(&records, again, 1e6);
    seconds(&mut run, &reference, 1.);
    assert!(
        run.stream.breakables.targets()[again].destroyed,
        "it breaks again"
    );
}

/// WD2. Source `meleeweaponscript` hit on a building lamp trigger, `breakablescript.Break` lamp
/// branch, `savescript`. A shovel hit breaks the lamps, a switch does not light them, and they
/// stay broken after save and load.
#[test]
#[ignore = "requires the installed source export"]
fn tp_wd2_shovel_breaks_a_building_lamp() {
    use crate::damage::BreakKind;
    let (reference, mut run) = load_garage();
    seconds(&mut run, &reference, 1.);
    let records = reference.breakables.clone();
    let lamp = |run: &GarageRuntime| {
        run.stream.breakables.targets().iter().position(|t| {
            matches!(t.kind, BreakKind::Lamp { .. })
                && !t.destroyed
                && !records[t.record].no_destroy
                && !records[t.record].lamp.as_ref().unwrap().lamps.is_empty()
        })
    };
    let mut target = lamp(&run);
    for name in ["Mansion", "Bar", "Motel", "Benzinkut01"] {
        if target.is_some() {
            break;
        }
        let json = format!(r#"[{{"action":"building","name":"{name}"}}]"#);
        let mut sandbox = crate::sandbox::Sandbox::default();
        sandbox.queued.push(json);
        crate::sandbox::run(&mut run, &reference, &mut sandbox);
        if !sandbox.replies.iter().any(|r| r.contains(r#""ok":true"#)) {
            eprintln!("{name}: {:?}", sandbox.replies);
            continue;
        }
        if !super::world::loads(&mut run, &reference) {
            eprintln!("{name}: the area does not load");
            continue;
        }
        seconds(&mut run, &reference, 2.);
        target = lamp(&run);
    }
    let target = target.expect("a building lamp trigger");
    let BreakKind::Lamp { parent, control } = run.stream.breakables.targets()[target].kind.clone()
    else {
        unreachable!()
    };
    let record = run.stream.breakables.targets()[target].record;
    let bits = records[record]
        .lamp
        .as_ref()
        .unwrap()
        .lamps
        .iter()
        .fold(0u64, |bits, i| bits | 1 << i);
    let dark = |run: &GarageRuntime| broken_lamps(run, &parent, control) & bits == bits;
    assert!(!dark(&run));
    let shovel = reference.melee.shovel(&reference).unwrap();
    for _ in 0..200 {
        if run.stream.breakables.targets()[target].destroyed {
            break;
        }
        run.stream
            .breakables
            .try_break_melee(&records, target, shovel.contact.damage, FIXED_DT);
        frame(&mut run, &reference);
    }
    seconds(&mut run, &reference, 0.5);
    assert!(dark(&run), "the shovel breaks the lamps");
    let mut loaded = save_load(&run, &reference);
    seconds(&mut loaded, &reference, 1.);
    assert!(dark(&loaded), "the lamps stay broken after load");
}

/// Teleports the player on foot to absolute `x`, `z` and waits for the area.
pub(super) fn teleport(run: &mut GarageRuntime, reference: &ReferenceManifest, x: f64, z: f64) {
    act(
        run,
        reference,
        &format!(r#"{{"action":"teleport","x":{x},"z":{z}}}"#),
    );
    super::world::wait_loaded(run, reference);
    seconds(run, reference, 1.);
}

/// CP17. Source `savescript` tool data and `itemPlaceRemoveScript`: loot spray paint left and
/// color, and a worn out loot brush, stay after save and load, and after the area unloads.
#[test]
#[ignore = "requires the installed source export"]
fn tp_cp17_loot_tool_state_saves_and_streams() {
    use crate::reference::PrototypeTool;
    let (reference, mut run) = load_garage();
    seconds(&mut run, &reference, 1.);
    let spawn = reference.area.player_spawn.translation.map(f64::from);
    let home = [spawn[0] + 80., spawn[2] + 80.];
    teleport(&mut run, &reference, home[0], home[1]);
    let tools = reference.conditions.prototype_tools.clone();
    assert!(!tools.is_empty());
    let mut placed = Vec::new();
    for (i, tool) in tools.iter().enumerate() {
        let item = place_prototype(&mut run, &reference, &tool.prototype, 1.5 + i as f32);
        let state = run
            .area
            .interaction
            .cleaning
            .get_mut(item)
            .expect("the loot tool binds");
        match tool.kind {
            PrototypeTool::Spray { .. } => {
                state.quantity = Some(0.25);
                state.color = Some([0.2, 0.4, 0.6, 1.]);
            }
            PrototypeTool::Brush { .. } => state.disabled = true,
        }
        let expected = (state.quantity, state.color, state.disabled);
        run.area.items.mark_dirty(item);
        placed.push((run.area.items.object_key(item), expected));
    }
    seconds(&mut run, &reference, 1.);
    let check = |run: &GarageRuntime, stage: &str| {
        for (key, expected) in &placed {
            let item = run
                .area
                .items
                .resolve_key(key)
                .unwrap_or_else(|| panic!("{stage}: {key:?} is back"));
            let s = run
                .area
                .interaction
                .cleaning
                .get(item)
                .unwrap_or_else(|| panic!("{stage}: {key:?} binds"));
            assert_eq!(
                (s.quantity, s.color, s.disabled),
                *expected,
                "{stage}: {key:?} keeps its state"
            );
        }
    };
    check(&run, "live");
    let mut loaded = save_load(&run, &reference);
    seconds(&mut loaded, &reference, 2.);
    check(&loaded, "after load");
    teleport(&mut loaded, &reference, home[0] + 3000., home[1]);
    teleport(&mut loaded, &reference, home[0], home[1]);
    seconds(&mut loaded, &reference, 2.);
    check(&loaded, "after the area streams in again");
}

/// CP12. Source `partconditionscript` paint and repair, `savescript` and
/// `itemPlaceRemoveScript`: a painted and brushed roadside car part keeps its color and state
/// after its area unloads and loads, and after save and load.
#[test]
#[ignore = "requires the installed source export"]
fn tp_cp12_roadside_part_keeps_paint_and_repair() {
    use crate::world::ObjectKey;
    let (reference, mut run) = load_garage();
    seconds(&mut run, &reference, 1.);
    // A world car placed on open ground, as `spawn('Bus02Full')` places one.
    let spawn = reference.area.player_spawn.translation.map(f64::from);
    teleport(&mut run, &reference, spawn[0] + 80., spawn[2] + 80.);
    let lada = run.vehicle.clone();
    act(
        &mut run,
        &reference,
        r#"{"action":"spawn","name":"Bus02Full","forward":12}"#,
    );
    let mut found = None;
    for _ in 0..1500 {
        frame(&mut run, &reference);
        found = run.vehicles.keys().find_map(|key| match key {
            ObjectKey::Generated(id) if Some(key) != lada.as_ref() => Some(id.clone()),
            _ => None,
        });
        if found.is_some() {
            break;
        }
    }
    let car = found.expect("the placed car loads");
    // Save before any edit: the placed car alone.
    save_load(&run, &reference);
    seconds(&mut run, &reference, 2.);
    let at = run
        .stream
        .origin
        .absolute(run.area.player.body_pose(&run.physics).position);
    let point = [at[0], at[1], at[2]];
    let entries = content_conditions(&run, &car).expect("roadside car content");
    let conditions = &mut run.area.interaction.conditions;
    let &(painted, painted_index) = entries
        .iter()
        .find(|(id, _)| conditions.can_repair(*id, 0) && conditions.get(*id).state < 3)
        .expect("a paintable roadside part");
    let color = [0.1, 0.6, 0.3, 1.];
    assert!(conditions.paint(painted, color));
    let repaired = entries.iter().find_map(|&(id, index)| {
        let state = conditions.get(id).state;
        (id != painted
            && state > 0
            && (conditions.can_repair(id, 1) || conditions.can_repair(id, 2)))
        .then_some((id, index, state))
    });
    let repaired = repaired.map(|(id, index, before)| {
        let tool = if conditions.can_repair(id, 1) { 1 } else { 2 };
        assert_eq!(
            conditions.repair(id, tool, 1.),
            crate::conditions::RepairResult::Improved
        );
        (index, before - 1)
    });
    eprintln!("painted index {painted_index}, repaired {repaired:?}");
    seconds(&mut run, &reference, 1.);
    let check = |run: &GarageRuntime, stage: &str| {
        let entries = content_conditions(run, &car)
            .unwrap_or_else(|| panic!("{stage}: the roadside car is back"));
        let get = |index: usize| {
            let (id, _) = entries.iter().find(|(_, i)| *i == index).unwrap();
            run.area.interaction.conditions.get(*id)
        };
        assert_eq!(get(painted_index).color, color, "{stage}: the paint stays");
        if let Some((index, state)) = repaired {
            assert_eq!(get(index).state, state, "{stage}: the repair stays");
        }
    };
    check(&run, "live");
    // A sandbox placement stays loaded far away, so this checks the far move only.
    teleport(&mut run, &reference, point[0] + 3000., point[2]);
    teleport(&mut run, &reference, point[0], point[2]);
    seconds(&mut run, &reference, 2.);
    check(&run, "after the area streams in again");
    let mut loaded = save_load(&run, &reference);
    seconds(&mut loaded, &reference, 2.);
    check(&loaded, "after load");
}

/// CP10. Source `partconditionscript` repair and Refresh, `gumiscript.Refresh`: a tire repair
/// lowers the tire state, the frame sends the new state, and the wheel grip follows the quality.
#[test]
#[ignore = "requires the installed source export"]
fn tp_cp10_tire_repair_changes_condition_and_grip() {
    use crate::interaction::items::ItemLocation;
    use crate::vehicle::WheelAssembly;
    let (reference, mut run) = load_garage();
    seconds(&mut run, &reference, 2.);
    let key = run.vehicle.clone().unwrap();
    // A tire rides on a rim in a wheel slot.
    let items = &run.area.items;
    let conditions = &run.area.interaction.conditions;
    let (tire, item) = items
        .iter()
        .filter(|s| !s.removed)
        .find_map(|s| {
            let ItemLocation::Attached(rim) = s.location else {
                return None;
            };
            matches!(items.get(rim)?.location, ItemLocation::WheelSlot(_))
                .then(|| conditions.item_condition(s.id).map(|c| (c, s.id)))
                .flatten()
        })
        .expect("a mounted Lada tire with a condition");
    let tool = (0..7)
        .find(|&t| {
            let c = &mut run.area.interaction.conditions;
            if t == 0 {
                c.wear(tire);
            }
            c.can_repair(tire, t)
        })
        .unwrap_or_else(|| {
            let c = &run.area.interaction.conditions;
            panic!("a tool repairs the tire: {:?}", c.get(tire))
        });
    let qualities = |run: &GarageRuntime| {
        (0..run.physics.wheel_contacts(&key).count())
            .map(|i| match run.physics.wheel_assembly(&key, i) {
                WheelAssembly::Tire { quality, .. } => quality,
                _ => -1.,
            })
            .collect::<Vec<_>>()
    };
    frame(&mut run, &reference);
    let worn_state = run.area.interaction.conditions.get(tire).state;
    let worn = run.area.interaction.conditions.item_quality(item).unwrap();
    let before = qualities(&run);
    assert!(
        before.contains(&worn),
        "a wheel takes the worn tire quality {worn}: {before:?}"
    );
    let mut improved = false;
    for _ in 0..100 {
        if run.area.interaction.conditions.repair(tire, tool, 0.5)
            == crate::conditions::RepairResult::Improved
        {
            improved = true;
            break;
        }
    }
    assert!(improved, "the repair improves the tire");
    let state = run.area.interaction.conditions.get(tire).state;
    assert_eq!(state, worn_state - 1);
    frame(&mut run, &reference);
    let better = run.area.interaction.conditions.item_quality(item).unwrap();
    assert!(better > worn);
    let after = qualities(&run);
    assert!(
        after.contains(&better),
        "the wheel grip follows the repaired tire: {after:?}"
    );
}

/// One trigger pull with the inventory frame, as the driver runs it.
fn fire(run: &mut GarageRuntime, reference: &ReferenceManifest) {
    for i in 0..super::weapons::PRESS_FRAMES {
        super::weapons::press_frame(run, reference, i, keys::PRIMARY, 0);
    }
}

/// WD13. Source `weaponscript.Shot` (`ShootDecals` by tag and glass layer) and `DecalDestroyer`:
/// a shot leaves a bullet hole of the surface kind, the hole moves with the item, and it goes
/// away after its lifetime.
#[test]
#[ignore = "requires the installed source export"]
fn tp_wd13_bullet_holes_by_surface_follow_items_and_fade() {
    let (reference, mut run) = load_garage();
    seconds(&mut run, &reference, 1.);
    let spawn = reference.area.player_spawn.translation.map(f64::from);
    teleport(&mut run, &reference, spawn[0] + 80., spawn[2] + 80.);
    super::weapons::equip(&mut run, &reference, "Revolver");
    let decals = |run: &GarageRuntime| run.area.interaction.decals.rows().collect::<Vec<_>>();
    // A metal shovel 2 m ahead at chest height, held still in the air.
    let shovel = crate::world::stream::tests::melee::spawn(
        &mut run,
        &reference,
        crate::world::stream::tests::melee::weapon(&reference, &["shovel"]),
    );
    let body = run.area.items.get(shovel).unwrap().body.unwrap();
    let pose = run.area.player.view(&run.physics).camera_pose;
    let forward = rapier3d::math::Rotation::from_array(pose.rotation) * -Vector::Z;
    let at = Vector::from_array(pose.position) + forward.with_y(0.).normalize() * 2.;
    run.physics
        .edit_body(body, |b| b.set_gravity_scale(0., true));
    crate::world::stream::tests::melee::place(&mut run, shovel, at);
    frame(&mut run, &reference);
    let center = run.area.items.get(shovel).unwrap().pose.position;
    run.area.player.look_at(center, &run.physics);
    frame(&mut run, &reference);
    let before = decals(&run).len();
    fire(&mut run, &reference);
    let after = decals(&run);
    assert_eq!(
        after.len(),
        before + 1,
        "the shot leaves a hole on the shovel"
    );
    let hole = *after.last().unwrap();
    eprintln!("shovel hole kind {}", hole.kind);
    // The hole moves with the item.
    let y = |run: &GarageRuntime| run.area.items.get(shovel).unwrap().pose.position[1];
    let item_before = y(&run);
    crate::world::stream::tests::melee::place(&mut run, shovel, at + Vector::Y);
    frame(&mut run, &reference);
    let moved = *decals(&run).last().unwrap();
    let dy = moved.current.position[1] - hole.current.position[1];
    let item_dy = y(&run) - item_before;
    assert!(
        (dy - item_dy).abs() < 0.05,
        "the hole moves with the item: hole {dy}, item {item_dy}"
    );

    // Each hole goes away after its decal lifetime.
    if let Some(lifetime) = reference.guns.decals[hole.kind as usize].lifetime {
        // The inventory frame ages the holes.
        for _ in 0..((lifetime + 1.) / crate::simulation::FIXED_DT) as usize {
            super::weapons::input(&mut run, &reference, FrameInput::default());
        }
        assert_eq!(
            decals(&run).len(),
            before,
            "the hole fades after its lifetime"
        );
    }
}

/// WD8. Source `fpscontroller` reload rules and `weaponscript.Reload`: with the gun in the hand,
/// R at aimed loose ammo loads it and E unloads into it. The round total stays the same. Held R
/// moves the left hand to the gun.
#[test]
#[ignore = "requires the installed source export"]
fn tp_wd8_r_and_e_at_aimed_ammo_reload_and_unload() {
    use super::weapons::{PRESS_FRAMES, equip, input, press_frame, spawn};
    let (reference, mut run) = load_garage();
    settle(&mut run, &reference);
    let gun = equip(&mut run, &reference, "Revolver");
    let guns = &mut run.area.interaction.guns;
    let rule = guns.record(gun).unwrap().fr.reload.clone();
    let fr = [0., 1., rule.value, 0.5]
        .into_iter()
        .find(|v| crate::interaction::guns::fr_allows(&rule, *v))
        .expect("reload FR value");
    let rt = guns.runtime_mut(gun).unwrap();
    rt.fr = fr;
    rt.ammo.count = 1.;
    let ammo = spawn(&mut run, &reference, "revolverammo");
    seconds(&mut run, &reference, 0.5);
    // The reload rules read the focus, grab and use aims.
    let aim = |run: &mut GarageRuntime| {
        let point = run.area.items.get(ammo).unwrap().pose.position;
        run.area.player.look_at(point, &run.physics);
        frame(run, &reference);
        let a = &run.aim;
        [a.focus_target, a.grab_target, a.use_target]
            .iter()
            .flatten()
            .any(|t| t.item == ammo)
    };
    assert!(aim(&mut run), "the aim takes the ammo");
    let rounds = |run: &GarageRuntime| {
        let g = run.area.interaction.guns.runtime(gun).unwrap().ammo.count;
        (
            g,
            run.stream.ammo_item(ammo).expect("loose ammo").ammo.count,
        )
    };
    let (g0, a0) = rounds(&run);
    // The left hand reaches for the gun while R is held.
    let left = |run: &GarageRuntime| {
        let t = run.area.body.sent_targets()[1];
        let hand = Vector::new(t[0], t[1], t[2]);
        let body = to_rapier(run.area.player.body_pose(&run.physics)).inverse();
        let at = Vector::from_array(run.area.items.get(gun).unwrap().pose.position);
        (hand - body * at).length()
    };
    let rest = left(&run);
    let mut reach = f32::MAX;
    for i in 0..PRESS_FRAMES {
        press_frame(&mut run, &reference, i, keys::RELOAD, 0);
        reach = reach.min(left(&run));
    }
    let (g1, a1) = rounds(&run);
    eprintln!("R: gun {g0} -> {g1}, ammo {a0} -> {a1}, left hand {rest} -> {reach}");
    assert!(g1 > g0, "R loads the gun from the aimed ammo");
    assert_eq!(g1 + a1, g0 + a0, "the reload keeps the rounds");
    assert!(reach < rest - 0.05, "the left hand moves to the gun");
    // E unloads into the aimed ammo.
    let rt = run.area.interaction.guns.runtime_mut(gun).unwrap();
    rt.fr = fr;
    assert!(aim(&mut run), "the aim takes the ammo");
    for i in 0..PRESS_FRAMES {
        press_frame(&mut run, &reference, i, keys::USE, 0);
    }
    input(&mut run, &reference, FrameInput::default());
    let (g2, a2) = rounds(&run);
    eprintln!("E: gun {g1} -> {g2}, ammo {a1} -> {a2}");
    assert!(g2 < g1, "E unloads the gun into the aimed ammo");
    assert_eq!(g2 + a2, g0 + a0, "the unload keeps the rounds");
}

/// Liters in tank `id`.
fn liters(run: &GarageRuntime, id: crate::reference::ItemId) -> f32 {
    run.area
        .items
        .get(id)
        .unwrap()
        .fluid
        .as_ref()
        .unwrap()
        .total()
}

/// Turns the head to cap `cap` until the fill ray hits it.
fn aim_fill(run: &mut GarageRuntime, reference: &ReferenceManifest, cap: crate::reference::ItemId) {
    for _ in 0..20 {
        let point = run.area.items.get(cap).unwrap().pose.position;
        run.area.player.look_at(point, &run.physics);
        frame(run, reference);
        if run.aim.filling_target.is_some_and(|t| t.item == cap) {
            return;
        }
    }
    panic!(
        "fill ray misses {cap:?}: {:?}, held {:?}",
        run.aim.filling_target,
        run.area.interaction.held()
    );
}

/// One short key tap.
fn tap(run: &mut GarageRuntime, reference: &ReferenceManifest, key: u32) {
    for input in [
        FrameInput {
            held: key,
            pressed: key,
            ..FrameInput::default()
        },
        FrameInput {
            released: key,
            ..FrameInput::default()
        },
    ] {
        frame_with(run, reference, input);
    }
}

/// Moves can `can` and the free hose `segments` by `can_at` and `hose_at` from cap `cap`, as if
/// carried there. The y of each offset is the ground height change.
fn bring(
    run: &mut GarageRuntime,
    reference: &ReferenceManifest,
    (can, segments): (crate::reference::ItemId, &[crate::reference::ItemId]),
    cap: crate::reference::ItemId,
    can_at: Vector,
    hose_at: Vector,
) {
    let at =
        |run: &GarageRuntime, id| Vector::from_array(run.area.items.get(id).unwrap().pose.position);
    let filler = at(run, cap);
    let offset = (filler - at(run, can)).with_y(0.) + can_at;
    move_items(run, &[can], offset);
    let hose = segments.iter().map(|id| at(run, *id)).sum::<Vector>() / segments.len() as f32;
    let offset = (filler - hose).with_y(0.) + hose_at;
    move_items(run, segments, offset);
    seconds(run, reference, 1.);
}

/// The driver opens the car door if needed and gets out.
fn get_out(run: &mut GarageRuntime, reference: &ReferenceManifest) {
    if !run.area.player.can_exit(&run.physics) {
        assert!(
            open_door_and_look_out(run, reference).is_some(),
            "clear exit"
        );
    }
    assert!(
        run.area.player.try_exit(&mut run.physics),
        "the driver gets out"
    );
}

/// Moves the bodies of `items` by `offset`, as if carried there.
fn move_items(run: &mut GarageRuntime, items: &[crate::reference::ItemId], offset: Vector) {
    for id in items {
        let body = run.area.items.get(*id).unwrap().body.unwrap();
        run.physics.edit_body(body, |b| {
            b.set_translation(b.translation() + offset, true);
            b.set_linvel(Vector::ZERO, true);
            b.set_angvel(Vector::ZERO, true);
        });
    }
}

/// Stands 0.8 m from cap `cap`, on the side away from `center` when given.
fn stand_by_cap(
    run: &mut GarageRuntime,
    reference: &ReferenceManifest,
    cap: crate::reference::ItemId,
    center: Option<[f32; 3]>,
) {
    let point = Vector::from_array(run.area.items.get(cap).unwrap().pose.position);
    let away = center.map_or(Vector::ZERO, |c| {
        (point - Vector::from_array(c))
            .with_y(0.)
            .normalize_or_zero()
    });
    if away == Vector::ZERO {
        stand_near(run, reference, point.to_array(), 0.8);
        return;
    }
    let mut pose = run.area.player.body_pose(&run.physics);
    let at = point + away * 0.8;
    pose.position = [at.x, pose.position[1], at.z];
    run.area.player.recover(pose, &mut run.physics);
    seconds(run, reference, 0.5);
}

/// Opens cap `cap` with E.
fn open_cap(
    run: &mut GarageRuntime,
    reference: &ReferenceManifest,
    opening: crate::reference::OpeningId,
    cap: crate::reference::ItemId,
    center: Option<[f32; 3]>,
) {
    stand_by_cap(run, reference, cap, center);
    assert!(aim_at_item(run, reference, cap), "aim at the cap");
    if run.area.interaction.opening_valve(opening) <= 0. {
        tap(run, reference, keys::USE);
    }
    // A screw cap turns with the mouse wheel.
    for wheel in [1., -1.] {
        for _ in 0..50 {
            if run.area.interaction.opening_valve(opening) >= 1. {
                break;
            }
            frame_with(
                run,
                reference,
                FrameInput {
                    wheel,
                    ..FrameInput::default()
                },
            );
        }
    }
    assert!(
        run.area.interaction.opening_valve(opening) > 0.,
        "the cap opens: use target {:?}",
        run.aim.use_target
    );
}

/// Carries hose end `end` to cap `cap` and attaches it with F, as a player does: look at the
/// end and pick it up, look at the cap, and scroll the end up to it.
fn attach_end(
    run: &mut GarageRuntime,
    reference: &ReferenceManifest,
    end: crate::reference::ItemId,
    cap: crate::reference::ItemId,
    center: Option<[f32; 3]>,
) {
    stand_by_cap(run, reference, cap, center);
    let at =
        |run: &GarageRuntime, id| Vector::from_array(run.area.items.get(id).unwrap().pose.position);
    if run.area.interaction.held() != Some(end) {
        let point = at(run, end);
        run.area.player.look_at(point.to_array(), &run.physics);
        frame(run, reference);
        run.area
            .interaction
            .pickup_item(
                end,
                point.to_array(),
                &run.area.player,
                &mut run.area.items,
                &mut run.physics,
            )
            .unwrap();
        run.area.sync_inventory();
    }
    // Turn to the cap slowly: a fast turn drops the end.
    let from = at(run, end);
    for step in 1..=40 {
        let to = at(run, cap);
        run.area
            .player
            .look_at(from.lerp(to, step as f32 / 40.).to_array(), &run.physics);
        frame(run, reference);
    }
    aim_fill(run, reference, cap);
    assert_eq!(
        run.area.interaction.held(),
        Some(end),
        "the end stays in the hand"
    );
    for _ in 0..100 {
        if run.area.interaction.held() != Some(end) {
            break;
        }
        let eye = Vector::from_array(run.area.player.view(&run.physics).camera_pose.position);
        let gap = at(run, end).distance(eye) - at(run, cap).distance(eye);
        if gap.abs() < 0.1 {
            break;
        }
        frame_with(
            run,
            reference,
            FrameInput {
                wheel: -gap.signum(),
                ..FrameInput::default()
            },
        );
    }
    aim_fill(run, reference, cap);
    tap(run, reference, keys::GRAB);
    seconds(run, reference, 0.8);
}

/// CP4. Source `tubescript`: the garage siphon hose spawns clear of the walls. With one end on
/// the Lada tank, LMB drains the tank (`JustDrain`). With both ends on, fuel flows from the
/// higher end to the lower one (`Fill`). Driving away breaks a joint (`breakForce`). R gathers
/// the hose (`CoilingUp`), and a stored segment carries all segments (`Store`).
#[test]
#[ignore = "requires the installed source export"]
fn tp_cp4_siphon_hose_drains_siphons_tears_coils_and_stores() {
    use crate::reference::{HOSE_SEGMENTS, TankRole};
    let (reference, mut run) = load_garage();
    seconds(&mut run, &reference, 3.);
    let prototype = reference
        .item_prototypes
        .iter()
        .position(|p| p.hose.is_some())
        .expect("hose prototype");
    let segments: Vec<_> = run
        .area
        .spawned
        .iter()
        .filter(|s| s.prototype == prototype)
        .map(|s| s.id)
        .collect();
    assert_eq!(segments.len(), HOSE_SEGMENTS, "one garage hose");

    // Clear of the walls: no segment touches a fixed collider with a side contact.
    let world = run.physics.rapier();
    for &id in &segments {
        let body = run.area.items.get(id).unwrap().body.unwrap();
        for &collider in world.bodies[body].colliders() {
            for pair in world.narrow_phase.contact_pairs_with(collider) {
                let other = if pair.collider1 == collider {
                    pair.collider2
                } else {
                    pair.collider1
                };
                let fixed = world.colliders[other]
                    .parent()
                    .is_none_or(|b| world.bodies[b].is_fixed());
                for manifold in pair.manifolds() {
                    let side = manifold.data.normal.y.abs() < 0.7;
                    let deep = manifold.points.iter().any(|p| p.dist < -0.01);
                    assert!(
                        !(fixed && side && deep),
                        "hose segment {id:?} touches a wall: {other:?} normal {:?}",
                        manifold.data.normal
                    );
                }
            }
        }
    }

    // The Lada tank and the can.
    let key = run.vehicle.clone().expect("start car");
    fill_fluids(&mut run, &key);
    let tank = run.vehicles[&key].fuel_tank.expect("Lada fuel tank");
    let can = reference
        .tanks
        .iter()
        .find(|t| {
            t.role == TankRole::Portable
                && (t.capacity_liters - 20.).abs() < 0.5
                && t.fluids.iter().any(|f| f.fluid_type == 0)
        })
        .expect("20 L can")
        .item;
    // Room in the can.
    {
        let fluid = run.area.items.get_mut(can).unwrap().fluid.as_mut().unwrap();
        let total = fluid.total();
        crate::fluids::spill(fluid, total * 0.5);
        run.area.items.mark_dirty(can);
    }
    let (lada_opening, lada_cap) = run
        .area
        .interaction
        .tank_opening(tank)
        .expect("Lada tank opening");
    let (can_opening, can_cap) = run.area.interaction.tank_opening(can).expect("can opening");
    let car = Some(run.physics.snapshot(&key).pose.position);
    // The 2 m hose does not reach from the Lada to the can at the bench. Carry the can and
    // the hose to the left of the Lada filler.
    // The garage Lada has its left side to +Z. The hose lies ahead of the filler, clear of
    // where the player stands.
    bring(
        &mut run,
        &reference,
        (can, &segments),
        lada_cap,
        Vector::Z,
        Vector::new(1., 0., 0.5),
    );
    open_cap(&mut run, &reference, lada_opening, lada_cap, car);
    open_cap(&mut run, &reference, can_opening, can_cap, None);

    use crate::interaction::items::ItemLocation;
    // Look at the item and pick it up, as F does.
    let pick = |run: &mut GarageRuntime, id: crate::reference::ItemId| {
        let point = run.area.items.get(id).unwrap().pose.position;
        run.area.player.look_at(point, &run.physics);
        frame(run, &reference);
        let point = run.area.items.get(id).unwrap().pose.position;
        run.area
            .interaction
            .pickup_item(
                id,
                point,
                &run.area.player,
                &mut run.area.items,
                &mut run.physics,
            )
            .unwrap();
        run.area.sync_inventory();
    };
    // One end on the Lada tank. Source `TryAttach` attaches the free end nearer the cap, which
    // need not be the carried end.
    let ends = run.area.interaction.hose_ends(segments[0]).unwrap();
    attach_end(&mut run, &reference, ends[0].0, lada_cap, car);
    let ends = run.area.interaction.hose_ends(segments[0]).unwrap();
    let on_lada = ends
        .iter()
        .position(|e| e.1 == Some(lada_opening))
        .expect("one end on the Lada tank");
    assert_eq!(ends[1 - on_lada].1, None, "the other end stays free");
    let (lada_end, can_end) = (ends[on_lada].0, ends[1 - on_lada].0);
    // (Lada end cap, can end cap).
    let ends_now = |run: &GarageRuntime| {
        let ends = run.area.interaction.hose_ends(segments[0]).unwrap();
        (ends[on_lada].1, ends[1 - on_lada].1)
    };

    // Drain: carry the free end and hold LMB. The Lada tank drains out of it.
    // Look at the floor below the Lada cap: LMB on an aimed cap toggles it, as in the source.
    // Hold the free end there, so the hose stays slack.
    let floor = {
        let player = Vector::from_array(run.area.player.body_pose(&run.physics).position);
        let cap = Vector::from_array(run.area.items.get(lada_cap).unwrap().pose.position);
        (player + cap) * 0.5 - Vector::Y * 0.6
    };
    run.area.player.look_at(floor.to_array(), &run.physics);
    frame(&mut run, &reference);
    let eye = Vector::from_array(run.area.player.view(&run.physics).camera_pose.position);
    let hand = eye + (floor - eye).normalize() * 0.8;
    let body = run.area.items.get(can_end).unwrap().body.unwrap();
    run.physics.edit_body(body, |b| {
        b.set_translation(hand, true);
        b.set_linvel(Vector::ZERO, true);
    });
    run.physics.refresh_queries();
    pick(&mut run, can_end);
    seconds(&mut run, &reference, 0.5);
    assert_eq!(
        run.area.interaction.held(),
        Some(can_end),
        "the end stays in the hand"
    );
    assert_ne!(
        run.aim.use_target.map(|t| t.item),
        Some(lada_cap),
        "the Lada cap is not aimed"
    );
    let lada = liters(&run, tank);
    hold(
        &mut run,
        &reference,
        keys::PRIMARY,
        0,
        keys::PRIMARY,
        3.,
        |_| false,
    );
    assert!(
        run.area.interaction.opening_valve(lada_opening) > 0.,
        "the Lada cap stays open"
    );
    let drained = lada - liters(&run, tank);
    assert!(drained > 0.05, "LMB drains the Lada tank: {drained} L");
    // Release LMB: the drain stops.
    seconds(&mut run, &reference, 2.);
    let lada = liters(&run, tank);
    seconds(&mut run, &reference, 1.);
    assert!(
        lada - liters(&run, tank) < 0.01,
        "the drain stops: {lada} -> {}",
        liters(&run, tank)
    );

    // Siphon: carry the free end to the can. End 1 stays on the Lada tank.
    attach_end(&mut run, &reference, can_end, can_cap, None);
    assert_eq!(
        ends_now(&run),
        (Some(lada_opening), Some(can_opening)),
        "both ends on"
    );
    // The Lada tank is higher than the can on the floor: fuel flows into the can.
    let (lada, canned) = (liters(&run, tank), liters(&run, can));
    seconds(&mut run, &reference, 5.);
    assert!(
        liters(&run, tank) < lada && liters(&run, can) > canned,
        "fuel flows down: Lada {lada} -> {}, can {canned} -> {}",
        liters(&run, tank),
        liters(&run, can)
    );

    // Tear off: take the joined car, hose and can to the road as one, and drive away.
    assert!(enter(&mut run, &reference), "driver seat");
    let bodies: Vec<_> = segments
        .iter()
        .chain([&can])
        .map(|id| run.area.items.get(*id).unwrap().body.unwrap())
        .collect();
    let car_pose = |run: &GarageRuntime| to_rapier(run.physics.snapshot(&key).pose);
    let old = car_pose(&run);
    let relative: Vec<_> = bodies
        .iter()
        .map(|b| old.inverse() * *run.physics.rapier().bodies[*b].position())
        .collect();
    let start = absolute(&run, &key).0;
    let points = road_points(&run, &reference);
    let i = points
        .iter()
        .position(|p| horizontal(*p, start) > 60.)
        .expect("road 60 m out");
    place_car(&mut run, &reference, points[i], points[i + 1], 0.8);
    let new = car_pose(&run);
    for (body, relative) in bodies.iter().zip(&relative) {
        run.physics.edit_body(*body, |b| {
            b.set_position(new * *relative, true);
            b.set_linvel(Vector::ZERO, true);
            b.set_angvel(Vector::ZERO, true);
        });
    }
    run.physics.refresh_queries();
    park(&mut run, &reference, points[i]);
    assert_eq!(
        ends_now(&run),
        (Some(lada_opening), Some(can_opening)),
        "both ends on the road"
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
    assert!(started, "the engine starts");
    frame_with(
        &mut run,
        &reference,
        FrameInput {
            released: keys::IGNITION,
            ..FrameInput::default()
        },
    );
    release_handbrake(&mut run, &reference, &key);
    let active = |run: &GarageRuntime| bodies.iter().all(|b| run.physics.body_active(*b));
    run.driving_assists.automatic_clutch = true;
    hold(&mut run, &reference, 0, 0, keys::GEAR_1, 0.2, |_| false);
    let start = absolute(&run, &key).0;
    hold(&mut run, &reference, 0, player_keys::GAS, 0, 10., |run| {
        ends_now(run) != (Some(lada_opening), Some(can_opening))
    });
    assert_ne!(
        ends_now(&run),
        (Some(lada_opening), Some(can_opening)),
        "driving away tears the hose off: {:.1} m",
        horizontal(absolute(&run, &key).0, start)
    );
    // The stream can lag behind a test drive. A hose frozen on the moving car would snap back.
    assert!(active(&run), "the hose and the can stay active");
    hold(&mut run, &reference, 0, player_keys::BRAKE, 0, 2., |_| {
        false
    });
    get_out(&mut run, &reference);
    seconds(&mut run, &reference, 1.);

    // Coil: carry a free end and hold R. Source `CoilingUp` pulls the other segments to it.
    // The tear frees either end. The other end can stay on its tank, so its segment is left out
    // of the spread, and the player does not walk: a pull on the tank drops the carried end.
    let (end, other) = match ends_now(&run) {
        (None, _) => (lada_end, can_end),
        _ => (can_end, lada_end),
    };
    let point = run.area.items.get(end).unwrap().pose.position;
    stand_near(&mut run, &reference, point, 0.8);
    pick(&mut run, end);
    seconds(&mut run, &reference, 0.2);
    assert_eq!(
        run.area.interaction.held(),
        Some(end),
        "the end stays in the hand: ends {:?}",
        ends_now(&run)
    );
    let spread = |run: &GarageRuntime| {
        let at = |id| Vector::from_array(run.area.items.get(id).unwrap().pose.position);
        segments
            .iter()
            .filter(|id| **id != other)
            .map(|id| at(*id).distance(at(end)))
            .fold(0., f32::max)
    };
    let before = spread(&run);
    assert!(before > 0.2, "the hose lies spread: {before}");
    hold(
        &mut run,
        &reference,
        keys::RELOAD,
        0,
        keys::RELOAD,
        1.,
        |_| false,
    );
    assert!(
        spread(&run) < 0.7 * before,
        "R coils the hose: {before} -> {}, held {:?}, ends {:?}",
        spread(&run),
        run.area.interaction.held(),
        ends_now(&run)
    );

    // Store: select another slot. The carried segment goes to the inventory, and all segments
    // follow into the slot. Both ends are free.
    assert_eq!(
        run.area.interaction.held(),
        Some(end),
        "the end stays in the hand"
    );
    let slot = run.area.inventory.selected;
    let select = |run: &mut GarageRuntime, slot: u8| {
        super::weapons::input(
            run,
            &reference,
            FrameInput {
                inventory_released: 1 << slot,
                ..FrameInput::default()
            },
        );
        seconds(run, &reference, 0.5);
    };
    select(&mut run, (slot + 1) % 8);
    for id in &segments {
        let location = run.area.items.get(*id).unwrap().location;
        assert!(
            matches!(location, ItemLocation::Stored(_)),
            "segment {id:?} stored: {location:?}"
        );
    }
    assert_eq!(ends_now(&run), (None, None), "storing frees both ends");
    // Take it out: the segments return to the world.
    select(&mut run, slot);
    for id in &segments {
        let location = run.area.items.get(*id).unwrap().location;
        assert!(
            !matches!(location, ItemLocation::Stored(_)),
            "segment {id:?} out: {location:?}"
        );
    }
}
