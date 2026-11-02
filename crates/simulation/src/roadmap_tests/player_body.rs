//! ROADMAP test pass: Player body and outfit (PB rows).
use crate::interaction::items::ItemLocation;
use crate::reference::{ItemId, ReferenceManifest};
use crate::simulation::{FIXED_DT, FrameInput, player_keys};
use crate::starting_area::GarageRuntime;
use crate::world::stream::tests::kit::*;
use rapier3d::math::Vector;

/// Kills the player with the full HP damage, as a hard crash does.
fn kill(run: &mut GarageRuntime, reference: &ReferenceManifest) {
    run.survival
        .damage_instant(&reference.survival, reference.survival.maximum.hp);
    frame(run, reference);
    assert!(run.needs_output().dead);
}

/// One frame with `input`: the inventory frame, as the driver runs it, then one fixed step.
fn input(run: &mut GarageRuntime, reference: &ReferenceManifest, input: FrameInput) {
    let view = run.area.player.view(&run.physics).head_pose;
    let fixed = run
        .advance_inventory_frame(reference, input, FIXED_DT, view)
        .fixed_input;
    frame_with(run, reference, fixed);
}

/// The root name of item prototype `prototype`.
fn root_name(reference: &ReferenceManifest, prototype: usize) -> &str {
    let root = reference.item_prototypes[prototype].root;
    reference
        .nodes
        .iter()
        .find(|n| n.id == root)
        .map_or("", |n| n.name.as_str())
}

fn worn(run: &GarageRuntime) -> Vec<ItemId> {
    (0..crate::interaction::wear::WEAR_SLOTS)
        .filter_map(|slot| run.area.wear.item(slot))
        .collect()
}

/// Source `fpscontroller` `removeSunglassesUp` and `Death2`: X takes off the first edible worn
/// item first, then the first worn slot, into the hand. Save and load keep worn items. Death takes
/// each one off without pickup.
#[test]
#[ignore = "requires the installed source export"]
fn tp_pb13_pb15_x_takes_off_the_cigarette_first_worn_items_save_and_death_drops_each() {
    let (reference, mut run) = load_garage();
    settle(&mut run, &reference);
    // One wearable per slot, then the cigarette.
    let mut names = Vec::new();
    for slot in 0..crate::interaction::wear::WEAR_SLOTS as u32 {
        let w = reference
            .clothing
            .wearables
            .iter()
            .find(|w| w.type_index == slot && w.lenses.is_empty() == (slot != 0))
            .or_else(|| {
                reference
                    .clothing
                    .wearables
                    .iter()
                    .find(|w| w.type_index == slot)
            })
            .unwrap_or_else(|| panic!("a wearable of type {slot}"));
        let prototype = reference
            .item_prototypes
            .iter()
            .position(|p| p.id == w.prototype)
            .unwrap();
        names.push(root_name(&reference, prototype).to_string());
    }
    let cigarette = root_name(&reference, run.area.cigarettes.cigarette_prototype).to_string();
    let wear = |run: &mut GarageRuntime, name: &str| -> ItemId {
        let id = act(
            run,
            &reference,
            &format!(r#"{{"action":"wear","name":"{name}"}}"#),
        );
        ItemId(id.as_u64().unwrap() as u32)
    };
    let cig = wear(&mut run, &cigarette);
    let cig_slot = run
        .area
        .wear
        .slot(&run.area.interaction, cig)
        .expect("the cigarette has a wear slot");
    for (slot, name) in names.iter().enumerate() {
        if slot != cig_slot {
            wear(&mut run, name);
        }
    }
    assert_eq!(
        worn(&run).len(),
        names.len(),
        "one item per slot: {names:?}"
    );
    let take_off = |run: &mut GarageRuntime| -> Option<ItemId> {
        let before = worn(run);
        input(
            run,
            &reference,
            FrameInput {
                player_released: player_keys::TAKE_OFF,
                ..Default::default()
            },
        );
        let off = before.into_iter().find(|id| !worn(run).contains(id))?;
        assert_eq!(
            run.area.interaction.held(),
            Some(off),
            "the item goes to the hand"
        );
        run.area
            .interaction
            .drop_held(&mut run.area.items, &mut run.physics);
        frame(run, &reference);
        Some(off)
    };
    assert_eq!(
        take_off(&mut run),
        Some(cig),
        "the cigarette comes off first"
    );
    let first = (0..names.len())
        .find_map(|slot| run.area.wear.item(slot))
        .unwrap();
    assert_eq!(take_off(&mut run), Some(first), "then the first worn slot");
    wear(&mut run, &cigarette);
    let count = worn(&run).len();
    let mut loaded = save_load(&run, &reference);
    seconds(&mut loaded, &reference, 0.5);
    assert_eq!(
        worn(&loaded).len(),
        count,
        "worn items stay worn after load"
    );
    // Death: each worn item comes off and lies in the world.
    let on = worn(&loaded);
    kill(&mut loaded, &reference);
    assert!(worn(&loaded).is_empty(), "death takes every item off");
    for id in on {
        let item = loaded.area.items.get(id).unwrap();
        assert_eq!(item.location, ItemLocation::World);
        assert!(loaded.physics.rapier().bodies[item.body.unwrap()].is_dynamic());
    }
}

/// Source `fpscontroller.SpawnRagdol` and `mainscript` `died` frame: the ragdoll takes the joints
/// of the record and the speed of the parent body. The camera follows its head. `DeadV.weight`
/// and `deathSens` run over 14 s, then the menu opens. A loaded save has no ragdoll.
#[test]
#[ignore = "requires the installed source export"]
fn tp_pb9_pb10_pb11_pb12_death_ragdoll_follows_fades_14_s_and_a_load_clears_it() {
    let (reference, mut run) = load_garage();
    settle(&mut run, &reference);
    assert_eq!(
        reference.survival.death_seconds, 14.,
        "mainscript InverseLerp(0, 14)"
    );
    let saved = save(&run, &reference);
    // Die while the player body moves.
    let body = run.area.player.body_handle();
    let speed = Vector::new(3., 0., 0.);
    run.physics.edit_body(body, |b| b.set_linvel(speed, true));
    kill(&mut run, &reference);
    let ragdoll = &run.area.ragdoll;
    assert!(ragdoll.active(), "the ragdoll spawns");
    let r = &reference.clothing.ragdoll;
    let joints = run
        .physics
        .rapier()
        .impulse_joints
        .iter()
        .filter(|(_, j)| {
            ragdoll.bodies().contains(&j.body1()) || ragdoll.bodies().contains(&j.body2())
        })
        .collect::<Vec<_>>();
    assert_eq!(joints.len(), r.joints.len(), "one joint per record joint");
    assert!(
        joints.iter().all(|(_, j)| !j.data.limit_axes.is_empty()),
        "each joint has limits"
    );
    // Head follow and fade.
    let head = |run: &GarageRuntime| {
        run.area
            .ragdoll
            .head_pose(&run.physics)
            .unwrap()
            .translation
    };
    let th = |run: &GarageRuntime| {
        Vector::from_array(run.area.player.view(&run.physics).head_pose.position)
    };
    seconds(&mut run, &reference, 7.);
    let needs = run.needs_output();
    assert!(
        (th(&run) - head(&run)).length() < 0.05,
        "the camera stays in the head"
    );
    let progress = needs.death_seconds as f32 / 14.;
    assert!(
        (needs.look_scale - 0.8 * (1. - progress)).abs() < 1e-3,
        "deathSens fades"
    );
    assert!(!needs.menu_due);
    seconds(&mut run, &reference, 7.5);
    let needs = run.needs_output();
    assert!(
        needs.menu_due && needs.look_scale == 0.,
        "the menu opens at 14 s"
    );
    let loaded = load(&reference, &saved);
    assert!(
        !loaded.area.ragdoll.active(),
        "a loaded save has no ragdoll"
    );
    assert!(!loaded.needs_output().dead);
}

/// Source `SpawnRagdol`: `ragdolRBs[i].velocity = GetComponentInParent<Rigidbody>().velocity`.
/// Seated, the parent body is the car.
#[test]
#[ignore = "requires the installed source export"]
fn tp_pb9_the_ragdoll_carries_the_speed_of_the_car() {
    let (reference, mut run) = load_garage();
    settle(&mut run, &reference);
    act(&mut run, &reference, r#"{"action":"seat","seat":"driver"}"#);
    seconds(&mut run, &reference, 0.5);
    let car = run.area.player.seat_body().expect("a seat body");
    let speed = Vector::new(0., 3., 0.);
    run.physics.edit_body(car, |b| b.set_linvel(speed, true));
    run.survival
        .damage_instant(&reference.survival, reference.survival.maximum.hp);
    frame(&mut run, &reference);
    let bodies = run.area.ragdoll.bodies();
    assert!(!bodies.is_empty());
    let car_speed = run.physics.rapier().bodies[car].linvel();
    for b in bodies {
        let v = run.physics.rapier().bodies[*b].linvel();
        assert!(
            (v - car_speed).length() < 0.5,
            "the ragdoll takes the car speed {car_speed:?}: {v:?}"
        );
    }
}

/// PB6. Source `pushscript` grip: F on a push grip of a car holds it, and W pushes the car.
#[test]
#[ignore = "requires the installed source export"]
fn tp_pb6_f_on_a_push_grip_pushes_the_car_on_foot() {
    use crate::simulation::keys;
    let (reference, mut run) = super::creatures::outdoors();
    let key = run.vehicle.clone().unwrap();
    let body = run.physics.chassis_handle(&key);
    let car = |run: &GarageRuntime| *run.physics.rapier().bodies[body].position();
    // The start car push grip, from its world prototype.
    let name = run
        .area
        .items
        .resolve_key(&key)
        .and_then(|id| run.area.items.name(id))
        .unwrap()
        .to_string();
    let grip = reference
        .world
        .prototypes
        .iter()
        .find(|p| p.nodes.iter().any(|n| n.parent.is_none() && n.name == name))
        .and_then(|p| p.pushes.iter().find_map(|push| push.grip.as_ref()))
        .expect("the start car has a push grip");
    let local = Vector::from_array(grip.local_pose.translation);
    let mut found = false;
    // Stand beside the grip, out from the car side, at a few distances.
    'search: for side in [1.0f32, 0.8, 1.2, 0.6] {
        let pose = car(&run);
        let stand = pose * Vector::new(local.x.signum() * side, 0., local.z);
        stand_near(&mut run, &reference, stand.to_array(), 0.);
        run.area
            .player
            .look_at((car(&run) * local).to_array(), &run.physics);
        frame(&mut run, &reference);
        // F at a push target: the grip holds only on a push grip.
        if run.aim.push_target.is_some() {
            for i in 0..3 {
                let input = FrameInput {
                    held: keys::GRAB,
                    pressed: if i == 0 { keys::GRAB } else { 0 },
                    ..FrameInput::default()
                };
                super::weapons::input(&mut run, &reference, input);
            }
            if run.area.interaction.push_gripping() {
                found = true;
                break 'search;
            }
            super::weapons::input(
                &mut run,
                &reference,
                FrameInput {
                    released: keys::GRAB,
                    ..FrameInput::default()
                },
            );
        }
    }
    assert!(found, "the aim takes a push grip of the car");
    // Source: a parked car holds on its handbrake. Release it, as the driver would.
    let vehicle = run.vehicles.get_mut(&key).unwrap();
    vehicle.handbrake = false;
    let start = car(&run).translation;
    let push = FrameInput {
        held: keys::GRAB | keys::FORWARD,
        ..FrameInput::default()
    };
    let mut gripping = 0;
    for _ in 0..(4. / FIXED_DT) as usize {
        super::weapons::input(&mut run, &reference, push);
        gripping += usize::from(run.area.interaction.push_gripping());
    }
    let moved = (car(&run).translation - start).length();
    eprintln!("gripping {gripping} frames, the car moves {moved} m");
    assert!(gripping > 100, "F holds the grip");
    assert!(moved > 0.5, "W pushes the car: {moved} m");
}

/// E held on a door or handle with mouse drag, in each direction until it moves.
/// Source `fpscontroller.UseUsable`. Returns the change of its open value (0 to 1).
fn drag_open(run: &mut GarageRuntime, reference: &ReferenceManifest, item: ItemId) -> f32 {
    use crate::simulation::keys;
    let value = |run: &GarageRuntime| run.area.interaction.mechanisms.value(item, false).unwrap();
    let before = value(run);
    let key = |held, pressed, released, mouse| FrameInput {
        held,
        pressed,
        released,
        mouse,
        ..FrameInput::default()
    };
    input(run, reference, key(keys::USE, keys::USE, 0, [0.; 2]));
    for mouse in [[6., 0.], [-6., 0.], [0., -6.], [0., 6.]] {
        for _ in 0..30 {
            input(run, reference, key(keys::USE, 0, 0, mouse));
        }
        if (value(run) - before).abs() > 0.05 {
            break;
        }
    }
    input(run, reference, key(0, 0, keys::USE, [0.; 2]));
    (value(run) - before).abs()
}

/// PB6. Source `fpscontroller` seat push: seated, held F at a surface of another root pushes
/// the seat body back from the camera with `seatPushForce`. The closed doors are the own car,
/// so the player first opens the driver door from the seat with E.
#[test]
#[ignore = "requires the installed source export"]
fn tp_pb6_f_from_a_seat_pushes_the_car_off_the_ground() {
    use crate::interaction::feedback::ActionMask;
    use crate::simulation::keys;
    let (reference, mut run) = super::creatures::outdoors();
    act(&mut run, &reference, r#"{"action":"seat","seat":"driver"}"#);
    seconds(&mut run, &reference, 0.5);
    assert!(run.area.player.seated());
    assert_eq!(
        run.area.player.seat_push_modifier(),
        Some(1.),
        "the Lada driver seat pushes with the full force"
    );
    let key = run.vehicle.clone().unwrap();
    run.vehicles.get_mut(&key).unwrap().handbrake = false;
    let car = run.area.player.seat_body().unwrap();
    // The driver door is on the left: E drags it open from the seat.
    look(&mut run, -90., -30.);
    frame(&mut run, &reference);
    let door = run.aim.use_target.map(|t| t.item).expect("the driver door");
    assert!(run.area.interaction.mechanisms.contains(door));
    let opened = drag_open(&mut run, &reference, door);
    assert!(
        opened > 0.1,
        "E opens the driver door from the seat: {opened}"
    );
    seconds(&mut run, &reference, 0.5);
    // Look out of the open door down at the ground.
    let push_shown = |run: &GarageRuntime| {
        let actions = run.area.interaction.available_actions(
            &run.aim,
            &run.area.player,
            &run.area.items,
            &run.physics,
            run.needs_output(),
        );
        actions.actions.0 & ActionMask::PUSH != 0
    };
    let mut aim = None;
    'search: for yaw in [-90.0f32, -75., -105., -60., -120.] {
        for pitch in [-60.0f32, -50., -70., -80., -40.] {
            look(&mut run, yaw, pitch);
            frame(&mut run, &reference);
            if push_shown(&run) {
                aim = Some((yaw, pitch));
                break 'search;
            }
        }
    }
    let (yaw, pitch) = aim.expect("the HUD shows push at the ground out of the open door");
    eprintln!("push aim yaw {yaw} pitch {pitch}");
    // The parked car stays without input.
    let start = run.physics.rapier().bodies[car].translation();
    for _ in 0..(1. / FIXED_DT) as usize {
        look(&mut run, yaw, pitch);
        input(&mut run, &reference, FrameInput::default());
    }
    let drift = (run.physics.rapier().bodies[car].translation() - start).length();
    assert!(drift < 0.01, "the car stays without F: {drift} m");
    let start = run.physics.rapier().bodies[car].translation();
    for _ in 0..(3. / FIXED_DT) as usize {
        look(&mut run, yaw, pitch);
        input(
            &mut run,
            &reference,
            FrameInput {
                held: keys::GRAB,
                ..FrameInput::default()
            },
        );
    }
    let moved = (run.physics.rapier().bodies[car].translation() - start).length();
    eprintln!("the car moves {moved} m");
    assert!(run.area.player.seated(), "the player stays seated");
    assert!(moved > 0.3, "F from the seat pushes the car: {moved} m");
}

/// PB6. Source `usablescript` doors and handles of the garage: the use ray takes each one, and
/// E with the mouse opens it. Car doors from outside are K3.
#[test]
#[ignore = "requires the installed source export"]
fn tp_pb6_e_opens_the_garage_doors_and_handles() {
    use rapier3d::parry::bounding_volume::Aabb;
    let (reference, mut run) = load_garage();
    settle(&mut run, &reference);
    let home = run.area.player.body_pose(&run.physics);
    let head = run.area.player.view(&run.physics).head_pose.position[1];
    let controls: Vec<ItemId> = reference
        .mechanisms
        .iter()
        .filter(|m| m.turn.is_none() && (m.rotation.is_some() || m.slide.is_some()))
        .map(|m| m.item)
        .collect();
    assert!(controls.len() >= 10, "the garage doors and the tap handle");
    let mut failed = Vec::new();
    for item in controls {
        // Aim points: the colliders of the item body nearest to the item pose, at head height
        // where the collider reaches it.
        let at = Vector::from_array(run.area.items.get(item).unwrap().pose.position);
        let rapier = run.physics.rapier();
        let mut boxes: Vec<_> = run
            .area
            .items
            .get(item)
            .and_then(|s| s.body)
            .map(|b| {
                rapier.bodies[b]
                    .colliders()
                    .iter()
                    .map(|c| rapier.colliders[*c].compute_aabb())
                    .collect()
            })
            .unwrap_or_default();
        boxes.sort_by(|a: &Aabb, b: &Aabb| {
            (a.center() - at)
                .length()
                .total_cmp(&(b.center() - at).length())
        });
        let floor = home.position[1];
        // A collider above the head stands on an upper floor at its bottom.
        let rise = boxes
            .first()
            .filter(|b| b.mins.y > head)
            .map_or(0., |b| b.mins.y - (head - 1.6));
        let mut points = vec![at];
        for aabb in boxes.iter().take(3) {
            let c = aabb.center();
            let y = (head + rise).clamp(aabb.mins.y, aabb.maxs.y);
            points.push(Vector::new(c.x, y, c.z));
        }
        // Stand on a ring around each point, on the home floor or at the floor of the item.
        let mut aimed = false;
        'search: for point in points {
            for y in [floor, floor + rise] {
                for radius in [0.7f32, 1.0, 0.4] {
                    for k in 0..12 {
                        let angle = k as f32 * std::f32::consts::TAU / 12.;
                        let mut pose = home;
                        pose.position = [
                            point.x + radius * angle.cos(),
                            y + 0.05,
                            point.z + radius * angle.sin(),
                        ];
                        run.area.player.recover(pose, &mut run.physics);
                        let hit = |run: &mut GarageRuntime| {
                            run.area.player.look_at(point.to_array(), &run.physics);
                            frame(run, &reference);
                            run.aim.use_target.is_some_and(|t| t.item == item)
                        };
                        if (0..3).any(|_| hit(&mut run)) {
                            // The player settles on the spot, then aims again.
                            seconds(&mut run, &reference, 0.5);
                            if (0..5).any(|_| hit(&mut run)) {
                                aimed = true;
                                break 'search;
                            }
                        }
                    }
                }
            }
        }
        let name = run.area.items.name(item).unwrap_or("").to_string();
        let opened = if aimed {
            drag_open(&mut run, &reference, item)
        } else {
            0.
        };
        eprintln!("{item:?} {name}: aimed {aimed}, opens {opened}");
        if opened <= 0.1 {
            failed.push((item, name, aimed));
        }
    }
    assert!(failed.is_empty(), "E does not open: {failed:?}");
}
