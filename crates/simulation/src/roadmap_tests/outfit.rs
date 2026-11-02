//! ROADMAP test pass: Player body and outfit rows on hands and glasses (PB7, PB8, PB23, PB25, PB26).
use crate::boundary::{GlassesState, SimFrame};
use crate::reference::{ItemId, ReferenceManifest};
use crate::simulation::{FIXED_DT, FrameInput, player_keys};
use crate::starting_area::GarageRuntime;
use crate::world::stream::tests::kit::*;

fn sim_frame(run: &mut GarageRuntime, reference: &ReferenceManifest) -> SimFrame {
    let mut out = SimFrame::default();
    run.fill_frame(reference, 1., &mut out);
    out
}

/// Spawns wearable `index` of `clothing.wearables` at the player and puts it on.
fn put_on(run: &mut GarageRuntime, reference: &ReferenceManifest, index: usize) -> ItemId {
    let w = &reference.clothing.wearables[index];
    let prototype = reference
        .item_prototypes
        .iter()
        .position(|p| p.id == w.prototype)
        .unwrap();
    let pose = run.area.player.body_pose(&run.physics);
    let item = crate::starting_area::spawned::spawn_prototype(
        reference,
        &mut run.area,
        &mut run.physics,
        prototype,
        pose,
    )
    .unwrap()[0];
    frame(run, reference);
    assert!(
        run.area.put_on(&mut run.physics, item),
        "wearable {index} goes on"
    );
    run.area.update_glasses(&run.stream);
    frame(run, reference);
    item
}

/// Indices of the glasses wearables.
fn glasses(reference: &ReferenceManifest) -> Vec<usize> {
    let all: Vec<_> = (0..reference.clothing.wearables.len())
        .filter(|&i| !reference.clothing.wearables[i].lenses.is_empty())
        .collect();
    assert!(!all.is_empty(), "the export has glasses");
    all
}

/// The lens states that wearable `index` can show.
fn lens_states(reference: &ReferenceManifest, index: usize) -> Vec<GlassesState> {
    reference.clothing.wearables[index]
        .lenses
        .iter()
        .map(|l| GlassesState {
            profile: l.profile.unwrap(),
            hide_flare: l.disable_lens_flare,
        })
        .collect()
}

/// PB23. Source `szbglasscript.Puton`: `PPVSunGlasses.profile` and `disableSunFlare` come from
/// the active lens. The frame sends both to the renderer.
#[test]
#[ignore = "requires the installed source export"]
fn tp_pb23_lens_flare_flag_follows_the_lens() {
    let reference = reference();
    let flare_lenses = reference
        .clothing
        .wearables
        .iter()
        .flat_map(|w| &w.lenses)
        .filter(|l| l.disable_lens_flare)
        .count();
    let all = reference
        .clothing
        .wearables
        .iter()
        .flat_map(|w| &w.lenses)
        .count();
    assert!(
        flare_lenses > 0 && flare_lenses < all,
        "some lenses hide the flare, others keep it: {flare_lenses} of {all}"
    );
    for index in glasses(&reference) {
        let (reference, mut run) = load_garage();
        seconds(&mut run, &reference, 1.);
        assert_eq!(sim_frame(&mut run, &reference).state.glasses(), None);
        put_on(&mut run, &reference, index);
        let state = run.area.wear.glasses.expect("worn glasses set a lens");
        assert!(
            lens_states(&reference, index).contains(&state),
            "glasses {index}: {state:?} is one of its lenses"
        );
        assert_eq!(sim_frame(&mut run, &reference).state.glasses(), Some(state));
    }
}

/// PB25. Source `szbglasscript.TakeOff` and `survivalscript.Death2`: taking the glasses off and
/// death clear the lens.
#[test]
#[ignore = "requires the installed source export"]
fn tp_pb25_glasses_off_and_death_clear_the_lens() {
    let (reference, mut run) = load_garage();
    seconds(&mut run, &reference, 1.);
    let index = glasses(&reference)[0];
    put_on(&mut run, &reference, index);
    assert!(sim_frame(&mut run, &reference).state.glasses().is_some());
    // X with empty hands takes the glasses off.
    frame_with(
        &mut run,
        &reference,
        FrameInput {
            player_released: player_keys::TAKE_OFF,
            ..FrameInput::default()
        },
    );
    frame(&mut run, &reference);
    assert_eq!(run.area.wear.glasses, None);
    assert_eq!(sim_frame(&mut run, &reference).state.glasses(), None);

    // Death with the glasses on.
    let (reference, mut run) = load_garage();
    seconds(&mut run, &reference, 1.);
    put_on(&mut run, &reference, index);
    assert!(run.area.wear.glasses.is_some());
    let hp = run.needs_output().needs.hp;
    let position = run.area.player.body_pose(&run.physics).position;
    let handle = run.physics.insert_collider(
        rapier3d::prelude::ColliderBuilder::cuboid(5., 5., 5.)
            .translation(rapier3d::math::Vector::from_array(position))
            .sensor(true)
            .build(),
    );
    run.physics.refresh_queries();
    add_trigger_role(
        &mut run,
        handle,
        crate::reference::TriggerRole::Hazard {
            damage_player: hp * 4.,
            damage_breakable: 0.,
        },
    );
    seconds(&mut run, &reference, 1.);
    assert!(run.needs_output().dead, "the player dies");
    assert_eq!(run.area.wear.glasses, None, "death clears the lens");
    assert_eq!(sim_frame(&mut run, &reference).state.glasses(), None);
}

/// PB26. Source `savescript` worn items and `szbglasscript.Start`: worn glasses load worn, and
/// the lens comes back.
#[test]
#[ignore = "requires the installed source export"]
fn tp_pb26_glasses_lens_returns_after_load() {
    let (reference, mut run) = load_garage();
    seconds(&mut run, &reference, 1.);
    let index = glasses(&reference)[0];
    put_on(&mut run, &reference, index);
    let slot = reference.clothing.wearables[index].type_index as usize;
    let state = run.area.wear.glasses.expect("lens on");
    let mut loaded = save_load(&run, &reference);
    seconds(&mut loaded, &reference, 1.);
    assert!(
        loaded.area.wear.worn.slots[slot].is_some(),
        "the glasses load worn"
    );
    assert_eq!(loaded.area.wear.glasses, Some(state), "the lens comes back");
    assert_eq!(
        sim_frame(&mut loaded, &reference).state.glasses(),
        Some(state)
    );
}

/// Hand target positions (left, right) relative to the player.
fn hands(run: &GarageRuntime) -> [rapier3d::math::Vector; 2] {
    let t = run.area.body.sent_targets();
    [1, 2].map(|i| rapier3d::math::Vector::new(t[i][0], t[i][1], t[i][2]))
}

/// A world pose relative to the player.
fn player_local(run: &GarageRuntime, pose: rapier3d::math::Pose) -> rapier3d::math::Vector {
    let body = crate::vehicle::to_rapier(run.area.player.body_pose(&run.physics)).inverse();
    (body * pose).translation
}

/// PB7. Source `fpscontroller.UpdHandPos`: a carried two-hand item puts the right hand on its
/// grab point and the left hand on the carry anchor.
#[test]
#[ignore = "requires the installed source export"]
fn tp_pb7_two_hand_carry_puts_both_hands_on_the_item() {
    let (reference, mut run) = load_garage();
    seconds(&mut run, &reference, 2.);
    let carry = &run.area.interaction.carry;
    let item = run
        .area
        .items
        .iter()
        .filter(|i| !i.removed && i.body.is_some())
        .find(|i| carry.rule(i.id).is_some_and(|r| r.double_hand))
        .map(|i| i.id)
        .expect("a two-hand item in the garage");
    let point = run.area.items.get(item).unwrap().pose.position;
    let mut pose = run.area.player.body_pose(&run.physics);
    let to = rapier3d::math::Vector::from_array(point);
    let from = rapier3d::math::Vector::from_array(pose.position);
    let at = to + (from - to).with_y(0.).normalize_or_zero();
    pose.position = [at.x, pose.position[1], at.z];
    run.area.player.recover(pose, &mut run.physics);
    seconds(&mut run, &reference, 0.5);
    run.area
        .interaction
        .pickup_item(
            item,
            point,
            &run.area.player,
            &mut run.area.items,
            &mut run.physics,
        )
        .unwrap();
    run.area.sync_inventory();
    seconds(&mut run, &reference, 1.5);
    assert_eq!(
        run.area.interaction.held(),
        Some(item),
        "the item is carried"
    );
    let (grab, anchor, _) = run
        .area
        .interaction
        .carry
        .hand_frames(&run.area.player, &run.physics)
        .unwrap();
    let [left, right] = hands(&run);
    let grab = player_local(&run, grab);
    let anchor = player_local(&run, anchor);
    eprintln!("left {left:?} anchor {anchor:?} right {right:?} grab {grab:?}");
    assert!(
        (right - grab).length() < 0.05,
        "the right hand holds the grab point"
    );
    assert!(
        (left - anchor).length() < 0.05,
        "the left hand holds the anchor"
    );
}

/// PB8. Source `fpscontroller` pointing (`pointingValue`, `pointAt`) and `UpdHandPos` Mutat:
/// holding Left Alt moves the free right hand to the aimed point.
#[test]
#[ignore = "requires the installed source export"]
fn tp_pb8_left_alt_points_the_hand_at_the_surface() {
    let (reference, mut run) = load_garage();
    seconds(&mut run, &reference, 1.);
    look(&mut run, 0., -70.);
    let point = FrameInput {
        player_held: player_keys::POINT,
        ..FrameInput::default()
    };
    // The inventory frame runs the pointing.
    let step = |run: &mut GarageRuntime, input| super::weapons::input(run, &reference, input);
    for _ in 0..50 {
        step(&mut run, point);
    }
    let p = run.area.player.point();
    assert!(p.value > 0.9, "the pointing value rises: {}", p.value);
    let at = p.at.translation;
    let [_, right] = hands(&run);
    assert!(
        (right - at).length() < 0.05,
        "the right hand points: {right:?} {at:?}"
    );
    for _ in 0..50 {
        step(&mut run, FrameInput::default());
    }
    assert!(
        run.area.player.point().value < 0.1,
        "the release lowers the hand"
    );
}

/// PB8. Source `fpscontroller.pointKefe.Use` while Left Alt is held: the hand brush scrubs a
/// garage cobweb (`washtriggerscript`, `CleanProperty` dust) until it is gone. The car blood is
/// `CleanProperty` blood: the source table gives the hand (cleaning type 6) no wash, so only a
/// sponge cleans it.
#[test]
#[ignore = "requires the installed source export"]
fn tp_pb8_left_alt_rubs_a_cobweb_off_and_not_the_car_blood() {
    use rapier3d::math::Vector;
    let (reference, mut run) = load_garage();
    settle(&mut run, &reference);
    let c = &reference.conditions;
    let hand = c.player_brush.tool.cleaning_type as usize;
    for p in reference.world.prototypes.iter() {
        if let Some(blood) = p.vehicle.as_ref().and_then(|v| v.blood.as_ref()) {
            assert!(
                !c.clean_tables[blood.permission as usize][hand],
                "the hand does not wash car blood"
            );
        }
    }
    let home = run.area.player.body_pose(&run.physics);
    // Visible cobwebs with their collider centers.
    let webs: Vec<_> =
        c.washes
            .iter()
            .filter(|w| run.area.interaction.conditions.wash(w.id).visible())
            .filter_map(|w| {
                let id = *w.colliders.first()?;
                let (_, collider) =
                    run.physics.rapier().colliders.iter().find(|(_, col)| {
                        (col.user_data >> 64) as u32 == id.0 && col.is_enabled()
                    })?;
                Some((w.id, collider.compute_aabb().center()))
            })
            .collect();
    assert!(!webs.is_empty(), "the garage has visible cobwebs");
    let point = FrameInput {
        player_held: player_keys::POINT,
        ..FrameInput::default()
    };
    let step = |run: &mut GarageRuntime, input| super::weapons::input(run, &reference, input);
    for (web, at) in webs {
        // Stand 0.6 m away on the home floor (the point range is 1 m), and scrub with small
        // look moves around the web.
        let mut pose = home;
        let away = (Vector::from_array(home.position) - at)
            .with_y(0.)
            .normalize_or_zero();
        let stand = at + away * 0.6;
        pose.position = [stand.x, home.position[1], stand.z];
        run.area.player.recover(pose, &mut run.physics);
        seconds(&mut run, &reference, 0.3);
        for i in 0..(6. / FIXED_DT) as usize {
            let wobble = if i % 2 == 0 { 0.03 } else { -0.03 };
            let target = at + Vector::new(wobble, 0., -wobble);
            run.area.player.look_at(target.to_array(), &run.physics);
            step(&mut run, point);
            if !run.area.interaction.conditions.wash(web).visible() {
                eprintln!("{web:?} is gone after {:.2} s", i as f32 * FIXED_DT);
                return;
            }
        }
        eprintln!(
            "{web:?} stays: progress {}",
            run.area.interaction.conditions.wash(web).repair_progress
        );
    }
    panic!("Left Alt rubs no cobweb off");
}
