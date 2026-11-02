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
    pub flyer: FlyerReference,
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
        let clips = m.audio["clips"].as_array();
        require(
            !self.clips.is_empty()
                && self.clips.iter().all(|id| {
                    clips.is_some_and(|c| c.iter().any(|c| c["id"].as_u64() == Some(*id as u64)))
                }),
            "flyer clips",
        )?;
        require(
            m.audio["car_voices"].as_array().is_some_and(|v| {
                v.iter().any(|v| {
                    v["id"].as_u64() == Some(u64::from(self.voice)) && v["one_shot"] == true
                })
            }),
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
        let node = |id: NodeId| m.nodes.iter().find(|n| n.id == id);
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
            if let Some(c) = &p.contrail {
                require(
                    c.role.starts_with("sky_") && c.role.ends_with("_contrail"),
                    "sky contrail role",
                )?;
                particles::validate_particle(c, 2)?;
            }
        }
        self.flyer.validate(m)
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
