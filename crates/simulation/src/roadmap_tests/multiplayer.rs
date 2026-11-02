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
            host.fill(&reference, run, FIXED_DT, 1., &mut sent);
            host.take_outbox(&mut packets);
            guest.receive(&packets);
            guest.fill(&reference, run, FIXED_DT, 1., &mut seen);
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

/// One frame of a host and a guest that run their own world copies.
fn exchange(
    reference: &crate::reference::ReferenceManifest,
    peers: [(&mut GarageRuntime, &mut Net, FrameInput); 2],
    frame: &mut SimFrame,
) {
    let mut packets = Vec::new();
    let [(a, a_net, a_input), (b, b_net, b_input)] = peers;
    frame_with(a, reference, a_input);
    frame_with(b, reference, b_input);
    a_net.fill(reference, a, FIXED_DT, 1., frame);
    a_net.take_outbox(&mut packets);
    b_net.receive(&packets);
    b_net.fill(reference, b, FIXED_DT, 1., frame);
    b_net.take_outbox(&mut packets);
    a_net.receive(&packets);
}

/// MP4. Source `SCarFloats`, `SCarHandbrake`, `SCarGear`, `SCarIgnition` and `SPlayerPosUpd`
/// sitting: the guest drives the start car. The host copy takes its gas, handbrake, gear and
/// ignition, and the guest figure sits in the host copy of the car.
#[test]
#[ignore = "requires the installed source export"]
fn tp_mp4_the_guest_drives_the_host_copy_of_the_car_and_sits_in_it() {
    let (reference, mut host) = load_garage();
    settle(&mut host, &reference);
    let mut guest = save_load(&host, &reference);
    frame(&mut guest, &reference);
    let (mut host_net, mut guest_net) = (Net::default(), Net::default());
    host_net.set_session(NetRole::Host, 0);
    guest_net.set_session(NetRole::Guest, 1);
    let mut seen = SimFrame::default();
    act(
        &mut guest,
        &reference,
        r#"{"action":"seat","seat":"driver"}"#,
    );
    let key = guest
        .area
        .player
        .seat_vehicle()
        .cloned()
        .expect("the guest sits");
    let gas = FrameInput {
        held: keys::FORWARD,
        ..Default::default()
    };
    for _ in 0..90 {
        exchange(
            &reference,
            [
                (&mut guest, &mut guest_net, gas),
                (&mut host, &mut host_net, FrameInput::default()),
            ],
            &mut seen,
        );
    }
    let car = &host.vehicles[&key];
    assert!(
        car.net.remote.is_some_and(|r| r.gas > 0.),
        "the host copy takes the guest gas"
    );
    // `exchange` fills the host frame last.
    assert_eq!(seen.remotes.len(), 1, "one remote figure");
    let figure = Vector::from_array(seen.remotes[0].body_pose.position);
    let chassis = host.physics.chassis_handle(&key);
    let at = host.physics.rapier().bodies[chassis].translation();
    assert!(
        (figure - Vector::new(at.x, at.y, at.z)).length() < 3.,
        "the guest figure sits in the host copy"
    );
    let vehicle = guest.vehicles.get_mut(&key).unwrap();
    vehicle.handbrake = !vehicle.handbrake;
    vehicle.ignition.ignition = !vehicle.ignition.ignition;
    let gear = (vehicle.drivetrain.gear() + 1) % vehicle.record(&reference).gear_ratios.len();
    vehicle.drivetrain.restore_gear(gear);
    let want = (vehicle.handbrake, vehicle.ignition.ignition);
    for _ in 0..3 {
        exchange(
            &reference,
            [
                (&mut guest, &mut guest_net, FrameInput::default()),
                (&mut host, &mut host_net, FrameInput::default()),
            ],
            &mut seen,
        );
    }
    let car = &host.vehicles[&key];
    assert_eq!((car.handbrake, car.ignition.ignition), want);
    assert_eq!(car.drivetrain.gear(), gear, "the host copy shifts");
    // No echo: the host sends no reliable car state back.
    let mut packets = Vec::new();
    host_net.fill(&reference, &mut host, FIXED_DT, 1., &mut seen);
    host_net.take_outbox(&mut packets);
    let mut rest = &packets[..];
    while let [a, b, tail @ ..] = rest {
        let n = u16::from_le_bytes([*a, *b]);
        assert_eq!(n & crate::net::message::RELIABLE, 0, "no echo");
        rest = &tail[(n & !crate::net::message::RELIABLE) as usize..];
    }
}
