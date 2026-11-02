use super::*;
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StormReference {
    pub source: SourceId,
    pub biome: u8,
    pub life_loss_on_other_biome: f32,
    pub life_gain: f32,
    pub destroy_dist: f32,
    pub life: f32,
    pub radius: f32,
    pub fog_density: f32,
    pub atmosphere_thickness: f32,
    pub intensity_multipler: f32,
    pub ambient_multipler: f32,
    pub volume: f32,
    pub ambient_decrease: f32,
    pub yoffset: f32,
    pub wind_force: f32,
    pub current_force: [f32; 3],
    pub next_force: [f32; 3],
    pub distance_lerp: f32,
    pub def_s_rate_t: f32,
    pub def_s_rate_d: f32,
    pub def_b_rate_t: f32,
    pub def_b_rate_d: f32,
    pub night_color: [f32; 4],
    pub sky_color: [f32; 4],
    pub sky_ground_color: [f32; 4],
    pub fog_color: [f32; 4],
    pub cloud_color: [f32; 4],
    pub day_color1: GradientReference,
    pub day_color2: GradientReference,
    pub wind_lm: u32,
    pub size_range: [f32; 2],
    pub speed_range: [f32; 2],
    pub wind_time_range: [f32; 2],
    pub wind_scan_seconds: f32,
    pub wind_bounds_range: [f32; 2],
}
/// Source thumbleweedMain and its prefab links. Ranges are [min, max].
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TumbleweedReference {
    pub source: SourceId,
    /// Item database index of the prefab.
    pub item_id: u32,
    /// Prefab GameObject.
    pub prefab: SourceId,
    /// The prefab thumbleweedScript, from tosaveitemscript.thumbleweed.
    pub script: SourceId,
    /// The Rigidbody on the thumbleweedScript object.
    pub body: SourceId,
    /// Gib GameObject, when the prefab links one.
    pub gib: Option<SourceId>,
    /// Source TerrainGenerationSettings.importantObjs transforms.
    pub important_objects: Vec<SourceId>,
    pub ray_count: u32,
    pub ray_length: f32,
    pub ray_count_to_stay: u32,
    pub limit: u32,
    /// Degrees.
    pub fov: f32,
    pub wind_force_range: [f32; 2],
    pub wind_time_range: [f32; 2],
    pub wind_live_time_range: [f32; 2],
    pub short_spawn_time_range: [f32; 2],
    pub long_spawn_time_range: [f32; 2],
    pub spawn_dist_range: [f32; 2],
    pub spawn_dist_in_fov_range: [f32; 2],
    pub spawn_ray_lm: u32,
    pub ray_lm: u32,
    pub long_spawn_chance: f32,
    pub spawn_height: f32,
    pub ray_height: f32,
    pub ring_degrees: f32,
    pub evidence: ManagedEvidence,
}
impl TumbleweedReference {
    pub fn validate(&self, m: &ReferenceManifest) -> Result<(), ReferenceError> {
        let ranges = [
            self.wind_force_range,
            self.wind_time_range,
            self.wind_live_time_range,
            self.short_spawn_time_range,
            self.long_spawn_time_range,
            self.spawn_dist_range,
            self.spawn_dist_in_fov_range,
        ];
        require(
            self.evidence.valid()
                && finite(&[
                    self.ray_length,
                    self.fov,
                    self.long_spawn_chance,
                    self.spawn_height,
                    self.ray_height,
                    self.ring_degrees,
                ])
                && ranges.iter().all(|r| finite(r) && r[0] <= r[1])
                && self.limit > 0
                && self.ray_count > 0
                && self.ray_count_to_stay > 0
                && self.ray_count_to_stay <= self.ray_count
                && (0. ..=180.).contains(&self.fov)
                && !self.important_objects.is_empty(),
            "tumbleweed spawner",
        )?;
        let item = m.world.catalog.items.get(self.item_id as usize);
        require(
            item.is_some_and(|i| i.field == "gthumbleweed" && i.source == self.prefab)
                && m.item_prototypes
                    .iter()
                    .any(|p| p.item_id == Some(self.item_id)),
            "tumbleweed item",
        )
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WeatherReference {
    pub source: SourceId,
    /// Source sandStormSpawnScript.prefabid: the storm index in the item database.
    pub prefab_id: u32,
    pub min_interval_seconds: f32,
    pub max_interval_seconds: f32,
    pub spawn_radius: f32,
    pub storm: StormReference,
    pub evidence: ManagedEvidence,
    /// The big and small storm systems, in this order.
    pub particles: Vec<ParticleReference>,
    pub tumbleweeds: TumbleweedReference,
}
impl WeatherReference {
    pub fn validate(&self, m: &ReferenceManifest) -> Result<(), ReferenceError> {
        require(
            self.evidence.valid()
                && finite(&[
                    self.min_interval_seconds,
                    self.max_interval_seconds,
                    self.spawn_radius,
                ])
                && self.min_interval_seconds > 0.
                && self.max_interval_seconds >= self.min_interval_seconds
                && self.spawn_radius > 0.,
            "weather schedule",
        )?;
        let s = &self.storm;
        require(
            finite(&[
                s.life_loss_on_other_biome,
                s.life_gain,
                s.destroy_dist,
                s.life,
                s.radius,
                s.fog_density,
                s.atmosphere_thickness,
                s.intensity_multipler,
                s.ambient_multipler,
                s.volume,
                s.ambient_decrease,
                s.yoffset,
                s.wind_force,
                s.distance_lerp,
                s.def_s_rate_t,
                s.def_s_rate_d,
                s.def_b_rate_t,
                s.def_b_rate_d,
                s.wind_scan_seconds,
            ]) && finite(&s.current_force)
                && finite(&s.next_force)
                && s.distance_lerp >= 0.
                && s.radius > 0.
                && s.destroy_dist >= s.radius
                && s.life_gain > 0.
                && s.life_loss_on_other_biome > 0.
                && s.wind_scan_seconds > 0.,
            "storm rates",
        )?;
        require(
            [
                s.size_range,
                s.speed_range,
                s.wind_time_range,
                s.wind_bounds_range,
            ]
            .iter()
            .all(|r| finite(r) && r[0] > 0. && r[1] >= r[0])
                && [
                    s.night_color,
                    s.sky_color,
                    s.sky_ground_color,
                    s.fog_color,
                    s.cloud_color,
                ]
                .iter()
                .all(|c| finite(c))
                && gradient_valid(&s.day_color1)
                && gradient_valid(&s.day_color2),
            "storm curves",
        )?;
        require(
            self.particles.len() == 2
                && self.particles[0].role == "storm_big"
                && self.particles[1].role == "storm_small",
            "storm particle roles",
        )?;
        for p in &self.particles {
            particles::validate_particle(p, 2)?;
        }
        self.tumbleweeds.validate(m)
    }
}
