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
