//! P04: leave the starting area and return. Loose items, collision, and re-entry must hold.
use super::*;
use crate::{reference::ItemId, simulation::FrameInput, vehicle::VehiclePose};
use rapier3d::{
    math::{Rotation, Vector},
    prelude::{QueryFilter, Ray},
};

const OUTWARD: f64 = 400.;

thread_local! {
    /// Surface renderer claim kept across frames, as the WASM presentation keeps it.
    static CLAIM: std::cell::RefCell<Option<(WorldRequest, SurfaceKey)>> = const { std::cell::RefCell::new(None) };
}

pub(super) fn frame(run: &mut GarageRuntime, reference: &ReferenceManifest) {
    frame_with(run, reference, FrameInput::default());
}

pub(super) fn frame_with(
    run: &mut GarageRuntime,
    reference: &ReferenceManifest,
    input: FrameInput,
) {
    let mut claim = CLAIM.take();
    let focus = run
        .stream
        .origin
        .absolute(run.area.player.focus_position(&run.physics));
    let mut view = views();
    view[0].origin = focus;
    prepare_step_for_views(run, reference, &mut claim, &view);
    CLAIM.set(claim);
    run.step(reference, input);
    assert!(
        run.stream.frame().failure.is_none(),
        "{:?}",
        run.stream.frame().failure
    );
}

/// Generation follows the focus over several frames. The game also waits here.
fn wait_supported(run: &mut GarageRuntime, reference: &ReferenceManifest) {
    for _ in 0..2000 {
        if run.world_supported() {
            return;
        }
        frame(run, reference);
    }
    panic!("world support did not return");
}

pub(super) fn settle(run: &mut GarageRuntime, reference: &ReferenceManifest, seconds: f32) {
    for _ in 0..(seconds / crate::simulation::FIXED_DT) as usize {
        frame(run, reference);
    }
}

fn request(run: &mut GarageRuntime) {
    let player = run
        .stream
        .origin
        .absolute(run.area.player.focus_position(&run.physics));
    let vehicle = run.persistent_world.absolute_vehicle_position(
        &run.physics,
        &run.stream.origin,
        run.vehicle.as_ref().unwrap(),
    );
    run.stream.request_at(player, vehicle);
}

/// Top of fixed collision below a local point.
pub(super) fn ground(run: &GarageRuntime, local: [f32; 3]) -> Option<f32> {
    let ray = Ray::new(Vector::new(local[0], local[1] + 3., local[2]), -Vector::Y);
    run.physics
        .queries(QueryFilter::only_fixed().exclude_sensors())
        .cast_ray(&ray, 200., true)
        .map(|(_, toi)| ray.origin.y - toi)
}

/// Absolute position of every loose starting-area item.
fn loose_items(
    run: &GarageRuntime,
    reference: &ReferenceManifest,
    resting: bool,
) -> Vec<(ItemId, [f64; 3])> {
    reference
        .items
        .iter()
        .filter_map(|source| {
            let item = run.area.items.get(source.id)?;
            let body = item.body?;
            (item.location == crate::interaction::items::ItemLocation::World
                && run.physics.vehicle_of(body).is_none()
                && run.physics.rapier().bodies[body].is_dynamic()
                && (!resting || run.physics.rapier().bodies[body].is_sleeping()))
            .then_some(())?;
            let position = item.inactive_pose.map_or_else(
                || run.stream.origin.absolute(item.pose.position),
                |pose| pose.position,
            );
            Some((source.id, position))
        })
        .collect()
}

fn item_position(run: &GarageRuntime, id: ItemId) -> [f64; 3] {
    let item = run.area.items.get(id).unwrap();
    item.inactive_pose.map_or_else(
        || run.stream.origin.absolute(item.pose.position),
        |pose| pose.position,
    )
}

fn assert_items_kept(run: &GarageRuntime, before: &[(ItemId, [f64; 3])], stage: &str) {
    let mut moved = Vec::new();
    for &(id, start) in before {
        let now = item_position(run, id);
        let delta = (0..3)
            .map(|i| (now[i] - start[i]).powi(2))
            .sum::<f64>()
            .sqrt();
        if delta > 0.05 {
            moved.push((id, delta, now[1] - start[1]));
        }
    }
    assert!(
        moved.is_empty(),
        "{stage}: {} of {} items moved: {moved:?}",
        moved.len(),
        before.len()
    );
}

pub(super) fn road_points(run: &GarageRuntime, reference: &ReferenceManifest) -> Vec<[f64; 3]> {
    let mut roads = crate::world::RoadIndex::new(
        crate::world::RoadSequence::new(&reference.world, run.stream.seed(), None).unwrap(),
        crate::world::terrain_evaluator(&reference.world, run.stream.seed()),
    );
    let start = run.persistent_world.absolute_vehicle_position(
        &run.physics,
        &run.stream.origin,
        run.vehicle.as_ref().unwrap(),
    );
    let mut points: Vec<[f64; 3]> = Vec::new();
    for index in 0_usize.. {
        roads.prepare(index + 1);
        let point = roads.segment(index as u32).unwrap().position();
        // Park every 25 m along the road.
        if let Some(&last) = points.last() {
            let span = ((point[0] - last[0]).powi(2) + (point[2] - last[2]).powi(2)).sqrt();
            let steps = (span / 25.).floor() as usize;
            for step in 1..steps {
                let t = step as f64 / steps as f64;
                points.push(std::array::from_fn(|i| last[i] + (point[i] - last[i]) * t));
            }
        }
        points.push(point);
        let distance = ((point[0] - start[0]).powi(2) + (point[2] - start[2]).powi(2)).sqrt();
        if distance > OUTWARD {
            break;
        }
    }
    points
}

/// Place the car on the ground at an absolute point, facing along the road.
pub(super) fn place_car(
    run: &mut GarageRuntime,
    reference: &ReferenceManifest,
    point: [f64; 3],
    next: [f64; 3],
    clearance: f32,
) {
    let yaw = (-(next[0] - point[0])).atan2(next[2] - point[2]) as f32;
    let mut pose = VehiclePose {
        position: run.stream.origin.local(point),
        rotation: Rotation::from_rotation_y(yaw).to_array(),
    };
    run.physics
        .reset_preserving_wheels(run.vehicle.as_ref().unwrap(), pose);
    request(run);
    prepare_ready(run, reference);
    pose.position = run.stream.origin.local(point);
    let probe = [pose.position[0], pose.position[1] + 20., pose.position[2]];
    let height = ground(run, probe).expect("ground under the road point");
    pose.position[1] = height + clearance;
    run.physics
        .reset_preserving_wheels(run.vehicle.as_ref().unwrap(), pose);
    run.physics.refresh_queries();
}

fn assert_car_resting(run: &GarageRuntime, stage: &str) {
    let snapshot = run.physics.snapshot(run.vehicle.as_ref().unwrap());
    let up = Rotation::from_array(snapshot.pose.rotation) * Vector::Y;
    assert!(up.y > 0.9, "{stage}: car tipped: {snapshot:?}");
    assert!(
        run.physics
            .wheel_contacts(run.vehicle.as_ref().unwrap())
            .all(|c| c),
        "{stage}: wheels lost the ground: {snapshot:?}"
    );
    assert!(
        snapshot.speed_mps.abs() < 1.,
        "{stage}: car still moving: {snapshot:?}"
    );
    let ground = ground(run, snapshot.pose.position).expect("ground under car");
    assert!(
        snapshot.pose.position[1] > ground,
        "{stage}: car below ground"
    );
}

/// Sits in the driver seat of the start Lada copy.
pub(super) fn enter(run: &mut GarageRuntime, _reference: &ReferenceManifest) -> bool {
    use crate::player::SeatOwner;
    let Some(key) = run.vehicles.keys().find(|key| {
        matches!(key, crate::world::ObjectKey::Generated(id)
            if id.layer == crate::world::WorldLayer::Startup)
    }) else {
        return false;
    };
    let Some(seat) =
        run.physics
            .rapier()
            .colliders
            .iter()
            .find_map(|(handle, _)| {
                run.area.interaction.item_seat(handle).filter(|seat| {
                    seat.owner == SeatOwner::Vehicle(key.clone()) && seat.seat.driver
                })
            })
    else {
        return false;
    };
    run.area.player.enter_item_seat(&seat, &mut run.physics)
}

/// Step until 75 frames simulated. Generation may block some frames.
pub(super) fn park(run: &mut GarageRuntime, reference: &ReferenceManifest, stage: [f64; 3]) {
    wait_supported(run, reference);
    let mut blocked = 0;
    let mut stepped = 0;
    while stepped < 75 {
        if run.world_supported() {
            stepped += 1;
        } else {
            blocked += 1;
            assert!(blocked < 3000, "world support did not return at {stage:?}");
        }
        frame(run, reference);
    }
}

fn assert_frozen(run: &GarageRuntime, items: &[(ItemId, [f64; 3])], stage: &str) {
    // Source `itemPlaceRemoveScript`: items beyond ItemFreezeDist do not simulate.
    for &(id, _) in items {
        let body = &run.physics.rapier().bodies[run.area.items.get(id).unwrap().body.unwrap()];
        assert!(
            !body.is_enabled() || !body.is_dynamic(),
            "{stage}: {id:?} still simulates far from the player and the car"
        );
    }
}

fn assert_returned(
    run: &GarageRuntime,
    before: &[(ItemId, [f64; 3])],
    all: &[(ItemId, [f64; 3])],
    enabled: &[bool],
    stage: &str,
) {
    assert_items_kept(run, before, stage);
    for (&(id, start), &enabled) in all.iter().zip(enabled) {
        let now = item_position(run, id);
        assert!(
            now[1] > start[1] - 0.5,
            "{stage}: {id:?} fell from {start:?} to {now:?}"
        );
        let body = &run.physics.rapier().bodies[run.area.items.get(id).unwrap().body.unwrap()];
        assert!(
            body.is_enabled() == enabled && body.is_dynamic(),
            "{stage}: {id:?} did not wake"
        );
    }
}

#[test]
#[ignore = "requires the installed source export"]
fn p04_drive_out_and_return() {
    let (reference, mut run) = garage_controls::load_garage();
    // Under-map rescue runs every 2 s. Let rescued items land first.
    settle(&mut run, &reference, 8.);
    let car_start = run.physics.snapshot(run.vehicle.as_ref().unwrap()).pose;
    let car_start_absolute = run.stream.origin.absolute(car_start.position);
    let player_start = run.area.player.body_pose(&run.physics);
    let player_start_absolute = run.stream.origin.absolute(player_start.position);
    // Some items never sleep in Rapier. They only must not fall.
    let all = loose_items(&run, &reference, false);
    let before = loose_items(&run, &reference, true);
    let enabled: Vec<_> = all
        .iter()
        .map(|(id, _)| {
            let body = run.area.items.get(*id).unwrap().body.unwrap();
            run.physics.rapier().bodies[body].is_enabled()
        })
        .collect();
    eprintln!(
        "awake items: {:?}",
        all.iter()
            .filter(|(id, _)| !before.iter().any(|(rest, _)| rest == id))
            .map(|(id, _)| &reference.items.iter().find(|i| i.id == *id).unwrap().name)
            .collect::<Vec<_>>()
    );
    assert!(
        before.len() * 2 > all.len(),
        "{} of {} items rest",
        before.len(),
        all.len()
    );
    let points = road_points(&run, &reference);

    // Walk out along the road and back. The car stays in the garage.
    for &point in points.iter().step_by(4).chain(points.last()) {
        let mut pose = player_start;
        pose.position = run.stream.origin.local(point);
        pose.position[1] += 1.5;
        run.area.player.recover(pose, &mut run.physics);
        request(&mut run);
        prepare_ready(&mut run, &reference);
        park(&mut run, &reference, point);
        assert!(
            run.area.player.grounded(),
            "player stands on the road at {point:?}"
        );
    }
    // The parked car is a source important object. Items near it keep simulating.
    let mut pose = player_start;
    pose.position = run.stream.origin.local(player_start_absolute);
    run.area.player.recover(pose, &mut run.physics);
    request(&mut run);
    prepare_ready(&mut run, &reference);
    park(&mut run, &reference, player_start_absolute);
    settle(&mut run, &reference, 2.);
    assert!(run.area.player.grounded(), "player stands in the garage");
    assert_returned(&run, &before, &all, &enabled, "after the walk");

    // Drive out along the road and park at each point.
    assert!(enter(&mut run, &reference));
    let started = std::time::Instant::now();
    for pair in points.windows(2) {
        place_car(&mut run, &reference, pair[0], pair[1], 0.8);
        park(&mut run, &reference, pair[0]);
        assert_car_resting(&run, &format!("road point {:?}", pair[0]));
    }
    eprintln!(
        "outward: {} points in {:?}",
        points.len(),
        started.elapsed()
    );
    settle(&mut run, &reference, 5.);
    assert_frozen(&run, &all, "in the car");

    // Return and park at the start pose.
    let pose = |run: &GarageRuntime| VehiclePose {
        position: run.stream.origin.local(car_start_absolute),
        rotation: car_start.rotation,
    };
    let start = pose(&run);
    run.physics
        .reset_preserving_wheels(run.vehicle.as_ref().unwrap(), start);
    request(&mut run);
    prepare_ready(&mut run, &reference);
    let start = pose(&run);
    run.physics
        .reset_preserving_wheels(run.vehicle.as_ref().unwrap(), start);
    park(&mut run, &reference, car_start_absolute);
    settle(&mut run, &reference, 2.);
    assert_car_resting(&run, "garage");
    assert_returned(&run, &before, &all, &enabled, "after the drive");
}
