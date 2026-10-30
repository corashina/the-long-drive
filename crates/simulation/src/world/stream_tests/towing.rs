//! V05: the Lada tows the small trailer on its hitch. The hitch survives save and load.
use super::drive_out::frame;
use super::*;
use crate::reference::ItemId;
use crate::simulation::{DriveInput, FIXED_DT};
use crate::vehicle::{DriveTorques, to_rapier};
use crate::world::ObjectKey;
use rapier3d::math::Vector;

/// The reference with a `UtanFuto` startup entry whose grip point lies on the hitch
/// target of the `StartCar02` Lada. Returns the car and trailer startup ordinals.
fn towing_reference() -> (ReferenceManifest, u32, u32) {
    let mut reference = reference();
    let world = std::sync::Arc::make_mut(&mut reference.world);
    let start = world
        .startup
        .iter()
        .position(|s| s.group == "StartCar02" && s.prototype.is_some())
        .expect("StartCar02 placement");
    let car = world.startup[start].clone();
    let lada = world
        .prototypes
        .iter()
        .find(|p| Some(&p.source) == car.prototype.as_ref())
        .unwrap();
    assert_eq!(lada.hitches.len(), 1, "Lada hitch");
    let target = crate::world::placement::node_pose(lada, lada.hitches[0].target as usize);
    let trailer = world
        .prototypes
        .iter()
        .find(|p| p.nodes[0].name == "UtanFuto")
        .expect("UtanFuto prototype");
    let record = trailer.trailer.as_ref().expect("trailer record");
    assert_eq!(record.wheels.len(), 2);
    assert!(record.brake.is_none());
    let rotate = |p| crate::world::placement::rotate(car.pose.rotation, p);
    let (point, grip) = (rotate(target.translation), rotate(record.grip.position));
    let mut copy = car.clone();
    copy.prototype = Some(trailer.source.clone());
    copy.pose.translation = std::array::from_fn(|i| car.pose.translation[i] + point[i] - grip[i]);
    world.startup.push(copy);
    let second = world.startup.len() - 1;
    (reference, start as u32, second as u32)
}

fn startup_item(run: &GarageRuntime, ordinal: u32) -> Option<(ObjectKey, ItemId)> {
    run.stream.contents.active.iter().find_map(|c| {
        let id = &c.content.placement.id;
        (id.layer == WorldLayer::Startup && id.ordinal == ordinal && id.children.is_empty())
            .then_some(())
            .and_then(|()| Some((ObjectKey::Generated(id.clone()), c.item?)))
    })
}

/// The grip content item of trailer `trailer`.
fn grip_item(run: &GarageRuntime, trailer: &ObjectKey) -> (ObjectKey, ItemId) {
    let ObjectKey::Generated(parent) = trailer else {
        panic!("generated trailer");
    };
    run.stream
        .contents
        .active
        .iter()
        .find_map(|c| {
            let id = &c.content.placement.id;
            (id.children.len() == parent.children.len() + 1
                && id.children.starts_with(&parent.children)
                && id.ordinal == parent.ordinal
                && id.layer == parent.layer)
                .then_some(())
                .and_then(|()| Some((ObjectKey::Generated(id.clone()), c.item?)))
        })
        .expect("grip item")
}

/// Steps until the car runtime and the trailer binding exist.
fn wait_bound(run: &mut GarageRuntime, reference: &ReferenceManifest, car: u32, trailer: u32) {
    for _ in 0..2000 {
        let ready = startup_item(run, car).is_some_and(|(key, _)| run.vehicles.contains_key(&key))
            && startup_item(run, trailer).is_some_and(|(_, item)| {
                let body = run.area.items.get(item).and_then(|s| s.body);
                body.is_some_and(|b| run.stream.trailer_lights(b).is_some())
            });
        if ready {
            return;
        }
        frame(run, reference);
    }
    panic!("car and trailer did not bind");
}

fn position(run: &GarageRuntime, item: ItemId) -> Vector {
    let body = run.area.items.get(item).unwrap().body.unwrap();
    run.physics.rapier().bodies[body].translation()
}

/// The hitch target point and the trailer grip point, in local world space.
fn hitch_points(
    run: &GarageRuntime,
    reference: &ReferenceManifest,
    car: ItemId,
    trailer: ItemId,
) -> (Vector, Vector) {
    let prototype = |item: ItemId| {
        let key = run.area.items.object_key(item);
        let ObjectKey::Generated(id) = key else {
            panic!("generated item");
        };
        let content = run
            .stream
            .contents
            .active
            .iter()
            .find(|c| c.content.placement.id == id)
            .unwrap();
        reference
            .world
            .prototypes
            .iter()
            .find(|p| p.source == content.content.placement.prototype)
            .unwrap()
    };
    let lada = prototype(car);
    let target = crate::world::placement::node_pose(lada, lada.hitches[0].target as usize);
    let state = run.area.items.get(car).unwrap();
    let target = to_rapier(state.pose)
        * to_rapier(state.local_pose).inverse()
        * Vector::from_array(target.translation);
    let grip = prototype(trailer).trailer.as_ref().unwrap().grip.position;
    let body = run.area.items.get(trailer).unwrap().body.unwrap();
    let grip = *run.physics.rapier().bodies[body].position() * Vector::from_array(grip);
    (target, grip)
}

/// Physics steps with drive torque on car `key`: trailer wheels, physics, hitch sync.
fn tow(run: &mut GarageRuntime, key: &ObjectKey, seconds: f32, input: DriveInput, motor_nm: f32) {
    let torques = DriveTorques {
        motor_nm,
        engine_brake_nm: 0.,
        pedal_nm: None,
    };
    for _ in 0..(seconds / FIXED_DT) as usize {
        run.physics.apply_vehicle_forces(key, input, torques);
        run.stream.step_trailers(&mut run.physics);
        run.physics.step_physics();
        run.area.items.sync_poses(&run.physics);
        run.area
            .interaction
            .hitch
            .sync(&mut run.area.items, &mut run.physics);
    }
    run.physics.refresh_queries();
}

#[test]
#[ignore = "requires the installed source export"]
fn lada_tows_the_trailer_and_the_hitch_saves() {
    let (reference, car, trailer) = towing_reference();
    let stamp = roadside_stamp(&reference);
    let mut run = GarageRuntime::new_default(&reference).unwrap();
    prepare_ready(&mut run, &reference);
    run.started = true;
    run.paused = false;
    wait_bound(&mut run, &reference, car, trailer);
    let (car_key, car_item) = startup_item(&run, car).unwrap();
    let (trailer_key, trailer_item) = startup_item(&run, trailer).unwrap();
    let (grip_key, grip) = grip_item(&run, &trailer_key);
    let lada = &reference.world.prototypes[run.vehicles[&car_key].prototype];
    let hitch = run
        .area
        .interaction
        .hitch
        .resolve_target(car_item, &lada.hitches[0].source)
        .expect("Lada hitch target");
    assert!(run.area.interaction.hitch.is_grip(grip));

    // Attach the grip as the player does: the car and the trailer join at the grip point.
    run.area
        .interaction
        .hitch
        .attach(grip, hitch, &mut run.area.items, &mut run.physics)
        .unwrap();
    assert_eq!(
        run.area.interaction.hitch.towing_car(trailer_item),
        Some(car_item)
    );
    tow(&mut run, &car_key, 2., DriveInput::default(), 0.);
    let start = position(&run, trailer_item);
    let wheel_rotations = |run: &GarageRuntime| -> Vec<[f32; 4]> {
        run.stream
            .trailer(match &trailer_key {
                ObjectKey::Generated(id) => id,
                _ => unreachable!("startup trailer"),
            })
            .expect("bound trailer")
            .graphics()
            .map(|(_, pose)| pose.rotation)
            .collect()
    };
    let rest = wheel_rotations(&run);

    // The car pulls the trailer forward. The grip point stays on the hitch target.
    let throttle = DriveInput {
        throttle: 1.,
        ..Default::default()
    };
    tow(&mut run, &car_key, 3., throttle, 1600.);
    let moved = position(&run, trailer_item) - start;
    assert!(
        Vector::new(moved.x, 0., moved.z).length() > 1.,
        "trailer moved {moved:?}"
    );
    // wheelgraphicsscript: the trailer wheel meshes roll.
    let rolled = wheel_rotations(&run);
    assert!(
        rest.iter().zip(&rolled).all(|(a, b)| a != b),
        "trailer wheel meshes roll"
    );
    let (target, point) = hitch_points(&run, &reference, car_item, trailer_item);
    assert!(
        (target - point).length() < 0.1,
        "grip {point:?} target {target:?}"
    );
    let brake = DriveInput {
        brake: 1.,
        ..Default::default()
    };
    tow(&mut run, &car_key, 4., brake, 0.);

    // Save and load: the grip returns to the car hitch.
    let snapshot = run
        .capture_save(&reference, &stamp, "2026-10-26T12:00:00Z")
        .unwrap();
    let snapshot: crate::save::SaveSnapshot =
        serde_json::from_slice(&serde_json::to_vec(&snapshot).unwrap()).unwrap();
    let mut loaded = GarageRuntime::from_save(
        &reference,
        &stamp,
        &snapshot,
        crate::world::GameMode::default(),
        reference.world.draw_distance.default,
    )
    .unwrap();
    for _ in 0..2000 {
        prepare_ready(&mut loaded, &reference);
        let grip = loaded.area.items.resolve_key(&grip_key);
        if loaded.advance_saved_restore(&reference).unwrap()
            && !loaded.restore_pending()
            && grip.is_some_and(|g| loaded.area.interaction.hitch.is_hitched(g))
        {
            break;
        }
    }
    assert!(!loaded.restore_pending());
    let car_item = loaded.area.items.resolve_key(&car_key).expect("loaded car");
    let trailer_item = loaded
        .area
        .items
        .resolve_key(&trailer_key)
        .expect("loaded trailer");
    let grip = loaded
        .area
        .items
        .resolve_key(&grip_key)
        .expect("loaded grip");
    assert_eq!(
        loaded
            .area
            .interaction
            .hitch
            .hitched_target(grip)
            .map(|(owner, source)| (owner, source.clone())),
        Some((car_item, lada.hitches[0].source.clone()))
    );
    let (target, point) = hitch_points(&loaded, &reference, car_item, trailer_item);
    assert!(
        (target - point).length() < 0.1,
        "loaded grip {point:?} target {target:?}"
    );
}
