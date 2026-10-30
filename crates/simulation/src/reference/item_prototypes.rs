use super::*;

#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct PrototypeId(pub SourceId);

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ItemPrototype {
    pub hose: Option<HoseReference>,
    pub consumable: Option<CigaretteConsumptionReference>,
    /// Source `lightdither` on the pumpkin light.
    pub light_dither: Option<LightDitherReference>,
    pub id: PrototypeId,
    pub source: SourceId,
    pub item_id: Option<u32>,
    pub asset: AssetId,
    pub root: NodeId,
    pub bodies: Vec<BodyRecord>,
    pub colliders: Vec<ColliderRecord>,
    pub pickups: Vec<PrototypePickup>,
    pub variants: Vec<PrototypeVariant>,
    pub renderers: Vec<PrototypeRenderer>,
    pub behavior: PrototypeBehavior,
}

pub const HOSE_SEGMENTS: usize = 6;

/// Light id of a `lightdither` light in its own table. The cigarette ember light is 1.
pub const DITHER_LIGHT_ID: u32 = 2;

/// Source `lightdither`: Perlin dithers set the intensity of one point light.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LightDitherReference {
    pub source: SourceId,
    pub light: LightReference,
    pub dithers: Vec<DitherReference>,
    /// Source `max` and `min`: the largest or smallest value. Neither: the sum.
    pub max: bool,
    pub min: bool,
}

/// Source `lightdither.dither`. `value` is the serialized start value.
#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DitherReference {
    pub speed: f32,
    pub speed2: f32,
    pub min_threshold: f32,
    pub max_threshold: f32,
    pub min_value: f32,
    pub max_value: f32,
    pub value: f32,
}

/// Siphon hose (`tubescript` chain). Body ids are local prototype bodies.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HoseReference {
    /// Main `tubescript`.
    pub source: SourceId,
    /// Segment bodies in chain order.
    pub segments: [BodyId; HOSE_SEGMENTS],
    /// Segments with `tubescript.isMain`. Each one runs `Fill` once per frame.
    pub mains: Vec<BodyId>,
    pub t1: BodyId,
    pub t2: BodyId,
    /// `tubescript.breakForce` per segment.
    pub break_force: [f32; HOSE_SEGMENTS],
    /// `tubescript.connectAnchor` per segment, in segment body space.
    pub connect_anchor: [[f32; 3]; HOSE_SEGMENTS],
    /// Ball joints from `segments[i]` to `segments[i + 1]`.
    pub joints: [HoseJoint; HOSE_SEGMENTS - 1],
    /// Ends on `t1` and `t2`.
    pub ends: [HoseEnd; 2],
    /// Bones are segment bodies.
    pub skin: SkinnedMeshReference<NodeId, BodyId>,
}

/// Anchors are in their body space.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HoseJoint {
    pub body: BodyId,
    pub connected_body: BodyId,
    pub anchor: [f32; 3],
    pub connected_anchor: [f32; 3],
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HoseEnd {
    pub body: BodyId,
    /// Cap Transform node.
    pub node: NodeId,
    /// Cap pose in end body space.
    pub pose: Transform,
    pub opening: HoseOpening,
    pub visual: ParticleReference,
    pub splash: ParticleReference,
}

/// Hose end `tankcapscript`. Its fields match `OpeningRecord`.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HoseOpening {
    pub source: SourceId,
    /// Source `Tank` is null. The attached cap supplies the tank.
    pub tank: (),
    pub flow_liters_per_second: f32,
    pub tilt_degrees: f32,
    pub tilt_axis: [f32; 3],
    pub angle_disabled: bool,
    pub spill_disabled: bool,
    pub tube_connected: bool,
    pub cant_tube: bool,
    pub flow_radius: f32,
    pub can_toggle: bool,
    pub particles: FluidParticleReference,
    pub valve: f32,
    pub can_tune: bool,
    pub tune_rate: f32,
    pub tune_icon: Option<SourceId>,
    pub presentation: FluidPresentation,
    pub visuals: CapVisualReference,
}

impl HoseReference {
    fn validate(&self, p: &ItemPrototype, m: &ReferenceManifest) -> Result<(), ReferenceError> {
        let segments = ids(self.segments.iter().copied(), "duplicate hose segment")?;
        require(
            checked(&self.source, self.source.path_id)
                && self
                    .segments
                    .iter()
                    .all(|s| p.bodies.iter().any(|b| b.id == *s)),
            "hose segment bodies",
        )?;
        require(
            self.joints
                .iter()
                .zip(self.segments.windows(2))
                .all(|(j, pair)| {
                    j.body == pair[0]
                        && j.connected_body == pair[1]
                        && finite(&j.anchor)
                        && finite(&j.connected_anchor)
                }),
            "hose joint chain",
        )?;
        ids(self.mains.iter().copied(), "duplicate hose main")?;
        require(
            !self.mains.is_empty()
                && self
                    .mains
                    .iter()
                    .chain([&self.t1, &self.t2])
                    .all(|b| segments.contains(b))
                && self.t1 != self.t2,
            "hose main or end segment",
        )?;
        require(
            finite(&self.break_force) && self.break_force.iter().all(|f| *f > 0.),
            "hose break force",
        )?;
        require(
            self.connect_anchor.iter().all(|a| finite(a)),
            "hose connect anchor",
        )?;
        for (index, (e, body)) in self.ends.iter().zip([self.t1, self.t2]).enumerate() {
            let o = &e.opening;
            require(
                e.body == body && p.within(m, e.node) && transform(&e.pose),
                "hose end pose",
            )?;
            require(
                checked(&o.source, o.source.path_id)
                    && valid_tuning(o.valve, o.can_tune, o.tune_rate)
                    && valid_tune_icon(o.can_tune, &o.tune_icon, &m.ui)
                    && valid_flow(
                        o.flow_liters_per_second,
                        o.tilt_degrees,
                        &o.tilt_axis,
                        o.flow_radius,
                        &o.particles,
                    ),
                "hose end opening",
            )?;
            validate_cap_visuals(&o.visuals, |node| p.within(m, node))?;
            require(
                e.visual.role == format!("outlet_hose_{}", index + 1)
                    && e.splash.role == format!("splash_hose_{}", index + 1),
                "hose end particle roles",
            )?;
            super::particles::validate_particle(&e.visual, 2)?;
            super::particles::validate_particle(&e.splash, 2)?;
        }
        let skin = &self.skin;
        require(
            checked(&skin.source, skin.source.path_id)
                && p.within(m, skin.node)
                && skin.bones.iter().all(|b| segments.contains(b))
                && skin.valid_streams(),
            "hose skin",
        )
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PrototypePickup {
    pub node: NodeId,
    pub body: BodyId,
    pub pickup: PickupReference,
    pub collision_clip: Option<SourceId>,
    pub collision_gain: f32,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PrototypeVariant {
    pub source: SourceId,
    pub node: NodeId,
    pub nodes: Vec<NodeId>,
    pub authored_index: u32,
    pub random_start: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PrototypeRenderer {
    pub state: render_lighting::RendererRecord,
    pub source: SourceId,
    pub node: NodeId,
    pub mesh: SourceId,
    pub enabled: bool,
    pub materials: Vec<SourceId>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PrototypeBehavior {
    pub pack: Option<SourceId>,
    pub ammo: Option<SourceId>,
    /// The root ammoscript of a magazine.
    pub ammo_rules: Option<PrototypeAmmo>,
    pub cigarette_prefab: Option<SourceId>,
    pub spawn: Option<SourceId>,
    /// Box breakablescript. Its record is in `breakables`.
    pub breakable: Option<SourceId>,
    pub mount: Option<SourceId>,
    pub melee: Option<SourceId>,
    pub attack_target: Option<SourceId>,
    pub attack_trigger: Option<SourceId>,
    /// gibscript of a gib root. Gibs only.
    pub gib: Option<GibReference>,
    /// gibscripts of gib pieces below the root. Gibs only.
    pub gib_pieces: Vec<GibPieceReference>,
    /// Gib nodes with scaleDestroyScript. Gibs only.
    pub scale_destroy: Vec<NodeId>,
    pub clips: Vec<SourceId>,
    pub contact_sound_source: Option<SourceId>,
    pub contact_clips: Vec<Vec<SourceId>>,
}

/// A root ammoscript as Start leaves it.
#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PrototypeAmmo {
    pub kind: u32,
    pub count: f32,
    pub maximum: f32,
    pub float: bool,
    pub is_mag: bool,
}

/// gibscript of a gib piece. It keeps its own life after the main gib detaches it.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GibPieceReference {
    pub source: SourceId,
    pub node: NodeId,
    /// True: `life` is a distance from the player in metres. False: seconds.
    pub distance: bool,
    pub life: f32,
    /// The piece node and every node below it.
    pub nodes: Vec<NodeId>,
}

/// gibscript on a gib root.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GibReference {
    pub source: SourceId,
    pub gib_main: bool,
    /// gibMainDontRot.
    pub dont_rot: bool,
    /// gibMainAddSmallPos.
    pub add_small_pos: bool,
    /// True: `life` is a distance from the player in metres. False: seconds.
    pub distance: bool,
    pub life: f32,
    pub children: Vec<NodeId>,
}

/// Item prototypes before the gibs, in export order.
const SELECTED: [i64; 12] = [
    75443, 51722, 61587, 70812, 53838, 53837, 63605, 63259, 54495, 83430, 51557, 51878,
];
/// Gib prototypes: beer, box, cacti, vase, wc, then box, vase and wc reduced gibs, then the
/// tumbleweed gib and the Attachable07 gib.
const GIB_COUNT: usize = 10;
/// Box gib and reduced gib roots.
pub(super) const BOX_GIBS: [i64; 2] = [81642, 81645];

fn checked(source: &SourceId, path: i64) -> bool {
    source.file == "sharedassets1.assets" && source.path_id == path
}
fn optional_checked(source: &Option<SourceId>, path: i64) -> bool {
    source.as_ref().is_some_and(|s| checked(s, path))
}
pub(super) fn clip_exists(m: &ReferenceManifest, source: &SourceId) -> bool {
    m.audio["clips"].as_array().is_some_and(|clips| {
        clips.iter().any(|c| {
            c["source"]["file"].as_str() == Some(source.file.as_str())
                && c["source"]["path_id"].as_i64() == Some(source.path_id)
        })
    })
}

impl PickupReference {
    pub fn validate_links(
        &self,
        m: &ReferenceManifest,
        asset: AssetId,
    ) -> Result<(), ReferenceError> {
        require(
            self.disable_when_stored.is_some(),
            "effective pickup storage owner",
        )?;
        for source in [
            &self.right_hand_grip,
            &self.left_hand_grip,
            &self.disable_when_stored,
            &self.keep_when_stored,
        ]
        .into_iter()
        .flatten()
        {
            require(
                m.nodes
                    .iter()
                    .any(|n| &n.source == source && n.asset == asset),
                "pickup Transform owner",
            )?;
        }
        require(
            self.inventory_icon
                .as_ref()
                .is_none_or(|s| m.ui.sprites.iter().any(|sprite| &sprite.source == s)),
            "pickup sprite",
        )?;
        require(
            finite(&self.right_hand_position)
                && finite(&self.recenter_position)
                && self.recenter_distance.is_finite()
                && [&self.right_hand_rotation, &self.recenter_rotation]
                    .iter()
                    .all(|q| {
                        finite(*q) && (q.iter().map(|v| v * v).sum::<f32>() - 1.).abs() < 0.002
                    }),
            "pickup pose",
        )
    }
}

pub fn validate_prototypes(m: &ReferenceManifest) -> Result<(), ReferenceError> {
    let (items, gibs) = m
        .item_prototypes
        .split_at_checked(SELECTED.len())
        .ok_or_else(|| ReferenceError("selected item prototypes".into()))?;
    require(
        items.iter().map(|p| p.id.0.path_id).eq(SELECTED)
            && items.iter().all(|p| p.item_id.is_some())
            && gibs.len() == GIB_COUNT
            && gibs.iter().all(|p| p.item_id.is_none())
            && [&gibs[1], &gibs[5]]
                .iter()
                .map(|p| p.id.0.path_id)
                .eq(BOX_GIBS),
        "selected item prototypes",
    )?;
    // Each gib prototype is the Gib or reducedGib of a breakable.
    require(
        gibs.iter().all(|p| {
            m.breakables
                .iter()
                .any(|b| [&b.gib, &b.reduced_gib].contains(&&Some(p.id.clone())))
        }),
        "gib prototype without breakable",
    )?;
    ids(
        m.item_prototypes.iter().map(|p| &p.id),
        "duplicate prototype",
    )?;
    ids(
        m.item_prototypes.iter().map(|p| p.asset),
        "duplicate prototype asset",
    )?;
    for p in &m.item_prototypes {
        p.validate(m)?;
    }
    Ok(())
}

impl ItemPrototype {
    pub(super) fn within(&self, m: &ReferenceManifest, mut node: NodeId) -> bool {
        for _ in 0..=m.nodes.len() {
            let Some(n) = m
                .nodes
                .iter()
                .find(|n| n.id == node && n.asset == self.asset)
            else {
                return false;
            };
            if node == self.root {
                return true;
            }
            let Some(parent) = n.parent else {
                return false;
            };
            node = parent;
        }
        false
    }
    pub fn body_for(&self, m: &ReferenceManifest, mut node: Option<NodeId>) -> Option<BodyId> {
        for _ in 0..=m.nodes.len() {
            let current = node?;
            if !self.within(m, current) {
                return None;
            }
            if let Some(b) = self.bodies.iter().find(|b| b.node == current) {
                return Some(b.id);
            }
            node = m.nodes.iter().find(|n| n.id == current)?.parent;
        }
        None
    }
    fn validate_gib(&self, m: &ReferenceManifest, generic: bool) -> Result<(), ReferenceError> {
        let b = &self.behavior;
        ids(
            b.scale_destroy.iter().copied(),
            "duplicate scaleDestroyScript node",
        )?;
        require(
            b.scale_destroy.iter().all(|n| self.within(m, *n)),
            "gib scaleDestroyScript owner",
        )?;
        require(
            b.gib_pieces.iter().all(|g| {
                checked(&g.source, g.source.path_id)
                    && g.life.is_finite()
                    && g.life >= 0.
                    && g.nodes.contains(&g.node)
                    && g.nodes
                        .iter()
                        .all(|n| *n != self.root && self.within(m, *n))
            }),
            "gib pieces",
        )?;
        let Some(gib) = &b.gib else {
            return require(generic, "box gib without gibscript");
        };
        ids(gib.children.iter().copied(), "duplicate gib child")?;
        // gibscript.Start adds torque or force to child Rigidbodies.
        let pushed = gib.gib_main && (!gib.dont_rot || gib.add_small_pos);
        require(
            checked(&gib.source, gib.source.path_id)
                && gib.life.is_finite()
                && gib.life >= 0.
                && !(gib.gib_main && gib.add_small_pos && gib.children.is_empty())
                && gib.children.iter().all(|n| {
                    *n != self.root
                        && self.within(m, *n)
                        && (!pushed || self.bodies.iter().any(|b| b.node == *n))
                }),
            "gib children",
        )?;
        if generic {
            return Ok(());
        }
        require(
            checked(
                &gib.source,
                if self.id.0.path_id == BOX_GIBS[0] {
                    153704
                } else {
                    153707
                },
            ) && gib
                .children
                .iter()
                .copied()
                .eq(self.pickups.iter().map(|p| p.node)),
            "debris child owners",
        )
    }
    pub fn validate(&self, m: &ReferenceManifest) -> Result<(), ReferenceError> {
        let spec = match self.id.0.path_id {
            75443 => Some((Some(74), 1, Some(4624), true, false, Some(149224))),
            51722 => Some((Some(62), 1, None, false, false, Some(154651))),
            61587 => Some((Some(216), 1, Some(4553), true, true, Some(144067))),
            81642 => Some((None, 56, Some(4623), true, false, None)),
            81645 => Some((None, 14, Some(4623), true, false, None)),
            70812 => Some((Some(217), 1, Some(4557), true, false, Some(157660))),
            53838 => Some((Some(218), 1, Some(4527), true, false, Some(147660))),
            53837 => Some((Some(221), 1, Some(4529), true, false, Some(147659))),
            63605 => Some((Some(245), 1, None, false, false, Some(147259))),
            63259 => Some((Some(92), 1, Some(4590), true, false, Some(149587))),
            54495 => Some((Some(230), 1, None, false, false, Some(134670))),
            // Hose segment inventory flags and icons are not checked against the source build.
            83430 => Some((Some(61), 6, None, false, false, None)),
            51557 => Some((Some(312), 1, Some(4569), false, false, Some(146482))),
            51878 => Some((Some(215), 1, Some(4580), true, false, Some(132733))),
            _ => None,
        };
        // Gibs other than the box gibs have no checked counts, icons or pickup flags.
        let generic = spec.is_none();
        require(!generic || self.item_id.is_none(), "unknown item prototype")?;
        let (item_id, count, icon, inventory, hand, pickup_source) =
            spec.unwrap_or((None, 0, None, false, false, None));
        let hose = item_id == Some(61);
        require(
            checked(&self.id.0, self.id.0.path_id)
                && self.source == self.id.0
                && self.item_id == item_id
                && m.assets.iter().any(|a| {
                    a.id == self.asset
                        && a.usage == AssetUsage::ItemPrototype
                        && a.root == self.source
                })
                && m.nodes
                    .iter()
                    .any(|n| n.id == self.root && n.source == self.source && n.parent.is_none())
                && self.within(m, self.root),
            "prototype identity",
        )?;
        let geometry = match self.id.0.path_id {
            75443 => Some((1, 3)),
            51722 => Some((2, 3)),
            61587 => Some((2, 3)),
            81642 => Some((58, 56)),
            81645 => Some((15, 14)),
            70812 => Some((3, 25)),
            53838 => Some((2, 4)),
            53837 => Some((1, 2)),
            63605 => Some((12, 2)),
            63259 => Some((3, 2)),
            54495 => Some((1, 7)),
            83430 => Some((6, 0)),
            51557 => Some((1, 1)),
            51878 => Some((1, 1)),
            _ => None,
        };
        require(
            geometry.is_none_or(|g| g == (self.colliders.len(), self.renderers.len())),
            "complete selected geometry",
        )?;
        let bodies = ids(self.bodies.iter().map(|b| b.id), "duplicate local body")?;
        ids(self.bodies.iter().map(|b| b.node), "duplicate body node")?;
        ids(
            self.bodies.iter().map(|b| &b.source),
            "duplicate body source",
        )?;
        ids(
            self.colliders.iter().map(|c| c.id),
            "duplicate local collider",
        )?;
        ids(
            self.colliders.iter().map(|c| &c.source),
            "duplicate collider source",
        )?;
        require(
            (generic || self.bodies.len() == count && self.pickups.len() == count)
                && (!self.bodies.is_empty() && !self.colliders.is_empty()
                    // A particle-only gib has no physics.
                    || self.item_id.is_none()
                        && self.bodies.is_empty()
                        && self.colliders.is_empty()
                        && self.pickups.is_empty()),
            "complete prototype physics",
        )?;
        for b in &self.bodies {
            let parent = m
                .nodes
                .iter()
                .find(|n| n.id == b.node)
                .and_then(|n| n.parent);
            require(
                self.within(m, b.node)
                    && checked(&b.source, b.source.path_id)
                    && b.source.path_id > 0
                    && b.id.0 > 0
                    && b.mass_kg.is_finite()
                    && b.mass_kg > 0.
                    && b.linear_damping.is_finite()
                    && b.linear_damping >= 0.
                    && b.angular_damping.is_finite()
                    && b.angular_damping >= 0.
                    && transform(&b.pose)
                    && b.parent_body == self.body_for(m, parent),
                "prototype body",
            )?;
        }
        for c in &self.colliders {
            require(
                self.within(m, c.node)
                    && checked(&c.source, c.source.path_id)
                    && c.source.path_id > 0
                    && bodies.contains(&c.body)
                    && self.body_for(m, Some(c.node)) == Some(c.body)
                    && c.id.0 > 0
                    && c.membership.is_power_of_two()
                    && finite(&[c.friction, c.restitution])
                    && c.friction >= 0.
                    && c.restitution >= 0.
                    && super::particles::valid_geometry(&c.geometry),
                "prototype collider owner or shape",
            )?;
        }
        ids(self.pickups.iter().map(|p| p.node), "duplicate pickup node")?;
        ids(
            self.pickups.iter().map(|p| &p.pickup.source),
            "duplicate pickup source",
        )?;
        for p in &self.pickups {
            let ineligible = match (self.id.0.path_id, p.pickup.source.path_id) {
                (81642, 135313) => Some(77169),
                (81642, 140775) => Some(82214),
                (81645, 135314) => Some(77170),
                _ => None,
            };
            let expected_inventory = inventory && ineligible.is_none();
            let expected_icon = if ineligible.is_some() { None } else { icon };
            require(
                ineligible.is_none_or(|source| {
                    m.nodes
                        .iter()
                        .any(|n| n.id == p.node && checked(&n.source, source))
                }),
                "ineligible debris source owner",
            )?;
            require(
                self.within(m, p.node)
                    && checked(&p.pickup.source, p.pickup.source.path_id)
                    && self.body_for(m, Some(p.node)) == Some(p.body)
                    && bodies.contains(&p.body)
                    && (hose
                        || generic
                        || (p.pickup.can_inventory == expected_inventory
                            && p.pickup.inventory_icon.as_ref().map(|s| s.path_id)
                                == expected_icon))
                    && (generic || p.pickup.can_hold_right_hand == hand)
                    && pickup_source
                        .is_none_or(|s| checked(&p.pickup.source, s) && p.node == self.root)
                    && p.collision_gain.is_finite()
                    && p.collision_gain >= 0.
                    && if hose || generic {
                        p.collision_clip.as_ref().is_none_or(|s| clip_exists(m, s))
                    } else {
                        p.collision_clip.as_ref().is_some_and(|s| {
                            checked(
                                s,
                                if matches!(item_id, Some(74 | 218 | 221 | 92 | 230 | 215)) {
                                    4270
                                } else {
                                    4166
                                },
                            ) && clip_exists(m, s)
                        })
                    },
                "prototype pickup owner",
            )?;
            p.pickup.validate_links(m, self.asset)?;
            for source in [
                &p.pickup.right_hand_grip,
                &p.pickup.left_hand_grip,
                &p.pickup.disable_when_stored,
                &p.pickup.keep_when_stored,
            ]
            .into_iter()
            .flatten()
            {
                require(
                    m.nodes
                        .iter()
                        .any(|n| &n.source == source && self.within(m, n.id)),
                    "prototype pickup subtree",
                )?;
            }
        }
        ids(
            self.renderers.iter().map(|r| r.node),
            "duplicate renderer node",
        )?;
        ids(
            self.renderers.iter().map(|r| &r.source),
            "duplicate prototype renderer",
        )?;
        for r in &self.renderers {
            r.state.validate_template()?;
            require(
                r.state.source == r.source,
                "prototype renderer state source",
            )?;
            require(
                self.within(m, r.node)
                    && checked(&r.source, r.source.path_id)
                    && r.source.path_id > 0
                    && !r.materials.is_empty()
                    && r.materials.iter().all(|source| {
                        m.rendering
                            .materials
                            .iter()
                            .any(|mat| mat.asset == self.asset && &mat.source == source)
                    }),
                "prototype renderer material slots",
            )?;
        }
        if item_id == Some(74) {
            require(self.variants.len() == 1, "pack selector")?;
            let v = &self.variants[0];
            ids(v.nodes.iter().copied(), "duplicate pack variant node")?;
            require(
                checked(&v.source, 140392)
                    && v.node == self.root
                    && v.authored_index == 0
                    && v.random_start
                    && v.nodes.len() == 3
                    && v.nodes
                        .iter()
                        .zip([56803, 56801, 56802])
                        .all(|(id, source)| {
                            m.nodes.iter().any(|n| {
                                n.id == *id && checked(&n.source, source) && self.within(m, n.id)
                            })
                        }),
                "pack variant branches",
            )?;
        } else {
            require(self.variants.is_empty(), "unexpected item variants")?;
        }
        let b = &self.behavior;
        let expected_clips: &[i64] = match item_id {
            Some(216) => &[4241, 4452, 4144, 4208],
            _ => &[],
        };
        require(
            b.clips
                .iter()
                .map(|s| s.path_id)
                .eq(expected_clips.iter().copied())
                && b.clips
                    .iter()
                    .all(|s| checked(s, s.path_id) && clip_exists(m, s)),
            "prototype behavior clips",
        )?;
        if item_id == Some(216) {
            let groups: &[&[i64]] = &[
                &[4166],
                &[4151, 4471, 4106],
                &[4164, 4223, 4140],
                &[4164, 4223, 4140],
                &[4354, 4119, 4406, 4305],
                &[4370],
            ];
            require(
                b.contact_sound_source
                    .as_ref()
                    .is_some_and(|s| s.file == "level1" && s.path_id == 64919)
                    && b.contact_clips.len() == groups.len()
                    && b.contact_clips.iter().zip(groups).all(|(clips, expected)| {
                        clips.iter().map(|s| s.path_id).eq(expected.iter().copied())
                            && clips
                                .iter()
                                .all(|s| checked(s, s.path_id) && clip_exists(m, s))
                    }),
                "shovel contact clips",
            )?;
        } else {
            require(
                b.contact_sound_source.is_none() && b.contact_clips.is_empty(),
                "unexpected contact clips",
            )?;
        }
        require(
            b.pack.is_some() == (item_id == Some(74))
                && b.ammo.is_some() == matches!(item_id, Some(74 | 215))
                && b.ammo_rules.is_some() == (item_id == Some(215))
                && b.cigarette_prefab.is_some() == (item_id == Some(74))
                && b.spawn.is_some() == (item_id == Some(62))
                && b.breakable.is_some() == (item_id == Some(62))
                && b.mount.is_some() == (item_id == Some(62))
                && b.melee.is_some() == (item_id == Some(216))
                && b.attack_target.is_some() == (item_id == Some(216))
                && b.attack_trigger.is_some() == (item_id == Some(216))
                && (item_id.is_none()
                    || b.gib.is_none() && b.gib_pieces.is_empty() && b.scale_destroy.is_empty()),
            "prototype behavior kinds",
        )?;
        match item_id {
            Some(74) => require(
                optional_checked(&b.pack, 141307)
                    && optional_checked(&b.ammo, 141757)
                    && optional_checked(&b.cigarette_prefab, 11985)
                    && m.item_prototypes.iter().any(|p| {
                        checked(&p.source, 54495)
                            && p.item_id == Some(230)
                            && p.consumable.is_some()
                    }),
                "pack source links",
            )?,
            Some(62) => require(
                optional_checked(&b.spawn, 158204)
                    && optional_checked(&b.breakable, 135113)
                    && optional_checked(&b.mount, 146589),
                "Box source links",
            )?,
            Some(216) => require(
                optional_checked(&b.melee, 132938)
                    && optional_checked(&b.attack_target, 53759)
                    && optional_checked(&b.attack_trigger, 70911)
                    && [&b.attack_target, &b.attack_trigger].iter().all(|source| {
                        m.nodes
                            .iter()
                            .any(|n| Some(&n.source) == source.as_ref() && self.within(m, n.id))
                    }),
                "shovel source links",
            )?,
            Some(215) => require(
                optional_checked(&b.ammo, 132742)
                    && b.ammo_rules.is_some_and(|a| {
                        a.is_mag && a.maximum.is_finite() && (0. ..=a.maximum).contains(&a.count)
                    }),
                "magazine source links",
            )?,
            None => self.validate_gib(m, generic)?,
            _ => {}
        }
        require(self.hose.is_some() == hose, "hose owner")?;
        if let Some(h) = &self.hose {
            h.validate(self, m)?;
        }
        require(
            self.consumable.is_some() == (item_id == Some(230)),
            "cigarette consumable owner",
        )?;
        if let Some(c) = &self.consumable {
            c.validate(self, m)?;
        }
        require(
            self.light_dither.is_some() == (item_id == Some(92)),
            "light dither owner",
        )?;
        if let Some(d) = &self.light_dither {
            let l = &d.light;
            require(
                l.id.0 == DITHER_LIGHT_ID
                    && l.kind == LightKind::Point
                    && l.cookie.is_none()
                    && l.node.is_some_and(|n| self.within(m, n))
                    && l.range_meters > 0.
                    && !d.dithers.is_empty()
                    && d.dithers.iter().all(|x| {
                        [
                            x.speed,
                            x.speed2,
                            x.min_threshold,
                            x.max_threshold,
                            x.min_value,
                            x.max_value,
                            x.value,
                        ]
                        .iter()
                        .all(|v| v.is_finite())
                    }),
                "light dither",
            )?;
        }
        require(
            m.bodies.iter().all(|b| !self.within(m, b.node))
                && m.items.iter().all(|i| !self.within(m, i.node))
                && m.colliders.iter().all(|c| {
                    !m.nodes
                        .iter()
                        .any(|n| n.id == c.node && n.asset == self.asset)
                }),
            "prototype active publication",
        )
    }
}
