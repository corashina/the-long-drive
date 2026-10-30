use super::{InstanceId, SaveError, SourceKey};
use serde::{Deserialize, Serialize};

pub const SAVE_SCHEMA_VERSION: u32 = 40;
pub const OLDEST_SAVE_SCHEMA_VERSION: u32 = 40;
pub fn schema_supported(version: u32) -> bool {
    (OLDEST_SAVE_SCHEMA_VERSION..=SAVE_SCHEMA_VERSION).contains(&version)
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SourceHash {
    pub file: String,
    pub sha256: String,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SourceStamp {
    pub app_id: u32,
    pub build_id: u32,
    pub manifest_sha256: String,
    pub source_hashes: Vec<SourceHash>,
}
impl SourceStamp {
    /// The save stamp of the build that `reference` comes from.
    pub fn new(reference: &crate::reference::ReferenceManifest, manifest_sha256: String) -> Self {
        Self {
            app_id: reference.app_id,
            build_id: reference.build_id,
            manifest_sha256,
            source_hashes: reference
                .source_hashes
                .iter()
                .map(|s| SourceHash {
                    file: s.file.clone(),
                    sha256: s.sha256.clone(),
                })
                .collect(),
        }
    }
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SaveSnapshot {
    pub driving_assists: crate::reference::DrivingAssists,
    pub schema_version: u32,
    pub saved_at: String,
    pub source: SourceStamp,
    pub world: WorldSnapshot,
    pub player: PlayerSnapshot,
    pub needs: NeedsSnapshot,
    pub hearing: HearingSnapshot,
    pub inventory: InventorySnapshot,
    pub bodies: Vec<BodySnapshot>,
    pub items: Vec<ItemSnapshot>,
    pub attachments: AttachmentsSnapshot,
    pub mechanisms: MechanismsSnapshot,
    pub progress: ProgressSnapshot,
    pub lighting: LightingSnapshot,
    pub conditions: ConditionSnapshot,
    /// Runtime item prototype copies.
    pub spawned: Vec<SpawnedItemSnapshot>,
    /// Runtime food copies.
    pub foods: Vec<FoodItemSnapshot>,
    /// Pack counts and cigarette burn state.
    pub cigarettes: CigarettesSnapshot,
    /// Source save_sandstorm and the storm spawner timer.
    pub weather: WeatherSnapshot,
    /// Spawner rabbits. The spawner timer is not saved: the source coroutine restarts.
    pub rabbits: Vec<RabbitSnapshot>,
}
/// Source save_prefab pose and save_nyulscript.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RabbitSnapshot {
    pub pose: AbsolutePose,
    pub size: f32,
    /// `breakable.shootHealth`.
    pub shoot_health: f32,
    pub alive: bool,
    pub eyes: bool,
    pub ears: bool,
    /// `save_ragdollpos`: the bone body poses of a dead rabbit, in body order. Empty while alive.
    pub bones: Vec<AbsolutePose>,
}
/// Source sandstormLastTime (negated) and sandstormNextTime.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WeatherSnapshot {
    /// Seconds since the last spawn.
    pub since_spawn: f32,
    /// Seconds from the last spawn to the next spawn.
    pub wait: f32,
    pub storms: Vec<StormSnapshot>,
}
/// Source save_sandstorm: pos, euler rot y, prefabid, life, size, speed, xdir and ydir.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StormSnapshot {
    pub position: [f64; 3],
    pub yaw: f32,
    pub prefab: u32,
    pub life: f32,
    pub size: f32,
    pub speed: f32,
    pub direction: [f32; 2],
}
/// Spawned items are named by their spawn serial.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CigarettesSnapshot {
    pub packs: Vec<PackSnapshot>,
    pub cigarettes: Vec<CigaretteSnapshot>,
}
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PackSnapshot {
    pub serial: u32,
    pub count: u32,
}
/// Source `ediblescript` hp and ateOnce.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CigaretteSnapshot {
    pub serial: u32,
    pub hp: f32,
    pub ate_once: bool,
}
/// One spawned prototype pickup body, saved in the world at its pose.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SpawnedItemSnapshot {
    pub serial: u32,
    pub prototype: SourceKey,
    pub pickup: u32,
    pub pose: AbsolutePose,
    /// Gun state; None at its spawn state.
    pub gun: Option<crate::interaction::guns::GunSnapshot>,
    /// Magazine rounds; None at the prefab count.
    pub ammo: Option<f32>,
}
/// One runtime food copy, saved in the world at its pose.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FoodItemSnapshot {
    pub source: SourceKey,
    pub bite: u8,
    pub pose: AbsolutePose,
}
impl SaveSnapshot {
    pub fn from_json(bytes: &[u8]) -> Result<Self, SaveError> {
        #[derive(Deserialize)]
        struct Version {
            schema_version: u32,
        }
        let version: Version =
            serde_json::from_slice(bytes).map_err(|e| SaveError::CorruptData(e.to_string()))?;
        if !schema_supported(version.schema_version) {
            return Err(SaveError::IncompatibleSources("Schema version".into()));
        }
        serde_json::from_slice(bytes).map_err(|e| SaveError::CorruptData(e.to_string()))
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AbsolutePose {
    pub position: [f64; 3],
    pub rotation: [f32; 4],
}
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LocalPose {
    pub position: [f32; 3],
    pub rotation: [f32; 4],
}
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum BodyIdentity {
    Source { source: SourceKey },
    Assembly { root: InstanceId },
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BodySnapshot {
    pub identity: BodyIdentity,
    pub pose: AbsolutePose,
    pub linear_velocity: [f32; 3],
    pub angular_velocity: [f32; 3],
    pub sleeping: bool,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorldSnapshot {
    pub generation: crate::world::GenerationSnapshot,
    pub seed: i32,
    /// Session date as year, month, day. Load regenerates the world with it.
    pub date: [i32; 3],
    /// The picked start car. Load spawns its start placement.
    pub start_car: crate::world::StartCar,
    #[serde(with = "super::identity::lossless")]
    pub changes: crate::world::ChangeRecords,
    #[serde(with = "super::identity::lossless")]
    pub spawn: crate::world::spawn_state::SpawnHistory,
    /// Broken generated boxes. Their removal is in `changes`.
    #[serde(with = "super::identity::lossless")]
    pub boxes: Vec<crate::world::BoxRelease>,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PlayerSnapshot {
    pub pose: AbsolutePose,
    pub yaw: f32,
    pub pitch: f32,
    pub seat: Option<SeatSnapshot>,
    pub posture: f32,
    pub vertical_velocity: f32,
    pub recoil_velocity: [f32; 3],
    /// Horizontal walk and air velocity.
    pub momentum: [f32; 3],
    pub running: bool,
    pub carry: Option<CarrySnapshot>,
    /// Source save_player.sleep: seconds since the last wake.
    pub since_wake: f64,
}
/// Source save_player hp, food, water, shit, to* queue, and bladder fluids.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NeedsSnapshot {
    pub needs: crate::reference::NeedValues,
    pub pending: crate::reference::ConsumptionEffect,
    pub bladder: crate::fluids::FluidSnapshot,
}
/// Source save_player hearing, earRinging, earRingingTarget and earRingDir.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HearingSnapshot {
    pub hearing: f32,
    pub ringing: f32,
    pub target: f32,
    pub dir: f32,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SeatSnapshot {
    pub owner: SeatOwnerSnapshot,
    /// Car seat record or world item main seatscript.
    pub source: SourceKey,
    /// sitPos relative to the owner body.
    pub local_pose: LocalPose,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum SeatOwnerSnapshot {
    /// A car.
    Vehicle {
        #[serde(with = "super::identity::lossless")]
        vehicle: crate::world::ObjectKey,
    },
    Item {
        #[serde(with = "super::identity::lossless")]
        item: crate::world::ObjectKey,
    },
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CarrySnapshot {
    pub item: InstanceId,
    pub offset: [f32; 3],
    pub rotation: [f32; 4],
    pub local_grip: LocalPose,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InventorySnapshot {
    pub slots: [Option<InstanceId>; 4],
    pub selected: u8,
    pub equipped: Option<InstanceId>,
    /// Worn items, indexed by `wearableType`.
    pub worn: [Option<WornSnapshot>; 5],
}
/// One worn item.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum WornSnapshot {
    /// A source or generated item.
    Item { item: InstanceId },
    /// A spawned item, named by its spawn serial.
    Spawned { serial: u32 },
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FoodSnapshot {
    pub source: SourceKey,
    pub bite: u8,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
/// Source `enginescript` state of one engine part object.
pub struct EngineSnapshot {
    pub rpm: f32,
    pub temperature: f32,
    pub elapsed: f32,
    pub last_run_time: f32,
    pub last_start_chance_time: f32,
    pub random_start: f32,
    pub supplied_fuel: f32,
    pub oil_penalty: f32,
    pub fuel_suitability: f32,
    pub heat_penalty: f32,
    pub start_draw: String,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ItemSnapshot {
    pub instance: InstanceId,
    pub pose: AbsolutePose,
    /// Offset in the owning body or attachment frame.
    pub local_pose: LocalPose,
    pub location: ItemLocationSave,
    pub temperature: Option<f32>,
    pub fluids: Option<Vec<crate::fluids::FluidEntry>>,
    pub food: Option<FoodSnapshot>,
    pub removed: bool,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum ItemLocationSave {
    World,
    Held,
    Stored {
        slot: u8,
    },
    Equipped,
    PartSlot {
        slot: SourceKey,
    },
    Attached {
        owner: InstanceId,
    },
    GeneratedAttachment {
        #[serde(with = "super::identity::lossless")]
        attachment: crate::world::AttachmentSnapshot,
    },
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AttachmentsSnapshot {
    pub parts: Vec<PartAttachment>,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PartAttachment {
    pub slot: SourceKey,
    pub part: Option<InstanceId>,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MechanismsSnapshot {
    pub controls: Vec<MechanismSnapshot>,
    pub openings: Vec<OpeningSnapshot>,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MechanismSnapshot {
    pub source: SourceKey,
    pub degrees: [f32; 2],
    pub slide: f32,
    pub enabled: bool,
    pub turn_state: u8,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OpeningSnapshot {
    pub source: SourceKey,
    pub valve: f32,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProgressSnapshot {
    /// The last seated car.
    #[serde(with = "super::identity::lossless")]
    pub selected_vehicle: Option<crate::world::ObjectKey>,
    pub start_attempts: String,
    /// Source DistanceWrite: km the player drove as driver.
    pub driven_km: f64,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LightingSnapshot {
    pub cycle: CycleSnapshot,
    pub controllers: Vec<ControllerSnapshot>,
}
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CycleSnapshot {
    pub elapsed_seconds: f64,
    pub cycle_seconds: f64,
    pub evaluated_cycle_seconds: f64,
    pub cloud_coordinate: f32,
    pub star_rotation: f32,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ControllerSnapshot {
    pub source: SourceKey,
    pub current_state: usize,
    pub requested_state: usize,
    pub refreshed_state: usize,
    pub energy: f32,
    pub enabled: bool,
    pub on: bool,
    pub switch_motion: Option<SwitchMotionSave>,
    pub switch_poses: Vec<SwitchPoseSnapshot>,
}
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum SwitchMotionSave {
    Initial,
    Moving { progress: f32 },
}
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RadioSnapshot {
    pub on: bool,
    pub am: bool,
    pub tuning: f32,
    pub volume: f32,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SwitchPoseSnapshot {
    pub node: SourceKey,
    pub pose: LocalPose,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConditionSnapshot {
    pub surfaces: Vec<SurfaceSnapshot>,
    pub washes: Vec<WashSnapshot>,
    pub tools: Vec<ToolSnapshot>,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SurfaceSnapshot {
    pub source: SourceKey,
    pub state: u8,
    pub state2: u32,
    pub state3: u32,
    pub color: [f32; 4],
    pub repair_progress: f32,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WashSnapshot {
    pub source: SourceKey,
    pub active: bool,
    pub on: bool,
    pub destroyed: bool,
    pub repair_progress: f32,
}
/// The live state of a loot tool: paint left, paint color, worn out.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ToolStateSnapshot {
    pub quantity: Option<f32>,
    pub color: Option<[f32; 4]>,
    pub disabled: bool,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ToolSnapshot {
    pub source: SourceKey,
    pub quantity: Option<f32>,
    pub color: Option<[f32; 4]>,
    pub disabled: bool,
}

#[cfg(test)]
mod g04_ordered_state {
    use super::*;

    #[test]
    fn older_schemas_are_rejected_before_payload_reconstruction() {
        for version in [
            6, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 23, 24, 25, 26, 27, 28, 29, 30,
            31, 32, 33, 34, 35, 36, 37, 38, 39, 41,
        ] {
            let payload = format!(r#"{{"schema_version":{version}}}"#);
            assert!(matches!(
                SaveSnapshot::from_json(payload.as_bytes()),
                Err(SaveError::IncompatibleSources(_))
            ));
        }
    }

    #[test]
    fn current_schema_passes_the_version_gate() {
        // Schema 40 reaches payload parsing; the empty payload then fails as corrupt data.
        assert!(matches!(
            SaveSnapshot::from_json(br#"{"schema_version":40}"#),
            Err(SaveError::CorruptData(_))
        ));
    }

    #[test]
    fn dead_rabbit_bones_round_trip() {
        let pose = |x: f64| AbsolutePose {
            position: [1e6 + x, 2., -3.],
            rotation: [0., 0.6, 0., 0.8],
        };
        let rabbit = RabbitSnapshot {
            pose: pose(0.),
            size: 0.4,
            shoot_health: 1.5,
            alive: false,
            eyes: false,
            ears: true,
            bones: (0..14).map(|i| pose(f64::from(i) * 0.01)).collect(),
        };
        let json = serde_json::to_string(&rabbit).unwrap();
        assert_eq!(
            serde_json::from_str::<RabbitSnapshot>(&json).unwrap(),
            rabbit
        );
    }

    #[test]
    fn worn_hat_and_cigarette_round_trip() {
        let hat = InstanceId::Generated {
            id: crate::world::GeneratedId {
                layer: crate::world::WorldLayer::Building,
                generator: crate::reference::SourceId {
                    file: "level0".into(),
                    path_id: i64::MIN + 1,
                },
                unit: [3, -4],
                ordinal: 5,
                children: vec![],
            },
        };
        let inventory = InventorySnapshot {
            slots: [None, None, None, None],
            selected: 0,
            equipped: None,
            worn: [
                None,
                Some(WornSnapshot::Item { item: hat }),
                None,
                None,
                Some(WornSnapshot::Spawned { serial: 3 }),
            ],
        };
        let json = serde_json::to_string(&inventory).unwrap();
        assert_eq!(
            serde_json::from_str::<InventorySnapshot>(&json).unwrap(),
            inventory
        );
    }

    #[test]
    fn seat_snapshot_round_trips_both_owners() {
        let source = SourceKey {
            file: "level0".into(),
            path_id: "-9223372036854775807".into(),
        };
        let local_pose = LocalPose {
            position: [0.1, 0.5, -0.2],
            rotation: [0., 0., 0., 1.],
        };
        let generated = crate::world::GeneratedId {
            layer: crate::world::WorldLayer::Nested,
            generator: crate::reference::SourceId {
                file: "level0".into(),
                path_id: i64::MIN + 1,
            },
            unit: [i64::MAX - 1, -3],
            ordinal: 7,
            children: vec![],
        };
        for owner in [
            SeatOwnerSnapshot::Vehicle {
                vehicle: crate::world::ObjectKey::Generated(generated.clone()),
            },
            SeatOwnerSnapshot::Item {
                item: crate::world::ObjectKey::Generated(generated),
            },
        ] {
            let seat = SeatSnapshot {
                owner,
                source: source.clone(),
                local_pose,
            };
            let json = serde_json::to_string(&seat).unwrap();
            assert_eq!(serde_json::from_str::<SeatSnapshot>(&json).unwrap(), seat);
        }
    }
}
