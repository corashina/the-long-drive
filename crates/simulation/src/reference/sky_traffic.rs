use super::*;
/// Source AirPlaneSpawnScript on Main. Ranges are [min, max].
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SkyReference {
    pub source: SourceId,
    /// Raw prefab chances in prefab order. The source picks by cumulative chance.
    pub chances: Vec<f32>,
    pub max_planes: u32,
    pub spawn_time_range: [f32; 2],
    pub spawn_distance: f32,
    /// Anchor height above the player.
    pub height: f32,
    pub speed: f32,
    /// Degrees of yaw from the source +Z axis.
    pub max_angle: f32,
    pub destroy_distance: f32,
    pub contrail_rate: f32,
    pub prefabs: Vec<SkyPrefab>,
    pub evidence: ManagedEvidence,
    pub ground_ufo: GroundUfoReference,
    pub flyer: FlyerReference,
}
/// Source fedospawnscript on the Player and the fedoscript AI of its prefab.
/// Distances are source meters. Ranges are [min, max].
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GroundUfoReference {
    pub source: SourceId,
    /// Index into `SkyReference::prefabs`.
    pub prefab: usize,
    /// Nights to the next spawn chance. The max is inclusive.
    pub night_range: [i32; 2],
    pub nights_after_spawn: i32,
    pub chance: f32,
    /// Spawn distance behind the player along source -Z.
    pub back_range: [f32; 2],
    /// Spawn offset range to each side.
    pub side_distance: f32,
    pub up_distance: f32,
    /// fedoscript on the prefab root.
    pub fedo: SourceId,
    pub speed: f32,
    pub high_speed: f32,
    /// Source Start squares it.
    pub target_distance: f32,
    pub road_height_offset: f32,
    pub ai_distance_range: [f32; 2],
    pub ai_multiplier_range: [f32; 2],
    /// Source Start adds `road_height_offset`.
    pub visited_distance: f32,
    pub disappear_distance: f32,
    /// Rigidbody mass. The body has no gravity and frozen rotation.
    pub mass: f32,
    /// Unity `collisionDetectionMode` is not Discrete. The body uses CCD.
    pub ccd: bool,
    /// fedoscript start `speedlerp`.
    pub speed_lerp: f32,
    /// Colliders in root space, port coordinates.
    pub colliders: Vec<UfoCollider>,
    pub seat: UfoSeat,
    /// The stimulscript on the root sphere collider.
    pub stimulus: UfoStimulus,
    pub evidence: ManagedEvidence,
}
/// The use of one Fedo01 collider.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UfoRole {
    Body,
    /// The seatscript collider.
    Seat,
    /// fedoscript `enableFollowRoad` usable: turn.
    AutoPilot,
    /// fedoscript `setHighSpeed` usable: tune.
    Speed,
    /// fedoscript `setAltitude` usable: tune.
    Altitude,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UfoCollider {
    pub source: SourceId,
    pub role: UfoRole,
    /// Under the fedoscript `rot` Transform: it turns with the rotor spin.
    pub spin: bool,
    /// Under fedoscript `seatedObj`: active only while the seat is in use.
    pub seated: bool,
    pub membership: u32,
    pub filter: u32,
    pub geometry: ColliderGeometry,
}
/// The Fedo01 seatscript. Positions are root space, port coordinates.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UfoSeat {
    pub source: SourceId,
    /// The seatscript Transform.
    pub node: [f32; 3],
    /// `sitPos`.
    pub sit: [f32; 3],
    pub rot_limit: f32,
    pub yaw_unlimited: bool,
    pub look_instead_of_lean: bool,
    pub root_origin: bool,
    pub default_pitch: f32,
    pub min_exit_angle: f32,
    pub exit_height: f32,
    pub show_sleep_text: bool,
    pub cant_push_from_seat: bool,
    pub push_force_modifier: f32,
    pub def_pose: u32,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UfoStimulus {
    /// The stimulscript. `CreaturesReference::stimuli` holds its values.
    pub source: SourceId,
    /// Sphere collider center in root space, port coordinates.
    pub center: [f32; 3],
    pub radius: f32,
}
/// Source randomFlyer on RandomBat, a world prototype. Ranges are [min, max].
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FlyerReference {
    pub source: SourceId,
    /// RandomBat root Transform: the world prototype source.
    pub prefab: SourceId,
    /// World prototype asset.
    pub asset: AssetId,
    /// tosaveitemscript id of a live bat item.
    pub item_id: u32,
    /// Seconds to the next target.
    pub time_range: [f32; 2],
    /// Target distance from the bat.
    pub range: [f32; 2],
    pub max_range_from_start: f32,
    pub speed_range: [f32; 2],
    pub velocity_lerp: f32,
    pub rot_lerp: f32,
    /// Seconds to the next squeak.
    pub sound_time_range: [f32; 2],
    pub volume_range: [f32; 2],
    pub pitch_range: [f32; 2],
    /// Squeak audio clip IDs.
    pub clips: Vec<u32>,
    /// One-shot car voice template of mainscript.PlayClipAtPoint for the squeaks.
    pub voice: u32,
    /// Looping wing clip. Track nodes are prototype node indices.
    pub wing: MotionClip,
}
impl FlyerReference {
    #[cfg(feature = "validate")]
    pub fn validate(&self, m: &ReferenceManifest) -> Result<(), ReferenceError> {
        let ranges = [
            self.time_range,
            self.range,
            self.speed_range,
            self.sound_time_range,
            self.volume_range,
            self.pitch_range,
        ];
        require(
            ranges
                .iter()
                .all(|r| finite(r) && 0. <= r[0] && r[0] <= r[1])
                && [self.max_range_from_start, self.velocity_lerp, self.rot_lerp]
                    .iter()
                    .all(|v| v.is_finite() && *v > 0.),
            "flyer values",
        )?;
        let audio = &m.audio;
        require(
            !self.clips.is_empty() && self.clips.iter().all(|id| audio.has_clip(*id)),
            "flyer clips",
        )?;
        require(
            audio.car_voice(self.voice).is_some_and(|v| v.one_shot),
            "flyer voice",
        )?;
        let prototype = m.world.prototypes.get(&self.prefab);
        require(
            prototype.is_some_and(|p| p.asset == self.asset),
            "flyer prototype",
        )?;
        let nodes = prototype.map_or(0, |p| p.nodes.len());
        let w = &self.wing;
        require(
            w.r#loop
                && w.start_time.is_finite()
                && w.stop_time.is_finite()
                && 0. <= w.start_time
                && w.start_time < w.stop_time
                && w.sample_rate > 0.
                && w.events.is_empty()
                && !w.tracks.is_empty(),
            "flyer wing clip",
        )?;
        require(
            w.tracks
                .iter()
                .all(|t| (t.node as usize) < nodes && t.keys_valid(w.stop_time)),
            "flyer wing tracks",
        )
    }
}
/// One AirPlanePrefabs entry, exported as a sky_traffic asset.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SkyPrefab {
    /// Prefab root Transform.
    pub source: SourceId,
    pub asset: AssetId,
    pub root: NodeId,
    /// First active ParticleSystem. It simulates in the anchor space.
    pub contrail: Option<ParticleReference>,
    /// Audio voice IDs of the prefab AudioSources.
    pub voices: Vec<u32>,
    pub spin: Option<SkySpin>,
    /// fedoscript `seatedObj`: shown only while the seat is in use.
    pub seated: Option<NodeId>,
    /// The empscript on the prefab root.
    pub emp: Option<crate::electronics::EmpReference>,
}
/// Source fedoscript rotor spin.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SkySpin {
    pub node: NodeId,
    /// Port space.
    pub axis: [f32; 3],
    pub degrees_per_second: f32,
}
impl SkyReference {
    #[cfg(feature = "validate")]
    pub fn validate(&self, m: &ReferenceManifest) -> Result<(), ReferenceError> {
        let range = self.spawn_time_range;
        require(
            self.evidence.valid()
                && finite(&self.chances)
                && finite(&range)
                && finite(&[
                    self.spawn_distance,
                    self.height,
                    self.speed,
                    self.max_angle,
                    self.destroy_distance,
                    self.contrail_rate,
                ])
                && self.max_planes > 0
                && 0. <= range[0]
                && range[0] <= range[1]
                && self.spawn_distance > 0.
                && self.speed > 0.
                && self.destroy_distance > 0.
                && self.contrail_rate >= 0.
                && (0. ..=180.).contains(&self.max_angle),
            "sky spawner",
        )?;
        require(
            self.chances.len() == self.prefabs.len()
                && self.chances.iter().all(|c| *c >= 0.)
                && self.chances.iter().sum::<f32>() > 0.,
            "sky chances",
        )?;
        let node = |id: NodeId| m.node(id);
        for p in &self.prefabs {
            require(
                m.assets.iter().any(|a| {
                    a.id == p.asset && a.usage == AssetUsage::SkyTraffic && a.root == p.source
                }) && node(p.root).is_some_and(|n| {
                    n.source == p.source && n.asset == p.asset && n.parent.is_none()
                }),
                "sky prefab root",
            )?;
            if let Some(s) = &p.spin {
                require(
                    node(s.node).is_some_and(|n| n.asset == p.asset)
                        && finite(&s.axis)
                        && s.axis.iter().any(|v| *v != 0.)
                        && s.degrees_per_second.is_finite(),
                    "sky spin",
                )?;
            }
            if let Some(e) = &p.emp {
                require(
                    finite(&[
                        e.range,
                        e.value,
                        e.range_spread,
                        e.range_speed,
                        e.value_spread,
                        e.value_speed,
                    ]) && e.range > 0.
                        && e.range_spread >= 0.
                        && e.value_spread >= 0.
                        && !e.fall_off.keys.is_empty()
                        && particles::curve_valid(&e.fall_off),
                    "sky emp",
                )?;
            }
            if let Some(c) = &p.contrail {
                require(
                    c.role.starts_with("sky_") && c.role.ends_with("_contrail"),
                    "sky contrail role",
                )?;
                particles::validate_particle(c, 2)?;
                particles::validate_textures(c, m)?;
            }
        }
        self.ground_ufo.validate(self)?;
        self.flyer.validate(m)
    }
}
impl GroundUfoReference {
    #[cfg(feature = "validate")]
    fn validate(&self, sky: &SkyReference) -> Result<(), ReferenceError> {
        let range = |r: [f32; 2]| finite(&r) && 0. <= r[0] && r[0] <= r[1];
        require(
            self.evidence.valid()
                && sky
                    .prefabs
                    .get(self.prefab)
                    .is_some_and(|p| p.emp.is_some())
                && 0 <= self.night_range[0]
                && self.night_range[0] <= self.night_range[1]
                && self.nights_after_spawn >= 0
                && (0. ..=1.).contains(&self.chance)
                && range(self.back_range)
                && range(self.ai_distance_range)
                && range(self.ai_multiplier_range)
                && finite(&[
                    self.side_distance,
                    self.up_distance,
                    self.road_height_offset,
                    self.visited_distance,
                ])
                && self.side_distance >= 0.
                && 0. < self.speed
                && self.speed <= self.high_speed
                && self.target_distance > 0.
                && self.disappear_distance > 0.
                && self.mass > 0.
                && (0. ..=1.).contains(&self.speed_lerp)
                && sky.prefabs[self.prefab].spin.is_some()
                && sky.prefabs[self.prefab].seated.is_some(),
            "ground ufo",
        )?;
        let roles = [
            UfoRole::Body,
            UfoRole::Seat,
            UfoRole::AutoPilot,
            UfoRole::Speed,
            UfoRole::Altitude,
        ];
        require(
            roles
                .iter()
                .all(|r| self.colliders.iter().any(|c| c.role == *r))
                && self.colliders.iter().all(|c| {
                    c.seated
                        == matches!(
                            c.role,
                            UfoRole::AutoPilot | UfoRole::Speed | UfoRole::Altitude
                        )
                }),
            "ground ufo colliders",
        )?;
        let s = &self.seat;
        let t = &self.stimulus;
        require(
            finite(&s.node)
                && finite(&s.sit)
                && finite(&[
                    s.rot_limit,
                    s.default_pitch,
                    s.min_exit_angle,
                    s.exit_height,
                    s.push_force_modifier,
                ])
                && s.rot_limit >= 0.
                && (-89. ..=89.).contains(&s.default_pitch)
                && s.def_pose < 3
                && finite(&t.center)
                && t.radius > 0.,
            "ground ufo seat",
        )
    }
}

#[cfg(test)]
impl FlyerReference {
    /// The source values with one quaternion wing track on prototype node 1.
    pub fn fixture() -> Self {
        serde_json::from_value(serde_json::json!({
            "source": {"file": "sharedassets1.assets", "path_id": 32397},
            "prefab": {"file": "sharedassets1.assets", "path_id": 32395},
            "asset": 1, "item_id": 318, "time_range": [0., 3.], "range": [5., 20.],
            "max_range_from_start": 20., "speed_range": [1., 4.], "velocity_lerp": 1., "rot_lerp": 2.,
            "sound_time_range": [10., 60.], "volume_range": [0.3, 0.6], "pitch_range": [0.8, 1.2], "clips": [1], "voice": 1,
            "wing": {
                "source": {"file": "sharedassets1.assets", "path_id": 3992},
                "source_tracks": {"bytes": 1, "file": "tracks.json", "sha256": "a"},
                "name": "Armature|Fly", "start_time": 0., "stop_time": 1.4166667, "sample_rate": 60.,
                "loop": true, "cycle_offset": 0., "events": [],
                "tracks": [{
                    "node": 1, "attribute": 2, "path_hash": 7, "custom_type": 0, "curve_offset": 0,
                    "storage": "streamed", "interpolation": "CUBIC", "times": [0., 1.4166667],
                    "values": [[0., 0., 0., 1.], [0., 0., 0., 1.]],
                    "in_tangents": [[0., 0., 0., 0.], [0., 0., 0., 0.]],
                    "out_tangents": [[0., 0., 0., 0.], [0., 0., 0., 0.]],
                    "coefficients": [[[0., 0., 0.], [0., 0., 0.], [0., 0., 0.], [0., 0., 0.]], [[0., 0., 0.], [0., 0., 0.], [0., 0., 0.], [0., 0., 0.]]]
                }]
            }
        }))
        .unwrap()
    }
}
