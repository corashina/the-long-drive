//! Wheel ground effects: source `talajnezo` per wheel, `carscript` gains and `mainscript` skid data.
use super::*;

/// One source `talajnezo`, in car wheel order.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WheelEffectsReference {
    pub source: SourceId,
    /// The talajnezo node pose relative to the vehicle root.
    pub pose: Transform,
    /// rateM: dust rate per m/s on sand.
    pub rate: f32,
    /// LM: layers that the ground ray hits.
    pub ray_mask: u32,
    /// The talajnezo gains when its Car link is null. Such a wheel has no slide sound.
    pub own_gains: Option<WheelGains>,
    /// ps. The runtime sets the rate.
    pub dust: Option<ParticleReference>,
    /// grasstrail first if present, then grasss.
    pub grass: Vec<ParticleReference>,
}

/// talajnezo groundSoundM, springSoundM and springPitchM.
#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WheelGains {
    pub ground: f32,
    pub spring: f32,
    pub spring_pitch: f32,
}

/// carscript groundSoundM, springSoundM, springPitchM, slideAngle and maxSlideVolumeSpeed.
#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EffectGains {
    pub ground: f32,
    pub spring: f32,
    pub spring_pitch: f32,
    pub slide_angle: f32,
    pub max_slide_volume_speed: f32,
}

/// mainscript skid data. The slide voice plays `road_clip` or `dirt_clip`.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SkidReference {
    pub source: SourceId,
    /// Audio clip ids of SSkidRoad and SSkidDirt.
    pub road_clip: u32,
    pub dirt_clip: u32,
    pub road_volume: f32,
    pub road_pitch: [f32; 2],
    pub dirt_volume: f32,
    pub dirt_pitch: [f32; 2],
    /// CarMinSpeedDif and CarMaxSpeedDif in km/h.
    pub speed_difference: [f32; 2],
    /// tireSmokeRate: burst count at slide level 1.
    pub smoke_rate: f32,
    /// mainscript.Particles `tiresmoke` (5) and `tiresand` (6).
    pub particles: [ParticleReference; 2],
}

pub(super) fn validate(m: &ReferenceManifest) -> Result<(), ReferenceError> {
    let world = m
        .world
        .prototypes
        .iter()
        .filter_map(|p| p.vehicle.as_ref())
        .map(|v| (&v.effect_gains, &v.wheel_effects[..], v.wheels.len()));
    for (g, wheels, count) in world {
        require(
            wheels.len() == count
                && finite(&[
                    g.ground,
                    g.spring,
                    g.spring_pitch,
                    g.slide_angle,
                    g.max_slide_volume_speed,
                ]),
            "vehicle effect gains",
        )?;
        for w in wheels {
            require(
                w.rate.is_finite()
                    && w.rate >= 0.
                    && transform(&w.pose)
                    // Particle groups number wheel systems wheel * 8 + system.
                    && w.grass.len() < 8
                    && w.own_gains
                        .is_none_or(|g| finite(&[g.ground, g.spring, g.spring_pitch])),
                "wheel effects",
            )?;
            for p in w.dust.iter().chain(&w.grass) {
                conditions::validate_particle(&w.source, p, m)?;
            }
        }
    }
    let s = &m.skid;
    let ordered = |r: [f32; 2]| r[0] <= r[1];
    require(
        s.source.file == "level1"
            && finite(&[s.road_volume, s.dirt_volume, s.smoke_rate])
            && finite(&s.road_pitch)
            && finite(&s.dirt_pitch)
            && finite(&s.speed_difference)
            && s.road_volume >= 0.
            && s.dirt_volume >= 0.
            && s.smoke_rate >= 0.
            && ordered(s.road_pitch)
            && ordered(s.dirt_pitch)
            && ordered(s.speed_difference)
            && s.particles[0].role == "skid_tiresmoke"
            && s.particles[1].role == "skid_tiresand",
        "skid",
    )?;
    for p in &s.particles {
        conditions::validate_particle(&s.source, p, m)?;
    }
    Ok(())
}
