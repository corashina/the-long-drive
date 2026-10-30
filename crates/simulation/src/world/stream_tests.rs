//! Native adapter for the same renderer receipts and publication boundary used by WASM.
#[path = "stream_tests/bikes.rs"]
mod bikes;
#[path = "stream_tests/breakables.rs"]
mod breakables;
#[path = "stream_tests/car_types.rs"]
mod car_types;
#[path = "stream_tests/cargo.rs"]
mod cargo;
#[path = "stream_tests/crash.rs"]
mod crash;
#[path = "stream_tests/drive_out.rs"]
mod drive_out;
#[path = "stream_tests/explosions.rs"]
mod explosions;
#[path = "stream_tests/garage_controls.rs"]
mod garage_controls;
#[path = "stream_tests/guns.rs"]
mod guns;
#[path = "stream_tests/item_seats.rs"]
mod item_seats;
#[path = "stream_tests/long_trip.rs"]
mod long_trip;
#[path = "stream_tests/loot_tools.rs"]
mod loot_tools;
#[path = "stream_tests/melee.rs"]
mod melee;
#[path = "stream_tests/parked_car.rs"]
mod parked_car;
#[path = "stream_tests/sled.rs"]
mod sled;
#[path = "stream_tests/towing.rs"]
mod towing;
#[path = "stream_tests/two_cars.rs"]
mod two_cars;
#[path = "stream_tests/world_conditions.rs"]
mod world_conditions;

use super::*;
use crate::starting_area::GarageRuntime;

fn reference() -> ReferenceManifest {
    ReferenceManifest::from_runtime_json(
        &std::fs::read(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../web/assets/reference/manifest.json"
        ))
        .unwrap(),
    )
    .unwrap()
}

#[test]
#[ignore = "requires the installed source export"]
fn terrain_admission_respects_pending_capacity_and_continues_at_capacity() {
    let reference = reference();
    let mut stream = WorldStream::new_default(&reference, 42).unwrap();
    stream.initial_generation_ready = true;
    stream.terrain_ready[0] = true;
    stream.wanted_tiles.push(SurfaceKey {
        layer: WorldLayer::Terrain,
        unit: [0, 0],
    });
    let mut allocation = stream.presentation_credit();
    allocation.pending = MAX_PENDING;
    stream.set_presentation_allocation(allocation).unwrap();

    stream.prepare_terrain(|| true);
    assert!(!stream.terrain_execution.has_pending());
    assert!(stream.ready.is_none());
    assert_eq!(stream.metrics().pending_work, MAX_PENDING);

    allocation.pending -= 1;
    stream.set_presentation_allocation(allocation).unwrap();
    stream.prepare_terrain(|| true);
    assert!(stream.terrain_execution.has_pending());
    assert_eq!(stream.metrics().pending_work, MAX_PENDING);
    stream.set_presentation_allocation(allocation).unwrap();

    for _ in 0..1000 {
        stream.prepare_terrain(|| true);
        assert!(stream.failure.is_none(), "{:?}", stream.failure);
        assert_eq!(stream.metrics().pending_work, MAX_PENDING);
        if stream.ready.is_some() {
            break;
        }
    }
    assert!(!stream.terrain_execution.has_pending());
    assert!(stream.ready.is_some());
    let bytes = stream.metrics().pending_bytes;
    stream.ready = None;
    assert!(stream.metrics().pending_bytes < bytes);

    // A stale request cannot finish or publish its retained terrain work.
    stream.prepare_terrain(|| true);
    assert!(stream.terrain_execution.has_pending());
    let bytes = stream.metrics().pending_bytes;
    stream.request.revision += 1;
    stream.prepare_terrain(|| true);
    assert!(!stream.terrain_execution.has_pending());
    assert!(stream.ready.is_none());
    assert!(stream.metrics().pending_bytes < bytes);
    assert!(
        stream
            .terrain_execution
            .advance(stream.request, || true)
            .unwrap()
            .is_none()
    );
    stream.prepare_terrain(|| true);
    for _ in 0..1000 {
        stream.prepare_terrain(|| true);
        if stream.ready.is_some() {
            break;
        }
    }
    assert!(stream.ready.is_some());
}

#[test]
#[ignore = "requires the installed source export"]
fn structure_refresh_accumulates_both_foci_and_forced_refreshes() {
    let reference = reference();
    let mut run = GarageRuntime::new_default(&reference).unwrap();
    run.stream.request_at([0.; 3], [0.; 3]);
    for _ in 0..10000 {
        run.stream.prepare(100.);
        assert!(run.stream.failure.is_none(), "{:?}", run.stream.failure);
        if run.stream.startup.is_some() {
            break;
        }
    }
    assert!(run.stream.startup.is_some());
    run.stream.prepare(0.);
    assert_eq!(run.stream.plan_revision, run.stream.request.revision);
    assert_eq!(run.stream.structure_focus, [[0.; 3]; 2]);
    let (item, body) = run
        .stream
        .imported_baselines
        .iter()
        .find_map(|(id, _)| {
            let item = run.area.items.get(*id)?;
            item.inactive_pose.is_none().then_some(())?;
            Some((*id, item.body?))
        })
        .expect("imported item with a live physical body");
    let capture_and_prepare = |run: &mut GarageRuntime, player, vehicle, expected| {
        let mut pose = *run.physics.rapier().bodies[body].position();
        pose.translation.y += 0.125;
        run.physics
            .edit_body(body, |body| body.set_position(pose, true));
        run.area.items.sync_poses(&run.physics);
        run.stream.request_at(player, vehicle);
        let captured = run.stream.changes.revision();
        run.stream.capture_changes(
            &reference,
            &mut run.area.items,
            &run.area.interaction,
            &run.physics,
        );
        assert_ne!(run.stream.changes.revision(), captured);
        assert!(
            run.stream
                .changes
                .get(super::super::ObjectKey::Imported(item))
                .is_some()
        );
        let revision = run.stream.request.revision;
        run.stream.prepare(0.);
        assert_ne!(run.stream.request.revision, revision);
        assert_eq!(run.stream.plan_revision, run.stream.request.revision);
        assert_eq!(run.stream.selection_changes, run.stream.changes.revision());
        assert_eq!(run.stream.structure_focus, expected);
        assert!(run.stream.failure.is_none(), "{:?}", run.stream.failure);
    };
    let vehicle = [0.; 3];
    for x in [0.25, 0.5, 0.75] {
        capture_and_prepare(&mut run, [x, 0., 0.], vehicle, [[0.; 3]; 2]);
    }
    capture_and_prepare(&mut run, [1., 0., 0.], vehicle, [[1., 0., 0.], vehicle]);
    capture_and_prepare(
        &mut run,
        [1., 0., 0.],
        [0., 0., 0.75],
        [[1., 0., 0.], vehicle],
    );
    capture_and_prepare(
        &mut run,
        [1., 0., 0.],
        [0., 0., 1.],
        [[1., 0., 0.], [0., 0., 1.]],
    );
    // A forced selection rebuild also moves the distance baseline.
    capture_and_prepare(
        &mut run,
        [1.25, 0., 0.],
        [0., 0., 1.],
        [[1., 0., 0.], [0., 0., 1.]],
    );
    run.stream.plan(true).unwrap();
    assert_eq!(run.stream.structure_focus, [[1.25, 0., 0.], [0., 0., 1.]]);
    capture_and_prepare(
        &mut run,
        [2., 0., 0.],
        [0., 0., 1.],
        [[1.25, 0., 0.], [0., 0., 1.]],
    );
    capture_and_prepare(
        &mut run,
        [2.25, 0., 0.],
        [0., 0., 1.],
        [[2.25, 0., 0.], [0., 0., 1.]],
    );
}

#[test]
#[ignore = "requires the installed source export"]
fn structure_publication_uses_nearest_focus_stable_ties_and_revision_guard() {
    let mut reference = reference();
    // Retain a checked source identity but remove collision from this ordering check.
    let (source, scale) = std::sync::Arc::get_mut(&mut reference.world)
        .unwrap()
        .prototypes
        .edit(|prototypes| {
            let prototype = prototypes
                .iter_mut()
                .find(|p| p.role == crate::reference::WorldPrototypeRole::Structure)
                .unwrap();
            prototype.nodes.clear();
            prototype.bodies.clear();
            prototype.colliders.clear();
            (prototype.source.clone(), prototype.prefab_pose.scale)
        });
    let mut run = GarageRuntime::new_default(&reference).unwrap();
    let stream = &mut run.stream;
    stream.request.player = [0.; 3];
    stream.request.vehicle = [100., 0., 0.];
    stream.wanted_structures.edit(|list| {
        *list = [50., 101., -1.]
            .into_iter()
            .enumerate()
            .map(|(index, x)| {
                let mut p = replay_placement(&source, index as u32, [x, 0., 0.]);
                p.scale = scale;
                p
            })
            .collect()
    });
    stream.hold_prototype(source.clone()).unwrap();
    stream.bind_prototype(&source).unwrap();
    stream.bind_structure_recipe(&source).unwrap();
    for index in 0..stream.wanted_structures.len() {
        let id = stream.wanted_structures[index].id.clone();
        accept_structure(stream, &id);
    }
    assert_ne!(stream.plan_revision, stream.request.revision);
    stream.publish_structure(&mut run.physics, &[]).unwrap();
    assert!(
        stream.structures.is_empty(),
        "stale selection cannot publish"
    );
    stream.plan_revision = stream.request.revision;
    for ordinal in [1, 2, 0] {
        stream.publish_structure(&mut run.physics, &[]).unwrap();
        assert_eq!(
            stream.structures.last().unwrap().placement.id.ordinal,
            ordinal
        );
    }
    assert!(stream.ready_structures.is_empty());
}
/// The native adapter charges and hands over one structure claim, as the renderer ledger does.
fn accept_structure(stream: &mut WorldStream, id: &super::super::GeneratedId) {
    stream.presentation.pending += 1;
    stream
        .accept_structure_renderer(stream.request, id)
        .unwrap();
}
fn views() -> [super::super::RequiredView; 1] {
    let planes = [
        [1., 0., 0., 150.],
        [-1., 0., 0., 150.],
        [0., 1., 0., 150.],
        [0., -1., 0., 150.],
        [0., 0., 1., 150.],
        [0., 0., -1., 150.],
    ];
    [super::super::RequiredView {
        camera: planes,
        shadow: crate::reference::shadow_candidates::DirectionalHull::new(planes, [1., 0., 0.])
            .unwrap(),
        mask: u32::MAX,
        light_mask: u32::MAX,
        origin: [0.; 3],
    }]
}
fn prepare_step(
    run: &mut GarageRuntime,
    reference: &ReferenceManifest,
    surface_claim: &mut Option<(WorldRequest, SurfaceKey)>,
) {
    prepare_step_for_views(run, reference, surface_claim, &views());
}
fn prepare_step_for_views(
    run: &mut GarageRuntime,
    reference: &ReferenceManifest,
    surface_claim: &mut Option<(WorldRequest, SurfaceKey)>,
    required_views: &[super::super::RequiredView],
) {
    let stream = &mut run.stream;
    stream.prepare(100.);
    if stream
        .ready
        .as_ref()
        .is_some_and(|w| !same_request(w.request, stream.request))
    {
        if let Some((request, key)) = surface_claim.take() {
            stream.discard_surface_renderer(request, key).unwrap();
        }
    }
    stream.select_required(required_views);
    let sources: Vec<_> = reference
        .world
        .terrain
        .materials
        .iter()
        .map(|m| m.source.clone())
        .chain(reference.world.road.prototypes.iter().cloned())
        .collect();
    for source in sources {
        stream.hold_prototype(source.clone()).unwrap();
        stream.bind_prototype(&source).unwrap();
    }
    let placement = stream
        .wanted_structures
        .iter()
        .find(|p| {
            (stream.structure_required(&p.id)
                || stream.structure_replay_required(&p.id)
                || stream.lights.iter().any(|light| {
                    stream.publication_light(light, &stream.required.lights)
                        && light.owned_by(&p.id)
                }))
                && stream.contents_initialized(&p.id)
                && !stream.structures.iter().any(|s| s.placement.id == p.id)
                && !stream.ready_structures.contains(&p.id)
        })
        .cloned()
        // The native adapter reserves one instance claim and no grass child job.
        .filter(|_| stream.can_request_independent(1));
    if let Some(p) = placement {
        stream.hold_prototype(p.prototype.clone()).unwrap();
        stream.bind_prototype(&p.prototype).unwrap();
        stream.bind_structure_recipe(&p.prototype).unwrap();
        accept_structure(stream, &p.id);
    }
    if let Some((request, surface)) = stream.prepared_surface() {
        let key = surface.key;
        if !stream.ready.as_ref().unwrap().renderer_reserved {
            stream.reserve_surface_renderer(request, key).unwrap();
            stream.accept_surface_renderer(request, key).unwrap();
            *surface_claim = Some((request, key));
        }
    }
    let lights = stream.required.lights.clone();
    stream.publish(&mut run.physics, &lights).unwrap();
    if stream.ready.is_none() {
        *surface_claim = None;
    }
    stream
        .sync_supplies(
            reference,
            &mut run.area.items,
            &mut run.area.interaction,
            &mut run.physics,
            &mut run.wheels,
            &mut run.vehicles,
            &mut run.powertrain,
        )
        .unwrap();
    assert!(stream.failure.is_none(), "{:?}", stream.failure);
    run.align_startup(reference, [1, 1]).unwrap();
}

pub fn prepare_ready(run: &mut GarageRuntime, reference: &ReferenceManifest) {
    let mut surface_claim = None;
    for step in 0..2000 {
        prepare_step(run, reference, &mut surface_claim);
        if run.stream.required_ready() {
            if let Some((request, key)) = surface_claim.take() {
                assert!(!run.stream.required.terrain.contains(&key));
                assert!(
                    key.layer != WorldLayer::Road
                        || !run.stream.required.roads.contains(&(key.unit[0] as u32))
                );
                run.stream.discard_surface_renderer(request, key).unwrap();
            }
            return;
        }
        if step == 1999 {
            println!("stalled: {}", run.stream.startup_job_audit());
        }
    }
    panic!("required publication did not reach readiness");
}
/// The renderer copy answers like the stream after one encoded delta.
fn assert_renderer_world(stream: &WorldStream) {
    let reference = stream.world_reference().clone();
    let mut sent = renderer_world::RendererWorld::new(reference.clone());
    let mut delta = renderer_world::WorldDelta::default();
    let encoded = |delta: &renderer_world::WorldDelta| -> renderer_world::WorldDelta {
        postcard::from_bytes(&postcard::to_allocvec(delta).unwrap()).unwrap()
    };
    stream.sync_renderer(&mut sent, &mut delta);
    let mut world = renderer_world::RendererWorld::new(reference);
    world.apply(&encoded(&delta));
    let frame = world.frame();
    assert_eq!(frame.surfaces.len(), stream.surfaces.len());
    assert_eq!(frame.structures.len(), stream.structures.len());
    assert_eq!(frame.lights.len(), stream.lights.len());
    assert_eq!(world.required_ready(), stream.required_ready());
    assert_eq!(world.structure_membership(), stream.structure_membership());
    assert_eq!(world.pending_structures(), &stream.wanted_structures[..]);
    for p in stream.wanted_structures.iter() {
        assert_eq!(
            world.contents_initialized(&p.id),
            stream.contents_initialized(&p.id)
        );
        assert_eq!(
            world.structure_required(&p.id),
            stream.structure_required(&p.id)
        );
        assert_eq!(
            world.structure_replay_required(&p.id),
            stream.structure_replay_required(&p.id)
        );
    }
    for p in stream
        .wanted_structures
        .iter()
        .chain(stream.structures.iter().map(|s| &s.placement))
    {
        assert_eq!(world.appearance(&p.id), stream.appearance(&p.id));
        assert_eq!(
            world.prop_material(p),
            stream.props.material(p),
            "prop material"
        );
    }
    for source in stream.reference.road.prototypes.iter() {
        let required = stream.selected_roads.iter().any(|r| {
            (stream.required.roads.contains(&r.segment.index) || stream.required_ready())
                && stream.reference.road.prototypes[r.segment.biome as usize] == *source
        });
        assert_eq!(world.road_prototype_required(source), required);
        assert_eq!(
            world.can_hold_prototype(source),
            stream.can_hold_prototype(source)
        );
    }
    // A second sync with no stream change keeps the copy.
    let surfaces = world.frame().surfaces.as_ptr();
    stream.sync_renderer(&mut sent, &mut delta);
    world.apply(&encoded(&delta));
    assert_eq!(world.frame().surfaces.as_ptr(), surfaces);
}
fn withdraw_terrain(run: &mut GarageRuntime, key: SurfaceKey) {
    let index = run
        .stream
        .surfaces
        .iter()
        .position(|s| s.key == key)
        .unwrap();
    let mut surface = run.stream.surfaces.remove(index);
    run.physics
        .remove_collider(surface.collider.take().unwrap());
    surface.collision_bytes = 0;
    assert!(run.stream.ready.is_none());
    run.stream.ready = Some(PreparedSurface {
        road_claim: None,
        pending_bytes: 0,
        renderer_ready: false,
        offered: true,
        renderer_reserved: false,
        request: run.stream.request,
        published: surface,
    });
}
fn publish_with_receipts(run: &mut GarageRuntime) {
    let stream = &mut run.stream;
    let (request, surface) = stream.prepared_surface().unwrap();
    let key = surface.key;
    stream.reserve_surface_renderer(request, key).unwrap();
    // The renderer accepts only after its uploads finish. Until then publication waits.
    let lights = stream.required.lights.clone();
    stream.publication_available = true;
    stream.publish(&mut run.physics, &lights).unwrap();
    assert!(
        !stream.required_ready(),
        "renderer work must block readiness"
    );
    stream.accept_surface_renderer(request, key).unwrap();
    let lights = stream.required.lights.clone();
    stream.publication_available = true;
    stream.publish(&mut run.physics, &lights).unwrap();
}
#[test]
#[ignore = "requires the installed source export"]
fn required_publication_reaches_ready_and_withholds_dependencies() {
    let reference = reference();
    let mut run = GarageRuntime::new_default(&reference).unwrap();
    prepare_ready(&mut run, &reference);
    assert!(run.stream.required_ready());
    assert!(run.world_supported());
    assert_renderer_world(&run.stream);
    println!(
        "ready: roads={} tiles={} structures={} lights={}; active bindings={} static={}",
        run.stream.required.roads.len(),
        run.stream.required.terrain.len(),
        run.stream.required.structures.len(),
        run.stream.required.lights.len(),
        run.stream.contents.active.len(),
        run.stream.static_controls.len()
    );
    let key = *run
        .stream
        .required
        .terrain
        .iter()
        .find(|k| k.layer == WorldLayer::Terrain)
        .unwrap();
    withdraw_terrain(&mut run, key);
    assert!(
        !run.stream.required_ready(),
        "unpublished geometry/collision must block readiness"
    );
    publish_with_receipts(&mut run);
    assert!(run.stream.required_ready());
    let light = run
        .stream
        .lights
        .iter()
        .find(|l| run.stream.required.lights.contains(&l.id))
        .unwrap();
    let index = run
        .stream
        .structures
        .iter()
        .position(|s| light.owned_by(&s.placement.id))
        .unwrap();
    let owner = run.stream.structures.remove(index);
    assert!(
        !run.stream.required_ready(),
        "required light owner must publish"
    );
    run.stream.structures.insert(index, owner);
    assert!(run.stream.required_ready());
    mechanism_binding_gate(&mut run, &reference);
    swept_boundaries(&mut run, &reference);
}

#[test]
#[ignore = "requires the installed source export"]
fn terrain_sample_matches_published_ground() {
    let reference = reference();
    let mut run = GarageRuntime::new_default(&reference).unwrap();
    prepare_ready(&mut run, &reference);
    let player = run.area.player.body_pose(&run.physics).position;
    let mut checked = 0;
    // Both Z signs: a sample with the source Z sign flipped misses one side.
    for offset in [
        [40., 0., 40.],
        [40., 0., -40.],
        [-60., 0., 25.],
        [-25., 0., -60.],
    ] {
        let point: [f32; 3] = std::array::from_fn(|i| player[i] + offset[i]);
        let Some(ground) = run.stream.ground_height(&run.physics, point) else {
            continue;
        };
        let (height, _) = run.stream.terrain_sample(run.stream.origin.absolute(point));
        assert!(
            (height - ground).abs() < 1.,
            "{point:?}: {height} != {ground}"
        );
        checked += 1;
    }
    assert!(checked >= 2);
}

fn move_player(run: &mut GarageRuntime, position: [f32; 3]) {
    let old = run.area.player.body_pose(&run.physics).position;
    run.area
        .player
        .rebase(std::array::from_fn(|i| old[i] - position[i]));
    let pose = run.area.player.body_pose(&run.physics);
    let body = run.area.player.body_handle();
    run.physics.edit_body(body, |body| {
        body.set_position(crate::vehicle::to_rapier(pose), true)
    });
    run.physics.refresh_queries();
}
fn swept_boundaries(run: &mut GarageRuntime, reference: &ReferenceManifest) {
    use crate::simulation::{FrameInput, keys};
    use rapier3d::math::Vector;
    let original_player = run.area.player.body_pose(&run.physics).position;
    let original_car = run.physics.snapshot(run.vehicle.as_ref().unwrap()).pose;
    let size = run.stream.tiles[0].size as f32;
    let boundary = size * 0.5;
    let key = SurfaceKey {
        layer: WorldLayer::Terrain,
        unit: [1, 0],
    };
    move_player(run, [boundary - 7.5, original_player[1] + 10., 0.]);
    run.started = true;
    assert!(run.world_supported());
    withdraw_terrain(run, key);
    let before = run.area.player.body_pose(&run.physics);
    assert!(
        run.stream
            .supports(window(run.stream.origin.absolute(before.position), 1.)),
        "player's current tile stays published"
    );
    assert!(
        !run.world_supported(),
        "player movement envelope reaches the unpublished neighbor"
    );
    for _ in 0..4 {
        run.step(
            reference,
            FrameInput {
                held: keys::RIGHT,
                ..Default::default()
            },
        );
    }
    assert_eq!(
        run.area.player.body_pose(&run.physics),
        before,
        "player must stop before unresolved collision"
    );
    publish_with_receipts(run);
    assert!(run.world_supported());
    run.step(
        reference,
        FrameInput {
            held: keys::RIGHT,
            ..Default::default()
        },
    );
    let after = run.area.player.body_pose(&run.physics);
    assert!(
        (after.position[0] - before.position[0]).abs()
            + (after.position[2] - before.position[2]).abs()
            > 1e-5,
        "player advances after collision publication"
    );

    let player = run.stream.origin.local([
        f64::from(boundary - 18.),
        f64::from(original_player[1] + 10.),
        0.,
    ]);
    move_player(run, player);
    let mut pose = original_car;
    pose.position = run.stream.origin.local([
        f64::from(boundary - 9.),
        f64::from(original_car.position[1] + 10.),
        0.,
    ]);
    run.physics
        .reset_preserving_wheels(run.vehicle.as_ref().unwrap(), pose);
    run.stream.request_at(
        run.stream
            .origin
            .absolute(run.area.player.focus_position(&run.physics)),
        run.stream.origin.absolute(
            run.physics
                .snapshot(run.vehicle.as_ref().unwrap())
                .pose
                .position,
        ),
    );
    prepare_ready(run, reference);
    run.sync_world_activity(reference).unwrap();
    run.physics
        .reset_preserving_wheels(run.vehicle.as_ref().unwrap(), pose);
    assert_eq!(
        run.physics.snapshot(run.vehicle.as_ref().unwrap()).pose,
        pose
    );
    let chassis = run.physics.chassis_handle(run.vehicle.as_ref().unwrap());
    assert!(
        run.physics.body_active(chassis),
        "vehicle must be active for the swept guard"
    );
    run.physics
        .edit_body(chassis, |body| body.set_linvel(Vector::ZERO, true));
    withdraw_terrain(run, key);
    assert!(
        run.world_supported(),
        "stationary car remains in the prepared region"
    );
    run.physics.edit_body(chassis, |body| {
        body.set_linvel(Vector::new(120., 0., 0.), true)
    });
    assert!(
        !run.world_supported(),
        "vehicle swept support includes its next step: pose={:?} velocity={:?} origin={:?} boundary={}",
        run.physics
            .snapshot(run.vehicle.as_ref().unwrap())
            .pose
            .position,
        run.physics.rapier().bodies[chassis].linvel(),
        run.stream.origin.offset,
        boundary
    );
    let before = run.physics.snapshot(run.vehicle.as_ref().unwrap()).pose;
    for _ in 0..4 {
        run.step(reference, FrameInput::default());
    }
    assert_eq!(
        run.physics.snapshot(run.vehicle.as_ref().unwrap()).pose,
        before,
        "vehicle must stop before unresolved collision"
    );
    publish_with_receipts(run);
    assert!(run.world_supported());
    run.step(reference, FrameInput::default());
    assert!(
        run.physics
            .snapshot(run.vehicle.as_ref().unwrap())
            .pose
            .position[0]
            > before.position[0] + 0.01,
        "vehicle advances after collision publication"
    );
}

fn mechanism_binding_gate(run: &mut GarageRuntime, reference: &ReferenceManifest) {
    let prototype = reference
        .world
        .prototypes
        .iter()
        .find(|p| {
            reference
                .world
                .mechanisms
                .iter()
                .any(|m| m.prototype == p.source)
                && run
                    .stream
                    .contents
                    .rules
                    .component(&reference.world, &p.source, "buildingscript")
                    .unwrap()
                    .is_none()
        })
        .unwrap();
    let mut placement = run.stream.wanted_structures[0].clone();
    placement.prototype = prototype.source.clone();
    placement.id.layer = WorldLayer::Nested;
    placement.id.children.push((prototype.source.clone(), 999));
    placement.pose.position = run.stream.request.player;
    placement.bounds = window(placement.pose.position, 2.);
    placement.scale = prototype.prefab_pose.scale;
    let id = placement.id.clone();
    run.stream.wanted_structures.push(placement);
    run.stream.select_required(&views());
    assert!(run.stream.structure_required(&id));
    assert!(!run.stream.required_ready());
    run.stream.hold_prototype(prototype.source.clone()).unwrap();
    run.stream.bind_prototype(&prototype.source).unwrap();
    run.stream.bind_structure_recipe(&prototype.source).unwrap();
    accept_structure(&mut run.stream, &id);
    run.stream.publication_available = true;
    let lights = run.stream.required.lights.clone();
    run.stream
        .publish_structure(&mut run.physics, &lights)
        .unwrap();
    run.stream
        .sync_static_controls(
            reference,
            &mut run.area.items,
            &mut run.area.interaction,
            &mut run.physics,
            &mut run.wheels,
        )
        .unwrap();
    let content = run
        .stream
        .static_controls
        .iter()
        .find(|c| c.content.placement.id == id)
        .unwrap();
    assert!(content.item.is_some());
    assert!(
        !content.bindings.is_empty(),
        "real mechanism must bind a control"
    );
    assert!(
        !run.stream.required_ready(),
        "unfinished control/interaction restoration blocks readiness"
    );
    run.stream
        .sync_supplies(
            reference,
            &mut run.area.items,
            &mut run.area.interaction,
            &mut run.physics,
            &mut run.wheels,
            &mut run.vehicles,
            &mut run.powertrain,
        )
        .unwrap();
    assert!(run.stream.bindings_ready(&id));
    assert!(
        run.stream.required_ready(),
        "control publication reaches terminal readiness"
    );
}

#[test]
#[ignore = "requires the installed source export"]
fn roadside_owner_controls_keep_state_after_owner_retirement() {
    let reference = reference();
    let mut run = GarageRuntime::new_default(&reference).unwrap();
    prepare_ready(&mut run, &reference);
    mechanism_binding_gate(&mut run, &reference);
    let owner = run
        .stream
        .static_controls
        .iter()
        .find(|c| c.item.is_some())
        .unwrap();
    let item = owner.item.unwrap();
    let key = run.area.items.object_key(item);
    let instance = owner.content.placement.id.clone();
    let baseline = super::super::changes::capture(
        item,
        &run.area.items,
        &run.area.interaction,
        &run.physics,
        &reference,
    )
    .unwrap();
    let mut changed = baseline.clone();
    changed.temperature = Some(baseline.temperature.unwrap_or(0.) + 1.);
    run.stream.changes.record(key.clone(), changed, &baseline);
    run.stream
        .changes
        .set_replay(&key, &owner.origin, &instance);
    let source_tank = reference
        .tanks
        .iter()
        .find(|t| t.role == crate::reference::TankRole::Supply)
        .unwrap();
    let source_cap = reference
        .world
        .records
        .iter()
        .find(|record| {
            matches!(record.fields.get("Tank"), Some(crate::reference::WorldValue::Link(source)) if source == &source_tank.source)
        })
        .unwrap()
        .source
        .clone();
    let placement = owner.content.placement.clone();
    let prototype = reference
        .world
        .prototypes
        .iter()
        .find(|p| p.source == placement.prototype)
        .unwrap();
    let definition = super::super::placement::SupplyDefinition {
        tank: source_tank.source.clone(),
        cap: source_cap,
        tank_pose: reference.world.generation_frame,
        cap_pose: reference.world.generation_frame,
        fluids: source_tank
            .fluids
            .iter()
            .map(|entry| super::super::placement::SupplyFluid {
                fluid_type: i32::from(entry.fluid_type),
                amount: entry.amount,
            })
            .collect(),
        capacity: source_tank.capacity_liters,
        no_fill_from: source_tank.no_fill_from,
        no_fill_this: source_tank.no_fill_this,
        no_pour: false,
        emission_min_rate: 0.,
        emission_max_rate: 0.,
        explosion: crate::world::TankExplosion {
            never_explode: true,
            always_explode: false,
            always_explode_force: 0.,
            force_needed: 0.,
            explosion_type: 0,
            no_shoot_hole: false,
            no_shoot_explode: false,
            parent_dest: None,
        },
    };
    let mut tank_key = instance.clone();
    tank_key.children.push((definition.tank.clone(), 0));
    let mut cap_key = instance.clone();
    cap_key.children.push((definition.cap.clone(), 0));
    let tank = run
        .area
        .items
        .insert_generated(tank_key, prototype, placement.pose)
        .unwrap();
    let cap = run
        .area
        .items
        .insert_generated(cap_key, prototype, placement.pose)
        .unwrap();
    run.area
        .items
        .initialize_generated_supply(tank, &definition)
        .unwrap();
    let supply_key = run.area.items.object_key(tank);
    let supply_baseline = run.area.items.generated_snapshot(tank, None).unwrap();
    let mut supply_changed = supply_baseline.clone();
    supply_changed
        .fluids
        .as_mut()
        .unwrap()
        .as_mut_slice()
        .iter_mut()
        .find(|entry| entry.fluid_type == 3)
        .unwrap()
        .amount -= 1.;
    run.stream
        .changes
        .record(supply_key.clone(), supply_changed, &supply_baseline);
    run.stream
        .changes
        .set_replay(&supply_key, &placement, &instance);
    run.stream.supplies.push(PublishedSupply {
        key: instance.clone(),
        point: super::super::SupplyPoint::nested(placement.pose, placement.pose),
        definition,
        items: Some((tank, cap)),
        baselines: Some([
            supply_baseline,
            run.area.items.generated_snapshot(cap, None).unwrap(),
        ]),
    });
    let (imported, sibling_baseline) = run.stream.imported_baselines[0].clone();
    let sibling_key = super::super::ObjectKey::Imported(imported);
    let mut sibling_changed = sibling_baseline.clone();
    sibling_changed.temperature = Some(sibling_baseline.temperature.unwrap_or(0.) + 1.);
    run.stream
        .changes
        .record(sibling_key.clone(), sibling_changed, &sibling_baseline);
    run.stream.request_at([10000., 0., 0.], [10000., 0., 0.]);
    assert!(run.stream.changes.get(&key).is_some());
    assert!(run.stream.changes.get(&supply_key).is_some());
    run.stream.request_at([0.; 3], [0.; 3]);
    prepare_ready(&mut run, &reference);
    run.stream.wanted_structures.retain(|p| p.id != instance);
    run.stream
        .retire_structures(
            &mut run.physics,
            &mut run.area.items,
            &mut run.area.interaction,
            &mut run.wheels,
            &mut run.vehicles,
            &mut run.powertrain,
        )
        .unwrap();
    assert!(
        !run.stream
            .structures
            .iter()
            .any(|s| s.placement.id == instance)
    );
    assert!(run.area.items.resolve_key(&key).is_none());
    assert!(run.area.items.resolve_key(&supply_key).is_none());
    assert!(run.stream.changes.get(&key).is_some());
    assert!(run.stream.changes.get(&supply_key).is_some());
    assert!(run.stream.changes.get(&sibling_key).is_some());
}

#[test]
#[ignore = "requires the installed source export"]
fn roadside_lamp_state_survives_its_source_owner() {
    let reference = reference();
    let mut run = GarageRuntime::new_default(&reference).unwrap();
    prepare_ready(&mut run, &reference);
    let garage_player = run
        .stream
        .origin
        .absolute(run.area.player.focus_position(&run.physics));
    let garage_car = run.stream.origin.absolute(
        run.physics
            .snapshot(run.vehicle.as_ref().unwrap())
            .pose
            .position,
    );
    let mut sequence = super::super::BuildingSequence::new(&reference.world, run.stream.seed());
    let site = (0..4).filter_map(|_| sequence.next()).last().unwrap();
    let focus = [
        site.pose.position[0],
        site.pose.position[1] + 2.,
        site.pose.position[2],
    ];
    let local = run.stream.origin.local(focus);
    move_player(&mut run, local);
    let mut car = run.physics.snapshot(run.vehicle.as_ref().unwrap()).pose;
    car.position = run.stream.origin.local(focus);
    run.physics
        .reset_preserving_wheels(run.vehicle.as_ref().unwrap(), car);
    run.stream.request_at(focus, focus);
    let mut claim = None;
    for _ in 0..300 {
        let required = roadside_views(&run.stream);
        prepare_step_for_views(&mut run, &reference, &mut claim, &required);
        if run.stream.lamp_bindings.iter().any(|b| b.parent == site.id) {
            break;
        }
    }
    let lamps: Vec<_> = run
        .stream
        .lamp_bindings
        .iter()
        .filter(|b| b.parent == site.id)
        .map(|b| {
            let mut id = b.parent.clone();
            id.children.push((b.source.clone(), 0));
            (super::super::ObjectKey::Generated(id), b.control)
        })
        .collect();
    assert_eq!(lamps.len(), 3);
    assert!(
        run.stream
            .use_generated_light(lamps[0].1, &mut run.area.items)
            .unwrap()
    );
    assert!(run.stream.changes.get(&lamps[0].0).is_some());
    assert!(run.stream.changes.get(&lamps[0].0).is_some());
    let garage_local = run.stream.origin.local(garage_player);
    move_player(&mut run, garage_local);
    car.position = run.stream.origin.local(garage_car);
    run.physics
        .reset_preserving_wheels(run.vehicle.as_ref().unwrap(), car);
    run.stream.request_at(garage_player, garage_car);
    for _ in 0..300 {
        let required = roadside_views(&run.stream);
        prepare_step_for_views(&mut run, &reference, &mut claim, &required);
        if run.stream.supports(window(run.stream.request.player, 3.))
            && run.stream.supports(window(run.stream.request.vehicle, 4.))
        {
            break;
        }
    }
    assert!(run.stream.supports(window(run.stream.request.player, 3.)));
    assert!(run.stream.supports(window(run.stream.request.vehicle, 4.)));
    run.stream
        .retire_contents(
            &mut run.physics,
            &mut run.area.items,
            &mut run.area.interaction,
            &mut run.wheels,
            &mut run.vehicles,
            &mut run.powertrain,
        )
        .unwrap();
    for _ in 0..20 {
        run.stream
            .retire_structures(
                &mut run.physics,
                &mut run.area.items,
                &mut run.area.interaction,
                &mut run.wheels,
                &mut run.vehicles,
                &mut run.powertrain,
            )
            .unwrap();
        if !run
            .stream
            .structures
            .iter()
            .any(|s| s.placement.id == site.id)
        {
            break;
        }
    }
    assert!(
        !run.stream
            .structures
            .iter()
            .any(|s| s.placement.id == site.id)
    );
    run.stream
        .sync_lamp_bindings(&run.area.items, &mut run.area.interaction, &mut run.physics)
        .unwrap();
    assert!(!run.stream.lamp_bindings.iter().any(|b| b.parent == site.id));
    for (key, _) in lamps.iter().take(1) {
        assert!(run.stream.changes.get(key).is_some());
    }
}

#[test]
#[ignore = "requires the installed source export"]
fn complete_probe_union_preserves_revision_and_fresh_cycles() {
    use crate::{
        reference::reflections::{ProbeJob, ProbeSchedule},
        render_readiness::{
            ContentKey, ContentPreparation, EffectsObservation, GraphObservation, ProbeObservation,
            ReadinessRequest, RenderIdentity, RendererReadiness, RendererStatus, Validation,
        },
        render_scale::SceneScale,
        world::{REQUIRED_VIEW_CAPACITY, RequiredView, RequiredViews},
    };
    let reference = reference();
    let mut run = GarageRuntime::new_default(&reference).unwrap();
    prepare_ready(&mut run, &reference);
    let tiny = |center: [f64; 3], mask| {
        let [x, y, z] = center.map(|v| v as f32);
        let planes = [
            [1., 0., 0., 0.25 - x],
            [-1., 0., 0., 0.25 + x],
            [0., 1., 0., 0.25 - y],
            [0., -1., 0., 0.25 + y],
            [0., 0., 1., 0.25 + z],
            [0., 0., -1., 0.25 - z],
        ];
        RequiredView {
            camera: planes,
            shadow: crate::reference::shadow_candidates::DirectionalHull::new(planes, [1., 0., 0.])
                .unwrap(),
            mask,
            light_mask: 0,
            origin: [0.; 3],
        }
    };
    // One probe face union includes distinct published geometry.
    let first_structure = run.stream.structures.iter().max_by(|a, b| {
        let a = a.placement.bounds.min[0].abs() + a.placement.bounds.min[2].abs();
        let b = b.placement.bounds.min[0].abs() + b.placement.bounds.min[2].abs();
        a.total_cmp(&b)
    });
    let center = first_structure
        .map(|s| {
            std::array::from_fn(|i| (s.placement.bounds.min[i] + s.placement.bounds.max[i]) * 0.5)
        })
        .expect("published source structures");
    let first = tiny(center, u32::MAX);
    let centers: Vec<_> = run
        .stream
        .structures
        .iter()
        .map(|s| {
            std::array::from_fn(|i| (s.placement.bounds.min[i] + s.placement.bounds.max[i]) * 0.5)
        })
        .collect();
    run.stream.select_required(&[first]);
    let first_ids = run.stream.required.structures.clone();
    let mut pair = None;
    for center in centers {
        let second = tiny(center, u32::MAX);
        run.stream.select_required(&[second]);
        let second_ids = &run.stream.required.structures;
        if first_ids.iter().any(|id| !second_ids.contains(id))
            && second_ids.iter().any(|id| !first_ids.contains(id))
        {
            pair = Some((second, second_ids.clone()));
            break;
        }
    }
    let (second, second_ids) = pair.expect("two face-exclusive published regions");
    let seed = tiny([0.; 3], 0);
    let mut required = RequiredViews::new(seed);
    let build = |required: &mut RequiredViews, enabled: [bool; 1]| {
        required.reset(seed);
        required
            .append_probe_faces(&enabled, |_, side| Ok([first, second][side % 2]))
            .unwrap();
    };
    build(&mut required, [true]);
    let storage = required.as_slice().as_ptr();
    run.stream.select_required(required.as_slice());
    let ids = run.stream.required.structures.clone();
    assert!(first_ids.iter().all(|id| ids.contains(id)));
    assert!(second_ids.iter().all(|id| ids.contains(id)));
    assert_eq!(required.as_slice().len(), 7);
    assert!(run.stream.required_ready());
    let revision = run.stream.required.revision;
    let key = RenderIdentity {
        session: run.stream.request.session,
        scene: 1,
        native_extent: [800, 600],
        scene_extent: [800, 600],
        scale: SceneScale::default(),
    };
    let mut request = ReadinessRequest {
        render: key,
        selection_revision: revision,
        probe_mask: 1,
        history_revision: 0,
    };
    let mut policy = RendererReadiness::default();
    let validation = Validation {
        requested: 1,
        completed: 1,
        validated: 1,
    };
    let mut graph = GraphObservation {
        identity: key,
        frame: 1,
        token: 1,
        sky: true,
        world_mask: 1,
        display_encoded: true,
        submitted_token: 1,
        targets_ready: true,
        lighting_textures: 1,
        lighting_gpu_textures: 1,
        surface_ready: true,
        camera_ready: true,
        sky_ready: true,
        cloud_ready: true,
        caster_ready: true,
        probes: [ProbeObservation {
            pipelines_ready: true,
            published: false,
            validation,
        }; crate::render_readiness::PROBE_COUNT],
        validation,
    };
    let mut effects = EffectsObservation {
        identity: key,
        history_revision: 0,
        frame: 1,
        token: 1,
        pipelines_ready: true,
        inputs_ready: true,
        encoded_token: 1,
        submitted_token: 1,
        committed_token: 1,
        validation,
    };
    let mut schedule = ProbeSchedule::default();
    for frame in 1..=8 {
        build(&mut required, [true]);
        run.stream.select_required(required.as_slice());
        assert_eq!(run.stream.required.structures, ids);
        assert_eq!(run.stream.required.revision, revision);
        assert_eq!(required.as_slice().as_ptr(), storage);
        let content = ContentKey {
            session: key.session,
            selection_revision: revision,
        };
        policy.content_checked(if run.stream.required_ready() {
            ContentPreparation::Prepared(content)
        } else {
            ContentPreparation::Pending(content)
        });
        policy.snapshot_loading(request, frame, false);
        let job = schedule.job();
        if job == ProbeJob::Face(0) {
            policy.capture_started(request, frame, 0);
        }
        schedule.complete(job).unwrap();
        if job == ProbeJob::RemainingMipsAndPublish {
            policy.publication_validated(request, 0);
        }
        graph.frame = frame;
        graph.token = frame;
        graph.submitted_token = frame;
        graph.probes[0].published = schedule.published();
        effects.frame = frame;
        effects.token = frame;
        effects.encoded_token = frame;
        effects.submitted_token = frame;
        effects.committed_token = frame;
        policy.observe_graph(graph);
        policy.observe_effects(effects);
        assert_eq!(policy.graph_ready(request), frame == 8);
    }
    assert_eq!(policy.check(request, 8, 8), RendererStatus::Loading);
    assert_eq!(policy.check(request, 9, 9), RendererStatus::Loading);
    assert_eq!(policy.check(request, 10, 10), RendererStatus::Ready);
    required.reset(seed);
    required
        .append_probe_faces(&[true], |_, _| Ok(first))
        .unwrap();
    run.stream.select_required(required.as_slice());
    assert_ne!(run.stream.required.revision, revision);
    request.selection_revision = run.stream.required.revision;
    policy.content_checked(ContentPreparation::Prepared(ContentKey {
        session: key.session,
        selection_revision: request.selection_revision,
    }));
    policy.snapshot_loading(request, 11, false);
    assert!(!policy.graph_ready(request));
    build(&mut required, [false]);
    assert_eq!(required.as_slice().len(), 1);
    run.stream.select_required(required.as_slice());
    assert_ne!(run.stream.required.revision, revision);
    request.selection_revision = run.stream.required.revision;
    request.probe_mask = 0;
    policy.content_checked(ContentPreparation::Prepared(ContentKey {
        session: key.session,
        selection_revision: request.selection_revision,
    }));
    policy.snapshot_loading(request, 12, false);
    assert!(policy.graph_ready(request));
    while required.as_slice().len() < REQUIRED_VIEW_CAPACITY {
        required.push(seed).unwrap();
    }
    assert!(required.push(seed).is_err());
}

fn replay_placement(source: &SourceId, ordinal: u32, position: [f64; 3]) -> Placement {
    Placement {
        id: super::super::GeneratedId {
            layer: WorldLayer::Nested,
            generator: source.clone(),
            unit: [0, 0],
            ordinal,
            children: vec![(source.clone(), ordinal)],
        },
        prototype: source.clone(),
        pose: AbsolutePose {
            position,
            rotation: [0., 0., 0., 1.],
        },
        scale: [1.; 3],
        bounds: WorldBounds {
            min: position,
            max: position,
        },
        object_seed: 42,
    }
}
fn replay_snapshot(position: [f64; 3]) -> super::super::ObjectSnapshot {
    super::super::ObjectSnapshot {
        conditions: Vec::new(),
        pose: AbsolutePose {
            position,
            rotation: [0., 0., 0., 1.],
        },
        food: None,
        temperature: None,
        fluids: None,
        valve: None,
        lamp: None,
        lamp_broken: None,
        dial: None,
        ammo: None,
        glow: None,
        radio: None,
        detector: None,
        tool: None,
        gun: None,
        linear_velocity: [0.; 3],
        angular_velocity: [0.; 3],
        sleeping: false,
        held: false,
        buried: false,
        attachment: None,
        mechanism: None,
    }
}
fn old_replay_keys(stream: &WorldStream) -> Vec<super::super::ObjectKey> {
    let radius = f64::from(stream.distances.item_spawn_dist);
    let mut result = Vec::new();
    let mut query = Vec::new();
    for focus in [stream.request.player, stream.request.vehicle] {
        stream.changes.old_keys_in(
            WorldBounds {
                min: [focus[0] - radius, f64::NEG_INFINITY, focus[2] - radius],
                max: [focus[0] + radius, f64::INFINITY, focus[2] + radius],
            },
            &mut query,
        );
        for key in query.drain(..) {
            if !result.contains(&key) {
                result.push(key);
            }
        }
    }
    result
}
fn assert_replay_selection(stream: &WorldStream) {
    let expected: Vec<_> = old_replay_keys(stream)
        .into_iter()
        .map(|key| {
            let change = stream.changes.get(&key).unwrap().clone();
            (key, change)
        })
        .collect();
    let actual: Vec<_> = stream
        .replay_indices
        .iter()
        .map(|&index| {
            let (key, change) = stream.changes.get_indexed(index);
            (key.clone(), change.clone())
        })
        .collect();
    assert_eq!(actual, expected);
}

#[test]
fn replay_index_query_matches_old_order_and_exact_bounds() {
    use super::super::{ChangeRecords, ObjectKey};
    let source = SourceId {
        file: "replay-test".into(),
        path_id: 1,
    };
    let mut changes = ChangeRecords::default();
    let baseline = replay_snapshot([9000.; 3]);
    let positions = [
        [256., 0., 256.],
        [-1., 4., 0.],
        [0., 0., -1.],
        [0., 9000., 0.],
        [0., 0., 255.],
        [0., 0., 256.],
        [300., 0., 0.],
        [0., 0., 9000.],
    ];
    for (i, position) in positions.into_iter().enumerate() {
        let key = if i == 3 {
            ObjectKey::Imported(crate::reference::ItemId(3))
        } else {
            ObjectKey::Generated(replay_placement(&source, i as u32, position).id)
        };
        changes.record(key, replay_snapshot(position), &baseline);
    }
    let removed = ObjectKey::Generated(replay_placement(&source, 9, [0.; 3]).id);
    changes.record(removed.clone(), replay_snapshot([0.; 3]), &baseline);
    changes.remove(removed.clone());
    let bounds = [
        WorldBounds {
            min: [-1., f64::NEG_INFINITY, -1.],
            max: [256., f64::INFINITY, 256.],
        },
        WorldBounds {
            min: [0., -1., 0.],
            max: [300., 1., 256.],
        },
        WorldBounds {
            min: [0.; 3],
            max: [0.; 3],
        },
    ];
    let mut indices = Vec::new();
    let mut keys = Vec::new();
    for bounds in bounds {
        changes.indices_in(bounds, &mut indices);
        changes.old_keys_in(bounds, &mut keys);
        let actual: Vec<_> = indices
            .iter()
            .map(|&i| changes.get_indexed(i).0.clone())
            .collect();
        assert_eq!(actual, keys);
        assert!(!actual.contains(&removed));
    }
    assert!(changes.get(&removed).unwrap().snapshot.is_none());
    // Serialization must contain stable records and object states only.
    let json = serde_json::to_value(&changes).unwrap();
    let mut fields: Vec<_> = json.as_object().unwrap().keys().collect();
    fields.sort();
    assert_eq!(
        fields,
        [
            "bus_doors",
            "engines",
            "gauges",
            "radiators",
            "records",
            "vehicles",
            "wheel_slots"
        ]
    );
}

#[test]
#[ignore = "requires the installed source export"]
fn replay_selector_refreshes_set_replay_forget_and_removal() {
    use super::super::ObjectKey;
    let reference = reference();
    let mut stream = WorldStream::new_default(&reference, 42).unwrap();
    let source = &reference.world.road.prototypes[0];
    let radius = f64::from(stream.distances.item_spawn_dist);
    stream.request.vehicle = [radius, 0., 0.];
    let baseline = replay_snapshot([radius * 20.; 3]);
    let deleted = ObjectKey::Generated(replay_placement(source, 1, baseline.pose.position).id);
    stream.changes.record(
        deleted.clone(),
        replay_snapshot([radius * 10.; 3]),
        &baseline,
    );
    let imported = ObjectKey::Imported(crate::reference::ItemId(999999));
    stream.changes.record(
        imported.clone(),
        replay_snapshot([-radius, 9000., -radius]),
        &baseline,
    );
    let player = replay_placement(source, 2, [radius * 0.5, 0., 0.]);
    let vehicle = replay_placement(source, 3, [radius * 1.5, 0., 0.]);
    let last = replay_placement(source, 4, [0.; 3]);
    let player_key = ObjectKey::Generated(player.id.clone());
    let vehicle_key = ObjectKey::Generated(vehicle.id.clone());
    let last_key = ObjectKey::Generated(last.id.clone());
    stream.changes.record(
        player_key.clone(),
        replay_snapshot(player.pose.position),
        &baseline,
    );
    stream.changes.record(
        vehicle_key.clone(),
        replay_snapshot(vehicle.pose.position),
        &baseline,
    );
    let mut attached = replay_snapshot(last.pose.position);
    attached.attachment = Some(super::super::AttachmentSnapshot {
        owner: Some(ObjectKey::Generated(vehicle.id.clone())),
        slot: None,
        volume: None,
        local_pose: last.pose,
    });
    stream.changes.record(last_key.clone(), attached, &baseline);
    stream.select_changed_destinations();
    assert_replay_selection(&stream);
    assert_eq!(old_replay_keys(&stream).last(), Some(&vehicle_key));
    assert_eq!(
        old_replay_keys(&stream)
            .iter()
            .filter(|k| **k == player_key)
            .count(),
        1
    );
    assert!(old_replay_keys(&stream).contains(&imported));
    // The last selected record moves into an unselected deleted slot.
    // Removing the selector revision check makes this selection assertion fail.
    stream
        .changes
        .record(deleted.clone(), baseline.clone(), &baseline);
    assert!(stream.changes.get(&deleted).is_none());
    let _ = stream.metrics(); // Metrics must not dereference the stale selection.
    stream.select_changed_destinations();
    assert_replay_selection(&stream);
    let revision = stream.change_revision;
    stream.changes.set_replay(&player_key, &player, &player.id);
    stream.changes.set_replay(&last_key, &last, &last.id);
    stream.initial_generation_ready = true;
    stream.select_changed_destinations();
    assert_ne!(stream.change_revision, revision);
    assert_replay_selection(&stream);
    let first = old_replay_keys(&stream)
        .iter()
        .find_map(|key| stream.changes.get(key).unwrap().replay.clone())
        .unwrap();
    assert_eq!(stream.replay_roots, vec![first]);
    let capacities = (
        stream.replay_indices.capacity(),
        stream.replay_query.capacity(),
    );
    stream.select_changed_destinations();
    assert_eq!(
        capacities,
        (
            stream.replay_indices.capacity(),
            stream.replay_query.capacity()
        )
    );
    assert_eq!(stream.replay_roots.len(), 1);
    let (_, selected) = stream
        .replay_indices
        .iter()
        .map(|&i| stream.changes.get_indexed(i))
        .find(|(key, _)| **key == last_key)
        .unwrap();
    assert_eq!(selected.instance.as_ref(), Some(&last.id));
    assert_eq!(
        selected
            .snapshot
            .as_ref()
            .unwrap()
            .attachment
            .as_ref()
            .unwrap()
            .owner,
        Some(ObjectKey::Generated(vehicle.id.clone()))
    );
    stream.changes.remove(player_key.clone());
    stream.select_changed_destinations();
    assert_replay_selection(&stream);
    assert!(stream.changes.get(&player_key).unwrap().snapshot.is_none());
    assert_eq!(stream.replay_roots, vec![last]);
    assert!(stream.failure.is_none(), "{:?}", stream.failure);
}

#[test]
#[ignore = "requires the installed source export"]
fn replay_predicates_match_old_keys_for_instances_owners_and_removals() {
    use super::super::ObjectKey;
    let reference = reference();
    let mut stream = WorldStream::new_default(&reference, 42).unwrap();
    let source = &reference.world.road.prototypes[0];
    let far = f64::from(stream.distances.item_spawn_dist) * 10.;
    let placements: Vec<_> = (1..=6)
        .map(|i| replay_placement(source, i, [far, 0., 0.]))
        .collect();
    let baseline = replay_snapshot([far * 2.; 3]);
    let leaf = ObjectKey::Generated(replay_placement(source, 10, [0.; 3]).id);
    stream
        .changes
        .record(leaf.clone(), replay_snapshot([0.; 3]), &baseline);
    stream
        .changes
        .set_replay(&leaf, &placements[0], &placements[0].id);
    let mut attached = replay_snapshot([0.; 3]);
    attached.attachment = Some(super::super::AttachmentSnapshot {
        owner: Some(ObjectKey::Generated(placements[1].id.clone())),
        slot: None,
        volume: None,
        local_pose: placements[1].pose,
    });
    stream.changes.record(
        ObjectKey::Imported(crate::reference::ItemId(999999)),
        attached,
        &baseline,
    );
    let removed = ObjectKey::Generated(placements[2].id.clone());
    stream
        .changes
        .record(removed.clone(), replay_snapshot([0.; 3]), &baseline);
    stream
        .changes
        .set_replay(&removed, &placements[2], &placements[2].id);
    stream.changes.remove(removed);
    // A changed destination outside both windows must not retain its source instance.
    let outside = ObjectKey::Generated(placements[4].id.clone());
    stream
        .changes
        .record(outside.clone(), replay_snapshot([far; 3]), &baseline);
    stream
        .changes
        .set_replay(&outside, &placements[4], &placements[4].id);
    let direct = ObjectKey::Generated(placements[5].id.clone());
    stream
        .changes
        .record(direct.clone(), replay_snapshot([0.; 3]), &baseline);
    stream
        .changes
        .set_replay(&direct, &placements[5], &placements[5].id);
    for placement in &placements {
        stream
            .contents
            .active
            .push(super::super::contents::ContentInstance::for_structure(
                placement.clone(),
            ));
    }
    let keys = old_replay_keys(&stream);
    let expected: Vec<_> = placements
        .iter()
        .filter(|p| {
            let selected = keys.iter().any(|key| {
                stream.changes.get(key).is_some_and(|c| {
                    c.instance.as_ref() == Some(&p.id)
                        || c.snapshot
                            .as_ref()
                            .and_then(|s| s.attachment.as_ref())
                            .and_then(|a| a.owner.as_ref())
                            == Some(&ObjectKey::Generated(p.id.clone()))
                })
            });
            stream
                .changes
                .get(ObjectKey::Generated(p.id.clone()))
                .is_none_or(|c| c.snapshot.is_some() && selected)
                && selected
        })
        .cloned()
        .collect();
    assert_eq!(
        expected,
        vec![
            placements[0].clone(),
            placements[1].clone(),
            placements[5].clone()
        ]
    );
    let history = stream.contents.state.capture_history();
    stream.prepare_contents();
    assert_eq!(*stream.wanted_structures, expected);
    // Existing unbound distant placements exercise the earlier retention predicate too.
    stream.prepare_contents();
    assert_eq!(*stream.wanted_structures, expected);
    assert_eq!(stream.contents.state.capture_history(), history);
    assert!(stream.failure.is_none(), "{:?}", stream.failure);
}

#[test]
#[ignore = "requires the installed source export"]
fn replay_restore_replaces_selection_and_saved_readiness_checks_revision() {
    use super::super::{ChangeRecords, ObjectKey};
    let reference = reference();
    let baseline = replay_snapshot([9000.; 3]);
    let old = ObjectKey::Imported(crate::reference::ItemId(999998));
    let replacement = ObjectKey::Imported(crate::reference::ItemId(999999));
    let mut candidate = WorldStream::new_default(&reference, 42).unwrap();
    candidate
        .changes
        .record(old.clone(), replay_snapshot([0.; 3]), &baseline);
    candidate.select_changed_destinations();
    let mut saved = crate::save::WorldSnapshot {
        generation: super::super::GenerationSnapshot {
            generator_contract: super::super::GENERATOR_CONTRACT,
            package_id: candidate.package_id.clone(),
            roads: candidate.roads.capture_index(),
            placements: candidate.placements.capture_index(),
        },
        seed: 42,
        date: reference.world.spawn_context.date,
        start_car: candidate.start_car(),
        changes: ChangeRecords::default(),
        spawn: candidate.contents.state.capture_history(),
        boxes: Vec::new(),
    };
    saved
        .changes
        .record(replacement.clone(), replay_snapshot([0.; 3]), &baseline);
    assert_eq!(candidate.changes.revision(), saved.changes.revision());
    candidate.restore_save(&saved).unwrap();
    assert!(candidate.change_bounds.is_none());
    assert_eq!(candidate.change_revision, u64::MAX);
    let _ = candidate.metrics();
    candidate.select_changed_destinations();
    assert_replay_selection(&candidate);
    assert_eq!(old_replay_keys(&candidate), vec![replacement.clone()]);
    // The active stream owns its selection while the candidate is replaced and dropped.
    let mut run = GarageRuntime::new_default(&reference).unwrap();
    prepare_ready(&mut run, &reference);
    mechanism_binding_gate(&mut run, &reference);
    let owner = run
        .stream
        .static_controls
        .iter()
        .find(|c| c.restored && c.item.is_some())
        .unwrap();
    let item = owner.item.unwrap();
    let origin = owner.origin.clone();
    let instance = owner.content.placement.id.clone();
    let generated = run.area.items.object_key(item);
    assert!(matches!(&generated, ObjectKey::Generated(_)));
    let original = super::super::changes::capture(
        item,
        &run.area.items,
        &run.area.interaction,
        &run.physics,
        &reference,
    )
    .unwrap();
    let mut changed = original.clone();
    changed.temperature = Some(original.temperature.unwrap_or(0.) + 1.);
    run.stream
        .changes
        .record(generated.clone(), changed, &original);
    run.stream
        .changes
        .set_replay(&generated, &origin, &instance);
    run.stream
        .static_controls
        .iter_mut()
        .find(|c| c.item == Some(item))
        .unwrap()
        .restored = false;
    run.stream.select_changed_destinations();
    assert!(old_replay_keys(&run.stream).contains(&generated));
    assert!(!run.stream.saved_bindings_ready(&run.area.items));
    run.stream
        .sync_supplies(
            &reference,
            &mut run.area.items,
            &mut run.area.interaction,
            &mut run.physics,
            &mut run.wheels,
            &mut run.vehicles,
            &mut run.powertrain,
        )
        .unwrap();
    assert_eq!(run.area.items.resolve_key(&generated), Some(item));
    assert!(
        run.stream
            .static_controls
            .iter()
            .any(|c| c.item == Some(item) && c.restored)
    );
    assert!(run.stream.saved_bindings_ready(&run.area.items));
    run.stream
        .changes
        .record(generated, original.clone(), &original);
    run.stream.select_changed_destinations();
    run.stream.changes.record(
        old.clone(),
        replay_snapshot(run.stream.request.player),
        &baseline,
    );
    assert!(run.stream.required_ready());
    assert!(!run.stream.saved_bindings_ready(&run.area.items));
    run.stream.select_changed_destinations();
    assert!(run.stream.saved_bindings_ready(&run.area.items));
    drop(candidate);
    saved.changes.remove(replacement);
    assert_eq!(old_replay_keys(&run.stream), vec![old]);
    assert!(run.stream.saved_bindings_ready(&run.area.items));
    let missing = ObjectKey::Generated(
        replay_placement(
            &reference.world.road.prototypes[0],
            900000,
            run.stream.request.player,
        )
        .id,
    );
    run.stream.changes.record(
        missing.clone(),
        replay_snapshot(run.stream.request.player),
        &baseline,
    );
    assert!(!run.stream.saved_bindings_ready(&run.area.items));
    run.stream.select_changed_destinations();
    assert!(!run.stream.saved_bindings_ready(&run.area.items));
    run.stream.changes.remove(missing);
    assert!(!run.stream.saved_bindings_ready(&run.area.items));
    run.stream.select_changed_destinations();
    assert!(run.stream.saved_bindings_ready(&run.area.items));
}

#[test]
#[ignore = "requires the installed source export"]
fn content_index_matches_linear_lookups() {
    use super::super::contents::ContentInstance;
    let reference = reference();
    let mut stream = WorldStream::new_default(&reference, 42).unwrap();
    let source = &reference.world.road.prototypes[0];
    let placements: Vec<_> = (1..=5)
        .map(|i| replay_placement(source, i, [i as f64, 0., 0.]))
        .collect();
    for placement in &placements[..3] {
        let mut content = ContentInstance::for_structure(placement.clone());
        content.poses.push(crate::vehicle::VehiclePose {
            position: [placement.pose.position[0] as f32, 1., 2.],
            rotation: [0., 0., 0., 1.],
        });
        stream.contents.active.push(content);
    }
    stream
        .static_controls
        .push(ContentInstance::for_structure(placements[3].clone()));
    let mut index = super::super::ContentIndex::default();
    stream.index_contents(&mut index);
    for placement in &placements {
        let id = &placement.id;
        let content = stream.indexed_content(&index, id);
        assert_eq!(content.bindings_ready(), stream.bindings_ready(id));
        assert_eq!(content.node_pose(0), stream.generated_node_pose(id, 0));
        assert_eq!(content.caps().len(), stream.generated_cap_visuals(id).len());
        assert_eq!(
            stream.indexed_appearance(&content, id).len(),
            stream.appearance(id).len()
        );
    }
}

#[test]
#[ignore = "requires the installed source export"]
fn road_pole_cable_reaches_the_next_pole() {
    use glam::{Mat4, Quat, Vec3};
    let reference = reference();
    let mut run = GarageRuntime::new_default(&reference).unwrap();
    prepare_ready(&mut run, &reference);
    let stream = &mut run.stream;
    let road = stream
        .selected_roads
        .iter()
        .find(|r| r.light.is_some() && r.segment.index + 1 < super::super::road::ROAD_SEGMENTS)
        .unwrap();
    let (segment, light) = (road.segment, road.light.clone().unwrap());
    let next = stream.roads.segment(segment.index + 1).unwrap();
    let next_light = stream.road_light.place(&stream.reference, &next).unwrap();
    let prototype = |light: &super::super::WorldLight| {
        stream
            .reference
            .prototypes
            .iter()
            .find(|p| p.source == light.prototype)
            .unwrap()
    };
    // Unity TRS chain relative to the first pole, with appearance overrides.
    let origin = light.pole.position;
    let worlds = |light: &super::super::WorldLight, appearance: &[super::super::NodeAppearance]| {
        let prototype = prototype(light);
        let root = Mat4::from_scale_rotation_translation(
            Vec3::from_array(light.scale),
            Quat::from_array(light.pole.rotation),
            Vec3::from_array([0, 1, 2].map(|a| (light.pole.position[a] - origin[a]) as f32)),
        );
        let local = |i: usize| {
            let node = &prototype.nodes[i];
            let last = |f: fn(&super::super::NodeAppearance) -> bool| {
                appearance
                    .iter()
                    .rev()
                    .find(|a| a.node == node.source && f(a))
            };
            Mat4::from_scale_rotation_translation(
                Vec3::from_array(
                    last(|a| a.scale.is_some()).map_or(node.transform.scale, |a| a.scale.unwrap()),
                ),
                Quat::from_array(
                    last(|a| a.rotation.is_some())
                        .map_or(node.transform.rotation, |a| a.rotation.unwrap()),
                ),
                Vec3::from_array(node.transform.translation),
            )
        };
        (0..prototype.nodes.len())
            .map(|i| {
                let mut matrix = local(i);
                let mut parent = prototype.nodes[i].parent;
                while let Some(p) = parent {
                    matrix = local(p as usize) * matrix;
                    parent = prototype.nodes[p as usize].parent;
                }
                root * matrix
            })
            .collect::<Vec<_>>()
    };
    let cable = stream.appearance(&light.id);
    assert!(!cable.is_empty());
    let from = worlds(&light, cable);
    let to = worlds(&next_light, &[]);
    for rope in cable {
        let i = prototype(&light)
            .nodes
            .iter()
            .position(|n| n.source == rope.node)
            .unwrap();
        let start = from[i].transform_point3(Vec3::ZERO);
        let forward = from[i].transform_vector3(Vec3::NEG_Z).normalize();
        // Source ropeScript.Connect: localScale z = distance / 100.
        let end = start + forward * rope.scale.unwrap()[2] * 100.;
        assert!(
            to.iter()
                .any(|m| m.transform_point3(Vec3::ZERO).distance(end) < 0.01),
            "rope {:?} does not reach a next pole node",
            rope.node
        );
    }
}

#[test]
#[ignore = "requires the installed source export"]
fn road_building_cable_is_in_building_appearance() {
    let reference = reference();
    let mut run = GarageRuntime::new_default(&reference).unwrap();
    prepare_ready(&mut run, &reference);
    let stream = &run.stream;
    assert!(!stream.building_cables.is_empty());
    for (id, cable) in stream.building_cables.iter().filter(|(_, c)| !c.is_empty()) {
        assert_eq!(cable[0].active, Some(true));
        let appearance = stream.appearance(id);
        for rope in &cable[1..] {
            // Source CheckCableConnections: target 0 under 200 m, plus the pole width.
            assert!(rope.scale.unwrap()[2] < 2.1);
            assert!(appearance.iter().any(|a| a.node == rope.node
                && a.rotation == rope.rotation
                && a.scale == rope.scale));
        }
    }
}

fn decoration_source(path_id: i64) -> SourceId {
    SourceId {
        file: "sharedassets1.assets".into(),
        path_id,
    }
}

fn publish_decoration_fixture(run: &mut GarageRuntime, placement: Placement) {
    let stream = &mut run.stream;
    stream
        .contents
        .active
        .push(super::super::contents::ContentInstance::for_structure(
            placement.clone(),
        ));
    stream.wanted_structures.push(placement.clone());
    stream.hold_prototype(placement.prototype.clone()).unwrap();
    stream.bind_prototype(&placement.prototype).unwrap();
    stream.bind_structure_recipe(&placement.prototype).unwrap();
    accept_structure(stream, &placement.id);
    stream.plan_revision = stream.request.revision;
    stream.publish_structure(&mut run.physics, &[]).unwrap();
}

fn sync_decoration_fixture(run: &mut GarageRuntime, reference: &ReferenceManifest) {
    run.stream
        .sync_supplies(
            reference,
            &mut run.area.items,
            &mut run.area.interaction,
            &mut run.physics,
            &mut run.wheels,
            &mut run.vehicles,
            &mut run.powertrain,
        )
        .unwrap();
}

#[test]
#[ignore = "requires the installed source export"]
fn fixed_decorations_preserve_collision_without_item_bindings() {
    use rapier3d::prelude::{Group, InteractionGroups, InteractionTestMode, RigidBodyType};
    let mut reference = reference();
    let mut wrong_file = reference
        .world
        .prototypes
        .iter()
        .find(|p| p.source == decoration_source(84448))
        .unwrap()
        .clone();
    wrong_file.source.file = "wrong.assets".into();
    let wrong_source = wrong_file.source.clone();
    std::sync::Arc::get_mut(&mut reference.world)
        .unwrap()
        .prototypes
        .edit(|prototypes| prototypes.push(wrong_file));
    let mut run = GarageRuntime::new_default(&reference).unwrap();
    let sources = [84448, 69558, 66737, 79572].map(decoration_source);
    for (ordinal, source) in sources.iter().enumerate() {
        let prototype = reference
            .world
            .prototypes
            .iter()
            .find(|p| &p.source == source)
            .unwrap();
        assert!(!prototype.bodies.is_empty());
        for scale in [1., 1.5] {
            let mut placement = replay_placement(
                source,
                (ordinal * 2 + usize::from(scale > 1.)) as u32,
                [ordinal as f64 * 20., 10., 0.],
            );
            placement.scale = prototype.prefab_pose.scale.map(|v| v * scale);
            let before = (
                run.area.items.len(),
                run.area.interaction.retained_binding_bytes(),
                run.wheels.retained_binding_bytes(),
            );
            let dynamic = run
                .physics
                .rapier()
                .bodies
                .iter()
                .filter(|(_, b)| b.is_dynamic())
                .count();
            publish_decoration_fixture(&mut run, placement.clone());
            assert_eq!(
                run.physics
                    .rapier()
                    .bodies
                    .iter()
                    .filter(|(_, b)| b.is_dynamic())
                    .count(),
                dynamic
            );
            let instance = run.stream.structures.last().unwrap();
            assert_eq!(instance.bodies.len(), prototype.bodies.len());
            for (index, handle) in instance.bodies.iter().enumerate() {
                let source_body = &prototype.bodies[index];
                let expected = crate::vehicle::to_rapier(crate::vehicle::VehiclePose {
                    position: placement.pose.position.map(|v| v as f32),
                    rotation: placement.pose.rotation,
                }) * crate::vehicle::to_rapier(crate::vehicle::VehiclePose {
                    position: std::array::from_fn(|i| {
                        source_body.pose.translation[i] * placement.scale[i]
                    }),
                    rotation: source_body.pose.rotation,
                });
                assert_eq!(run.physics.rapier().bodies[*handle].position(), &expected);
                assert_eq!(
                    run.physics.rapier().bodies[*handle].body_type(),
                    RigidBodyType::Fixed,
                    "{} scale={scale}",
                    source.path_id
                );
            }
            for (source, handle) in &instance.colliders {
                let record = prototype
                    .colliders
                    .iter()
                    .find(|c| &c.source == source)
                    .unwrap();
                let collider = &run.physics.rapier().colliders[*handle];
                assert_eq!(
                    collider.parent(),
                    record.body.map(|i| instance.bodies[i as usize])
                );
                let layer = prototype.nodes[record.node as usize].layer as usize;
                assert_eq!(
                    collider.collision_groups(),
                    InteractionGroups::new(
                        Group::from_bits_truncate(1 << layer),
                        Group::from_bits_truncate(reference.world.collision_rules.layers[layer]),
                        InteractionTestMode::And
                    )
                );
                assert_eq!(collider.is_sensor(), record.trigger);
                if prototype.source.path_id == 66737 {
                    assert_eq!(prototype.prefab_pose.scale, [0.94613105, 1., 1.2322059]);
                    assert!(
                        prototype
                            .bodies
                            .iter()
                            .all(|b| b.pose.rotation == [0., 0., 0., 1.])
                    );
                    let bounds = collider.shape().compute_aabb(collider.position());
                    assert!(
                        (bounds.maxs.x
                            - placement.pose.position[0] as f32
                            - 0.84375 * placement.scale[0])
                            .abs()
                            < 0.001
                    );
                    assert!(
                        (bounds.maxs.z
                            - placement.pose.position[2] as f32
                            - 1.0625 * placement.scale[2])
                            .abs()
                            < 0.001
                    );
                    let sphere = rapier3d::parry::shape::Ball::new(0.1);
                    let contact = rapier3d::parry::query::contact(
                        collider.position(),
                        collider.shape(),
                        &rapier3d::math::Pose::translation(
                            placement.pose.position[0] as f32,
                            10.05,
                            0.,
                        ),
                        &sphere,
                        0.,
                    )
                    .unwrap();
                    assert!(
                        contact.is_some(),
                        "actual nonuniform plank must retain contact"
                    );
                }
            }
            assert_eq!(
                instance.colliders.len(),
                prototype
                    .colliders
                    .iter()
                    .filter(|c| c.source_empty_mesh.is_none())
                    .count()
            );
            sync_decoration_fixture(&mut run, &reference);
            assert_eq!(
                before,
                (
                    run.area.items.len(),
                    run.area.interaction.retained_binding_bytes(),
                    run.wheels.retained_binding_bytes()
                ),
                "fixed decoration must not register item, pickup, attachment, part, or mechanism bindings"
            );
            let content = run
                .stream
                .contents
                .active
                .iter()
                .find(|c| c.content.placement.id == placement.id)
                .unwrap();
            assert!(
                content.item.is_none()
                    && content.bindings.is_empty()
                    && content.slots.is_empty()
                    && content.nodes.is_empty()
            );
        }
    }
    let ordinary = reference
        .world
        .prototypes
        .iter()
        .find(|p| {
            !sources.contains(&p.source)
                && !p.bodies.is_empty()
                && p.bodies.iter().any(|b| !b.kinematic)
                && p.role == crate::reference::WorldPrototypeRole::Nested
        })
        .unwrap();
    for prototype in [
        ordinary,
        reference
            .world
            .prototypes
            .iter()
            .find(|p| p.source == wrong_source)
            .unwrap(),
    ] {
        let mut placement = replay_placement(
            &prototype.source,
            100 + run.stream.structures.len() as u32,
            [0., 20., 0.],
        );
        placement.scale = prototype.prefab_pose.scale;
        publish_decoration_fixture(&mut run, placement);
        assert!(
            run.stream
                .structures
                .last()
                .unwrap()
                .bodies
                .iter()
                .any(|h| run.physics.rapier().bodies[*h].body_type() == RigidBodyType::Dynamic)
        );
    }
}

#[test]
#[ignore = "requires the installed source export"]
fn fixed_decoration_replay_rebases_retires_and_republishes() {
    use rapier3d::prelude::RigidBodyType;
    let reference = reference();
    let mut run = GarageRuntime::new_default(&reference).unwrap();
    let source = decoration_source(66737);
    let prototype = reference
        .world
        .prototypes
        .iter()
        .find(|p| p.source == source)
        .unwrap();
    let mut placement = replay_placement(&source, 400, [2., 20., 3.]);
    placement.scale = prototype.prefab_pose.scale;
    let key = super::super::ObjectKey::Generated(placement.id.clone());
    run.stream.changes.record(
        key.clone(),
        replay_snapshot(placement.pose.position),
        &replay_snapshot([9000.; 3]),
    );
    run.stream
        .changes
        .set_replay(&key, &placement, &placement.id);
    run.stream.changes.remove(key.clone());
    let mut replacement = placement.clone();
    replacement.id.ordinal = 401;
    replacement.id.children.last_mut().unwrap().1 = 401;
    replacement.pose.position = [10000., 20., 3.];
    let replacement_key = super::super::ObjectKey::Generated(replacement.id.clone());
    run.stream.changes.record(
        replacement_key.clone(),
        replay_snapshot(replacement.pose.position),
        &replay_snapshot([9000.; 3]),
    );
    run.stream
        .changes
        .set_replay(&replacement_key, &replacement, &replacement.id);
    let saved = run.stream.capture_save(
        &reference,
        &run.area.items,
        &run.area.interaction,
        &run.physics,
        &[],
    );
    let saved: crate::save::WorldSnapshot =
        serde_json::from_slice(&serde_json::to_vec(&saved).unwrap()).unwrap();
    run.stream = WorldStream::new(
        &reference,
        saved.seed,
        saved.date,
        crate::world::GameMode::default(),
        run.distances,
    )
    .unwrap();
    run.stream.restore_save(&saved).unwrap();
    publish_decoration_fixture(&mut run, placement.clone());
    assert!(
        run.stream.changes.get(&key).is_none(),
        "old removal records cannot suppress fixed decorations"
    );
    assert!(run.stream.replay_roots.is_empty());
    let original = run.stream.contents.active.pop().unwrap();
    run.stream
        .contents
        .active
        .push(super::super::contents::ContentInstance::for_structure(
            replacement,
        ));
    run.stream.discard_fixed_decoration_changes();
    assert!(
        run.stream.changes.get(&replacement_key).is_none(),
        "equal-count replacement must classify the new source instance"
    );
    assert_replay_selection(&run.stream);
    assert!(run.stream.replay_roots.is_empty());
    assert!(run.stream.replay_query.is_empty());
    assert_eq!(run.stream.change_revision, run.stream.changes.revision());
    run.stream.contents.active.pop();
    run.stream.contents.active.push(original);
    run.stream.contents.active.last_mut().unwrap().replay_only = true;
    sync_decoration_fixture(&mut run, &reference);
    let content = run.stream.contents.active.last().unwrap();
    assert!(
        content.restored,
        "itemless fixed replay must complete restoration"
    );
    assert!(content.item.is_none());
    let handles = run.stream.structures.last().unwrap().bodies.clone();
    let poses: Vec<_> = handles
        .iter()
        .map(|h| *run.physics.rapier().bodies[*h].position())
        .collect();
    for _ in 0..30 {
        run.physics.step_physics();
    }
    for (handle, pose) in handles.iter().zip(&poses) {
        assert_eq!(run.physics.rapier().bodies[*handle].position(), pose);
    }
    let shift = run
        .stream
        .rebase([4096., 0., 0.], &mut run.physics)
        .unwrap();
    for (handle, pose) in handles.iter().zip(&poses) {
        let body = &run.physics.rapier().bodies[*handle];
        assert_eq!(body.body_type(), RigidBodyType::Fixed);
        assert!((body.translation().x - (pose.translation.x - shift[0])).abs() < 0.001);
    }
    run.stream.request.player = [10000., 0., 0.];
    run.stream.request.vehicle = [10000., 0., 0.];
    run.stream
        .retire_contents(
            &mut run.physics,
            &mut run.area.items,
            &mut run.area.interaction,
            &mut run.wheels,
            &mut run.vehicles,
            &mut run.powertrain,
        )
        .unwrap();
    assert!(
        run.stream.contents.active.is_empty(),
        "itemless replay cannot pin retirement"
    );
    assert!(
        !run.stream
            .wanted_structures
            .iter()
            .any(|p| p.id == placement.id)
    );
    run.stream
        .structures
        .pop()
        .unwrap()
        .retire(&mut run.physics, &mut run.stream.trigger_roles);
    assert!(
        handles
            .iter()
            .all(|h| run.physics.rapier().bodies.get(*h).is_none())
    );
    publish_decoration_fixture(&mut run, placement);
    sync_decoration_fixture(&mut run, &reference);
    assert!(
        run.stream
            .structures
            .last()
            .unwrap()
            .bodies
            .iter()
            .all(|h| run.physics.rapier().bodies[*h].body_type() == RigidBodyType::Fixed)
    );
    assert!(run.stream.contents.active.last().unwrap().restored);
}

#[test]
#[ignore = "requires the installed source export"]
fn fixed_decoration_possessions_restore_on_item_path() {
    use super::super::{AttachmentSnapshot, ObjectKey};
    use crate::interaction::items::ItemLocation;
    let reference = reference();
    let mut run = GarageRuntime::new_default(&reference).unwrap();
    let sources = [84448, 69558, 66737, 79572].map(decoration_source);
    let mut placements: Vec<_> = sources
        .iter()
        .enumerate()
        .map(|(i, source)| {
            let mut p = replay_placement(source, 500 + i as u32, [i as f64 * 5., 10., 0.]);
            p.scale = reference
                .world
                .prototypes
                .iter()
                .find(|p| &p.source == source)
                .unwrap()
                .prefab_pose
                .scale;
            p
        })
        .collect();
    let part = reference
        .world
        .prototypes
        .iter()
        .find(|p| {
            !sources.contains(&p.source)
                && !p.bodies.is_empty()
                && p.prefab_pose.scale[0] == p.prefab_pose.scale[1]
                && p.prefab_pose.scale[0] == p.prefab_pose.scale[2]
                && run
                    .stream
                    .contents
                    .rules
                    .component(&reference.world, &p.source, "partscript")
                    .unwrap()
                    .is_some()
        })
        .expect("unrelated generated vehicle part");
    let mut part_placement = replay_placement(&part.source, 600, [25., 10., 0.]);
    part_placement.scale = part.prefab_pose.scale;
    placements.push(part_placement);
    for (ordinal, source) in
        [700, 701, 702]
            .into_iter()
            .zip([&sources[0], &sources[1], &sources[0]])
    {
        let mut p = replay_placement(
            source,
            ordinal,
            [30. + f64::from(ordinal - 700) * 5., 10., 0.],
        );
        p.scale = reference
            .world
            .prototypes
            .iter()
            .find(|p| &p.source == source)
            .unwrap()
            .prefab_pose
            .scale;
        placements.push(p);
    }
    // These keys arrive from saved inventory and an imported child's generated-owner link.
    let required: Vec<_> = placements[5..]
        .iter()
        .map(|p| ObjectKey::Generated(p.id.clone()))
        .collect();
    let baseline = replay_snapshot([9000.; 3]);
    for (i, p) in placements.iter().enumerate() {
        let mut snapshot = replay_snapshot(p.pose.position);
        snapshot.held = i == 0;
        if i == 1 {
            snapshot.attachment = Some(AttachmentSnapshot {
                owner: Some(ObjectKey::Generated(placements[2].id.clone())),
                slot: None,
                volume: None,
                local_pose: p.pose,
            });
        }
        let key = ObjectKey::Generated(p.id.clone());
        run.stream.changes.record(key.clone(), snapshot, &baseline);
        run.stream.changes.set_replay(&key, p, &p.id);
    }
    let saved = run.stream.capture_save(
        &reference,
        &run.area.items,
        &run.area.interaction,
        &run.physics,
        &[],
    );
    let json = serde_json::to_vec(&saved).unwrap();
    let saved: crate::save::WorldSnapshot = serde_json::from_slice(&json).unwrap();
    run.stream = WorldStream::new(
        &reference,
        saved.seed,
        saved.date,
        crate::world::GameMode::default(),
        run.distances,
    )
    .unwrap();
    run.stream.restore_save(&saved).unwrap();
    run.stream.protect_saved_items(&required);
    for p in &placements {
        publish_decoration_fixture(&mut run, p.clone());
    }
    sync_decoration_fixture(&mut run, &reference);
    sync_decoration_fixture(&mut run, &reference);
    assert!(
        run.stream.saved_item_protection.is_empty(),
        "save protection must end after binding"
    );
    for (i, p) in placements.iter().enumerate() {
        let instance = run
            .stream
            .structures
            .iter()
            .find(|s| s.placement.id == p.id)
            .unwrap();
        if i == 3 {
            assert!(
                instance
                    .bodies
                    .iter()
                    .all(|h| run.physics.rapier().bodies[*h].is_fixed())
            );
            assert!(
                run.area
                    .items
                    .resolve_key(&ObjectKey::Generated(p.id.clone()))
                    .is_none()
            );
            assert!(
                run.stream
                    .changes
                    .get(ObjectKey::Generated(p.id.clone()))
                    .is_none(),
                "obsolete mutable decoration must not block saved readiness"
            );
        } else {
            let item = run
                .area
                .items
                .resolve_key(&ObjectKey::Generated(p.id.clone()))
                .expect("retained possession or dependency binding");
            assert!(
                !instance
                    .bodies
                    .iter()
                    .any(|h| run.physics.rapier().bodies[*h].is_fixed())
            );
            if i == 0 {
                assert_eq!(
                    run.area.items.get(item).unwrap().location,
                    ItemLocation::Held
                );
            }
            if i == 1 {
                let owner = run
                    .area
                    .items
                    .resolve_key(&ObjectKey::Generated(placements[2].id.clone()))
                    .unwrap();
                let expected = crate::vehicle::to_rapier(run.area.items.get(owner).unwrap().pose)
                    * crate::vehicle::to_rapier(crate::vehicle::VehiclePose {
                        position: p.pose.position.map(|v| v as f32),
                        rotation: p.pose.rotation,
                    });
                assert_eq!(
                    crate::vehicle::to_rapier(run.area.items.get(item).unwrap().pose),
                    expected
                );
                assert!(
                    run.stream
                        .changes
                        .get(ObjectKey::Generated(p.id.clone()))
                        .unwrap()
                        .snapshot
                        .as_ref()
                        .unwrap()
                        .attachment
                        .is_some()
                );
            }
            if i == 4 {
                assert!(run.area.interaction.parts.contains(item));
            }
            if i == 5 {
                run.area.items.get_mut(item).unwrap().location = ItemLocation::Stored(0);
            }
            if i == 6 {
                run.area.items.get_mut(item).unwrap().location = ItemLocation::Equipped;
            }
            if i == 7 {
                let imported = reference
                    .items
                    .iter()
                    .find(|r| {
                        run.area
                            .items
                            .get(r.id)
                            .is_some_and(|s| s.location == ItemLocation::World && s.body.is_some())
                    })
                    .unwrap()
                    .id;
                run.area.items.get_mut(imported).unwrap().location = ItemLocation::Attached(item);
                assert!(
                    run.stream
                        .contents
                        .active
                        .iter()
                        .find(|c| c.item == Some(item))
                        .unwrap()
                        .has_external_dependents(
                            &run.area.items,
                            &run.area.interaction.parts,
                            &run.wheels
                        )
                );
            }
            assert!(run.stream.bindings_ready(&p.id));
        }
    }
    prepare_ready(&mut run, &reference);
    run.stream.select_changed_destinations();
    assert!(run.stream.saved_bindings_ready(&run.area.items));
    let dropped_key = ObjectKey::Generated(placements[5].id.clone());
    let dropped = run.area.items.resolve_key(&dropped_key).unwrap();
    run.area.items.get_mut(dropped).unwrap().location = ItemLocation::World;
    run.stream.request.player = [10000., 0., 0.];
    run.stream.request.vehicle = [10000., 0., 0.];
    run.stream
        .retire_contents(
            &mut run.physics,
            &mut run.area.items,
            &mut run.area.interaction,
            &mut run.wheels,
            &mut run.vehicles,
            &mut run.powertrain,
        )
        .unwrap();
    assert!(
        run.area.items.resolve_key(&dropped_key).is_none(),
        "dropped loot must not retain save protection"
    );
    let equipped_key = ObjectKey::Generated(placements[6].id.clone());
    assert_eq!(
        run.area
            .items
            .get(run.area.items.resolve_key(&equipped_key).unwrap())
            .unwrap()
            .location,
        ItemLocation::Equipped
    );
    let owner_key = ObjectKey::Generated(placements[7].id.clone());
    assert!(
        run.area.items.resolve_key(&owner_key).is_some(),
        "imported child must protect its generated owner"
    );
    let index = run
        .stream
        .structures
        .iter()
        .position(|p| p.placement.id == placements[5].id)
        .unwrap();
    run.stream
        .structures
        .swap_remove(index)
        .retire(&mut run.physics, &mut run.stream.trigger_roles);
    publish_decoration_fixture(&mut run, placements[5].clone());
    sync_decoration_fixture(&mut run, &reference);
    assert!(
        run.stream.structures.last().unwrap().fixed_decoration,
        "dropped, retired possessions return as fixed decoration"
    );
}

#[test]
#[ignore = "requires the installed source export"]
fn fixed_decoration_imported_inventory_reload_waits_for_real_readiness() {
    use crate::inventory::InventoryCommand;
    use crate::save::{SourceHash, SourceStamp};
    let reference = reference();
    let digest = std::process::Command::new("sha256sum")
        .arg(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../web/assets/reference/manifest.json"
        ))
        .output()
        .unwrap();
    assert!(digest.status.success());
    let stamp = SourceStamp {
        app_id: reference.app_id,
        build_id: reference.build_id,
        manifest_sha256: String::from_utf8(digest.stdout)
            .unwrap()
            .split_whitespace()
            .next()
            .unwrap()
            .into(),
        source_hashes: reference
            .source_hashes
            .iter()
            .map(|h| SourceHash {
                file: h.file.clone(),
                sha256: h.sha256.clone(),
            })
            .collect(),
    };
    let mut run = GarageRuntime::new_default(&reference).unwrap();
    let oil = reference
        .items
        .iter()
        .find(|i| i.name == "OilCan")
        .unwrap()
        .id;
    let light = reference
        .flashlights
        .instances
        .iter()
        .find(|f| f.startup_active)
        .unwrap()
        .item;
    for (item, slot) in [(oil, 1), (light, 0)] {
        let point = run.area.items.get(item).unwrap().pose.position;
        if run.area.interaction.storage.stored(item) {
            run.area
                .interaction
                .pickup_stored_item(
                    item,
                    point,
                    &run.area.player,
                    &mut run.area.items,
                    &mut run.physics,
                )
                .unwrap();
        } else {
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
        }
        run.area.sync_inventory();
        assert!(
            run.area
                .apply_inventory_command(
                    InventoryCommand::Select(slot),
                    &reference,
                    &mut run.physics
                )
                .unwrap()
        );
    }
    assert!(
        run.area
            .apply_inventory_command(InventoryCommand::Select(1), &reference, &mut run.physics)
            .unwrap()
    );
    assert_eq!(run.area.interaction.equipment.item(), Some(light));
    let saved = run
        .capture_save(&reference, &stamp, "2026-10-18T12:00:00Z")
        .unwrap();
    let saved = serde_json::from_slice(&serde_json::to_vec(&saved).unwrap()).unwrap();
    let mut loaded = GarageRuntime::from_save(
        &reference,
        &stamp,
        &saved,
        crate::world::GameMode::default(),
        reference.world.draw_distance.default,
    )
    .unwrap();
    assert!(loaded.restore_pending());
    assert!(
        loaded
            .capture_save(&reference, &stamp, "2026-10-18T12:00:00Z")
            .is_err()
    );
    prepare_ready(&mut loaded, &reference);
    assert!(loaded.advance_saved_restore(&reference).unwrap());
    assert!(!loaded.restore_pending());
    let recaptured = loaded
        .capture_save(&reference, &stamp, "2026-10-18T12:00:00Z")
        .unwrap();
    assert_eq!(loaded.area.inventory.slots, run.area.inventory.slots);
    assert_eq!(loaded.area.interaction.equipment.item(), Some(light));
    for item in [oil, light] {
        let key = crate::save::InstanceId::initial(
            &reference
                .items
                .iter()
                .find(|i| i.id == item)
                .unwrap()
                .source,
        );
        let before = saved.items.iter().find(|i| i.instance == key).unwrap();
        let after = recaptured.items.iter().find(|i| i.instance == key).unwrap();
        assert_eq!(before.location, after.location);
        for axis in 0..3 {
            assert!((before.pose.position[axis] - after.pose.position[axis]).abs() < 0.0001);
        }
    }
}

fn roadside_loot(
    run: &mut GarageRuntime,
    reference: &ReferenceManifest,
    ordinal: u32,
) -> (Placement, crate::reference::ItemId) {
    let source = decoration_source(84448);
    let mut placement = replay_placement(&source, ordinal, [2., 20., 3.]);
    placement.scale = reference
        .world
        .prototypes
        .iter()
        .find(|p| p.source == source)
        .unwrap()
        .prefab_pose
        .scale;
    let key = super::super::ObjectKey::Generated(placement.id.clone());
    run.stream.protect_saved_items(std::slice::from_ref(&key));
    publish_decoration_fixture(run, placement.clone());
    sync_decoration_fixture(run, reference);
    let id = run.area.items.resolve_key(&key).unwrap();
    (placement, id)
}

#[test]
#[ignore = "requires the installed source export"]
fn roadside_edits_survive_retirement_and_save_reload() {
    let reference = reference();
    let mut run = GarageRuntime::new_default(&reference).unwrap();
    let (placement, id) = roadside_loot(&mut run, &reference, 810);
    let key = run.area.items.object_key(id);
    run.area.items.get_mut(id).unwrap().temperature = Some(25.);
    run.area.items.mark_dirty(id);
    run.stream.capture_changes(
        &reference,
        &mut run.area.items,
        &run.area.interaction,
        &run.physics,
    );
    assert!(run.stream.changes.get(&key).is_some());
    let before = (
        run.stream.changes.len(),
        run.stream.changes.retained_bytes(),
    );
    run.stream.request.player = [10000., 0., 0.];
    run.stream.request.vehicle = [10000., 0., 0.];
    run.stream
        .retire_contents(
            &mut run.physics,
            &mut run.area.items,
            &mut run.area.interaction,
            &mut run.wheels,
            &mut run.vehicles,
            &mut run.powertrain,
        )
        .unwrap();
    assert!(run.area.items.resolve_key(&key).is_none());
    assert!(
        run.stream.changes.get(&key).is_some(),
        "distant abandoned edit must stay after retirement"
    );
    run.stream.capture_changes(
        &reference,
        &mut run.area.items,
        &run.area.interaction,
        &run.physics,
    );
    let saved = run.stream.capture_save(
        &reference,
        &run.area.items,
        &run.area.interaction,
        &run.physics,
        &[],
    );
    assert!(saved.changes.get(&key).is_some());
    eprintln!(
        "roadside retirement records/bytes: {:?} -> {:?}",
        before,
        (
            run.stream.changes.len(),
            run.stream.changes.retained_bytes()
        )
    );
    run.stream = WorldStream::new(
        &reference,
        saved.seed,
        saved.date,
        crate::world::GameMode::default(),
        run.distances,
    )
    .unwrap();
    run.stream.restore_save(&saved).unwrap();
    assert_eq!(
        run.stream.changes.get(&key).and_then(|c| c.replay.as_ref()),
        saved.changes.get(&key).and_then(|c| c.replay.as_ref())
    );
    assert!(
        run.stream
            .changes
            .iter()
            .any(|(_, c)| c.instance.as_ref() == Some(&placement.id))
    );
}

#[test]
#[ignore = "requires the installed source export"]
fn roadside_unresolved_attachment_keeps_retired_parent() {
    use super::super::{AttachmentSnapshot, ChangeRecords, ObjectKey};
    let reference = reference();
    let mut run = GarageRuntime::new_default(&reference).unwrap();
    let (parent, parent_id) = roadside_loot(&mut run, &reference, 813);
    let parent_key = run.area.items.object_key(parent_id);
    run.area.items.get_mut(parent_id).unwrap().temperature = Some(25.);
    run.area.items.mark_dirty(parent_id);
    run.stream.capture_changes(
        &reference,
        &mut run.area.items,
        &run.area.interaction,
        &run.physics,
    );
    assert!(
        run.stream
            .changes
            .get(&parent_key)
            .unwrap()
            .replay
            .is_some()
    );

    let baseline = replay_snapshot([0.; 3]);
    let internal = replay_placement(&decoration_source(84448), 815, [9000.; 3]);
    let internal_key = ObjectKey::Generated(internal.id.clone());
    let mut internal_snapshot = replay_snapshot(internal.pose.position);
    internal_snapshot.attachment = Some(AttachmentSnapshot {
        owner: Some(parent_key.clone()),
        slot: None,
        volume: None,
        local_pose: AbsolutePose {
            position: [0.; 3],
            rotation: [0., 0., 0., 1.],
        },
    });
    run.stream
        .changes
        .record(internal_key.clone(), internal_snapshot, &baseline);
    run.stream
        .changes
        .set_replay(&internal_key, &internal, &parent.id);
    let child = replay_placement(&decoration_source(84448), 814, [9000.; 3]);
    let child_key = ObjectKey::Generated(child.id.clone());
    let mut child_snapshot = replay_snapshot(child.pose.position);
    child_snapshot.attachment = Some(AttachmentSnapshot {
        owner: Some(internal_key.clone()),
        slot: None,
        volume: None,
        local_pose: AbsolutePose {
            position: [0.; 3],
            rotation: [0., 0., 0., 1.],
        },
    });
    run.stream
        .changes
        .record(child_key.clone(), child_snapshot.clone(), &baseline);
    run.stream.changes.set_replay(&child_key, &child, &child.id);
    assert!(run.stream.changes.has_attachment_dependents(&parent_key));
    assert!(run.stream.changes.has_attachment_dependents(&internal_key));
    let restored: ChangeRecords =
        serde_json::from_slice(&serde_json::to_vec(&run.stream.changes).unwrap()).unwrap();
    assert!(restored.has_attachment_dependents(&parent_key));
    assert!(restored.has_attachment_dependents(&internal_key));
    child_snapshot.temperature = Some(25.);
    run.stream
        .changes
        .record(child_key.clone(), child_snapshot, &baseline);

    run.stream.request.player = [10000., 0., 0.];
    run.stream.request.vehicle = [10000., 0., 0.];
    run.stream
        .retire_contents(
            &mut run.physics,
            &mut run.area.items,
            &mut run.area.interaction,
            &mut run.wheels,
            &mut run.vehicles,
            &mut run.powertrain,
        )
        .unwrap();
    assert!(run.area.items.resolve_key(&parent_key).is_none());
    assert!(run.stream.changes.get(&parent_key).is_some());
    assert!(run.stream.changes.get(&parent_key).is_some());
    assert!(run.stream.changes.get(&child_key).is_some());
    assert!(run.stream.changes.get(&internal_key).is_some());

    let saved = run.stream.capture_save(
        &reference,
        &run.area.items,
        &run.area.interaction,
        &run.physics,
        &[],
    );
    let readback: crate::save::WorldSnapshot =
        serde_json::from_slice(&serde_json::to_vec(&saved).unwrap()).unwrap();
    let mut reloaded = WorldStream::new(
        &reference,
        readback.seed,
        readback.date,
        crate::world::GameMode::default(),
        run.distances,
    )
    .unwrap();
    reloaded.restore_save(&readback).unwrap();
    for key in [&parent_key, &internal_key, &child_key] {
        assert!(
            reloaded.changes.get(key).is_some(),
            "missing attachment chain key {key:?}"
        );
    }
}

#[test]
#[ignore = "requires the installed source export"]
fn roadside_consumed_tombstone_survives_departure() {
    use super::super::ObjectKey;
    let reference = reference();
    let mut run = GarageRuntime::new_default(&reference).unwrap();
    let (placement, id) = roadside_loot(&mut run, &reference, 811);
    let key = run.area.items.object_key(id);
    let distant = replay_placement(&decoration_source(84448), 812, [9000.; 3]);
    let distant_key = ObjectKey::Generated(distant.id.clone());
    run.stream.changes.record(
        distant_key.clone(),
        replay_snapshot(distant.pose.position),
        &replay_snapshot([0.; 3]),
    );
    run.stream
        .changes
        .set_replay(&distant_key, &distant, &distant.id);
    run.stream.request.player = placement.pose.position;
    run.stream.request.vehicle = placement.pose.position;
    run.area.items.get_mut(id).unwrap().removed = true;
    run.area.items.mark_dirty(id);
    run.stream.capture_changes(
        &reference,
        &mut run.area.items,
        &run.area.interaction,
        &run.physics,
    );
    run.stream
        .retire_contents(
            &mut run.physics,
            &mut run.area.items,
            &mut run.area.interaction,
            &mut run.wheels,
            &mut run.vehicles,
            &mut run.powertrain,
        )
        .unwrap();
    assert!(run.area.items.resolve_key(&key).is_none());
    assert!(run.stream.changes.get(&key).unwrap().snapshot.is_none());
    assert!(
        run.stream.changes.get(&distant_key).is_some(),
        "nearby consumption must not sweep unrelated distant history"
    );
    run.stream.request.player = [10000., 0., 0.];
    run.stream.request.vehicle = [10000., 0., 0.];
    run.stream
        .retire_contents(
            &mut run.physics,
            &mut run.area.items,
            &mut run.area.interaction,
            &mut run.wheels,
            &mut run.vehicles,
            &mut run.powertrain,
        )
        .unwrap();
    assert!(run.stream.changes.get(&distant_key).is_some());
    assert!(
        run.stream.changes.get(&key).unwrap().snapshot.is_none(),
        "consumed source stays consumed after departure"
    );
}

#[test]
#[ignore = "requires the installed source export"]
fn parked_car_keeps_nearby_item_dynamic() {
    let reference = reference();
    let mut run = GarageRuntime::new_default(&reference).unwrap();
    let (placement, id) = roadside_loot(&mut run, &reference, 813);
    let key = run.area.items.object_key(id);
    let far = [10000., 0., 0.];
    run.stream.request.player = far;
    run.stream.request.vehicle = far;
    run.stream.request_cars([
        (rapier3d::prelude::RigidBodyHandle::invalid(), far),
        (
            rapier3d::prelude::RigidBodyHandle::invalid(),
            placement.pose.position,
        ),
    ]);
    run.stream
        .retire_contents(
            &mut run.physics,
            &mut run.area.items,
            &mut run.area.interaction,
            &mut run.wheels,
            &mut run.vehicles,
            &mut run.powertrain,
        )
        .unwrap();
    let id = run
        .area
        .items
        .resolve_key(&key)
        .expect("parked car keeps item");
    let body = run.area.items.get(id).unwrap().body.unwrap();
    assert!(run.physics.rapier().bodies[body].is_dynamic());
}

fn roadside_stamp(reference: &ReferenceManifest) -> crate::save::SourceStamp {
    let digest = std::process::Command::new("sha256sum")
        .arg(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../web/assets/reference/manifest.json"
        ))
        .output()
        .unwrap();
    assert!(digest.status.success());
    crate::save::SourceStamp {
        app_id: reference.app_id,
        build_id: reference.build_id,
        manifest_sha256: String::from_utf8(digest.stdout)
            .unwrap()
            .split_whitespace()
            .next()
            .unwrap()
            .into(),
        source_hashes: reference
            .source_hashes
            .iter()
            .map(|h| crate::save::SourceHash {
                file: h.file.clone(),
                sha256: h.sha256.clone(),
            })
            .collect(),
    }
}

#[test]
#[ignore = "requires the installed source export"]
fn roadside_possession_survives_owner_retirement_without_duplicate_spawn() {
    use crate::interaction::items::ItemLocation;
    use crate::inventory::InventoryCommand;

    let reference = reference();
    let stamp = roadside_stamp(&reference);
    let mut run = GarageRuntime::new_default(&reference).unwrap();
    prepare_ready(&mut run, &reference);
    let garage_player = run.area.player.body_pose(&run.physics).position;
    let garage_car = run.physics.snapshot(run.vehicle.as_ref().unwrap()).pose;
    let mut sequence = super::super::BuildingSequence::new(&reference.world, run.stream.seed());
    let site = (0..4)
        .filter_map(|_| sequence.next())
        .last()
        .expect("source building");
    assert_eq!(site.prototype.path_id, 83471);
    let focus = [
        site.pose.position[0],
        site.pose.position[1] + 2.,
        site.pose.position[2],
    ];
    let local = run.stream.origin.local(focus);
    move_player(&mut run, local);
    let mut car = run.physics.snapshot(run.vehicle.as_ref().unwrap()).pose;
    car.position = run.stream.origin.local(focus);
    car.position[0] += 5.;
    run.physics
        .reset_preserving_wheels(run.vehicle.as_ref().unwrap(), car);
    run.stream.request_at(focus, focus);
    let mut claim = None;
    let mut held = None;
    let mut stored = None;
    for _ in 0..300 {
        let required = roadside_views(&run.stream);
        prepare_step_for_views(&mut run, &reference, &mut claim, &required);
        for (key, item) in run.area.items.generated_bindings() {
            if key.unit != site.id.unit || key.layer != site.id.layer {
                continue;
            }
            let Some(rule) = run.area.interaction.pickup_rule(item.id) else {
                continue;
            };
            if rule.can_hold_right_hand && rule.can_inventory && !rule.cant_pickup {
                held = Some(item.id);
            } else if rule.can_inventory && !rule.cant_pickup {
                stored = Some(item.id);
            }
        }
        if held.is_some() && stored.is_some() {
            break;
        }
    }
    let held = held.expect("natural right-hand item");
    let stored = stored.expect("natural inventory item");
    let source_part = run
        .area
        .items
        .generated_bindings()
        .find(|(key, item)| {
            key.unit == site.id.unit
                && key.layer == site.id.layer
                && run.area.interaction.parts.contains(item.id)
        })
        .unwrap()
        .1
        .id;
    let keys = [
        run.area.items.object_key(held),
        run.area.items.object_key(stored),
        run.area.items.object_key(source_part),
    ];
    for id in [stored, held] {
        let point = run.area.items.get(id).unwrap().pose.position;
        run.area
            .interaction
            .pickup_item(
                id,
                point,
                &run.area.player,
                &mut run.area.items,
                &mut run.physics,
            )
            .unwrap();
        run.area.sync_inventory();
        if id == stored {
            assert!(
                run.area
                    .apply_inventory_command(
                        InventoryCommand::Select(1),
                        &reference,
                        &mut run.physics,
                    )
                    .unwrap()
            );
        }
    }
    assert_eq!(
        run.area.items.get(held).unwrap().location,
        ItemLocation::Held
    );
    assert_eq!(
        run.area.items.get(stored).unwrap().location,
        ItemLocation::Stored(0)
    );
    let travel: [f32; 3] = std::array::from_fn(|i| garage_player[i] - local[i]);
    move_player(&mut run, garage_player);
    run.physics
        .reset_preserving_wheels(run.vehicle.as_ref().unwrap(), garage_car);
    for id in [held, stored, source_part] {
        let body = run.area.items.get(id).unwrap().body.unwrap();
        let mut pose = *run.physics.rapier().bodies[body].position();
        for (axis, offset) in travel.into_iter().enumerate() {
            pose.translation[axis] += offset;
        }
        run.physics
            .edit_body(body, |state| state.set_position(pose, true));
    }
    run.area.items.sync_poses(&run.physics);
    run.area
        .interaction
        .sync_parts(&mut run.area.items, &mut run.physics)
        .unwrap();
    let mut mount_slot = None;
    for slot in &reference.part_slots {
        let Some(existing) = run.area.interaction.parts.slot_item(slot.id) else {
            continue;
        };
        if run
            .area
            .interaction
            .parts
            .can_remove(existing, &run.area.player, &run.area.items)
            .is_err()
        {
            continue;
        }
        run.area
            .interaction
            .parts
            .remove(existing, &mut run.area.items, &mut run.physics)
            .unwrap();
        if run
            .area
            .interaction
            .parts
            .can_install(source_part, slot.id, &run.area.items)
            .is_ok()
        {
            run.area
                .interaction
                .parts
                .install(source_part, slot.id, &mut run.area.items, &mut run.physics)
                .unwrap();
            mount_slot = Some(slot.id);
            break;
        }
        run.area
            .interaction
            .parts
            .install(existing, slot.id, &mut run.area.items, &mut run.physics)
            .unwrap();
    }
    let mount_slot = mount_slot.expect("compatible garage slot for natural source part");
    assert_eq!(
        run.area.items.get(source_part).unwrap().location,
        ItemLocation::PartSlot(mount_slot)
    );
    run.stream.request_at(
        run.stream
            .origin
            .absolute(run.area.player.focus_position(&run.physics)),
        run.stream.origin.absolute(garage_car.position),
    );
    run.stream
        .retire_contents(
            &mut run.physics,
            &mut run.area.items,
            &mut run.area.interaction,
            &mut run.wheels,
            &mut run.vehicles,
            &mut run.powertrain,
        )
        .unwrap();
    for id in [held, stored, source_part] {
        run.area.items.mark_dirty(id);
    }
    run.stream.capture_changes(
        &reference,
        &mut run.area.items,
        &run.area.interaction,
        &run.physics,
    );
    let saved_unattached = run
        .capture_save(&reference, &stamp, "2026-10-19T12:00:00Z")
        .unwrap();
    let oil = reference
        .items
        .iter()
        .find(|item| item.name == "OilCan")
        .unwrap()
        .id;
    let oil_entries = [
        crate::fluids::FluidEntry {
            fluid_type: 0,
            amount: -0.5,
        },
        crate::fluids::FluidEntry {
            fluid_type: 3,
            amount: 1.,
        },
        crate::fluids::FluidEntry {
            fluid_type: 2,
            amount: -0.25,
        },
    ];
    run.area
        .items
        .get_mut(oil)
        .unwrap()
        .fluid
        .as_mut()
        .unwrap()
        .restore_entries(&oil_entries);
    let owner_colliders = run.physics.rapier().bodies
        [run.area.items.get(held).unwrap().body.unwrap()]
    .colliders()
    .len();
    let oil_colliders = run.physics.rapier().bodies[run.area.items.get(oil).unwrap().body.unwrap()]
        .colliders()
        .len();
    assert!(oil_colliders > 0);
    let mut attached = super::super::changes::capture(
        oil,
        &run.area.items,
        &run.area.interaction,
        &run.physics,
        &reference,
    )
    .unwrap();
    attached.attachment = Some(super::super::AttachmentSnapshot {
        owner: Some(keys[0].clone()),
        slot: None,
        volume: None,
        local_pose: super::super::AbsolutePose {
            position: [0.; 3],
            rotation: [0., 0., 0., 1.],
        },
    });
    assert!(
        super::content_bindings::restore_item(
            oil,
            &attached,
            &reference,
            &mut run.area.items,
            &mut run.area.interaction,
            &mut run.physics,
            &mut run.wheels,
        )
        .unwrap()
    );
    assert_eq!(
        run.area.items.get(oil).unwrap().location,
        ItemLocation::Attached(held)
    );
    let saved = run
        .capture_save(&reference, &stamp, "2026-10-19T12:00:00Z")
        .unwrap();
    let saved: crate::save::SaveSnapshot =
        serde_json::from_slice(&serde_json::to_vec(&saved).unwrap()).unwrap();
    assert_eq!(saved.items.len(), reference.items.len());
    for key in &keys {
        assert!(saved.world.changes.get(key).is_some());
    }
    let mut invalid = saved.clone();
    assert!(invalid.world.changes.forget(&keys[1]));
    assert!(
        GarageRuntime::from_save(
            &reference,
            &stamp,
            &invalid,
            crate::world::GameMode::default(),
            reference.world.draw_distance.default
        )
        .is_err()
    );
    let mut loaded = GarageRuntime::from_save(
        &reference,
        &stamp,
        &saved,
        crate::world::GameMode::default(),
        reference.world.draw_distance.default,
    )
    .unwrap();
    assert!(loaded.restore_pending());
    assert_eq!(
        loaded
            .area
            .items
            .get(oil)
            .unwrap()
            .fluid
            .as_ref()
            .unwrap()
            .entries()
            .collect::<Vec<_>>(),
        oil_entries[1..]
    );
    let mut claim = None;
    for _ in 0..1000 {
        let required = roadside_views(&loaded.stream);
        prepare_step_for_views(&mut loaded, &reference, &mut claim, &required);
        if loaded.advance_saved_restore(&reference).unwrap() {
            break;
        }
    }
    assert!(
        !loaded.restore_pending(),
        "generated outer restore remained pending"
    );
    let restored_held = loaded.area.items.resolve_key(&keys[0]).unwrap();
    let restored_stored = loaded.area.items.resolve_key(&keys[1]).unwrap();
    let restored_mounted = loaded.area.items.resolve_key(&keys[2]).unwrap();
    assert_eq!(
        loaded.area.items.get(restored_held).unwrap().location,
        ItemLocation::Held
    );
    assert_eq!(
        loaded.area.items.get(restored_stored).unwrap().location,
        ItemLocation::Stored(0)
    );
    assert_eq!(
        loaded.area.items.get(restored_mounted).unwrap().location,
        ItemLocation::PartSlot(mount_slot)
    );
    assert_eq!(
        loaded.area.interaction.parts.slot_item(mount_slot),
        Some(restored_mounted)
    );
    assert_eq!(loaded.area.interaction.held(), Some(restored_held));
    assert_eq!(
        loaded.area.items.get(oil).unwrap().location,
        ItemLocation::Attached(restored_held)
    );
    for _ in 0..2 {
        assert!(loaded.advance_saved_restore(&reference).unwrap());
        assert_eq!(
            loaded
                .area
                .items
                .get(oil)
                .unwrap()
                .fluid
                .as_ref()
                .unwrap()
                .entries()
                .collect::<Vec<_>>(),
            oil_entries[1..]
        );
    }
    assert_eq!(
        loaded.area.items.get(oil).unwrap().body,
        loaded.area.items.get(restored_held).unwrap().body
    );
    let restored_body = loaded.area.items.get(restored_held).unwrap().body.unwrap();
    assert!(
        loaded.physics.rapier().bodies[restored_body]
            .colliders()
            .len()
            >= owner_colliders + oil_colliders
    );
    let recaptured = loaded
        .capture_save(&reference, &stamp, "2026-10-19T12:00:00Z")
        .unwrap();
    assert_eq!(saved.inventory, recaptured.inventory);
    let oil_instance = crate::save::InstanceId::initial(
        &reference
            .items
            .iter()
            .find(|item| item.id == oil)
            .unwrap()
            .source,
    );
    assert_eq!(
        recaptured
            .items
            .iter()
            .find(|item| item.instance == oil_instance)
            .unwrap()
            .fluids
            .as_deref(),
        Some(&oil_entries[1..])
    );
    assert_eq!(
        saved
            .items
            .iter()
            .find(|item| item.instance == oil_instance)
            .unwrap()
            .location,
        recaptured
            .items
            .iter()
            .find(|item| item.instance == oil_instance)
            .unwrap()
            .location,
    );
    for key in &keys {
        assert!(recaptured.world.changes.get(key).is_some());
        assert_eq!(
            loaded
                .area
                .items
                .generated_bindings()
                .filter(|(id, _)| { super::super::ObjectKey::Generated((*id).clone()) == *key })
                .count(),
            1
        );
    }
    let child_before = loaded.area.items.get(oil).unwrap().pose.position;
    let original_pose = *loaded.physics.rapier().bodies[restored_body].position();
    let mut shifted_pose = original_pose;
    shifted_pose.translation.x += 1.;
    loaded
        .physics
        .edit_body(restored_body, |body| body.set_position(shifted_pose, true));
    loaded.area.items.sync_poses(&loaded.physics);
    assert!(
        (loaded.area.items.get(oil).unwrap().pose.position[0] - child_before[0] - 1.).abs()
            < 0.0001
    );
    loaded
        .physics
        .edit_body(restored_body, |body| body.set_position(original_pose, true));
    loaded.area.items.sync_poses(&loaded.physics);
    assert!(
        loaded
            .area
            .apply_inventory_command(InventoryCommand::Select(0), &reference, &mut loaded.physics,)
            .unwrap()
    );
    assert_eq!(loaded.area.interaction.held(), Some(restored_stored));
    assert_eq!(
        loaded
            .area
            .interaction
            .drop_held(&mut loaded.area.items, &mut loaded.physics),
        Some(restored_stored)
    );
    loaded.area.sync_inventory();
    loaded.area.items.mark_dirty(restored_stored);
    loaded.stream.capture_changes(
        &reference,
        &mut loaded.area.items,
        &loaded.area.interaction,
        &loaded.physics,
    );
    let far = [10000., 0., 0.];
    let far_local = loaded.stream.origin.local(far);
    move_player(&mut loaded, far_local);
    let mut car = loaded
        .physics
        .snapshot(loaded.vehicle.as_ref().unwrap())
        .pose;
    car.position = far_local;
    loaded
        .physics
        .reset_preserving_wheels(loaded.vehicle.as_ref().unwrap(), car);
    loaded.stream.request_at(far, far);
    loaded
        .stream
        .retire_contents(
            &mut loaded.physics,
            &mut loaded.area.items,
            &mut loaded.area.interaction,
            &mut loaded.wheels,
            &mut loaded.vehicles,
            &mut loaded.powertrain,
        )
        .unwrap();
    assert!(loaded.area.items.resolve_key(&keys[1]).is_none());
    assert!(loaded.stream.changes.get(&keys[1]).is_some());
    let mut reset = GarageRuntime::from_save(
        &reference,
        &stamp,
        &saved_unattached,
        crate::world::GameMode::default(),
        reference.world.draw_distance.default,
    )
    .unwrap();
    let mut claim = None;
    for _ in 0..1000 {
        let required = roadside_views(&reset.stream);
        prepare_step_for_views(&mut reset, &reference, &mut claim, &required);
        if reset.advance_saved_restore(&reference).unwrap() {
            break;
        }
    }
    assert!(!reset.restore_pending());
    let reset_stored = reset.area.items.resolve_key(&keys[1]).unwrap();
    let reset_mounted = reset.area.items.resolve_key(&keys[2]).unwrap();
    assert!(
        reset
            .area
            .apply_inventory_command(InventoryCommand::Select(0), &reference, &mut reset.physics,)
            .unwrap()
    );
    assert_eq!(
        reset
            .area
            .interaction
            .drop_held(&mut reset.area.items, &mut reset.physics),
        Some(reset_stored)
    );
    reset.area.sync_inventory();
    assert!(
        reset
            .area
            .apply_inventory_command(InventoryCommand::Select(1), &reference, &mut reset.physics,)
            .unwrap()
    );
    let reset_held = reset.area.items.resolve_key(&keys[0]).unwrap();
    assert_eq!(reset.area.interaction.equipment.item(), Some(reset_held));
    reset.area.items.sync_poses(&reset.physics);
    reset.area.items.mark_dirty(reset_held);
    reset.stream.capture_changes(
        &reference,
        &mut reset.area.items,
        &reset.area.interaction,
        &reset.physics,
    );
    let equipped_save = reset
        .capture_save(&reference, &stamp, "2026-10-19T12:00:00Z")
        .unwrap();
    assert_eq!(
        equipped_save.inventory.equipped,
        Some(crate::save::InstanceId::Generated {
            id: match &keys[0] {
                super::super::ObjectKey::Generated(id) => id.clone(),
                _ => unreachable!(),
            },
        })
    );
    let mut unlisted_equipped = equipped_save.clone();
    unlisted_equipped.inventory.slots[unlisted_equipped.inventory.selected as usize] = None;
    let mut independent = GarageRuntime::from_save(
        &reference,
        &stamp,
        &unlisted_equipped,
        crate::world::GameMode::default(),
        reference.world.draw_distance.default,
    )
    .unwrap();
    prepare_ready(&mut independent, &reference);
    assert!(independent.advance_saved_restore(&reference).unwrap());
    assert!(!independent.restore_pending());
    let independent_tool = independent.area.items.resolve_key(&keys[0]).unwrap();
    assert_eq!(
        independent.area.interaction.equipment.item(),
        Some(independent_tool)
    );
    assert!(
        !independent
            .area
            .inventory
            .slots
            .contains(&Some(independent_tool))
    );
    g08_assert_equipment_pose(&independent, &reference, independent_tool);
    let independent_save = independent
        .capture_save(&reference, &stamp, "2026-10-19T12:00:01Z")
        .unwrap();
    assert_eq!(independent_save.inventory, unlisted_equipped.inventory);
    let tool_key = &keys[0];
    let before_tool = unlisted_equipped
        .world
        .changes
        .get(tool_key)
        .unwrap()
        .snapshot
        .as_ref()
        .unwrap();
    let after_tool = independent_save
        .world
        .changes
        .get(tool_key)
        .unwrap()
        .snapshot
        .as_ref()
        .unwrap();
    for axis in 0..3 {
        assert!((before_tool.pose.position[axis] - after_tool.pose.position[axis]).abs() < 0.0001);
    }

    let rotation_alignment: f32 = before_tool
        .pose
        .rotation
        .iter()
        .zip(after_tool.pose.rotation)
        .map(|(before, after)| before * after)
        .sum();
    assert!(rotation_alignment.abs() > 0.99999);
    assert!(independent.stream.required_ready());
    for key in &keys {
        assert_eq!(
            independent
                .area
                .items
                .generated_bindings()
                .filter(|(id, _)| super::super::ObjectKey::Generated((*id).clone()) == *key)
                .count(),
            1
        );
    }
    let mut invalid = unlisted_equipped.clone();
    invalid.world.changes.remove(tool_key.clone());
    assert!(
        GarageRuntime::from_save(
            &reference,
            &stamp,
            &invalid,
            crate::world::GameMode::default(),
            reference.world.draw_distance.default
        )
        .is_err()
    );
    let mut invalid = equipped_save.clone();
    invalid.inventory.slots[2] = invalid.inventory.equipped.clone();
    assert!(
        GarageRuntime::from_save(
            &reference,
            &stamp,
            &invalid,
            crate::world::GameMode::default(),
            reference.world.draw_distance.default
        )
        .is_err()
    );
    let mut invalid = unlisted_equipped.clone();
    let mut held_state = before_tool.clone();
    held_state.held = true;
    invalid
        .world
        .changes
        .record(tool_key.clone(), held_state, before_tool);
    assert!(
        GarageRuntime::from_save(
            &reference,
            &stamp,
            &invalid,
            crate::world::GameMode::default(),
            reference.world.draw_distance.default
        )
        .is_err()
    );
    let mut invalid = unlisted_equipped.clone();
    invalid.inventory.equipped = Some(crate::save::InstanceId::Generated {
        id: match &keys[1] {
            super::super::ObjectKey::Generated(id) => id.clone(),
            _ => unreachable!(),
        },
    });
    invalid.inventory.slots[0] = None;
    assert!(
        invalid.validate(&reference, &stamp).is_ok(),
        "prototype checks wait for generated bindings"
    );
    let mut ineligible = GarageRuntime::from_save(
        &reference,
        &stamp,
        &invalid,
        crate::world::GameMode::default(),
        reference.world.draw_distance.default,
    )
    .unwrap();
    prepare_ready(&mut ineligible, &reference);
    assert!(
        ineligible.advance_saved_restore(&reference).is_err(),
        "generated equipment eligibility remains enforced"
    );
    // Reject generated ownership and attachment conflicts without slot membership.
    let mut invalid = unlisted_equipped.clone();
    invalid.player.carry = saved_unattached.player.carry.clone();
    assert!(
        GarageRuntime::from_save(
            &reference,
            &stamp,
            &invalid,
            crate::world::GameMode::default(),
            reference.world.draw_distance.default
        )
        .is_err()
    );
    let mut invalid = unlisted_equipped.clone();
    let mut change = invalid.world.changes.get(tool_key).unwrap().clone();
    change.snapshot.as_mut().unwrap().attachment = Some(super::super::AttachmentSnapshot {
        owner: Some(keys[1].clone()),
        slot: None,
        volume: None,
        local_pose: super::super::AbsolutePose {
            position: [0.; 3],
            rotation: [0., 0., 0., 1.],
        },
    });
    invalid
        .world
        .changes
        .record(tool_key.clone(), change.snapshot.unwrap(), before_tool);
    assert!(
        GarageRuntime::from_save(
            &reference,
            &stamp,
            &invalid,
            crate::world::GameMode::default(),
            reference.world.draw_distance.default
        )
        .is_err()
    );
    assert_eq!(
        reset
            .area
            .interaction
            .equipment
            .detach(&mut reset.area.items, &mut reset.physics),
        Some(reset_held)
    );
    reset.area.sync_inventory();
    reset
        .area
        .interaction
        .parts
        .remove(reset_mounted, &mut reset.area.items, &mut reset.physics)
        .unwrap();
    for id in [reset_held, reset_stored, reset_mounted] {
        assert_eq!(
            reset.area.items.get(id).unwrap().location,
            ItemLocation::World
        );
        reset.area.items.mark_dirty(id);
    }
    reset.stream.capture_changes(
        &reference,
        &mut reset.area.items,
        &reset.area.interaction,
        &reset.physics,
    );
    let reset_far_local = reset.stream.origin.local(far);
    move_player(&mut reset, reset_far_local);
    let mut reset_car = reset.physics.snapshot(reset.vehicle.as_ref().unwrap()).pose;
    reset_car.position = reset_far_local;
    reset
        .physics
        .reset_preserving_wheels(reset.vehicle.as_ref().unwrap(), reset_car);
    reset.stream.request_at(far, far);
    reset
        .stream
        .retire_contents(
            &mut reset.physics,
            &mut reset.area.items,
            &mut reset.area.interaction,
            &mut reset.wheels,
            &mut reset.vehicles,
            &mut reset.powertrain,
        )
        .unwrap();
    for key in &keys {
        assert!(reset.area.items.resolve_key(key).is_none());
        assert!(reset.stream.changes.get(key).is_some());
    }
    let revisit_local = reset.stream.origin.local(focus);
    move_player(&mut reset, revisit_local);
    reset_car.position = revisit_local;
    reset
        .physics
        .reset_preserving_wheels(reset.vehicle.as_ref().unwrap(), reset_car);
    reset.stream.request_at(focus, focus);
    for _ in 0..300 {
        let required = roadside_views(&reset.stream);
        prepare_step_for_views(&mut reset, &reference, &mut claim, &required);
        if reset.area.items.resolve_key(&keys[1]).is_some() {
            break;
        }
    }
    let revisited = reset
        .area
        .items
        .resolve_key(&keys[1])
        .expect("dropped loot returns after full departure");
    assert_eq!(
        reset.area.items.get(revisited).unwrap().location,
        ItemLocation::World
    );
    assert!(reset.stream.changes.get(&keys[1]).is_some());
}
fn roadside_views(stream: &WorldStream) -> [super::super::RequiredView; 1] {
    let [x, y, z] = stream.origin.local(stream.request.player);
    let planes = [
        [1., 0., 0., 30. - x],
        [-1., 0., 0., 30. + x],
        [0., 1., 0., 30. - y],
        [0., -1., 0., 30. + y],
        [0., 0., 1., 30. + z],
        [0., 0., -1., 30. - z],
    ];
    [super::super::RequiredView {
        camera: planes,
        shadow: crate::reference::shadow_candidates::DirectionalHull::new(planes, [1., 0., 0.])
            .unwrap(),
        mask: u32::MAX,
        light_mask: u32::MAX,
        origin: stream.origin.offset,
    }]
}

fn g08_ready(reference: &ReferenceManifest) -> GarageRuntime {
    let mut run = GarageRuntime::new_default(reference).unwrap();
    prepare_ready(&mut run, reference);
    run.sync_world_activity(reference).unwrap();
    run.started = true;
    assert!(run.world_supported());
    run
}
fn g08_pickup(run: &mut GarageRuntime, item: crate::reference::ItemId) {
    let point = run.area.items.get(item).unwrap().pose.position;
    if run.area.interaction.storage.stored(item) {
        run.area
            .interaction
            .pickup_stored_item(
                item,
                point,
                &run.area.player,
                &mut run.area.items,
                &mut run.physics,
            )
            .unwrap();
    } else {
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
    }
    run.area.sync_inventory();
}
fn g08_frame(
    run: &mut GarageRuntime,
    reference: &ReferenceManifest,
    input: crate::simulation::FrameInput,
    dt: f32,
) -> crate::starting_area::InventoryFrame {
    let view = run.area.player.view(&run.physics).head_pose;
    run.advance_inventory_frame(reference, input, dt, view)
}
/// PB13: a short Tab tap on foot puts the carried sunglasses on.
#[test]
#[ignore = "requires the installed source export"]
fn tab_tap_puts_on_carried_sunglasses() {
    use crate::{
        interaction::items::ItemLocation,
        simulation::{FIXED_DT, FrameInput, keys},
    };
    let reference = reference();
    let mut run = g08_ready(&reference);
    let glasses = reference
        .clothing
        .wearables
        .iter()
        .find(|w| !w.lenses.is_empty())
        .unwrap();
    let prototype = reference
        .item_prototypes
        .iter()
        .position(|p| p.id == glasses.prototype)
        .unwrap();
    let pose = run.area.player.body_pose(&run.physics);
    let item = crate::starting_area::spawned::spawn_prototype(
        &reference,
        &mut run.area,
        &mut run.physics,
        prototype,
        pose,
    )
    .unwrap()[0];
    g08_pickup(&mut run, item);
    assert!(!run.area.player.seated());
    let tap = [
        FrameInput {
            pressed: keys::RECENTER,
            held: keys::RECENTER,
            ..Default::default()
        },
        FrameInput {
            held: keys::RECENTER,
            ..Default::default()
        },
        FrameInput {
            released: keys::RECENTER,
            ..Default::default()
        },
    ];
    for input in tap {
        let frame = g08_frame(&mut run, &reference, input, FIXED_DT);
        let mut pending = FrameInput::default();
        frame.merge_fixed_input(&mut pending);
        run.step(&reference, pending);
    }
    assert_eq!(run.area.interaction.held(), None);
    assert!(matches!(
        run.area.items.get(item).unwrap().location,
        ItemLocation::Worn(_)
    ));
}
#[test]
#[ignore = "requires the installed source export"]
fn g08_frame_combined_input_and_cancellation() {
    use crate::{
        interaction::items::ItemLocation,
        simulation::{FrameInput, keys},
    };
    let reference = reference();
    let mut run = g08_ready(&reference);
    let tool = reference.conditions.sprays[0].item;
    g08_pickup(&mut run, tool);
    let down = FrameInput {
        pressed: keys::RECENTER,
        held: keys::RECENTER,
        ..Default::default()
    };
    let up = FrameInput {
        released: keys::RECENTER,
        held: keys::ROTATE,
        ..Default::default()
    };
    g08_frame(&mut run, &reference, down, 0.02);
    g08_frame(&mut run, &reference, up, 0.1);
    assert_eq!(run.area.interaction.equipment.item(), Some(tool));
    assert_eq!(run.area.interaction.held(), None);
    // The guard ends next frame. Held MMB can now release.
    g08_frame(
        &mut run,
        &reference,
        FrameInput {
            held: keys::ROTATE,
            ..Default::default()
        },
        0.02,
    );
    assert_eq!(run.area.interaction.held(), Some(tool));
    g08_frame(&mut run, &reference, FrameInput::default(), 0.3);
    g08_frame(&mut run, &reference, down, 0.02);
    let selected = run.area.inventory.selected;
    let target = (selected + 1) % 4;
    g08_frame(
        &mut run,
        &reference,
        FrameInput {
            inventory_released: 1 << selected,
            ..up
        },
        0.1,
    );
    assert_eq!(
        run.area.interaction.equipment.item(),
        Some(tool),
        "explicit equip precedes slot retrieval and retains its update guard"
    );
    g08_frame(
        &mut run,
        &reference,
        FrameInput {
            inventory_released: 1 << target,
            ..Default::default()
        },
        0.1,
    );
    assert_eq!(run.area.inventory.selected, target);
    assert_eq!(
        run.area.items.get(tool).unwrap().location,
        ItemLocation::Stored(selected)
    );
    assert_eq!(
        run.area.interaction.held(),
        None,
        "Tab equip must occur before selection stores the owner"
    );
    // Each number phase executes once. Selection resets progress, then held adds 0.5.
    let next = (target + 1) % 4;
    g08_frame(
        &mut run,
        &reference,
        FrameInput {
            inventory_released: 1 << next,
            inventory_held: 1 << selected,
            ..Default::default()
        },
        0.1,
    );
    assert_eq!(run.area.inventory.selected, next);
    assert!((run.area.inventory.swap_progress - 0.5).abs() < 0.00001);
    g08_frame(
        &mut run,
        &reference,
        FrameInput {
            inventory_released: 1 << selected,
            held: keys::ROTATE,
            ..Default::default()
        },
        0.02,
    );
    assert_eq!(
        run.area.interaction.held(),
        Some(tool),
        "immediate retrieval does not set the explicit-equip guard"
    );
    for pause in [true, false] {
        g08_frame(&mut run, &reference, down, 0.02);
        if pause {
            run.paused = true;
            g08_frame(&mut run, &reference, up, 0.1);
            run.paused = false;
        } else {
            run.clear_input();
        }
        g08_frame(&mut run, &reference, up, 0.1);
        assert_eq!(run.area.interaction.held(), Some(tool));
        assert_eq!(run.area.interaction.equipment.item(), None);
    }
    // Reset creates fresh gesture state. A stale release cannot equip.
    let mut reset = g08_ready(&reference);
    g08_pickup(&mut reset, tool);
    g08_frame(&mut reset, &reference, up, 0.1);
    assert_eq!(reset.area.interaction.equipment.item(), None);
}
#[test]
#[ignore = "requires the installed source export"]
fn g08_frame_zero_and_multiple_fixed_steps() {
    use crate::{
        simulation::{FrameInput, keys},
        vehicle::to_rapier,
    };
    let reference = reference();
    let spray = reference.conditions.sprays[0].item;
    let sample = |steps: usize| {
        let mut run = g08_ready(&reference);
        g08_pickup(&mut run, spray);
        assert!(run.area.equip_carried(&mut run.physics).unwrap());
        run.area.sync_equipment(&reference, &mut run.physics);
        let anchor = reference.clothing.pose.anchors["RH"].node;
        let item = run.area.items.get(spray).unwrap();
        let local = to_rapier(item.local_pose);
        let initial = to_rapier(run.area.pose.anchor(anchor).unwrap()).inverse()
            * run.physics.rapier().bodies[item.body.unwrap()].position()
            * local;
        let rule = run.area.interaction.pickup_rule(spray).unwrap();
        let target = if rule.disable_rh_fix {
            rapier3d::math::Pose::IDENTITY
        } else {
            to_rapier(crate::vehicle::VehiclePose {
                position: rule.right_hand_position,
                rotation: rule.right_hand_rotation,
            })
        };
        let gain = (0.04 * reference.inventory.hand_interpolation_factor).clamp(0., 1.);
        let expected_position = initial
            .translation
            .lerp(target.translation, gain)
            .lerp(target.translation, gain);
        let expected_rotation = initial
            .rotation
            .lerp(target.rotation, gain)
            .lerp(target.rotation, gain);
        let input = FrameInput {
            held: keys::PRIMARY,
            inventory_held: 2,
            ..Default::default()
        };
        g08_frame(&mut run, &reference, input, 0.04);
        let before_quantity = run
            .area
            .interaction
            .cleaning
            .get(spray)
            .unwrap()
            .quantity
            .unwrap();
        let before_age = run.area.interaction.cleaning.particles(spray, 0)[0].age;
        let frame = g08_frame(&mut run, &reference, input, 0.04);
        let mut pending = FrameInput::default();
        frame.merge_fixed_input(&mut pending);
        for _ in 0..steps {
            run.step(&reference, pending.take_step());
        }
        let after_quantity = run
            .area
            .interaction
            .cleaning
            .get(spray)
            .unwrap()
            .quantity
            .unwrap();
        let after_age = run.area.interaction.cleaning.particles(spray, 0)[0].age;
        let anchor = reference.clothing.pose.anchors["RH"].node;
        let hand = to_rapier(run.area.pose.anchor(anchor).unwrap()).inverse()
            * run.physics.rapier().bodies[run.area.items.get(spray).unwrap().body.unwrap()]
                .position()
            * local;
        assert!(
            (hand.translation - expected_position).length() < 0.0001,
            "hand time advances once per frame"
        );
        assert!(hand.rotation.dot(expected_rotation).abs() > 0.99999);
        (
            after_quantity - before_quantity,
            after_age - before_age,
            run.area.inventory.swap_progress,
            hand,
        )
    };
    let expected = sample(0);
    assert!(expected.0 < 0.);
    assert!((expected.1 - 0.04).abs() < 0.00001);
    for steps in [1, 2] {
        let actual = sample(steps);
        assert!((actual.0 - expected.0).abs() < 0.00001);
        assert!((actual.1 - expected.1).abs() < 0.00001);
        assert!((actual.2 - expected.2).abs() < 0.00001);
        assert!((actual.3.translation - expected.3.translation).length() < 0.0001);
        assert!(actual.3.rotation.dot(expected.3.rotation).abs() > 0.99999);
    }
    let mut run = g08_ready(&reference);
    let light = reference
        .flashlights
        .instances
        .iter()
        .find(|l| l.startup_active)
        .unwrap()
        .item;
    g08_pickup(&mut run, light);
    assert!(run.area.equip_carried(&mut run.physics).unwrap());
    let control = run.area.interaction.owned_light_control(light).unwrap();
    let index = reference
        .lighting
        .controllers
        .iter()
        .position(|c| c.id == control)
        .unwrap();
    let carried_spray = reference.conditions.sprays[0].item;
    g08_pickup(&mut run, carried_spray);
    let quantity = run
        .area
        .interaction
        .cleaning
        .get(carried_spray)
        .unwrap()
        .quantity
        .unwrap();
    let on = run.lighting.controllers[index].on;
    let frame = g08_frame(
        &mut run,
        &reference,
        FrameInput {
            pressed: keys::PRIMARY | keys::USE,
            held: keys::PRIMARY,
            mouse: [0.2, 0.3],
            ..Default::default()
        },
        0.04,
    );
    assert!(
        run.area
            .interaction
            .cleaning
            .get(carried_spray)
            .unwrap()
            .quantity
            .unwrap()
            < quantity,
        "flashlight reservation preserves raw LMB for carried spray"
    );
    assert!(frame.primary_reserved);
    assert_eq!(run.lighting.controllers[index].on, !on);
    assert_eq!(frame.fixed_input.pressed & keys::PRIMARY, 0);
    let mut pending = FrameInput::default();
    frame.merge_fixed_input(&mut pending);
    let empty = g08_frame(&mut run, &reference, FrameInput::default(), 0.01);
    empty.merge_fixed_input(&mut pending);
    let step = pending.take_step();
    assert_eq!(step.pressed & keys::USE, keys::USE);
    assert_eq!(step.mouse, [0.2, 0.3]);
    run.step(&reference, step);
    assert_eq!(
        run.lighting.controllers[index].on, !on,
        "fixed interaction cannot repeat consumed PRIMARY"
    );
    assert_eq!(pending.take_step().pressed & keys::USE, 0);
    let body = run.area.items.get(light).unwrap().body.unwrap();
    let hand_before = *run.physics.rapier().bodies[body].position();
    run.paused = true;
    let blocked = g08_frame(
        &mut run,
        &reference,
        FrameInput {
            held: keys::PRIMARY,
            ..Default::default()
        },
        0.2,
    );
    assert!(blocked.actions_blocked);
    assert_eq!(
        *run.physics.rapier().bodies[body].position(),
        hand_before,
        "blocked frame does not advance hand animation"
    );
}
#[test]
#[ignore = "requires the installed source export"]
fn g08_frame_blocked_states_do_not_mutate() {
    use crate::simulation::{FrameInput, keys};
    let reference = reference();
    for blocked in 0..11 {
        let mut run = g08_ready(&reference);
        let tool = reference.conditions.sprays[0].item;
        g08_pickup(&mut run, tool);
        g08_frame(
            &mut run,
            &reference,
            FrameInput {
                held: keys::PRIMARY,
                ..Default::default()
            },
            0.04,
        );
        g08_frame(
            &mut run,
            &reference,
            FrameInput {
                held: keys::RECENTER,
                pressed: keys::RECENTER,
                ..Default::default()
            },
            0.02,
        );
        match blocked {
            0 => run.started = false,
            1 => run.paused = true,
            2 => run.recovering = true,
            3 => run.startup_aligned = false,
            4 => {
                let input = crate::sleep::SleepInput {
                    held: true,
                    active: true,
                    camera_allowed: true,
                    dead: false,
                    show_text: false,
                };
                run.sleep
                    .step(&reference.sleep, input, 1., 1. / reference.sleep.hold_gain);
                run.sleep.step(&reference.sleep, input, 1.1, 0.02);
                assert!(run.sleeping());
            }
            5 => run.survival.damage_instant(&reference.survival, 10000.),
            _ => {}
        }
        let snapshot = |run: &GarageRuntime| {
            let item = run.area.items.get(tool).unwrap();
            let body = &run.physics.rapier().bodies[item.body.unwrap()];
            format!(
                "{:?}{:?}{:?}{:?}{:?}{:?}{:?}{:?}{:?}{:?}",
                run.area.inventory.slots,
                run.area.inventory.selected,
                run.area.interaction.held(),
                run.area.interaction.equipment.item(),
                item,
                body.body_type(),
                body.is_enabled(),
                body.position(),
                run.area.interaction.cleaning.get(tool).unwrap().quantity,
                reference
                    .conditions
                    .surfaces
                    .iter()
                    .map(|s| format!("{:?}", run.area.interaction.conditions.get(s.id)))
                    .collect::<Vec<_>>()
            )
        };
        let particles_before: Vec<_> = run
            .area
            .interaction
            .cleaning
            .particles(reference.conditions.sprays[0].item, 0)
            .iter()
            .map(|p| p.age)
            .collect();
        assert!(!particles_before.is_empty());
        let physical = |run: &GarageRuntime| {
            let body = run.area.items.get(tool).unwrap().body.unwrap();
            let colliders: Vec<_> = run.physics.rapier().bodies[body]
                .colliders()
                .iter()
                .map(|h| format!("{:?}", run.physics.rapier().colliders[*h]))
                .collect();
            let joints: Vec<_> = run
                .physics
                .rapier()
                .impulse_joints
                .iter()
                .map(|(h, j)| format!("{h:?}{j:?}"))
                .collect();
            (colliders, joints)
        };
        let physical_before = physical(&run);
        let before = snapshot(&run);
        let dt = match blocked {
            6 => 0.,
            7 => -0.1,
            8 => f32::NAN,
            9 => f32::INFINITY,
            10 => f32::NEG_INFINITY,
            _ => 0.1,
        };
        let result = g08_frame(
            &mut run,
            &reference,
            FrameInput {
                held: keys::PRIMARY | keys::ROTATE,
                released: keys::RECENTER,
                inventory_released: 2,
                inventory_held: 4,
                ..Default::default()
            },
            dt,
        );
        assert!(result.actions_blocked, "blocked state {blocked}");
        assert_eq!(snapshot(&run), before, "blocked state {blocked}");
        assert_eq!(
            physical(&run),
            physical_before,
            "blocked physical ownership {blocked}"
        );
        assert_eq!(
            run.area
                .interaction
                .cleaning
                .particles(reference.conditions.sprays[0].item, 0)
                .iter()
                .map(|p| p.age)
                .collect::<Vec<_>>(),
            particles_before,
            "blocked particle ages {blocked}"
        );
        run.started = true;
        run.paused = false;
        run.recovering = false;
        run.startup_aligned = true;
        run.sleep.interrupt();
        // Death remains blocked. All other states resume with the old release canceled.
        g08_frame(
            &mut run,
            &reference,
            FrameInput {
                released: keys::RECENTER,
                ..Default::default()
            },
            0.1,
        );
        assert_eq!(
            run.area.interaction.equipment.item(),
            None,
            "stale release in state {blocked}"
        );
    }
}

#[test]
#[ignore = "requires the installed source export"]
fn g08_frame_clutch_binding_collision() {
    use crate::simulation::{FrameInput, keys};
    let reference = reference();
    for collision in [false, true] {
        let mut run = g08_ready(&reference);
        let mut settings = run.player_settings;
        settings.inventory_gear_keys = collision;
        run.set_player_settings(settings);
        assert!(drive_out::enter(&mut run, &reference));
        let input = FrameInput {
            held: keys::CLUTCH,
            inventory_released: 2,
            inventory_held: 4,
            ..Default::default()
        };
        assert_eq!(run.area.inventory_context(input).seated_clutch, collision);
        g08_frame(&mut run, &reference, input, 0.1);
        assert_eq!(run.area.inventory.selected, if collision { 0 } else { 1 });
        assert_eq!(
            run.area.inventory.swap_progress,
            if collision { 0. } else { 0.5 }
        );
    }
}

#[test]
#[ignore = "requires the installed source export"]
fn g08_frame_fresh_gesture_after_cancellation() {
    use crate::simulation::{FrameInput, keys};
    let reference = reference();
    let mut run = g08_ready(&reference);
    let tool = reference.conditions.sprays[0].item;
    g08_pickup(&mut run, tool);
    let down = FrameInput {
        pressed: keys::RECENTER,
        held: keys::RECENTER,
        ..Default::default()
    };
    let up = FrameInput {
        released: keys::RECENTER,
        ..Default::default()
    };
    for pause in [true, false] {
        g08_frame(&mut run, &reference, down, 0.02);
        if pause {
            run.paused = true;
            assert!(g08_frame(&mut run, &reference, FrameInput::default(), 0.02).actions_blocked);
            run.paused = false;
        } else {
            run.clear_input();
        }
        // No stale release or idle frame rearms this fresh gesture.
        g08_frame(&mut run, &reference, down, 0.02);
        g08_frame(&mut run, &reference, up, 0.1);
        assert_eq!(
            run.area.interaction.equipment.item(),
            Some(tool),
            "fresh gesture after pause={pause}"
        );
        assert_eq!(run.area.interaction.held(), None);
        assert!(run.area.release_equipped(&mut run.physics).unwrap());
    }
}

#[test]
#[ignore = "Requires the installed runtime export"]
fn source_transfers_reject_atomically_and_restore_both_hands() {
    use crate::{
        inventory::InventoryCommand,
        save::{SourceHash, SourceStamp},
        simulation::FrameInput,
    };
    let bytes = std::fs::read(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../web/assets/reference/manifest.json"
    ))
    .unwrap();
    let r = ReferenceManifest::from_json(&bytes).unwrap();
    let digest = std::process::Command::new("sha256sum")
        .arg(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../web/assets/reference/manifest.json"
        ))
        .output()
        .unwrap();
    assert!(digest.status.success());
    let stamp = SourceStamp {
        app_id: r.app_id,
        build_id: r.build_id,
        manifest_sha256: String::from_utf8(digest.stdout)
            .unwrap()
            .split_whitespace()
            .next()
            .unwrap()
            .into(),
        source_hashes: r
            .source_hashes
            .iter()
            .map(|h| SourceHash {
                file: h.file.clone(),
                sha256: h.sha256.clone(),
            })
            .collect(),
    };
    let mut game = GarageRuntime::new_default(&r).unwrap();
    let oil = r.items.iter().find(|i| i.name == "OilCan").unwrap().id;
    let light = r
        .flashlights
        .instances
        .iter()
        .find(|f| f.startup_active)
        .unwrap()
        .item;
    let food = r.survival.starter_food;
    for (id, select) in [(oil, 1), (light, 0)] {
        let point = game.area.items.get(id).unwrap().pose.position;
        if game.area.interaction.storage.stored(id) {
            game.area
                .interaction
                .pickup_stored_item(
                    id,
                    point,
                    &game.area.player,
                    &mut game.area.items,
                    &mut game.physics,
                )
                .unwrap();
        } else {
            game.area
                .interaction
                .pickup_item(
                    id,
                    point,
                    &game.area.player,
                    &mut game.area.items,
                    &mut game.physics,
                )
                .unwrap();
        }
        game.area.sync_inventory();
        assert!(
            game.area
                .apply_inventory_command(InventoryCommand::Select(select), &r, &mut game.physics)
                .unwrap()
        );
    }
    assert!(
        game.area
            .apply_inventory_command(InventoryCommand::Select(1), &r, &mut game.physics)
            .unwrap()
    );
    assert_eq!(game.area.interaction.equipment.item(), Some(light));
    let point = game.area.items.get(food).unwrap().pose.position;
    game.area
        .interaction
        .pickup_item(
            food,
            point,
            &game.area.player,
            &mut game.area.items,
            &mut game.physics,
        )
        .unwrap();
    game.area.sync_inventory();
    let handle = game.area.items.get(food).unwrap().body.unwrap();
    let before = (
        *game.physics.rapier().bodies[handle].position(),
        game.physics.rapier().bodies[handle].linvel(),
        game.physics.rapier().impulse_joints.len(),
        game.area.inventory.slots,
    );
    for slot in [0, 1] {
        assert!(
            !game
                .area
                .apply_inventory_command(InventoryCommand::Select(slot), &r, &mut game.physics)
                .unwrap()
        );
    }
    assert_eq!(
        before,
        (
            *game.physics.rapier().bodies[handle].position(),
            game.physics.rapier().bodies[handle].linvel(),
            game.physics.rapier().impulse_joints.len(),
            game.area.inventory.slots
        )
    );
    assert!(game.consume(&r, food).unwrap());
    let saved = game
        .capture_save(&r, &stamp, "2026-10-15T12:00:00Z")
        .unwrap();
    let mut loaded = GarageRuntime::from_save(
        &r,
        &stamp,
        &saved,
        crate::world::GameMode::default(),
        r.world.draw_distance.default,
    )
    .unwrap();
    prepare_ready(&mut loaded, &r);
    assert!(loaded.advance_saved_restore(&r).unwrap());
    let recaptured = loaded
        .capture_save(&r, &stamp, "2026-10-15T12:00:00Z")
        .unwrap();
    for (before, after) in saved.items.iter().zip(&recaptured.items) {
        if matches!(
            before.location,
            crate::save::ItemLocationSave::Stored { .. }
        ) {
            assert_eq!(before.instance, after.instance);
            for axis in 0..3 {
                assert!((before.pose.position[axis] - after.pose.position[axis]).abs() < 0.0001);
            }
        }
    }
    assert_eq!(loaded.area.inventory.slots, game.area.inventory.slots);
    assert_eq!(loaded.area.interaction.held(), Some(food));
    assert_eq!(loaded.area.interaction.equipment.item(), Some(light));
    assert_eq!(loaded.area.items.get(food).unwrap().food.unwrap().bite, 1);
    let oil_body = loaded.area.items.get(oil).unwrap().body.unwrap();
    assert!(!loaded.physics.rapier().bodies[oil_body].is_enabled());
    let mut invalid = saved.clone();
    invalid.inventory.slots[0] = invalid.inventory.slots[1].clone();
    assert!(invalid.validate(&r, &stamp).is_err());
    let mut invalid = saved.clone();
    invalid.inventory.equipped = invalid.player.carry.as_ref().map(|c| c.item.clone());
    assert!(invalid.validate(&r, &stamp).is_err());
    let mut invalid = saved.clone();
    invalid.schema_version = 1;
    assert!(invalid.validate(&r, &stamp).is_err());
    loaded.started = true;
    loaded.paused = false;
    loaded.step(&r, FrameInput::default());
    assert!(!loaded.physics.rapier().bodies[oil_body].is_enabled());
    assert_eq!(
        loaded
            .area
            .items
            .get(oil)
            .unwrap()
            .fluid
            .as_ref()
            .unwrap()
            .mixture()
            .quantities_liters,
        game.area
            .items
            .get(oil)
            .unwrap()
            .fluid
            .as_ref()
            .unwrap()
            .mixture()
            .quantities_liters
    );
    // Stored food retains its bite, and exhausted food does not return on load.
    loaded
        .area
        .interaction
        .drop_held(&mut loaded.area.items, &mut loaded.physics);
    loaded.area.sync_inventory();
    loaded
        .area
        .apply_inventory_command(InventoryCommand::Select(0), &r, &mut loaded.physics)
        .unwrap();
    loaded
        .area
        .apply_inventory_command(InventoryCommand::Select(2), &r, &mut loaded.physics)
        .unwrap();
    let point = loaded.area.items.get(food).unwrap().pose.position;
    loaded
        .area
        .interaction
        .pickup_item(
            food,
            point,
            &loaded.area.player,
            &mut loaded.area.items,
            &mut loaded.physics,
        )
        .unwrap();
    loaded.area.sync_inventory();
    loaded
        .area
        .apply_inventory_command(InventoryCommand::Select(3), &r, &mut loaded.physics)
        .unwrap();
    let stored = loaded
        .capture_save(&r, &stamp, "2026-10-15T12:00:01Z")
        .unwrap();
    let mut restored = GarageRuntime::from_save(
        &r,
        &stamp,
        &stored,
        crate::world::GameMode::default(),
        r.world.draw_distance.default,
    )
    .unwrap();
    prepare_ready(&mut restored, &r);
    assert!(restored.advance_saved_restore(&r).unwrap());
    assert_eq!(restored.area.items.get(food).unwrap().food.unwrap().bite, 1);
    restored
        .area
        .apply_inventory_command(InventoryCommand::Select(2), &r, &mut restored.physics)
        .unwrap();
    let bites = r.survival.foods[restored
        .area
        .items
        .get(food)
        .unwrap()
        .food
        .unwrap()
        .prototype_index]
        .bites;
    for _ in 1..bites {
        assert!(restored.consume(&r, food).unwrap());
    }
    assert!(!restored.area.inventory.slots.contains(&Some(food)));
    let removed = restored
        .capture_save(&r, &stamp, "2026-10-15T12:00:02Z")
        .unwrap();
    let restored = GarageRuntime::from_save(
        &r,
        &stamp,
        &removed,
        crate::world::GameMode::default(),
        r.world.draw_distance.default,
    )
    .unwrap();
    assert!(restored.area.items.get(food).unwrap().removed);
}

fn g08_assert_equipment_pose(
    run: &GarageRuntime,
    reference: &ReferenceManifest,
    id: crate::reference::ItemId,
) {
    use crate::vehicle::{VehiclePose, to_rapier};
    let item = run.area.items.get(id).unwrap();
    let rule = run.area.interaction.pickup_rule(id).unwrap();
    let hand = to_rapier(
        run.area
            .pose
            .anchor(reference.clothing.pose.anchors["RH"].node)
            .unwrap(),
    );
    let offset = if rule.disable_rh_fix {
        rapier3d::math::Pose::IDENTITY
    } else {
        to_rapier(VehiclePose {
            position: rule.right_hand_position,
            rotation: rule.right_hand_rotation,
        })
    };
    let expected = hand * offset * to_rapier(item.local_pose).inverse();
    let body = &run.physics.rapier().bodies[item.body.unwrap()];
    assert!(
        (body.position().translation - expected.translation).length() < 0.0001,
        "completed hand position"
    );
    assert!(
        body.position().rotation.dot(expected.rotation).abs() > 0.99999,
        "completed hand rotation"
    );
    assert!(
        (to_rapier(item.pose).translation - (hand * offset).translation).length() < 0.0001,
        "published item hand position"
    );
    assert!(body.is_kinematic());
    assert!(body.is_enabled());
    assert!(
        body.colliders()
            .iter()
            .all(|handle| !run.physics.rapier().colliders[*handle].is_enabled())
    );
}

#[test]
#[ignore = "requires the installed source export"]
fn g08_save_independent_equipment_and_both_owners() {
    use crate::{
        interaction::items::ItemLocation,
        inventory::InventoryCommand,
        save::{InstanceId, ItemLocationSave},
        simulation::{FrameInput, keys},
    };
    let reference = reference();
    let stamp = roadside_stamp(&reference);
    let mut run = g08_ready(&reference);
    let oil = reference
        .items
        .iter()
        .find(|i| i.name == "OilCan")
        .unwrap()
        .id;
    let tool = reference.conditions.sprays[0].item;
    let carry = reference
        .flashlights
        .instances
        .iter()
        .find(|f| f.startup_active)
        .unwrap()
        .item;
    g08_pickup(&mut run, oil);
    assert!(
        run.area
            .apply_inventory_command(InventoryCommand::Select(1), &reference, &mut run.physics)
            .unwrap()
    );
    g08_pickup(&mut run, tool);
    assert!(run.area.equip_carried(&mut run.physics).unwrap());
    // Slot membership is optional for equipment. Pickup registers a different carry.
    run.area.inventory.forget(tool);
    g08_pickup(&mut run, carry);
    let initial_quantity = run
        .area
        .interaction
        .cleaning
        .get(tool)
        .unwrap()
        .quantity
        .unwrap();
    g08_frame(
        &mut run,
        &reference,
        FrameInput {
            held: keys::PRIMARY,
            ..Default::default()
        },
        0.04,
    );
    g08_frame(
        &mut run,
        &reference,
        FrameInput {
            held: keys::PRIMARY,
            ..Default::default()
        },
        0.04,
    );
    let quantity = run
        .area
        .interaction
        .cleaning
        .get(tool)
        .unwrap()
        .quantity
        .unwrap();
    assert!(quantity < initial_quantity);
    let saved = run
        .capture_save(&reference, &stamp, "2026-10-20T12:00:00Z")
        .unwrap();
    let tool_instance = InstanceId::initial(
        &reference
            .items
            .iter()
            .find(|i| i.id == tool)
            .unwrap()
            .source,
    );
    let carry_instance = saved.player.carry.as_ref().unwrap().item.clone();
    assert_eq!(saved.inventory.equipped.as_ref(), Some(&tool_instance));
    assert_eq!(
        saved.inventory.slots[saved.inventory.selected as usize].as_ref(),
        Some(&carry_instance)
    );
    assert!(!saved.inventory.slots.contains(&Some(tool_instance.clone())));
    let members: std::collections::HashSet<_> = saved.inventory.slots.iter().flatten().collect();
    assert_eq!(
        members.len(),
        saved.inventory.slots.iter().flatten().count()
    );
    let mut loaded = GarageRuntime::from_save(
        &reference,
        &stamp,
        &saved,
        crate::world::GameMode::default(),
        reference.world.draw_distance.default,
    )
    .unwrap();
    assert!(loaded.restore_pending());
    prepare_ready(&mut loaded, &reference);
    assert!(loaded.stream.required_ready());
    assert!(loaded.advance_saved_restore(&reference).unwrap());
    assert!(!loaded.restore_pending());
    assert_eq!(loaded.area.interaction.equipment.item(), Some(tool));
    assert_eq!(loaded.area.interaction.held(), Some(carry));
    assert_eq!(loaded.area.inventory.slots, run.area.inventory.slots);
    assert_eq!(
        loaded.area.items.get(tool).unwrap().location,
        ItemLocation::Equipped
    );
    assert_eq!(
        loaded.area.items.get(carry).unwrap().location,
        ItemLocation::Held
    );
    assert_eq!(
        loaded.area.items.get(oil).unwrap().location,
        ItemLocation::Stored(0)
    );
    assert!(
        !loaded.physics.rapier().bodies[loaded.area.items.get(oil).unwrap().body.unwrap()]
            .is_enabled()
    );
    assert_eq!(
        loaded.area.interaction.cleaning.get(tool).unwrap().quantity,
        Some(quantity)
    );
    assert_eq!(
        loaded
            .area
            .items
            .get(oil)
            .unwrap()
            .fluid
            .as_ref()
            .unwrap()
            .mixture()
            .quantities_liters,
        run.area
            .items
            .get(oil)
            .unwrap()
            .fluid
            .as_ref()
            .unwrap()
            .mixture()
            .quantities_liters
    );
    g08_assert_equipment_pose(&loaded, &reference, tool);
    let recaptured = loaded
        .capture_save(&reference, &stamp, "2026-10-20T12:00:01Z")
        .unwrap();
    assert_eq!(recaptured.inventory, saved.inventory);
    assert_eq!(
        recaptured.player.carry.as_ref().unwrap().item,
        carry_instance
    );
    assert_eq!(recaptured.conditions.tools, saved.conditions.tools);
    for id in [oil, tool, carry] {
        let instance =
            InstanceId::initial(&reference.items.iter().find(|i| i.id == id).unwrap().source);
        let before = saved.items.iter().find(|i| i.instance == instance).unwrap();
        let after = recaptured
            .items
            .iter()
            .find(|i| i.instance == instance)
            .unwrap();
        assert_eq!(before.instance, after.instance);
        assert_eq!(before.location, after.location);
        assert_eq!(before.fluids, after.fluids);
    }
    let mut invalid = saved.clone();
    invalid.inventory.equipped = Some(carry_instance.clone());
    assert!(
        invalid.validate(&reference, &stamp).is_err(),
        "shared carry and equipment"
    );
    let mut invalid = saved.clone();
    invalid.inventory.slots[2] = invalid.inventory.slots[0].clone();
    assert!(
        invalid.validate(&reference, &stamp).is_err(),
        "duplicate membership"
    );
    let mut invalid = saved.clone();
    invalid
        .items
        .iter_mut()
        .find(|i| i.instance == tool_instance)
        .unwrap()
        .location = ItemLocationSave::Attached {
        owner: carry_instance,
    };
    assert!(
        invalid.validate(&reference, &stamp).is_err(),
        "attached equipment"
    );
    let mut invalid = saved.clone();
    invalid
        .items
        .iter_mut()
        .find(|i| i.instance == tool_instance)
        .unwrap()
        .removed = true;
    assert!(
        invalid.validate(&reference, &stamp).is_err(),
        "removed equipment"
    );
    let mut invalid = saved.clone();
    invalid
        .items
        .iter_mut()
        .find(|i| i.instance == tool_instance)
        .unwrap()
        .location = ItemLocationSave::World;
    let oil_instance =
        InstanceId::initial(&reference.items.iter().find(|i| i.id == oil).unwrap().source);
    invalid.inventory.slots[0] = None;
    invalid.inventory.equipped = Some(oil_instance.clone());
    invalid
        .items
        .iter_mut()
        .find(|i| i.instance == oil_instance)
        .unwrap()
        .location = ItemLocationSave::Equipped;
    assert!(
        invalid.validate(&reference, &stamp).is_err(),
        "ineligible equipment"
    );

    // Save a pressed Tab while the tool is carried. Its old release cannot equip after load.
    loaded
        .area
        .interaction
        .drop_held(&mut loaded.area.items, &mut loaded.physics);
    loaded.area.sync_inventory();
    assert!(loaded.area.release_equipped(&mut loaded.physics).unwrap());
    loaded.paused = false;
    let down = FrameInput {
        pressed: keys::RECENTER,
        held: keys::RECENTER,
        ..Default::default()
    };
    let up = FrameInput {
        released: keys::RECENTER,
        ..Default::default()
    };
    g08_frame(&mut loaded, &reference, down, 0.02);
    assert_eq!(loaded.area.interaction.held(), Some(tool));
    let pressed_save = loaded
        .capture_save(&reference, &stamp, "2026-10-20T12:00:02Z")
        .unwrap();
    let mut after_load = GarageRuntime::from_save(
        &reference,
        &stamp,
        &pressed_save,
        crate::world::GameMode::default(),
        reference.world.draw_distance.default,
    )
    .unwrap();
    prepare_ready(&mut after_load, &reference);
    assert!(after_load.advance_saved_restore(&reference).unwrap());
    after_load.paused = false;
    g08_frame(&mut after_load, &reference, up, 0.02);
    assert_eq!(after_load.area.interaction.held(), Some(tool));
    assert_eq!(after_load.area.interaction.equipment.item(), None);
    g08_frame(&mut after_load, &reference, down, 0.02);
    g08_frame(&mut after_load, &reference, up, 0.02);
    assert_eq!(after_load.area.interaction.equipment.item(), Some(tool));
}
#[test]
#[ignore = "requires the installed source export"]
fn g08_app_input_after_recovery_retains_fresh_edges() {
    use crate::simulation::{FrameInput, keys};
    let reference = reference();
    let mut run = g08_ready(&reference);
    let tool = reference.conditions.sprays[0].item;
    g08_pickup(&mut run, tool);
    assert!(run.area.equip_carried(&mut run.physics).unwrap());
    run.recover(&reference);
    assert!(run.recovering);
    for _ in 0..10 {
        prepare_ready(&mut run, &reference);
        run.sync_world_activity(&reference).unwrap();
        if run.advance_recovery().unwrap() {
            break;
        }
    }
    assert!(!run.recovering, "real recovery must complete");
    assert!(run.world_supported());
    assert_eq!(run.area.interaction.equipment.item(), Some(tool));
    // sync_world clears both buffers after physics and arms the next time skip.
    run.clear_input();
    let mut pending = FrameInput::default();
    let mut skip = true;
    pending.merge(FrameInput {
        pressed: keys::RECENTER | keys::USE,
        held: keys::RECENTER,
        mouse: [0.2, 0.3],
        ..Default::default()
    });
    assert!(pending.take_frame(&mut skip, false).is_none());
    assert!(!skip);
    assert_eq!(run.area.interaction.equipment.item(), Some(tool));
    pending.merge(FrameInput::default());
    let raw = pending.take_frame(&mut skip, false).unwrap();
    let result = g08_frame(&mut run, &reference, raw, 0.02);
    assert_eq!(
        run.area.interaction.held(),
        Some(tool),
        "fresh Tab press survives the recovery time skip"
    );
    assert_eq!(run.area.interaction.equipment.item(), None);
    assert_eq!(pending, FrameInput::default());
    let mut fixed = FrameInput::default();
    result.merge_fixed_input(&mut fixed);
    let first = fixed.take_step();
    assert_eq!(first.pressed & keys::USE, keys::USE);
    assert_eq!(first.mouse, [0.2, 0.3]);
    assert_eq!(fixed.take_step().pressed, 0);
    assert_eq!(fixed.take_step().mouse, [0.; 2]);
    pending.merge(FrameInput {
        released: keys::RECENTER,
        ..Default::default()
    });
    g08_frame(
        &mut run,
        &reference,
        pending.take_frame(&mut skip, false).unwrap(),
        0.02,
    );
    assert_eq!(
        run.area.interaction.held(),
        Some(tool),
        "the following Tab up cannot equip the released item"
    );
    run.clear_input();
    skip = true;
    pending.merge(FrameInput {
        inventory_released: 2,
        ..Default::default()
    });
    assert!(pending.take_frame(&mut skip, false).is_none());
    assert_eq!(run.area.inventory.selected, 0);
    pending.merge(FrameInput::default());
    g08_frame(
        &mut run,
        &reference,
        pending.take_frame(&mut skip, false).unwrap(),
        0.02,
    );
    assert_eq!(
        run.area.inventory.selected, 1,
        "fresh number release survives the time skip"
    );
    assert_eq!(pending, FrameInput::default());
}

#[test]
#[ignore = "requires the installed source export"]
fn g08_app_input_skip_consumes_tools_and_particles_once() {
    use crate::simulation::{FrameInput, keys};
    let reference = reference();
    let sample = |skip_first: bool| {
        let mut run = g08_ready(&reference);
        let light = reference
            .flashlights
            .instances
            .iter()
            .find(|l| l.startup_active)
            .unwrap()
            .item;
        g08_pickup(&mut run, light);
        assert!(run.area.equip_carried(&mut run.physics).unwrap());
        let control = run.area.interaction.owned_light_control(light).unwrap();
        let index = reference
            .lighting
            .controllers
            .iter()
            .position(|c| c.id == control)
            .unwrap();
        let on = run.lighting.controllers[index].on;
        let spray = reference.conditions.sprays[0].item;
        g08_pickup(&mut run, spray);
        let quantity = run
            .area
            .interaction
            .cleaning
            .get(spray)
            .unwrap()
            .quantity
            .unwrap();
        let mut pending = FrameInput::default();
        let mut skip = skip_first;
        pending.merge(FrameInput {
            pressed: keys::PRIMARY | keys::USE,
            held: keys::PRIMARY,
            mouse: [0.2, 0.3],
            ..Default::default()
        });
        if skip_first {
            assert!(pending.take_frame(&mut skip, false).is_none());
            assert_eq!(run.lighting.controllers[index].on, on);
            assert_eq!(
                run.area
                    .interaction
                    .cleaning
                    .get(spray)
                    .unwrap()
                    .quantity
                    .unwrap(),
                quantity
            );
            assert!(
                run.area
                    .interaction
                    .cleaning
                    .particles(reference.conditions.sprays[0].item, 0)
                    .is_empty()
            );
            pending.merge(FrameInput {
                held: keys::PRIMARY,
                ..Default::default()
            });
        }
        let result = g08_frame(
            &mut run,
            &reference,
            pending.take_frame(&mut skip, false).unwrap(),
            0.04,
        );
        assert_eq!(run.lighting.controllers[index].on, !on);
        assert!(result.primary_reserved);
        let after_quantity = run
            .area
            .interaction
            .cleaning
            .get(spray)
            .unwrap()
            .quantity
            .unwrap();
        assert!(after_quantity < quantity);
        let ages: Vec<_> = run
            .area
            .interaction
            .cleaning
            .particles(reference.conditions.sprays[0].item, 0)
            .iter()
            .map(|p| p.age)
            .collect();
        assert!(!ages.is_empty());
        assert_eq!(pending, FrameInput::default());
        let mut fixed = FrameInput::default();
        result.merge_fixed_input(&mut fixed);
        let input = fixed.take_step();
        assert_eq!(input.pressed & keys::PRIMARY, 0);
        assert_eq!(input.pressed & keys::USE, keys::USE);
        assert_eq!(input.mouse, [0.2, 0.3]);
        run.step(&reference, input);
        run.step(&reference, fixed.take_step());
        assert_eq!(run.lighting.controllers[index].on, !on);
        assert_eq!(
            run.area
                .interaction
                .cleaning
                .get(spray)
                .unwrap()
                .quantity
                .unwrap(),
            after_quantity
        );
        assert_eq!(
            run.area
                .interaction
                .cleaning
                .particles(reference.conditions.sprays[0].item, 0)
                .iter()
                .map(|p| p.age)
                .collect::<Vec<_>>(),
            ages
        );
        assert_eq!(fixed.take_step().pressed, 0);
        assert_eq!(fixed.take_step().mouse, [0.; 2]);
        let result = g08_frame(
            &mut run,
            &reference,
            pending.take_frame(&mut skip, false).unwrap(),
            0.01,
        );
        assert_eq!(result.fixed_input.pressed, 0);
        assert_eq!(result.fixed_input.mouse, [0.; 2]);
        assert_eq!(run.lighting.controllers[index].on, !on);
        assert_eq!(
            run.area
                .interaction
                .cleaning
                .get(spray)
                .unwrap()
                .quantity
                .unwrap(),
            after_quantity
        );
        (quantity - after_quantity, ages)
    };
    assert_eq!(sample(true), sample(false));
}

#[test]
#[ignore = "requires the installed source export"]
fn g08_app_input_blocked_skip_cancels_stale_edges() {
    use crate::simulation::{FrameInput, keys};
    let reference = reference();
    let mut run = g08_ready(&reference);
    let tool = reference.conditions.sprays[0].item;
    g08_pickup(&mut run, tool);
    g08_frame(
        &mut run,
        &reference,
        FrameInput {
            pressed: keys::RECENTER,
            held: keys::RECENTER,
            ..Default::default()
        },
        0.02,
    );
    let mut pending = FrameInput {
        released: keys::RECENTER,
        pressed: keys::USE,
        inventory_released: 2,
        mouse: [0.2, 0.3],
        wheel: 1.,
        ..Default::default()
    };
    let mut skip = true;
    run.paused = true;
    assert!(pending.take_frame(&mut skip, true).is_none());
    assert_eq!(pending, FrameInput::default());
    run.clear_input();
    run.paused = false;
    pending.merge(FrameInput {
        released: keys::RECENTER,
        ..Default::default()
    });
    let result = g08_frame(
        &mut run,
        &reference,
        pending.take_frame(&mut skip, false).unwrap(),
        0.1,
    );
    assert_eq!(run.area.interaction.held(), Some(tool));
    assert_eq!(
        run.area.interaction.equipment.item(),
        None,
        "canceled Tab press cannot equip on stale release"
    );
    assert_eq!(run.area.inventory.selected, 0);
    assert_eq!(result.fixed_input.pressed, 0);
    assert_eq!(result.fixed_input.mouse, [0.; 2]);
    assert_eq!(result.fixed_input.wheel, 0.);
    // A normal blocked frame still reaches the coordinator's death-look path.
    run.survival.damage_instant(&reference.survival, 10000.);
    pending.merge(FrameInput {
        mouse: [0.4, 0.5],
        ..Default::default()
    });
    let result = g08_frame(
        &mut run,
        &reference,
        pending.take_frame(&mut skip, true).unwrap(),
        0.02,
    );
    assert!(result.actions_blocked);
    assert_eq!(result.fixed_input.mouse, [0.4, 0.5]);
    assert_eq!(pending, FrameInput::default());
}

#[test]
#[ignore = "requires the installed source export"]
fn trigger_roles_follow_publish_and_retire() {
    let reference = reference();
    let mut prototype = reference
        .world
        .prototypes
        .iter()
        .find(|p| {
            p.colliders
                .iter()
                .any(|c| c.trigger && c.geometry.is_some())
        })
        .unwrap()
        .clone();
    let collider = prototype
        .colliders
        .iter_mut()
        .find(|c| c.trigger && c.geometry.is_some())
        .unwrap();
    collider.role = Some(crate::reference::TriggerRole::Ladder {
        speed: 1.,
        up: [0., 1., 0.],
    });
    let body = collider.body;
    let body = body.map(|b| prototype.bodies[b as usize].pose.rotation);
    let recipe = crate::world::StructureRecipe::new(&reference.world, &prototype).unwrap();
    let mut run = GarageRuntime::new_default(&reference).unwrap();
    let mut roles = crate::world::TriggerRoles::default();
    let mut placement = replay_placement(&prototype.source, 9823, [50.; 3]);
    placement.scale = prototype.prefab_pose.scale;
    placement.pose.rotation = rapier3d::math::Rotation::from_rotation_y(1.).to_array();
    let instance = recipe
        .publish(
            placement.clone(),
            WorldOrigin::default(),
            false,
            &mut run.physics,
            &mut roles,
        )
        .unwrap();
    assert_eq!(roles.len(), 1);
    let (handle, collider) = instance
        .colliders
        .iter()
        .map(|(_, h)| (*h, &run.physics.rapier().colliders[*h]))
        .find(|(_, c)| c.is_sensor())
        .unwrap();
    let Some(crate::reference::TriggerRole::Ladder { up, .. }) = roles.get(handle, collider) else {
        panic!("ladder role");
    };
    let frame = rapier3d::math::Rotation::from_array(placement.pose.rotation)
        * body.map_or(
            rapier3d::math::Rotation::IDENTITY,
            rapier3d::math::Rotation::from_array,
        );
    let expected = frame * rapier3d::math::Vector::Y;
    assert!((rapier3d::math::Vector::from_array(up) - expected).length() < 1e-4);
    instance.retire(&mut run.physics, &mut roles);
    assert!(roles.is_empty());
}

#[test]
#[ignore = "requires the installed source export"]
fn hazard_trigger_damages_player_until_death() {
    let reference = reference();
    let mut run = GarageRuntime::new_default(&reference).unwrap();
    let position = run.area.player.body_pose(&run.physics).position;
    let handle = run.physics.insert_collider(
        rapier3d::prelude::ColliderBuilder::cuboid(5., 5., 5.)
            .translation(rapier3d::math::Vector::from_array(position))
            .sensor(true)
            .build(),
    );
    run.physics.refresh_queries();
    let damage = 50.;
    run.stream.trigger_roles.insert(
        handle,
        crate::reference::TriggerRole::Hazard {
            damage_player: damage,
            damage_breakable: 0.,
        },
    );
    let steps = (run.needs_output().needs.hp / (damage * crate::simulation::FIXED_DT)) as usize;
    for _ in 0..steps / 2 {
        run.step(&reference, crate::simulation::FrameInput::default());
    }
    assert!(!run.needs_output().dead);
    for _ in 0..steps {
        run.step(&reference, crate::simulation::FrameInput::default());
    }
    assert!(run.needs_output().dead);
}
