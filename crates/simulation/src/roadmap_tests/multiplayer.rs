//! ROADMAP test pass: Multiplayer (MP rows).
use crate::boundary::SimFrame;
use crate::interaction::items::ItemLocation;
use crate::net::{Net, NetRole};
use crate::reference::ItemId;
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

/// Host and guest runtimes that pass their packets each frame.
struct Pair {
    host: Net,
    guest: Net,
    frame: SimFrame,
    packets: Vec<u8>,
}

impl Pair {
    fn new() -> Self {
        let (mut host, mut guest) = (Net::default(), Net::default());
        host.set_session(NetRole::Host, 0);
        guest.set_session(NetRole::Guest, 1);
        Self {
            host,
            guest,
            frame: SimFrame::default(),
            packets: Vec::new(),
        }
    }
    fn run(
        &mut self,
        reference: &crate::reference::ReferenceManifest,
        host: &mut GarageRuntime,
        guest: &mut GarageRuntime,
        seconds: f32,
    ) {
        for _ in 0..(seconds / FIXED_DT).ceil() as usize {
            frame(host, reference);
            self.host.fill(reference, host, FIXED_DT, &mut self.frame);
            self.host.take_outbox(&mut self.packets);
            self.guest.receive(&self.packets);
            frame(guest, reference);
            self.guest.fill(reference, guest, FIXED_DT, &mut self.frame);
            self.guest.take_outbox(&mut self.packets);
            self.host.receive(&self.packets);
        }
    }
}

/// The same item on the other runtime.
fn same(from: &GarageRuntime, to: &GarageRuntime, item: ItemId) -> ItemId {
    to.area
        .items
        .resolve_key(&from.area.items.object_key(item))
        .expect("the item is on both peers")
}

fn absolute(run: &GarageRuntime, item: ItemId) -> [f64; 3] {
    run.stream
        .origin
        .absolute(run.area.items.get(item).unwrap().pose.position)
}

fn apart(a: [f64; 3], b: [f64; 3]) -> f64 {
    (0..3).map(|i| (a[i] - b[i]).powi(2)).sum::<f64>().sqrt()
}

/// The nearest loose item with a dynamic body.
fn loose_item(run: &GarageRuntime) -> ItemId {
    let player = body_position(run);
    run.area
        .items
        .iter()
        .filter(|i| !i.removed && i.location == ItemLocation::World && i.inactive_pose.is_none())
        .filter(|i| {
            i.body
                .is_some_and(|b| run.physics.rapier().bodies[b].is_dynamic())
        })
        .min_by(|a, b| {
            let a = (Vector::from_array(a.pose.position) - player).length();
            let b = (Vector::from_array(b.pose.position) - player).length();
            a.total_cmp(&b)
        })
        .expect("a loose item")
        .id
}

/// M03. Source `MultiUpd` and `UpdInWorld`: the host owns unclaimed items and sends their pose each
/// second. An item that the host moves moves on the guest.
#[test]
#[ignore = "requires the installed source export"]
fn tp_m03_a_host_item_moves_on_the_guest() {
    let (reference, mut run) = load_garage();
    settle(&mut run, &reference);
    let mut guest = save_load(&run, &reference);
    let mut pair = Pair::new();
    pair.run(&reference, &mut run, &mut guest, 0.5);
    let item = loose_item(&run);
    let copy = same(&run, &guest, item);
    let body = run.area.items.get(item).unwrap().body.unwrap();
    run.physics.edit_body(body, |b| {
        let mut pose = *b.position();
        pose.translation.y += 0.5;
        b.set_position(pose, true);
    });
    pair.run(&reference, &mut run, &mut guest, 1.2);
    let (host_at, guest_at) = (absolute(&run, item), absolute(&guest, copy));
    assert!(
        apart(host_at, guest_at) < 0.1,
        "the guest item follows: host {host_at:?}, guest {guest_at:?}"
    );
}

/// M03. Source `Claim`: a guest pickup claims the item, so the host does not send it. The drop
/// sends the last pose, then the release, and the host owns the item again.
#[test]
#[ignore = "requires the installed source export"]
fn tp_m03_a_guest_pickup_claims_the_item_and_the_drop_returns_it() {
    let (reference, mut run) = load_garage();
    settle(&mut run, &reference);
    let mut guest = save_load(&run, &reference);
    let mut pair = Pair::new();
    pair.run(&reference, &mut run, &mut guest, 0.5);
    let copy = loose_item(&guest);
    let item = same(&guest, &run, copy);
    pick_up(&mut guest, &reference, copy);
    pair.run(&reference, &mut run, &mut guest, 0.5);
    assert!(pair.guest.claims().claimed(copy), "the guest claims it");
    assert_eq!(
        pair.host.claims().other(item),
        Some(1),
        "the host sees the claim"
    );
    assert!(!pair.host.claims().owns(item, true));
    pair.run(&reference, &mut run, &mut guest, 1.2);
    assert!(
        apart(absolute(&run, item), absolute(&guest, copy)) < 0.2,
        "the host item follows the held item"
    );
    guest
        .area
        .interaction
        .drop_held(&mut guest.area.items, &mut guest.physics);
    guest.area.sync_inventory();
    pair.run(&reference, &mut run, &mut guest, 0.2);
    assert!(!pair.guest.claims().claimed(copy), "the drop releases it");
    assert_eq!(pair.host.claims().other(item), None);
    assert!(
        pair.host.claims().owns(item, true),
        "the host owns it again"
    );
}
