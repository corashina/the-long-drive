//! Multiplayer of the source (`sns`, `snTempPlayerSync`). The browser owns the peers and the session messages.
//! The simulation sends the local player and poses the remote players.
pub mod body;
pub mod car;
pub mod claim;
pub mod inventory;
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
use inventory::InventorySync;
use items::ItemSync;
use message::{
    BodyMessage, CarFloatsMessage, CarGearMessage, CarHandbrakeMessage, CarIgnitionMessage,
    ClaimMessage, InventoryMessage, ItemPoseMessage, Message, PoseMessage, RemoveMessage,
    RequestMessage, VelocityMessage,
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
    /// Bodies of figures that went away. The next frame removes them.
    dropped: Vec<rapier3d::prelude::RigidBodyHandle>,
    /// Scratch: the item bodies that one remote player holds.
    held_bodies: Vec<rapier3d::prelude::RigidBodyHandle>,
    cars: Vec<CarValues>,
    /// The cars carry session state that must clear when the session ends.
    cars_touched: bool,
    inventory: InventorySync,
    /// Remote figure poses of this frame, in stream space.
    figures: Vec<(u8, rapier3d::math::Pose)>,
    /// Peers that left since the last frame.
    left: Vec<u8>,
    /// Guest spawn serials start at `slot << 24`.
    serials_reserved: bool,
}

impl Net {
    pub fn set_session(&mut self, role: NetRole, slot: u8) {
        self.role = role;
        self.slot = slot;
        self.items.slot = slot;
        if role == NetRole::Off {
            self.dropped.extend(
                self.remotes
                    .drain(..)
                    .filter_map(|r| r.collision.map(|c| c.handle)),
            );
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
        self.items.claims.leave(slot);
        self.cars.retain(|c| c.slot != slot);
        self.left.push(slot);
        if let Some(i) = self.remotes.iter().position(|r| r.slot == slot) {
            let remote = self.remotes.remove(i);
            self.dropped.extend(remote.collision.map(|c| c.handle));
        }
    }
    /// The collision body of the figure of `slot`.
    #[cfg(test)]
    pub(crate) fn remote_body(&self, slot: u8) -> Option<&body::RemoteBody> {
        self.remotes
            .iter()
            .find(|r| r.slot == slot)
            .and_then(|r| r.collision.as_ref())
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
        for handle in self.dropped.drain(..) {
            runtime.physics.remove_body(handle);
        }
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
            self.inventory.clear(runtime);
            self.serials_reserved = false;
            return;
        }
        self.cars_touched = true;
        for slot in std::mem::take(&mut self.left) {
            self.inventory.leave(runtime, slot);
        }
        if self.role == NetRole::Guest && !std::mem::replace(&mut self.serials_reserved, true) {
            runtime
                .area
                .spawned
                .reserve_serials(u32::from(self.slot) << 24);
        }
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
        self.items.bound(
            runtime,
            &mut self.objects,
            self.role == NetRole::Host,
            &mut self.outbox,
        );
        self.items.send(
            runtime,
            &self.objects,
            self.role == NetRole::Host,
            self.clock,
            real_dt,
            &mut self.outbox,
        );
        self.inventory.send(
            runtime,
            &self.objects,
            &mut items::Out {
                slot: self.slot,
                bytes: &mut self.outbox,
            },
        );
        let origin = &runtime.stream.origin;
        self.figures.clear();
        let body = &reference.player.remote;
        for remote in &mut self.remotes {
            let seated = remote
                .smoothing
                .car()
                .and_then(|(car, seat)| car::seat_pose(reference, runtime, alpha, car, seat));
            let Some((pose, root)) = remote.step(
                self.clock,
                real_dt,
                body,
                origin,
                &mut runtime.physics,
                |_, _| seated,
            ) else {
                continue;
            };
            self.figures
                .push((remote.slot, crate::vehicle::to_rapier(pose)));
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
        self.inventory.follow(runtime, &self.figures, real_dt);
        // Source `UpdItemParent`: a figure ignores the items its player holds.
        for remote in &mut self.remotes {
            if let Some(c) = &mut remote.collision {
                self.inventory
                    .held_bodies(runtime, remote.slot, &mut self.held_bodies);
                c.ignore_only(&mut runtime.physics, &self.held_bodies);
            }
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
            } else if !self.read_item(reference, packet, runtime)
                && !self.read_car(reference, packet, runtime)
            {
                WorldSync::receive(self.role, packet, runtime);
            }
        }
        // The buffer keeps its capacity.
        self.inbox = inbox;
        self.inbox.clear();
    }

    /// Item messages. False for other packets.
    fn read_item(
        &mut self,
        reference: &ReferenceManifest,
        packet: &[u8],
        runtime: &mut GarageRuntime,
    ) -> bool {
        let host = self.role == NetRole::Host;
        let mine = self.slot;
        let item = |slot: u8, key: u64| {
            (slot != mine)
                .then(|| self.objects.item(NetKey(key)))
                .flatten()
        };
        if let Ok((slot, m)) = ItemPoseMessage::decode(packet) {
            if let Some(id) = item(slot, m.key) {
                if !self.inventory.target(runtime, &self.figures, id, &m) {
                    items::apply_pose(runtime, &self.items.claims, id, &m);
                }
            } else if slot != mine {
                if items::spawn(reference, runtime, &m).is_some() {
                    // Later packets of this frame name the new item.
                    self.objects
                        .refresh(&runtime.area.items, &runtime.area.spawned);
                } else if host {
                    self.items.keep(NetKey(m.key), Some(m), None);
                }
            }
        } else if let Ok((slot, m)) = InventoryMessage::decode(packet) {
            if let Some(id) = item(slot, m.key) {
                self.inventory.got(runtime, id, slot, m.flags);
            }
        } else if let Ok((slot, m)) = VelocityMessage::decode(packet) {
            if let Some(id) = item(slot, m.key) {
                items::apply_velocity(runtime, &self.items.claims, id, &m);
            } else if slot != mine && host {
                self.items.keep(NetKey(m.key), None, Some(m));
            }
        } else if let Ok((slot, m)) = RequestMessage::decode(packet) {
            if slot != mine && host {
                self.items.requested(m.keys());
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
