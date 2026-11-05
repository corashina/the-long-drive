//! Multiplayer of the source (`sns`, `snTempPlayerSync`). The browser owns the peers and the session messages.
//! The simulation sends the local player and poses the remote players.
pub mod body;
pub mod car;
pub mod claim;
pub mod conditions;
pub mod consumables;
pub mod creatures;
pub mod inventory;
pub mod items;
pub mod lamps;
pub mod message;
pub mod mounts;
pub mod objects;
pub mod remote;
pub mod state;
pub mod world;

use crate::boundary::{PoseRow, RemoteRow, SimFrame};
use crate::player::body::BodyState;
use crate::reference::ReferenceManifest;
use crate::starting_area::GarageRuntime;
use car::CarSync;
use conditions::ConditionSync;
use consumables::ConsumableSync;
use creatures::CreatureSync;
use inventory::InventorySync;
use items::ItemSync;
use lamps::LampSync;
use message::{BodyMessage, BrokeMessage, Message, PoseMessage};
use mounts::MountSync;
use objects::ObjectIndex;
use remote::Remote;
use state::StateSync;
use world::WorldSync;

/// Source `snTempPlayerSync.syncTime`.
pub const SYNC_TIME: f32 = 0.15;
/// The source `snl` default. It is also the port cap.
pub const MAX_PLAYERS: usize = 8;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum NetRole {
    #[default]
    Off,
    Host,
    Guest,
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
    creatures: CreatureSync,
    objects: ObjectIndex,
    items: ItemSync,
    /// Bodies of figures that went away. The next frame removes them.
    dropped: Vec<rapier3d::prelude::RigidBodyHandle>,
    /// Scratch: the item bodies that one remote player holds.
    held_bodies: Vec<rapier3d::prelude::RigidBodyHandle>,
    cars: CarSync,
    inventory: InventorySync,
    state: StateSync,
    mounts: MountSync,
    conditions: ConditionSync,
    lamps: LampSync,
    consumables: ConsumableSync,
    /// Remote figure poses of this frame, in stream space.
    figures: Vec<(u8, rapier3d::math::Pose)>,
    /// Peers that left since the last frame.
    left: Vec<u8>,
    /// A new guest still stands on the host spot.
    step_aside: bool,
}

impl Net {
    pub fn set_session(&mut self, role: NetRole, slot: u8) {
        self.step_aside = role == NetRole::Guest;
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
            self.state.clear();
            self.mounts.clear();
            self.conditions.clear();
            self.lamps.clear();
            self.consumables.clear();
        }
    }
    /// Marks `runtime` for the role before each step, also the first after a load. Source
    /// `isServerOrNoMulti`. A guest spawns no rabbits or weeds. Guest spawn serials start at
    /// `slot << 24`, so a host spawn never takes the serial of a guest spawn. A new guest steps off
    /// the host spot once its world stands.
    pub fn prepare(&mut self, runtime: &mut GarageRuntime) {
        runtime.net_guest = self.role == NetRole::Guest;
        if runtime.net_guest {
            runtime
                .area
                .spawned
                .reserve_serials(u32::from(self.slot) << 24);
        }
        if self.step_aside && runtime.started && runtime.world_supported() {
            self.step_aside = false;
            runtime.step_aside(f32::from(self.slot) / MAX_PLAYERS as f32);
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
        self.cars.leave(slot);
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

    /// Runs one rendered frame. It reads the inbox, shares the cars, sends the local player when due and
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
        self.prepare(runtime);
        if self.role == NetRole::Off {
            runtime.shared_sounds.clear();
            runtime.shared_breaks.clear();
            runtime.area.interaction.push.pushes.clear();
            runtime.shared_radios.clear();
            self.cars.end(runtime);
            self.inventory.clear(runtime);
            return;
        }
        for slot in std::mem::take(&mut self.left) {
            self.inventory.leave(runtime, slot);
        }
        self.clock += real_dt as f64;
        self.objects
            .refresh(&runtime.area.items, &runtime.area.spawned);
        self.read_inbox(reference, runtime);
        self.cars.send(
            runtime,
            self.clock,
            &mut items::Out {
                slot: self.slot,
                bytes: &mut self.outbox,
            },
        );
        self.send(reference, runtime, real_dt);
        self.world.send(
            self.role,
            self.slot,
            runtime,
            real_dt,
            &self.objects,
            &mut self.outbox,
        );
        if self.role == NetRole::Host {
            self.creatures
                .send(self.slot, runtime, real_dt, &mut self.outbox);
        }
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
        let out = &mut items::Out {
            slot: self.slot,
            bytes: &mut self.outbox,
        };
        self.inventory.send(runtime, &self.objects, out);
        items::send_pushes(
            runtime,
            &self.objects,
            &self.items.claims,
            self.role == NetRole::Host,
            out,
        );
        for (item, record) in runtime.shared_breaks.drain(..) {
            if let Some(key) = self.objects.key(item) {
                out.push(&BrokeMessage {
                    key: key.0,
                    record,
                    pad: 0,
                });
            }
        }
        self.state
            .send(runtime, &self.objects, &self.items.requested, real_dt, out);
        self.mounts
            .send(runtime, &self.objects, &self.items.requested, real_dt, out);
        self.conditions
            .send(runtime, &self.objects, &self.items.requested, real_dt, out);
        self.lamps.send(reference, runtime, real_dt, out);
        self.consumables
            .send(runtime, &self.objects, &self.items.requested, real_dt, out);
        self.items.requested.clear();
        let origin = &runtime.stream.origin;
        self.figures.clear();
        let body = &reference.player.remote;
        let player = runtime.area.player.body_handle();
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
            if let Some(collision) = &mut remote.collision {
                collision.tick(self.clock, &mut runtime.physics, player);
            }
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
        // Source `UpdItemParent`. A figure ignores the items its player holds.
        for remote in &mut self.remotes {
            if let Some(c) = &mut remote.collision {
                self.inventory
                    .held_bodies(runtime, remote.slot, &mut self.held_bodies);
                c.ignore_only(&mut runtime.physics, &self.held_bodies);
            }
        }
    }

    /// Routes each packet on its type byte to the module that owns the message. That module
    /// decodes it and checks who may send it.
    fn read_inbox(&mut self, reference: &ReferenceManifest, runtime: &mut GarageRuntime) {
        let inbox = std::mem::take(&mut self.inbox);
        let mut rest = &inbox[..];
        let host = self.role == NetRole::Host;
        while let [a, b, tail @ ..] = rest {
            // The outbox flag is masked, so an outbox reads as an inbox.
            let n = (u16::from_le_bytes([*a, *b]) & !message::RELIABLE) as usize;
            let Some((packet, tail)) = tail.split_at_checked(n) else {
                break;
            };
            rest = tail;
            let &[kind, slot, ..] = packet else {
                continue;
            };
            // `web/net.js` stamps the sender slot and never sends a packet back to its sender.
            if slot == self.slot {
                continue;
            }
            let index = &self.objects;
            match kind {
                message::POSE | message::BODY => self.read_remote(reference, packet),
                message::ITEM_POSE
                | message::VELOCITY
                | message::REQUEST
                | message::CLAIM
                | message::REMOVE
                | message::PUSH
                | message::BROKE => self.items.receive(
                    reference,
                    runtime,
                    &mut self.objects,
                    &mut self.inventory,
                    &self.figures,
                    host,
                    packet,
                ),
                message::INVENTORY => self.inventory.receive(runtime, index, packet),
                message::FLUID | message::TANK_CAP | message::USABLE => {
                    self.state.receive(runtime, index, packet)
                }
                message::PART_CONDITION => self.conditions.receive(runtime, index, packet),
                message::FOOD | message::AMMO | message::WEAPON => {
                    self.consumables.receive(reference, runtime, index, packet)
                }
                message::FLASHLIGHT | message::BUS => {
                    self.lamps.receive(reference, runtime, packet)
                }
                message::PART_SLOT => self.mounts.receive(runtime, index, packet),
                message::CAR_FLOATS
                | message::CAR_HANDBRAKE
                | message::CAR_LAMPS
                | message::CAR_GEAR
                | message::CAR_IGNITION => {
                    self.cars.receive(reference, runtime, self.clock, packet)
                }
                message::TIME | message::RADIO_CHANNEL | message::RADIO | message::SOUND => {
                    WorldSync::receive(self.role, packet, index, runtime)
                }
                message::CREATURE if self.role == NetRole::Guest => {
                    creatures::receive(reference, packet, runtime)
                }
                _ => {}
            }
        }
        // The buffer keeps its capacity.
        self.inbox = inbox;
        self.inbox.clear();
    }

    /// Source `UpdPos` and `UpdAnim`: the pose and the body of a remote figure. A new slot joins.
    fn read_remote(&mut self, reference: &ReferenceManifest, packet: &[u8]) {
        if let Ok((slot, pose)) = PoseMessage::decode(packet) {
            let remote = match self.remotes.iter_mut().position(|r| r.slot == slot) {
                Some(i) => &mut self.remotes[i],
                None if self.remotes.len() < MAX_PLAYERS - 1 => {
                    let Ok(body) = BodyState::new(reference) else {
                        return;
                    };
                    self.remotes.push(Remote::new(slot, body));
                    if self.role == NetRole::Host {
                        self.world.joined();
                    }
                    self.remotes.last_mut().unwrap()
                }
                None => return,
            };
            remote.smoothing.got_pose(&pose, self.clock);
        } else if let Ok((slot, body)) = BodyMessage::decode(packet)
            && let Some(remote) = self.remotes.iter_mut().find(|r| r.slot == slot)
        {
            remote.got_body(&body);
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
