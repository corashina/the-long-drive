//! Multiplayer of the source (`sns`, `snTempPlayerSync`). The browser owns the peers and the session messages.
//! The simulation sends the local player and poses the remote players.
pub mod body;
pub mod message;
pub mod remote;
pub mod world;

use crate::boundary::{PoseRow, RemoteRow, SimFrame};
use crate::player::body::BodyState;
use crate::reference::ReferenceManifest;
use crate::starting_area::GarageRuntime;
use message::{BodyMessage, Message, PoseMessage};
use remote::Remote;
use world::WorldSync;

/// Source `snTempPlayerSync.syncTime`.
pub const SYNC_TIME: f32 = 0.15;
/// Source `snl` default and the port cap.
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
    /// Bodies of figures that went away. The next frame removes them.
    dropped: Vec<rapier3d::prelude::RigidBodyHandle>,
}

impl Net {
    pub fn set_session(&mut self, role: NetRole, slot: u8) {
        self.role = role;
        self.slot = slot;
        if role == NetRole::Off {
            self.dropped.extend(
                self.remotes
                    .drain(..)
                    .filter_map(|r| r.collision.map(|c| c.handle)),
            );
            self.inbox.clear();
        }
    }
    pub fn role(&self) -> NetRole {
        self.role
    }
    /// Adds length-prefixed packets from the browser.
    pub fn receive(&mut self, packets: &[u8]) {
        if self.role != NetRole::Off {
            self.inbox.extend_from_slice(packets);
        }
    }
    /// A peer left. Its figure goes away.
    pub fn leave(&mut self, slot: u8) {
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

    /// One rendered frame: reads the inbox, sends the local player when due and poses the remotes.
    pub fn fill(
        &mut self,
        reference: &ReferenceManifest,
        runtime: &mut GarageRuntime,
        real_dt: f32,
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
            return;
        }
        self.clock += real_dt as f64;
        self.read_inbox(reference, runtime);
        self.send(runtime, real_dt);
        self.world
            .send(self.role, self.slot, runtime, real_dt, &mut self.outbox);
        let origin = &runtime.stream.origin;
        let body = &reference.player.remote;
        for remote in &mut self.remotes {
            let Some((pose, root)) =
                remote.step(self.clock, real_dt, body, origin, &mut runtime.physics)
            else {
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
        let mut rest = &self.inbox[..];
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
            } else if let Ok((slot, body)) = BodyMessage::decode(packet)
                && let Some(remote) = self.remotes.iter_mut().find(|r| r.slot == slot)
            {
                remote.got_body(&body);
            } else {
                WorldSync::receive(self.role, packet, runtime);
            }
        }
        self.inbox.clear();
    }

    fn send(&mut self, runtime: &GarageRuntime, dt: f32) {
        let area = &runtime.area;
        let interval = area.body.send_interval();
        self.pose_timer += dt;
        self.body_timer += dt;
        if self.pose_timer >= SYNC_TIME {
            self.pose_timer = 0.;
            let pose = area.player.body_pose(&runtime.physics);
            let message = PoseMessage {
                position: runtime.stream.origin.absolute(pose.position),
                rotation: pose.rotation,
                duck: area.player.posture(),
                flags: if area.player.seated() {
                    PoseMessage::SEATED
                } else {
                    0
                },
            };
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
