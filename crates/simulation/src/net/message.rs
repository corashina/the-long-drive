//! Player messages of source `sns`. A packet is the type byte, the sender slot byte, then the fixed body.
//! `web/net.js` reads only the two header bytes.
use bytemuck::{Pod, Zeroable};

/// Source `msgType.playerPosUpd`.
pub const POSE: u8 = 10;
/// Source `msgType.playerAnimUpd`.
pub const BODY: u8 = 13;
/// Source `msgType.radioChannel`.
pub const RADIO_CHANNEL: u8 = 35;
/// Source `msgType.soundplay`.
pub const SOUND: u8 = 45;
/// Source `msgType.time`.
pub const TIME: u8 = 49;
/// Source `msgType.itemPosUpd`.
pub const ITEM_POSE: u8 = 16;
/// Source `msgType.claim`.
pub const CLAIM: u8 = 18;
/// Source `msgType.velocity`.
pub const VELOCITY: u8 = 19;
/// Source `msgType.removeItem`.
pub const REMOVE: u8 = 48;
pub const HEADER: usize = 2;
/// Outbox length prefix bit: `web/net.js` sends the packet on the reliable connection.
pub const RELIABLE: u16 = 0x8000;

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

/// Source `SendTime`: the host `napszak.t`.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Pod, Zeroable)]
pub struct TimeMessage {
    /// `CycleState::cycle_seconds`.
    pub cycle_seconds: f64,
}

/// Source `SSound`: a fart, shit or burp at a world position.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Pod, Zeroable)]
pub struct SoundMessage {
    /// World space. It does not change with origin shifts.
    pub position: [f64; 3],
    /// `AudioKind`.
    pub kind: u32,
    pub clip: u32,
    pub gain: f32,
    pub pitch: f32,
}

/// Source `SRadioChannel`: the clip of each station, in reference station order. FM and AM share one list.
/// One entry is the source clip index (talk index, or talk count plus music index; -1 for none) and the clip time.
pub struct RadioChannelMessage;

impl RadioChannelMessage {
    /// Appends the packet body of `stations` after the header.
    pub fn encode(
        slot: u8,
        stations: impl ExactSizeIterator<Item = (i32, f32)>,
        out: &mut Vec<u8>,
    ) {
        out.extend_from_slice(&[RADIO_CHANNEL, slot]);
        out.extend_from_slice(&(stations.len() as u32).to_le_bytes());
        for (clip, offset) in stations {
            out.extend_from_slice(&clip.to_le_bytes());
            out.extend_from_slice(&offset.to_le_bytes());
        }
    }
    /// The sender slot and the station entries.
    pub fn decode(
        packet: &[u8],
    ) -> Result<(u8, impl ExactSizeIterator<Item = (i32, f32)> + '_), NetError> {
        let [kind, slot, a, b, c, d, body @ ..] = packet else {
            return Err(NetError);
        };
        let n = u32::from_le_bytes([*a, *b, *c, *d]) as usize;
        if *kind != RADIO_CHANNEL || body.len() != n * 8 {
            return Err(NetError);
        }
        Ok((
            *slot,
            body.chunks_exact(8).map(|e| {
                (
                    i32::from_le_bytes(e[..4].try_into().unwrap()),
                    f32::from_le_bytes(e[4..].try_into().unwrap()),
                )
            }),
        ))
    }
}

/// Appends `message` to an outbox with its length prefix.
pub fn push<M: Message>(slot: u8, message: &M, out: &mut Vec<u8>) {
    push_with(M::RELIABLE, HEADER + size_of::<M>(), out, |out| {
        message.encode(slot, out)
    });
}

/// Appends a packet of `n` bytes that `encode` writes, with its length prefix.
pub fn push_with(reliable: bool, n: usize, out: &mut Vec<u8>, encode: impl FnOnce(&mut Vec<u8>)) {
    debug_assert!(n < usize::from(RELIABLE));
    let flag = if reliable { RELIABLE } else { 0 };
    out.extend_from_slice(&(n as u16 | flag).to_le_bytes());
    let start = out.len();
    encode(out);
    debug_assert_eq!(out.len() - start, n);
}

/// Source `SItemPosUpd`: the world pose of an item, sent by its owner.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Pod, Zeroable)]
pub struct ItemPoseMessage {
    pub key: u64,
    /// Item pose in world space. It does not change with origin shifts.
    pub position: [f64; 3],
    pub rotation: [f32; 4],
}

/// Source `SItemVelocityUpd`.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Pod, Zeroable)]
pub struct VelocityMessage {
    pub key: u64,
    pub linear: [f32; 3],
    pub angular: [f32; 3],
}

/// Source `SClaim`.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Pod, Zeroable)]
pub struct ClaimMessage {
    pub key: u64,
    /// 1 claims, 0 releases.
    pub claim: u32,
    pub pad: u32,
}

/// Source `SRemoveItem`.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Pod, Zeroable)]
pub struct RemoveMessage {
    pub key: u64,
}

pub trait Message: Pod {
    const TYPE: u8;
    /// Source `EP2PSendReliable`. Other messages can drop.
    const RELIABLE: bool = false;
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
impl Message for TimeMessage {
    const TYPE: u8 = TIME;
}
impl Message for SoundMessage {
    const TYPE: u8 = SOUND;
    const RELIABLE: bool = true;
}
impl Message for ItemPoseMessage {
    const TYPE: u8 = ITEM_POSE;
}
impl Message for VelocityMessage {
    const TYPE: u8 = VELOCITY;
}
impl Message for ClaimMessage {
    const TYPE: u8 = CLAIM;
    const RELIABLE: bool = true;
}
impl Message for RemoveMessage {
    const TYPE: u8 = REMOVE;
    const RELIABLE: bool = true;
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
        out.clear();
        let sound = SoundMessage {
            position: [-4e5, 1., 2.],
            kind: 2,
            clip: 77,
            gain: 0.8,
            pitch: 1.1,
        };
        sound.encode(0, &mut out);
        assert_eq!(SoundMessage::decode(&out), Ok((0, sound)));
        out.clear();
        let stations = [(3, 12.5), (-1, 0.), (40, 0.25)];
        RadioChannelMessage::encode(0, stations.into_iter(), &mut out);
        let (slot, got) = RadioChannelMessage::decode(&out).unwrap();
        assert_eq!((slot, got.collect::<Vec<_>>()), (0, stations.to_vec()));
        assert!(RadioChannelMessage::decode(&out[..out.len() - 1]).is_err());
        out.clear();
        let item = ItemPoseMessage {
            key: u64::MAX - 7,
            position: [-1e7, 2., 3.],
            rotation: [0., 0., 0., 1.],
        };
        item.encode(5, &mut out);
        assert_eq!(ItemPoseMessage::decode(&out), Ok((5, item)));
        assert_eq!(ClaimMessage::decode(&out), Err(NetError));
    }
}
