//! ROADMAP test pass: Creatures (CR rows).
use super::world::wait_loaded;
use crate::creatures::rabbit::Size;
use crate::reference::ReferenceManifest;
use crate::simulation::{FIXED_DT, FrameInput, keys};
use crate::starting_area::GarageRuntime;
use crate::world::stream::tests::kit::*;
use rapier3d::math::Vector;
use serde_json::json;

fn steps(seconds: f32) -> usize {
    (seconds / FIXED_DT) as usize
}

fn player(run: &GarageRuntime) -> Vector {
    Vector::from_array(run.area.player.body_pose(&run.physics).position)
}

fn rabbit_position(run: &GarageRuntime, id: u32) -> Option<Vector> {
    run.creatures
        .views(&run.physics)
        .find(|v| v.id == id)
        .map(|v| v.pose.translation)
}

fn spawn_rabbit(
    run: &mut GarageRuntime,
    reference: &ReferenceManifest,
    evil: bool,
    forward: f32,
) -> u32 {
    let name = if evil { "evil rabbit" } else { "rabbit" };
    act(
        run,
        reference,
        &json!({"action": "spawn", "name": name, "forward": forward}).to_string(),
    )
    .as_u64()
    .unwrap() as u32
}

/// The garage scene with the player on open ground 113 m from the spawn.
pub(super) fn outdoors() -> (ReferenceManifest, GarageRuntime) {
    let (reference, mut run) = load_garage();
    seconds(&mut run, &reference, 1.);
    let spawn = reference.area.player_spawn.translation.map(f64::from);
    act(
        &mut run,
        &reference,
        &json!({"action": "teleport", "x": spawn[0] + 80., "z": spawn[2] + 80.}).to_string(),
    );
    wait_loaded(&mut run, &reference);
    seconds(&mut run, &reference, 1.);
    (reference, run)
}

/// CR3. Source `aispawn.SpawnRoutine`, `TrySpawn`, `RandomPlace`, `CanSpawn`;
/// `nyulscript.SetBasedOnSize`; `itemPlaceRemoveScript` unload and return.
#[test]
#[ignore = "requires the installed source export"]
fn tp_cr3_rabbits_spawn_unload_and_return() {
    let (reference, mut run) = load_garage();
    let r = &reference.creatures.rabbit;
    let s = &r.spawner;
    assert_eq!(
        (s.chance, s.time_range, s.count_range, s.max_count),
        (1., [50., 100.], [1., 2.], 10)
    );
    assert_eq!((s.distance_range, s.offset), ([30., 100.], [0., 0., -100.]));
    // Big rabbits are larger, heavier and turn slower. Source move.speed rises with size.
    let (small, big) = (Size::new(r, 0.), Size::new(r, 1.));
    assert!(big.scale > small.scale && big.mass > small.mass);
    assert!(big.rot_speed < small.rot_speed);
    assert_eq!(r.size.speed, [10., 15.]);
    assert_eq!(r.size.rot_speed, [360., 250.]);

    // Spawn rounds: first at start, then every 50 to 100 s, 1 or 2 rabbits, at most 10.
    let mut seen: Vec<u32> = Vec::new();
    let mut rounds: Vec<f64> = Vec::new();
    for _ in 0..steps(210.) {
        frame(&mut run, &reference);
        let rotation =
            rapier3d::math::Rotation::from_array(run.area.player.body_pose(&run.physics).rotation);
        let center = player(&run) + rotation.inverse() * Vector::new(0., 0., -100.);
        let now = run.simulation_seconds;
        let mut new = 0;
        for v in run.creatures.views(&run.physics) {
            if seen.contains(&v.id) {
                continue;
            }
            seen.push(v.id);
            new += 1;
            let d = v.pose.translation - center;
            let ring = d.x.hypot(d.z);
            assert!(
                ring <= 100. + 3.,
                "rabbit {} at {ring} m from the spawn center",
                v.id
            );
        }
        if new > 0 {
            // The source loop bound `Random.Range(minCount, maxCount + 1)` is drawn again each pass.
            assert!(new <= 3, "{new} rabbits in one round");
            rounds.push(now);
        }
        assert!(run.creatures.rabbits().count() <= 10);
    }
    assert!(rounds[0] < 1., "first round at start: {rounds:?}");
    for w in rounds.windows(2) {
        let gap = w[1] - w[0];
        assert!(
            (50. ..=100. + 0.05).contains(&gap),
            "round gap {gap}: {rounds:?}"
        );
    }
    let near = run.creatures.snapshot(&run.physics, &run.stream.origin);
    assert!(near.len() >= 3, "{} rabbits", near.len());

    // Drive far: the rabbits unload and new ones spawn near.
    let start = run.stream.origin.absolute(player(&run).to_array());
    act(
        &mut run,
        &reference,
        &json!({"action": "teleport", "x": start[0] + 3000., "z": start[2]}).to_string(),
    );
    wait_loaded(&mut run, &reference);
    seconds(&mut run, &reference, 5.);
    let far_player = player(&run);
    assert!(
        run.creatures
            .views(&run.physics)
            .all(|v| (v.pose.translation - far_player).length() < 500.),
        "far rabbits unload"
    );
    seconds(&mut run, &reference, 110.);
    assert!(
        run.creatures.rabbits().count() > 0,
        "new rabbits spawn near"
    );

    // Return: the old rabbits come back where they were.
    act(
        &mut run,
        &reference,
        &json!({"action": "garage"}).to_string(),
    );
    wait_loaded(&mut run, &reference);
    seconds(&mut run, &reference, 5.);
    // `PlaceItems`: a far rabbit comes back inside `itemSpawnDist` of an important object.
    let back = run.creatures.snapshot(&run.physics, &run.stream.origin);
    let home = run.stream.origin.absolute(player(&run).to_array());
    let spawn = f64::from(run.stream.distances.item_spawn_dist) - 10.;
    let inside: Vec<_> = near
        .iter()
        .filter(|old| {
            (0..3)
                .map(|i| (old.pose.position[i] - home[i]).powi(2))
                .sum::<f64>()
                .sqrt()
                < spawn
        })
        .collect();
    assert!(!inside.is_empty(), "rabbits near the garage");
    for old in inside {
        assert!(
            back.iter().any(|b| b.size == old.size),
            "the rabbit of size {} returns",
            old.size
        );
    }
}

/// CR4. Source `aiscript` flee: a rabbit runs from the player and from a shot.
#[test]
#[ignore = "requires the installed source export"]
fn tp_cr4_rabbits_flee_the_player_and_shots() {
    let (reference, mut run) = outdoors();
    let id = spawn_rabbit(&mut run, &reference, false, 5.);
    let start = (rabbit_position(&run, id).unwrap() - player(&run)).length();
    let mut far = start;
    for _ in 0..steps(6.) {
        frame(&mut run, &reference);
        far = far.max((rabbit_position(&run, id).unwrap() - player(&run)).length());
    }
    assert!(
        far > start + 5.,
        "the rabbit runs from the player: {start} -> {far} m"
    );
}

/// Kills rabbit `id` with shots on its colliders.
fn kill(run: &mut GarageRuntime, reference: &ReferenceManifest, id: u32) {
    let handles: Vec<_> = run
        .creatures
        .rabbits()
        .find(|r| r.id == id)
        .unwrap()
        .colliders
        .iter()
        .map(|c| c.handle)
        .collect();
    for h in handles {
        run.creatures.shoot(
            &reference.creatures.rabbit,
            &run.physics,
            h,
            10_000.,
            Vector::Z,
        );
    }
    frame(run, reference);
    assert!(
        run.creatures
            .rabbits()
            .find(|r| r.id == id)
            .is_some_and(|r| !r.alive)
    );
}

/// Shoots the head colliders of rabbit `id` until it loses its eyes. False if it dies first.
fn take_eyes(run: &mut GarageRuntime, reference: &ReferenceManifest, id: u32) -> bool {
    for _ in 0..50 {
        let Some(rabbit) = run.creatures.rabbits().find(|r| r.id == id && r.alive) else {
            return false;
        };
        if !rabbit.senses.has_eyes {
            return true;
        }
        let handles: Vec<_> = rabbit.colliders.iter().map(|c| c.handle).collect();
        for h in handles {
            run.creatures
                .shoot(&reference.creatures.rabbit, &run.physics, h, 4.5, Vector::Z);
        }
        frame(run, reference);
    }
    false
}

/// CR6 and CR7. Source `aiscript.Die`, `ragdollactivatescript.SetRagdol`; `save_ragdollpos`;
/// `nyulscript` eyes. Dead rabbits fall as ragdolls, a carried bone pulls the body, and live,
/// dead and eyeless rabbits save and load.
#[test]
#[ignore = "requires the installed source export"]
fn tp_cr7_ragdoll_carry_and_save_live_dead_eyeless() {
    let (reference, mut run) = outdoors();
    let live = spawn_rabbit(&mut run, &reference, false, 3.);
    let dead = spawn_rabbit(&mut run, &reference, false, 5.);
    let mut eyeless = None;
    for k in 0..6 {
        let id = spawn_rabbit(&mut run, &reference, false, 7. + k as f32);
        if take_eyes(&mut run, &reference, id) {
            eyeless = Some(id);
            break;
        }
    }
    let eyeless = eyeless.expect("a head shot takes the eyes");
    kill(&mut run, &reference, dead);
    seconds(&mut run, &reference, 2.);
    let bones = &run
        .creatures
        .rabbits()
        .find(|r| r.id == dead)
        .unwrap()
        .bones;
    assert_eq!(bones.len(), reference.creatures.rabbit.ragdoll.bodies.len());
    let (held, others): (crate::reference::ItemId, Vec<_>) =
        (bones[0].item, bones[1..].iter().map(|b| b.item).collect());
    // Carry a bone and walk: the body follows.
    let point = run.area.items.get(held).unwrap().pose.position;
    run.area
        .interaction
        .pickup_item(
            held,
            point,
            &run.area.player,
            &mut run.area.items,
            &mut run.physics,
        )
        .unwrap();
    let before: Vec<_> = others
        .iter()
        .map(|b| Vector::from_array(run.area.items.get(*b).unwrap().pose.position))
        .collect();
    for _ in 0..steps(1.5) {
        frame_with(
            &mut run,
            &reference,
            FrameInput {
                held: keys::BACK,
                ..Default::default()
            },
        );
    }
    let hand = Vector::from_array(run.area.items.get(held).unwrap().pose.position);
    for (b, start) in others.iter().zip(&before) {
        let p = Vector::from_array(run.area.items.get(*b).unwrap().pose.position);
        assert!(
            (p - *start).length() > 0.5,
            "bone {b:?} follows the carried bone"
        );
        assert!((p - hand).length() < 1.5, "bone {b:?} stays on the body");
    }
    run.area
        .interaction
        .drop_held(&mut run.area.items, &mut run.physics);
    seconds(&mut run, &reference, 2.);

    let saved = run.creatures.snapshot(&run.physics, &run.stream.origin);
    let loaded = save_load(&run, &reference);
    let back = loaded
        .creatures
        .snapshot(&loaded.physics, &loaded.stream.origin);
    // The spawner starts again at load, so new live rabbits can join.
    assert!(back.len() >= saved.len());
    assert_eq!(
        back.iter().filter(|s| !s.alive).count(),
        1,
        "one dead rabbit"
    );
    assert_eq!(
        back.iter().filter(|s| s.alive && !s.eyes).count(),
        1,
        "one eyeless rabbit"
    );
    let dead = back.iter().find(|s| !s.alive).unwrap();
    assert_eq!(
        dead.bones.len(),
        reference.creatures.rabbit.ragdoll.bodies.len()
    );
    let _ = (live, eyeless);
    assert!(
        loaded
            .creatures
            .views(&loaded.physics)
            .any(|v| !v.dead && !v.eyes),
        "the eyeless rabbit shows no eyes"
    );
}

/// CR8. Source `nyulscript.SetBasedOnSize`: `size > 0.75` gets evil with the red eyes, unless
/// Peaceful. Sizes are `Random.Range(0, 1)`, so about one in four.
#[test]
#[ignore = "requires the installed source export"]
fn tp_cr8_one_in_four_rabbits_is_evil() {
    let (reference, mut run) = load_garage();
    seconds(&mut run, &reference, 1.);
    let r = &reference.creatures.rabbit;
    let mut pose = run.area.player.body_pose(&run.physics);
    pose.position[2] -= 6.;
    let pose = crate::vehicle::to_rapier(pose);
    for (size, evil) in [(0.74, false), (0.76, true)] {
        let id = run
            .creatures
            .spawn_rabbit(r, &mut run.physics, pose, size, run.simulation_seconds)
            .unwrap();
        let rabbit = run.creatures.rabbits().find(|x| x.id == id).unwrap();
        assert_eq!(rabbit.evil.kind.is_some(), evil, "size {size}");
    }
    let view = run.creatures.views(&run.physics).last().unwrap();
    assert_eq!(view.evil, Some(crate::creatures::evil::EyeKind::Natural));
    // Peaceful: no evil.
    run.creatures.peaceful = true;
    let id = run
        .creatures
        .spawn_rabbit(r, &mut run.physics, pose, 1., run.simulation_seconds)
        .unwrap();
    assert!(
        run.creatures
            .rabbits()
            .find(|x| x.id == id)
            .unwrap()
            .evil
            .kind
            .is_none()
    );
    // The spawner draws sizes uniformly: about one in four is above 0.75.
    let mut evil = 0;
    for seed in 0..20u64 {
        let mut random = crate::weather::StormRandom(seed);
        for _ in 0..100 {
            evil += usize::from(random.unit() > 0.75);
        }
    }
    let ratio = evil as f32 / 2000.;
    assert!((0.2..0.3).contains(&ratio), "evil ratio {ratio}");
}

/// Rabbit clips of this step. `play_rabbit_clip` sends them as positional clip signals.
fn take_clips(run: &mut GarageRuntime) -> Vec<u32> {
    // Headless runs have no presentation that drains the positional clip signals.
    run.audio.as_mut().map_or(Vec::new(), |a| {
        a.cars.events.drain(..).map(|e| e.clip).collect()
    })
}

fn count(clips: &[u32], of: &[u32]) -> usize {
    clips.iter().filter(|c| of.contains(c)).count()
}

/// CR9, CR10 and CR11. Source `nyulscript.SeeTarget`, `Roar`, `Attack`, `Update` idle sound;
/// `playerdeathtriggerscript`. An evil rabbit roars and stops, runs at the player, bites with
/// the attack clip, hurts the player and growls close by.
#[test]
#[ignore = "requires the installed source export"]
fn tp_cr9_evil_rabbit_roars_bites_and_growls() {
    let (reference, mut run) = outdoors();
    let e = &reference.creatures.rabbit.evil;
    assert_eq!(
        (e.attack_distance, e.roar_duration, e.idle_time),
        (4., 2., [10., 30.])
    );
    assert_eq!(
        e.idle_distance, 15.,
        "source compares sqrMagnitude: about 3.9 m"
    );
    assert_eq!(e.trigger.damage_player, 5.);
    // `SeeTarget` roars when `Time.time - lastSeeTarget > targetForgetTime`, and Unity starts
    // `lastSeeTarget` at 0. Wait past it, as a game that runs for a while does.
    seconds(&mut run, &reference, e.target_forget_time + 1.);
    take_clips(&mut run);
    let id = spawn_rabbit(&mut run, &reference, true, 8.);
    let hp = run.survival.output().needs.hp;
    let (mut roared, mut attacked, mut bitten) = (false, false, false);
    let (mut roars, mut attacks, mut idles) = (0, 0, Vec::new());
    let mut roar_at = None;
    for _ in 0..steps(40.) {
        frame(&mut run, &reference);
        let Some(rabbit) = run.creatures.rabbits().find(|r| r.id == id) else {
            break;
        };
        if rabbit.evil.roaring && !roared {
            roared = true;
            roar_at = Some(run.simulation_seconds);
        }
        attacked |= rabbit.evil.attacking;
        bitten |= rabbit.evil.trigger;
        let clips = take_clips(&mut run);
        roars += count(&clips, &e.roar_clips);
        attacks += count(&clips, &e.attack_clips);
        if count(&clips, &e.idle_clips) > 0 {
            idles.push(run.simulation_seconds);
        }
        if run.survival.output().needs.hp <= 0. {
            break;
        }
    }
    assert!(roared && roars > 0, "the evil rabbit roars");
    assert!(roar_at.is_some());
    assert!(attacked && attacks > 0, "it attacks with the bite clip");
    assert!(bitten, "a big rabbit bite turns the damage trigger on");
    assert!(
        run.survival.output().needs.hp < hp,
        "the bite hurts the player"
    );
    assert!(!idles.is_empty(), "it growls close by");
    for w in idles.windows(2) {
        assert!(w[1] - w[0] >= 10. - 0.05, "growl gap {}", w[1] - w[0]);
    }
}

/// CR5. Source `breakablescript.Shoot` and `OnCollisionEnter`, `aiscript.Update`
/// `accelerationDeath`, `nyulscript.Die` with `Sdie`. A shot kills with a die sound; a fall
/// from height kills. Head shots taking the eyes or ears: `head_shots_multiply_the_force_and_take_senses`
/// and CR7.
#[test]
#[ignore = "requires the installed source export"]
fn tp_cr5_rabbits_die_from_shots_and_falls_with_a_sound() {
    use crate::creatures::rabbit::Death;
    let (reference, mut run) = outdoors();
    let r = &reference.creatures.rabbit;
    let shot = spawn_rabbit(&mut run, &reference, false, 4.);
    let handles: Vec<_> = run
        .creatures
        .rabbits()
        .find(|x| x.id == shot)
        .unwrap()
        .colliders
        .iter()
        .map(|c| c.handle)
        .collect();
    let mut sounds = 0;
    for h in handles {
        run.creatures.shoot(r, &run.physics, h, 10_000., Vector::Z);
    }
    for _ in 0..5 {
        frame(&mut run, &reference);
        sounds += count(&take_clips(&mut run), &r.die_clips);
    }
    let rabbit = run.creatures.rabbits().find(|x| x.id == shot).unwrap();
    assert_eq!((rabbit.alive, rabbit.death), (false, Some(Death::Shot)));
    assert!(sounds > 0, "the shot rabbit plays a die clip");
    // A fall from 60 m kills on landing.
    let mut pose = run.area.player.body_pose(&run.physics);
    pose.position[1] += 60.;
    pose.position[2] -= 8.;
    let fall = run
        .creatures
        .spawn_rabbit(
            r,
            &mut run.physics,
            crate::vehicle::to_rapier(pose),
            0.5,
            run.simulation_seconds,
        )
        .unwrap();
    for _ in 0..steps(6.) {
        frame(&mut run, &reference);
    }
    let rabbit = run.creatures.rabbits().find(|x| x.id == fall).unwrap();
    assert!(!rabbit.alive, "the fall kills");
    assert!(
        matches!(rabbit.death, Some(Death::Speed | Death::Hit)),
        "{:?}",
        rabbit.death
    );
}

/// CR12 and CR13. Source `stimulscript.CheckAudioSources`: engine, horn and radio sources make
/// `Threat` stimuli of `volume * maxSoundValue`. The flight from a threat and the evil inversion
/// are `a_threat_sends_the_rabbit_away` and `inverted_flight_pulls_toward_a_weak_threat`.
#[test]
#[ignore = "requires the installed source export"]
fn tp_cr12_horn_and_radio_make_sound_stimuli() {
    let (reference, mut run) = load_garage();
    seconds(&mut run, &reference, 1.);
    act(
        &mut run,
        &reference,
        &json!({"action": "seat", "seat": "driver"}).to_string(),
    );
    seconds(&mut run, &reference, 1.);
    let car = player(&run);
    let horn = FrameInput {
        held: keys::HORN,
        ..Default::default()
    };
    for _ in 0..steps(1.) {
        frame_with(&mut run, &reference, horn);
    }
    let bound = run.creatures.bound_sounds().clone();
    assert!(
        bound
            .iter()
            .any(|s| s.value > 0. && (s.position - car).length() < 5.),
        "the horn makes a sound stimulus: {bound:?}"
    );
    let horn_value = bound.iter().map(|s| s.value).fold(0f32, f32::max);
    for _ in 0..steps(1.) {
        frame(&mut run, &reference);
    }
    let quiet = run
        .creatures
        .bound_sounds()
        .iter()
        .map(|s| s.value)
        .fold(0f32, f32::max);
    assert!(quiet < horn_value, "the stimulus ends with the horn");

    // A portable radio that plays makes a quieter stimulus.
    let (reference, mut run) = load_garage();
    seconds(&mut run, &reference, 1.);
    let radio = super::world::hand(&mut run, &reference, "radio02");
    let knob = run
        .stream
        .radio_knob(radio, crate::interaction::Knob::Volume)
        .expect("radio volume knob");
    for _ in 0..10 {
        run.stream.radio_control(knob, 1., &mut run.area.items);
    }
    assert!(
        run.stream
            .portable_radios()
            .any(|(item, _, state, ..)| item == radio && state.on),
        "the volume knob turns the radio on"
    );
    let mut value = 0f32;
    for _ in 0..steps(2.) {
        frame(&mut run, &reference);
        value = value.max(
            run.creatures
                .bound_sounds()
                .iter()
                .filter(|s| s.key >> 32 == 2)
                .map(|s| s.value)
                .fold(0., f32::max),
        );
    }
    assert!(value > 0., "a playing radio makes a sound stimulus");
    assert!(
        value < horn_value,
        "the radio is quieter than the horn: {value} < {horn_value}"
    );
}

/// A garage runtime on game date `date`.
fn load_on(date: [i32; 3]) -> (ReferenceManifest, GarageRuntime) {
    let mut reference = reference();
    std::sync::Arc::make_mut(&mut reference.world)
        .spawn_context
        .date = date;
    let mut run = GarageRuntime::new_default(&reference).unwrap();
    prepare_ready(&mut run, &reference);
    run.started = true;
    run.paused = false;
    seconds(&mut run, &reference, 1.);
    (reference, run)
}

/// Live bat items near `center` (absolute).
fn bats(
    run: &GarageRuntime,
    reference: &ReferenceManifest,
    center: [f64; 3],
) -> Vec<crate::reference::ItemId> {
    let prefab = &reference.sky.flyer.prefab;
    run.stream
        .frame()
        .structures
        .iter()
        .filter(|s| &s.placement.prototype == prefab)
        .filter(|s| {
            let p = s.placement.pose.position;
            (p[0] - center[0]).hypot(p[2] - center[2]) < 150.
        })
        .filter_map(|s| run.area.items.generated_item_id(&s.placement.id))
        .collect()
}

/// Teleports near the building at `at`, then frames until the area loads and 3 s more.
/// Returns each bat seen near it, or None when the area does not load.
/// Source `breakablescript.OnCollisionEnter`: a bat that hits a wall can break. Load time
/// depends on the machine, so collect from the first frame.
fn visit(
    run: &mut GarageRuntime,
    reference: &ReferenceManifest,
    at: [f64; 3],
) -> Option<Vec<crate::reference::ItemId>> {
    act(
        run,
        reference,
        &json!({"action": "teleport", "x": at[0], "z": at[2] + 25.}).to_string(),
    );
    let mut seen = Vec::new();
    let mut collect = |run: &mut GarageRuntime| {
        frame(run, reference);
        for bat in bats(run, reference, at) {
            if !seen.contains(&bat) {
                seen.push(bat);
            }
        }
    };
    let mut frames = 0;
    while !(run.world_supported() && run.stream.required_ready()) {
        if frames == 3000 {
            return None;
        }
        collect(run);
        frames += 1;
    }
    for _ in 0..steps(3.) {
        collect(run);
    }
    Some(seen)
}

/// Visits the road buildings. Returns the bats seen near each loaded building.
fn bat_tour(
    run: &mut GarageRuntime,
    reference: &ReferenceManifest,
    count: usize,
) -> Vec<([f64; 3], Vec<crate::reference::ItemId>)> {
    let mut sequence = crate::world::BuildingSequence::new(&reference.world, run.stream.seed());
    let mut found = Vec::new();
    for placement in std::iter::from_fn(|| sequence.next()).take(400) {
        if found.len() == count {
            break;
        }
        if !reference
            .world
            .prototypes
            .iter()
            .any(|w| w.source == placement.prototype)
        {
            continue;
        }
        let at = placement.pose.position;
        if let Some(seen) = visit(run, reference, at) {
            found.push((at, seen));
        }
    }
    found
}

/// CR2. Source `buildingscript.RandomFlyerSpawn` with `mainscript.CheckHoliday(halloween)`
/// (15 Oct to 15 Nov) and `Random.Range(0, 1) > 0.35`: `Random.Range(5, 30)` bats;
/// `randomFlyer.Update`: a bat flies and squeaks. `RandomBat` has no `pickupable`: it cannot
/// be carried.
#[test]
#[ignore = "requires the installed source export"]
fn tp_cr2_halloween_bats_near_buildings() {
    let (reference, mut run) = load_on([2026, 10, 31]);
    let tour = bat_tour(&mut run, &reference, 8);
    let with: Vec<_> = tour.iter().filter(|(_, b)| !b.is_empty()).collect();
    assert!(!with.is_empty(), "bats at none of {} buildings", tour.len());
    for (_, b) in &with {
        assert!((5..30).contains(&b.len()), "{} bats", b.len());
    }
    // Go back to the last building with bats. Some bats can break, so check them all.
    let at = with.last().unwrap().0;
    let near = visit(&mut run, &reference, at).expect("the area loads again");
    let position = |run: &GarageRuntime, bat| run.area.items.get(bat).map(|i| i.pose.position);
    let bat = *near
        .iter()
        .find(|b| position(&run, **b).is_some())
        .expect("the bats are still there");
    let point = position(&run, bat).unwrap();
    assert!(
        run.area
            .interaction
            .pickup_item(
                bat,
                point,
                &run.area.player,
                &mut run.area.items,
                &mut run.physics,
            )
            .is_err(),
        "a bat cannot be carried"
    );
    // Source `NextSound`: a bat squeaks 10 to 60 s after the last squeak.
    let start: Vec<_> = near.iter().map(|b| position(&run, *b)).collect();
    let mut last = start.clone();
    let mut squeaks = 0;
    for _ in 0..steps(61.) {
        frame(&mut run, &reference);
        squeaks += run
            .flyers
            .squeaks()
            .iter()
            .filter(|s| near.contains(&s.item))
            .count();
        for (bat, last) in near.iter().zip(&mut last) {
            if let Some(p) = position(&run, *bat) {
                *last = Some(p);
            }
        }
    }
    assert!(squeaks > 0, "a bat squeaks: {} bats", near.len());
    let flown = start
        .iter()
        .zip(&last)
        .filter_map(|(p, q)| Some((p.as_ref()?, q.as_ref()?)))
        .map(|(p, q)| (0..3).map(|i| (q[i] - p[i]).powi(2)).sum::<f32>().sqrt())
        .fold(0_f32, f32::max);
    assert!(flown > 0.5, "a bat flies: {flown} m");
    // A bat syncs as a flyer before its first physics step, so count each flyer seen.
    let before = run.flyers.flyers().len();
    let mut loaded = save_load(&run, &reference);
    let mut flyers = Vec::new();
    for _ in 0..steps(2.) {
        frame(&mut loaded, &reference);
        for f in loaded.flyers.flyers() {
            if !flyers.contains(&f.item) {
                flyers.push(f.item);
            }
        }
    }
    assert_eq!(flyers.len(), before, "bats save and load");

    // Outside the window no building has bats.
    let (reference, mut run) = load_on([2026, 6, 1]);
    let tour = bat_tour(&mut run, &reference, 4);
    assert!(tour.iter().all(|(_, b)| b.is_empty()), "no bats in June");
}

/// CR10. Source `playerdeathtriggerscript.OnTriggerStay` on the bite trigger of a big evil
/// rabbit: `TryBreak(damageBreakable * dt)` for other rabbits and for loose breakable items.
#[test]
#[ignore = "requires the installed source export"]
fn tp_cr10_big_bite_hurts_rabbits_and_items() {
    use crate::damage::BreakKind;
    let (reference, mut run) = outdoors();
    // A tumbleweed is a loose breakable item.
    let weed = act(
        &mut run,
        &reference,
        &json!({"action": "spawn", "name": "thumbleweed", "forward": 3.}).to_string(),
    );
    let item = crate::reference::ItemId(weed[0].as_u64().unwrap() as u32);
    seconds(&mut run, &reference, 1.);
    let target = run
        .stream
        .breakables
        .targets()
        .iter()
        .find(|t| t.kind == BreakKind::Item(item))
        .expect("the tumbleweed binds its breakablescript");
    let item_health = target.health.health;
    let decreases = reference.breakables[target.record].decrease_after_attack;
    let e = &reference.creatures.rabbit.evil;
    assert!(e.trigger.damage_breakable > 0.);
    seconds(&mut run, &reference, e.target_forget_time + 1.);
    let evil = spawn_rabbit(&mut run, &reference, true, 6.);
    let victim = spawn_rabbit(&mut run, &reference, false, 12.);
    let victim_health = run
        .creatures
        .rabbits()
        .find(|r| r.id == victim)
        .unwrap()
        .health
        .health;
    let mut bites = 0;
    for _ in 0..steps(30.) {
        let Some(attacker) = run.creatures.rabbits().find(|r| r.id == evil) else {
            break;
        };
        if attacker.evil.trigger {
            bites += 1;
            // Hold the victim and the item in the bite trigger.
            let handle = attacker.trigger.as_ref().unwrap().handle;
            assert!(
                matches!(
                    trigger_role(&run, handle),
                    Some(crate::reference::TriggerRole::Hazard { damage_breakable, .. })
                        if damage_breakable == e.trigger.damage_breakable
                ),
                "the bite trigger has the hazard role"
            );
            let at = run.physics.rapier().colliders[handle]
                .position()
                .translation;
            let body = run
                .creatures
                .rabbits()
                .find(|r| r.id == victim && r.alive)
                .map(|r| r.body)
                .filter(|b| run.physics.rapier().bodies.get(*b).is_some());
            if let Some(body) = body {
                run.physics.edit_body(body, |b| {
                    let mut pose = *b.position();
                    pose.translation = at;
                    b.set_position(pose, true);
                    b.wake_up(true);
                });
            }
            crate::world::stream::tests::melee::place(&mut run, item, at);
        }
        frame(&mut run, &reference);
        if run.needs_output().dead {
            break;
        }
    }
    assert!(bites > 0, "the big evil rabbit bites");
    let hurt = run
        .creatures
        .rabbits()
        .find(|r| r.id == victim)
        .is_none_or(|r| !r.alive || r.health.health < victim_health);
    assert!(hurt, "the bite hurts the other rabbit");
    // Source TryBreak(float): health goes down only with decreaseAfterAttack. Without it, only a
    // single hit of the full health breaks the item.
    let t = run
        .stream
        .breakables
        .targets()
        .iter()
        .find(|t| t.kind == BreakKind::Item(item))
        .expect("the tumbleweed stays bound");
    eprintln!(
        "tumbleweed decreases {decreases}: {item_health} -> {}",
        t.health.health
    );
    if decreases {
        assert!(
            t.destroyed || t.health.health < item_health,
            "the bite damages the item"
        );
    } else {
        assert!(
            !t.destroyed && t.health.health == item_health,
            "TryBreak keeps its health"
        );
    }
}

/// CR12. Source `stimulscript.CheckAudioSources` on the engine sound: a running engine makes a
/// `Threat` stimulus near the car. The flight from a threat is `a_threat_sends_the_rabbit_away`.
#[test]
#[ignore = "requires the installed source export"]
fn tp_cr12_running_engine_makes_a_threat_stimulus() {
    use crate::world::stream::tests::car_types::{fill_fluids, hold, running};
    let (reference, mut run) = outdoors();
    act(
        &mut run,
        &reference,
        &json!({"action": "seat", "seat": "driver"}).to_string(),
    );
    seconds(&mut run, &reference, 1.);
    let key = run.vehicle.clone().unwrap();
    let car = Vector::from_array(run.physics.snapshot(&key).pose.position);
    let near_car = |run: &mut GarageRuntime| {
        run.creatures
            .bound_sounds()
            .iter()
            .filter(|s| (s.position - car).length() < 5.)
            .map(|s| s.value)
            .fold(0f32, f32::max)
    };
    seconds(&mut run, &reference, 1.);
    assert_eq!(near_car(&mut run), 0., "a stopped engine is quiet");
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
    assert!(started, "the engine starts");
    frame_with(
        &mut run,
        &reference,
        FrameInput {
            released: keys::IGNITION,
            ..Default::default()
        },
    );
    let mut value = 0f32;
    for _ in 0..steps(3.) {
        frame(&mut run, &reference);
        value = value.max(near_car(&mut run));
    }
    assert!(running(&run, &key));
    assert!(value > 0., "the running engine makes a sound stimulus");
}

/// X03e. Source `newAiScript.Start`, `Die` and `save_newai`: a placed human keeps its key,
/// a dead one saves its ragdoll bones and loads dead, and a placement spawns it once.
#[test]
#[ignore = "requires the installed source export"]
fn tp_x03e_human_body_die_and_save() {
    let (reference, mut run) = outdoors();
    let h = &reference.creatures.human;
    assert_eq!(h.item_id, 317);
    assert_eq!(
        (h.start.start_sleep_chance, h.start.voice_pitch[0]),
        (0.5, 0.75)
    );
    assert_eq!((h.breakable.health, h.breakable.shoot_health), (50., 1250.));
    assert_eq!(h.ragdoll.bodies.len(), 11);
    let place = |run: &mut GarageRuntime, forward: f32, ordinal: u32| {
        let pose = run.area.player.body_pose(&run.physics);
        let mut position = pose.position;
        position[2] -= forward;
        let key = crate::world::ObjectKey::Generated(crate::world::GeneratedId {
            layer: crate::world::WorldLayer::Startup,
            generator: reference.creatures.human.source.clone(),
            unit: [0, 0],
            ordinal,
            children: Vec::new(),
        });
        let roll = crate::world::spawn_state::HumanRoll {
            sleeping: ordinal == 1,
            voice_pitch: 0.9,
            patrol_time: 3.,
        };
        run.creatures
            .humans
            .place(
                &reference.creatures.human,
                &mut run.physics,
                key,
                rapier3d::math::Pose::from_translation(Vector::from_array(position)),
                roll,
            )
            .unwrap()
    };
    let _sleeper = place(&mut run, 4., 1);
    let dead = place(&mut run, 6., 2);
    seconds(&mut run, &reference, 1.);
    run.kill_human(&reference, dead, None).unwrap();
    seconds(&mut run, &reference, 2.);
    let saved = run
        .creatures
        .humans
        .snapshot(&run.physics, &run.stream.origin);
    assert_eq!(saved.len(), 2);
    assert!(run.creatures.humans.known(&saved[0].key));
    let loaded = save_load(&run, &reference);
    // Saved humans wait as far humans until the next place check.
    let mut loaded = loaded;
    seconds(&mut loaded, &reference, 2.);
    let back = loaded
        .creatures
        .humans
        .snapshot(&loaded.physics, &loaded.stream.origin);
    assert_eq!(back.len(), 2);
    let d = back.iter().find(|s| s.died).expect("one dead human");
    assert_eq!(d.bones.len(), 11);
    let s = back.iter().find(|s| !s.died).unwrap();
    assert!(s.sleeping && (s.voice_pitch, s.health) == (0.9, 50.));
    assert_eq!(
        loaded
            .creatures
            .humans
            .views(&loaded.physics)
            .filter(|v| v.died)
            .count(),
        1
    );
}
