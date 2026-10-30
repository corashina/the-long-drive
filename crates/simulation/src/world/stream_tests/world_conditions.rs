//! X05: world part conditions repair and paint live, like garage surfaces, and stay after save
//! and load.
use super::drive_out::{frame, settle};
use super::*;
use crate::world::ObjectKey;

#[test]
#[ignore = "requires the installed source export"]
fn lada_part_paint_and_repair_change_its_appearance_and_save() {
    let reference = reference();
    let mut run = GarageRuntime::new_default(&reference).unwrap();
    let mut lada = None;
    for _ in 0..2000 {
        lada = run.vehicles.keys().find_map(|key| match key {
            ObjectKey::Generated(id) if id.layer == WorldLayer::Startup => Some(id.clone()),
            _ => None,
        });
        if lada.is_some() {
            break;
        }
        frame(&mut run, &reference);
    }
    let lada = lada.expect("start car spawns");
    let content = run
        .stream
        .contents
        .active
        .iter()
        .position(|c| c.content.placement.id == lada)
        .expect("start car content");
    let entries = run.stream.contents.active[content].conditions.clone();
    assert!(!entries.is_empty(), "start car has part conditions");
    let conditions = &mut run.area.interaction.conditions;
    // The scene start car starts worn: its body draws a state from 1 to 3.
    assert!(
        entries
            .iter()
            .any(|(id, ..)| (1..=3).contains(&conditions.get(*id).state)),
        "start car starts worn"
    );
    // Paint the first entry that spray paint can paint and that shows its color (state below 3).
    let &(painted, index, _) = entries
        .iter()
        .find(|(id, ..)| conditions.can_repair(*id, 0) && conditions.get(*id).state < 3)
        .expect("a paintable start car part");
    let color = [0.1, 0.6, 0.3, 1.];
    assert!(conditions.paint(painted, color));
    // Repair the first entry that the sponge or the wire brush can improve.
    let repaired = entries.iter().find_map(|&(id, ..)| {
        let before = conditions.get(id).state;
        (before > 0 && (conditions.can_repair(id, 2) || conditions.can_repair(id, 1)))
            .then_some((id, before))
    });
    if let Some((id, before)) = repaired.filter(|(id, _)| *id != painted) {
        let tool = if conditions.can_repair(id, 2) { 2 } else { 1 };
        assert_eq!(
            conditions.repair(id, tool, 1.),
            crate::conditions::RepairResult::Improved
        );
        assert_eq!(conditions.get(id).state, before - 1);
    }
    frame(&mut run, &reference);
    let content = &run.stream.contents.active[content];
    let source = &content.content.inputs.conditions[index].source;
    let renderers = run
        .stream
        .contents
        .rules
        .get(&run.stream.reference, source)
        .unwrap()
        .fields;
    let renderers = crate::world::spawn_rules::array(renderers, "renderers").unwrap();
    if !renderers.is_empty() {
        assert!(
            content
                .content
                .appearance
                .iter()
                .any(|a| a.color == Some(color)),
            "the painted color reaches the appearance"
        );
    }
    let owner = run
        .area
        .interaction
        .conditions
        .world_owner(painted)
        .expect("an item saves the painted part");
    let key = run.area.items.object_key(owner);
    assert!(
        run.stream
            .changes
            .get(&key)
            .and_then(|c| c.snapshot.as_ref())
            .is_some_and(|s| s.conditions.iter().any(|c| c.color == color)),
        "the paint is a world change"
    );

    // Save and load.
    let stamp = roadside_stamp(&reference);
    let saved = run
        .capture_save(&reference, &stamp, "2026-10-29T12:00:00Z")
        .unwrap();
    let saved: crate::save::SaveSnapshot =
        serde_json::from_slice(&serde_json::to_vec(&saved).unwrap()).unwrap();
    let mut loaded = GarageRuntime::from_save(
        &reference,
        &stamp,
        &saved,
        crate::world::GameMode::default(),
        reference.world.draw_distance.default,
    )
    .unwrap();
    for _ in 0..2000 {
        prepare_ready(&mut loaded, &reference);
        if loaded.advance_saved_restore(&reference).unwrap() && !loaded.restore_pending() {
            break;
        }
    }
    assert!(!loaded.restore_pending());
    loaded.started = true;
    loaded.paused = false;
    settle(&mut loaded, &reference, 1.);
    let content = loaded
        .stream
        .contents
        .active
        .iter()
        .find(|c| c.content.placement.id == lada)
        .expect("start car content after load");
    let &(entry, ..) = content
        .conditions
        .iter()
        .find(|(_, i, _)| *i == index)
        .expect("painted part after load");
    assert_eq!(loaded.area.interaction.conditions.get(entry).color, color);
}
