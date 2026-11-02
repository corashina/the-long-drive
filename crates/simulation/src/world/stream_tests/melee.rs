//! X02 part 2: melee helpers. A `hurt_food` weapon cuts food, and the pieces reload with their
//! bites. `roadmap_tests::weapons` swings each weapon, breaks a bottle and digs.
use super::kit::*;
use super::*;
use crate::reference::{ItemId, MeleeWeaponReference};
use crate::simulation::{FIXED_DT, FrameInput, keys};
use crate::vehicle::to_rapier;
use rapier3d::math::{Pose, Vector};

/// Frames from one swing press to the next.
pub(crate) const SWING_FRAMES: usize = 50;

/// The weapon whose world prototype root name has one of `names`.
pub(crate) fn weapon<'a>(
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
pub(crate) fn spawn(
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
pub(crate) fn equip(
    run: &mut GarageRuntime,
    reference: &ReferenceManifest,
    w: &MeleeWeaponReference,
) -> ItemId {
    let item = spawn(run, reference, w);
    super::g08_pickup(run, item);
    assert!(run.area.equip_carried(&mut run.physics).unwrap());
    seconds(run, reference, 1.);
    item
}

/// The weapon `Trigger` in physics space.
pub(crate) fn trigger(
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
pub(crate) fn swing_frame(run: &mut GarageRuntime, reference: &ReferenceManifest, index: usize) {
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
pub(crate) fn place(run: &mut GarageRuntime, item: ItemId, at: Vector) {
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
    let (reference, mut run) = load_garage();
    seconds(&mut run, &reference, 1.);
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

    let saved = save(&run, &reference);
    assert_eq!(saved.foods.len(), cut.len() - 1);
    let loaded = load(&reference, &saved);
    let mut reloaded = pieces(&loaded, &reference);
    cut.sort();
    reloaded.sort();
    assert_eq!(reloaded, cut, "the pieces reload with their bites");
}
