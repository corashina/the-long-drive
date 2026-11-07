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
/// Source `msgType.carlamps`.
pub const CAR_LAMPS: u8 = 27;
/// Source `msgType.cargears`.
pub const CAR_GEAR: u8 = 28;
/// Source `msgType.carignition`.
pub const CAR_IGNITION: u8 = 29;
/// Source `msgType.radio`.
pub const RADIO: u8 = 34;
/// Source `msgType.radioChannel`.
pub const RADIO_CHANNEL: u8 = 35;
/// Source `msgType.soundplay`.
pub const SOUND: u8 = 45;
/// Source `msgType.time`.
pub const TIME: u8 = 49;
/// Source `msgType.requestData`.
pub const REQUEST: u8 = 8;
/// Source `msgType.inventory`.
pub const INVENTORY: u8 = 15;
/// Source `msgType.partcondition`.
pub const PART_CONDITION: u8 = 20;
/// Source `msgType.flashlight`.
pub const FLASHLIGHT: u8 = 30;
/// Source `msgType.bus`.
pub const BUS: u8 = 32;
/// Source `msgType.food`.
pub const FOOD: u8 = 37;
/// Source `msgType.ammo`.
pub const AMMO: u8 = 38;
/// Source `msgType.weapon`.
pub const WEAPON: u8 = 40;
/// Source `msgType.push`.
pub const PUSH: u8 = 39;
/// Source `msgType.broke`.
pub const BROKE: u8 = 43;
/// Source `msgType.partslot`.
pub const PART_SLOT: u8 = 21;
/// Source `msgType.usable`.
pub const USABLE: u8 = 22;
/// Source `msgType.fluid`.
pub const FLUID: u8 = 23;
/// Source `msgType.tankcap`.
pub const TANK_CAP: u8 = 24;
/// Source `msgType.itemPosUpd`.
pub const ITEM_POSE: u8 = 16;
/// Source `msgType.claim`.
pub const CLAIM: u8 = 18;
/// Source `msgType.velocity`.
pub const VELOCITY: u8 = 19;
/// Source `msgType.removeItem`.
pub const REMOVE: u8 = 48;
/// Port only. The source sends an AI as a save item (`itemPosUpd`). Port rabbits are not items.
pub const CREATURE: u8 = 64;
/// Port only. The source shows no body of a dead peer.
pub const RAGDOLL: u8 = 65;
pub const HEADER: usize = 2;
/// Outbox length prefix bit. With it, `web/net.js` sends the packet on the reliable connection.
pub const RELIABLE: u16 = 0x8000;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct NetError;

/// Source `SPlayerPosUpd`. A player in a car seat sends its pose relative to the car chassis.
/// A player in an item seat sends its world pose. Item keys come with shared items.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Pod, Zeroable)]
pub struct PoseMessage {
    /// Player Transform in world space, or in the car chassis space with `IN_CAR`.
    /// World space does not change with origin shifts.
    pub position: [f64; 3],
    /// The source sends euler angles. The port sends a quaternion.
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
    /// Port only. The player is dead. Peers hide the figure and turn off its colliders.
    pub const DEAD: u32 = 4;
}

/// Source `SCarFloats`: the driver controls. Each peer adds them to its copy of the car.
/// The source sends it reliably. It goes each frame, so the port lets it drop.
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

/// Source `carlamps` layout: the car and `lampak`. Port addition. The source sends it nowhere and
/// `CarUpdateLamps` does nothing, so peers saw other headlight modes.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Pod, Zeroable)]
pub struct CarLampsMessage {
    pub car: u64,
    pub headlights: u32,
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
/// One entry is the source clip index (talk index, or talk count plus music index, or -1 for none) and the clip time.
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

/// Port only: the world poses of the ragdoll bodies of a dead player, in
/// `RagdollReference::bodies` order. One entry is the position, then the rotation quaternion.
pub struct RagdollMessage;

impl RagdollMessage {
    const ENTRY: usize = 40;
    /// Packet length of `n` bodies.
    pub fn len(n: usize) -> usize {
        HEADER + 4 + n * Self::ENTRY
    }
    pub fn encode(
        slot: u8,
        bodies: impl ExactSizeIterator<Item = ([f64; 3], [f32; 4])>,
        out: &mut Vec<u8>,
    ) {
        out.extend_from_slice(&[RAGDOLL, slot]);
        out.extend_from_slice(&(bodies.len() as u32).to_le_bytes());
        for (position, rotation) in bodies {
            out.extend_from_slice(bytemuck::bytes_of(&position));
            out.extend_from_slice(bytemuck::bytes_of(&rotation));
        }
    }
    /// The sender slot and the body poses.
    pub fn decode(
        packet: &[u8],
    ) -> Result<(u8, impl ExactSizeIterator<Item = ([f64; 3], [f32; 4])> + '_), NetError> {
        let [kind, slot, a, b, c, d, body @ ..] = packet else {
            return Err(NetError);
        };
        let n = u32::from_le_bytes([*a, *b, *c, *d]) as usize;
        if *kind != RAGDOLL || body.len() != n * Self::ENTRY {
            return Err(NetError);
        }
        Ok((
            *slot,
            body.chunks_exact(Self::ENTRY).map(|e| {
                (
                    bytemuck::pod_read_unaligned(&e[..24]),
                    bytemuck::pod_read_unaligned(&e[24..]),
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
    /// Source `category` and `prefabId`. A runtime spawn has its item prototype index plus 1, its
    /// serial and its pickup index. They are zero for other items.
    pub spawn: [u32; 3],
    pub pad: u32,
}

/// Source `SRequestData`. A guest asks the host for the state of items that bound.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Pod, Zeroable)]
pub struct RequestMessage {
    pub keys: [u64; RequestMessage::KEYS],
    pub count: u32,
    pub pad: u32,
}

impl RequestMessage {
    pub const KEYS: usize = 32;
    pub fn keys(&self) -> &[u64] {
        &self.keys[..(self.count as usize).min(Self::KEYS)]
    }
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

/// Source `SInventory`: where the sender keeps an item.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Pod, Zeroable)]
pub struct InventoryMessage {
    pub key: u64,
    /// `STORED`, `EQUIPPED`, `PICKED`.
    pub flags: u32,
    pub pad: u32,
}

impl InventoryMessage {
    /// Source `inIninventory != 0`: in an inventory slot, not in a hand.
    pub const STORED: u32 = 1;
    /// Source `inRightHand`.
    pub const EQUIPPED: u32 = 2;
    /// Source `pickedUp`.
    pub const PICKED: u32 = 4;
}

/// Source `STank`: the fluids of a tank, in mix order.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Pod, Zeroable)]
pub struct FluidMessage {
    pub key: u64,
    pub types: [u32; 6],
    pub amounts: [f32; 6],
    pub count: u32,
    pub pad: u32,
}

/// Source `STankCap`.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Pod, Zeroable)]
pub struct TankCapMessage {
    pub key: u64,
    pub valve: f32,
    pub pad: u32,
}

/// Source `SUsable`: a door, lid, lever or switch.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Pod, Zeroable)]
pub struct UsableMessage {
    pub key: u64,
    pub degrees: [f32; 2],
    pub slide: f32,
    pub enabled: u32,
    pub turn_state: u32,
    pub pad: u32,
}

/// Source `SPartSlot` and `SAttachable`: where an item is mounted.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Pod, Zeroable)]
pub struct MountMessage {
    pub key: u64,
    /// The key of the item that owns the slot, or 0.
    pub owner: u64,
    /// `world::source_seed` of the slot or target source.
    pub seed: u64,
    /// `LOOSE`, `PART`, `WHEEL`, `TIRE`, `HITCH`.
    pub kind: u32,
    pub pad: u32,
}

impl MountMessage {
    pub const LOOSE: u32 = 0;
    /// In a part slot.
    pub const PART: u32 = 1;
    /// A rim in a wheel slot.
    pub const WHEEL: u32 = 2;
    /// A tire on rim `owner`.
    pub const TIRE: u32 = 3;
    /// A trailer grip on a hitch target.
    pub const HITCH: u32 = 4;
}

/// Source `SPartCondition`.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Pod, Zeroable)]
pub struct ConditionMessage {
    /// The key of the item that saves the entry, or 0 for a garage surface.
    pub key: u64,
    /// The seed of the entry source, or the garage `ConditionId`.
    pub seed: u64,
    pub state: u32,
    pub state2: u32,
    pub state3: u32,
    pub pad: u32,
    pub color: [f32; 4],
}

/// Source `SFlashlight`: a lamp switch.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Pod, Zeroable)]
pub struct LampMessage {
    /// The seed of a world lamp object, or 0 for garage switch `control`.
    pub key: u64,
    /// Source `Lamp.broken`: bit i is lamp i.
    pub broken: u64,
    pub control: u32,
    pub on: u32,
}

/// Source `SBus`: bit i is door i.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Pod, Zeroable)]
pub struct BusMessage {
    /// The seed of the bus.
    pub key: u64,
    pub opened: u32,
    pub signed: u32,
}

/// Source `SFood`: the bites taken.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Pod, Zeroable)]
pub struct FoodMessage {
    pub key: u64,
    pub bite: u32,
    pub pad: u32,
}

/// Source `SAmmo`: the count of a loose ammo item or a magazine.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Pod, Zeroable)]
pub struct AmmoMessage {
    pub key: u64,
    pub count: f32,
    pub pad: u32,
}

/// Source `SWeapon`: the state of a gun.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Pod, Zeroable)]
pub struct WeaponMessage {
    pub key: u64,
    pub ammo: f32,
    pub fr: f32,
    pub chambers: u32,
    pub cylinder: i32,
    pub power: u32,
    pub has_mag: u32,
}

/// Source `SPush`. A player pushed an item that another peer owns.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Pod, Zeroable)]
pub struct PushMessage {
    pub key: u64,
    /// World impulse.
    pub impulse: [f32; 3],
    /// The point in the item frame.
    pub point: [f32; 3],
}

/// Source `SBroke`. A guest asks the host to break an item.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Pod, Zeroable)]
pub struct BrokeMessage {
    pub key: u64,
    /// The breakable record index.
    pub record: u32,
    pub pad: u32,
}

/// Source `SRemoveItem`.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Pod, Zeroable)]
pub struct RemoveMessage {
    pub key: u64,
}

/// The host pose of one rabbit (source item pose of an `aiscript` item).
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Pod, Zeroable)]
pub struct CreatureMessage {
    /// `RabbitSnapshot::id`.
    pub id: u32,
    pub alive: u32,
    pub size: f32,
    pub pad: u32,
    /// World space. It does not change with origin shifts.
    pub position: [f64; 3],
    pub rotation: [f32; 4],
    pub velocity: [f32; 3],
    pub pad2: u32,
}

/// Source `SRadio`: the controls of one radio item.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Pod, Zeroable)]
pub struct RadioMessage {
    /// `NetKey` of the radio item.
    pub key: u64,
    pub tuning: f32,
    pub volume: f32,
    pub on: u32,
    pub am: u32,
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
impl Message for CarLampsMessage {
    const TYPE: u8 = CAR_LAMPS;
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
impl Message for CreatureMessage {
    const TYPE: u8 = CREATURE;
}
impl Message for RadioMessage {
    const TYPE: u8 = RADIO;
    const RELIABLE: bool = true;
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
impl Message for FluidMessage {
    const TYPE: u8 = FLUID;
    const RELIABLE: bool = true;
}
impl Message for TankCapMessage {
    const TYPE: u8 = TANK_CAP;
    const RELIABLE: bool = true;
}
impl Message for UsableMessage {
    const TYPE: u8 = USABLE;
    const RELIABLE: bool = true;
}
impl Message for RequestMessage {
    const TYPE: u8 = REQUEST;
    const RELIABLE: bool = true;
}
impl Message for InventoryMessage {
    const TYPE: u8 = INVENTORY;
    const RELIABLE: bool = true;
}
impl Message for MountMessage {
    const TYPE: u8 = PART_SLOT;
    const RELIABLE: bool = true;
}
impl Message for ConditionMessage {
    const TYPE: u8 = PART_CONDITION;
    const RELIABLE: bool = true;
}
impl Message for LampMessage {
    const TYPE: u8 = FLASHLIGHT;
    const RELIABLE: bool = true;
}
impl Message for BusMessage {
    const TYPE: u8 = BUS;
    const RELIABLE: bool = true;
}
impl Message for FoodMessage {
    const TYPE: u8 = FOOD;
    const RELIABLE: bool = true;
}
impl Message for AmmoMessage {
    const TYPE: u8 = AMMO;
    const RELIABLE: bool = true;
}
impl Message for WeaponMessage {
    const TYPE: u8 = WEAPON;
    const RELIABLE: bool = true;
}
impl Message for PushMessage {
    const TYPE: u8 = PUSH;
}
impl Message for BrokeMessage {
    const TYPE: u8 = BROKE;
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
        let bodies = [
            ([-4e5, 1., 2.], [0., 0.6, 0., 0.8]),
            ([3., 4., 5.], [0., 0., 0., 1.]),
        ];
        RagdollMessage::encode(5, bodies.into_iter(), &mut out);
        assert_eq!(out.len(), RagdollMessage::len(2));
        let (slot, got) = RagdollMessage::decode(&out).unwrap();
        assert_eq!((slot, got.collect::<Vec<_>>()), (5, bodies.to_vec()));
        assert!(RagdollMessage::decode(&out[..out.len() - 1]).is_err());
        out.clear();
        let item = ItemPoseMessage {
            key: u64::MAX - 7,
            position: [-1e7, 2., 3.],
            rotation: [0., 0., 0., 1.],
            spawn: [4, 9, 0],
            pad: 0,
        };
        item.encode(5, &mut out);
        assert_eq!(ItemPoseMessage::decode(&out), Ok((5, item)));
        assert_eq!(ClaimMessage::decode(&out), Err(NetError));
    }
}
