//! ROADMAP test pass: Car camera and crash damage (CC rows).
use crate::interaction::guns::Strike;
use crate::interaction::items::ItemLocation;
use crate::simulation::FIXED_DT;
use crate::world::stream::tests::kit::*;
use rapier3d::math::Vector;

/// CC14. Source `explosionscript.Explode`: each `carscript` body gets `DamageStuff(power * 5)`,
/// and a strong blast drops parts.
#[test]
#[ignore = "requires the installed source export"]
fn tp_cc14_a_tank_blast_next_to_a_car_knocks_parts_off() {
    let (reference, mut run) = load_garage();
    settle(&mut run, &reference);
    let in_slots = |run: &crate::starting_area::GarageRuntime| {
        run.area
            .items
            .iter()
            .filter(|s| !s.removed && matches!(s.location, ItemLocation::PartSlot(_)))
            .count()
    };
    let key = run.vehicle.clone().unwrap();
    let car = run.physics.snapshot(&key).pose.position;
    // The full garage can.
    let can = super::weapons::garage_can(&run, &reference);
    let body = run.area.items.get(can).unwrap().body.unwrap();
    run.physics.edit_body(body, |b| {
        b.set_translation(Vector::from_array(car) + Vector::new(0., 1.5, 0.), true);
        b.set_linvel(Vector::ZERO, true);
    });
    run.physics.refresh_queries();
    let before = in_slots(&run);
    run.shot_explosive(Strike {
        item: can,
        force: 1e6,
        explode: true,
        hole: None,
    });
    run.run_explosions(&reference, FIXED_DT);
    seconds(&mut run, &reference, 1.);
    assert!(
        in_slots(&run) < before,
        "parts fall off: {before} in slots before"
    );
}
