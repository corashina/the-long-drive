//! Native adapter for the same renderer receipts and publication boundary used by WASM.
#[path = "stream_tests/kit.rs"]
pub mod kit;

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
    stream.ledger.add_claim();
    let claims = stream.ledger.credit().pending;
    let mut facts = crate::world::RendererFacts::default();
    facts.push(crate::world::RendererFact::AcceptStructure(
        stream.request,
        placement.id.clone(),
    ));
    stream.apply_renderer_facts(&mut facts).unwrap();
    assert_eq!(stream.ledger.credit().pending, claims - 1);
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
                .filter(|w| w.published.key.layer == crate::world::WorldLayer::Terrain && w.offered)
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
