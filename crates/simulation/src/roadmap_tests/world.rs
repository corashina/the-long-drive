//! ROADMAP test pass: World and terrain (W rows).
use crate::reference::ReferenceManifest;
use crate::starting_area::GarageRuntime;
use crate::world::GameMode;
use crate::world::stream::tests::kit::*;
use serde_json::json;

/// Shovel prefab name. The reference export spawns no crowbar copy.
const SHOVEL: &[&str] = &["shovel"];

/// Frames until the stream supports the player again. False after 60 s.
pub(super) fn loads(run: &mut GarageRuntime, reference: &ReferenceManifest) -> bool {
    for _ in 0..3000 {
        if run.world_supported() && run.stream.required_ready() {
            return true;
        }
        frame(run, reference);
    }
    false
}

/// Frames until the stream supports the player again. Fails after 60 s with the first gap.
pub(super) fn wait_loaded(run: &mut GarageRuntime, reference: &ReferenceManifest) {
    if loads(run, reference) {
        return;
    }
    let focus = player_absolute(run);
    let bounds = crate::world::WorldBounds {
        min: focus.map(|v| v - 8.),
        max: focus.map(|v| v + 8.),
    };
    panic!(
        "the area at {focus:?} does not load: {:?}",
        run.stream.support_gap(bounds)
    );
}

/// Saves `run` and loads the save in `mode`, as the browser Load does.
fn load_in_mode(
    run: &GarageRuntime,
    reference: &ReferenceManifest,
    mode: GameMode,
) -> GarageRuntime {
    let saved = save(run, reference);
    let mut loaded = GarageRuntime::from_save(
        reference,
        &roadside_stamp(reference),
        &saved,
        mode,
        reference.world.draw_distance.default,
    )
    .unwrap();
    restore(&mut loaded, reference, |_| true);
    loaded
}

fn player_absolute(run: &GarageRuntime) -> [f64; 3] {
    run.stream
        .origin
        .absolute(run.area.player.body_pose(&run.physics).position)
}

/// W3. Source `gamemodesettingsscript.SetInMain`: the biome is a game mode setting, not save data.
/// `TerrainGenerator.GetBiome` returns `GSettings.biome - 1` when it is not Mixed.
#[test]
#[ignore = "requires the installed source export"]
fn tp_w3_mixed_save_loads_as_winter() {
    let (reference, mut run) = load_garage();
    seconds(&mut run, &reference, 1.);
    assert_eq!(run.stream.mode().biome, 0);
    let winter = GameMode {
        biome: 2,
        ..GameMode::default()
    };
    let mut loaded = load_in_mode(&run, &reference, winter);
    assert_eq!(loaded.stream.mode().biome, 2);
    let at = player_absolute(&loaded);
    let color = loaded.stream.terrain_color(at);
    assert_eq!(color[..3], [0., 0., 1.], "winter terrain color");
    assert_eq!(crate::world::biome(color, None), 1);
    seconds(&mut loaded, &reference, 1.5);
    assert_eq!(
        loaded.lighting.cycle.output().grading_temperature,
        Some(-10.),
        "winter color grading"
    );
}

/// A loose shovel copy `forward` metres in front of the player.
fn loose_item(
    run: &mut GarageRuntime,
    reference: &ReferenceManifest,
    forward: f32,
) -> crate::reference::ItemId {
    use crate::world::stream::tests::melee::{place, spawn, weapon};
    let item = spawn(run, reference, weapon(reference, SHOVEL));
    let pose = run.area.player.body_pose(&run.physics);
    let ahead = rapier3d::math::Rotation::from_array(pose.rotation)
        * rapier3d::math::Vector::NEG_Z
        * forward;
    let at = rapier3d::math::Vector::from_array(pose.position) + ahead;
    place(run, item, at + rapier3d::math::Vector::Y * 0.5);
    item
}

/// Speed of the item body.
fn speed(run: &GarageRuntime, item: crate::reference::ItemId) -> f32 {
    let body = run.area.items.get(item).unwrap().body.unwrap();
    run.physics.rapier().bodies[body].linvel().length()
}

/// The first garage interior collider of the audio interior list.
fn garage_interior(run: &GarageRuntime) -> Option<rapier3d::prelude::ColliderHandle> {
    let ids: Vec<_> = run
        .audio
        .as_ref()?
        .reference
        .interiors
        .iter()
        .map(|i| i.collider)
        .collect();
    run.physics
        .rapier()
        .colliders
        .iter()
        .find(|(_, c)| ids.contains(&crate::reference::ColliderId((c.user_data >> 64) as u32)))
        .map(|(handle, _)| handle)
}

/// W5. Source `sandstormscript.Update`, `Move`, `Wind` and `WindScan`; `save_sandstorm`.
#[test]
#[ignore = "requires the installed source export"]
fn tp_w5_storm_wind_interiors_and_save() {
    let (reference, mut run) = load_garage();
    seconds(&mut run, &reference, 1.);
    // Outside the garage, on open sand.
    let spawn = reference.area.player_spawn.translation.map(f64::from);
    act(
        &mut run,
        &reference,
        &json!({"action": "teleport", "x": spawn[0] + 60., "z": spawn[2] + 60.}).to_string(),
    );
    wait_loaded(&mut run, &reference);
    seconds(&mut run, &reference, 2.);
    let bucket = loose_item(&mut run, &reference, 2.);
    seconds(&mut run, &reference, 3.);
    assert!(speed(&run, bucket) < 0.05, "the bucket rests");
    act(
        &mut run,
        &reference,
        &json!({"action": "storm", "distance": 0.}).to_string(),
    );
    let mut moved = 0f32;
    for _ in 0..(6. / crate::simulation::FIXED_DT) as usize {
        frame(&mut run, &reference);
        moved = moved.max(speed(&run, bucket));
    }
    assert_eq!(run.weather.storms().len(), 1);
    assert!(
        run.weather.influence().strength > 0.,
        "the player is in the storm"
    );
    assert!(run.weather.particle_count() > 0, "dust");
    assert!(moved > 0.1, "wind moves a loose item: {moved}");

    // Save and load inside the storm: the storm keeps its values.
    let now = run.simulation_seconds;
    let before = run.weather.snapshot(&reference.weather, now);
    let mut loaded = save_load(&run, &reference);
    let after = loaded
        .weather
        .snapshot(&reference.weather, loaded.simulation_seconds);
    assert_eq!(after.storms.len(), 1);
    assert_eq!(
        [after.storms[0].size, after.storms[0].speed],
        [before.storms[0].size, before.storms[0].speed]
    );
    seconds(&mut loaded, &reference, 1.);
    assert!(
        loaded.weather.influence().strength > 0.,
        "the storm goes on"
    );

    // Inside the garage interior the wind does not move items.
    let (reference, mut run) = load_garage();
    seconds(&mut run, &reference, 2.);
    let bucket = loose_item(&mut run, &reference, 1.);
    let interior = garage_interior(&run).expect("garage interior collider");
    let center = run.physics.rapier().colliders[interior]
        .compute_aabb()
        .center();
    let floor = run.area.player.body_pose(&run.physics).position[1];
    let at = rapier3d::math::Vector::new(center.x, floor + 0.5, center.z);
    crate::world::stream::tests::melee::place(&mut run, bucket, at);
    seconds(&mut run, &reference, 3.);
    assert!(speed(&run, bucket) < 0.05, "the item rests");
    let position =
        rapier3d::math::Vector::from_array(run.area.items.get(bucket).unwrap().pose.position);
    let c = &run.physics.rapier().colliders[interior];
    assert!(
        c.shape().contains_point(c.position(), position),
        "the item is inside"
    );
    act(
        &mut run,
        &reference,
        &json!({"action": "storm", "distance": 0.}).to_string(),
    );
    let mut moved = 0f32;
    for _ in 0..(6. / crate::simulation::FIXED_DT) as usize {
        frame(&mut run, &reference);
        moved = moved.max(speed(&run, bucket));
    }
    assert!(run.weather.influence().strength > 0.);
    assert!(moved < 0.05, "no wind inside the garage: {moved}");
}

/// W14. Source `thumbleweedMain.FixedUpdate`, `Spawn`, `CheckWalls`; `thumbleweedScript.NewWind`.
#[test]
#[ignore = "requires the installed source export"]
fn tp_w14_tumbleweeds_spawn_roll_and_save() {
    let r = &reference().weather.tumbleweeds;
    assert_eq!(
        (
            r.ray_count,
            r.ray_length,
            r.ray_count_to_stay,
            r.limit,
            r.fov
        ),
        (8, 4., 2, 30, 45.)
    );
    assert_eq!(r.spawn_dist_range, [20., 100.]);
    assert_eq!(r.spawn_dist_in_fov_range, [50., 200.]);
    assert_eq!(r.long_spawn_chance, 0.3);
    assert_eq!(r.spawn_height, 2.);
    let (reference, mut run) = load_garage();
    let spawn = reference.area.player_spawn.translation.map(f64::from);
    act(
        &mut run,
        &reference,
        &json!({"action": "teleport", "x": spawn[0] + 200., "z": spawn[2] + 200.}).to_string(),
    );
    wait_loaded(&mut run, &reference);
    let prototype = reference.weather.tumbleweeds.item_id;
    let weeds = |run: &GarageRuntime| -> Vec<(crate::reference::ItemId, [f64; 3])> {
        run.area
            .spawned
            .iter()
            .filter(|s| reference.item_prototypes[s.prototype].item_id == Some(prototype))
            .filter_map(|s| run.area.items.get(s.id).filter(|i| !i.removed))
            .map(|i| (i.id, run.stream.origin.absolute(i.pose.position)))
            .collect()
    };
    seconds(&mut run, &reference, 60.);
    let player = player_absolute(&run);
    let first = weeds(&run);
    assert!(!first.is_empty(), "tumbleweeds spawn");
    assert!(first.len() <= 30);
    for (_, p) in &first {
        let d = ((p[0] - player[0]).powi(2) + (p[2] - player[2]).powi(2)).sqrt();
        assert!(d < 260., "a weed spawns near the player: {d} m");
    }
    seconds(&mut run, &reference, 20.);
    let rolled = weeds(&run)
        .iter()
        .filter(|(id, p)| {
            first.iter().any(|(f, q)| {
                f == id && ((p[0] - q[0]).powi(2) + (p[2] - q[2]).powi(2)).sqrt() > 1.
            })
        })
        .count();
    assert!(rolled > 0, "gusts roll the weeds");
    let before = weeds(&run);
    let loaded = save_load(&run, &reference);
    // A fresh scene tries a spawn on its first fixed step, so one more weed can come.
    let after = weeds(&loaded);
    for (_, p) in &before {
        assert!(
            after
                .iter()
                .any(|(_, q)| (0..3).all(|i| (p[i] - q[i]).abs() < 1.5)),
            "the weed at {p:?} saves with the world"
        );
    }
    // CheckWalls: `ray_count_to_stay` ring hits stop the gust.
    let r = &reference.weather.tumbleweeds;
    for (hits, gust) in [
        (r.ray_count_to_stay, false),
        (r.ray_count_to_stay - 1, true),
    ] {
        let mut weeds = crate::weather::Tumbleweeds::new(1);
        weeds.add(crate::reference::ItemId(1));
        let mut cast = 0;
        weeds.gust(
            r,
            1.,
            0.02,
            |_| Some(rapier3d::math::Vector::ZERO),
            |_, _, _, _| {
                cast += 1;
                (cast <= hits).then_some(1.)
            },
        );
        assert_eq!(weeds.forces().count() > 0, gust, "{hits} wall hits");
    }
}

/// W17. Source far move: `building('Benzinkut01')` streams the area without a material error.
#[test]
#[ignore = "requires the installed source export"]
fn tp_w17_far_move_loads_without_material_error() {
    let (reference, mut run) = load_garage();
    seconds(&mut run, &reference, 1.);
    act(
        &mut run,
        &reference,
        &json!({"action": "building", "name": "Benzinkut01"}).to_string(),
    );
    // `frame` asserts the stream has no failure each step.
    wait_loaded(&mut run, &reference);
    seconds(&mut run, &reference, 5.);
    assert!(run.world_supported());
    assert!(run.stream.frame().failure.is_none());
}

/// The road position of the `skip`-th building whose root name contains `name`.
pub(super) fn road_building(
    run: &GarageRuntime,
    reference: &ReferenceManifest,
    name: &str,
    skip: usize,
) -> [f64; 3] {
    let mut sequence = crate::world::BuildingSequence::new(&reference.world, run.stream.seed());
    std::iter::from_fn(|| sequence.next())
        .take(4000)
        .filter(|p| {
            reference
                .world
                .prototypes
                .iter()
                .find(|w| w.source == p.prototype)
                .and_then(|w| w.nodes.iter().find(|n| n.parent.is_none()))
                .is_some_and(|n| n.name.to_lowercase().contains(name))
        })
        .nth(skip)
        .unwrap_or_else(|| panic!("building {name} {skip}"))
        .pose
        .position
}

/// W17. A far move next to the first gas station on the road (about 7.7 km out) loads. The road
/// grows past the 10 km start road once per world step.
#[test]
#[ignore = "requires the installed source export"]
fn tp_w17_far_move_to_the_first_gas_station_loads() {
    let (reference, mut run) = load_garage();
    seconds(&mut run, &reference, 1.);
    let at = road_building(&run, &reference, "benzinkut", 0);
    act(
        &mut run,
        &reference,
        &json!({"action": "teleport", "x": at[0], "z": at[2] + 25.}).to_string(),
    );
    wait_loaded(&mut run, &reference);
}

fn melee_hits(run: &GarageRuntime) -> usize {
    run.audio.as_ref().map_or(0, |a| {
        a.survival_events
            .iter()
            .filter(|e| e.kind == crate::audio::AudioKind::MeleeHit)
            .count()
    })
}

/// W11. Source `weaponhandlingscript` attack choice and `meleeweaponscript.Update` hit:
/// force on a loose body, spark particles on metal, sand particles on layer 17, hit sound.
#[test]
#[ignore = "requires the installed source export"]
fn tp_w11_shovel_swings_hits_and_pushes() {
    use crate::simulation::{FIXED_DT, FrameInput, keys};
    use crate::world::stream::tests::melee::{
        SWING_FRAMES, equip, place, spawn, swing_frame, trigger, weapon,
    };
    let (reference, mut run) = load_garage();
    seconds(&mut run, &reference, 1.);
    let shovel = reference.melee.shovel(&reference).unwrap();
    let item = equip(&mut run, &reference, shovel);
    // S with A or D is a stab, then D, A, S, else front. Types: 1 front, 2 left, 3 right,
    // 4 up (back key), 5 stab.
    for (held, expected) in [
        (0, 1),
        (keys::LEFT, 2),
        (keys::RIGHT, 3),
        (keys::BACK, 4),
        (keys::BACK | keys::LEFT, 5),
    ] {
        let input = FrameInput {
            held: held | keys::PRIMARY,
            pressed: keys::PRIMARY,
            ..Default::default()
        };
        crate::world::stream::tests::g08_frame(&mut run, &reference, input, FIXED_DT);
        assert_eq!(
            run.area.interaction.melee.attack_type(),
            expected,
            "keys {held}"
        );
        let mut swing = 0;
        for i in 1..SWING_FRAMES {
            let input = if i == 1 {
                FrameInput {
                    held,
                    released: keys::PRIMARY,
                    ..Default::default()
                }
            } else {
                FrameInput {
                    held,
                    ..Default::default()
                }
            };
            crate::world::stream::tests::g08_frame(&mut run, &reference, input, FIXED_DT);
            swing += melee_hits(&run);
            frame(&mut run, &reference);
        }
        assert!(swing > 0, "keys {held}: the swing sounds");
    }
    // A loose metal item in the swing gets pushed, sparks fly and a hit sounds.
    let target = spawn(&mut run, &reference, weapon(&reference, SHOVEL));
    seconds(&mut run, &reference, 1.);
    let start = run.area.items.get(target).unwrap().pose.position;
    let (mut sparks, mut sounds) = (0, 0);
    for i in 0..2 * SWING_FRAMES {
        let at = trigger(&run, &reference, shovel, item).translation;
        if i % SWING_FRAMES < 25 {
            place(&mut run, target, at);
        }
        swing_frame(&mut run, &reference, i);
        sounds += melee_hits(&run);
        sparks = sparks.max(run.area.interaction.melee.particles(0).len());
        frame(&mut run, &reference);
    }
    assert!(sparks > 0, "metal hits spark");
    assert!(sounds > 2, "swing and hit sounds: {sounds}");
    let end = run.area.items.get(target).unwrap().pose.position;
    let moved = (0..3)
        .map(|k| (end[k] - start[k]).powi(2))
        .sum::<f32>()
        .sqrt();
    assert!(moved > 0.05, "the hit pushes the item: {moved} m");
}

/// Swings weapon `w`, item `item` in the hand, at a buried shovel copy. True when the copy comes up.
fn digs_up(
    run: &mut GarageRuntime,
    reference: &ReferenceManifest,
    w: &crate::reference::MeleeWeaponReference,
    item: crate::reference::ItemId,
) -> bool {
    use crate::world::stream::tests::melee::{SWING_FRAMES, place, spawn, swing_frame, trigger};
    let target = spawn(run, reference, reference.melee.shovel(reference).unwrap());
    seconds(run, reference, 1.);
    crate::interaction::buried::bury(target, &mut run.area.items, &mut run.physics);
    for i in 0..4 * SWING_FRAMES {
        let at = trigger(run, reference, w, item).translation;
        place(run, target, at);
        swing_frame(run, reference, i);
        frame(run, reference);
        if !run.area.items.get(target).unwrap().buried {
            return true;
        }
    }
    false
}

/// W12. Source `meleeweaponscript.Update`: only a weapon with `shovelSpeed` lifts a buried item.
/// The shovel digs a buried shovel copy up. A crowbar hit leaves it buried.
#[test]
#[ignore = "requires the installed source export"]
fn tp_w12_a_crowbar_hit_does_not_dig_up_a_buried_item() {
    use crate::world::stream::tests::melee::{equip, weapon};
    let (reference, mut run) = load_garage();
    seconds(&mut run, &reference, 1.);
    let shovel = reference.melee.shovel(&reference).unwrap();
    let item = equip(&mut run, &reference, shovel);
    assert!(
        digs_up(&mut run, &reference, shovel, item),
        "the shovel digs"
    );
    super::weapons::drop_hand(&mut run);
    seconds(&mut run, &reference, 0.2);
    // The crowbar is a world prototype only. The sandbox places it.
    let crowbar = weapon(&reference, &["crowbar", "feszito"]);
    assert_eq!(crowbar.shovel_speed, 0.);
    let item = super::weapons::spawn(&mut run, &reference, "crowbar");
    super::weapons::hold(&mut run, &reference, item);
    assert!(
        !digs_up(&mut run, &reference, crowbar, item),
        "the crowbar does not dig"
    );
}

/// W12. Source `tosaveitemscript.UpdBuried`, `IsBuriedRay`, `UnBury`; `pickupable.CanPickup`
/// refuses `IsBuried`. The save keeps `buried`.
#[test]
#[ignore = "requires the installed source export"]
fn tp_w12_dig_up_refuse_pickup_and_save() {
    use crate::interaction::items::ItemLocation;
    use crate::world::stream::tests::melee::{
        SWING_FRAMES, equip, place, spawn, swing_frame, trigger, weapon,
    };
    let (reference, mut run) = load_garage();
    seconds(&mut run, &reference, 1.);
    let shovel = reference.melee.shovel(&reference).unwrap();
    // The kept item is a loose garage world item; the dug one is a shovel copy.
    let focus =
        rapier3d::math::Vector::from_array(run.area.player.body_pose(&run.physics).position);
    let (key, kept) = run
        .area
        .items
        .generated_bindings()
        .filter(|(_, i)| i.location == ItemLocation::World && !i.removed)
        .filter(|(_, i)| {
            i.body
                .is_some_and(|b| run.physics.rapier().bodies[b].is_dynamic())
        })
        .min_by(|(_, a), (_, b)| {
            let d = |i: &crate::interaction::items::ItemState| {
                (rapier3d::math::Vector::from_array(i.pose.position) - focus).length()
            };
            d(a).total_cmp(&d(b))
        })
        .map(|(key, i)| (crate::world::ObjectKey::Generated(key.clone()), i.id))
        .expect("a loose garage item");
    let dug = spawn(&mut run, &reference, weapon(&reference, SHOVEL));
    seconds(&mut run, &reference, 1.);
    for id in [kept, dug] {
        crate::interaction::buried::bury(id, &mut run.area.items, &mut run.physics);
    }
    // Move the kept item under the floor: no collider lies below it.
    let at = run.area.items.get(kept).unwrap().pose.position;
    place(
        &mut run,
        kept,
        rapier3d::math::Vector::new(at[0], at[1] - 3., at[2]),
    );
    let point = run.area.items.get(kept).unwrap().pose.position;
    let refused = run.area.interaction.pickup_item(
        kept,
        point,
        &run.area.player,
        &mut run.area.items,
        &mut run.physics,
    );
    assert!(refused.is_err(), "a buried item refuses pickup");
    let item = equip(&mut run, &reference, shovel);
    for i in 0..4 * SWING_FRAMES {
        let at = trigger(&run, &reference, shovel, item).translation;
        place(&mut run, dug, at);
        swing_frame(&mut run, &reference, i);
        frame(&mut run, &reference);
        if !run.area.items.get(dug).unwrap().buried {
            break;
        }
    }
    let state = run.area.items.get(dug).unwrap();
    assert!(!state.buried, "the shovel digs the item up");
    let body = state.body.unwrap();
    assert!(
        run.physics.rapier().bodies[body].is_dynamic(),
        "it is loose"
    );
    seconds(&mut run, &reference, 2.);
    assert!(run.area.items.get(kept).unwrap().buried);

    let loaded = save_load(&run, &reference);
    let state = |id: crate::reference::ItemId| {
        let i = loaded.area.items.get(id).unwrap();
        let dynamic = loaded.physics.rapier().bodies[i.body.unwrap()].is_dynamic();
        (i.buried, dynamic)
    };
    let kept = loaded
        .area
        .items
        .resolve_key(&key)
        .expect("the buried item reloads");
    assert_eq!(state(kept), (true, false), "the buried item stays buried");
    // Spawned items reload by their spawn serial.
    let serial = run.area.spawned.iter().position(|s| s.id == dug).unwrap();
    let dug = loaded.area.spawned.iter().nth(serial).unwrap().id;
    assert_eq!(state(dug), (false, true), "the dug item stays loose");
}

/// Calls `hand` until the world prototype item loads. Returns the item.
pub(super) fn hand(
    run: &mut GarageRuntime,
    reference: &ReferenceManifest,
    name: &str,
) -> crate::reference::ItemId {
    for _ in 0..600 {
        let data = act(
            run,
            reference,
            &json!({"action": "hand", "name": name}).to_string(),
        );
        if let Some(id) = data.get(0).and_then(serde_json::Value::as_u64) {
            return crate::reference::ItemId(id as u32);
        }
        for _ in 0..5 {
            frame(run, reference);
        }
    }
    panic!("{name} does not load");
}

fn detector_loop(run: &GarageRuntime, item: crate::reference::ItemId) -> Option<(f32, f32)> {
    run.audio.as_ref().and_then(|a| {
        a.survival_loops
            .iter()
            .find(|e| e.kind == crate::audio::AudioKind::MetalDetector && e.emitter == item.0)
            .map(|e| (e.pitch, e.gain))
    })
}

/// Moves `item` so that the center of its Metal collider is at `at`.
fn place_metal(
    run: &mut GarageRuntime,
    item: crate::reference::ItemId,
    at: rapier3d::math::Vector,
) {
    let body = run.area.items.get(item).unwrap().body.unwrap();
    let rapier = run.physics.rapier();
    let pose = *rapier.bodies[body].position();
    let center = rapier.bodies[body]
        .colliders()
        .iter()
        .map(|c| &rapier.colliders[*c])
        .find(|c| {
            crate::vehicle::collider_tag(c.user_data) == Some(crate::reference::ColliderTag::Metal)
        })
        .expect("a metal collider")
        .compute_aabb()
        .center();
    crate::world::stream::tests::melee::place(run, item, at - (center - pose.translation));
}

/// W13. Source `metalDetectorScript.Turn`, `Update`, `UpdSound`, `Scan`, `UpdMeter`. The wall rule
/// is `metal_ahead_beeps_and_a_wall_in_front_weakens_it`.
#[test]
#[ignore = "requires the installed source export"]
fn tp_w13_detector_beeps_near_metal_and_saves_on() {
    use crate::world::stream::tests::melee::{spawn, weapon};
    let (reference, mut run) = load_garage();
    seconds(&mut run, &reference, 1.);
    let detector = hand(&mut run, &reference, "metalDetector");
    seconds(&mut run, &reference, 1.);
    let probe = |run: &GarageRuntime| {
        run.stream
            .detector_probe(detector, &run.area.items)
            .expect("detector binding")
    };
    let (on, _, _, _, knob) = probe(&run);
    assert!(!on, "the detector starts off");
    assert!(detector_loop(&run, detector).is_none(), "off: no beep");
    assert!(run.stream.switch_detector(knob, &mut run.area.items));
    seconds(&mut run, &reference, 1.);
    let (on, quiet, _, _, _) = probe(&run);
    assert!(on);
    let (low_pitch, _) = detector_loop(&run, detector).expect("on: the beep loop plays");
    // Metal in front of the detector raises the value, the pitch, the volume and the needle.
    let metal = spawn(&mut run, &reference, weapon(&reference, SHOVEL));
    let (mut smooth, mut needle) = (0f32, 0f32);
    for _ in 0..(2. / crate::simulation::FIXED_DT) as usize {
        let (_, _, _, pose, _) = probe(&run);
        let at = pose.translation + pose.rotation * rapier3d::math::Vector::NEG_Z * 0.6;
        place_metal(&mut run, metal, at);
        frame(&mut run, &reference);
        let (_, s, n, _, _) = probe(&run);
        (smooth, needle) = (smooth.max(s), needle.max(n));
    }
    assert!(
        smooth > quiet + 0.05,
        "metal raises the value: {quiet} -> {smooth}"
    );
    assert!(needle > 0.01, "the needle moves: {needle}");
    let (pitch, gain) = detector_loop(&run, detector).unwrap();
    assert!(pitch > low_pitch, "higher pitch near metal");
    assert!(gain > 0., "audible near metal");
    // Save and load with the detector on.
    let key = run.area.items.object_key(detector);
    let loaded = save_load(&run, &reference);
    let id = loaded
        .area
        .items
        .resolve_key(&key)
        .expect("detector reloads");
    let (on, _, _, _, _) = loaded
        .stream
        .detector_probe(id, &loaded.area.items)
        .expect("detector binding after load");
    assert!(on, "the detector stays on");
    // Off again: the beep stops.
    let mut run = loaded;
    let (_, _, _, _, knob) = run.stream.detector_probe(id, &run.area.items).unwrap();
    assert!(run.stream.switch_detector(knob, &mut run.area.items));
    seconds(&mut run, &reference, 0.5);
    assert!(detector_loop(&run, id).is_none(), "off: the beep stops");
}

/// W9. Source `buildingscript.SPAWN` and `newRandomStuff.Spawn`: building loot spawns gas and
/// oil cans, beer, barrels, buckets, the WC and loot flashlights. A flashlight does not stop
/// the world (`frame` asserts no stream failure).
#[test]
#[ignore = "requires the installed source export"]
fn tp_w9_building_loot_spawns_the_named_items() {
    let (reference, mut run) = load_garage();
    seconds(&mut run, &reference, 1.);
    // World prototype root names: gas can, oil can, beer, barrel, bucket, WC, old flashlight.
    let wanted = [
        "benzineskanna",
        "OilCan",
        "Beer",
        "BARREL",
        "Vodor",
        "wc",
        "regizseblampa",
    ];
    let sources: Vec<_> = wanted
        .iter()
        .map(|name| {
            reference
                .world
                .prototypes
                .iter()
                .find(|w| {
                    w.nodes
                        .iter()
                        .any(|n| n.parent.is_none() && n.name == *name)
                })
                .map(|w| w.source.clone())
                .unwrap_or_else(|| panic!("world prototype {name}"))
        })
        .collect();
    let mut found = [false; 7];
    let mut sequence = crate::world::BuildingSequence::new(&reference.world, run.stream.seed());
    let mut visited = Vec::new();
    for placement in std::iter::from_fn(|| sequence.next()).take(400) {
        if found.iter().all(|f| *f) || visited.len() >= 16 {
            break;
        }
        let Some(name) = reference
            .world
            .prototypes
            .iter()
            .find(|w| w.source == placement.prototype)
            .and_then(|w| w.nodes.iter().find(|n| n.parent.is_none()))
            .map(|n| n.name.clone())
        else {
            continue;
        };
        if visited.contains(&name) {
            continue;
        }
        visited.push(name.clone());
        // `building` moves to the first building on the road with this name: this one.
        let p = act(
            &mut run,
            &reference,
            &json!({"action": "building", "name": name}).to_string(),
        );
        let p: [f64; 3] = serde_json::from_value(p).unwrap();
        // The first gas station stalls: `tp_w17_far_move_to_the_first_gas_station_loads`.
        if !loads(&mut run, &reference) {
            act(&mut run, &reference, r#"{"action":"garage"}"#);
            wait_loaded(&mut run, &reference);
            continue;
        }
        seconds(&mut run, &reference, 2.);
        for s in run.stream.frame().structures {
            let q = s.placement.pose.position;
            if (q[0] - p[0]).hypot(q[2] - p[2]) > 120. {
                continue;
            }
            if let Some(i) = sources.iter().position(|w| *w == s.placement.prototype) {
                found[i] = true;
            }
        }
    }
    let missing: Vec<_> = wanted
        .iter()
        .zip(found)
        .filter(|(_, f)| !f)
        .map(|(n, _)| *n)
        .collect();
    assert!(
        missing.is_empty(),
        "no loot {missing:?} in {} buildings",
        visited.len()
    );
}

/// W16. Source draw distance setting `mainscript.viewDistance`. A 5 min road trip at 25 m/s,
/// at 500 m and at the largest fitting draw distance. `frame` replays the renderer claims and
/// asserts that `offer_surface` never fails the stream.
#[test]
#[ignore = "requires the installed source export"]
fn tp_w16_road_trip_stays_in_the_renderer_budget() {
    let reference = reference();
    let largest = load_garage().1.stream.largest_fitting().unwrap();
    assert!(largest > 500.);
    for distance in [500., largest] {
        let mut run = GarageRuntime::new(&reference, GameMode::default(), distance).unwrap();
        prepare_ready(&mut run, &reference);
        run.started = true;
        run.paused = false;
        let path =
            crate::world::stream::tests::long_trip::road_path(&run, &reference, 300. * 25., 25.);
        for point in &path {
            act(
                &mut run,
                &reference,
                &json!({"action": "teleport", "x": point[0], "z": point[2]}).to_string(),
            );
            seconds(&mut run, &reference, 1.);
        }
        let end = player_absolute(&run);
        let last = path.last().unwrap();
        assert!(
            (end[0] - last[0]).hypot(end[2] - last[2]) < 30.,
            "{distance} m: the trip ends on the road"
        );
        assert!(run.stream.frame().failure.is_none(), "{distance} m");
    }
}

/// W5. Source `sandstormscript.Wind` and `WindScan`: the wind does not move an item inside a car.
#[test]
#[ignore = "requires the installed source export"]
fn tp_w5_wind_does_not_move_items_in_a_car() {
    let (reference, mut run) = load_garage();
    seconds(&mut run, &reference, 1.);
    let spawn = reference.area.player_spawn.translation.map(f64::from);
    // The teleport moves the start car into a row next to the player.
    act(
        &mut run,
        &reference,
        &json!({"action": "teleport", "x": spawn[0] + 60., "z": spawn[2] + 60.}).to_string(),
    );
    wait_loaded(&mut run, &reference);
    seconds(&mut run, &reference, 2.);
    act(
        &mut run,
        &reference,
        &json!({"action": "seat", "seat": "driver"}).to_string(),
    );
    seconds(&mut run, &reference, 1.);
    // The passenger seat mirrors the driver seat across the car center line.
    let key = run.vehicle.clone().unwrap();
    let chassis = *run.physics.rapier().bodies[run.physics.chassis_handle(&key)].position();
    let driver =
        rapier3d::math::Vector::from_array(run.area.player.body_pose(&run.physics).position);
    let local = chassis.inverse() * driver;
    let seat = (chassis * local.with_x(-local.x)).to_array();
    let food = find(&run, "Kolbasz");
    let at = rapier3d::math::Vector::from_array(seat) + rapier3d::math::Vector::Y * 0.3;
    crate::world::stream::tests::melee::place(&mut run, food, at);
    seconds(&mut run, &reference, 3.);
    assert!(speed(&run, food) < 0.05, "the item rests on the seat");
    let body = run.area.items.get(food).unwrap().body.unwrap();
    assert!(
        run.physics.rapier().bodies[body].is_enabled(),
        "the item simulates"
    );
    act(
        &mut run,
        &reference,
        &json!({"action": "storm", "distance": 0.}).to_string(),
    );
    // The wind rocks the car. The item must not move on its seat.
    let in_car = |run: &GarageRuntime| {
        let chassis = *run.physics.rapier().bodies[run.physics.chassis_handle(&key)].position();
        let p = run.area.items.get(food).unwrap().pose.position;
        chassis.inverse() * rapier3d::math::Vector::from_array(p)
    };
    let start = in_car(&run);
    let mut moved = 0f32;
    for _ in 0..(6. / crate::simulation::FIXED_DT) as usize {
        frame(&mut run, &reference);
        moved = moved.max((in_car(&run) - start).length());
    }
    assert!(
        run.weather.influence().strength > 0.,
        "the player is in the storm"
    );
    assert!(
        moved < 0.05,
        "no wind inside the car: the item moves {moved} m on the seat"
    );
}
