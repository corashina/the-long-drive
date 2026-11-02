//! X02 part 4: explosions. The record loads, a generated tank reads its explosion fields, a shot
//! hole leaks a tank, and the garage can explodes and stays removed. `roadmap_tests::weapons`
//! covers a blast that breaks a bottle and sets off a second tank.
use super::kit::*;
use crate::interaction::guns::Strike;
use crate::interaction::items::ItemLocation;
use crate::reference::ItemId;
use crate::simulation::FIXED_DT;
use rapier3d::math::Vector;

#[test]
#[ignore = "requires the installed source export"]
fn explosion_record_loads_and_an_explosive_tank_binds() {
    let (reference, mut run) = load_garage();
    seconds(&mut run, &reference, 1.);
    let explosions = &reference.explosions;
    assert!(explosions.types.iter().all(|t| t.explosion_force > 0.));
    assert!(!explosions.explosive_fluids.is_empty());
    let interaction = &run.area.interaction;
    assert!(
        run.area.items.iter().any(|s| {
            s.fluid.as_ref().is_some_and(|f| f.total() > 0.)
                && interaction
                    .tank_explosion(s.id)
                    .is_some_and(|e| !e.never_explode)
        }),
        "an explosive generated tank binds near the start"
    );
}

fn position(run: &crate::starting_area::GarageRuntime, item: ItemId) -> Vector {
    let body = run.area.items.get(item).unwrap().body.unwrap();
    run.physics.rapier().bodies[body].translation()
}

#[test]
#[ignore = "requires the installed source export"]
fn a_shot_hole_facing_down_leaks_a_tank() {
    let (reference, mut run) = load_garage();
    seconds(&mut run, &reference, 1.);
    assert!(
        reference.guns.weapons.iter().any(|w| w.hithole),
        "a hithole gun"
    );
    let interaction = &run.area.interaction;
    let fluids = interaction.explosions.explosive_fluids();
    let tank = run
        .area
        .items
        .iter()
        .find(|s| {
            !s.removed
                && s.location == ItemLocation::World
                && s.body.is_some()
                && s.fluid.as_ref().is_some_and(|f| {
                    f.total() > 0.5 && !crate::interaction::explosions::is_explosive(f, fluids)
                })
                && interaction
                    .tank_explosion(s.id)
                    .is_some_and(|e| !e.no_shoot_hole && !e.always_explode)
        })
        .expect("a non-explosive tank with fluid near the start")
        .id;
    let amount = |run: &crate::starting_area::GarageRuntime| {
        run.area
            .items
            .get(tank)
            .unwrap()
            .fluid
            .as_ref()
            .unwrap()
            .total()
    };
    let before = amount(&run);
    run.shot_explosive(Strike {
        item: tank,
        force: 1.,
        explode: false,
        hole: Some([position(&run, tank), -Vector::Y]),
    });
    assert_eq!(run.area.interaction.holes.count(tank), 1);
    seconds(&mut run, &reference, 3.);
    assert!(amount(&run) < before, "the fluid leaks through the hole");
}

#[test]
#[ignore = "requires the installed source export"]
fn x02_leftovers_garage_can_explodes_and_stays_removed() {
    let (reference, mut run) = load_garage();
    seconds(&mut run, &reference, 1.);
    let interaction = &run.area.interaction;
    let fluids = interaction.explosions.explosive_fluids();
    let can = reference
        .tanks
        .iter()
        .filter(|t| t.role == crate::reference::TankRole::Portable)
        .map(|t| t.item)
        .find(|&id| {
            let state = run.area.items.get(id).unwrap();
            interaction.tank_explosion(id).is_some_and(|e| {
                crate::interaction::explosions::tank_factor(
                    e,
                    state.fluid.as_ref().unwrap(),
                    fluids,
                    1e6,
                )
                .is_some()
            })
        })
        .expect("an explosive garage can");
    let on_body: Vec<ItemId> = {
        let body = run.area.items.get(can).unwrap().body;
        run.area
            .items
            .iter()
            .filter(|s| s.body == body)
            .map(|s| s.id)
            .collect()
    };
    run.shot_explosive(Strike {
        item: can,
        force: 1e6,
        explode: true,
        hole: None,
    });
    run.run_explosions(&reference, FIXED_DT);
    seconds(&mut run, &reference, 0.1);
    for id in &on_body {
        let state = run.area.items.get(*id).unwrap();
        assert!(
            state.removed && state.body.is_none(),
            "the can and its cap are gone"
        );
    }
    let mut loaded = save_load(&run, &reference);
    seconds(&mut loaded, &reference, 1.);
    for id in &on_body {
        let state = loaded.area.items.get(*id).unwrap();
        assert!(
            state.removed && state.body.is_none(),
            "the can stays gone after load"
        );
    }
}
