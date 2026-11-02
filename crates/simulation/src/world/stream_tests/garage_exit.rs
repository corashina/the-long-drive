//! The seated start car driver gets out in the garage.
use super::kit::*;
use super::*;
use crate::simulation::{FrameInput, keys};

#[test]
#[ignore = "requires the installed source export"]
fn start_car_driver_exits_in_garage() {
    let (reference, mut run) = load_garage();
    seconds(&mut run, &reference, 3.);
    act(&mut run, &reference, r#"{"action":"seat","seat":"driver"}"#);
    assert!(run.area.player.seated());
    seconds(&mut run, &reference, 2.);
    let mut exits = Vec::new();
    for step in 0..24 {
        let yaw = step as f32 * 15.;
        look(&mut run, yaw, 0.);
        frame(&mut run, &reference);
        let player = &run.area.player;
        if player.can_exit(&run.physics) {
            exits.push(yaw);
        } else if step % 6 == 0 {
            eprintln!("yaw {yaw}:\n{}", player.exit_trace(&run.physics));
        }
    }
    eprintln!("exits at yaw {exits:?} with the doors closed");
    exits.extend(open_door_and_look_out(&mut run, &reference));
    eprintln!("after door: {}", run.area.player.exit_trace(&run.physics));
    assert!(!exits.is_empty(), "no clear exit");
    act(&mut run, &reference, r#"{"action":"stand"}"#);
    assert!(!run.area.player.seated());
}

/// Source CanGetOut casts toward the view on useLayer. A closed door stops the search.
/// Doors open with an LMB drag. A half open door blocks a straight side look, so the
/// driver also looks a little back and forth. Returns the yaw of the first clear exit.
pub(crate) fn open_door_and_look_out(
    run: &mut GarageRuntime,
    reference: &ReferenceManifest,
) -> Option<f32> {
    for pitch in [0., -30.] {
        for mouse in [[5., 0.], [-5., 0.], [0., 5.], [0., -5.]] {
            look(run, 270., pitch);
            frame(run, reference);
            for index in 0..40 {
                frame_with(
                    run,
                    reference,
                    FrameInput {
                        held: keys::PRIMARY,
                        pressed: if index == 0 { keys::PRIMARY } else { 0 },
                        mouse,
                        ..FrameInput::default()
                    },
                );
            }
            frame_with(
                run,
                reference,
                FrameInput {
                    released: keys::PRIMARY,
                    ..FrameInput::default()
                },
            );
            for yaw in [270., 255., 285., 240., 300.] {
                look(run, yaw, 0.);
                frame(run, reference);
                if run.area.player.can_exit(&run.physics) {
                    eprintln!(
                        "exit clear at yaw {yaw} after a drag at pitch {pitch} with mouse {mouse:?}"
                    );
                    return Some(yaw);
                }
            }
        }
    }
    None
}
