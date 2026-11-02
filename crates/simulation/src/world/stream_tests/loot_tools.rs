//! X11: loot brushes, sponges and spray paint bind as tools and keep their state after save
//! and load.
use super::*;
use crate::reference::PrototypeTool;
use crate::world::ObjectKey;

fn place(
    run: &mut GarageRuntime,
    reference: &ReferenceManifest,
    source: &SourceId,
) -> (ObjectKey, crate::reference::ItemId) {
    let mut placement = replay_placement(source, 820, [2., 20., 3.]);
    placement.scale = reference
        .world
        .prototypes
        .iter()
        .find(|p| &p.source == source)
        .unwrap()
        .prefab_pose
        .scale;
    let key = ObjectKey::Generated(placement.id.clone());
    run.stream.protect_saved_items(std::slice::from_ref(&key));
    publish_decoration_fixture(run, placement);
    sync_decoration_fixture(run, reference);
    let id = run.area.items.resolve_key(&key).unwrap();
    (key, id)
}

#[test]
#[ignore = "requires the installed source export"]
fn loot_tools_bind_and_keep_state_after_save_reload() {
    let reference = reference();
    assert_eq!(reference.conditions.prototype_tools.len(), 3);
    for tool in &reference.conditions.prototype_tools {
        let mut run = GarageRuntime::new_default(&reference).unwrap();
        let (key, id) = place(&mut run, &reference, &tool.prototype);
        let cleaning = &mut run.area.interaction.cleaning;
        assert!(cleaning.signal(id).is_some(), "loot tool binds");
        let state = cleaning.get_mut(id).unwrap();
        match tool.kind {
            PrototypeTool::Spray { .. } => state.quantity = Some(0.25),
            PrototypeTool::Brush { .. } => state.disabled = true,
        }
        let expected = (state.quantity, state.disabled);
        run.area.items.mark_dirty(id);
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
        let saved: crate::save::WorldSnapshot =
            serde_json::from_slice(&serde_json::to_vec(&saved).unwrap()).unwrap();
        let change = saved.changes.get(&key).expect("loot tool change saves");
        let snapshot = change
            .snapshot
            .as_ref()
            .and_then(|s| s.tool)
            .expect("changed tool state saves");
        assert_eq!((snapshot.quantity, snapshot.disabled), expected);

        let mut run = GarageRuntime::new_default(&reference).unwrap();
        run.stream = WorldStream::new(
            &reference,
            saved.seed,
            saved.date,
            crate::world::GameMode::default(),
            run.distances,
        )
        .unwrap();
        run.stream.restore_save(&saved).unwrap();
        let (_, id) = place(&mut run, &reference, &tool.prototype);
        let state = run
            .area
            .interaction
            .cleaning
            .get(id)
            .expect("loot tool binds");
        assert_eq!((state.quantity, state.disabled), expected);
    }
}
