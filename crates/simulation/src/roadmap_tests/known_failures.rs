//! Headless scenario tests: fixed known failures (K rows).
use crate::boundary::SimInit;
use crate::session::Session;
use crate::starting_area::GarageRuntime;
use crate::world::stream::tests::kit::*;
use std::sync::Arc;

/// K5, S1: Load and Load last reload the page, then `Session::new` starts from the stored save, as
/// the source `SceneManager.LoadScene` loads the save at scene start. The save opens without an
/// error and its restore finishes. A save after a death opens the same way.
#[test]
#[ignore = "requires the installed source export"]
fn tp_k5_s1_a_stored_save_starts_the_session_and_its_restore_finishes() {
    let (reference, mut run) = load_garage();
    settle(&mut run, &reference);
    seconds(&mut run, &reference, 2.);
    let saved = save(&run, &reference);
    let stamp = roadside_stamp(&reference);
    let reference = Arc::new(reference);
    let init = SimInit {
        mode: Default::default(),
        options: Default::default(),
        draw_distance: reference.world.draw_distance.default,
        manifest_sha256: stamp.manifest_sha256.clone(),
        save: serde_json::to_vec(&saved).unwrap(),
    };
    let session = Session::new(reference.clone(), init).unwrap();
    assert_eq!(session.load_error(), None, "the stored save opens");
    drop(session);
    // The restore the session frames run.
    let mut loaded = load(&reference, &saved);
    seconds(&mut loaded, &reference, 1.);
    assert!(loaded.world_supported(), "the loaded world is ready");
    // After a death, the pause menu Load opens the same stored save.
    run.survival
        .damage_instant(&reference.survival, reference.survival.maximum.hp);
    frame(&mut run, &reference);
    assert!(run.needs_output().dead);
    let after = load(&reference, &saved);
    assert!(!after.needs_output().dead && after.world_supported());
}

/// K3. Source `fpscontroller` use ray on a car door handle from outside. The aim takes the door
/// mechanism, the HUD shows the door action and E with the mouse opens the door.
#[test]
#[ignore = "requires the installed source export"]
fn tp_k3_e_on_the_outside_door_handle_opens_the_door() {
    use crate::interaction::feedback::ActionMask;
    use crate::simulation::{FIXED_DT, FrameInput, keys};
    use rapier3d::math::Vector;
    use rapier3d::parry::bounding_volume::BoundingVolume;
    let (reference, mut run) = load_garage();
    settle(&mut run, &reference);
    let key = run.vehicle.clone().unwrap();
    let car = Vector::from_array(run.physics.snapshot(&key).pose.position);
    let mechanisms = &run.area.interaction.mechanisms;
    let doors: Vec<_> = run
        .area
        .items
        .iter()
        .filter(|s| {
            mechanisms.contains(s.id)
                && mechanisms.turn_state(s.id).is_none()
                && (Vector::from_array(s.pose.position) - car).length() < 3.
        })
        .map(|s| (s.id, Vector::from_array(s.pose.position)))
        .collect();
    let car_name = run
        .area
        .items
        .resolve_key(&key)
        .and_then(|id| run.area.items.name(id))
        .unwrap_or("")
        .to_string();
    // Side doors of the start car: mechanism items of the car more than 0.5 m to the side.
    let rotation = rapier3d::math::Rotation::from_array(run.physics.snapshot(&key).pose.rotation);
    let sides: Vec<_> = doors
        .iter()
        .filter(|(id, p)| {
            run.area.items.name(*id) == Some(car_name.as_str())
                && (rotation.inverse() * (p - car)).x.abs() > 0.5
        })
        .copied()
        .collect();
    let mut seen = Vec::new();
    let mut found = None;
    for (door, at) in sides {
        // Stand 1 m outside the door, as `stand_near` in the Starting area tests does.
        let mut out = at - car;
        out.y = 0.;
        let stand = at + out.normalize();
        let mut pose = run.area.player.body_pose(&run.physics);
        pose.position = [stand.x, pose.position[1], stand.z];
        run.area.player.recover(pose, &mut run.physics);
        seconds(&mut run, &reference, 0.5);
        // Aim at the middle of the door colliders.
        let body = run.area.items.get(door).and_then(|s| s.body);
        let rapier = run.physics.rapier();
        let point = body
            .and_then(|b| {
                rapier.bodies[b]
                    .colliders()
                    .iter()
                    .map(|c| rapier.colliders[*c].compute_aabb())
                    .reduce(|a, b| a.merged(&b))
            })
            .map_or(at, |aabb| aabb.center());
        let mut target = None;
        for _ in 0..20 {
            run.area.player.look_at(point.to_array(), &run.physics);
            frame(&mut run, &reference);
            target = run.aim.use_target.map(|t| t.item);
            if target == Some(door) {
                break;
            }
        }
        seen.push(target.and_then(|t| run.area.items.name(t).map(str::to_string)));
        if target == Some(door) {
            found = Some(door);
            break;
        }
    }
    let door = found.unwrap_or_else(|| {
        panic!("the use ray takes no side door of {car_name} from outside. It takes: {seen:?}")
    });
    let before = run.area.interaction.mechanisms.rotation_degrees(door);
    let press = |run: &mut GarageRuntime, input: FrameInput| {
        let view = run.area.player.view(&run.physics).head_pose;
        let fixed = run
            .advance_inventory_frame(&reference, input, FIXED_DT, view)
            .fixed_input;
        frame_with(run, &reference, fixed);
    };
    press(&mut run, FrameInput::default());
    let actions = run.area.interaction.available_actions(
        &run.aim,
        &run.area.player,
        &run.area.items,
        &run.physics,
        run.needs_output(),
    );
    assert!(
        actions.actions.0 & ActionMask::DOOR != 0,
        "the HUD shows the door action"
    );
    press(
        &mut run,
        FrameInput {
            held: keys::USE,
            pressed: keys::USE,
            ..Default::default()
        },
    );
    // Source `fpscontroller.UseUsable`. A held E drags the door with the mouse. The closed
    // door turns one way only, so try both.
    for x in [6., -6.] {
        for _ in 0..30 {
            press(
                &mut run,
                FrameInput {
                    held: keys::USE,
                    mouse: [x, 0.],
                    ..Default::default()
                },
            );
        }
        if run.area.interaction.mechanisms.rotation_degrees(door) != before {
            break;
        }
    }
    press(
        &mut run,
        FrameInput {
            released: keys::USE,
            ..Default::default()
        },
    );
    seconds(&mut run, &reference, 1.);
    assert_ne!(
        run.area.interaction.mechanisms.rotation_degrees(door),
        before,
        "E opens the door"
    );
}

/// K6. A world car hood is a part on a hinge. Open, its colliders turn with it, so the use ray
/// takes the engine filler caps from the side, as source `fpscontroller` does.
#[test]
#[ignore = "requires the installed source export"]
fn tp_k6_an_open_hood_leaves_the_engine_caps_in_reach() {
    use crate::reference::ItemId;
    use rapier3d::math::{Rotation, Vector};
    let (reference, mut run) = load_garage();
    settle(&mut run, &reference);
    let known: Vec<_> = run.area.interaction.valves().map(|(cap, _)| cap).collect();
    act(
        &mut run,
        &reference,
        r#"{"action":"spawn","name":"Car04Full","forward":8.0}"#,
    );
    seconds(&mut run, &reference, 4.);
    let aim = |run: &GarageRuntime, eye: Vector, at: Vector| {
        let view = crate::vehicle::VehiclePose {
            position: eye.to_array(),
            rotation: Rotation::from_rotation_arc(-Vector::Z, (at - eye).normalize()).to_array(),
        };
        let aim = run
            .area
            .interaction
            .resolve(&run.physics, view, &run.area.items);
        aim.use_target.map(|t| t.item)
    };
    // The engine filler caps of the new car. The closed hood covers them from above.
    let caps: Vec<_> = run.area.interaction.valves().map(|(cap, _)| cap).collect();
    let mut filler = None;
    for (handle, collider) in run.physics.rapier().colliders.iter() {
        let Some(cap) = run.area.interaction.use_owner(handle) else {
            continue;
        };
        if !caps.contains(&cap) || known.contains(&cap) {
            continue;
        }
        let at = collider.compute_aabb().center();
        if let Some(hood) = aim(&run, at + Vector::Y, at)
            .filter(|h| *h != cap && run.area.interaction.mechanisms.contains(*h))
            && filler.is_none_or(|(_, _, y): (ItemId, ItemId, Vector)| at.y > y.y)
        {
            filler = Some((cap, hood, at));
        }
    }
    let (cap, hood, at) = filler.expect("the closed hood covers an engine cap");
    for _ in 0..60 {
        run.area.interaction.mechanisms.push(hood, 20., 1. / 60.);
        frame(&mut run, &reference);
    }
    seconds(&mut run, &reference, 1.);
    let reach = [(0.9, 0.), (-0.9, 0.), (0., 0.9), (0., -0.9)]
        .into_iter()
        .filter(|&(x, z)| aim(&run, at + Vector::new(x, 0.75, z), at) == Some(cap))
        .count();
    assert!(
        reach >= 2,
        "the use ray takes the cap from {reach} of 4 sides"
    );
}

/// K7. A seated player changes seat in the same car. The use ray from the driver head takes
/// another seat of the car, and the seat change keeps the player in the car.
#[test]
#[ignore = "requires the installed source export"]
fn tp_k7_a_seated_player_changes_seat_in_the_same_car() {
    use rapier3d::math::{Rotation, Vector};
    let (reference, mut run) = load_garage();
    settle(&mut run, &reference);
    act(&mut run, &reference, r#"{"action":"seat","seat":"driver"}"#);
    seconds(&mut run, &reference, 0.5);
    let car = run.area.player.seat_vehicle().cloned();
    assert!(
        car.is_some() && run.area.player.driving(),
        "the player drives"
    );
    let eye = Vector::from_array(run.area.player.view(&run.physics).camera_pose.position);
    let interaction = &run.area.interaction;
    let switchable = |handle| {
        interaction
            .controls
            .seat(handle)
            .is_some_and(|seat| run.area.player.can_switch_to(seat))
    };
    let reach = run
        .physics
        .rapier()
        .colliders
        .iter()
        .filter(|(handle, _)| switchable(*handle))
        .filter(|(_, collider)| {
            let at = collider.compute_aabb().center();
            let view = crate::vehicle::VehiclePose {
                position: eye.to_array(),
                rotation: Rotation::from_rotation_arc(-Vector::Z, (at - eye).normalize())
                    .to_array(),
            };
            let aim = interaction.resolve(&run.physics, view, &run.area.items);
            aim.use_target.is_some_and(|t| switchable(t.collider))
        })
        .count();
    assert!(reach >= 1, "the use ray takes another seat of the car");
    act(
        &mut run,
        &reference,
        r#"{"action":"seat","seat":"passenger"}"#,
    );
    seconds(&mut run, &reference, 0.5);
    let player = &run.area.player;
    assert!(
        player.seated() && !player.driving(),
        "the player sits as a passenger"
    );
    assert_eq!(player.seat_vehicle().cloned(), car, "in the same car");
}

/// K8. Renderer loads can hold all staging while a held item changes every frame, as the spray
/// bottle count does. The changed destination selection waits for staging room and the game
/// does not stop with "Changed destination selection exceeds staging admission".
#[test]
#[ignore = "requires the installed source export"]
fn tp_k8_full_renderer_staging_does_not_stop_a_changing_held_item() {
    use crate::world::stream::STAGING;
    let (reference, mut run) = load_garage();
    settle(&mut run, &reference);
    let item = find(&run, "OilCan");
    hold_renderer_staging(&mut run.stream, STAGING);
    pick_up(&mut run, &reference, item);
    // `frame` fails the test on a stream failure.
    for step in 0..60 {
        look(&mut run, (step % 20) as f32 * 3., 0.);
        frame(&mut run, &reference);
    }
    hold_renderer_staging(&mut run.stream, 0);
    seconds(&mut run, &reference, 1.);
    assert!(run.world_supported(), "the world stays ready");
}

/// K9. Source `fpscontroller` `Bbinoculars`. LMB with the binoculars in the hand looks through
/// them. The frame input route keeps LMB for the fixed step, and the frame state has the
/// binocular camera.
#[test]
#[ignore = "requires the installed source export"]
fn tp_k9_lmb_looks_through_the_binoculars_in_the_hand() {
    use crate::boundary::StateBlock;
    use crate::simulation::{FIXED_DT, FrameInput, keys};
    let (reference, mut run) = load_garage();
    settle(&mut run, &reference);
    let hand = r#"{"action":"hand","name":"binoculars"}"#;
    for _ in 0..30 {
        if act(&mut run, &reference, hand)["pending"] != true {
            break;
        }
        seconds(&mut run, &reference, 1.);
    }
    let held = |run: &GarageRuntime| {
        run.area
            .interaction
            .equipment
            .item()
            .or(run.area.interaction.held())
    };
    assert!(held(&run).is_some(), "the binoculars are in the hand");
    let press = |run: &mut GarageRuntime, input: FrameInput| {
        let view = run.area.player.view(&run.physics).head_pose;
        let fixed = run
            .advance_inventory_frame(&reference, input, FIXED_DT, view)
            .fixed_input;
        frame_with(run, &reference, fixed);
    };
    let lmb = FrameInput {
        held: keys::PRIMARY,
        pressed: keys::PRIMARY,
        ..Default::default()
    };
    press(&mut run, lmb);
    press(
        &mut run,
        FrameInput {
            held: keys::PRIMARY,
            ..Default::default()
        },
    );
    let used = run.area.interaction.binoculars.active();
    assert_eq!(
        used.map(|b| Some(b.item)),
        Some(held(&run)),
        "LMB uses the binoculars"
    );
    // One wheel notch focuses by `scroll * FMouseSensZ * dt * focusDistanceSens * WHEEL_GAIN`.
    let lens = &reference.small_items.binoculars;
    let before = used.unwrap().focus;
    let notch = if before < lens.max_focus { 0.1 } else { -0.1 };
    press(
        &mut run,
        FrameInput {
            held: keys::PRIMARY,
            wheel: notch,
            ..Default::default()
        },
    );
    let scroll = crate::reference::PlayerSettings::defaults(&reference).player_scroll();
    let expected = (before
        + notch
            * scroll
            * FIXED_DT
            * lens.focus_sensitivity
            * crate::interaction::binoculars::WHEEL_GAIN)
        .clamp(lens.min_focus, lens.max_focus);
    let after = run.area.interaction.binoculars.active().unwrap().focus;
    assert!(
        (after - expected).abs() < 1e-4,
        "the wheel focuses the binoculars: {before} -> {after}, expected {expected}"
    );
    let mut frame = crate::boundary::SimFrame::default();
    run.fill_frame(&reference, 1., &mut frame);
    let flags = frame.state.flags;
    assert!(
        flags & StateBlock::BINOCULARS != 0,
        "the view is the binocular camera"
    );
    press(
        &mut run,
        FrameInput {
            released: keys::PRIMARY,
            ..Default::default()
        },
    );
    assert!(
        run.area.interaction.binoculars.active().is_none(),
        "release ends the use"
    );
}

/// K11. A world car wheel comes off by hand. The use ray from beside a mounted wheel takes its
/// hubcap or the wheel, not the push of the car body.
#[test]
#[ignore = "requires the installed source export"]
fn tp_k11_the_hand_takes_a_mounted_wheel_or_its_hubcap() {
    use crate::interaction::items::ItemLocation;
    use rapier3d::math::{Rotation, Vector};
    let (reference, mut run) = load_garage();
    settle(&mut run, &reference);
    act(
        &mut run,
        &reference,
        r#"{"action":"spawn","name":"Car04Full","forward":8.0}"#,
    );
    seconds(&mut run, &reference, 4.);
    let sensors: Vec<_> = run.wheels.sensor_rims().collect();
    let mut wheels = 0;
    for (sensor, rim) in sensors {
        let rim_state = run.area.items.get(rim).unwrap();
        if !matches!(rim_state.location, ItemLocation::WheelSlot(_)) {
            continue;
        }
        let chassis = run.physics.rapier().bodies[rim_state.body.unwrap()].position();
        let at = run.physics.rapier().colliders[sensor]
            .compute_aabb()
            .center();
        // From 1 m beside the wheel, on its side of the car.
        let side = chassis.rotation * Vector::X;
        let eye = at + side * (at - chassis.translation).dot(side).signum() + Vector::Y * 0.3;
        let view = crate::vehicle::VehiclePose {
            position: eye.to_array(),
            rotation: Rotation::from_rotation_arc(-Vector::Z, (at - eye).normalize()).to_array(),
        };
        let aim = run
            .area
            .interaction
            .resolve(&run.physics, view, &run.area.items);
        let target = aim.grab_target.map(|t| t.item);
        assert!(
            target == Some(rim) || target.is_some_and(|t| run.area.interaction.parts.contains(t)),
            "the hand takes rim {rim:?} or its hubcap, not {target:?} (push {:?})",
            aim.push_target.map(|t| t.item)
        );
        wheels += 1;
    }
    assert!(wheels >= 4, "the car has {wheels} mounted wheels");
}

/// K10. In a seated world car (the van Car09full), the player looks at a cabin control and uses
/// it without a car key: source `fpscontroller` turns `turnable` usables (InteractHandbrake,
/// InteractLight, the cabin light) with E or left click and tunes the radio `tuneAble` knobs.
#[test]
#[ignore = "requires the installed source export"]
fn tp_k10_a_seated_player_uses_the_van_cabin_controls() {
    use crate::interaction::feedback::ActionMask;
    use crate::simulation::{FIXED_DT, FrameInput, keys};
    use rapier3d::parry::bounding_volume::BoundingVolume;
    let (reference, mut run) = load_garage();
    settle(&mut run, &reference);
    act(
        &mut run,
        &reference,
        r#"{"action":"spawn","name":"Car09full","forward":8.0}"#,
    );
    seconds(&mut run, &reference, 4.);
    act(
        &mut run,
        &reference,
        r#"{"action":"seat","car":"Car09","seat":"driver"}"#,
    );
    seconds(&mut run, &reference, 1.);
    let key = run.area.player.seat_vehicle().cloned().expect("seated");
    let car = &run.vehicles[&key];
    let (handbrake, lights) = (
        car.handbrake_switch.expect("the van has InteractHandbrake"),
        car.light_switch.expect("the van has InteractLight"),
    );
    let press = |run: &mut GarageRuntime, input: FrameInput| {
        let view = run.area.player.view(&run.physics).head_pose;
        let fixed = run
            .advance_inventory_frame(&reference, input, FIXED_DT, view)
            .fixed_input;
        frame_with(run, &reference, fixed);
    };
    let look = |run: &mut GarageRuntime, item| {
        let body = run.area.items.get(item).and_then(|s| s.body).unwrap();
        let rapier = run.physics.rapier();
        let point = rapier
            .colliders
            .iter()
            .filter(|(h, c)| {
                c.parent() == Some(body) && run.area.interaction.use_owner(*h) == Some(item)
            })
            .map(|(_, c)| c.compute_aabb())
            .reduce(|a, b| a.merged(&b))
            .expect("the control has a collider")
            .center();
        run.area.player.look_at(point.to_array(), &run.physics);
        press(run, FrameInput::default());
        run.aim.use_target.map(|t| t.item)
    };
    let before = run.vehicles[&key].handbrake;
    assert_eq!(
        look(&mut run, handbrake),
        Some(handbrake),
        "the use ray takes the handbrake"
    );
    let actions = run.area.interaction.available_actions(
        &run.aim,
        &run.area.player,
        &run.area.items,
        &run.physics,
        run.needs_output(),
    );
    assert!(
        actions.actions.0 & ActionMask::SWITCH != 0,
        "the HUD shows the switch"
    );
    press(
        &mut run,
        FrameInput {
            held: keys::USE,
            pressed: keys::USE,
            ..Default::default()
        },
    );
    press(
        &mut run,
        FrameInput {
            released: keys::USE,
            ..Default::default()
        },
    );
    assert_ne!(
        run.vehicles[&key].handbrake, before,
        "E toggles the handbrake"
    );
    let mode = run.vehicles[&key].requested_headlights;
    assert_eq!(
        look(&mut run, lights),
        Some(lights),
        "the use ray takes the light switch"
    );
    press(
        &mut run,
        FrameInput {
            held: keys::PRIMARY,
            pressed: keys::PRIMARY,
            ..Default::default()
        },
    );
    press(
        &mut run,
        FrameInput {
            released: keys::PRIMARY,
            ..Default::default()
        },
    );
    assert_ne!(
        run.vehicles[&key].requested_headlights, mode,
        "left click turns the lights"
    );
    // The radio knobs: the use ray takes each one from the driver head.
    let eye =
        rapier3d::math::Vector::from_array(run.area.player.view(&run.physics).camera_pose.position);
    let interaction = &run.area.interaction;
    let knobs = run
        .physics
        .rapier()
        .colliders
        .iter()
        .filter(|(h, c)| {
            let at = c.compute_aabb().center();
            let target = crate::interaction::AimTarget {
                collider: *h,
                item: crate::reference::ItemId(0),
                distance: 0.,
                point: at.to_array(),
                slot: None,
            };
            (at - eye).length() < 1.5 && interaction.dial_knob(&target).is_some()
        })
        .map(|(h, c)| (h, c.compute_aabb().center()))
        .collect::<Vec<_>>();
    assert!(!knobs.is_empty(), "the van radio has knobs");
    for (handle, at) in knobs {
        let view = crate::vehicle::VehiclePose {
            position: eye.to_array(),
            rotation: rapier3d::math::Rotation::from_rotation_arc(
                -rapier3d::math::Vector::Z,
                (at - eye).normalize(),
            )
            .to_array(),
        };
        let aim = interaction.resolve(&run.physics, view, &run.area.items);
        assert_eq!(
            aim.use_target.map(|t| t.collider),
            Some(handle),
            "the use ray takes a radio knob"
        );
    }
}

/// K12. A paint spray in the hand owns LMB: source `fpscontroller` sets `noLMBUse` for any item in
/// the hand, so LMB paints and does not grab or open the aimed car part.
#[test]
#[ignore = "requires the installed source export"]
fn tp_k12_a_paint_spray_in_the_hand_owns_lmb() {
    use crate::interaction::claims::LmbOwner;
    let (reference, mut run) = load_garage();
    settle(&mut run, &reference);
    let spray = reference.conditions.sprays[0].item;
    let area = &mut run.area;
    let point = area.items.get(spray).unwrap().pose.position;
    area.interaction
        .pickup_item(
            spray,
            point,
            &area.player,
            &mut area.items,
            &mut run.physics,
        )
        .unwrap();
    area.sync_inventory();
    assert!(
        area.equip_carried(&mut run.physics).unwrap(),
        "the spray is in the hand"
    );
    assert_eq!(area.interaction.lmb_owner(), Some(LmbOwner::CleaningTool));
}

/// K13. The start car drives away from the start along the road at 25 m/s. Each 100 m the
/// road plan rereads about 256 candidates. The step never holds for 1 s, so the car does not
/// stop in place while the world loads.
#[test]
#[ignore = "requires the installed source export"]
fn tp_k13_driving_away_from_the_start_never_stops_the_car() {
    use crate::simulation::FIXED_DT;
    const SPEED: f64 = 25.;
    let (reference, mut run) = load_garage();
    settle(&mut run, &reference);
    let key = run.vehicle.clone().unwrap();
    let car = run.physics.snapshot(&key).pose;
    stand_near(&mut run, &reference, car.position, 2.);
    act(&mut run, &reference, r#"{"action":"seat","seat":"driver"}"#);
    let start = run.stream.origin.absolute(car.position)[2] - 40.;
    let (mut travelled, mut held) = (0., 0);
    while travelled < 2000. {
        if run.world_supported() {
            held = 0;
            travelled += SPEED * f64::from(FIXED_DT);
            let point = run
                .stream
                .road_surface_points()
                .find(|p| p[2] <= start - travelled)
                .expect("the road reaches the car");
            let mut position = run.stream.origin.local(point);
            position[1] += 1.;
            run.physics.reset_preserving_wheels(
                &key,
                crate::vehicle::VehiclePose {
                    position,
                    rotation: car.rotation,
                },
            );
            run.physics.refresh_queries();
        } else {
            held += 1;
            let absolute = run
                .stream
                .origin
                .absolute(run.physics.snapshot(&key).pose.position);
            let bounds = crate::world::WorldBounds {
                min: absolute.map(|v| v - 8.),
                max: absolute.map(|v| v + 8.),
            };
            assert!(
                held < 50,
                "the step holds at {travelled:.0} m: {:?}",
                run.stream.support_gap(bounds)
            );
        }
        frame(&mut run, &reference);
    }
}

/// K14. The start car rolls forward. Its dashboard is a mounted gauge part with content poses.
/// The speed needle turns, and the last pose row of each needle and digit is its gauge rotation.
#[test]
#[ignore = "requires the installed source export"]
fn tp_k14_the_start_car_gauges_turn_over_the_part_poses() {
    use crate::boundary::NodePoseRow;
    let (reference, mut run) = load_garage();
    settle(&mut run, &reference);
    let key = run.vehicle.clone().unwrap();
    let car = run.physics.snapshot(&key).pose;
    stand_near(&mut run, &reference, car.position, 2.);
    act(&mut run, &reference, r#"{"action":"seat","seat":"driver"}"#);
    let handle = run.physics.chassis_handle(&key);
    let mut out = crate::boundary::SimFrame::default();
    let mut posed = std::collections::HashMap::new();
    for _ in 0..100 {
        run.physics.edit_body(handle, |b| {
            let forward = b.rotation() * rapier3d::math::Vector::Z;
            b.set_linvel(forward * 10., true);
        });
        frame(&mut run, &reference);
        run.fill_frame(&reference, 1., &mut out);
        let gauge = run.powertrain.gauges().next().unwrap();
        let slot = run.structure_views.slots[&gauge.parent];
        for row in out.structures.iter().filter(|r| r.slot == slot) {
            for pose in &out.structure_poses[row.poses[0] as usize..row.poses[1] as usize] {
                posed.insert(pose.node, pose.kind);
            }
        }
    }
    let gauge = run.powertrain.gauges().next().unwrap();
    let speed = gauge
        .record
        .needles
        .iter()
        .position(|n| n.signal == "speed")
        .unwrap();
    let rest = crate::simulation::instruments::needle_angle(&gauge.record.needles[speed], 0.);
    assert!(
        gauge.state.needle_angles[speed] != rest,
        "the speed needle turns"
    );
    let digits = gauge
        .record
        .counters
        .iter()
        .flat_map(|c| &c.digits)
        .map(|d| d.node);
    for node in gauge.record.needles.iter().map(|n| n.node).chain(digits) {
        assert_eq!(
            posed.get(&node.0),
            Some(&NodePoseRow::ROTATION),
            "node {}",
            node.0
        );
    }
}

/// The start car drives. Each frame between two steps shows its mounted wheels at their rest
/// offset on the car body, so the wheels do not trail or lead the body.
#[test]
#[ignore = "requires the installed source export"]
fn the_start_car_wheels_stay_on_the_driving_body() {
    use crate::boundary::NodePoseRow;
    use crate::world::ObjectKey;
    use rapier3d::math::{Pose, Rotation, Vector};
    let (reference, mut run) = load_garage();
    settle(&mut run, &reference);
    let key = run.vehicle.clone().unwrap();
    let ObjectKey::Generated(car_id) = key.clone() else {
        panic!("the start car is a world car");
    };
    let car = run.physics.snapshot(&key).pose;
    stand_near(&mut run, &reference, car.position, 2.);
    act(&mut run, &reference, r#"{"action":"seat","seat":"driver"}"#);
    let handle = run.physics.chassis_handle(&key);
    let mut out = crate::boundary::SimFrame::default();
    for _ in 0..50 {
        run.physics.edit_body(handle, |b| {
            let forward = b.rotation() * Vector::Z;
            b.set_linvel(forward * 20., true);
        });
        frame(&mut run, &reference);
    }
    run.fill_frame(&reference, 0.5, &mut out);
    let body_row = |slot: u32| {
        let row = out.structures.iter().find(|r| r.slot == slot)?;
        let pose = out.structure_poses[row.poses[0] as usize..row.poses[1] as usize]
            .iter()
            .find(|p| p.kind == NodePoseRow::BODY && p.node == 0)?;
        Some(Pose::from_parts(
            Vector::from_array(pose.translation),
            Rotation::from_array(pose.rotation),
        ))
    };
    let shown_car = body_row(run.structure_views.slots[&car_id]).expect("the car is shown");
    let chassis = *run.physics.rapier().bodies[handle].position();
    let mut wheels = 0;
    for item in run.area.items.iter().filter(|i| i.body == Some(handle)) {
        let ObjectKey::Generated(id) = run.area.items.object_key(item.id) else {
            continue;
        };
        let Some(&slot) = run.structure_views.slots.get(&id) else {
            continue;
        };
        let structure = run
            .stream
            .frame()
            .structures
            .iter()
            .find(|s| s.placement.id == id)
            .unwrap();
        if structure.bodies.contains(&handle) {
            continue;
        }
        let own = *run.physics.rapier().bodies[structure.bodies[0]].position();
        let rest = chassis.inverse() * own;
        let shown = body_row(slot).expect("a mounted part is shown with its car");
        let error = (shown_car.inverse() * shown).translation - rest.translation;
        assert!(
            error.length() < 1e-3,
            "part {id:?} is {error} m off its car"
        );
        wheels += 1;
    }
    assert!(wheels >= 4, "the start car has its wheels");
}
