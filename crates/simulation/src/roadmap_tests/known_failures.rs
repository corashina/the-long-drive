//! ROADMAP test pass: Known failures (K rows).
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
    let (session, _) = Session::new(reference.clone(), init).unwrap();
    assert_eq!(session.load_error(), None, "the stored save opens");
    drop(session);
    // The restore the session frames run.
    let mut loaded = load(&reference, &saved);
    seconds(&mut loaded, &reference, 1.);
    assert!(loaded.world_supported(), "the loaded world is ready");
    // After a death: the pause menu Load opens the same stored save.
    run.survival
        .damage_instant(&reference.survival, reference.survival.maximum.hp);
    frame(&mut run, &reference);
    assert!(run.needs_output().dead);
    let after = load(&reference, &saved);
    assert!(!after.needs_output().dead && after.world_supported());
}

/// K3. Source `fpscontroller` use ray on a car door handle from outside: the aim takes the door
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
    // Source `fpscontroller.UseUsable`: a held E drags the door with the mouse. The closed
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
