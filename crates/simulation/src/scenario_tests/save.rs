//! Headless scenario tests: saves.
use crate::boundary::SimInit;
use crate::session::Session;
use crate::world::stream::tests::kit::*;
use std::sync::Arc;


/// Load and Load last reload the page, then `Session::new` starts from the stored save, as
/// the source `SceneManager.LoadScene` loads the save at scene start. The save opens without an
/// error and its restore finishes. A save after a death opens the same way.
#[test]
#[ignore = "requires the installed source export"]
fn a_stored_save_starts_the_session_and_its_restore_finishes() {
    let (reference, mut run) = load_garage();
    settle(&mut run, &reference);
    seconds(&mut run, &reference, 2.);
    let saved = save(&run, &reference);
    let stamp = roadside_stamp(&reference);
    let reference = Arc::new(reference);
    let init = SimInit {
        mode: Default::default(),
        options: Default::default(),
        draw_distance: reference.world.draw_distance.default,
        manifest_sha256: stamp.manifest_sha256.clone(),
        save: serde_json::to_vec(&saved).unwrap(),
    };
    let session = Session::new(reference.clone(), init).unwrap();
    assert_eq!(session.load_error(), None, "the stored save opens");
    drop(session);
    // The restore the session frames run.
    let mut loaded = load(&reference, &saved);
    seconds(&mut loaded, &reference, 1.);
    assert!(loaded.world_supported(), "the loaded world is ready");
    // After a death, the pause menu Load opens the same stored save.
    run.survival
        .damage_instant(&reference.survival, reference.survival.maximum.hp);
    frame(&mut run, &reference);
    assert!(run.needs_output().dead);
    let after = load(&reference, &saved);
    assert!(!after.needs_output().dead && after.world_supported());
}
