//! Host rabbits on the guests. Source creatures are save items: guests get them by item sync,
//! run their AI and take the owner pose each second. Only the host spawns them (`aispawn`).
use super::message::{self, CreatureMessage, Message, RELIABLE};
use crate::reference::ReferenceManifest;
use crate::starting_area::GarageRuntime;
use rapier3d::math::{Pose, Rotation, Vector};

/// Source `tosaveitemscript` sync check interval.
pub const POSE_INTERVAL: f32 = 1.;

#[derive(Default)]
pub struct CreatureSync {
    timer: f32,
    /// Rabbits whose death went out. A death goes out at once and reliably.
    dead: Vec<u32>,
}

impl CreatureSync {
    /// Host: every rabbit each `POSE_INTERVAL`, and each new death at once.
    pub fn send(&mut self, slot: u8, runtime: &GarageRuntime, dt: f32, out: &mut Vec<u8>) {
        self.timer += dt;
        let due = self.timer >= POSE_INTERVAL;
        if due {
            self.timer = 0.;
            let creatures = &runtime.creatures;
            self.dead
                .retain(|id| creatures.rabbits().any(|r| r.id == *id && !r.alive));
        }
        let origin = &runtime.stream.origin;
        for (id, size, alive, pose, velocity) in runtime.creatures.host_rows(&runtime.physics) {
            let died = !alive && !self.dead.contains(&id);
            if !due && !died {
                continue;
            }
            let m = CreatureMessage {
                id,
                alive: alive.into(),
                size,
                position: origin.absolute(pose.translation.to_array()),
                rotation: pose.rotation.to_array(),
                velocity: velocity.to_array(),
                ..Default::default()
            };
            if died {
                self.dead.push(id);
                message::push_with(
                    true,
                    message::HEADER + size_of::<CreatureMessage>(),
                    out,
                    |b| m.encode(slot, b),
                );
            } else {
                message::push(slot, &m, out);
            }
        }
    }
}

/// Guest: one host rabbit. False when it is not a creature packet.
pub fn receive(reference: &ReferenceManifest, packet: &[u8], runtime: &mut GarageRuntime) -> bool {
    let Ok((slot, m)) = CreatureMessage::decode(packet) else {
        return false;
    };
    let finite = m.position.iter().all(|v| v.is_finite())
        && m.rotation.iter().chain(&m.velocity).all(|v| v.is_finite())
        && m.size.is_finite();
    if slot != 0 || !finite {
        return true;
    }
    let pose = Pose::from_parts(
        Vector::from_array(runtime.stream.origin.local(m.position)),
        Rotation::from_array(m.rotation).normalize(),
    );
    let now = runtime.simulation_seconds;
    let origin = runtime.stream.origin;
    let _ = runtime.creatures.apply_host(
        &reference.creatures.rabbit,
        &mut runtime.physics,
        &origin,
        m.id,
        m.size,
        m.alive != 0,
        pose,
        Vector::from_array(m.velocity),
        now,
    );
    true
}

const _: () = assert!(message::HEADER + size_of::<CreatureMessage>() < RELIABLE as usize);
