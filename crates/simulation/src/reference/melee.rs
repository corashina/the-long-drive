//! Checked melee weapons and hit effects: meleeweaponscript records, controllers,
//! mainscript hit particles, hit sounds and hit volumes.
use super::*;
pub mod motion;
pub use motion::*;

/// mainscript.particle order used by meleeweaponscript.
pub const MELEE_HIT_PARTICLES: [&str; 5] = ["spark", "blood", "plant", "wood", "sand"];
/// The shovel keeps its checked source values. Other weapons get structure and link checks.
pub const SHOVEL_ITEM_ID: u32 = 216;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MeleeReference {
    pub source: SourceId,
    /// mainscript.hitVolume in soundHitType order: metal, blood, plant, wood, sand, glass.
    pub hit_volumes: [f32; 6],
    /// mainscript.Particles in `MELEE_HIT_PARTICLES` order.
    pub particles: [ParticleReference; 5],
    /// mainscript.meleeHitSounds in soundHitType order.
    pub hit_clips: [Vec<SourceId>; 6],
    /// tosaveitemscript.IsRecentlyUnburied: an unburied item stays recent for this time.
    pub recent_recovery_seconds: f32,
    /// One record per world prototype with a meleeweaponscript, sorted by prototype.
    pub weapons: Vec<MeleeWeaponReference>,
    /// One motion per distinct weapon controller, sorted by controller.
    pub motions: Vec<MeleeMotion>,
}

/// meleeweaponscript of one world prototype.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MeleeWeaponReference {
    pub prototype: PrototypeId,
    pub source: SourceId,
    pub pickup: SourceId,
    /// `T`: the tip.
    pub target: SourceId,
    pub trigger: SourceId,
    pub controller: SourceId,
    pub contact_mask: u32,
    pub speed: f32,
    pub shovel_speed: f32,
    pub attack_volume: f32,
    pub attack_clips: Vec<SourceId>,
    pub right_hand_poses: Vec<SourceId>,
    pub left_hand_poses: Vec<SourceId>,
    pub contact: MeleeContact,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MeleeContact {
    pub force: f32,
    pub damage: f32,
    pub damage_tank: f32,
    pub hurt_food: bool,
    pub all_particles: f32,
    pub spark: f32,
    pub sand: f32,
    pub blood: f32,
    pub plant: f32,
    pub wood: f32,
}

impl MeleeReference {
    /// The weapon record of the item 216 prototype.
    pub fn shovel(&self, m: &ReferenceManifest) -> Option<&MeleeWeaponReference> {
        let p = m
            .item_prototypes
            .iter()
            .find(|p| p.item_id == Some(SHOVEL_ITEM_ID))?;
        self.weapons.iter().find(|w| w.prototype == p.id)
    }
    pub fn motion(&self, w: &MeleeWeaponReference) -> Option<&MeleeMotion> {
        self.motions.iter().find(|m| m.controller == w.controller)
    }
    pub fn validate(&self, m: &ReferenceManifest) -> Result<(), ReferenceError> {
        require(
            self.source.file == "level1"
                && self.hit_volumes.iter().all(|v| v.is_finite() && *v > 0.)
                && self.recent_recovery_seconds == 1.
                && self
                    .particles
                    .iter()
                    .zip(MELEE_HIT_PARTICLES)
                    .all(|(p, name)| p.role.strip_prefix("melee_") == Some(name)),
            "melee hit effects",
        )?;
        for p in &self.particles {
            conditions::validate_particle(&self.source, p, m)?;
        }
        for (i, w) in self.weapons.iter().enumerate() {
            require(
                self.weapons[..i]
                    .iter()
                    .all(|v| v.source != w.source && v.prototype != w.prototype),
                "melee weapon identity",
            )?;
            require(
                self.motions
                    .iter()
                    .filter(|v| v.controller == w.controller)
                    .count()
                    == 1,
                "melee weapon controller",
            )?;
            w.validate(m)?;
        }
        for (i, motion) in self.motions.iter().enumerate() {
            require(
                self.motions[..i]
                    .iter()
                    .all(|v| v.controller != motion.controller)
                    && self
                        .weapons
                        .iter()
                        .any(|w| w.controller == motion.controller),
                "melee motion owner",
            )?;
            motion.validate(m)?;
        }
        let shovel = self
            .shovel(m)
            .ok_or_else(|| ReferenceError("Missing melee shovel".into()))?;
        require(
            self.motion(shovel).is_some_and(MeleeMotion::is_shovel),
            "shovel motion",
        )?;
        shovel.validate_shovel(self, m)?;
        for clip in self
            .weapons
            .iter()
            .flat_map(|w| &w.attack_clips)
            .chain(self.hit_clips.iter().flatten())
        {
            require(
                m.audio["clips"].as_array().is_some_and(|clips| {
                    clips.iter().any(|c| {
                        c["source"]["file"].as_str() == Some(clip.file.as_str())
                            && c["source"]["path_id"].as_i64() == Some(clip.path_id)
                    })
                }),
                "melee audio resource",
            )?;
        }
        Ok(())
    }
}

impl MeleeWeaponReference {
    fn validate(&self, m: &ReferenceManifest) -> Result<(), ReferenceError> {
        let prototype = m
            .world
            .prototypes
            .iter()
            .find(|p| p.source == self.prototype.0)
            .ok_or_else(|| ReferenceError("melee weapon world prototype".into()))?;
        require(
            [&self.target, &self.trigger]
                .into_iter()
                .chain(&self.right_hand_poses)
                .chain(&self.left_hand_poses)
                .all(|s| prototype.nodes.iter().any(|n| n.source == *s)),
            "melee weapon transform links",
        )?;
        let c = &self.contact;
        require(
            [self.speed, self.shovel_speed, self.attack_volume]
                .into_iter()
                .chain([
                    c.force,
                    c.damage,
                    c.damage_tank,
                    c.all_particles,
                    c.spark,
                    c.sand,
                    c.blood,
                    c.plant,
                    c.wood,
                ])
                .all(f32::is_finite),
            "melee weapon values",
        )
    }
    /// Checked source values of the shovel and its links to the item 216 prototype.
    fn validate_shovel(
        &self,
        melee: &MeleeReference,
        m: &ReferenceManifest,
    ) -> Result<(), ReferenceError> {
        require(
            self.right_hand_poses.is_empty() && self.left_hand_poses.is_empty(),
            "shovel hand pose arrays",
        )?;
        require(
            self.contact
                == MeleeContact {
                    force: 20000.,
                    damage: 40.,
                    damage_tank: 12.,
                    hurt_food: true,
                    all_particles: 0.01,
                    spark: 0.5,
                    sand: 2.,
                    blood: 0.5,
                    plant: 1.,
                    wood: 0.5,
                },
            "shovel contact inputs",
        )?;
        let p = m
            .item_prototypes
            .iter()
            .find(|p| p.id == self.prototype)
            .ok_or_else(|| ReferenceError("shovel prototype link".into()))?;
        let behavior = &p.behavior;
        require(
            behavior.melee.as_ref() == Some(&self.source)
                && behavior.attack_target.as_ref() == Some(&self.target)
                && behavior.attack_trigger.as_ref() == Some(&self.trigger)
                && p.pickups
                    .first()
                    .is_some_and(|p| p.pickup.source == self.pickup)
                && self.contact_mask == 8552961
                && self.speed == 0.7
                && self.shovel_speed == 0.5
                && self.attack_volume == 0.5
                && self.attack_clips == behavior.clips
                && melee.hit_clips.as_slice() == behavior.contact_clips,
            "shovel source contact rules",
        )
    }
}
