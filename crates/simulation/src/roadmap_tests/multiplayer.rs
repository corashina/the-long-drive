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
        guest.fill(&reference, &mut guest_run, FIXED_DT, 1., &mut frame);
        guest.take_outbox(&mut packets);
        host.receive(&packets);
        host.fill(&reference, &mut host_run, FIXED_DT, 1., &mut frame);
        host.take_outbox(&mut packets);
        guest.receive(&packets);
    }
    guest.fill(&reference, &mut guest_run, FIXED_DT, 1., &mut frame);
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
            self.host
                .fill(reference, host, FIXED_DT, 1., &mut self.frame);
            self.host.take_outbox(&mut self.packets);
            self.guest.receive(&self.packets);
            frame(guest, reference);
            self.guest
                .fill(reference, guest, FIXED_DT, 1., &mut self.frame);
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

/// M03. Source `Claim` and `UpdInventory`: a guest pickup claims the item, so the host does not send
/// it, and the host copy rides on the guest. The drop sends the last pose, then the release, and the
/// host owns the loose item again.
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
    let body = run.area.items.get(item).unwrap().body.unwrap();
    assert!(
        run.physics.rapier().bodies[body].is_kinematic(),
        "source UpdInventory: the host copy rides on the guest"
    );
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
    assert!(
        run.physics.rapier().bodies[body].is_dynamic(),
        "the host copy is loose again"
    );
}

/// M03. Source `UpdOrSpawnItem`: a runtime spawn of the host appears on the guest with the same
/// serial, where the host has it.
#[test]
#[ignore = "requires the installed source export"]
fn tp_m03_a_host_spawn_appears_on_the_guest() {
    let (reference, mut run) = load_garage();
    settle(&mut run, &reference);
    let mut guest = save_load(&run, &reference);
    let mut pair = Pair::new();
    pair.run(&reference, &mut run, &mut guest, 0.5);
    let prototype = reference
        .item_prototypes
        .iter()
        .position(|p| !p.pickups.is_empty())
        .expect("an item prototype");
    let mut root = run.area.player.body_pose(&run.physics);
    root.position[1] += 1.;
    let ids = crate::starting_area::spawned::spawn_prototype(
        &reference,
        &mut run.area,
        &mut run.physics,
        prototype,
        root,
    )
    .unwrap();
    let serial = run.area.spawned.get(ids[0]).unwrap().serial;
    pair.run(&reference, &mut run, &mut guest, 1.2);
    let copy = guest
        .area
        .spawned
        .iter()
        .find(|s| s.serial == serial && s.prototype == prototype)
        .expect("the guest spawns it")
        .id;
    assert!(apart(absolute(&run, ids[0]), absolute(&guest, copy)) < 0.2);
}

/// M03. Source `STank` and `FluidUpdate`: a fluid change on the host reaches the guest copy, and
/// the guest does not send it back.
#[test]
#[ignore = "requires the installed source export"]
fn tp_m03_a_host_fluid_change_reaches_the_guest() {
    let (reference, mut run) = load_garage();
    settle(&mut run, &reference);
    let mut guest = save_load(&run, &reference);
    let mut pair = Pair::new();
    pair.run(&reference, &mut run, &mut guest, 0.5);
    let tank = run
        .area
        .items
        .iter()
        .find(|i| !i.removed && i.fluid.as_ref().is_some_and(|f| f.capacity_liters > 1.))
        .expect("a tank")
        .id;
    let copy = same(&run, &guest, tank);
    let fluid = run
        .area
        .items
        .get_mut(tank)
        .unwrap()
        .fluid
        .as_mut()
        .unwrap();
    let liters = fluid.capacity_liters * 0.5;
    fluid.restore_entries(&[crate::fluids::FluidEntry {
        fluid_type: 1,
        amount: liters,
    }]);
    pair.run(&reference, &mut run, &mut guest, 0.5);
    let got = guest.area.items.get(copy).unwrap().fluid.as_ref().unwrap();
    assert!(
        (got.total() - liters).abs() < 1e-3,
        "the guest has the host fluid"
    );
    pair.run(&reference, &mut run, &mut guest, 0.1);
    let host = run.area.items.get(tank).unwrap().fluid.as_ref().unwrap();
    assert!((host.total() - liters).abs() < 1e-3, "no echo");
}

/// M03. Source `PartSlotUnCraft` and `PartSlotCraft`: a part that comes off the host car comes off
/// the guest car, and goes back on.
#[test]
#[ignore = "requires the installed source export"]
fn tp_m03_a_host_part_comes_off_and_goes_on_at_the_guest() {
    let (reference, mut run) = load_garage();
    settle(&mut run, &reference);
    let mut guest = save_load(&run, &reference);
    let mut pair = Pair::new();
    pair.run(&reference, &mut run, &mut guest, 0.5);
    let (part, slot) = run
        .area
        .interaction
        .parts
        .mounted()
        .find_map(|(item, _)| match run.area.items.get(item)?.location {
            ItemLocation::PartSlot(slot) => {
                let i = &run.area.interaction;
                (i.parts
                    .can_remove(item, &run.area.player, &run.area.items)
                    .is_ok())
                .then_some((item, slot))
            }
            _ => None,
        })
        .expect("a removable part");
    let copy = same(&run, &guest, part);
    let i = &mut run.area.interaction;
    i.fall_off_part(part, &mut run.area.items, &mut run.physics)
        .unwrap();
    pair.run(&reference, &mut run, &mut guest, 0.5);
    assert_eq!(
        guest.area.items.get(copy).unwrap().location,
        ItemLocation::World,
        "the part is off at the guest"
    );
    let i = &mut run.area.interaction;
    i.install_part(part, slot, &mut run.area.items, &mut run.physics)
        .unwrap();
    pair.run(&reference, &mut run, &mut guest, 0.5);
    assert!(
        matches!(
            guest.area.items.get(copy).unwrap().location,
            ItemLocation::PartSlot(_)
        ),
        "the part is on at the guest"
    );
}

/// M03. Source `SPartCondition` and `SFlashlight`: rust on the host shows on the guest, and a
/// garage light switch on the host switches on the guest.
#[test]
#[ignore = "requires the installed source export"]
fn tp_m03_host_rust_and_light_switch_reach_the_guest() {
    use crate::reference::LightControllerKind;
    let (reference, mut run) = load_garage();
    settle(&mut run, &reference);
    let mut guest = save_load(&run, &reference);
    let mut pair = Pair::new();
    pair.run(&reference, &mut run, &mut guest, 0.5);
    let id = run
        .area
        .interaction
        .conditions
        .iter()
        .find(|(_, s)| s.state < 4)
        .expect("a garage surface")
        .0;
    run.area.interaction.conditions.wear(id);
    let state = run.area.interaction.conditions.get(id).state;
    let (i, control) = reference
        .lighting
        .controllers
        .iter()
        .enumerate()
        .find(|(_, c)| c.kind == LightControllerKind::Switch)
        .expect("a garage switch");
    run.lighting.use_control(&reference.lighting, control.id);
    let on = run.lighting.controllers[i].on;
    pair.run(&reference, &mut run, &mut guest, 0.5);
    assert_eq!(
        guest.area.interaction.conditions.get(id).state,
        state,
        "the guest has the rust"
    );
    assert_eq!(
        guest.lighting.controllers[i].on, on,
        "the guest light switched"
    );
}

/// MP M04c. Source `aispawn.SpawnRoutine` (`isServerOrNoMulti`) and the item pose of an AI: the
/// guest spawns no rabbits of its own, and a rabbit that the host spawns appears on the guest with
/// the host id and follows the host pose.
#[test]
#[ignore = "requires the installed source export"]
fn tp_mp_m04c_host_rabbits_appear_on_the_guest() {
    let (reference, mut host_run) = super::creatures::outdoors();
    let mut guest_run = save_load(&host_run, &reference);
    let (mut host, mut guest) = (Net::default(), Net::default());
    host.set_session(NetRole::Host, 0);
    guest.set_session(NetRole::Guest, 1);
    let mut frame = SimFrame::default();
    let id = act(
        &mut host_run,
        &reference,
        r#"{"action":"spawn","name":"rabbit","forward":4}"#,
    )
    .as_u64()
    .unwrap() as u32;
    let before: Vec<u32> = guest_run.creatures.rabbits().map(|r| r.id).collect();
    for _ in 0..(2. / FIXED_DT) as usize {
        exchange(
            &reference,
            [
                (&mut host_run, &mut host, FrameInput::default()),
                (&mut guest_run, &mut guest, FrameInput::default()),
            ],
            &mut frame,
        );
    }
    let at = |run: &GarageRuntime| {
        run.creatures
            .views(&run.physics)
            .find(|v| v.id == id)
            .map(|v| run.stream.origin.absolute(v.pose.translation.to_array()))
    };
    let (h, g) = (
        at(&host_run).unwrap(),
        at(&guest_run).expect("the host rabbit on the guest"),
    );
    assert!(
        apart(h, g) < 1.,
        "the guest rabbit follows the host: {h:?} {g:?}"
    );
    let spawned = guest_run
        .creatures
        .rabbits()
        .filter(|r| !before.contains(&r.id) && r.id != id)
        .count();
    assert_eq!(spawned, 0, "the guest spawns no rabbits of its own");
}
