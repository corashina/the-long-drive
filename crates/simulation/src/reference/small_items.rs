//! Checked spirit level, spray bottle, genie lamp, binoculars, start pack, mouth smoke and old heat haze inputs.
use super::*;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SmallItemsReference {
    pub spirit_level: SpiritLevelReference,
    pub spray_bottle: SprayBottleReference,
    pub genie_lamp: GenieLampReference,
    pub binoculars: BinocularsReference,
    pub start_packs: Vec<SmallItemStartReference>,
    pub exhale: ExhaleReference,
    /// fpscontroller `heatHaze`: the old particle heat haze in player root space.
    pub heat_haze: ParticleReference,
    pub washes: Vec<WashTriggerReference>,
    pub wash_effects: Vec<WashEffectReference>,
}

/// A washtriggerscript on a world prototype node: the building cobwebs.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WashTriggerReference {
    pub source: SourceId,
    pub prototype: SourceId,
    /// The trigger node (Transform).
    pub node: SourceId,
    /// Colliders on the node and below it (GetComponentInParent).
    pub colliders: Vec<SourceId>,
    /// CleanProperty: the clean table row.
    pub permission: u8,
    pub never_repair: bool,
    pub destroy_on_repair: bool,
    /// Nodes that GotClean deactivates.
    pub objects: Vec<SourceId>,
    pub chance_to_start_disabled: f32,
    pub particle_on_destroy: Option<SourceId>,
}

/// A particleOnDestroy prefab: its particle system and its play-on-awake sound.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WashEffectReference {
    pub source: SourceId,
    pub particle: ParticleReference,
    pub clip: SourceId,
    pub volume: f32,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SpiritLevelReference {
    pub item: u32,
    pub prototype: SourceId,
    pub source: SourceId,
    pub bubbles: Vec<BubbleReference>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BubbleReference {
    pub bubble: SourceId,
    pub t1: SourceId,
    pub t2: SourceId,
    pub lerp: f32,
    pub bubble_curve: TorqueCurve,
    pub scale_z_curve: TorqueCurve,
    pub scale_xy_curve: TorqueCurve,
    pub move_xy_curve: TorqueCurve,
    pub min_scale_xy: f32,
    pub max_scale_xy: f32,
    pub min_scale_z: f32,
    pub max_scale_z: f32,
    pub move_xy: f32,
    pub startup_value: f32,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SprayBottleReference {
    pub item: u32,
    pub prototype: SourceId,
    pub source: SourceId,
    pub ammo: AcquisitionAmmo,
    /// ammoscript.canPour: refill repeats every `pour_time` while R is held.
    pub can_pour: bool,
    pub pour_time: f32,
    pub cleaning_type: u8,
    pub text: u32,
    pub angle_speed: f32,
    pub trigger_max_angle: f32,
    pub trigger_set_angle: f32,
    pub trigger_reset_angle: f32,
    pub invert: bool,
    pub trigger: SourceId,
    pub hole: SourceId,
    pub set_clip: SourceId,
    pub reset_clip: SourceId,
    pub set_volume: f32,
    pub reset_volume: f32,
    pub mask: u32,
    pub gain: f32,
    pub distance: f32,
    pub wash_radius: f32,
    pub wash_mask: u32,
    pub spray: ParticleReference,
    pub splash: ParticleReference,
    pub startup: Vec<SmallItemStartReference>,
}

/// A StartSpawnItems copy. Its pose comes from the world startup records.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SmallItemStartReference {
    pub source: SourceId,
    pub transform: SourceId,
    pub item_id: u32,
    pub ammo: AcquisitionAmmo,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GenieLampReference {
    pub item: u32,
    pub prototype: SourceId,
    pub source: SourceId,
    pub light: SourceId,
    pub renderer: SourceId,
    pub material: SourceId,
    pub wash_trigger: SourceId,
    pub gain: f32,
    pub lose: f32,
    pub intensity: TorqueCurve,
    pub range: TorqueCurve,
    pub emission: GradientReference,
    pub startup_value: f32,
}

/// Source `tavcsoscript`.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BinocularsReference {
    pub item: u32,
    pub prototype: SourceId,
    pub source: SourceId,
    /// `CamR` Transform. The view moves to it.
    pub camera: SourceId,
    pub focus_distance: f32,
    pub focus_sensitivity: f32,
    pub min_focus: f32,
    pub max_focus: f32,
    pub min_fov: f32,
    pub max_fov: f32,
    pub startup: Vec<BinocularsStartReference>,
}

/// A StartSpawnItems binoculars copy.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BinocularsStartReference {
    pub source: SourceId,
    pub transform: SourceId,
    pub item_id: u32,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExhaleReference {
    pub controller: SourceId,
    pub node: NodeId,
    pub max_rate: f32,
    pub charge_speed: f32,
    pub particle: ParticleReference,
}

const SPIRIT_LEVEL_ITEM: u32 = 401;
const SPRAY_BOTTLE_ITEM: u32 = 121;
const GENIE_LAMP_ITEM: u32 = 211;
const BINOCULARS_ITEM: u32 = 85;
const PACK_ITEM: u32 = 74;
const START_BOTTLES: usize = 2;
const START_PACKS: usize = 3;
const COBWEBS: usize = 529;

fn invalid(field: &str) -> ReferenceError {
    ReferenceError(format!("Invalid small item field {field}"))
}

fn checked(ok: bool, field: &str) -> Result<(), ReferenceError> {
    if ok { Ok(()) } else { Err(invalid(field)) }
}

fn curve(c: &TorqueCurve) -> bool {
    !c.keys.is_empty() && particles::curve_valid(c)
}

impl SmallItemsReference {
    pub fn validate(&self, m: &ReferenceManifest) -> Result<(), ReferenceError> {
        let records: HashMap<&SourceId, &WorldSourceRecord> =
            m.world.records.iter().map(|r| (&r.source, r)).collect();
        let class = |source: &SourceId, name: &str| {
            records
                .get(source)
                .is_some_and(|r| r.class_name == name && r.fields.len() > 1)
        };
        // Each script lies on a selected catalog prototype; its nodes are prototype nodes.
        let prototype = |item: u32, name: &str, source: &SourceId| {
            let entry = m.world.catalog.items.get(item as usize);
            let owner = entry
                .filter(|i| i.index == item && i.name == name)
                .and_then(|i| i.prototype.as_ref())
                .filter(|p| *p == source)
                .and_then(|p| m.world.prototypes.iter().find(|w| &w.source == p));
            owner.ok_or_else(|| invalid(name))
        };
        let inside = |p: &WorldPrototypeReference, node: &SourceId| {
            p.nodes.iter().any(|n| &n.source == node)
        };

        let l = &self.spirit_level;
        let p = prototype(SPIRIT_LEVEL_ITEM, "waterlevel", &l.prototype)?;
        checked(
            l.item == SPIRIT_LEVEL_ITEM && class(&l.source, "bubbleLevelScript"),
            "spirit level",
        )?;
        checked(l.bubbles.len() == 3, "spirit level bubbles")?;
        for b in &l.bubbles {
            checked(
                [&b.bubble, &b.t1, &b.t2].iter().all(|n| inside(p, n))
                    && b.t1 != b.t2
                    && [
                        &b.bubble_curve,
                        &b.scale_z_curve,
                        &b.scale_xy_curve,
                        &b.move_xy_curve,
                    ]
                    .iter()
                    .all(|c| curve(c))
                    && finite(&[
                        b.lerp,
                        b.min_scale_xy,
                        b.max_scale_xy,
                        b.min_scale_z,
                        b.max_scale_z,
                        b.move_xy,
                        b.startup_value,
                    ])
                    && b.lerp > 0.
                    && b.min_scale_xy > 0.
                    && b.min_scale_xy <= b.max_scale_xy
                    && b.min_scale_z > 0.
                    && b.min_scale_z <= b.max_scale_z
                    && (0. ..=1.).contains(&b.startup_value),
                "spirit level bubble",
            )?;
        }

        let s = &self.spray_bottle;
        let p = prototype(SPRAY_BOTTLE_ITEM, "jizzonit", &s.prototype)?;
        let ammo = |a: &AcquisitionAmmo| {
            class(&a.source, "ammoscript")
                && finite(&[
                    a.float_count,
                    a.maximum,
                    a.random_range[0],
                    a.random_range[1],
                ])
                && a.maximum > 0.
        };
        checked(
            s.item == SPRAY_BOTTLE_ITEM
                && class(&s.source, "spricniscript")
                && ammo(&s.ammo)
                && inside(p, &s.trigger)
                && inside(p, &s.hole)
                && item_prototypes::clip_exists(m, &s.set_clip)
                && item_prototypes::clip_exists(m, &s.reset_clip)
                && finite(&[
                    s.angle_speed,
                    s.trigger_max_angle,
                    s.trigger_set_angle,
                    s.trigger_reset_angle,
                    s.set_volume,
                    s.reset_volume,
                    s.gain,
                    s.distance,
                    s.wash_radius,
                ])
                && s.angle_speed > 0.
                && 0. < s.trigger_reset_angle
                && s.trigger_reset_angle < s.trigger_set_angle
                && s.trigger_set_angle < s.trigger_max_angle
                && (0. ..=1.).contains(&s.set_volume)
                && (0. ..=1.).contains(&s.reset_volume)
                && s.gain > 0.
                && s.distance > 0.
                && s.wash_radius > 0.
                && s.pour_time.is_finite()
                && s.pour_time >= 0.
                && s.startup.len() == START_BOTTLES,
            "spray bottle",
        )?;
        conditions::validate_particle(&s.source, &s.spray, m)?;
        conditions::validate_particle(&s.source, &s.splash, m)?;
        let start = |v: &SmallItemStartReference, script: &str, item: u32| {
            v.source.file == "level1"
                && class(&v.source, script)
                && records
                    .get(&v.transform)
                    .is_some_and(|r| r.class_name == "Transform")
                && v.item_id == item
                && ammo(&v.ammo)
        };
        for v in &s.startup {
            checked(
                start(v, "spricniscript", SPRAY_BOTTLE_ITEM) && v.ammo.r#type == s.ammo.r#type,
                "start spray bottle",
            )?;
        }

        let g = &self.genie_lamp;
        let p = prototype(GENIE_LAMP_ITEM, "ginlamp", &g.prototype)?;
        let on_prototype = |source: &SourceId, name: &str| {
            records.get(source).is_some_and(|r| {
                r.class_name == name
                    && matches!(r.fields.get("m_GameObject"), Some(WorldValue::Link(go))
                        if records.contains_key(go))
            })
        };
        checked(
            g.item == GENIE_LAMP_ITEM
                && class(&g.source, "ginlampscript")
                && class(&g.wash_trigger, "washtriggerscript")
                && on_prototype(&g.light, "Light")
                && on_prototype(&g.renderer, "MeshRenderer")
                && !p.nodes.is_empty()
                && !g.material.file.is_empty()
                && curve(&g.intensity)
                && curve(&g.range)
                && particles::gradient_valid(&g.emission)
                && finite(&[g.gain, g.lose, g.startup_value])
                && 0. < g.lose
                && g.lose < g.gain
                && (0. ..=1.).contains(&g.startup_value),
            "genie lamp",
        )?;

        let b = &self.binoculars;
        let p = prototype(BINOCULARS_ITEM, "binoculars", &b.prototype)?;
        checked(
            b.item == BINOCULARS_ITEM
                && class(&b.source, "tavcsoscript")
                && inside(p, &b.camera)
                && finite(&[
                    b.focus_distance,
                    b.focus_sensitivity,
                    b.min_focus,
                    b.max_focus,
                    b.min_fov,
                    b.max_fov,
                ])
                && b.focus_sensitivity > 0.
                && b.min_focus < b.max_focus
                && 0. < b.min_fov
                && b.min_fov < b.max_fov
                && b.startup.len() == 1
                && b.startup.iter().all(|v| {
                    v.source.file == "level1"
                        && class(&v.source, "tavcsoscript")
                        && records
                            .get(&v.transform)
                            .is_some_and(|r| r.class_name == "Transform")
                        && v.item_id == BINOCULARS_ITEM
                }),
            "binoculars",
        )?;

        checked(self.start_packs.len() == START_PACKS, "start packs")?;
        for v in &self.start_packs {
            checked(
                start(v, "cigiscript", PACK_ITEM) && v.ammo.r#type == 5 && !v.ammo.float_or_int,
                "start pack",
            )?;
        }
        ids(
            s.startup
                .iter()
                .chain(&self.start_packs)
                .map(|v| &v.transform)
                .chain(b.startup.iter().map(|v| &v.transform)),
            "duplicate small item start",
        )?;

        self.validate_washes(m, &class)?;

        let e = &self.exhale;
        checked(
            e.node.0 >= 1
                && m.nodes
                    .get(e.node.0 as usize - 1)
                    .is_some_and(|n| n.id == e.node)
                && finite(&[e.max_rate, e.charge_speed])
                && e.max_rate > 0.
                && e.charge_speed > 0.,
            "mouth smoke",
        )?;
        conditions::validate_particle(&e.controller, &e.particle, m)?;
        let h = &self.heat_haze;
        checked(
            h.simulation_space == 0
                && h.flags.get("prewarm") == Some(&true)
                && h.flags.get("looping") == Some(&true)
                && h.scalars.get("ShapeModule.type") == Some(&0.)
                && h.curves.contains_key("EmissionModule.rateOverTime")
                && h.materials.len() == 1
                && h.materials[0].shader.name == "FX/Glass/Stained BumpDistort",
            "old heat haze",
        )?;
        conditions::validate_particle(&e.controller, h, m)
    }
}

impl SmallItemsReference {
    fn validate_washes(
        &self,
        m: &ReferenceManifest,
        class: &dyn Fn(&SourceId, &str) -> bool,
    ) -> Result<(), ReferenceError> {
        for f in &self.wash_effects {
            checked(
                item_prototypes::clip_exists(m, &f.clip) && (0. ..=1.).contains(&f.volume),
                "wash destroy sound",
            )?;
            conditions::validate_particle(&f.source, &f.particle, m)?;
        }
        for w in &self.washes {
            let p = m
                .world
                .prototypes
                .iter()
                .find(|p| p.source == w.prototype)
                .ok_or_else(|| invalid("wash prototype"))?;
            let node = p
                .nodes
                .iter()
                .position(|n| n.source == w.node)
                .ok_or_else(|| invalid("wash node"))?;
            let below = |mut i: usize| loop {
                if i == node {
                    return true;
                }
                match p.nodes[i].parent {
                    Some(parent) => i = parent as usize,
                    None => return false,
                }
            };
            checked(
                class(&w.source, "washtriggerscript")
                    && usize::from(w.permission) < m.conditions.clean_tables.len()
                    && w.colliders.iter().all(|c| {
                        p.colliders
                            .iter()
                            .any(|pc| pc.source == *c && below(pc.node as usize))
                    })
                    && w.objects
                        .iter()
                        .all(|o| p.nodes.iter().any(|n| n.source == *o))
                    && (0. ..1.).contains(&w.chance_to_start_disabled)
                    && w.particle_on_destroy
                        .as_ref()
                        .is_none_or(|e| self.wash_effects.iter().any(|f| f.source == *e)),
                "wash trigger",
            )?;
        }
        // The checked cobwebs: CleanProperty 2, destroyOnRepair, one sphere trigger each.
        checked(
            self.washes.len() == COBWEBS
                && self.washes.iter().all(|w| {
                    w.permission == 2
                        && w.destroy_on_repair
                        && !w.never_repair
                        && w.objects.is_empty()
                        && w.colliders.len() == 1
                        && w.particle_on_destroy.is_some()
                }),
            "cobweb wash triggers",
        )?;
        ids(self.washes.iter().map(|w| &w.node), "duplicate wash node").map(|_| ())
    }
}
