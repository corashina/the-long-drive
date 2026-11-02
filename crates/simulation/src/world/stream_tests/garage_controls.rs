use crate::{
    reference::ReferenceManifest,
    simulation::{FrameInput, keys},
    starting_area::GarageRuntime,
};

use super::kit::*;

#[path = "garage_controls/reactions.rs"]
mod reactions;

fn press(run: &mut GarageRuntime, reference: &ReferenceManifest, held: u32) {
    run.step(
        reference,
        FrameInput {
            held,
            pressed: held,
            ..FrameInput::default()
        },
    );
}

#[test]
#[ignore = "requires the installed source export"]
fn x04b_autosave_safe_state() {
    let (reference, mut run) = load_garage();
    settle(&mut run, &reference);
    assert!(run.autosave_safe(false));
    assert!(!run.autosave_safe(true));

    press(&mut run, &reference, keys::JUMP);
    for _ in 0..30 {
        if !run.area.player.grounded() {
            break;
        }
        run.step(&reference, FrameInput::default());
    }
    assert!(!run.area.player.grounded());
    assert!(!run.autosave_safe(false));

    settle(&mut run, &reference);
    for _ in 0..2000 {
        if run.sleeping() {
            break;
        }
        press(&mut run, &reference, keys::SLEEP);
    }
    assert!(run.sleeping());
    assert!(!run.autosave_safe(false));

    let (reference, mut run) = load_garage();
    settle(&mut run, &reference);
    let tool = reference.conditions.brushes[0].item;
    let point = run.area.items.get(tool).unwrap().pose.position;
    run.area
        .interaction
        .pickup_item(
            tool,
            point,
            &run.area.player,
            &mut run.area.items,
            &mut run.physics,
        )
        .unwrap();
    assert!(!run.autosave_safe(false));
}
