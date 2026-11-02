//! W02: long trips. Far edits, revisited buildings, and history size.
use super::drive_out::{enter, park, place_car};
use super::kit::*;
use super::*;
use crate::interaction::items::ItemLocation;
use crate::reference::ItemId;
use crate::world::ObjectKey;

/// Road points from the car start out to `length` metres, one every `stride` metres.
pub(crate) fn road_path(
    run: &GarageRuntime,
    reference: &ReferenceManifest,
    length: f64,
    stride: f64,
) -> Vec<[f64; 3]> {
    let mut roads = crate::world::RoadIndex::new(
        crate::world::RoadSequence::new(&reference.world, run.stream.seed(), None).unwrap(),
        crate::world::terrain_evaluator(&reference.world, run.stream.seed()),
    );
    let mut points: Vec<[f64; 3]> = Vec::new();
    let mut travelled = 0.;
    let mut last: Option<[f64; 3]> = None;
    for index in 0_usize.. {
        roads.prepare(index + 1);
        let point = roads.segment(index as u32).unwrap().position();
        if let Some(previous) = last {
            travelled +=
                ((point[0] - previous[0]).powi(2) + (point[2] - previous[2]).powi(2)).sqrt();
        }
        last = Some(point);
        if points.is_empty() || travelled >= stride * points.len() as f64 {
            points.push(point);
        }
        if travelled > length {
            break;
        }
    }
    points
}

fn absolute(run: &GarageRuntime, id: ItemId) -> [f64; 3] {
    let item = run.area.items.get(id).unwrap();
    item.inactive_pose.map_or_else(
        || run.stream.origin.absolute(item.pose.position),
        |pose| pose.position,
    )
}

fn horizontal(a: [f64; 3], b: [f64; 3]) -> f64 {
    ((a[0] - b[0]).powi(2) + (a[2] - b[2]).powi(2)).sqrt()
}

/// Drive to a road point with the player in the car, and wait for the world.
fn stop(run: &mut GarageRuntime, reference: &ReferenceManifest, path: &[[f64; 3]], index: usize) {
    let next = path[(index + 1).min(path.len() - 1)];
    let next = if next == path[index] {
        path[index - 1]
    } else {
        next
    };
    place_car(run, reference, path[index], next, 0.8);
    park(run, reference, path[index]);
}

/// Generated world contents near a road stop: building items lie about 90 m off the road.
/// The start car and its parts are startup items.
fn roadside(run: &GarageRuntime, focus: [f64; 3]) -> impl Iterator<Item = (ObjectKey, ItemId)> {
    run.area
        .items
        .generated_bindings()
        .filter(|(key, item)| {
            key.layer != crate::world::WorldLayer::Startup
                && item.location == ItemLocation::World
                && item.body.is_some()
        })
        .map(|(key, item)| (ObjectKey::Generated(key.clone()), item.id))
        .filter(move |(_, id)| horizontal(absolute(run, *id), focus) < 150.)
}

/// The nearest loose generated world item near the focus.
fn loose_generated(run: &GarageRuntime, focus: [f64; 3]) -> Option<(ObjectKey, ItemId)> {
    roadside(run, focus).min_by_key(|(_, id)| horizontal(absolute(run, *id), focus) as i64)
}

fn history(run: &GarageRuntime) -> (usize, usize, usize) {
    (
        run.stream.metrics().checkpoint_bytes,
        run.stream.changes.retained_bytes(),
        run.stream.contents.state.boundary_count(),
    )
}

#[test]
#[ignore = "requires the installed source export"]
fn w02_long_trip_keeps_far_edits() {
    const LENGTH: f64 = 20_000.;
    let (reference, mut run) = load_garage();
    assert!(enter(&mut run, &reference));
    let path = road_path(&run, &reference, LENGTH, 250.);
    let start = history(&run);
    let started = std::time::Instant::now();
    // Edited items: (stop index, key, rest position).
    let mut edited: Vec<(usize, ObjectKey, [f64; 3])> = Vec::new();
    // Other generated items near one stop after 15 km.
    let mut building: Option<(usize, Vec<(ObjectKey, [f64; 3])>)> = None;
    for index in 0..path.len() {
        stop(&mut run, &reference, &path, index);
        let km = index as f64 * 0.25;
        let wanted = match edited.len() {
            0 => true,
            1 => km >= 10.,
            _ => false,
        };
        if wanted {
            if let Some((key, id)) = loose_generated(&run, path[index]) {
                let body = run.area.items.get(id).unwrap().body.unwrap();
                run.physics.edit_body(body, |body| {
                    let mut t = body.translation();
                    t.x += 5.;
                    t.y += 1.;
                    body.set_translation(t, true);
                });
                seconds(&mut run, &reference, 3.);
                run.area.items.mark_dirty(id);
                edited.push((index, key, absolute(&run, id)));
            }
        }
        if building.is_none() && km >= 15. {
            // New building items still fall when the stop loads.
            seconds(&mut run, &reference, 10.);
            let near: Vec<_> = roadside(&run, path[index])
                .filter(|(key, _)| edited.iter().all(|(_, k, _)| k != key))
                // A settled item keeps its place.
                .filter(|(_, id)| {
                    let body = run.area.items.get(*id).unwrap().body.unwrap();
                    run.physics.rapier().bodies[body].is_sleeping()
                })
                .map(|(key, id)| (key, absolute(&run, id)))
                .collect();
            if !near.is_empty() {
                building = Some((index, near));
            }
        }
        if index % 20 == 0 {
            eprintln!("km {km}: history {:?}", history(&run));
        }
    }
    let end = history(&run);
    let per_km = (end.0 - start.0) as f64 / (LENGTH / 1000.);
    eprintln!(
        "outward {:?}: checkpoint {} -> {} bytes, {per_km:.0} bytes/km, 5,000 km estimate {:.1} MB; changes {} -> {}; boundaries {} -> {}",
        started.elapsed(),
        start.0,
        end.0,
        per_km * 5000. / 1e6,
        start.1,
        end.1,
        start.2,
        end.2,
    );
    assert_eq!(edited.len(), 2, "two roadside items edited");
    let (site, near) = building.expect("generated contents after 15 km");

    // Return and check each site.
    // Only the items at this stop load.
    let check = |run: &GarageRuntime, stop: usize, stage: &str| {
        for (_, key, rest) in edited.iter().filter(|(index, _, _)| *index == stop) {
            let id = run.area.items.resolve_key(key).expect(stage);
            let now = absolute(run, id);
            assert!(
                horizontal(now, *rest) < 0.3,
                "{stage}: {key:?} {rest:?} -> {now:?}"
            );
        }
    };
    stop(&mut run, &reference, &path, site);
    let moved: Vec<_> = near
        .iter()
        .filter_map(|(key, position)| {
            let id = run
                .area
                .items
                .resolve_key(key)
                .unwrap_or_else(|| panic!("revisited building lost {key:?}"));
            let now = absolute(&run, id);
            (horizontal(now, *position) >= 0.3).then(|| (key.clone(), *position, now))
        })
        .collect();
    assert!(
        moved.is_empty(),
        "{} of {} revisited items moved: {moved:?}",
        moved.len(),
        near.len()
    );
    for (index, key, rest) in edited.iter().rev() {
        stop(&mut run, &reference, &path, *index);
        let id = run
            .area
            .items
            .resolve_key(key)
            .expect("edited item returns");
        let now = absolute(&run, id);
        assert!(horizontal(now, *rest) < 0.3, "{key:?} {rest:?} -> {now:?}");
    }

    // Save at the first edit and reload into a fresh runtime.
    let saved = save(&run, &reference);
    for (_, key, _) in &edited {
        assert!(saved.world.changes.get(key).is_some(), "save keeps {key:?}");
    }
    let mut loaded = load(&reference, &saved);
    seconds(&mut loaded, &reference, 1.);
    check(&loaded, edited[0].0, "after reload");
    stop(&mut loaded, &reference, &path, edited[1].0);
    check(&loaded, edited[1].0, "after reload at the second edit");
}

#[test]
#[ignore = "requires the installed source export"]
fn w02_garage_can_returns_after_far_trip() {
    let (reference, mut run) = load_garage();
    assert!(enter(&mut run, &reference));
    let path = road_path(&run, &reference, 3_200., 250.);
    let can = reference
        .items
        .iter()
        .find(|i| i.name.starts_with("benzineskanna"))
        .expect("20 L can")
        .id;
    // Drop the can beside the road at the start. A far can is inactive: a move does not reach it.
    let drop = 0;
    stop(&mut run, &reference, &path, drop);
    let body = run.area.items.get(can).unwrap().body.unwrap();
    let car = run
        .physics
        .snapshot(run.vehicle.as_ref().unwrap())
        .pose
        .position;
    run.physics.edit_body(body, |body| {
        body.set_translation(
            rapier3d::math::Vector::new(car[0] + 4., car[1] + 1.5, car[2]),
            true,
        );
        body.set_linvel(rapier3d::math::Vector::ZERO, true);
    });
    run.physics.refresh_queries();
    seconds(&mut run, &reference, 4.);
    let rest = absolute(&run, can);
    // Drive about 3 km out.
    for index in drop + 1..path.len() {
        stop(&mut run, &reference, &path, index);
    }
    assert!(horizontal(path[path.len() - 1], rest) > 1900.);
    let item = run.area.items.get(can).unwrap();
    assert!(
        item.inactive_pose.is_some(),
        "far can keeps an absolute pose"
    );
    assert!(run.physics.inactive_bodies.contains(&item.body.unwrap()));
    // Return.
    stop(&mut run, &reference, &path, drop);
    seconds(&mut run, &reference, 2.);
    let item = run.area.items.get(can).unwrap();
    assert!(item.inactive_pose.is_none(), "can is active again");
    let now = absolute(&run, can);
    let delta = (0..3)
        .map(|i| (now[i] - rest[i]).powi(2))
        .sum::<f64>()
        .sqrt();
    assert!(delta < 0.05, "can moved {delta} m: {rest:?} -> {now:?}");
}
