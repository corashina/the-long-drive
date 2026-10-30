//! Player messages of source `sns`. A packet is the type byte, the sender slot byte, then the fixed body.
//! `web/net.js` reads only the two header bytes.
use bytemuck::{Pod, Zeroable};

/// Source `msgType.playerPosUpd`.
pub const POSE: u8 = 10;
/// Source `msgType.playerAnimUpd`.
pub const BODY: u8 = 13;
pub const HEADER: usize = 2;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct NetError;

/// Source `SPlayerPosUpd`. Seats are not shared until shared cars, so a seated player sends its world pose.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Pod, Zeroable)]
pub struct PoseMessage {
    /// Player Transform in world space. It does not change with origin shifts.
    pub position: [f64; 3],
    pub rotation: [f32; 4],
    /// Source `duckValue`.
    pub duck: f32,
    pub flags: u32,
}

impl PoseMessage {
    pub const SEATED: u32 = 1;
}

/// Source `SPlayerAnim`: targets relative to the player. Rotations are quaternions, not source euler angles.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Pod, Zeroable)]
pub struct BodyMessage {
    /// `BodyState::sent_targets`: position, then rotation quaternion.
    pub targets: [[f32; 7]; 6],
}

pub trait Message: Pod {
    const TYPE: u8;
    /// Appends the packet to `out`.
    fn encode(&self, slot: u8, out: &mut Vec<u8>) {
        out.extend_from_slice(&[Self::TYPE, slot]);
        out.extend_from_slice(bytemuck::bytes_of(self));
    }
    /// The sender slot and the message of a packet of this type.
    fn decode(packet: &[u8]) -> Result<(u8, Self), NetError> {
        match packet {
            [kind, slot, body @ ..] if *kind == Self::TYPE && body.len() == size_of::<Self>() => {
                Ok((*slot, bytemuck::pod_read_unaligned(body)))
            }
            _ => Err(NetError),
        }
    }
}

impl Message for PoseMessage {
    const TYPE: u8 = POSE;
}
impl Message for BodyMessage {
    const TYPE: u8 = BODY;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn messages_round_trip() {
        let pose = PoseMessage {
            position: [1e6, -2.5, 3.25],
            rotation: [0., 0.7071, 0., 0.7071],
            duck: 0.5,
            flags: PoseMessage::SEATED,
        };
        let mut body = BodyMessage::default();
        body.targets[2] = [0.1, 1.2, 0.3, 0., 0., 0., 1.];
        let mut out = Vec::new();
        pose.encode(3, &mut out);
        assert_eq!(PoseMessage::decode(&out), Ok((3, pose)));
        out.clear();
        body.encode(4, &mut out);
        assert_eq!(BodyMessage::decode(&out), Ok((4, body)));
        assert_eq!(PoseMessage::decode(&out), Err(NetError));
        assert_eq!(BodyMessage::decode(&out[..out.len() - 1]), Err(NetError));
    }
}
