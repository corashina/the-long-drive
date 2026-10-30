use super::*;
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ParticleCurve {
    pub mode: u8,
    pub min_scalar: f32,
    pub scalar: f32,
    pub min_curve: TorqueCurve,
    pub max_curve: TorqueCurve,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ParticleColor {
    pub mode: u8,
    pub min_color: [f32; 4],
    pub max_color: [f32; 4],
    pub min_gradient: GradientReference,
    pub max_gradient: GradientReference,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ParticleLimit {
    pub source: SourceId,
    pub module: String,
    pub visible_effect: String,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ParticleReference {
    pub source: SourceId,
    pub role: String,
    pub capacity: u32,
    pub rate_over_time: f32,
    pub rate_over_distance: f32,
    pub duration: f32,
    pub simulation_space: u8,
    pub local_pose: Transform,
    pub module_flags: HashMap<String, bool>,
    pub curves: HashMap<String, ParticleCurve>,
    pub colors: HashMap<String, ParticleColor>,
    pub vectors: HashMap<String, [f32; 3]>,
    pub scalars: HashMap<String, f32>,
    pub flags: HashMap<String, bool>,
    pub links: HashMap<String, SourceId>,
    pub materials: Vec<SkyMaterialReference>,
    pub limits: Vec<ParticleLimit>,
}
impl ParticleReference {
    pub fn validate_motion(&self) -> Result<(), ReferenceError> {
        let invalid = |field: &str| {
            ReferenceError(format!(
                "Invalid particle {}:{} field {field}",
                self.source.file, self.source.path_id
            ))
        };
        let scalar = |field: &str| {
            self.scalars
                .get(field)
                .copied()
                .filter(|v| v.is_finite())
                .ok_or_else(|| invalid(field))
        };
        let flag = |field: &str| self.flags.get(field).copied().ok_or_else(|| invalid(field));
        let curve = |field: &str| {
            let c = self.curves.get(field).ok_or_else(|| invalid(field))?;
            if c.mode > 3
                || !finite(&[c.scalar, c.min_scalar])
                || !curve_valid(&c.min_curve)
                || !curve_valid(&c.max_curve)
                || ((c.mode == 1 || c.mode == 2) && c.max_curve.keys.is_empty())
                || (c.mode == 2 && c.min_curve.keys.is_empty())
            {
                return Err(invalid(field));
            }
            Ok(c)
        };
        if self.module_flags.get("VelocityModule") == Some(&true) {
            flag("VelocityModule.inWorldSpace")?;
            for field in [
                "VelocityModule.x",
                "VelocityModule.y",
                "VelocityModule.z",
                "VelocityModule.speedModifier",
            ] {
                curve(field)?;
            }
            // Checked selected systems have no orbital or radial motion.
            for field in [
                "VelocityModule.orbitalX",
                "VelocityModule.orbitalY",
                "VelocityModule.orbitalZ",
                "VelocityModule.orbitalOffsetX",
                "VelocityModule.orbitalOffsetY",
                "VelocityModule.orbitalOffsetZ",
                "VelocityModule.radial",
            ] {
                let c = curve(field)?;
                if c.mode != 0 || c.scalar != 0. {
                    return Err(invalid(field));
                }
            }
        }
        if self.module_flags.get("NoiseModule") == Some(&true) {
            for field in [
                "NoiseModule.strength",
                "NoiseModule.strengthY",
                "NoiseModule.strengthZ",
                "NoiseModule.positionAmount",
                "NoiseModule.scrollSpeed",
            ] {
                curve(field)?;
            }
            flag("NoiseModule.separateAxes")?;
            flag("NoiseModule.damping")?;
            if scalar("NoiseModule.frequency")? <= 0. {
                return Err(invalid("NoiseModule.frequency"));
            }
            let octaves = scalar("NoiseModule.octaves")?;
            if !(1. ..=4.).contains(&octaves) || octaves.fract() != 0. {
                return Err(invalid("NoiseModule.octaves"));
            }
            scalar("NoiseModule.octaveMultiplier")?;
            scalar("NoiseModule.octaveScale")?;
            // The runtime moves positions only. Remap, rotation and size noise are not run.
            if flag("NoiseModule.remapEnabled")? {
                return Err(invalid("NoiseModule.remapEnabled"));
            }
            for field in ["NoiseModule.rotationAmount", "NoiseModule.sizeAmount"] {
                let c = curve(field)?;
                if c.mode != 0 || c.scalar != 0. {
                    return Err(invalid(field));
                }
            }
        }
        if self.module_flags.get("InheritVelocityModule") == Some(&true) {
            let mode = scalar("InheritVelocityModule.m_Mode")?;
            if mode != 0. && mode != 1. {
                return Err(invalid("InheritVelocityModule.m_Mode"));
            }
            curve("InheritVelocityModule.m_Curve")?;
            if scalar("emitterVelocityMode")? != 1. {
                return Err(invalid("emitterVelocityMode"));
            }
        }
        if self.module_flags.get("CollisionModule") == Some(&true) {
            for field in [
                "CollisionModule.m_Bounce",
                "CollisionModule.m_Dampen",
                "CollisionModule.m_EnergyLossOnCollision",
            ] {
                let c = curve(field)?;
                if !(0. ..=1.).contains(&c.scalar) || !(0. ..=1.).contains(&c.min_scalar) {
                    return Err(invalid(field));
                }
            }
            for (field, value) in [
                ("CollisionModule.type", 1.),
                ("CollisionModule.collisionMode", 0.),
                ("CollisionModule.colliderForce", 0.),
            ] {
                if scalar(field)? != value {
                    return Err(invalid(field));
                }
            }
            flag("CollisionModule.collidesWithDynamic")?;
            if flag("CollisionModule.interiorCollisions")? {
                return Err(invalid("CollisionModule.interiorCollisions"));
            }
            if scalar("CollisionModule.radiusScale")? <= 0. {
                return Err(invalid("CollisionModule.radiusScale"));
            }
            let mask = scalar("CollisionModule.collidesWith.m_Bits")?;
            if mask < 0. || mask > u32::MAX as f32 || mask.fract() != 0. {
                return Err(invalid("CollisionModule.collidesWith.m_Bits"));
            }
            let min = scalar("CollisionModule.minKillSpeed")?;
            if min < 0. || scalar("CollisionModule.maxKillSpeed")? < min {
                return Err(invalid("CollisionModule.maxKillSpeed"));
            }
        }
        Ok(())
    }
}
pub(super) fn valid_geometry(g: &ColliderGeometry) -> bool {
    match g {
        ColliderGeometry::Box {
            center,
            half_extents,
            rotation,
        } => {
            finite(center)
                && finite(half_extents)
                && half_extents.iter().all(|v| *v > 0.)
                && finite(rotation)
        }
        ColliderGeometry::Sphere { center, radius } => {
            finite(center) && radius.is_finite() && *radius > 0.
        }
        ColliderGeometry::Capsule { a, b, radius } => {
            finite(a) && finite(b) && radius.is_finite() && *radius > 0.
        }
        ColliderGeometry::Convex { points } => {
            points.len() >= 4 && points.iter().all(|p| finite(p))
        }
        ColliderGeometry::Trimesh { points, triangles } => {
            points.len() >= 3
                && points.iter().all(|p| finite(p))
                && !triangles.is_empty()
                && triangles
                    .iter()
                    .flatten()
                    .all(|i| (*i as usize) < points.len())
        }
    }
}
pub(super) fn gradient_valid(g: &GradientReference) -> bool {
    g.mode <= 1
        && !g.colors.is_empty()
        && !g.alphas.is_empty()
        && g.colors
            .iter()
            .all(|k| k.time.is_finite() && (0. ..=1.).contains(&k.time) && finite(&k.value))
        && g.alphas
            .iter()
            .all(|k| finite(&[k.time, k.value]) && (0. ..=1.).contains(&k.time))
        && g.colors.windows(2).all(|w| w[0].time <= w[1].time)
        && g.alphas.windows(2).all(|w| w[0].time <= w[1].time)
}
pub(super) fn curve_valid(c: &TorqueCurve) -> bool {
    c.keys.iter().all(|k| {
        finite(&[
            k.time,
            k.value,
            k.in_tangent,
            k.out_tangent,
            k.in_weight,
            k.out_weight,
        ]) && k.weighted_mode <= 3
    }) && c.keys.windows(2).all(|w| w[0].time < w[1].time)
}
pub(super) fn validate_particles(m: &ReferenceManifest) -> Result<(), ReferenceError> {
    require(
        m.particles.len() == 2 + 2 * m.openings.len(),
        "selected particles",
    )?;
    ids(m.particles.iter().map(|p| &p.source), "duplicate particle")?;
    ids(
        m.particles.iter().map(|p| p.role.as_str()),
        "duplicate particle role",
    )?;
    for role in ["urine", "urine_splash"] {
        require(
            m.particles.iter().any(|p| p.role == role),
            "player particle roles",
        )?;
    }
    for o in &m.openings {
        for role in [format!("outlet_{}", o.id.0), format!("splash_{}", o.id.0)] {
            require(
                m.particles.iter().any(|p| p.role == role),
                "outlet particle roles",
            )?;
        }
    }
    for p in &m.particles {
        validate_particle(p, 2)?;
    }
    Ok(())
}
/// Checks one particle record that the runtime simulates in `simulation_space`.
pub(super) fn validate_particle(
    p: &ParticleReference,
    simulation_space: u8,
) -> Result<(), ReferenceError> {
    p.validate_motion()?;
    let check = |valid: bool, field: &str| {
        if valid {
            Ok(())
        } else {
            Err(ReferenceError(format!(
                "Invalid particle {}:{} role {} field {field}",
                p.source.file, p.source.path_id, p.role
            )))
        }
    };
    let scalar = |field: &str| -> Result<f32, ReferenceError> {
        check(p.scalars.contains_key(field), field)?;
        let value = p.scalars[field];
        check(value.is_finite(), field)?;
        Ok(value)
    };
    for name in ["gameObject.active", "renderer.enabled"] {
        check(p.flags.contains_key(name), name)?;
    }
    for name in ["RotationModule", "EmissionModule"] {
        check(p.module_flags.contains_key(name), name)?;
    }
    if p.module_flags.get("ShapeModule") == Some(&true) {
        validate_shape(p, &check, &scalar)?;
    }
    for name in [
        "InitialModule.startSize",
        "InitialModule.startSpeed",
        "InitialModule.startRotation",
        "InitialModule.startLifetime",
        "InitialModule.gravityModifier",
        "SizeModule.curve",
    ] {
        check(p.curves.contains_key(name), name)?;
    }
    for name in ["InitialModule.startColor", "ColorModule.gradient"] {
        check(p.colors.contains_key(name), name)?;
    }
    let layer = scalar("gameObject.layer")?;
    check(
        (0. ..=31.).contains(&layer) && layer.fract() == 0.,
        "gameObject.layer",
    )?;
    let sort = scalar("renderer.m_SortMode")?;
    check(
        (0. ..=5.).contains(&sort) && sort.fract() == 0.,
        "renderer.m_SortMode",
    )?;
    let min_size = scalar("renderer.m_MinParticleSize")?;
    let max_size = scalar("renderer.m_MaxParticleSize")?;
    check(min_size >= 0., "renderer.m_MinParticleSize")?;
    check(max_size >= min_size, "renderer.m_MaxParticleSize")?;
    scalar("renderer.m_SortingFudge")?;
    let ring = scalar("ringBufferMode")?;
    check(
        (0. ..=2.).contains(&ring) && ring.fract() == 0.,
        "ringBufferMode",
    )?;
    if p.module_flags["RotationModule"] {
        check(
            p.curves.contains_key("RotationModule.curve"),
            "RotationModule.curve",
        )?;
    }
    if p.module_flags["EmissionModule"] {
        scalar("EmissionModule.m_BurstCount")?;
    }
    if p.scalars.contains_key("EmissionModule.m_BurstCount") {
        let bursts = scalar("EmissionModule.m_BurstCount")?;
        check(
            (0. ..=1.).contains(&bursts) && bursts.fract() == 0.,
            "EmissionModule.m_BurstCount",
        )?;
        if bursts == 1. {
            let name = "EmissionModule.m_Bursts.0.countCurve";
            check(p.curves.contains_key(name), name)?;
            check(p.curves[name].scalar >= 0., name)?;
        }
    }
    if p.role == "urine" || p.role.starts_with("outlet_") {
        for module in ["CollisionModule", "ShapeModule"] {
            check(p.module_flags.get(module) == Some(&true), module)?;
        }
        for name in [
            "CollisionModule.m_Bounce",
            "CollisionModule.m_Dampen",
            "CollisionModule.m_EnergyLossOnCollision",
        ] {
            check(p.curves.contains_key(name), name)?;
            check((0. ..=1.).contains(&p.curves[name].scalar), name)?;
        }
        check(
            p.curves["InitialModule.startLifetime"].scalar > 0.,
            "InitialModule.startLifetime.scalar",
        )?;
        check(
            p.curves["InitialModule.startSize"].scalar > 0.,
            "InitialModule.startSize.scalar",
        )?;
        check(
            scalar("CollisionModule.radiusScale")? > 0.,
            "CollisionModule.radiusScale",
        )?;
        let mask = scalar("CollisionModule.collidesWith.m_Bits")?;
        // Unity exports its unsigned layer mask through a floating-point map.
        check(
            mask >= 0. && mask <= u32::MAX as f32 && mask.fract() == 0.,
            "CollisionModule.collidesWith.m_Bits",
        )?;
        check(scalar("outlet.maxSpeed")? >= 0., "outlet.maxSpeed")?;
        let min_rate = scalar("outlet.minRate")?;
        check(min_rate >= 0., "outlet.minRate")?;
        check(scalar("outlet.maxRate")? >= min_rate, "outlet.maxRate")?;
        for index in 0..6 {
            let name = format!("fluid.{index}");
            check(p.vectors.contains_key(&name), &name)?;
            scalar(&format!("fluid.{index}.alpha"))?;
        }
        check(
            p.flags.contains_key("outlet.forceColor"),
            "outlet.forceColor",
        )?;
        if p.flags["outlet.forceColor"] {
            check(
                p.vectors.contains_key("outlet.forceColor"),
                "outlet.forceColor",
            )?;
            scalar("outlet.forceAlpha")?;
            check(
                (0. ..=1.).contains(&scalar("outlet.forceColorM")?),
                "outlet.forceColorM",
            )?;
        }
    }
    p.validate_data(simulation_space)
}
/// Checks the fields of an enabled shape module.
fn validate_shape(
    p: &ParticleReference,
    check: &impl Fn(bool, &str) -> Result<(), ReferenceError>,
    scalar: &impl Fn(&str) -> Result<f32, ReferenceError>,
) -> Result<(), ReferenceError> {
    // Checked storm and coolant steam systems use a hemisphere (2), food bites a sphere (0).
    // Other selected systems use a cone (4).
    let shape = if p.role.starts_with("storm_") || p.role.starts_with("coolant_steam_") {
        2.
    } else if p.role.starts_with("food_") {
        0.
    } else {
        4.
    };
    check(scalar("ShapeModule.type")? == shape, "ShapeModule.type")?;
    for name in ["ShapeModule.m_Rotation", "ShapeModule.m_Scale"] {
        check(p.vectors.contains_key(name), name)?;
    }
    check(
        (0. ..=90.).contains(&scalar("ShapeModule.angle")?),
        "ShapeModule.angle",
    )?;
    check(
        scalar("ShapeModule.radius.value")? >= 0.,
        "ShapeModule.radius.value",
    )?;
    check(
        (0. ..=1.).contains(&scalar("ShapeModule.radiusThickness")?),
        "ShapeModule.radiusThickness",
    )?;
    check(
        scalar("ShapeModule.randomPositionAmount")? >= 0.,
        "ShapeModule.randomPositionAmount",
    )
}
impl ParticleReference {
    pub fn validate_data(&self, simulation_space: u8) -> Result<(), ReferenceError> {
        let p = self;
        let check =
            |valid: bool, field: &str| require(valid, &format!("particle {} {field}", p.role));
        require(
            p.capacity > 0
                && p.capacity <= 1_000_000
                && p.scalars.get("InitialModule.maxNumParticles") == Some(&(p.capacity as f32)),
            "particle capacity",
        )?;
        require(
            finite(&[p.rate_over_time, p.rate_over_distance, p.duration])
                && p.rate_over_time >= 0.
                && p.rate_over_distance >= 0.
                && p.duration > 0.
                && p.simulation_space == simulation_space
                && transform(&p.local_pose),
            "particle rates",
        )?;
        require(
            p.module_flags.get("InitialModule") == Some(&true)
                && p.curves.contains_key("InitialModule.startLifetime")
                && !p.materials.is_empty(),
            "particle modules",
        )?;
        for (name, value) in &p.scalars {
            check(value.is_finite(), name)?;
        }
        for (name, value) in &p.vectors {
            check(finite(value), name)?;
        }
        for (name, c) in &p.curves {
            check(
                c.mode <= 3
                    && finite(&[c.scalar, c.min_scalar])
                    && curve_valid(&c.min_curve)
                    && curve_valid(&c.max_curve),
                name,
            )?;
        }
        for (name, c) in &p.colors {
            check(
                c.mode <= 4
                    && finite(&c.min_color)
                    && finite(&c.max_color)
                    && gradient_valid(&c.min_gradient)
                    && gradient_valid(&c.max_gradient),
                name,
            )?;
        }
        check(p.materials.len() == 1, "materials")?;
        let material = &p.materials[0];
        check(
            material.textures.iter().any(|t| t.property == "_MainTex"),
            "materials.textures._MainTex",
        )?;
        let tint = if material.shader.name == "Legacy Shaders/Particles/Alpha Blended" {
            "_TintColor"
        } else {
            "_Color"
        };
        check(
            material.colors.get(tint).is_some_and(|v| finite(v)),
            &format!("materials.colors.{tint}"),
        )?;
        for (name, c) in &p.curves {
            check(
                (c.mode != 1 && c.mode != 2 || !c.max_curve.keys.is_empty())
                    && (c.mode != 2 || !c.min_curve.keys.is_empty()),
                name,
            )?;
        }

        for l in &p.limits {
            require(
                l.source == p.source
                    && (p.module_flags.get(&l.module) == Some(&true)
                        || l.module == "material.soft_particles"
                        || l.module == "material.distortion")
                    && !l.visible_effect.is_empty(),
                "particle limit",
            )?;
        }
        Ok(())
    }
}
