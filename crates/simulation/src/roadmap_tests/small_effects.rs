//! ROADMAP test pass: Small effects (SE rows). Each test cites its source method.
use crate::audio::AudioKind;
use crate::starting_area::GarageRuntime;
use crate::world::stream::tests::kit::*;

/// The survival audio kinds since the last call.
fn kinds(run: &mut GarageRuntime) -> Vec<AudioKind> {
    let audio = run.audio.as_mut().expect("audio binds");
    let kinds = audio.survival_events.iter().map(|e| e.kind).collect();
    audio.survival_events.clear();
    kinds
}

/// SE1: source `pickupscript.OnCollisionEnter` plays the hit sound of the food when it lands.
/// Spawned pieces take the sound of their pickup: `spawned_copy_takes_the_hit_sound_of_its_pickup`.
#[test]
#[ignore = "requires the installed source export"]
fn tp_se1_dropped_food_plays_its_hit_sound() {
    let (reference, mut run) = load_garage();
    seconds(&mut run, &reference, 1.);
    let food = reference.survival.starter_food;
    take(&mut run, &reference, food);
    run.area
        .interaction
        .drop_held(&mut run.area.items, &mut run.physics);
    run.area.sync_inventory();
    let body = run.area.items.get(food).unwrap().body.unwrap();
    run.physics.edit_body(body, |b| {
        let mut pose = *b.position();
        pose.translation.y += 1.5;
        b.set_position(pose, true);
    });
    kinds(&mut run);
    let mut hit = false;
    for _ in 0..150 {
        frame(&mut run, &reference);
        hit |= kinds(&mut run).contains(&AudioKind::ItemHit);
    }
    assert!(hit, "the food plays its hit sound");
}

/// SE2: source `mainscript.FoodParticle` on each bite (`ediblescript.particleType`, min and max
/// count), and `cigiscript` embers while the cigarette burns.
#[test]
#[ignore = "requires the installed source export"]
fn tp_se2_bites_drop_bits_and_cigarettes_drop_ash() {
    use crate::simulation::{FrameInput, player_keys};
    let (reference, mut run) = load_garage();
    seconds(&mut run, &reference, 1.);
    let food = reference.survival.starter_food;
    let index = run
        .area
        .items
        .get(food)
        .unwrap()
        .food
        .unwrap()
        .prototype_index;
    let prototype = &reference.survival.foods[index];
    assert!(prototype.has_particle, "the starter food drops bits");
    take(&mut run, &reference, food);
    let kind = prototype.particle_type.into();
    let before = run.food_particles.particles(kind).len();
    assert!(run.consume(&reference, food).unwrap(), "a bite");
    let bits = run.food_particles.particles(kind).len() - before;
    assert!(
        bits > 0 && bits <= prototype.max_particle as usize,
        "{bits} bits of {}..{}",
        prototype.min_particle,
        prototype.max_particle
    );
    // Smoke a cigarette: it burns and drops embers.
    let p = run.area.cigarettes.cigarette_prototype;
    let root = reference.item_prototypes[p].root;
    let name = &reference.nodes.iter().find(|n| n.id == root).unwrap().name;
    run.area
        .interaction
        .drop_held(&mut run.area.items, &mut run.physics);
    run.area.sync_inventory();
    act(
        &mut run,
        &reference,
        &format!(r#"{{"action":"wear","name":"{name}"}}"#),
    );
    let mut ash = 0;
    for _ in 0..300 {
        frame_with(
            &mut run,
            &reference,
            FrameInput {
                player_held: player_keys::SMOKE,
                ..FrameInput::default()
            },
        );
        ash = ash.max(
            run.area
                .cigarettes
                .embers()
                .map(|(_, p)| p.len())
                .sum::<usize>(),
        );
    }
    assert!(ash > 0, "the burning cigarette drops ash");
}

/// SE4: source `mapscript.StartDateItems` with `mainscript.CheckHoliday`: the start pumpkin
/// spawns only in its holiday. Its `lightdither` light follows it on the ground, in the hand
/// and on the head.
#[test]
#[ignore = "requires the installed source export"]
fn tp_se4_holiday_pumpkin_spawns_and_its_light_follows() {
    let reference = reference();
    let pumpkins = |run: &GarageRuntime| -> Vec<crate::reference::ItemId> {
        run.area
            .spawned
            .iter()
            .filter(|s| {
                reference.item_prototypes[s.prototype]
                    .light_dither
                    .is_some()
            })
            .map(|s| s.id)
            .collect()
    };
    let h = &reference.acquisition.holiday_startup;
    assert_eq!(
        (h.start, h.end),
        ([10, 15], [11, 15]),
        "source holiday dates"
    );
    set_calendar([1, 1]);
    let run = ready(&reference);
    assert!(pumpkins(&run).is_empty(), "no pumpkin on 1 January");
    set_calendar([10, 31]);
    let mut run = ready(&reference);
    set_calendar([1, 1]);
    run.started = true;
    run.paused = false;
    seconds(&mut run, &reference, 1.);
    let found = pumpkins(&run);
    assert_eq!(found.len(), 1, "one start pumpkin on 31 October");
    let pumpkin = found[0];
    let light = |run: &mut GarageRuntime| {
        let mut out = crate::boundary::SimFrame::default();
        run.fill_frame(&reference, 1., &mut out);
        out.state.dither_light
    };
    assert_eq!(
        light(&mut run),
        pumpkin.0 + 1,
        "the pumpkin light on the ground"
    );
    take(&mut run, &reference, pumpkin);
    assert_eq!(
        light(&mut run),
        pumpkin.0 + 1,
        "the pumpkin light in the hand"
    );
}
