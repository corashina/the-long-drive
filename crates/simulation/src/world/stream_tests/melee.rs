//! X02 part 2: every melee weapon swings. A katana breaks a bottle. Only a weapon with
//! `shovel_speed` digs: the shovel lifts a buried item, the crowbar does not. A `hurt_food`
//! weapon cuts food, and the pieces reload with their bites.
use super::drive_out::{frame, settle};
use super::*;
use crate::damage::BreakKind;
use crate::interaction::items::ItemLocation;
use crate::reference::{ItemId, MeleeWeaponReference};
use crate::simulation::{FIXED_DT, FrameInput, keys};
use crate::vehicle::to_rapier;
use rapier3d::math::{Pose, Vector};

/// Crowbar prefab name parts.
pub(super) const CROWBAR: &[&str] = &["crowbar", "feszito"];
/// Frames from one swing press to the next.
const SWING_FRAMES: usize = 50;

/// The weapon whose world prototype root name has one of `names`.
pub(super) fn weapon<'a>(
    reference: &'a ReferenceManifest,
    names: &[&str],
) -> &'a MeleeWeaponReference {
    reference
        .melee
        .weapons
        .iter()
        .find(|w| {
            reference
                .world
                .prototypes
                .iter()
                .find(|p| p.source == w.prototype.0)
                .and_then(|p| p.nodes.iter().find(|n| n.parent.is_none()))
                .is_some_and(|n| {
                    let name = n.name.to_lowercase();
                    names.iter().any(|s| name.contains(s))
                })
        })
        .unwrap_or_else(|| panic!("melee weapon {names:?}"))
}

/// Spawns the weapon prototype at the player and returns its pickup item.
pub(super) fn spawn(
    run: &mut GarageRuntime,
    reference: &ReferenceManifest,
    w: &MeleeWeaponReference,
) -> ItemId {
    let prototype = reference
        .item_prototypes
        .iter()
        .position(|p| p.id == w.prototype)
        .unwrap();
    let pickup = reference.item_prototypes[prototype]
        .pickups
        .iter()
        .position(|p| p.pickup.source == w.pickup)
        .unwrap();
    let pose = run.area.player.body_pose(&run.physics);
    crate::starting_area::spawned::spawn_prototype(
        reference,
        &mut run.area,
        &mut run.physics,
        prototype,
        pose,
    )
    .unwrap()[pickup]
}

/// Spawns the weapon and puts it in the right hand.
fn equip(
    run: &mut GarageRuntime,
    reference: &ReferenceManifest,
    w: &MeleeWeaponReference,
) -> ItemId {
    let item = spawn(run, reference, w);
    super::g08_pickup(run, item);
    assert!(run.area.equip_carried(&mut run.physics).unwrap());
    settle(run, reference, 1.);
    item
}

/// The weapon `Trigger` in physics space.
fn trigger(
    run: &GarageRuntime,
    reference: &ReferenceManifest,
    w: &MeleeWeaponReference,
    item: ItemId,
) -> Pose {
    let prototype = reference
        .world
        .prototypes
        .iter()
        .find(|p| p.source == w.prototype.0)
        .unwrap();
    let node = prototype
        .nodes
        .iter()
        .position(|n| n.source == w.trigger)
        .unwrap();
    let state = run.area.items.get(item).unwrap();
    let body = run.physics.rapier().bodies[state.body.unwrap()].position();
    *body * to_rapier(state.local_pose) * prototype.root_relative(node).0
}

/// One frame of repeated short swings: press, release, then wait.
fn swing_frame(run: &mut GarageRuntime, reference: &ReferenceManifest, index: usize) {
    let input = match index % SWING_FRAMES {
        0 => FrameInput {
            held: keys::PRIMARY,
            pressed: keys::PRIMARY,
            ..Default::default()
        },
        1 => FrameInput {
            released: keys::PRIMARY,
            ..Default::default()
        },
        _ => FrameInput::default(),
    };
    super::g08_frame(run, reference, input, FIXED_DT);
}

/// Moves a body to `at` with no velocity.
pub(super) fn place(run: &mut GarageRuntime, item: ItemId, at: Vector) {
    let body = run.area.items.get(item).unwrap().body.unwrap();
    run.physics.edit_body(body, |b| {
        let mut pose = *b.position();
        pose.translation = at;
        b.set_position(pose, true);
        if b.is_kinematic() {
            b.set_next_kinematic_position(pose);
        }
        b.set_linvel(Vector::ZERO, true);
        b.set_angvel(Vector::ZERO, true);
    });
    run.physics.refresh_queries();
}

#[test]
#[ignore = "requires the installed source export"]
fn katana_swing_breaks_a_bottle() {
    let (reference, mut run) = super::garage_controls::load_garage();
    settle(&mut run, &reference, 1.);
    let records = &reference.breakables;
    let (target, bottle) = run
        .stream
        .breakables
        .targets()
        .iter()
        .enumerate()
        .filter_map(|(i, t)| match t.kind {
            BreakKind::Item(item) => Some((i, t.record, item)),
            _ => None,
        })
        .filter(|(_, record, item)| {
            let r = &records[*record];
            r.source != reference.acquisition.r#box.breakable
                && !(r.no_destroy || r.no_collider)
                && r.health <= 6.
                && run
                    .area
                    .items
                    .get(*item)
                    .is_some_and(|s| s.location == ItemLocation::World && !s.buried)
        })
        .map(|(i, _, item)| (i, item))
        .next()
        .expect("bottle near the start");
    let katana = weapon(&reference, &["katana"]);
    let item = equip(&mut run, &reference, katana);
    let w = run.area.interaction.melee.weapon_for(&katana.pickup);
    assert!(w.is_some() && reference.melee.weapons[w.unwrap()].pickup == katana.pickup);
    for i in 0..10 * SWING_FRAMES {
        if run.stream.breakables.targets()[target].destroyed {
            break;
        }
        let at = trigger(&run, &reference, katana, item).translation;
        place(&mut run, bottle, at);
        swing_frame(&mut run, &reference, i);
        frame(&mut run, &reference);
    }
    assert!(
        run.stream.breakables.targets()[target].destroyed,
        "the katana breaks the bottle"
    );
}

/// Swings the shovel or the crowbar at a buried crowbar copy. True when the copy rises.
fn digs(shovel: bool) -> bool {
    let (reference, mut run) = super::garage_controls::load_garage();
    settle(&mut run, &reference, 1.);
    let w = if shovel {
        reference.melee.shovel(&reference).unwrap()
    } else {
        weapon(&reference, CROWBAR)
    };
    let item = equip(&mut run, &reference, w);
    let target = spawn(&mut run, &reference, weapon(&reference, CROWBAR));
    crate::interaction::buried::bury(target, &mut run.area.items, &mut run.physics);
    for i in 0..4 * SWING_FRAMES {
        let at = trigger(&run, &reference, w, item).translation;
        place(&mut run, target, at);
        swing_frame(&mut run, &reference, i);
        let body = run.area.items.get(target).unwrap().body.unwrap();
        if run.physics.rapier().bodies[body].position().translation.y > at.y + 1e-5 {
            return true;
        }
        assert!(run.area.items.get(target).unwrap().buried);
        frame(&mut run, &reference);
    }
    false
}

#[test]
#[ignore = "requires the installed source export"]
fn crowbar_hit_does_not_lift_a_buried_item() {
    assert!(digs(true), "the shovel digs");
    assert!(!digs(false), "the crowbar does not dig");
}

/// Remaining bites of each live food piece: the starter food and runtime copies.
fn pieces(run: &GarageRuntime, reference: &ReferenceManifest) -> Vec<u8> {
    std::iter::once(reference.survival.starter_food)
        .chain(run.area.foods.iter().copied())
        .filter_map(|id| run.area.items.get(id).filter(|s| !s.removed)?.food)
        .map(|f| reference.survival.foods[f.prototype_index].bites - f.bite)
        .collect()
}

#[test]
#[ignore = "requires the installed source export"]
fn shovel_cuts_food_and_the_pieces_reload() {
    let (reference, mut run) = super::garage_controls::load_garage();
    settle(&mut run, &reference, 1.);
    let shovel = reference.melee.shovel(&reference).unwrap();
    assert!(shovel.contact.hurt_food);
    let food = reference.survival.starter_food;
    let state = run.area.items.get(food).unwrap().food.unwrap();
    assert!(reference.survival.foods[state.prototype_index].spawn_if_shot);
    let before = pieces(&run, &reference);
    assert_eq!(before.len(), 1);
    assert!(before[0] >= 2);
    let item = equip(&mut run, &reference, shovel);
    for i in 0..10 * SWING_FRAMES {
        let at = trigger(&run, &reference, shovel, item).translation;
        place(&mut run, food, at);
        swing_frame(&mut run, &reference, i);
        if !run.area.foods.is_empty() {
            break;
        }
        frame(&mut run, &reference);
    }
    let mut cut = pieces(&run, &reference);
    assert!(cut.len() >= 2, "the shovel cuts the food");
    assert_eq!(
        cut.iter().sum::<u8>(),
        before[0],
        "the pieces keep every bite"
    );

    let stamp = super::roadside_stamp(&reference);
    let saved = run
        .capture_save(&reference, &stamp, "2026-10-26T12:00:00Z")
        .unwrap();
    let saved: crate::save::SaveSnapshot =
        serde_json::from_slice(&serde_json::to_vec(&saved).unwrap()).unwrap();
    assert_eq!(saved.foods.len(), cut.len() - 1);
    let mut loaded = GarageRuntime::from_save(
        &reference,
        &stamp,
        &saved,
        crate::world::GameMode::default(),
        reference.world.draw_distance.default,
    )
    .unwrap();
    for _ in 0..2000 {
        super::prepare_ready(&mut loaded, &reference);
        if loaded.advance_saved_restore(&reference).unwrap() && !loaded.restore_pending() {
            break;
        }
    }
    assert!(!loaded.restore_pending());
    let mut reloaded = pieces(&loaded, &reference);
    cut.sort();
    reloaded.sort();
    assert_eq!(reloaded, cut, "the pieces reload with their bites");
}
