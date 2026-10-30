//! X09: a fast stop knocks parts off the start Lada, a slow stop does not.
use super::drive_out::{frame, settle};
use crate::interaction::items::ItemLocation;
use crate::reference::ItemId;
use crate::starting_area::GarageRuntime;

/// Mounted parts and rims of all cars.
fn mounted(run: &GarageRuntime, car: ItemId) -> Vec<ItemId> {
    let parts = run.area.interaction.parts.mounted().map(|(item, _)| item);
    let rims = run.wheels.mounted_rims(car).map(|(_, rim, _)| rim);
    parts.chain(rims).collect()
}

#[test]
#[ignore = "requires the installed source export"]
fn x09_crash_knocks_parts_off() {
    let (reference, mut run) = super::garage_controls::load_garage();
    let key = run.vehicle.clone().expect("start Lada");
    for _ in 0..2000 {
        if run.vehicles.contains_key(&key) {
            break;
        }
        frame(&mut run, &reference);
    }
    settle(&mut run, &reference, 1.);
    let car = run.area.items.resolve_key(&key).expect("Lada item");
    let before = mounted(&run, car);
    let mut vehicle = run.vehicles.remove(&key).unwrap();

    // A 5 km/h stop in one step.
    run.crash_car(&reference, &key, &mut vehicle, 5.);
    assert_eq!(mounted(&run, car), before, "a slow stop drops nothing");

    // A 100 km/h stop in one step.
    run.crash_car(&reference, &key, &mut vehicle, 100.);
    run.vehicles.insert(key, vehicle);
    let after = mounted(&run, car);
    let fallen: Vec<_> = before.iter().filter(|i| !after.contains(i)).collect();
    assert!(!fallen.is_empty(), "a fast stop knocks parts off");
    for item in fallen {
        let location = run.area.items.get(*item).unwrap().location;
        assert_eq!(location, ItemLocation::World, "{item:?}");
    }
}
