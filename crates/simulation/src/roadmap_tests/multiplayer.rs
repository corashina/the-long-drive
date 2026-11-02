//! ROADMAP test pass: Multiplayer (MP rows).
use crate::boundary::SimFrame;
use crate::net::{Net, NetRole};
use crate::simulation::{FIXED_DT, FrameInput, keys};
use crate::starting_area::GarageRuntime;
use crate::world::stream::tests::kit::*;
use rapier3d::math::Vector;

fn body_position(run: &GarageRuntime) -> Vector {
    Vector::from_array(run.area.player.body_pose(&run.physics).position)
}

/// MP2. The host shares its save (`saveSession.share`), and the guest starts from it: the guest
/// stands where the host stood, and a seated host puts the guest in the same seat.
#[test]
#[ignore = "requires the installed source export"]
fn tp_mp2_the_guest_starts_where_the_host_stood_and_in_its_seat() {
    let (reference, mut run) = load_garage();
    settle(&mut run, &reference);
    let mut guest = save_load(&run, &reference);
    frame(&mut guest, &reference);
    // The loaded origin sits at the player: compare absolute positions.
    let absolute = |run: &GarageRuntime| {
        let p = run.stream.origin.absolute(body_position(run).to_array());
        Vector::new(p[0] as f32, p[1] as f32, p[2] as f32)
    };
    let (host_at, guest_at) = (absolute(&run), absolute(&guest));
    assert!(
        (guest_at - host_at).length() < 0.1,
        "the guest stands where the host stood: host {host_at:?}, guest {guest_at:?}"
    );
    act(&mut run, &reference, r#"{"action":"seat","seat":"driver"}"#);
    seconds(&mut run, &reference, 0.5);
    let seat = run.area.player.seat_vehicle().cloned();
    assert!(seat.is_some());
    let guest = save_load(&run, &reference);
    assert!(guest.area.player.seated(), "the guest sits");
    assert_eq!(
        guest.area.player.seat_vehicle().cloned(),
        seat,
        "in the host seat"
    );
}

/// MP3. Source `snTempPlayerSync` and `SPlayerAnim`: the host sends its pose, duck value and body
/// targets. The guest figure stands at the host, and ducks and looks up with it.
#[test]
#[ignore = "requires the installed source export"]
fn tp_mp3_the_other_figure_follows_walk_duck_and_look() {
    let (reference, mut run) = load_garage();
    settle(&mut run, &reference);
    let (mut host, mut guest) = (Net::default(), Net::default());
    host.set_session(NetRole::Host, 0);
    guest.set_session(NetRole::Guest, 1);
    let (mut sent, mut seen, mut packets) = (SimFrame::default(), SimFrame::default(), Vec::new());
    let mut sync = |run: &mut GarageRuntime, input: FrameInput, frames: usize| {
        for _ in 0..frames {
            let view = run.area.player.view(&run.physics).head_pose;
            let fixed = run
                .advance_inventory_frame(&reference, input, FIXED_DT, view)
                .fixed_input;
            frame_with(run, &reference, fixed);
            host.fill(&reference, run, FIXED_DT, &mut sent);
            host.take_outbox(&mut packets);
            guest.receive(&packets);
            guest.fill(&reference, run, FIXED_DT, &mut seen);
        }
        assert_eq!(seen.remotes.len(), 1, "one remote figure");
        let row = seen.remotes[0];
        (row, seen.remote_joints.clone())
    };
    let walk = FrameInput {
        held: keys::FORWARD,
        ..Default::default()
    };
    let (row, standing) = sync(&mut run, walk, 100);
    assert_eq!(row.slot, 0, "the host figure");
    let at = Vector::from_array(row.body_pose.position);
    assert!(
        (at - body_position(&run)).length() < 1.,
        "the figure walks with the host"
    );
    let stand = run.area.player.posture();
    let duck = FrameInput {
        held: keys::CROUCH,
        ..Default::default()
    };
    sync(
        &mut run,
        FrameInput {
            pressed: keys::CROUCH,
            ..duck
        },
        1,
    );
    let (_, ducked) = sync(&mut run, duck, 100);
    assert_ne!(
        run.area.player.posture(),
        stand,
        "the host ducks with held CROUCH"
    );
    assert_ne!(ducked, standing, "the figure ducks");
    let up = sync(&mut run, FrameInput::default(), 50).1;
    look(&mut run, 0., 60.);
    let (_, looked) = sync(&mut run, FrameInput::default(), 50);
    assert_ne!(looked, up, "the figure looks up");
}

/// MP M04a. Source `SendStartStuff`, `RTime`, `RadioChannelMultiplayerUpd` and `SSound`: when a guest
/// joins, the host sends its clock and its radio stations. The guest takes them. A host burp plays at
/// the host position on the guest.
#[test]
#[ignore = "requires the installed source export"]
fn tp_mp_m04a_guest_takes_host_clock_stations_and_burp() {
    use crate::audio::AudioKind;
    let (reference, mut host_run) = load_garage();
    settle(&mut host_run, &reference);
    let mut guest_run = save_load(&host_run, &reference);
    host_run.lighting.cycle.set_cycle_seconds(123.);
    let clip = host_run
        .audio_gameplay
        .binding(AudioKind::Burp as u32)
        .clips[0];
    let at = [1e5, 20., -3e4];
    host_run
        .shared_sounds
        .push(crate::net::message::SoundMessage {
            position: at,
            kind: AudioKind::Burp as u32,
            clip,
            gain: 1.,
            pitch: 1.,
        });
    let (mut host, mut guest) = (Net::default(), Net::default());
    host.set_session(NetRole::Host, 0);
    guest.set_session(NetRole::Guest, 1);
    let (mut frame, mut packets) = (SimFrame::default(), Vec::new());
    for _ in 0..20 {
        guest.fill(&reference, &mut guest_run, FIXED_DT, &mut frame);
        guest.take_outbox(&mut packets);
        host.receive(&packets);
        host.fill(&reference, &mut host_run, FIXED_DT, &mut frame);
        host.take_outbox(&mut packets);
        guest.receive(&packets);
    }
    guest.fill(&reference, &mut guest_run, FIXED_DT, &mut frame);
    assert_eq!(
        guest_run.lighting.cycle.cycle_seconds(),
        123.,
        "the host clock"
    );
    let (h, g) = (
        host_run.audio.as_ref().unwrap(),
        guest_run.audio.as_ref().unwrap(),
    );
    let host_channels: Vec<_> = h.broadcasts.host_channels(h.game_seconds).collect();
    let guest_channels: Vec<_> = g.broadcasts.host_channels(g.game_seconds).collect();
    assert_eq!(
        host_channels.iter().map(|c| c.0).collect::<Vec<_>>(),
        guest_channels.iter().map(|c| c.0).collect::<Vec<_>>(),
        "the host clips"
    );
    assert!(
        g.survival_events
            .iter()
            .any(|e| e.kind == AudioKind::Burp && e.emitter == 1 && e.position == at),
        "the host burp at the host"
    );
}
