//! Native adapter for the same renderer receipts and publication boundary used by WASM.
#[path = "stream_tests/kit.rs"]
pub mod kit;

/// The renderer finishes only a surface that it reserved. The finish keeps the stale fact.
#[test]
fn a_surface_finishes_only_after_its_reservation() {
    use super::SurfaceState;
    assert_eq!(
        SurfaceState::Prepared.accept().unwrap_err().0,
        "Unreserved source renderer"
    );
    let mut state = SurfaceState::Reserved { stale: true };
    state.accept().unwrap();
    assert_eq!(state, SurfaceState::RendererReady { stale: true });
}

/// The stream drops a structure while the renderer finishes it. The late receipt ends its claim
/// and does not stop the game.
#[test]
#[ignore = "requires the installed source export"]
fn a_receipt_for_a_dropped_structure_ends_its_claim() {
    let (_reference, mut run) = kit::outdoors();
    let stream = &mut run.stream;
    let placement = stream
        .wanted_structures
        .iter()
        .find(|p| !stream.structures.iter().any(|s| s.placement.id == p.id))
        .cloned()
        .expect("a structure the renderer has not finished");
    stream.wanted_structures.retain(|p| p.id != placement.id);
    let mut facts = crate::world::RendererFacts::default();
    facts.push(crate::world::RendererFact::AcceptStructure(
        stream.request,
        placement.id.clone(),
    ));
    stream.apply_renderer_facts(&mut facts).unwrap();
    assert!(!stream.ready_structures.contains(&placement.id));
}

/// A request change and a changed terrain context keep a finished tile when its input is the
/// same. Before, every request change dropped it, so near tiles drew late while driving.
#[test]
#[ignore = "requires the installed source export"]
fn a_request_change_keeps_a_finished_terrain_tile() {
    let (_reference, mut run) = kit::outdoors();
    let stream = &mut run.stream;
    let car = stream.request.vehicle;
    let point = [car[0] + 400., car[1], car[2] + 400.];
    stream.request_at(point, point, point);
    // The tile under the new point builds first.
    let key = (0..20_000)
        .find_map(|_| {
            stream.prepare(100.);
            stream
                .ready
                .as_ref()
                .filter(|w| w.published.key.layer == crate::world::WorldLayer::Terrain)
                .map(|w| w.published.key)
        })
        .expect("a finished near tile");
    stream.terrain_context_dirty = true;
    stream.revise_selection();
    for _ in 0..10 {
        stream.prepare(100.);
    }
    assert_eq!(
        stream.ready.as_ref().map(|w| w.published.key),
        Some(key),
        "the tile stays ready"
    );
    let (request, _) = stream
        .prepared_surface()
        .expect("the renderer still sees the tile");
    stream.reserve_surface_renderer(request, key).unwrap();
    stream.accept_surface_renderer(request, key).unwrap();
    stream.prepare(100.);
    stream.publish(&mut run.physics, &[]).unwrap();
    assert!(stream.surfaces.iter().any(|s| s.key == key));
}

/// A bus stop 600 m away looks larger than a 1 m rock 120 m away, so it loads first.
/// A rock in the collision window of the player loads before both. The step waits for it.
#[test]
fn a_large_far_structure_loads_before_a_small_near_prop() {
    use super::{AbsolutePose, Placement, WorldBounds, WorldLayer, WorldRequest};
    use crate::reference::SourceId;
    use crate::world::GeneratedKey;
    let placement = |z: f64, radius: f64| {
        let source = SourceId {
            file: "test".into(),
            path_id: 0,
        };
        Placement {
            id: GeneratedKey {
                layer: WorldLayer::Building,
                generator: source.clone(),
                unit: [z as i64, 0],
                ordinal: 0,
                children: Vec::new(),
            }
            .into(),
            prototype: source,
            pose: AbsolutePose {
                position: [0., 0., z],
                rotation: [0., 0., 0., 1.],
            },
            scale: [1.; 3],
            bounds: WorldBounds {
                min: [-radius, -radius, z - radius],
                max: [radius, radius, z + radius],
            },
            object_seed: 0,
        }
    };
    let request = WorldRequest {
        session: 1,
        revision: 0,
        player: [0.; 3],
        vehicle: [0.; 3],
        ahead: [0.; 3],
    };
    let (rock, bus_stop, touched) = (
        placement(120., 1.),
        placement(600., 6.6),
        placement(5., 0.5),
    );
    let mut list = [&rock, &bus_stop, &touched];
    list.sort_by(|a, b| super::load_order(request, a, b));
    assert_eq!(list.map(|p| p.pose.position[2]), [5., 600., 120.]);
}
