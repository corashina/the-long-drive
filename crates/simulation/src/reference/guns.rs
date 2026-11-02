//! Checked guns: weaponscript records, ammo, gun kind helpers, shot tracers and
//! bullet-hole decals. Vectors keep source Unity local axes.
use super::*;

/// mainscript.ammoType count.
pub const AMMO_TYPES: usize = 7;
/// mainscript.GetDecal order: default, metal, flesh, wood, plant, glass.
pub const GUN_DECALS: usize = 6;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GunsReference {
    pub source: SourceId,
    /// mainscript.ammoMass per ammo type.
    pub ammo_mass: [f32; AMMO_TYPES],
    /// mainscript.ammoVelocity per ammo type.
    pub ammo_velocity: [f32; AMMO_TYPES],
    pub tracer: GunTracer,
    /// mainscript.ShootDecals in `GUN_DECALS` order.
    pub decals: [GunDecal; GUN_DECALS],
    /// One record per world prototype with a weaponscript, sorted by prototype.
    pub weapons: Vec<GunReference>,
}

/// mainscript.ParticleShootStart and ParticleShootEnd.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GunTracer {
    pub start: ParticleReference,
    pub end: ParticleReference,
}

/// One ShootDecals prefab.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GunDecal {
    /// The prefab root Transform.
    pub source: SourceId,
    /// The Transform with the mesh.
    pub node: SourceId,
    /// Mesh node pose under the root.
    pub local_pose: Transform,
    pub mesh: GunDecalMesh,
    pub material: SkyMaterialReference,
    /// DecalDestroyer.lifeTime. None: the decal stays.
    pub lifetime: Option<f32>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GunDecalMesh {
    pub source: SourceId,
    pub bounds_center: [f32; 3],
    pub bounds_extents: [f32; 3],
}

/// weaponscript of one world prototype.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GunReference {
    pub prototype: PrototypeId,
    pub source: SourceId,
    pub pickup: SourceId,
    pub ammo: AcquisitionAmmo,
    pub kind: GunKind,
    pub condition: Option<SourceId>,
    pub max_condition_weakness: f32,
    pub infinite: bool,
    pub shot_count: u32,
    pub shot_count_spread: f32,
    pub automatic: bool,
    pub automatic_false_shot: bool,
    pub min_shoot_time: f32,
    /// LM: the shot ray mask.
    pub mask: u32,
    /// LM2.
    pub mask2: u32,
    /// S: the muzzle.
    pub muzzle: SourceId,
    pub clips: GunClips,
    pub audio_m: f32,
    pub flash: Option<GunFlash>,
    pub recoil: Option<GunRecoil>,
    pub reload_hand: Option<GunReloadHand>,
    pub barrels: Vec<GunBarrel>,
    pub moving_parts: Vec<GunMovingPart>,
    pub triggers: Vec<GunTriggerPull>,
    pub trigger_triggers: Vec<GunTriggerTrigger>,
    pub use_conditional_trigger_triggers: bool,
    pub conditional_trigger_triggers: Vec<GunConditionalTrigger>,
    pub fr: GunBreak,
    pub sounds: GunSounds,
    pub crosshair: GunCrosshair,
    pub explode_stuffs: bool,
    pub explode_chance: f32,
    pub hithole: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum GunKind {
    None,
    /// revolverScript.
    Revolver {
        /// Bullets: one mesh node per chamber.
        chambers: Vec<SourceId>,
        cylinder: SourceId,
        axis: [f32; 3],
        angle: f32,
        duration: f32,
        shoot_bullet_pos: SourceId,
        cylinder_start_rotation: [f32; 3],
        turn_clip: Option<SourceId>,
        turn_volume: f32,
    },
    /// bbgunscript.
    BbGun {
        power: u32,
        power_gain: u32,
        max_power: u32,
        ammo_gain: u32,
        dont_use_power: bool,
        bullets: Vec<SourceId>,
    },
    /// geppityuscript.
    Magazine {
        mag: SourceId,
        mag_pos: SourceId,
        /// World prototype of the magazine item.
        mag_prefab: SourceId,
        /// weaponscript.hasMag at start.
        has_mag: bool,
    },
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GunClips {
    pub one_shot: Vec<SourceId>,
    pub false_shot: Vec<SourceId>,
    pub hit_sand: Vec<SourceId>,
    pub hit_concrete: Vec<SourceId>,
    pub hit_flesh: Vec<SourceId>,
    pub hit_glass: Vec<SourceId>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GunFlash {
    pub light: SourceId,
    pub node: SourceId,
    pub time: f32,
    pub intensity: f32,
    pub curve: TorqueCurve,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GunRecoil {
    pub node: SourceId,
    pub pos: [f32; 3],
    /// Euler degrees.
    pub rot: [f32; 3],
    pub curve: TorqueCurve,
    pub duration: f32,
}

/// ReloadLHPos and its targets.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GunReloadHand {
    pub node: SourceId,
    pub default: SourceId,
    pub barrel: Option<SourceId>,
    pub reload: Option<SourceId>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GunBarrel {
    pub node: SourceId,
    pub max_angle: f32,
    pub inv: bool,
    /// Local Euler axis: 0 x, 1 y, 2 z. None: the barrel does not turn.
    pub axis: Option<u8>,
    /// The FR curve when useCurve is set.
    pub curve: Option<TorqueCurve>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GunMovingPart {
    pub node: SourceId,
    pub t0: SourceId,
    pub t1: SourceId,
    pub inv: bool,
}

/// weaponscript.OnTriggerPull.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GunTriggerPull {
    pub node: SourceId,
    pub dont_work_without_shoot: bool,
    pub duration: f32,
    pub use_angle: bool,
    pub def_angle: [f32; 3],
    pub angle: [f32; 3],
    pub angle_curve: TorqueCurve,
    pub use_pos: bool,
    pub def_pos: [f32; 3],
    pub pos: [f32; 3],
    pub pos_curve: TorqueCurve,
}

/// weaponscript.TriggerTrigger.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GunTriggerTrigger {
    pub node: SourceId,
    pub dont_trigger: bool,
    pub def_rotation: [f32; 3],
    pub pulled_rotation: [f32; 3],
}

/// weaponscript.condTriggerTrigger.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GunConditionalTrigger {
    pub ammo_count: u32,
    pub r#use: Vec<bool>,
}

/// Break barrel (FR) state and rules.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GunBreak {
    /// FRValue at start.
    pub value: f32,
    pub fr_speed: f32,
    pub use_x: bool,
    pub use_y: bool,
    /// letorhetocso.
    pub breakable: bool,
    pub play_sound_even_if_cant_shoot: bool,
    pub shoot: GunBreakRule,
    pub reload: GunBreakRule,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GunBreakRule {
    pub value: f32,
    pub equal: bool,
    pub bigger: bool,
    pub smaller: bool,
    pub near: bool,
    pub near_value: f32,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GunSounds {
    /// SSource: the AudioSource for the break sounds.
    pub source: Option<SourceId>,
    pub open: Option<GunSound>,
    pub open_end: Option<GunSound>,
    pub close: Option<GunSound>,
    pub r#move: Option<SourceId>,
    pub reload: Option<GunSound>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GunSound {
    pub clip: SourceId,
    pub volume: f32,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GunCrosshair {
    pub icon: Option<SourceId>,
    pub color: [f32; 4],
    pub empty_color: [f32; 4],
}

impl GunsReference {
    pub fn validate(&self, m: &ReferenceManifest) -> Result<(), ReferenceError> {
        require(
            self.source.file == "level1" && finite(&self.ammo_mass) && finite(&self.ammo_velocity),
            "gun ammo values",
        )?;
        conditions::validate_particle(&self.source, &self.tracer.start, m)?;
        conditions::validate_particle(&self.source, &self.tracer.end, m)?;
        for d in &self.decals {
            require(
                transform(&d.local_pose)
                    && finite(&d.mesh.bounds_center)
                    && finite(&d.mesh.bounds_extents)
                    && d.lifetime.is_none_or(f32::is_finite)
                    && d.material.textures.iter().all(|t| {
                        m.lighting
                            .textures
                            .iter()
                            .any(|texture| texture.id == t.texture)
                    }),
                "gun decal",
            )?;
        }
        for (i, w) in self.weapons.iter().enumerate() {
            require(
                self.weapons[..i]
                    .iter()
                    .all(|v| v.source != w.source && v.prototype != w.prototype),
                "gun identity",
            )?;
            w.validate(m)?;
        }
        Ok(())
    }
}

impl GunReference {
    fn validate(&self, m: &ReferenceManifest) -> Result<(), ReferenceError> {
        let error = |field: &str| {
            ReferenceError(format!(
                "Invalid gun {}:{} field {field}",
                self.source.file, self.source.path_id
            ))
        };
        let check = |ok: bool, field: &str| if ok { Ok(()) } else { Err(error(field)) };
        let prototype = m
            .world
            .prototypes
            .iter()
            .find(|p| p.source == self.prototype.0)
            .ok_or_else(|| error("world prototype"))?;
        let node = |s: &SourceId| prototype.nodes.iter().any(|n| n.source == *s);
        let clips = m.audio["clips"].as_array();
        let clip = |s: &SourceId| {
            clips.is_some_and(|clips| {
                clips.iter().any(|c| {
                    c["source"]["file"].as_str() == Some(s.file.as_str())
                        && c["source"]["path_id"].as_i64() == Some(s.path_id)
                })
            })
        };

        check((self.ammo.r#type as usize) < AMMO_TYPES, "ammo type")?;
        let mut nodes = vec![&self.muzzle];
        if let Some(f) = &self.flash {
            nodes.push(&f.node);
        }
        if let Some(r) = &self.recoil {
            nodes.push(&r.node);
        }
        if let Some(h) = &self.reload_hand {
            nodes.extend([&h.node, &h.default]);
            nodes.extend(h.barrel.iter().chain(&h.reload));
        }
        nodes.extend(self.barrels.iter().map(|b| &b.node));
        nodes.extend(
            self.moving_parts
                .iter()
                .flat_map(|p| [&p.node, &p.t0, &p.t1]),
        );
        nodes.extend(self.triggers.iter().map(|t| &t.node));
        nodes.extend(self.trigger_triggers.iter().map(|t| &t.node));
        match &self.kind {
            GunKind::None => {}
            GunKind::Revolver {
                chambers,
                cylinder,
                shoot_bullet_pos,
                ..
            } => nodes.extend(chambers.iter().chain([cylinder, shoot_bullet_pos])),
            GunKind::BbGun { bullets, .. } => nodes.extend(bullets),
            GunKind::Magazine {
                mag,
                mag_pos,
                mag_prefab,
                ..
            } => {
                nodes.extend([mag, mag_pos]);
                check(
                    m.item_prototypes
                        .iter()
                        .any(|p| p.source == *mag_prefab && p.behavior.ammo_rules.is_some()),
                    "magazine prefab",
                )?;
            }
        }
        check(nodes.into_iter().all(node), "transform links")?;

        let c = &self.clips;
        let s = &self.sounds;
        let turn = match &self.kind {
            GunKind::Revolver { turn_clip, .. } => turn_clip.as_ref(),
            _ => None,
        };
        check(
            [
                &c.one_shot,
                &c.false_shot,
                &c.hit_sand,
                &c.hit_concrete,
                &c.hit_flesh,
                &c.hit_glass,
            ]
            .into_iter()
            .flatten()
            .chain(
                [&s.open, &s.open_end, &s.close, &s.reload]
                    .into_iter()
                    .flatten()
                    .map(|s| &s.clip),
            )
            .chain(&s.r#move)
            .chain(turn)
            .all(clip),
            "audio clips",
        )?;
        check(
            self.crosshair
                .icon
                .as_ref()
                .is_none_or(|icon| m.ui.sprite(icon).is_some()),
            "crosshair icon",
        )?;
        check(
            self.conditional_trigger_triggers
                .iter()
                .all(|t| t.r#use.len() >= self.trigger_triggers.len()),
            "conditional trigger flags",
        )?;
        let curves = self
            .flash
            .iter()
            .map(|f| &f.curve)
            .chain(self.recoil.iter().map(|r| &r.curve))
            .chain(self.barrels.iter().filter_map(|b| b.curve.as_ref()))
            .chain(
                self.triggers
                    .iter()
                    .flat_map(|t| [&t.angle_curve, &t.pos_curve]),
            );
        check(
            curves.into_iter().all(particles::curve_valid)
                && self.barrels.iter().all(|b| b.axis.is_none_or(|a| a < 3)),
            "curves",
        )?;
        let fr = &self.fr;
        check(
            finite(&[
                self.max_condition_weakness,
                self.shot_count_spread,
                self.min_shoot_time,
                self.audio_m,
                self.explode_chance,
                fr.value,
                fr.fr_speed,
                fr.shoot.value,
                fr.shoot.near_value,
                fr.reload.value,
                fr.reload.near_value,
            ]) && finite(&self.crosshair.color)
                && finite(&self.crosshair.empty_color),
            "values",
        )
    }
}
