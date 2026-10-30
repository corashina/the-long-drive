//! Checked explosions: mainscript explosion prefabs, the tank hole prefab, player blast
//! fields and explosive fluids.
use super::*;

/// mainscript.GetExplosionType: 0 and 1 ExplosionRed, 2 ExplosionBlue, 3 ExplosionPaint.
pub const EXPLOSION_TYPES: usize = 4;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExplosionsReference {
    pub source: SourceId,
    /// GetExplosionType prefabs in type order.
    pub types: [ExplosionType; EXPLOSION_TYPES],
    /// mainscript.tankcaphole: the leak opening of a shot tank.
    pub hole: ExplosionHole,
    pub player: ExplosionPlayer,
    /// mainscript.explosivefluids: fluid type indices.
    pub explosive_fluids: Vec<u8>,
}

/// explosionscript of one explosion prefab.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExplosionType {
    /// The prefab root Transform.
    pub prefab: SourceId,
    pub source: SourceId,
    pub explosion_force: f32,
    /// Unused by explosionscript.Explode.
    pub multiplier: f32,
    pub explosion_dist: f32,
    /// explosionscript.particle; its pose is under the prefab root.
    pub particle: ParticleReference,
    /// explosionscript.Source.
    pub sound: ExplosionSound,
}

/// One AudioSource and its clip.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExplosionSound {
    pub source: SourceId,
    pub clip: SourceId,
    pub volume: f32,
}

/// tankcapscript pour fields of the hole prefab.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExplosionHole {
    pub prefab: SourceId,
    pub source: SourceId,
    pub radius: f32,
    pub valve: f32,
    pub def_pour_angle: f32,
    pub tune_speed: f32,
    pub min_rate: f32,
    pub max_rate: f32,
    pub no_angle: bool,
    pub no_pour: bool,
    pub cant_tube: bool,
    pub can_toggle: bool,
    pub can_tune: bool,
    pub has_name: bool,
    pub no_text: bool,
}

/// fpscontroller blast fields.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExplosionPlayer {
    pub source: SourceId,
    /// minForceToDieFromExplosion.
    pub min_force_to_die: f32,
    pub hear_loss_modifier: f32,
    pub hear_regen: f32,
    pub ear_ringing_gain: f32,
    pub ear_ringing_regen: f32,
    /// EarRingSource.
    pub ear_ring: ExplosionSound,
}

impl ExplosionsReference {
    pub fn validate(&self, m: &ReferenceManifest) -> Result<(), ReferenceError> {
        let clips = m.audio["clips"].as_array();
        let sound = |s: &ExplosionSound| {
            s.volume.is_finite()
                && clips.is_some_and(|clips| {
                    clips.iter().any(|c| {
                        c["source"]["file"].as_str() == Some(s.clip.file.as_str())
                            && c["source"]["path_id"].as_i64() == Some(s.clip.path_id)
                    })
                })
        };
        for (i, t) in self.types.iter().enumerate() {
            require(
                finite(&[t.explosion_force, t.multiplier, t.explosion_dist])
                    && t.particle.role == format!("explosion_{i}")
                    && sound(&t.sound),
                "explosion type",
            )?;
            conditions::validate_particle(&t.source, &t.particle, m)?;
        }
        let h = &self.hole;
        require(
            finite(&[h.radius, h.def_pour_angle, h.min_rate, h.max_rate])
                && valid_tuning(h.valve, h.can_tune, h.tune_speed),
            "explosion hole",
        )?;
        let p = &self.player;
        require(
            finite(&[
                p.min_force_to_die,
                p.hear_loss_modifier,
                p.hear_regen,
                p.ear_ringing_gain,
                p.ear_ringing_regen,
            ]) && sound(&p.ear_ring),
            "explosion player",
        )?;
        let fluids = &self.explosive_fluids;
        // mainscript.fluidenum has 6 types.
        require(
            !fluids.is_empty()
                && fluids
                    .iter()
                    .enumerate()
                    .all(|(i, f)| *f < 6 && !fluids[..i].contains(f)),
            "explosive fluids",
        )
    }
}
