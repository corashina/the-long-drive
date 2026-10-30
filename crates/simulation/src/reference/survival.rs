use super::*;
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NeedValues {
    pub hp: f32,
    pub food: f32,
    pub water: f32,
    pub bowel: f32,
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConsumptionEffect {
    pub food: f32,
    pub water: f32,
    pub bowel: f32,
    pub hp: f32,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FoodKind {
    Sausage,
    Bacon,
    BreadRoll,
    Kifli,
    Linzer,
    Chocolate,
    Turd,
}
impl FoodKind {
    pub fn name(self) -> &'static str {
        match self {
            Self::Sausage => "Sausage",
            Self::Bacon => "Bacon",
            Self::BreadRoll => "Bread roll",
            Self::Kifli => "Kifli",
            Self::Linzer => "Linzer",
            Self::Chocolate => "Chocolate",
            Self::Turd => "Turd",
        }
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ManagedEvidence {
    pub assembly_sha256: String,
    pub methods: Vec<String>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FoodCollider {
    pub source: SourceId,
    pub node: NodeId,
    pub membership: u32,
    pub filter: u32,
    pub sensor: bool,
    pub enabled: bool,
    pub geometry: ColliderGeometry,
    pub step_surface: Option<StepSurface>,
    pub tag: Option<super::ColliderTag>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FoodReference {
    pub source: SourceId,
    pub kind: FoodKind,
    pub effect: ConsumptionEffect,
    pub bites: u8,
    pub index: u8,
    pub destroy_on_end: bool,
    /// `ediblescript.spawnIfShot`: a `hurtFood` melee hit cuts the food in two.
    pub spawn_if_shot: bool,
    pub asset: AssetId,
    pub root: NodeId,
    pub mass_kg: f32,
    pub linear_damping: f32,
    pub angular_damping: f32,
    pub pickup: PickupReference,
    pub parts: Vec<Vec<NodeId>>,
    pub colliders: Vec<FoodCollider>,
    /// Source `ediblescript.hasParticle`. A bite emits at the food root.
    pub has_particle: bool,
    /// Index in `SurvivalReference::food_particles`.
    pub particle_type: u8,
    pub min_particle: u32,
    pub max_particle: u32,
    pub sound: u8,
    pub text: u32,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SourceInput {
    pub action: String,
    pub index: u32,
    pub key: String,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ToiletReference {
    pub source: SourceId,
    pub bladder: SourceId,
    pub radius: f32,
    pub no_angle: bool,
    pub valve_gain: f32,
    pub empty_amount: f32,
    pub bowel_fraction: f32,
    pub fart_burp_ratio: f32,
    pub fart_shit_ratio: f32,
    pub min_fart_pitch: f32,
    pub max_fart_pitch: f32,
    pub min_fart_volume: f32,
    pub max_fart_volume: f32,
    pub min_burp_pitch: f32,
    pub max_burp_pitch: f32,
    pub min_burp_volume: f32,
    pub max_burp_volume: f32,
    pub max_rate: f32,
    pub min_rate: f32,
    pub max_speed: f32,
    pub force_color: [f32; 4],
    pub force_color_m: f32,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SurvivalReference {
    pub mode_source: SourceId,
    pub mode: u8,
    pub death_seconds: f32,
    pub drinking: DrinkingReference,
    pub source: SourceId,
    pub player: SourceId,
    pub blood: SourceId,
    pub initial: NeedValues,
    pub maximum: NeedValues,
    pub initial_bladder: Vec<crate::fluids::FluidEntry>,
    pub bladder_capacity_liters: f32,
    pub initial_queue: ConsumptionEffect,
    pub starter_food: ItemId,
    pub foods: Vec<FoodReference>,
    pub starter_parts: Vec<Vec<NodeId>>,
    pub starter_parent: SourceId,
    pub starter_pose: Transform,
    pub evidence: ManagedEvidence,
    pub toilet: ToiletReference,
    pub inputs: Vec<SourceInput>,
    pub drink_modifier: f32,
    pub death_velocity_change: f32,
    pub food_stat_show_time: f32,
    pub food_stat_hide_time: f32,
    pub water_to_piss_ratio: f32,
    pub food_to_shit_ratio: f32,
    pub blood_to_hp_ratio: f32,
    pub alcohol_heal: f32,
    pub alcohol_convert_to_piss: f32,
    pub car_fluids_damage: f32,
    pub max_healing_speed: f32,
    pub healing_toilet_full_modfiier: f32,
    pub water_damage: f32,
    pub food_damage: f32,
    pub def_temp: f32,
    pub cold_temp: f32,
    pub hot_temp: f32,
    pub food_loss: f32,
    pub cold_food_loss_modifier: f32,
    pub hot_food_loss_modifier: f32,
    pub water_loss: f32,
    pub cold_water_loss_modifier: f32,
    pub hot_water_loss_modifier: f32,
    pub run_food_loss_modifier: f32,
    pub run_water_loss_modifier: f32,
    pub delay_speed: f32,
    /// Source `mainscript.FoodParticles`, in `foodParticleType` order. Emit only, world space.
    pub food_particles: Vec<ParticleReference>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SleepReference {
    pub source: SourceId,
    /// survivalscript.sleepModifier, source level1:73239.
    pub need_modifier: f32,
    pub hold_gain: f32,
    pub sleep_real_seconds: f32,
    pub sleep_cycle_seconds: f32,
    pub cooldown_seconds: f32,
    pub sleep_cam_ring_min: f32,
    pub sleep_cam_ring_max: f32,
    pub sleep_cam_height_min: f32,
    pub sleep_cam_height_max: f32,
    pub sleep_layer_mask: u32,
    pub cloud_low_min_speed: f32,
    pub cloud_low_mid_speed: f32,
    pub cloud_low_max_speed: f32,
    pub cloud_high_min_speed: f32,
    pub cloud_high_mid_speed: f32,
    pub cloud_high_max_speed: f32,
    pub evidence: ManagedEvidence,
}
impl NeedValues {
    fn values(self) -> [f32; 4] {
        [self.hp, self.food, self.water, self.bowel]
    }
}
impl ConsumptionEffect {
    fn valid(self) -> bool {
        finite(&[self.food, self.water, self.bowel, self.hp])
    }
}
impl ManagedEvidence {
    pub(super) fn valid(&self) -> bool {
        self.assembly_sha256 == "9919a9eeddcce23bb4ff86e890437f32b4a3ef3c10af2efe3b8d889d894dd62e"
            && !self.methods.is_empty()
    }
}
impl SurvivalReference {
    pub fn validate(&self, m: &ReferenceManifest) -> Result<(), ReferenceError> {
        require(
            self.evidence.valid() && self.source.file == "level1" && self.source.path_id == 73239,
            "survival source",
        )?;
        require(
            self.player == m.player.source
                && self
                    .initial
                    .values()
                    .iter()
                    .zip(self.maximum.values())
                    .all(|(a, b)| a.is_finite() && b.is_finite() && *a >= 0. && *a <= b && b > 0.),
            "initial needs",
        )?;
        require(
            crate::fluids::validate_fluid_entries(&self.initial_bladder)
                && self.initial_bladder.iter().all(|v| v.amount >= 0.)
                && self.bladder_capacity_liters.is_finite()
                && self.bladder_capacity_liters > 0.
                && self.initial_bladder.iter().map(|v| v.amount).sum::<f32>()
                    <= self.bladder_capacity_liters
                && self.initial_queue.valid(),
            "initial bladder and queue",
        )?;
        let starter = m
            .items
            .iter()
            .find(|i| i.id == self.starter_food)
            .filter(|i| matches!(i.kind, ItemKind::Food))
            .and_then(|i| {
                m.nodes
                    .iter()
                    .find(|n| n.id == i.node && n.source == i.source)
            })
            .filter(|n| n.source.file == "level1" && n.source.path_id == 24573)
            .ok_or_else(|| ReferenceError("Invalid starter food source".into()))?;
        require(transform(&self.starter_pose), "starter pose")?;
        require(
            self.foods.len() == 7
                && ids(self.foods.iter().map(|f| f.kind), "food kinds")?.len() == 7,
            "food prototypes",
        )?;
        require(
            self.food_particles.len() == 5
                && self
                    .food_particles
                    .iter()
                    .all(|p| p.simulation_space == 1 && p.role.starts_with("food_")),
            "food particles",
        )?;
        for p in &self.food_particles {
            super::particles::validate_particle(p, 1)?;
        }
        for f in &self.foods {
            require(
                f.bites > 0
                    && f.index < f.bites
                    && f.parts.len() == f.bites as usize
                    && f.destroy_on_end
                    && f.sound <= 1
                    && usize::from(f.particle_type) < self.food_particles.len()
                    && f.effect.valid(),
                "food consumption",
            )?;
            require(
                m.assets
                    .iter()
                    .any(|a| a.id == f.asset && a.usage == AssetUsage::FoodPrototype)
                    && m.nodes.iter().any(|n| n.id == f.root && n.asset == f.asset),
                "food asset",
            )?;
            require(
                finite(&[f.mass_kg, f.linear_damping, f.angular_damping])
                    && f.mass_kg > 0.
                    && f.linear_damping >= 0.
                    && f.angular_damping >= 0.
                    && !f.colliders.is_empty(),
                "food mass",
            )?;
            for group in &f.parts {
                require(
                    !group.is_empty()
                        && group
                            .iter()
                            .all(|id| m.nodes.iter().any(|n| n.id == *id && n.asset == f.asset)),
                    "food part",
                )?;
            }
            for c in &f.colliders {
                require(
                    c.membership != 0
                        && m.nodes.iter().any(|n| n.id == c.node && n.asset == f.asset)
                        && super::particles::valid_geometry(&c.geometry),
                    "food collider",
                )?;
            }
        }
        require(
            self.starter_parts.len() == 6
                && self.starter_parts.iter().all(|g| {
                    !g.is_empty()
                        && g.iter().all(|id| {
                            m.nodes
                                .iter()
                                .any(|n| n.id == *id && n.asset == starter.asset)
                        })
                }),
            "starter parts",
        )?;
        require(
            finite(&[
                self.food_loss,
                self.water_loss,
                self.food_damage,
                self.water_damage,
                self.max_healing_speed,
                self.delay_speed,
                self.drink_modifier,
                self.death_velocity_change,
                self.water_to_piss_ratio,
                self.food_to_shit_ratio,
                self.blood_to_hp_ratio,
                self.alcohol_heal,
                self.alcohol_convert_to_piss,
                self.car_fluids_damage,
                self.healing_toilet_full_modfiier,
                self.def_temp,
                self.cold_temp,
                self.hot_temp,
                self.cold_food_loss_modifier,
                self.hot_food_loss_modifier,
                self.cold_water_loss_modifier,
                self.hot_water_loss_modifier,
                self.run_food_loss_modifier,
                self.run_water_loss_modifier,
                self.food_stat_show_time,
                self.food_stat_hide_time,
            ]) && self.food_loss >= 0.
                && self.delay_speed > 0.
                && self.cold_temp < self.def_temp
                && self.def_temp < self.hot_temp,
            "survival rates",
        )?;
        require(
            self.mode == 2 && self.death_seconds.is_finite() && self.death_seconds > 0.,
            "survival mode and death",
        )?;
        let d = &self.drinking;
        require(
            d.evidence.valid()
                && super::particles::valid_geometry(&d.geometry)
                && finite(&d.local_position)
                && finite(&[d.radius, d.valve, d.particle_contact_radius_multiplier])
                && d.radius > 0.
                && d.valve > 0.
                && d.is_drink_cap
                && d.tank == self.toilet.bladder,
            "drinking contacts",
        )?;
        let t = &self.toilet;
        require(
            finite(&[
                t.radius,
                t.valve_gain,
                t.empty_amount,
                t.bowel_fraction,
                t.fart_burp_ratio,
                t.fart_shit_ratio,
                t.min_fart_pitch,
                t.max_fart_pitch,
                t.min_burp_pitch,
                t.max_burp_pitch,
                t.min_fart_volume,
                t.max_fart_volume,
                t.min_burp_volume,
                t.max_burp_volume,
                t.max_rate,
                t.min_rate,
                t.max_speed,
                t.force_color_m,
            ]) && finite(&t.force_color)
                && t.radius > 0.
                && t.min_fart_pitch > 0.
                && t.max_fart_pitch >= t.min_fart_pitch
                && t.min_burp_pitch > 0.
                && t.max_burp_pitch >= t.min_burp_pitch,
            "toilet rules",
        )?;
        require(
            self.inputs.len() == 4
                && ids(
                    self.inputs.iter().map(|i| i.action.as_str()),
                    "input action",
                )?
                .len()
                    == 4
                && self.inputs.iter().all(|i| !i.key.is_empty()),
            "survival inputs",
        )
    }
}
impl SleepReference {
    pub fn validate(&self, _: &ReferenceManifest) -> Result<(), ReferenceError> {
        require(
            self.evidence.valid()
                && finite(&[
                    self.need_modifier,
                    self.hold_gain,
                    self.sleep_real_seconds,
                    self.sleep_cycle_seconds,
                    self.cooldown_seconds,
                    self.sleep_cam_ring_min,
                    self.sleep_cam_ring_max,
                    self.sleep_cam_height_min,
                    self.sleep_cam_height_max,
                    self.cloud_low_min_speed,
                    self.cloud_low_mid_speed,
                    self.cloud_low_max_speed,
                    self.cloud_high_min_speed,
                    self.cloud_high_mid_speed,
                    self.cloud_high_max_speed,
                ])
                && self.need_modifier > 0.
                && self.hold_gain > 0.
                && self.sleep_real_seconds > 0.
                && self.sleep_cycle_seconds > 0.
                && self.cooldown_seconds >= 0.
                && self.sleep_cam_ring_min > 0.
                && self.sleep_cam_ring_max >= self.sleep_cam_ring_min
                && self.sleep_cam_height_min > 0.
                && self.sleep_cam_height_max >= self.sleep_cam_height_min
                && self.sleep_layer_mask != 0,
            "sleep rules",
        )
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DrinkingReference {
    pub source: SourceId,
    pub tank: SourceId,
    pub collider: SourceId,
    pub geometry: ColliderGeometry,
    pub local_position: [f32; 3],
    pub radius: f32,
    pub valve: f32,
    pub no_angle: bool,
    pub no_pour: bool,
    pub is_drink_cap: bool,
    pub particle_contact_radius_multiplier: f32,
    pub evidence: ManagedEvidence,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CigaretteConsumptionReference {
    pub source: SourceId,
    pub continuous: bool,
    pub automatic: bool,
    pub destroy_on_end: bool,
    pub ate_once: bool,
    pub serialized_hp: f32,
    pub startup_hp: f32,
    pub eat_speed: f32,
    pub auto_eat_speed: f32,
    pub effect: ConsumptionEffect,
    pub parts: Vec<NodeId>,
    pub start: Vec<NodeId>,
    pub end: Vec<NodeId>,
    pub index: u32,
    pub is_smoke: bool,
    pub has_particle: bool,
    pub particle_position: NodeId,
    pub particle_type: u32,
    pub min_particle: i32,
    pub max_particle: i32,
    pub sound: u32,
    pub text: u32,
    pub dont_show_stats: bool,
    pub ember_in_auto: bool,
    pub ember: CigaretteEmberReference,
    pub evidence: ManagedEvidence,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CigaretteEmberReference {
    pub source: SourceId,
    pub renderer: NodeId,
    pub renderer_source: SourceId,
    pub material: SourceId,
    pub light: LightReference,
    pub particle: ParticleReference,
    pub particle_node: NodeId,
    pub particle_renderer: SourceId,
    pub colors: [[f32; 4]; 4],
    pub light_intensities: [f32; 4],
    pub particle_rates: [f32; 4],
    pub speed_up: f32,
    pub speed_down: f32,
    pub lerp: f32,
}
impl CigaretteConsumptionReference {
    pub fn validate(&self, p: &ItemPrototype, m: &ReferenceManifest) -> Result<(), ReferenceError> {
        let sid = |path_id| SourceId {
            file: "sharedassets1.assets".into(),
            path_id,
        };
        let node = |n: NodeId, id| {
            m.nodes
                .iter()
                .any(|v| v.id == n && v.source == sid(id) && p.within(m, n))
        };
        require(
            self.source == sid(134642)
                && self.continuous
                && self.automatic
                && !self.destroy_on_end
                && !self.ate_once
                && self.serialized_hp == 0.
                && self.startup_hp == 1.
                && self.min_particle == -100
                && self.max_particle == 1
                && !self.ember_in_auto
                && self.index == 0
                && self.is_smoke
                && self.has_particle
                && self.particle_type == 3
                && self.text == 324
                && self.sound == 0
                && !self.dont_show_stats,
            "cigarette continuous source",
        )?;
        require(
            self.eat_speed == 0.02
                && self.auto_eat_speed == 0.0020833334
                && self.effect.food == -0.0002
                && self.effect.water == -0.0002
                && self.effect.bowel == 0.0002
                && self.effect.hp == 0.,
            "cigarette consumption rates",
        )?;
        for (nodes, expected) in [
            (&self.parts, [78874, 87786]),
            (&self.start, [73923, 74031]),
            (&self.end, [50758, 78154]),
        ] {
            require(
                nodes.len() == 2 && nodes.iter().zip(expected).all(|(n, id)| node(*n, id)),
                "cigarette source part links",
            )?;
        }
        require(
            node(self.particle_position, 84276)
                && self.evidence.valid()
                && self
                    .evidence
                    .methods
                    .iter()
                    .map(String::as_str)
                    .eq(CIGARETTE_METHODS.iter().copied()),
            "cigarette source method evidence",
        )?;
        let e = &self.ember;
        require(
            e.source == sid(156940)
                && e.renderer_source == sid(94155)
                && e.material == sid(460)
                && p.renderers
                    .iter()
                    .any(|r| r.node == e.renderer && r.source == e.renderer_source)
                && m.rendering
                    .materials
                    .iter()
                    .any(|r| r.asset == p.asset && r.source == e.material),
            "cigarette ember renderer and material",
        )?;
        require(
            e.colors.iter().all(|c| finite(c))
                && finite(&e.light_intensities)
                && finite(&e.particle_rates)
                && e.light_intensities == [0., 0.1, 0.25, 0.]
                && e.particle_rates == [0., 100., 0., 0.]
                && e.speed_up == 1.
                && e.speed_down == 1.
                && e.lerp == 0.,
            "cigarette ember source values",
        )?;
        let l = &e.light;
        require(
            l.source == sid(123875)
                && l.node.is_some_and(|n| node(n, 85874))
                && l.kind == LightKind::Point
                && transform(&l.pose)
                && finite(&l.color)
                && l.serialized_intensity.is_finite()
                && l.startup_intensity == 0.
                && l.range_meters.is_finite()
                && l.range_meters > 0.
                && l.cookie.is_none()
                && l.cookie_size.is_finite()
                && finite(&[
                    l.full_cone_degrees,
                    l.inner_cone_degrees,
                    l.shadows.strength,
                    l.shadows.bias,
                    l.shadows.normal_bias,
                    l.shadows.near_plane,
                ])
                && l.full_cone_degrees > 0.
                && l.full_cone_degrees <= 180.
                && l.inner_cone_degrees >= 0.
                && l.inner_cone_degrees <= l.full_cone_degrees
                && l.shadows.kind <= 2
                && (0. ..=1.).contains(&l.shadows.strength)
                && l.shadows.bias >= 0.
                && l.shadows.normal_bias >= 0.
                && l.shadows.near_plane >= 0.
                && l.culling_mask != 0,
            "cigarette local ember light",
        )?;
        let particle = &e.particle;
        require(
            particle.source == sid(128014)
                && e.particle_renderer == sid(128908)
                && node(e.particle_node, 87786)
                && particle.role == "cigarette"
                && particle.simulation_space == 1
                && particle.materials.len() == 1
                && particle.materials[0].source == sid(60)
                && particle.materials[0].shader.name == "Legacy Shaders/Particles/Alpha Blended",
            "cigarette particle source",
        )?;
        let modules: &[(&str, bool)] = &[
            ("ClampVelocityModule", false),
            ("CollisionModule", false),
            ("ColorBySpeedModule", false),
            ("ColorModule", true),
            ("CustomDataModule", false),
            ("EmissionModule", true),
            ("ExternalForcesModule", false),
            ("ForceModule", true),
            ("InheritVelocityModule", false),
            ("InitialModule", true),
            ("LifetimeByEmitterSpeedModule", false),
            ("LightsModule", false),
            ("NoiseModule", false),
            ("RotationBySpeedModule", false),
            ("RotationModule", false),
            ("ShapeModule", true),
            ("SizeBySpeedModule", false),
            ("SizeModule", true),
            ("SubModule", false),
            ("TrailModule", false),
            ("TriggerModule", false),
            ("UVModule", false),
            ("VelocityModule", true),
        ];
        require(
            particle.module_flags.len() == modules.len()
                && modules
                    .iter()
                    .all(|(name, value)| particle.module_flags.get(*name) == Some(value)),
            "cigarette source particle modules",
        )?;
        let required = &[
            "EmissionModule.rateOverDistance",
            "EmissionModule.rateOverTime",
            "ForceModule.x",
            "ForceModule.y",
            "ForceModule.z",
            "InitialModule.gravityModifier",
            "InitialModule.startLifetime",
            "InitialModule.startRotation",
            "InitialModule.startRotationX",
            "InitialModule.startRotationY",
            "InitialModule.startSize",
            "InitialModule.startSizeY",
            "InitialModule.startSizeZ",
            "InitialModule.startSpeed",
            "ShapeModule.arc.speed",
            "ShapeModule.m_MeshSpawn.speed",
            "ShapeModule.radius.speed",
            "SizeModule.curve",
            "SizeModule.y",
            "SizeModule.z",
            "VelocityModule.orbitalOffsetX",
            "VelocityModule.orbitalOffsetY",
            "VelocityModule.orbitalOffsetZ",
            "VelocityModule.orbitalX",
            "VelocityModule.orbitalY",
            "VelocityModule.orbitalZ",
            "VelocityModule.radial",
            "VelocityModule.speedModifier",
            "VelocityModule.x",
            "VelocityModule.y",
            "VelocityModule.z",
            "startDelay",
        ];
        require(
            particle.curves.len() == required.len()
                && required
                    .iter()
                    .all(|key| particle.curves.contains_key(*key)),
            "cigarette required particle curves",
        )?;
        let required = &["ColorModule.gradient", "InitialModule.startColor"];
        require(
            particle.colors.len() == required.len()
                && required
                    .iter()
                    .all(|key| particle.colors.contains_key(*key)),
            "cigarette required particle colors",
        )?;
        let required = &[
            "InitialModule.customEmitterVelocity",
            "ShapeModule.boxThickness",
            "ShapeModule.m_Position",
            "ShapeModule.m_Rotation",
            "ShapeModule.m_Scale",
            "renderer.m_Flip",
            "renderer.m_Pivot",
        ];
        require(
            particle.vectors.len() == required.len()
                && required
                    .iter()
                    .all(|key| particle.vectors.contains_key(*key)),
            "cigarette required particle vectors",
        )?;
        let required = &[
            "EmissionModule.m_BurstCount",
            "InitialModule.maxNumParticles",
            "InitialModule.randomizeRotationDirection",
            "ShapeModule.angle",
            "ShapeModule.arc.mode",
            "ShapeModule.arc.spread",
            "ShapeModule.arc.value",
            "ShapeModule.donutRadius",
            "ShapeModule.length",
            "ShapeModule.m_MeshMaterialIndex",
            "ShapeModule.m_MeshNormalOffset",
            "ShapeModule.m_MeshSpawn.mode",
            "ShapeModule.m_MeshSpawn.spread",
            "ShapeModule.m_TextureClipChannel",
            "ShapeModule.m_TextureClipThreshold",
            "ShapeModule.m_TextureUVChannel",
            "ShapeModule.placementMode",
            "ShapeModule.radius.mode",
            "ShapeModule.radius.spread",
            "ShapeModule.radius.value",
            "ShapeModule.radiusThickness",
            "ShapeModule.randomDirectionAmount",
            "ShapeModule.randomPositionAmount",
            "ShapeModule.sphericalDirectionAmount",
            "ShapeModule.type",
            "cullingMode",
            "emitterVelocityMode",
            "gameObject.layer",
            "lengthInSec",
            "moveWithTransform",
            "randomSeed",
            "renderer.m_CameraVelocityScale",
            "renderer.m_LengthScale",
            "renderer.m_MaxParticleSize",
            "renderer.m_MinParticleSize",
            "renderer.m_RenderAlignment",
            "renderer.m_RenderMode",
            "renderer.m_SortMode",
            "renderer.m_SortingFudge",
            "renderer.m_VelocityScale",
            "ringBufferLoopRange.x",
            "ringBufferLoopRange.y",
            "ringBufferMode",
            "scalingMode",
            "simulationSpeed",
            "stopAction",
        ];
        require(
            particle.scalars.len() == required.len()
                && required
                    .iter()
                    .all(|key| particle.scalars.contains_key(*key)),
            "cigarette required particle scalars",
        )?;
        let flags = &[
            ("ColorModule.enabled", true),
            ("EmissionModule.enabled", true),
            ("ForceModule.enabled", true),
            ("ForceModule.inWorldSpace", true),
            ("ForceModule.randomizePerFrame", true),
            ("InitialModule.enabled", true),
            ("InitialModule.rotation3D", false),
            ("InitialModule.size3D", false),
            ("ShapeModule.alignToDirection", false),
            ("ShapeModule.enabled", true),
            ("ShapeModule.m_TextureAlphaAffectsParticles", true),
            ("ShapeModule.m_TextureBilinearFiltering", false),
            ("ShapeModule.m_TextureColorAffectsParticles", true),
            ("ShapeModule.m_UseMeshColors", true),
            ("ShapeModule.m_UseMeshMaterialIndex", false),
            ("SizeModule.enabled", true),
            ("SizeModule.separateAxes", false),
            ("VelocityModule.enabled", true),
            ("VelocityModule.inWorldSpace", true),
            ("autoRandomSeed", true),
            ("looping", true),
            ("playOnAwake", true),
            ("prewarm", false),
            ("useUnscaledTime", false),
            ("gameObject.active", true),
            ("renderer.enabled", true),
        ];
        require(
            particle.flags.len() == flags.len()
                && flags
                    .iter()
                    .all(|(key, value)| particle.flags.get(*key) == Some(value)),
            "cigarette particle flags",
        )?;
        require(
            particle.links.len() == 1 && particle.links.get("m_GameObject") == Some(&sid(37823)),
            "cigarette particle source links",
        )?;
        // Checked local emitter and renderer operation selectors.
        for (name, value) in [
            ("ShapeModule.type", 0.),
            ("ShapeModule.arc.mode", 0.),
            ("ShapeModule.radius.mode", 0.),
            ("ShapeModule.m_MeshSpawn.mode", 0.),
            ("ShapeModule.placementMode", 0.),
            ("ShapeModule.m_TextureClipChannel", 3.),
            ("ShapeModule.m_TextureUVChannel", 0.),
            ("renderer.m_RenderMode", 0.),
            ("renderer.m_RenderAlignment", 0.),
            ("renderer.m_SortMode", 0.),
            ("cullingMode", 0.),
            ("emitterVelocityMode", 1.),
            ("ringBufferMode", 0.),
            ("scalingMode", 1.),
            ("stopAction", 0.),
            ("moveWithTransform", 1.),
            ("EmissionModule.m_BurstCount", 0.),
        ] {
            require(
                particle.scalars.get(name) == Some(&value),
                &format!("cigarette particle operation {name}"),
            )?;
        }
        require(
            particle.curves.values().all(|c| {
                [&c.min_curve, &c.max_curve]
                    .iter()
                    .all(|c| c.pre_wrap_mode == 2 && c.post_wrap_mode == 2)
            }),
            "cigarette particle curve wrap operation",
        )?;
        particle.validate_data(1)?;
        require(
            !m.lighting.lights.iter().any(|l| l.source == e.light.source)
                && !m.particles.iter().any(|x| x.source == particle.source),
            "cigarette active effects publication",
        )?;
        Ok(())
    }
}

const CIGARETTE_METHODS: &[&str] = &[
    "public void UpdParts(bool _updMulti)\n\t{\n\t\tif (continous)\n\t\t{\n\t\t\tfor (int i = 0; i < parts.Length; i++)\n\t\t\t{\n\t\t\t\tparts[i].transform.position = Vector3.Lerp(end[i].position, start[i].position, hp);\n\t\t\t\tparts[i].transform.rotation = Quaternion.Lerp(end[i].rotation, start[i].rotation, hp);\n\t\t\t\tparts[i].transform.localScale = Vector3.Lerp(end[i].localScale, start[i].localScale, hp);\n\t\t\t}\n\t\t}\n\t\telse if (moreParts)",
    "if (!ateOnce)\n\t\t{\n\t\t\thp = 1f;",
    "float num = ((!auto) ? (eatSpeed * Time.deltaTime) : (autoEatSpeed * Time.deltaTime));",
    "hp = Mathf.Clamp(hp - num, 0f, 1f);",
    "if (ember != null && (!auto || emberInAuto))",
    "if (hp <= 0f)\n\t\t\t\t{\n\t\t\t\t\tember.End();",
    "parts[i].transform.position = Vector3.Lerp(end[i].position, start[i].position, hp);",
    "Eat(out var _, out var _, out var _, _playSound: true, auto: true);",
    "survival.Eat(weared[i].edible.Eat(out tempShitValue, out tempHpValue, out tempWaterValue, _playSound: true), tempShitValue, tempHpValue, tempWaterValue);",
    "if (!(weared[num3] == null) && weared[num3].edible != null)\n\t\t\t\t{\n\t\t\t\t\tTakeOff(num3);",
];
