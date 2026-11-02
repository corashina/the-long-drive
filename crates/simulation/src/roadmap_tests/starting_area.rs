//! ROADMAP test pass: Starting area (SA rows). Each test cites its source method.
use crate::audio::AudioKind;
use crate::boundary::{HudBlock, SimFrame, StateBlock};
use crate::interaction::crosshair::{Crosshair, crosshair};
use crate::reference::ItemId;
use crate::reference::ReferenceManifest;
use crate::simulation::{FIXED_DT, FrameInput, keys};
use crate::starting_area::GarageRuntime;
use crate::world::stream::tests::kit::*;

/// The audio kinds of the frame since the last call.
fn audio(run: &mut GarageRuntime) -> Vec<AudioKind> {
    let audio = run.audio.as_mut().expect("audio binds");
    let kinds = audio
        .survival_events
        .iter()
        .chain(&audio.survival_loops)
        .map(|e| e.kind)
        .collect();
    audio.survival_events.clear();
    kinds
}

fn sim_frame(run: &mut GarageRuntime, reference: &ReferenceManifest) -> SimFrame {
    let mut out = SimFrame::default();
    run.fill_frame(reference, 1., &mut out);
    out
}

/// The crosshair layers of the hud, as `fill_hud` computes them.
fn crosshair_bits(run: &GarageRuntime) -> u32 {
    let available = run.area.interaction.available_actions(
        &run.aim,
        &run.area.player,
        &run.area.items,
        &run.physics,
        run.survival.output(),
    );
    if run.area.interaction.binoculars.active().is_some() {
        return 0;
    }
    let weight = run.area.interaction.carry.weight_factors(&run.physics);
    crosshair(
        &available,
        run.area.interaction.held().is_some(),
        run.area.interaction.guns.crosshair().is_some(),
        weight[1],
    )
}

fn named(run: &GarageRuntime, name: &str) -> ItemId {
    run.area
        .items
        .iter()
        .find(|i| !i.removed && run.area.items.name(i.id) == Some(name))
        .unwrap_or_else(|| panic!("garage item {name}"))
        .id
}

/// The crosshair bits while the use ray hits `collider`.
fn looking_at(run: &mut GarageRuntime, collider: rapier3d::prelude::ColliderHandle) -> u32 {
    let point = run.physics.rapier().colliders[collider]
        .compute_aabb()
        .center()
        .to_array();
    run.aim.use_target = Some(crate::interaction::AimTarget {
        collider,
        item: ItemId(0),
        distance: 0.5,
        point,
        slot: None,
    });
    crosshair_bits(run)
}

/// SA8: source `tavcsoscript.Update`, `fpscontroller.Sight` (0.1 look scale) and
/// `mainscript.CrossHairStuff` (hidden while `Bbinoculars`).
#[test]
#[ignore = "requires the installed source export"]
fn tp_sa8_binoculars_zoom_hide_and_slow_the_look() {
    let (reference, mut run) = load_garage();
    seconds(&mut run, &reference, 1.);
    let b = &reference.small_items.binoculars;
    // Source tavcsoscript fields on the TÁVCSO prefab.
    assert_eq!((b.min_fov, b.max_fov), (3., 4.));
    let item = named(&run, "TÁVCSO");
    pick_up(&mut run, &reference, item);
    frame(&mut run, &reference);
    assert_eq!(
        run.area.interaction.held(),
        Some(item),
        "binoculars in hand"
    );
    assert!(
        run.area
            .interaction
            .binoculars
            .reserves_lmb(None, Some(item)),
        "the binoculars bind"
    );
    assert!(crosshair_bits(&run) != 0, "crosshair shows before use");
    let yaw = run.area.player.yaw_degrees();
    frame_with(
        &mut run,
        &reference,
        FrameInput {
            mouse: [20., 0.],
            ..FrameInput::default()
        },
    );
    let normal = run.area.player.yaw_degrees() - yaw;
    assert!(normal.abs() > 0.1, "look turns: {normal}");

    let lmb = FrameInput {
        held: keys::PRIMARY,
        pressed: keys::PRIMARY,
        ..FrameInput::default()
    };
    frame_with(&mut run, &reference, lmb);
    let lmb = FrameInput {
        held: keys::PRIMARY,
        ..FrameInput::default()
    };
    frame_with(&mut run, &reference, lmb);
    let used = run
        .area
        .interaction
        .binoculars
        .active()
        .expect("LMB uses the binoculars");
    assert!(
        (3. ..=4.).contains(&used.fov_degrees),
        "{}",
        used.fov_degrees
    );
    let out = sim_frame(&mut run, &reference);
    assert!(out.state.flags & StateBlock::BINOCULARS != 0);
    assert_eq!(out.state.fov_degrees, used.fov_degrees);
    assert_eq!(crosshair_bits(&run), 0, "crosshair hides");
    assert!(
        out.structures
            .iter()
            .any(|r| r.flags & crate::boundary::StructureRow::CAMERA_HIDDEN != 0),
        "the binoculars hide from the camera"
    );
    let yaw = run.area.player.yaw_degrees();
    frame_with(
        &mut run,
        &reference,
        FrameInput {
            mouse: [20., 0.],
            ..lmb
        },
    );
    let slow = run.area.player.yaw_degrees() - yaw;
    assert!(
        (slow / normal - 0.1).abs() < 0.02,
        "look speed drops to 0.1: {slow} / {normal}"
    );
    // Scroll in each way: the zoom stays between 3 and 4 degrees and reaches both ends.
    for wheel in [1., -1.] {
        for _ in 0..240 {
            frame_with(&mut run, &reference, FrameInput { wheel, ..lmb });
        }
        let fov = run
            .area
            .interaction
            .binoculars
            .active()
            .unwrap()
            .fov_degrees;
        let end = if wheel > 0. { b.min_fov } else { b.max_fov };
        assert!((fov - end).abs() < 1e-4, "wheel {wheel}: fov {fov}");
    }
    frame(&mut run, &reference);
    assert!(
        run.area.interaction.binoculars.active().is_none(),
        "release returns"
    );
    let out = sim_frame(&mut run, &reference);
    assert!(out.state.flags & StateBlock::BINOCULARS == 0);
    assert!(crosshair_bits(&run) != 0);
}

/// Knob colliders of one kind.
fn knobs(
    run: &GarageRuntime,
    kinds: &[crate::interaction::Knob],
) -> Vec<(
    rapier3d::prelude::ColliderHandle,
    crate::reference::ColliderId,
    crate::interaction::Knob,
)> {
    run.physics
        .rapier()
        .colliders
        .iter()
        .filter_map(|(handle, _)| {
            let target = crate::interaction::AimTarget {
                collider: handle,
                item: ItemId(0),
                distance: 0.,
                point: [0.; 3],
                slot: None,
            };
            let (id, kind) = run.area.interaction.dial_knob(&target, &run.physics)?;
            kinds.contains(&kind).then_some((handle, id, kind))
        })
        .collect()
}

/// SA9: source `mainscript.CrossHairStuff`: GCMMB shows `lookedUsable.img` for a tuneAble usable,
/// and `lookedCap.img`, or `ICap` without one, for a canTune cap.
#[test]
#[ignore = "requires the installed source export"]
fn tp_sa9_tune_icons_per_knob_and_cap() {
    use crate::interaction::Knob;
    let (reference, mut run) = load_garage();
    seconds(&mut run, &reference, 1.);
    let mut icons = Vec::new();
    // The water tap: a canTune cap with its own image.
    let tap = reference
        .openings
        .iter()
        .find(|o| o.can_tune && o.tune_icon.is_some())
        .expect("tuneable tap");
    let expected = reference
        .ui
        .sprite(tap.tune_icon.as_ref().unwrap())
        .unwrap()
        + 1;
    let point = run.area.items.get(tap.cap).unwrap().pose.position;
    let start = run.area.player.body_pose(&run.physics);
    let mut reached = false;
    for k in 0..8 {
        let a = k as f32 * std::f32::consts::FRAC_PI_4;
        let mut pose = start;
        pose.position = [
            point[0] + 0.8 * a.cos(),
            start.position[1],
            point[2] + 0.8 * a.sin(),
        ];
        run.area.player.recover(pose, &mut run.physics);
        seconds(&mut run, &reference, 0.3);
        if aim_at_item(&mut run, &reference, tap.cap) {
            reached = true;
            break;
        }
    }
    assert!(reached, "tap");
    let bits = crosshair_bits(&run);
    assert!(bits & Crosshair::MMB != 0, "tap: MMB layer");
    assert_eq!(
        bits >> Crosshair::MMB_SPRITE_SHIFT,
        expected,
        "tap: own image"
    );
    icons.push(("tap", expected));
    // A canTune cap without img shows ICap: `crosshair::tests::layers_follow_the_source_rules`.

    // Compass and clock knobs.
    spawn_scripts(&mut run, &reference, &["compassscript", "clockscript"]);
    for (handle, _, kind) in knobs(&run, &[Knob::Dial]) {
        let bits = looking_at(&mut run, handle);
        assert!(bits & Crosshair::MMB != 0, "{kind:?}: MMB layer");
        let sprite = bits >> Crosshair::MMB_SPRITE_SHIFT;
        assert!(sprite > 0, "{kind:?}: own image");
        icons.push(("dial", sprite));
    }
    assert!(icons.len() >= 3, "compass and clock knobs: {icons:?}");

    // The garage radio is in the Volna dashboard.
    for (handle, _, kind) in knobs(&run, &[Knob::Volume, Knob::Tuning]) {
        let bits = looking_at(&mut run, handle);
        assert!(bits & Crosshair::MMB != 0, "{kind:?}: MMB layer");
        let sprite = bits >> Crosshair::MMB_SPRITE_SHIFT;
        assert!(sprite > 0, "{kind:?}: own image");
        icons.push(("radio", sprite));
    }
    assert_eq!(
        icons.iter().filter(|i| i.0 == "radio").count(),
        2,
        "radio volume and tune knobs"
    );
}

/// SA6: source `musicboxscript.Update`, `radioscript` tune, `compassscript.GetNorthValue` and
/// `clockscript.Update` (add 0.0625).
#[test]
#[ignore = "requires the installed source export"]
fn tp_sa6_music_box_radio_compass_and_clock() {
    use crate::interaction::Knob;
    let (reference, mut run) = load_garage();
    seconds(&mut run, &reference, 1.);
    assert_eq!(crate::simulation::dials::CLOCK_ADD, 0.0625);
    let parents = spawn_scripts(
        &mut run,
        &reference,
        &["compassscript", "clockscript", "musicboxscript"],
    );
    let [compass, clock, music] = [&parents[0], &parents[1], &parents[2]];
    let nodes = |run: &GarageRuntime, p: &crate::world::GeneratedId| {
        run.stream
            .dial(p)
            .unwrap()
            .nodes()
            .map(|n| n.pose.rotation)
            .collect::<Vec<_>>()
    };
    // Clock hands follow the cycle.
    let cycle = &reference.lighting.cycle;
    let length = f64::from(cycle.day_seconds + cycle.night_seconds);
    act(&mut run, &reference, r#"{"action":"cycle","seconds":10}"#);
    seconds(&mut run, &reference, 1.);
    let before = nodes(&run, clock);
    act(
        &mut run,
        &reference,
        &format!(r#"{{"action":"cycle","seconds":{}}}"#, length / 3.),
    );
    seconds(&mut run, &reference, 1.);
    assert_ne!(nodes(&run, clock), before, "clock hands move with the time");
    // The compass needle reads the heading: turning the compass turns the needle.
    let before = nodes(&run, compass);
    let item = content_item(&run, compass).unwrap();
    let body = run.area.items.get(item).unwrap().body.unwrap();
    run.physics.edit_body(body, |b| {
        let mut pose = *b.position();
        pose.rotation = rapier3d::math::Rotation::from_rotation_y(1.5) * pose.rotation;
        b.set_position(pose, true);
    });
    seconds(&mut run, &reference, 2.);
    assert_ne!(
        nodes(&run, compass),
        before,
        "compass needle follows the heading"
    );
    // Crank the music box: notes play.
    let crank = knobs(&run, &[Knob::Crank]);
    assert_eq!(crank.len(), 1, "one music box crank");
    let _ = audio(&mut run);
    let _ = music;
    let mut played = 0;
    for i in 0..240 {
        let a = i as f32 * 0.2;
        run.stream
            .crank_music_box(crank[0].1, [a.cos() * 30., a.sin() * 30.]);
        frame(&mut run, &reference);
        played += audio(&mut run)
            .iter()
            .filter(|k| **k == AudioKind::MusicBox)
            .count();
    }
    assert!(played > 0, "the crank plays notes");

    // Garage radio in the Volna dashboard: the tune knob changes the frequency, the volume
    // knob the volume.
    for (_, knob, kind) in knobs(&run, &[Knob::Volume, Knob::Tuning]) {
        let before = *run.stream.knob_radio(knob).unwrap();
        for _ in 0..5 {
            run.stream.radio_control(knob, 1., &mut run.area.items);
        }
        let after = run.stream.knob_radio(knob).unwrap();
        match kind {
            Knob::Volume => assert_ne!(after.volume, before.volume, "volume knob"),
            _ => assert_ne!(after.frequency(), before.frequency(), "tune knob"),
        }
    }
}

/// SA12: source `fpscontroller.Update` burpDown and `Shit()` (bowel >= 0.3 of max, turd spawn).
#[test]
#[ignore = "requires the installed source export"]
fn tp_sa12_burp_key_spawns_a_turd_on_foot_and_seated() {
    let (reference, mut run) = load_garage();
    seconds(&mut run, &reference, 1.);
    assert_eq!(reference.survival.toilet.bowel_fraction, 0.3);
    for seated in [false, true] {
        if seated {
            act(&mut run, &reference, r#"{"action":"seat","seat":"driver"}"#);
            assert!(run.area.player.seated());
        }
        let turds = |run: &GarageRuntime| run.area.items.iter().filter(|i| !i.removed).count();
        let before = turds(&run);
        let mut spawned = false;
        for _ in 0..400 {
            let mut needs = run.survival.capture_save();
            needs.needs.bowel = reference.survival.maximum.bowel;
            run.survival.restore_save(&needs);
            frame_with(
                &mut run,
                &reference,
                FrameInput {
                    held: keys::BURP,
                    pressed: keys::BURP,
                    ..FrameInput::default()
                },
            );
            for _ in 0..30 {
                frame(&mut run, &reference);
            }
            if turds(&run) > before {
                spawned = true;
                break;
            }
        }
        assert!(spawned, "seated {seated}: a turd spawns");
        assert!(run.stream.frame().failure.is_none());
    }
}

/// SA4: source `fpscontroller` walk head bob: `currentAdd = dt * walkBobSpeed + v * walkBobSpeedRB
/// * dt` and `Sin(walkBobState) * maxWalkBob`.
#[test]
#[ignore = "requires the installed source export"]
fn tp_sa4_head_bob_amplitude() {
    let (reference, mut run) = load_garage();
    seconds(&mut run, &reference, 1.);
    let player = run.area.player.reference().clone();
    let max = player.max_walk_bob;
    let (mut low, mut high) = (f32::MAX, f32::MIN);
    let mut active = 0;
    for i in 0..240 {
        let before = run.area.player.body_pose(&run.physics).position;
        frame_with(
            &mut run,
            &reference,
            FrameInput {
                held: keys::FORWARD,
                ..FrameInput::default()
            },
        );
        let after = run.area.player.body_pose(&run.physics).position;
        let speed = (rapier3d::math::Vector::from_array(after)
            - rapier3d::math::Vector::from_array(before))
        .length()
            / FIXED_DT;
        let expected =
            FIXED_DT * player.walk_bob_speed + speed * player.walk_bob_speed_rb * FIXED_DT;
        let add = run.area.player.walk_add();
        if i > 30 && add > 0. {
            active += 1;
            assert!(
                (add - expected).abs() < 0.15 * expected,
                "currentAdd {add}, source {expected} at {speed} m/s"
            );
        }
        let view = run.area.player.view(&run.physics);
        let body = run.area.player.body_pose(&run.physics);
        let y = view.head_pose.position[1] - body.position[1];
        low = low.min(y);
        high = high.max(y);
    }
    assert!(active > 100, "walk bob runs on {active} frames");
    // The head follows the bob target through the head lerp, so the span stays under 2 * max.
    let span = high - low;
    assert!(
        span > max && span < 2.05 * max,
        "bob span {span} for maxWalkBob {max}"
    );
}

/// SA4: source `fpscontroller.LadderMove` (`FladderSpeed * TLadder.speed`, up while looking
/// within 70 degrees of the ladder up) and `ladderscript.OnTriggerStay` (ladderV 5).
#[test]
#[ignore = "requires the installed source export"]
fn tp_sa4_ladder_climb() {
    let (reference, mut run) = load_garage();
    // World buildings carry ladder triggers.
    let ladders = reference
        .world
        .prototypes
        .iter()
        .flat_map(|p| &p.colliders)
        .filter(|c| matches!(c.role, Some(crate::reference::TriggerRole::Ladder { .. })))
        .count();
    assert!(ladders >= 10, "{ladders} ladder triggers");
    seconds(&mut run, &reference, 1.);
    // A ladder trigger around the player, as `trigger_roles` publishes it.
    let position = run.area.player.body_pose(&run.physics).position;
    let handle = run.physics.insert_collider(
        rapier3d::prelude::ColliderBuilder::cuboid(4., 6., 4.)
            .translation(rapier3d::math::Vector::from_array(position))
            .sensor(true)
            .build(),
    );
    run.physics.refresh_queries();
    add_trigger_role(
        &mut run,
        handle,
        crate::reference::TriggerRole::Ladder {
            speed: 1.,
            up: [0., 1., 0.],
        },
    );
    let start = run.area.player.body_pose(&run.physics).position[1];
    // Looking level, a grounded player walks: the angle to the ladder up is over 70 degrees.
    let look = |run: &mut GarageRuntime, up: f32| {
        let eye = run.area.player.view(&run.physics).camera_pose.position;
        run.area
            .player
            .look_at([eye[0] + 1., eye[1] + up, eye[2]], &run.physics);
    };
    for _ in 0..30 {
        look(&mut run, 0.);
        frame_with(
            &mut run,
            &reference,
            FrameInput {
                held: keys::FORWARD,
                ..FrameInput::default()
            },
        );
    }
    let level = run.area.player.body_pose(&run.physics).position[1];
    assert!(
        (level - start).abs() < 0.05,
        "level look walks: {start} -> {level}"
    );
    // Looking up climbs.
    let climb = 2.;
    for _ in 0..(climb / FIXED_DT) as usize {
        look(&mut run, 3.);
        frame_with(
            &mut run,
            &reference,
            FrameInput {
                held: keys::FORWARD,
                ..FrameInput::default()
            },
        );
    }
    let end = run.area.player.body_pose(&run.physics).position[1];
    let speed = run.area.player.reference().ladder_speed;
    assert!(
        end - level > 0.5 * speed * climb,
        "climbs {} m",
        end - level
    );
    // Back climbs down.
    for _ in 0..60 {
        look(&mut run, 3.);
        frame_with(
            &mut run,
            &reference,
            FrameInput {
                held: keys::BACK,
                ..FrameInput::default()
            },
        );
    }
    let down = run.area.player.body_pose(&run.physics).position[1];
    assert!(down < end, "back climbs down: {end} -> {down}");
}

/// SA4: source `playerdeathtriggerscript.OnTriggerStay`
/// (`DamageInstant(damagePlayer * Time.deltaTime)`) and `survivalscript.DamageInstant` (death at 0 hp).
#[test]
#[ignore = "requires the installed source export"]
fn tp_sa4_hazard_damages_until_death() {
    let (reference, mut run) = load_garage();
    seconds(&mut run, &reference, 1.);
    let hp = run.needs_output().needs.hp;
    // A hazard trigger around the player, as `trigger_roles` publishes it.
    let position = run.area.player.body_pose(&run.physics).position;
    let handle = run.physics.insert_collider(
        rapier3d::prelude::ColliderBuilder::cuboid(5., 5., 5.)
            .translation(rapier3d::math::Vector::from_array(position))
            .sensor(true)
            .build(),
    );
    run.physics.refresh_queries();
    // Damage per second equal to full hp kills in 1 s.
    add_trigger_role(
        &mut run,
        handle,
        crate::reference::TriggerRole::Hazard {
            damage_player: hp,
            damage_breakable: 0.,
        },
    );
    seconds(&mut run, &reference, 0.5);
    let half = run.needs_output();
    assert!(!half.dead);
    assert!(half.needs.hp < hp * 0.6, "hurt: {hp} -> {}", half.needs.hp);
    seconds(&mut run, &reference, 1.);
    let end = run.needs_output();
    assert!(end.dead, "hp {}", end.needs.hp);
    assert_eq!(end.needs.hp, 0.);
}

/// SA4: source `mainscript` `menu.GV.SetActive(player.showSleepText && canSleep)`.
#[test]
#[ignore = "requires the installed source export"]
fn tp_sa4_sleep_hint_in_a_seat() {
    let (reference, mut run) = load_garage();
    seconds(&mut run, &reference, 1.);
    let flags = |run: &mut GarageRuntime| sim_frame(run, &reference).hud.flags;
    assert!(
        flags(&mut run) & HudBlock::SLEEP_HINT == 0,
        "no hint on foot"
    );
    let seats: Vec<_> = run
        .physics
        .rapier()
        .colliders
        .iter()
        .filter_map(|(h, _)| run.area.interaction.item_seat(h))
        .collect();
    let seat = seats
        .iter()
        .find(|s| s.seat.show_sleep_text)
        .expect("a garage seat with showSleepText")
        .clone();
    assert!(run.area.player.enter_item_seat(&seat, &mut run.physics));
    seconds(&mut run, &reference, 0.5);
    assert!(run.area.player.show_sleep_text(), "seat shows sleep text");
    assert!(
        flags(&mut run) & HudBlock::SLEEP_HINT != 0,
        "hint in the seat"
    );
}

/// SA5: source `stepscript.Step`, `tosaveitemscript` hit sounds, `mainscript` ambience and
/// `buildingscript` interior muffle.
#[test]
#[ignore = "requires the installed source export"]
fn tp_sa5_player_sound_events_and_garage_muffle() {
    let (reference, mut run) = load_garage();
    seconds(&mut run, &reference, 1.);
    let _ = audio(&mut run);
    let mut kinds = Vec::new();
    for _ in 0..180 {
        frame_with(
            &mut run,
            &reference,
            FrameInput {
                held: keys::FORWARD,
                ..FrameInput::default()
            },
        );
        kinds.extend(audio(&mut run));
        let _ = sim_frame(&mut run, &reference);
    }
    assert!(kinds.contains(&AudioKind::Footstep), "footsteps: {kinds:?}");
    assert!(
        kinds.contains(&AudioKind::AmbientDay) || kinds.contains(&AudioKind::AmbientNight),
        "ambience: {kinds:?}"
    );
    // Drop a can from 2 m: it hits with a sound.
    let can = named(&run, "OilCan");
    let body = run.area.items.get(can).unwrap().body.unwrap();
    run.physics.edit_body(body, |b| {
        let mut pose = *b.position();
        pose.translation.y += 2.;
        b.set_position(pose, true);
    });
    let mut hit = false;
    for _ in 0..120 {
        frame(&mut run, &reference);
        hit |= audio(&mut run).contains(&AudioKind::ItemHit);
    }
    assert!(hit, "the dropped can makes a hit sound");
    let audio = run.audio.as_ref().unwrap();
    assert!(
        audio.current_interior.is_some(),
        "the player is in a garage interior"
    );
    assert!(
        audio.outside_gain < 1.,
        "garage muffles outside sound: {}",
        audio.outside_gain
    );
}

/// Moves the player to the ground at absolute `x`, `z` and waits for the world.
fn teleport(run: &mut GarageRuntime, reference: &ReferenceManifest, x: f64, z: f64) {
    act(
        run,
        reference,
        &format!(r#"{{"action":"teleport","x":{x},"z":{z}}}"#),
    );
    for _ in 0..3000 {
        frame(run, reference);
        if run.world_supported() && run.area.player.grounded() {
            seconds(run, reference, 1.);
            return;
        }
    }
    panic!("world at {x}, {z} does not load");
}

/// SA4: far from the origin the ground stays ground: source `fpscontroller` walks on the
/// terrain normal. The port rebases the origin and reads manifold normals (`player::normals`).
#[test]
#[ignore = "requires the installed source export"]
fn tp_sa4_walks_on_slopes_100_km_out() {
    let (reference, mut run) = load_garage();
    seconds(&mut run, &reference, 1.);
    teleport(&mut run, &reference, 100_000., 0.);
    let absolute = run
        .stream
        .origin
        .absolute(run.area.player.body_pose(&run.physics).position);
    assert!(absolute[0] > 99_000., "player at {absolute:?}");
    for yaw in [0., 90., 180., 270.] {
        let start = run.area.player.body_pose(&run.physics).position;
        let a = f32::to_radians(yaw);
        let eye = run.area.player.view(&run.physics).camera_pose.position;
        run.area
            .player
            .look_at([eye[0] + a.sin(), eye[1], eye[2] - a.cos()], &run.physics);
        let mut grounded = 0;
        for _ in 0..120 {
            frame_with(
                &mut run,
                &reference,
                FrameInput {
                    held: keys::FORWARD,
                    ..FrameInput::default()
                },
            );
            grounded += run.area.player.grounded() as u32;
        }
        let end = run.area.player.body_pose(&run.physics).position;
        let moved = ((end[0] - start[0]).powi(2) + (end[2] - start[2]).powi(2)).sqrt();
        let walk = run.area.player.reference().walk_speed * 2.;
        assert!(moved > 0.7 * walk, "yaw {yaw}: walked {moved} m of {walk}");
        assert!(
            grounded > 100,
            "yaw {yaw}: grounded {grounded} of 120 frames"
        );
    }
}

/// Lifts the player onto a flat fixed platform, then builds `count` steps of `rise` by
/// `depth` metres ahead. Returns the colliders to remove.
fn stairs(
    run: &mut GarageRuntime,
    count: usize,
    rise: f32,
    depth: f32,
) -> Vec<rapier3d::prelude::ColliderHandle> {
    use rapier3d::math::{Rotation, Vector};
    use rapier3d::prelude::ColliderBuilder;
    let pose = run.area.player.body_pose(&run.physics);
    let at = Vector::from_array(pose.position);
    let ground = |run: &GarageRuntime, x: f32, z: f32| {
        run.stream
            .ground_height(&run.physics, [x, at.y, z])
            .expect("terrain under the player")
    };
    let offset = at.y - ground(run, at.x, at.z);
    let mut top = f32::MIN;
    for i in -6..=6 {
        for j in -6..=6 {
            top = top.max(ground(run, at.x + i as f32, at.z + j as f32));
        }
    }
    top += 0.05;
    let forward = (Rotation::from_array(pose.rotation) * -Vector::Z)
        .with_y(0.)
        .normalize();
    let yaw = Vector::Y * forward.x.atan2(forward.z);
    let mut handles = vec![
        run.physics.insert_collider(
            ColliderBuilder::cuboid(6., 0.5, 6.)
                .translation(Vector::new(at.x, top - 0.5, at.z))
                .rotation(yaw)
                .build(),
        ),
    ];
    for i in 0..count {
        let height = rise * (i + 1) as f32;
        let center = at + forward * (1.5 + depth * (i as f32 + 0.5));
        handles.push(
            run.physics.insert_collider(
                ColliderBuilder::cuboid(1.5, height * 0.5, depth * 0.5)
                    .translation(Vector::new(center.x, top + height * 0.5, center.z))
                    .rotation(yaw)
                    .build(),
            ),
        );
    }
    let landing = at + forward * (1.5 + depth * count as f32 + 1.);
    let height = rise * count as f32;
    handles.push(
        run.physics.insert_collider(
            ColliderBuilder::cuboid(1.5, height * 0.5, 1.)
                .translation(Vector::new(landing.x, top + height * 0.5, landing.z))
                .rotation(yaw)
                .build(),
        ),
    );
    run.physics.refresh_queries();
    let mut lifted = pose;
    lifted.position[1] = top + offset;
    run.area.player.recover(lifted, &mut run.physics);
    handles
}

/// SA11: source `fpscontroller` `Feet2` sphere (active while `duckValue > ctrlDuckValue`) carries
/// the player up a step. Prone has no feet sphere and does not step. The garage pit stairs:
/// `player::collision::tests::standing_player_walks_up_stairs`.
#[test]
#[ignore = "requires the installed source export"]
fn tp_sa11_steps_standing_and_crouched_not_prone() {
    let (reference, mut run) = load_garage();
    seconds(&mut run, &reference, 1.);
    teleport(&mut run, &reference, 100_000., 0.);
    let ctrl = reference.player.posture.ctrl_value;
    let home = run.area.player.body_pose(&run.physics);
    for (name, held, wheel) in [
        ("standing", keys::FORWARD, 0.),
        ("crouched", keys::FORWARD | keys::CROUCH, 0.),
        ("prone", keys::FORWARD, -1.),
    ] {
        run.area.player.recover(home, &mut run.physics);
        let handles = stairs(&mut run, 4, 0.18, 0.3);
        seconds(&mut run, &reference, 0.5);
        if wheel != 0. {
            for _ in 0..60 {
                frame_with(
                    &mut run,
                    &reference,
                    FrameInput {
                        wheel,
                        ..FrameInput::default()
                    },
                );
            }
            assert!(
                run.area.player.posture() < ctrl,
                "prone posture {}",
                run.area.player.posture()
            );
        }
        let start = run.area.player.body_pose(&run.physics).position[1];
        let mut rise = f32::MIN;
        for _ in 0..300 {
            frame_with(
                &mut run,
                &reference,
                FrameInput {
                    held,
                    ..FrameInput::default()
                },
            );
            rise = rise.max(run.area.player.body_pose(&run.physics).position[1] - start);
        }
        if name == "prone" {
            assert!(rise < 0.1, "prone steps up {rise} m");
        } else {
            assert!(rise > 0.6, "{name}: steps up {rise} m of 0.72");
        }
        for handle in handles {
            run.physics.remove_collider(handle);
        }
        run.physics.refresh_queries();
    }
}

/// SA12: source `meleeweaponscript.hurtFood` cuts a food on hit. The cut itself:
/// `melee::shovel_cuts_food_and_the_pieces_reload`. This lists the cutters, the source has no knife.
#[test]
#[ignore = "requires the installed source export"]
fn tp_sa12_hurt_food_weapons_cut_food() {
    let (reference, _run) = load_garage();
    let cutters: Vec<_> = reference
        .melee
        .weapons
        .iter()
        .filter(|w| w.contact.hurt_food)
        .filter_map(|w| {
            let p = reference
                .world
                .prototypes
                .iter()
                .find(|p| p.source == w.prototype.0)?;
            Some(p.nodes[0].name.to_lowercase())
        })
        .collect();
    // The source has no knife. The crowbar, the katana and the shovel cut food.
    assert_eq!(cutters, ["crowbar", "katana", "shovel"]);
}

/// SA7: source `mainscript.CrossHairStuff` layers for the targets the use ray finds in the
/// garage, and `fpscontroller` `weightM2 = InverseLerp(maxWeight * 2, minWeight, mass)`.
#[test]
#[ignore = "requires the installed source export"]
fn tp_sa7_crosshair_icons_follow_the_looked_target() {
    use crate::interaction::crosshair::AdjustIcon;
    use crate::interaction::feedback::FeedbackContext as C;
    let (reference, mut run) = load_garage();
    seconds(&mut run, &reference, 1.);
    let home = run.area.player.body_pose(&run.physics);
    // A player placed inside the start car launches it.
    let car = {
        use rapier3d::parry::bounding_volume::BoundingVolume;
        let world = run.physics.rapier();
        let chassis = run.physics.chassis_handle(run.vehicle.as_ref().unwrap());
        let mut aabbs = world.bodies[chassis]
            .colliders()
            .iter()
            .map(|c| world.colliders[*c].compute_aabb());
        let first = aabbs.next().unwrap();
        aabbs.fold(first, |a, b| a.merged(&b)).loosened(0.5)
    };
    let mut seen = std::collections::BTreeMap::<u32, std::collections::BTreeSet<u32>>::new();
    for ring in 0..9 {
        let mut pose = home;
        if ring > 0 {
            let a = ring as f32 * std::f32::consts::FRAC_PI_4;
            pose.position[0] += 2.5 * a.cos();
            pose.position[2] += 2.5 * a.sin();
        }
        let at = rapier3d::math::Vector::from_array(pose.position);
        if car.contains_local_point(at.with_y(car.center().y)) {
            continue;
        }
        run.area.player.recover(pose, &mut run.physics);
        seconds(&mut run, &reference, 0.3);
        for yaw in 0..24 {
            for pitch in [-35f32, -15., 5., 25.] {
                let a = (yaw as f32 * 15.).to_radians();
                let p = pitch.to_radians();
                let eye = run.area.player.view(&run.physics).camera_pose.position;
                let dir = [a.sin() * p.cos(), p.sin(), -a.cos() * p.cos()];
                run.area.player.look_at(
                    [eye[0] + dir[0], eye[1] + dir[1], eye[2] + dir[2]],
                    &run.physics,
                );
                frame(&mut run, &reference);
                if run.aim.use_target.is_none() {
                    continue;
                }
                let available = run.area.interaction.available_actions(
                    &run.aim,
                    &run.area.player,
                    &run.area.items,
                    &run.physics,
                    run.survival.output(),
                );
                seen.entry(available.context as u32)
                    .or_default()
                    .insert(crosshair_bits(&run));
            }
        }
    }
    let e = |bits: u32| bits & 3;
    let adjust = |bits: u32| (bits >> Crosshair::ADJUST_SHIFT) & 3;
    // Every target shows at least one layer, and the default only with no E, F, LMB or MMB.
    for bits in seen.values().flatten() {
        assert_ne!(*bits, 0);
        if bits & Crosshair::DEFAULT != 0 {
            assert_eq!(
                bits & (3 | 7 << Crosshair::F_SHIFT | Crosshair::LMB | Crosshair::MMB),
                0
            );
        }
    }
    let has = |c: C, test: &dyn Fn(u32) -> bool| {
        seen.get(&(c as u32))
            .is_some_and(|b| b.iter().any(|&b| test(b)))
    };
    assert!(
        has(C::LightSwitch, &|b| e(b) == 3),
        "a light switch shows the E switch icon"
    );
    assert!(
        has(C::Mechanism, &|b| adjust(b) != AdjustIcon::None as u32),
        "a usable shows its adjust icon"
    );
    // Carry: a light item shows pickup over the default; the heaviest garage item is too heavy.
    run.area.player.recover(home, &mut run.physics);
    seconds(&mut run, &reference, 0.3);
    let can = named(&run, "OilCan");
    pick_up(&mut run, &reference, can);
    frame(&mut run, &reference);
    assert_eq!(run.area.interaction.held(), Some(can));
    let light = crosshair_bits(&run);
    assert_eq!(
        light & (Crosshair::DEFAULT | Crosshair::PICKUP | Crosshair::TOO_HEAVY),
        Crosshair::DEFAULT | Crosshair::PICKUP
    );
    let p = reference.player.carry_max_weight * 2.;
    let w = run.area.interaction.carry.weight_factors(&run.physics)[1];
    let mass = run
        .physics
        .authored_body_mass(run.area.interaction.carry.body().unwrap());
    let min = reference.player.carry_min_weight;
    assert!((w - ((mass - p) / (min - p)).clamp(0., 1.)).abs() < 1e-5);
    // No garage item over (min + 2 max) / 2 = 86 kg can be carried: every heavier body is a
    // 982.6 kg fixture. The TOO_HEAVY layer at weightM2 < 0.5: crosshair unit test.
}
