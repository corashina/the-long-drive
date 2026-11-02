//! Multiplayer of the source (`sns`, `snTempPlayerSync`). The browser owns the peers and the session messages.
//! The simulation sends the local player and poses the remote players.
pub mod car;
pub mod claim;
pub mod items;
pub mod message;
pub mod objects;
pub mod remote;
pub mod world;

use crate::boundary::{PoseRow, RemoteRow, SimFrame};
use crate::player::body::BodyState;
use crate::reference::ReferenceManifest;
use crate::starting_area::GarageRuntime;
use car::CarFloats;
use items::ItemSync;
use message::{
    BodyMessage, CarFloatsMessage, CarGearMessage, CarHandbrakeMessage, CarIgnitionMessage,
    ClaimMessage, ItemPoseMessage, Message, PoseMessage, RemoveMessage, VelocityMessage,
};
use objects::{NetKey, ObjectIndex};
use remote::Remote;
use world::WorldSync;

/// Source `snTempPlayerSync.syncTime`.
pub const SYNC_TIME: f32 = 0.15;
/// Source `snl` default and the port cap.
pub const MAX_PLAYERS: usize = 8;
/// Source `carscript.UpdMulti`: driver values older than 1 s drop.
const CAR_VALUES_SECONDS: f64 = 1.;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum NetRole {
    #[default]
    Off,
    Host,
    Guest,
}

/// Source `carscript.CarSyncValues` entry of one peer and one car.
struct CarValues {
    slot: u8,
    car: u64,
    floats: CarFloats,
    time: f64,
}

#[derive(Default)]
pub struct Net {
    role: NetRole,
    slot: u8,
    /// Real seconds since the session started, like source `Time.unscaledTime`.
    clock: f64,
    pose_timer: f32,
    body_timer: f32,
    /// Inbound packets, each with a u16 length.
    inbox: Vec<u8>,
    /// Outbound packets, each with a u16 length. Bit `message::RELIABLE` of the length marks a reliable packet.
    outbox: Vec<u8>,
    remotes: Vec<Remote>,
    world: WorldSync,
    objects: ObjectIndex,
    items: ItemSync,
    cars: Vec<CarValues>,
    /// The cars carry session state that must clear when the session ends.
    cars_touched: bool,
}

impl Net {
    pub fn set_session(&mut self, role: NetRole, slot: u8) {
        self.role = role;
        self.slot = slot;
        self.items.slot = slot;
        if role == NetRole::Off {
            self.remotes.clear();
            self.inbox.clear();
            self.objects.clear();
            self.items.clear();
            self.cars.clear();
        }
    }
    pub fn role(&self) -> NetRole {
        self.role
    }
    pub fn claims(&self) -> &claim::Claims {
        &self.items.claims
    }
    /// Adds length-prefixed packets from the browser.
    pub fn receive(&mut self, packets: &[u8]) {
        if self.role != NetRole::Off {
            self.inbox.extend_from_slice(packets);
        }
    }
    /// A peer left. Its figure goes away.
    pub fn leave(&mut self, slot: u8) {
        self.remotes.retain(|r| r.slot != slot);
        self.items.claims.leave(slot);
        self.cars.retain(|c| c.slot != slot);
    }
    /// Length-prefixed packets for the browser. The next call starts empty.
    pub fn take_outbox(&mut self, out: &mut Vec<u8>) {
        out.clear();
        std::mem::swap(out, &mut self.outbox);
    }

    /// One rendered frame: reads the inbox, shares the cars, sends the local player when due and
    /// poses the remotes. `alpha`: the render interpolation between the last two steps.
    pub fn fill(
        &mut self,
        reference: &ReferenceManifest,
        runtime: &mut GarageRuntime,
        real_dt: f32,
        alpha: f32,
        frame: &mut SimFrame,
    ) {
        frame.remotes.clear();
        frame.remote_joints.clear();
        if let Some(audio) = &mut runtime.audio {
            audio.broadcasts.set_remote(self.role == NetRole::Guest);
        }
        if self.role == NetRole::Off {
            runtime.shared_sounds.clear();
            if std::mem::take(&mut self.cars_touched) {
                for vehicle in runtime.vehicles.values_mut() {
                    vehicle.net.clear();
                }
            }
            return;
        }
        self.cars_touched = true;
        self.clock += real_dt as f64;
        self.objects
            .refresh(&runtime.area.items, &runtime.area.spawned);
        self.read_inbox(reference, runtime);
        self.share_cars(runtime);
        self.send_cars(runtime);
        self.send(reference, runtime, real_dt);
        self.world
            .send(self.role, self.slot, runtime, real_dt, &mut self.outbox);
        self.objects
            .refresh(&runtime.area.items, &runtime.area.spawned);
        self.items.send(
            runtime,
            &self.objects,
            self.role == NetRole::Host,
            self.clock,
            real_dt,
            &mut self.outbox,
        );
        let origin = &runtime.stream.origin;
        for remote in &mut self.remotes {
            let seat = |car, seat| car::seat_pose(reference, runtime, alpha, car, seat);
            let Some((pose, root)) = remote.step(self.clock, real_dt, origin, seat) else {
                continue;
            };
            frame.remotes.push(RemoteRow {
                slot: remote.slot as u32,
                flags: RemoteRow::VISIBLE,
                body_pose: PoseRow::new(pose, 0),
                body_root: PoseRow::new(root, 0),
            });
            let body = &remote.body;
            frame
                .remote_joints
                .extend((0..body.joint_count()).map(|i| PoseRow::new(body.joint_local(i), 0)));
        }
    }

    fn read_inbox(&mut self, reference: &ReferenceManifest, runtime: &mut GarageRuntime) {
        let inbox = std::mem::take(&mut self.inbox);
        let mut rest = &inbox[..];
        while let [a, b, tail @ ..] = rest {
            // The outbox flag is masked, so an outbox reads as an inbox.
            let n = (u16::from_le_bytes([*a, *b]) & !message::RELIABLE) as usize;
            let Some((packet, tail)) = tail.split_at_checked(n) else {
                break;
            };
            rest = tail;
            if let Ok((slot, pose)) = PoseMessage::decode(packet) {
                if slot == self.slot {
                    continue;
                }
                let remote = match self.remotes.iter_mut().position(|r| r.slot == slot) {
                    Some(i) => &mut self.remotes[i],
                    None if self.remotes.len() < MAX_PLAYERS - 1 => {
                        let Ok(body) = BodyState::new(reference) else {
                            continue;
                        };
                        self.remotes.push(Remote::new(slot, body));
                        if self.role == NetRole::Host {
                            self.world.joined();
                        }
                        self.remotes.last_mut().unwrap()
                    }
                    None => continue,
                };
                remote.smoothing.got_pose(&pose, self.clock);
            } else if let Ok((slot, body)) = BodyMessage::decode(packet) {
                if let Some(remote) = self.remotes.iter_mut().find(|r| r.slot == slot) {
                    remote.got_body(&body);
                }
            } else if !self.read_item(packet, runtime) && !self.read_car(reference, packet, runtime)
            {
                WorldSync::receive(self.role, packet, runtime);
            }
        }
        // The buffer keeps its capacity.
        self.inbox = inbox;
        self.inbox.clear();
    }

    /// Item messages. False for other packets.
    fn read_item(&mut self, packet: &[u8], runtime: &mut GarageRuntime) -> bool {
        let mine = self.slot;
        let item = |slot: u8, key: u64| {
            (slot != mine)
                .then(|| self.objects.item(NetKey(key)))
                .flatten()
        };
        if let Ok((slot, m)) = ItemPoseMessage::decode(packet) {
            if let Some(id) = item(slot, m.key) {
                items::apply_pose(runtime, &self.items.claims, id, &m);
            }
        } else if let Ok((slot, m)) = VelocityMessage::decode(packet) {
            if let Some(id) = item(slot, m.key) {
                items::apply_velocity(runtime, &self.items.claims, id, &m);
            }
        } else if let Ok((slot, m)) = ClaimMessage::decode(packet) {
            if let Some(id) = item(slot, m.key) {
                self.items.claims.got(id, slot, m.claim != 0);
            }
        } else if let Ok((slot, m)) = RemoveMessage::decode(packet) {
            // Source `RemoveItem`: Destroy with `removeFromMemory`.
            if let Some(id) = item(slot, m.key) {
                let area = &mut runtime.area;
                let _ = runtime.stream.remove_item(
                    id,
                    &mut area.items,
                    &mut area.interaction.storage,
                    &mut runtime.physics,
                );
            }
        } else {
            return false;
        }
        true
    }

    /// Car messages. False for other packets.
    fn read_car(
        &mut self,
        reference: &ReferenceManifest,
        packet: &[u8],
        runtime: &mut GarageRuntime,
    ) -> bool {
        let mine = self.slot;
        if let Ok((slot, m)) = CarFloatsMessage::decode(packet) {
            // Source `UpdMultiValues`.
            if slot != mine {
                let floats = CarFloats::of(&m);
                match self
                    .cars
                    .iter_mut()
                    .find(|c| c.slot == slot && c.car == m.car)
                {
                    Some(c) => {
                        c.floats = floats;
                        c.time = self.clock;
                    }
                    None => self.cars.push(CarValues {
                        slot,
                        car: m.car,
                        floats,
                        time: self.clock,
                    }),
                }
            }
        } else if let Ok((slot, m)) = CarHandbrakeMessage::decode(packet) {
            if slot != mine {
                car::got_handbrake(runtime, m.car, m.handbrake);
            }
        } else if let Ok((slot, m)) = CarGearMessage::decode(packet) {
            if slot != mine {
                car::got_gear(reference, runtime, m.car, m.gear as usize);
            }
        } else if let Ok((slot, m)) = CarIgnitionMessage::decode(packet) {
            if slot != mine {
                car::got_ignition(runtime, &m);
            }
        } else {
            return false;
        }
        true
    }

    /// Source `carscript.UpdMulti` receiver half: drops old driver values and merges the rest.
    fn share_cars(&mut self, runtime: &mut GarageRuntime) {
        let now = self.clock;
        self.cars.retain(|c| now - c.time < CAR_VALUES_SECONDS);
        for vehicle in runtime.vehicles.values_mut() {
            vehicle.net.remote = self
                .cars
                .iter()
                .filter(|c| c.car == vehicle.net.key)
                .map(|c| c.floats)
                .reduce(CarFloats::merge);
        }
    }

    /// Source `carscript.UpdMulti` and the `SCar*` senders on change.
    fn send_cars(&mut self, runtime: &mut GarageRuntime) {
        for (key, vehicle) in &runtime.vehicles {
            let net = &vehicle.net;
            if let Some(f) = net.sent {
                self.push(&CarFloatsMessage {
                    car: net.key,
                    steer: f.steer,
                    horn: f.horn,
                    gas: f.gas,
                    brake: f.brake,
                    clutch: f.clutch,
                    bike_gas: f.bike_gas,
                });
            }
            let Some(was) = net.shared else {
                continue;
            };
            let now = car::shared(vehicle);
            if now.handbrake != was.handbrake {
                self.push(&CarHandbrakeMessage {
                    car: net.key,
                    handbrake: now.handbrake as u8 as f32,
                    pad: 0,
                });
            }
            if now.gear != was.gear {
                self.push(&CarGearMessage {
                    car: net.key,
                    gear: now.gear as u32,
                    pad: 0,
                });
            }
            // Source sends on a toggle and when the starter begins.
            if now.ignition != was.ignition || (now.starter && !was.starter) {
                let mut flags = 0;
                if now.ignition {
                    flags |= CarIgnitionMessage::IGNITION;
                }
                if now.starter {
                    flags |= CarIgnitionMessage::STARTING;
                }
                self.push(&CarIgnitionMessage {
                    car: net.key,
                    flags,
                    rpm: runtime.engine_rpm(key),
                });
            }
        }
        for vehicle in runtime.vehicles.values_mut() {
            vehicle.net.shared = Some(car::shared(vehicle));
        }
    }

    fn send(&mut self, reference: &ReferenceManifest, runtime: &GarageRuntime, dt: f32) {
        let area = &runtime.area;
        let interval = area.body.send_interval();
        self.pose_timer += dt;
        self.body_timer += dt;
        if self.pose_timer >= SYNC_TIME {
            self.pose_timer = 0.;
            let pose = area.player.body_pose(&runtime.physics);
            let mut message = PoseMessage {
                position: runtime.stream.origin.absolute(pose.position),
                rotation: pose.rotation,
                duck: area.player.posture(),
                flags: if area.player.seated() {
                    PoseMessage::SEATED
                } else {
                    0
                },
                ..PoseMessage::default()
            };
            car::seat_message(reference, runtime, pose, &mut message);
            self.push(&message);
        }
        if self.body_timer >= interval {
            self.body_timer = 0.;
            self.push(&BodyMessage {
                targets: area.body.sent_targets(),
            });
        }
    }

    fn push<M: Message>(&mut self, message: &M) {
        message::push(self.slot, message, &mut self.outbox);
    }
}
