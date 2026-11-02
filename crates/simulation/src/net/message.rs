//! Player messages of source `sns`. A packet is the type byte, the sender slot byte, then the fixed body.
//! `web/net.js` reads only the two header bytes.
use bytemuck::{Pod, Zeroable};

/// Source `msgType.playerPosUpd`.
pub const POSE: u8 = 10;
/// Source `msgType.playerAnimUpd`.
pub const BODY: u8 = 13;
/// Source `msgType.carfloats`.
pub const CAR_FLOATS: u8 = 25;
/// Source `msgType.carhandbrake`.
pub const CAR_HANDBRAKE: u8 = 26;
/// Source `msgType.cargears`.
pub const CAR_GEAR: u8 = 28;
/// Source `msgType.carignition`.
pub const CAR_IGNITION: u8 = 29;
pub const HEADER: usize = 2;
/// Outbox length prefix bit: `web/net.js` sends the packet on the reliable connection.
pub const RELIABLE: u16 = 0x8000;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct NetError;

/// Source `SPlayerPosUpd`. A player in a car seat sends its pose relative to the car chassis.
/// A player in an item seat sends its world pose: item keys come with shared items.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Pod, Zeroable)]
pub struct PoseMessage {
    /// Player Transform in world space, or in the car chassis space with `IN_CAR`.
    /// World space does not change with origin shifts.
    pub position: [f64; 3],
    /// Source sends euler angles. The port sends a quaternion.
    pub rotation: [f32; 4],
    /// Source `duckValue`.
    pub duck: f32,
    pub flags: u32,
    /// Source `seatId`: `ObjectKey::seed` of the car with `IN_CAR`.
    pub car: u64,
    /// Source `seatId2`: the index in `WorldVehicleReference::seats` with `IN_CAR`.
    pub seat: u32,
    pub pad: u32,
}

impl PoseMessage {
    pub const SEATED: u32 = 1;
    /// The player sits in a car seat. `car` and `seat` name it.
    pub const IN_CAR: u32 = 2;
}

/// Source `SCarFloats`: the driver controls. Each peer adds them to its copy of the car.
/// Source sends it reliable. It goes each frame, so the port lets it drop.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Pod, Zeroable)]
pub struct CarFloatsMessage {
    /// `ObjectKey::seed` of the car.
    pub car: u64,
    /// Port steer: positive steers left.
    pub steer: f32,
    pub horn: f32,
    pub gas: f32,
    pub brake: f32,
    pub clutch: f32,
    pub bike_gas: f32,
}

/// Source `SCarHandbrake`.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Pod, Zeroable)]
pub struct CarHandbrakeMessage {
    pub car: u64,
    pub handbrake: f32,
    pub pad: u32,
}

/// Source `SCarGear`. The port gear index: source `gear + 1`.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Pod, Zeroable)]
pub struct CarGearMessage {
    pub car: u64,
    pub gear: u32,
    pub pad: u32,
}

/// Source `SCarIgnition`.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Pod, Zeroable)]
pub struct CarIgnitionMessage {
    pub car: u64,
    pub flags: u32,
    pub rpm: f32,
}

impl CarIgnitionMessage {
    pub const IGNITION: u32 = 1;
    pub const STARTING: u32 = 2;
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
impl Message for CarFloatsMessage {
    const TYPE: u8 = CAR_FLOATS;
}
impl Message for CarHandbrakeMessage {
    const TYPE: u8 = CAR_HANDBRAKE;
    const RELIABLE: bool = true;
}
impl Message for CarGearMessage {
    const TYPE: u8 = CAR_GEAR;
    const RELIABLE: bool = true;
}
impl Message for CarIgnitionMessage {
    const TYPE: u8 = CAR_IGNITION;
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
            flags: PoseMessage::SEATED | PoseMessage::IN_CAR,
            car: u64::MAX - 3,
            seat: 2,
            pad: 0,
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
