//! Headless scenario tests: multiplayer.
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

/// The absolute body position of `run`. A loaded origin is at the player.
fn absolute_body(run: &GarageRuntime) -> Vector {
    let p = run.stream.origin.absolute(body_position(run).to_array());
    Vector::new(p[0] as f32, p[1] as f32, p[2] as f32)
}

/// The player capsule of `run` overlaps no solid collider and stands clear of every car.
fn stands_free(run: &GarageRuntime) -> bool {
    use rapier3d::parry::bounding_volume::BoundingVolume;
    use rapier3d::prelude::QueryFilter;
    let own = run.area.player.body_handle();
    let rapier = run.physics.rapier();
    let solid = |_, c: &rapier3d::prelude::Collider| c.is_enabled() && c.parent() != Some(own);
    let pose = crate::vehicle::to_rapier(run.area.player.body_pose(&run.physics));
    let shape = run.area.player.sweep_shape();
    let bounds = rapier3d::parry::shape::Shape::compute_aabb(shape, &pose);
    let in_car = run.vehicles.keys().any(|key| {
        rapier.bodies[run.physics.chassis_handle(key)]
            .colliders()
            .iter()
            .any(|c| rapier.colliders[*c].compute_aabb().intersects(&bounds))
    });
    !in_car
        && run
            .physics
            .queries(QueryFilter::default().exclude_sensors().predicate(&solid))
            .intersect_shape(pose, shape)
            .next()
            .is_none()
}

/// The host shares its save (`saveSession.share`) with its player in the garage. The host
/// sits in its car, but the guest starts on foot near the garage start spot and clips nothing.
#[test]
#[ignore = "requires the installed source export"]
fn a_joining_guest_starts_in_the_garage() {
    let (reference, mut host) = load_garage();
    settle(&mut host, &reference);
    act(
        &mut host,
        &reference,
        r#"{"action":"seat","seat":"driver"}"#,
    );
    seconds(&mut host, &reference, 0.5);
    assert!(host.area.player.seated(), "the host sits");
    let mut shared = save(&host, &reference);
    host.garage_start(&reference, &mut shared.player);
    let mut guest = load(&reference, &shared);
    let mut net = Net::default();
    net.set_session(NetRole::Guest, 1);
    let mut out = SimFrame::default();
    for _ in 0..120 {
        frame(&mut guest, &reference);
        net.fill(&reference, &mut guest, FIXED_DT, 1., &mut out);
    }
    assert!(!guest.area.player.seated(), "the guest stands");
    // The spot is on the startup-aligned garage floor, not on the raw reference spawn.
    let spot = guest.garage_spot(&reference).translation.to_array();
    let spot = guest.stream.origin.absolute(spot).map(|v| v as f32);
    let gap = (absolute_body(&guest) - Vector::from_array(spot)).length();
    assert!(gap < 7., "the guest is near the garage start spot: {gap} m");
    assert!(stands_free(&guest), "the guest clips nothing");
}

/// Source `snTempPlayerSync` and `SPlayerAnim`. The host sends its pose, duck value and body
/// targets. The guest figure stands at the host, and ducks and looks up with it.
#[test]
#[ignore = "requires the installed source export"]
fn the_other_figure_follows_walk_duck_and_look() {
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

/// Source `SCarFloats`, `SCarHandbrake`, `SCarGear`, `SCarIgnition` and `SPlayerPosUpd`
/// sitting. The guest drives the start car. The host copy takes its gas, handbrake, gear and
/// ignition, and the guest figure sits in the host copy of the car.
#[test]
#[ignore = "requires the installed source export"]
fn the_guest_drives_the_host_copy_of_the_car_and_sits_in_it() {
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
        player_held: crate::simulation::player_keys::GAS,
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
    // The seated guest AutoShift would pick its own gear, so the guest shifts by hand.
    guest.driving_assists = crate::reference::DrivingAssists {
        automatic_clutch: false,
        automatic_gears: false,
    };
    let vehicle = guest.vehicles.get_mut(&key).unwrap();
    vehicle.handbrake = !vehicle.handbrake;
    let on = vehicle.ignition.on();
    vehicle.ignition.set_on(!on);
    let gear = (vehicle.drivetrain.gear() + 1) % vehicle.record(&reference).gear_ratios.len();
    vehicle.drivetrain.restore_gear(gear);
    let want = (vehicle.handbrake, vehicle.ignition.on());
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
    assert_eq!((car.handbrake, car.ignition.on()), want);
    assert_eq!(car.drivetrain.gear(), gear, "the host copy shifts");
    // There is no echo. The host sends no reliable car state back.
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

/// A guest drives and the host sits beside it. The car moves fast. Each frame between two steps
/// shows the guest figure at the same place on the shown host car, so the figure does not jitter.
#[test]
#[ignore = "requires the installed source export"]
fn a_seated_figure_stays_on_the_shown_car() {
    use rapier3d::math::{Pose, Rotation};
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
    act(
        &mut host,
        &reference,
        r#"{"action":"seat","seat":"passenger"}"#,
    );
    let key = guest
        .area
        .player
        .seat_vehicle()
        .cloned()
        .expect("the guest sits");
    let chassis = host.physics.chassis_handle(&key);
    let mut seat = None;
    for i in 0..60 {
        for run in [&mut guest, &mut host] {
            let handle = run.physics.chassis_handle(&key);
            run.physics.edit_body(handle, |b| {
                let forward = b.rotation() * Vector::Z;
                b.set_linvel(
                    forward * 15. + Vector::Y * if i % 20 < 10 { 0.5 } else { -0.5 },
                    true,
                );
            });
        }
        exchange(
            &reference,
            [
                (&mut guest, &mut guest_net, FrameInput::default()),
                (&mut host, &mut host_net, FrameInput::default()),
            ],
            &mut seen,
        );
        for alpha in [0., 0.5, 1.] {
            host_net.fill(&reference, &mut host, 0., alpha, &mut seen);
            // The first pose comes after one send interval.
            let Some(figure) = seen.remotes.first().map(|r| r.body_pose) else {
                continue;
            };
            let figure = Pose::from_parts(
                Vector::from_array(figure.position),
                Rotation::from_array(figure.rotation),
            );
            let car = host
                .physics
                .presented_pose_or(chassis, alpha, Pose::IDENTITY);
            let local = (car.inverse() * figure).translation;
            let first = *seat.get_or_insert(local);
            assert!(
                (local - first).length() < 1e-3,
                "frame {i} alpha {alpha}: the figure is {} m off its seat",
                (local - first).length()
            );
        }
    }
}

/// A seat with another player in it takes nobody else. The host sits in the driver seat. The
/// guest cannot sit there, and the hint offers no seat. A guest that joins in the host seat gets
/// out, because the lower slot keeps a seat.
#[test]
#[ignore = "requires the installed source export"]
fn a_seat_with_another_player_takes_nobody_else() {
    let (reference, mut host) = load_garage();
    settle(&mut host, &reference);
    let mut guest = save_load(&host, &reference);
    frame(&mut guest, &reference);
    act(
        &mut host,
        &reference,
        r#"{"action":"seat","seat":"driver"}"#,
    );
    let key = host
        .area
        .player
        .seat_vehicle()
        .cloned()
        .expect("the host sits");
    let mut pair = Pair::new();
    pair.run(&reference, &mut host, &mut guest, 1.);
    assert!(
        !guest.area.interaction.in_use.seats.is_empty(),
        "the guest sees the host seat"
    );
    let reply = try_act(
        &mut guest,
        &reference,
        r#"{"action":"seat","seat":"driver"}"#,
    );
    assert_eq!(reply["ok"], false, "the guest cannot sit in the host seat");
    assert!(!guest.area.player.seated());
    // A guest that starts in the host seat gets out.
    let mut joined = save_load(&host, &reference);
    frame(&mut joined, &reference);
    assert_eq!(
        joined.area.player.seat_vehicle(),
        Some(&key),
        "the save puts it in the seat"
    );
    pair.run(&reference, &mut host, &mut joined, 1.);
    assert!(
        !joined.area.player.seated(),
        "the guest gets out of the host seat"
    );
    assert!(host.area.player.seated(), "the host keeps it");
}

/// Source `mpplayerscript`. The figure of the other player turns solid 3 s after it
/// shows, on layer 0, but not while it overlaps the player. The guest steps off the host spot
/// first. A ray with the player capsule layers
/// then hits its body at the figure.
#[test]
#[ignore = "requires the installed source export"]
fn the_other_figure_turns_solid_after_3_s() {
    use rapier3d::prelude::{QueryFilter, Ray};
    let (reference, mut host_run) = load_garage();
    settle(&mut host_run, &reference);
    let mut guest_run = save_load(&host_run, &reference);
    let (mut host, mut guest) = (Net::default(), Net::default());
    host.set_session(NetRole::Host, 0);
    guest.set_session(NetRole::Guest, 1);
    let (mut out, mut packets) = (SimFrame::default(), Vec::new());
    let mut sync =
        |host_run: &mut GarageRuntime, guest_run: &mut GarageRuntime, guest: &mut Net, s: f32| {
            for _ in 0..(s / FIXED_DT) as usize {
                frame(host_run, &reference);
                frame(guest_run, &reference);
                host.fill(&reference, host_run, FIXED_DT, 1., &mut out);
                host.take_outbox(&mut packets);
                guest.receive(&packets);
                guest.fill(&reference, guest_run, FIXED_DT, 1., &mut out);
                guest.take_outbox(&mut packets);
            }
        };
    sync(&mut host_run, &mut guest_run, &mut guest, 2.5);
    let body = guest.remote_body(0).expect("the host figure has a body");
    assert!(!body.enabled(), "not solid before 3 s");
    sync(&mut host_run, &mut guest_run, &mut guest, 1.);
    // The loaded origin is at the player, so compare absolute positions.
    let absolute = |run: &GarageRuntime| {
        let p = run.stream.origin.absolute(body_position(run).to_array());
        Vector::new(p[0] as f32, p[1] as f32, p[2] as f32)
    };
    assert!(
        (absolute(&guest_run) - absolute(&host_run)).length() > 1.,
        "the guest stepped off the host spot"
    );
    let body = guest.remote_body(0).unwrap();
    assert!(body.enabled(), "solid after 3 s");
    let at = body.position(&guest_run.physics);
    let capsule = &reference.player.posture;
    let handle = body.handle;
    // Skip the own body.
    let own = guest_run.area.player.body_handle();
    let filter = |_, c: &rapier3d::prelude::Collider| {
        c.is_enabled()
            && c.parent() != Some(own)
            && c.collision_groups().memberships.bits() & capsule.capsule_filter != 0
            && c.collision_groups().filter.bits() & capsule.capsule_membership != 0
    };
    let ray = Ray::new(at + Vector::new(2., 0., 0.), Vector::new(-1., 0., 0.));
    let (hit, _) = guest_run
        .physics
        .queries(QueryFilter::default().predicate(&filter))
        .cast_ray(&ray, 4., true)
        .expect("the ray hits the figure");
    assert_eq!(
        guest_run.physics.rapier().colliders[hit].parent(),
        Some(handle),
        "the figure body blocks the player capsule layers"
    );
}

/// Source `SendStartStuff`, `RTime`, `RadioChannelMultiplayerUpd` and `SSound`. When a guest
/// joins, the host sends its clock and its radio stations. The guest takes them. A host burp plays at
/// the host position on the guest.
#[test]
#[ignore = "requires the installed source export"]
fn guest_takes_host_clock_stations_and_burp() {
    use crate::audio::AudioKind;
    let (reference, mut host_run) = load_garage();
    settle(&mut host_run, &reference);
    let mut guest_run = save_load(&host_run, &reference);
    host_run.lighting.cycle.set_cycle_seconds(123.);
    let clip = host_run
        .audio
        .reference
        .survival_row(AudioKind::Burp as u32)
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
    let (h, g) = (&host_run.audio, &guest_run.audio);
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
            self.step(reference, host, guest);
        }
    }
    /// One frame of each peer, marked like `Driver::advance`.
    fn step(
        &mut self,
        reference: &crate::reference::ReferenceManifest,
        host: &mut GarageRuntime,
        guest: &mut GarageRuntime,
    ) {
        self.host.prepare(host);
        frame(host, reference);
        self.host
            .fill(reference, host, FIXED_DT, 1., &mut self.frame);
        self.host.take_outbox(&mut self.packets);
        self.guest.receive(&self.packets);
        self.guest.prepare(guest);
        frame(guest, reference);
        self.guest
            .fill(reference, guest, FIXED_DT, 1., &mut self.frame);
        self.guest.take_outbox(&mut self.packets);
        self.host.receive(&self.packets);
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

/// Source `MultiUpd` and `UpdInWorld`. The host owns unclaimed items and sends their pose each
/// second. An item that the host moves moves on the guest.
#[test]
#[ignore = "requires the installed source export"]
fn a_host_item_moves_on_the_guest() {
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

/// Source `mountStuff.Upd` and `UpdLocalPos`. The host sends the owner pose of a stored item.
/// The guest copy moves on its owner to the host pose.
#[test]
#[ignore = "requires the installed source export"]
fn a_host_stored_item_moves_on_the_guest() {
    let (reference, mut run) = load_garage();
    settle(&mut run, &reference);
    let mut guest = save_load(&run, &reference);
    let mut pair = Pair::new();
    pair.run(&reference, &mut run, &mut guest, 0.5);
    let storage = &run.area.interaction.storage;
    let (item, (_, local)) = run
        .area
        .items
        .iter()
        .find_map(|i| Some((i.id, storage.owner_local(i.id, &run.area.items)?)))
        .expect("a stored item");
    let copy = same(&run, &guest, item);
    let mut moved = local;
    moved.translation.x += 0.2;
    let area = &mut run.area;
    area.interaction
        .storage
        .set_owner_local(item, moved, &mut area.items, &mut run.physics);
    pair.run(&reference, &mut run, &mut guest, 1.2);
    let (_, got) = guest
        .area
        .interaction
        .storage
        .owner_local(copy, &guest.area.items)
        .expect("the guest copy is stored");
    assert!(
        got.translation.distance(moved.translation) < 0.01,
        "the guest copy takes the host pose: host {moved:?}, guest {got:?}"
    );
}

/// Source `Claim` and `UpdInventory`. A guest pickup claims the item, so the host does not send
/// it, and the host copy moves with the guest. The drop sends the last pose, then the release, and the
/// host owns the loose item again.
#[test]
#[ignore = "requires the installed source export"]
fn a_guest_pickup_claims_the_item_and_the_drop_returns_it() {
    let (reference, mut run) = load_garage();
    settle(&mut run, &reference);
    let mut guest = save_load(&run, &reference);
    let mut pair = Pair::new();
    pair.run(&reference, &mut run, &mut guest, 0.5);
    // The nearest loose item can be one that the hand cannot carry, so take a light garage item.
    let copy = find(&guest, "OilCan");
    let item = same(&guest, &run, copy);
    // The host figure of the guest moves after source `readyToMove` (1 s), and a held item moves
    // with it at `smoothMoveSpeed`. The guest walks to the item first, so the figure is there.
    let point = guest.area.items.get(copy).unwrap().pose.position;
    stand_near(&mut guest, &reference, point, 1.);
    pair.run(&reference, &mut run, &mut guest, 1.);
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
    for _ in 0..6 {
        pair.run(&reference, &mut run, &mut guest, 0.2);
        let figure = pair.host.remote_body(1).map(|b| b.position(&run.physics));
        let guest_body = guest
            .stream
            .origin
            .absolute(body_position(&guest).to_array());
        eprintln!(
            "m03 pickup: host item {:?}, guest item {:?}, apart {:.3}, host figure of the guest {:?}, guest body {guest_body:?}",
            absolute(&run, item),
            absolute(&guest, copy),
            apart(absolute(&run, item), absolute(&guest, copy)),
            figure.map(|f| run.stream.origin.absolute(f.to_array())),
        );
    }
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

/// Source `pickupable.Pick` and `Claim`. The guest takes an item from the hands of the host.
/// The host lets go, and the host copy follows the guest.
#[test]
#[ignore = "requires the installed source export"]
fn the_guest_takes_an_item_from_the_hands_of_the_host() {
    let (reference, mut run) = load_garage();
    settle(&mut run, &reference);
    let mut guest = save_load(&run, &reference);
    let mut pair = Pair::new();
    pair.run(&reference, &mut run, &mut guest, 0.5);
    let item = find(&run, "OilCan");
    let copy = same(&run, &guest, item);
    pick_up(&mut run, &reference, item);
    let point = guest.area.items.get(copy).unwrap().pose.position;
    stand_near(&mut guest, &reference, point, 1.);
    pair.run(&reference, &mut run, &mut guest, 1.);
    assert!(
        guest.area.interaction.in_use.hand(copy).is_some(),
        "the guest sees the item in the hands of the host"
    );
    pick_up(&mut guest, &reference, copy);
    pair.run(&reference, &mut run, &mut guest, 0.5);
    assert!(pair.guest.claims().claimed(copy), "the guest claims it");
    assert_eq!(run.area.interaction.held(), None, "the host lets go");
    assert_eq!(pair.host.claims().other(item), Some(1));
    let body = run.area.items.get(item).unwrap().body.unwrap();
    assert!(
        run.physics.rapier().bodies[body].is_kinematic(),
        "the host copy rides on the guest"
    );
    let body = guest.area.items.get(copy).unwrap().body.unwrap();
    assert!(
        guest.physics.rapier().bodies[body].is_dynamic(),
        "the guest carries it"
    );
}

/// Source `UpdOrSpawnItem`. A runtime spawn of the host appears on the guest with the same
/// serial, where the host has it.
#[test]
#[ignore = "requires the installed source export"]
fn a_host_spawn_appears_on_the_guest() {
    let (reference, mut run) = load_garage();
    settle(&mut run, &reference);
    let mut guest = save_load(&run, &reference);
    let mut pair = Pair::new();
    pair.run(&reference, &mut run, &mut guest, 0.5);
    // A metal shovel where both players stood. They sit in the car first, because a spawn in a player
    // capsule is thrown out and can break before the host sends it.
    let shovel = weapon(&reference, &["shovel"]);
    let prototype = reference
        .item_prototypes
        .iter()
        .position(|p| p.id == shovel.prototype)
        .expect("the shovel prototype");
    let root = run.area.player.body_pose(&run.physics);
    for peer in [&mut run, &mut guest] {
        act(peer, &reference, r#"{"action":"seat","seat":"driver"}"#);
        assert!(peer.area.player.seated(), "the player sits");
    }
    pair.run(&reference, &mut run, &mut guest, 0.2);
    let ids = crate::starting_area::spawned::spawn_prototype(
        &reference,
        &mut run.area,
        &mut run.physics,
        prototype,
        root,
    )
    .unwrap();
    let serial = run.area.spawned.get(ids[0]).unwrap().serial;
    // The last host pose of the 1 s checks comes after the shovel lies still.
    pair.run(&reference, &mut run, &mut guest, 2.2);
    let serials: Vec<u32> = guest.area.spawned.iter().map(|s| s.serial).collect();
    let copy = guest
        .area
        .spawned
        .iter()
        .find(|s| s.serial == serial && s.prototype == prototype)
        .unwrap_or_else(|| panic!("the guest spawns serial {serial}: guest serials {serials:?}"))
        .id;
    assert!(apart(absolute(&run, ids[0]), absolute(&guest, copy)) < 0.2);
}

/// Source `STank` and `FluidUpdate`. A fluid change on the host reaches the guest copy, and
/// the guest does not send it back.
#[test]
#[ignore = "requires the installed source export"]
fn a_host_fluid_change_reaches_the_guest() {
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

/// Source `PartSlotUnCraft` and `PartSlotCraft`. A part that comes off the host car comes off
/// the guest car, and goes back on.
#[test]
#[ignore = "requires the installed source export"]
fn a_host_part_comes_off_and_goes_on_at_the_guest() {
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

/// Source `SPartCondition` and `SFlashlight`. Rust on the host shows on the guest, and a
/// garage light switch on the host switches on the guest.
#[test]
#[ignore = "requires the installed source export"]
fn host_rust_and_light_switch_reach_the_guest() {
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

/// Source `SFood`. A bite that the host takes shows on the guest.
#[test]
#[ignore = "requires the installed source export"]
fn a_host_bite_reaches_the_guest() {
    let (reference, mut run) = load_garage();
    settle(&mut run, &reference);
    let mut guest = save_load(&run, &reference);
    let mut pair = Pair::new();
    pair.run(&reference, &mut run, &mut guest, 0.5);
    let food = run
        .area
        .items
        .iter()
        .find(|i| {
            !i.removed
                && i.food
                    .is_some_and(|f| f.bite + 1 < reference.survival.foods[f.prototype_index].bites)
        })
        .expect("a food")
        .id;
    let copy = same(&run, &guest, food);
    let bite = run.area.items.get(food).unwrap().food.unwrap().bite + 1;
    run.set_food_bite(&reference, food, bite).unwrap();
    pair.run(&reference, &mut run, &mut guest, 0.5);
    assert_eq!(guest.area.items.get(copy).unwrap().food.unwrap().bite, bite);
}

/// Source `aispawn.SpawnRoutine` (`isServerOrNoMulti`) and the item pose of an AI. The
/// guest spawns no rabbits of its own, and a rabbit that the host spawns appears on the guest with
/// the host id and follows the host pose.
#[test]
#[ignore = "requires the installed source export"]
fn host_rabbits_appear_on_the_guest() {
    let (reference, mut host_run) = outdoors();
    let mut guest_run = save_load(&host_run, &reference);
    let (mut host, mut guest) = (Net::default(), Net::default());
    host.set_session(NetRole::Host, 0);
    guest.set_session(NetRole::Guest, 1);
    // `Driver::advance` marks a guest runtime before each step, also the first after the load.
    guest.prepare(&mut guest_run);
    let mut frame = SimFrame::default();
    let id = act(
        &mut host_run,
        &reference,
        r#"{"action":"spawn","name":"rabbit","forward":4}"#,
    )
    .as_u64()
    .unwrap() as u32;
    let before: Vec<u32> = guest_run.creatures.rabbits().map(|r| r.id).collect();
    let at = |run: &GarageRuntime| {
        run.creatures
            .views(&run.physics)
            .find(|v| v.id == id)
            .map(|v| run.stream.origin.absolute(v.pose.translation.to_array()))
    };
    // The guest runs its own rabbit AI between host poses, so the copies drift for up to 1 s.
    // Right after a host pose lands, the guest rabbit is at the host rabbit.
    let mut synced = 0;
    let mut gap = f64::INFINITY;
    for step in 0..(2.5 / FIXED_DT) as usize {
        exchange(
            &reference,
            [
                (&mut host_run, &mut host, FrameInput::default()),
                (&mut guest_run, &mut guest, FrameInput::default()),
            ],
            &mut frame,
        );
        let h = at(&host_run).unwrap();
        let Some(g) = at(&guest_run) else {
            continue;
        };
        let d = apart(h, g);
        if (step as f32) * FIXED_DT > 0.5 {
            gap = gap.min(d);
            synced += usize::from(d < 0.05);
        }
    }
    assert!(at(&guest_run).is_some(), "the host rabbit on the guest");
    assert!(
        synced > 0,
        "the guest rabbit takes the host pose: nearest {gap} m"
    );
    let spawned = guest_run
        .creatures
        .rabbits()
        .filter(|r| !before.contains(&r.id) && r.id != id)
        .count();
    assert_eq!(spawned, 0, "the guest spawns no rabbits of its own");
}

/// A dead player without other players respawns near the garage start spot with new-game
/// needs, on foot, and clips nothing.
#[test]
#[ignore = "requires the installed source export"]
fn respawn_without_other_players_goes_to_the_garage() {
    let (reference, mut run) = load_garage();
    settle(&mut run, &reference);
    let mut net = Net::default();
    net.set_session(NetRole::Host, 0);
    run.survival
        .damage_instant(&reference.survival, reference.survival.maximum.hp);
    seconds(&mut run, &reference, 1.);
    assert!(run.needs_output().dead);
    assert!(run.area.ragdoll.active(), "the body falls");
    net.respawn(&reference, &mut run);
    assert!(!run.needs_output().dead, "the player lives");
    assert!(!run.area.ragdoll.active());
    let mut out = SimFrame::default();
    for _ in 0..60 {
        frame(&mut run, &reference);
        net.fill(&reference, &mut run, FIXED_DT, 1., &mut out);
    }
    assert!(!run.needs_output().dead, "the player stays alive");
    let spawn = Vector::from_array(reference.area.player_spawn.translation);
    let gap = (absolute_body(&run) - spawn).length();
    assert!(gap < 7., "near the garage start spot: {gap} m");
    assert!(
        run.area.player.grounded(),
        "the player stands on the ground"
    );
    assert!(stands_free(&run), "the player clips nothing");
}

/// The guest can start before its restore spawns the host car. The step, the request and
/// the frame then do without the car, and the car comes in later.
#[test]
#[ignore = "requires the installed source export"]
fn the_guest_starts_before_the_host_car_spawns() {
    let (reference, mut host) = load_garage();
    settle(&mut host, &reference);
    // The host sat in the start car and stands again. The car stays its last car.
    let key = host.vehicles.keys().next().cloned().expect("the start car");
    host.vehicle = Some(key.clone());
    let mut guest = open(&reference, &save(&host, &reference));
    assert!(
        !guest.vehicles.contains_key(&key),
        "the car is not spawned yet"
    );
    guest.phase = crate::starting_area::SessionPhase::Playing;
    let mut out = SimFrame::default();
    for _ in 0..600 {
        frame(&mut guest, &reference);
        guest.advance_saved_restore(&reference).unwrap();
        guest.fill_frame(&reference, 1., &mut out);
        if guest.vehicles.contains_key(&key) {
            return;
        }
    }
    panic!("the host car does not spawn");
}

/// Source `SDmgPlayer`. The figure of the host has layer 9 bone colliders that take hits. The
/// guest sends the hit damage, and the host loses that much HP.
#[test]
#[ignore = "requires the installed source export"]
fn a_hit_on_the_other_figure_hurts_that_player() {
    use rapier3d::prelude::{QueryFilter, Ray};
    let (reference, mut host_run) = load_garage();
    settle(&mut host_run, &reference);
    let mut guest_run = save_load(&host_run, &reference);
    let (mut host, mut guest) = (Net::default(), Net::default());
    host.set_session(NetRole::Host, 0);
    guest.set_session(NetRole::Guest, 1);
    let mut out = SimFrame::default();
    let mut sync = |runs: [&mut GarageRuntime; 2], nets: [&mut Net; 2], s: f32| {
        let ([host_run, guest_run], [host, guest]) = (runs, nets);
        for _ in 0..(s / FIXED_DT) as usize {
            exchange(
                &reference,
                [
                    (&mut *host_run, &mut *host, FrameInput::default()),
                    (&mut *guest_run, &mut *guest, FrameInput::default()),
                ],
                &mut out,
            );
        }
    };
    sync(
        [&mut host_run, &mut guest_run],
        [&mut host, &mut guest],
        1.5,
    );
    let body = guest.remote_body(0).expect("the host figure has a body");
    let at = body.position(&guest_run.physics);
    let hits = |_, c: &rapier3d::prelude::Collider| {
        c.is_enabled() && c.collision_groups().memberships.bits() == 1 << 9
    };
    // Rays across the figure from 1 m below to 1 m above its root.
    let hit = (0..=20)
        .find_map(|i| {
            let from = at + Vector::new(2., i as f32 * 0.1 - 1., 0.);
            guest_run
                .physics
                .queries(QueryFilter::default().predicate(&hits))
                .cast_ray(&Ray::new(from, Vector::new(-1., 0., 0.)), 4., true)
        })
        .map(|(hit, _)| hit)
        .expect("a ray through the figure hits a bone collider");
    let data = guest_run.physics.rapier().colliders[hit].user_data;
    assert_eq!(
        crate::vehicle::figure_slot(data),
        Some(0),
        "the host figure"
    );
    let hp = |run: &GarageRuntime| run.needs_output().needs.hp + run.survival.pending().hp;
    let (host_before, guest_before) = (hp(&host_run), hp(&guest_run));
    guest_run.area.interaction.guns.player_damage.add(0, 20.);
    sync(
        [&mut host_run, &mut guest_run],
        [&mut host, &mut guest],
        0.1,
    );
    assert!(
        (hp(&guest_run) - guest_before).abs() < 1.,
        "the guest keeps its HP"
    );
    let lost = host_before - hp(&host_run);
    assert!((lost - 20.).abs() < 1., "the host loses 20 HP: {lost}");
}
