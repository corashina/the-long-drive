pub mod pose;
pub mod preview;
pub mod ragdoll;
use super::postprocessing::{DeadProfile, GlassesProfile};
use super::*;
pub use pose::*;
pub use preview::*;
pub use ragdoll::*;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ClothingReference {
    pub characters: Vec<CharacterReference>,
    pub pose: PlayerPoseReference,
    pub selected_character: u32,
    pub outfit_source: SourceId,
    pub invisible_material: SourceId,
    pub wearable_type_count: u32,
    pub player_root: SourceId,
    pub anchor_nodes: Vec<SourceAnchorNode>,
    pub anchors: Vec<SourceId>,
    pub wearables: Vec<WearableReference>,
    pub glasses_profiles: Vec<GlassesProfile>,
    /// `fpscontroller.DeadV`.
    pub dead_profile: DeadProfile,
    pub ragdoll: RagdollReference,
    pub preview: PreviewReference,
    pub evidence: ManagedEvidence,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SourceCoordinateSpace {
    SourceParentLocal,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SourceAnchorNode {
    pub source: SourceId,
    pub parent: Option<SourceId>,
    pub coordinate_space: SourceCoordinateSpace,
    pub transform: Transform,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WearableReference {
    pub source: SourceId,
    pub prototype: PrototypeId,
    pub type_index: u32,
    pub colliders: Vec<ColliderId>,
    pub renderers: Vec<NodeId>,
    pub use_center_object: bool,
    pub center: Option<NodeId>,
    pub selectors: Vec<WearableSelector>,
    pub lenses: Vec<GlassesLens>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GlassesLens {
    pub source: SourceId,
    pub node: NodeId,
    pub wearable: SourceId,
    pub profile: Option<u32>,
    pub disable_lens_flare: bool,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WearableSelectorKind {
    Random,
    Material,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WearableSelector {
    pub source: SourceId,
    pub node: NodeId,
    pub kind: WearableSelectorKind,
    pub authored_index: u32,
    pub random_start: bool,
    pub save_index: Option<u32>,
    pub children: Vec<SourceId>,
    pub material_rules: Option<WearableMaterialRules>,
    pub variants: Vec<WearableVariant>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WearableMaterialRules {
    #[serde(rename = "randomMin")]
    pub random_min: u32,
    #[serde(rename = "randomMax")]
    pub random_max: u32,
    #[serde(rename = "isChild")]
    pub is_child: bool,
    #[serde(rename = "noInheritState")]
    pub no_inherit_state: bool,
    #[serde(rename = "sameStateAsParent")]
    pub same_state_as_parent: bool,
    #[serde(rename = "DontUseWorn")]
    pub dont_use_worn: bool,
    #[serde(rename = "disableColor")]
    pub disable_color: bool,
    #[serde(rename = "useMatListDontCreateNewMaterial")]
    pub use_mat_list_dont_create_new_material: bool,
    #[serde(rename = "enableOverrideMin")]
    pub enable_override_min: bool,
    #[serde(rename = "enableOverrideMax")]
    pub enable_override_max: bool,
    #[serde(rename = "overRideMin")]
    pub override_min: u32,
    #[serde(rename = "overRideMax")]
    pub override_max: u32,
    pub loaded: bool,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VariantMaterialSlot {
    pub node: NodeId,
    pub slot: u32,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VariantFluid {
    pub all: f32,
    pub f: f32,
    pub max_c: f32,
    pub fluids: Vec<VariantFluidAmount>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VariantFluidAmount {
    pub kind: u32,
    pub amount: f32,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WearableVariant {
    pub nodes: Vec<NodeId>,
    pub weight: Option<f32>,
    pub material: Option<SourceId>,
    pub renderers: Vec<VariantMaterialSlot>,
    pub moves: Vec<NodeId>,
    pub targets: Vec<NodeId>,
    pub part_slots: Vec<SourceId>,
    pub parts: Vec<SourceId>,
    pub tank: Option<SourceId>,
    pub fluid: VariantFluid,
    pub lens: Option<SourceId>,
    pub profile: Option<u32>,
    pub disable_lens_flare: Option<bool>,
}
fn sid(path_id: i64) -> SourceId {
    SourceId {
        file: "sharedassets1.assets".into(),
        path_id,
    }
}
fn scene(path_id: i64) -> SourceId {
    SourceId {
        file: "level1".into(),
        path_id,
    }
}
impl ClothingReference {
    pub fn validate(&self, m: &ReferenceManifest) -> Result<(), ReferenceError> {
        require(
            self.outfit_source == scene(70544)
                && self.invisible_material
                    == SourceId {
                        file: "sharedassets0.assets".into(),
                        path_id: 56,
                    },
            "outfit appearance source",
        )?;
        self.pose.validate(m)?;
        self.validate_characters(m)?;
        const ANCESTRY: [i64; 6] = [39843, 39851, 39811, 22249, 39820, 25362];
        const ANCHORS: [i64; 5] = [26477, 28766, 35477, 36615, 39162];
        require(
            self.wearable_type_count == 5
                && self.player_root == scene(39843)
                && self.anchors.iter().eq(ANCHORS
                    .iter()
                    .map(|p| scene(*p))
                    .collect::<Vec<_>>()
                    .iter())
                && self.evidence.valid(),
            "wearable anchor identities",
        )?;
        let sources: Vec<_> = ANCESTRY.into_iter().chain(ANCHORS).collect();
        require(
            self.anchor_nodes.len() == sources.len(),
            "complete wearable anchor ancestry",
        )?;
        for (i, (n, source)) in self.anchor_nodes.iter().zip(sources).enumerate() {
            let parent = if i == 0 {
                None
            } else {
                Some(scene(if i < 6 { ANCESTRY[i - 1] } else { 25362 }))
            };
            require(
                n.source == scene(source)
                    && n.parent == parent
                    && n.coordinate_space == SourceCoordinateSpace::SourceParentLocal
                    && transform(&n.transform),
                "wearable source anchor pose",
            )?;
        }
        require(
            self.glasses_profiles.len() == 6,
            "selected glasses profiles",
        )?;
        for (i, p) in self.glasses_profiles.iter().enumerate() {
            p.validate(i, m)?;
        }
        self.dead_profile.validate(m)?;
        self.ragdoll.validate(m, &self.characters)?;
        let joint_names: Vec<HashSet<&str>> = self
            .characters
            .iter()
            .map(|c| {
                m.nodes
                    .iter()
                    .filter(|n| c.skins.iter().any(|s| s.joints.contains(&n.id)))
                    .map(|n| n.name.as_str())
                    .collect()
            })
            .collect();
        self.preview.validate(&joint_names)?;
        require(self.wearables.len() == 6, "selected wearable families")?;
        for (w, (wearable, root, item, type_index)) in self.wearables.iter().zip([
            (146739, 70812, 217, 0),
            (147668, 53838, 218, 1),
            (147667, 53837, 221, 2),
            (131348, 63605, 245, 1),
            (148817, 63259, 92, 3),
            (142834, 54495, 230, 4),
        ]) {
            let p = m
                .item_prototypes
                .iter()
                .find(|p| p.id == w.prototype)
                .ok_or_else(|| ReferenceError("wearable prototype missing".into()))?;
            let node_source = |node: NodeId, source: i64| {
                m.nodes
                    .iter()
                    .any(|n| n.id == node && n.source == sid(source) && p.within(m, node))
            };
            require(
                w.source == sid(wearable)
                    && w.prototype.0 == sid(root)
                    && p.item_id == Some(item)
                    && w.type_index == type_index,
                "wearable family source",
            )?;
            ids(w.renderers.iter().copied(), "duplicate wearable renderer")?;
            require(
                w.renderers
                    .iter()
                    .all(|n| p.renderers.iter().any(|r| r.node == *n)),
                "wearable renderer nodes",
            )?;
            let (expected_renderers, expected_colliders): (&[i64], &[i64]) = match wearable {
                146739 => (
                    &[
                        81689, 70739, 63552, 66918, 82244, 73036, 66333, 56341, 60387, 66684,
                        87658, 79878, 83500, 74142, 77647, 80742, 86499, 53868, 58013, 55142,
                        57753, 81212, 83913, 83695, 87099,
                    ],
                    &[112724, 113317, 121089],
                ),
                147668 => (&[86632, 88813], &[117484, 117486]),
                147667 => (&[80368], &[117485]),
                131348 => (
                    &[75397],
                    &[
                        112577, 119446, 118285, 113889, 112407, 110965, 119440, 111925, 121241,
                        116901, 114763, 114256,
                    ],
                ),
                148817 => (&[84062], &[111200, 116405, 124754]),
                142834 => (&[], &[112435]),
                _ => unreachable!(),
            };
            require(
                w.renderers.len() == expected_renderers.len()
                    && w.renderers
                        .iter()
                        .zip(expected_renderers)
                        .all(|(n, id)| node_source(*n, *id)),
                "exact wearable source renderers",
            )?;
            require(
                w.colliders.len() == expected_colliders.len()
                    && w.colliders
                        .iter()
                        .zip(expected_colliders)
                        .all(|(id, source)| {
                            p.colliders
                                .iter()
                                .any(|c| c.id == *id && c.source == sid(*source))
                        }),
                "exact wearable source colliders",
            )?;
            let center = match wearable {
                131348 => Some(69634),
                142834 => Some(56826),
                _ => None,
            };
            require(
                w.use_center_object == center.is_some()
                    && match (w.center, center) {
                        (Some(n), Some(s)) => node_source(n, s),
                        (None, None) => true,
                        _ => false,
                    },
                "wearable center source",
            )?;
            if wearable != 146739 {
                require(
                    w.selectors.is_empty() && w.lenses.is_empty(),
                    "unexpected wearable selector",
                )?;
                continue;
            }
            require(
                w.selectors
                    .iter()
                    .map(|s| s.source.path_id)
                    .eq([135226, 132932, 157851, 154182, 149977, 150967])
                    && w.lenses.len() == 19,
                "complete glasses selection",
            )?;
            ids(
                w.selectors.iter().map(|s| &s.source),
                "duplicate wearable selector",
            )?;
            ids(w.lenses.iter().map(|s| &s.source), "duplicate glasses lens")?;
            for l in &w.lenses {
                require(
                    l.wearable == w.source
                        && p.within(m, l.node)
                        && p.renderers.iter().any(|r| r.node == l.node)
                        && l.profile.is_some_and(|v| v < 6),
                    "glasses startup lens",
                )?;
            }
            for s in &w.selectors {
                require(
                    s.source.file == "sharedassets1.assets"
                        && p.within(m, s.node)
                        && (s.authored_index as usize) < s.variants.len()
                        && s.authored_index == 0
                        && s.random_start,
                    "wearable selector owner",
                )?;
                let (source, weights): (i64, &[f32]) = match s.source.path_id {
                    135226 => (70812, &[10., 10., 10., 10., 10.]),
                    157851 => (57452, &[25., 10., 10., 10., 10., 10.]),
                    154182 => (49311, &[25., 10., 10., 10., 10., 10.]),
                    149977 => (63724, &[10., 10., 10., 10., 10.]),
                    132932 => (70812, &[]),
                    150967 => (85646, &[]),
                    _ => return Err(ReferenceError("unknown wearable selector".into())),
                };
                require(
                    node_source(s.node, source),
                    "wearable selector node identity",
                )?;
                if weights.is_empty() {
                    require(
                        s.kind == WearableSelectorKind::Material
                            && s.variants.len() == 3
                            && s.save_index.is_none(),
                        "wearable material selector",
                    )?;
                    let rules = s
                        .material_rules
                        .as_ref()
                        .ok_or_else(|| ReferenceError("missing material selection rules".into()))?;
                    require(
                        rules.random_min == 0
                            && rules.random_max == 2
                            && rules.is_child == (s.source.path_id == 150967)
                            && !rules.no_inherit_state
                            && !rules.same_state_as_parent
                            && rules.dont_use_worn
                            && rules.disable_color
                            && rules.use_mat_list_dont_create_new_material
                            && !rules.enable_override_min
                            && !rules.enable_override_max
                            && rules.override_min == 0
                            && rules.override_max == 0
                            && !rules.loaded,
                        "material selector source rules",
                    )?;
                    let expected = if s.source.path_id == 132932 {
                        vec![sid(150967)]
                    } else {
                        vec![]
                    };
                    require(s.children == expected, "material selector children")?;
                } else {
                    require(
                        s.kind == WearableSelectorKind::Random
                            && s.save_index == Some(0)
                            && s.material_rules.is_none()
                            && s.children.is_empty()
                            && s.variants
                                .iter()
                                .map(|v| v.weight)
                                .eq(weights.iter().map(|v| Some(*v))),
                        "wearable random selection",
                    )?;
                }
                for v in &s.variants {
                    ids(v.nodes.iter().copied(), "duplicate wearable variant node")?;
                    require(
                        v.nodes.iter().all(|n| p.within(m, *n))
                            && v.moves.is_empty()
                            && v.targets.is_empty()
                            && v.part_slots.is_empty()
                            && v.parts.is_empty()
                            && v.tank.is_none()
                            && v.fluid.fluids.is_empty()
                            && v.fluid.all == 0.
                            && v.fluid.f == 0.
                            && v.fluid.max_c == 0.,
                        "wearable complete selection payload",
                    )?;
                    for r in &v.renderers {
                        require(
                            p.renderers
                                .iter()
                                .any(|n| n.node == r.node && (r.slot as usize) < n.materials.len()),
                            "wearable variant renderer material slot",
                        )?;
                    }
                    require(
                        v.material.as_ref().is_none_or(|s| {
                            m.rendering
                                .materials
                                .iter()
                                .any(|r| r.asset == p.asset && &r.source == s)
                        }),
                        "wearable variant material dependency",
                    )?;
                    let candidates: Vec<_> = w
                        .lenses
                        .iter()
                        .filter(|l| v.nodes.iter().any(|n| descendant(m, l.node, *n)))
                        .collect();
                    let expected = if candidates.len() == 1 {
                        Some(&candidates[0].source)
                    } else {
                        None
                    };
                    require(
                        v.lens.as_ref() == expected,
                        "missing effective variant lens",
                    )?;
                    if let Some(lens) = &v.lens {
                        require(
                            w.lenses.iter().any(|l| {
                                &l.source == lens
                                    && v.nodes.iter().any(|n| descendant(m, l.node, *n))
                                    && l.profile == v.profile
                                    && Some(l.disable_lens_flare) == v.disable_lens_flare
                            }),
                            "effective variant lens",
                        )?;
                    } else {
                        require(
                            v.profile.is_none() && v.disable_lens_flare.is_none(),
                            "unexpected variant profile",
                        )?;
                    }
                }
            }
            validate_selected_glasses(w, m, p)?;
        }
        Ok(())
    }
}

fn validate_selected_glasses(
    w: &WearableReference,
    m: &ReferenceManifest,
    p: &ItemPrototype,
) -> Result<(), ReferenceError> {
    let matches = |node: NodeId, source: i64| {
        m.nodes
            .iter()
            .any(|n| n.id == node && n.source == sid(source) && p.within(m, node))
    };
    let branches: &[(i64, &[i64])] = &[
        (135226, &[57452, 49311, 69685, 63724, 74215]),
        (157851, &[63552, 66918, 82244, 73036, 66333, 56341]),
        (154182, &[66684, 87658, 79878, 83500, 74142, 77647]),
        (149977, &[58013, 55142, 57753, 81212, 83913]),
    ];
    for (source, branches) in branches {
        let selector = w
            .selectors
            .iter()
            .find(|s| s.source == sid(*source))
            .ok_or_else(|| ReferenceError("missing source selector".into()))?;
        require(
            selector.variants.len() == branches.len()
                && selector.variants.iter().zip(*branches).all(|(v, node)| {
                    v.nodes.len() == 1
                        && matches(v.nodes[0], *node)
                        && v.material.is_none()
                        && v.renderers.is_empty()
                }),
            "exact selected variant nodes",
        )?;
    }
    let lenses = &[
        (154009, 63552, 0, true),
        (135972, 66918, 4, true),
        (150488, 82244, 2, true),
        (143500, 73036, 1, true),
        (156901, 66333, 5, true),
        (151834, 56341, 3, true),
        (140782, 66684, 0, true),
        (141256, 87658, 4, true),
        (153056, 79878, 2, true),
        (147364, 83500, 1, true),
        (147011, 74142, 5, true),
        (136426, 77647, 3, true),
        (157020, 86499, 0, true),
        (155894, 58013, 4, false),
        (152881, 55142, 2, false),
        (140470, 57753, 1, false),
        (131005, 81212, 5, false),
        (144723, 83913, 3, false),
        (151873, 87099, 4, false),
    ];
    require(
        w.lenses.len() == lenses.len()
            && w.lenses
                .iter()
                .zip(lenses)
                .all(|(l, (source, node, profile, flare))| {
                    l.source == sid(*source)
                        && matches(l.node, *node)
                        && l.profile == Some(*profile)
                        && l.disable_lens_flare == *flare
                }),
        "exact selected startup lenses",
    )?;
    for (source, nodes) in [(132932, &[81689, 60387, 83695][..]), (150967, &[70739][..])] {
        let selector = w
            .selectors
            .iter()
            .find(|s| s.source == sid(source))
            .ok_or_else(|| ReferenceError("missing material selector".into()))?;
        require(selector.variants.len() == 3, "selected material count")?;
        for (v, (file, material)) in selector.variants.iter().zip([
            ("sharedassets1.assets", 335),
            ("sharedassets1.assets", 253),
            ("sharedassets0.assets", 89),
        ]) {
            require(
                v.material
                    == Some(SourceId {
                        file: file.into(),
                        path_id: material,
                    })
                    && v.nodes.is_empty()
                    && v.weight.is_none()
                    && v.renderers.len() == nodes.len()
                    && v.renderers
                        .iter()
                        .zip(nodes)
                        .all(|(r, node)| r.slot == 0 && matches(r.node, *node)),
                "exact material variant slots",
            )?;
        }
    }
    Ok(())
}

pub fn descendant(m: &ReferenceManifest, mut node: NodeId, root: NodeId) -> bool {
    for _ in 0..m.nodes.len() {
        if node == root {
            return true;
        }
        match m.nodes.iter().find(|n| n.id == node).and_then(|n| n.parent) {
            Some(parent) => node = parent,
            None => return false,
        }
    }
    false
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CharacterReference {
    pub gltf_accessor_count: u32,
    pub gltf_mesh_count: u32,
    pub source: SourceId,
    pub asset: AssetId,
    pub root: NodeId,
    pub renderers: Vec<CharacterRendererReference>,
    pub skins: Vec<SkinReference>,
    pub segments: Vec<CharacterSegmentReference>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CharacterRendererReference {
    pub source: SourceId,
    pub node: NodeId,
    pub mesh: SourceId,
    pub gltf_mesh: u32,
    pub materials: Vec<SourceId>,
    pub state: render_lighting::RendererRecord,
    pub enabled: bool,
    pub skin: Option<u32>,
    pub update_when_offscreen: Option<bool>,
    pub skinned_motion_vectors: Option<bool>,
    pub quality: Option<u32>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SkinAccessorReference {
    pub index: u32,
    pub count: u32,
    pub component_type: u32,
    pub shape: String,
    pub buffer_view: u32,
    pub byte_offset: u64,
    pub byte_stride: u32,
    pub view_byte_length: u64,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SkinAttributesReference {
    pub joints: SkinAccessorReference,
    pub weights: SkinAccessorReference,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SkinReference {
    pub node: NodeId,
    pub index: u32,
    pub joints: Vec<NodeId>,
    pub skeleton: NodeId,
    pub inverse_bind: SkinAccessorReference,
    pub attributes: Vec<SkinAttributesReference>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CharacterSegmentReference {
    pub name: String,
    pub language_name: u32,
    pub renderers: Vec<VariantMaterialSlot>,
    pub default_material: SourceId,
    pub default_enabled: bool,
    pub enabled: bool,
    pub disable_able: bool,
    pub default_color: [f32; 4],
    pub color: [f32; 4],
    pub default_hsv: [f32; 3],
}
impl CharacterSegmentReference {
    pub fn visible(&self) -> bool {
        !self.disable_able || self.enabled
    }
}
impl SkinAccessorReference {
    fn validate(&self, shape: &str, components: &[u32]) -> Result<(), ReferenceError> {
        let size = match self.component_type {
            5121 => 1,
            5123 => 2,
            5126 => 4,
            _ => 0,
        };
        let width = if shape == "MAT4" { 16 } else { 4 };
        require(
            self.shape == shape
                && components.contains(&self.component_type)
                && self.count > 0
                && self.byte_stride >= width * size
                && self.byte_offset.is_multiple_of(u64::from(size.max(1)))
                && u64::from(self.count - 1) * u64::from(self.byte_stride)
                    + u64::from(width * size)
                    <= self.view_byte_length,
            "skin accessor layout",
        )
    }
}
impl ClothingReference {
    fn validate_characters(&self, m: &ReferenceManifest) -> Result<(), ReferenceError> {
        require(
            self.characters.len() == 2 && self.selected_character == 0,
            "player character selection",
        )?;
        let node = |id| m.nodes.iter().find(|n| n.id == id);
        require(
            self.characters
                .iter()
                .map(|c| c.source.clone())
                .eq([scene(71215), scene(67763)]),
            "character source order",
        )?;
        for c in &self.characters {
            c.validate(m)?;
        }
        require(
            self.characters[0].asset == self.characters[1].asset,
            "one complete player asset",
        )?;
        require(
            m.rendering.materials.iter().any(|v| {
                v.asset == self.characters[0].asset && v.source == self.invisible_material
            }),
            "player invisible material",
        )?;
        for a in &self.anchor_nodes {
            let n = m
                .nodes
                .iter()
                .find(|n| n.source == a.source)
                .ok_or_else(|| ReferenceError("retained wearable anchor".into()))?;
            require(
                n.asset == self.characters[0].asset
                    && pose::same_pose(&n.transform, &a.transform)
                    && n.parent.and_then(node).map(|n| &n.source) == a.parent.as_ref(),
                "retained wearable anchor ancestry",
            )?;
        }
        Ok(())
    }
}

const CHARACTER_RENDERERS: &[&[(i64, i64, i64, &[i64])]] = &[
    &[
        (58557, 32922, 473, &[75, 74, 59, 55, 58, 61, 57, 60, 73]),
        (58502, 25876, 473, &[9, 9, 9, 9, 9, 9, 9, 9, 9]),
        (43612, 36602, 697, &[54, 58]),
        (44815, 25048, 645, &[58, 54]),
    ],
    &[
        (58487, 38614, 690, &[65]),
        (58545, 23205, 648, &[66]),
        (58525, 27836, 627, &[71]),
        (58478, 22144, 637, &[72]),
        (58512, 30679, 375, &[69]),
        (58561, 23000, 429, &[62]),
        (58491, 31080, 618, &[67]),
        (58529, 39486, 494, &[68]),
        (58531, 32302, 700, &[70]),
        (58516, 30051, 613, &[63]),
        (58540, 29368, 705, &[64]),
        (58511, 37355, 627, &[9]),
        (58500, 39907, 637, &[9]),
        (58523, 30182, 375, &[9]),
        (58489, 30613, 494, &[9]),
        (58535, 36960, 700, &[9]),
        (42694, 23849, 697, &[54, 58]),
        (41622, 40180, 645, &[58, 54]),
    ],
];
const CHARACTER_JOINTS: &[&[i64]] = &[
    &[
        26068, 29799, 36277, 36234, 27719, 38049, 32657, 37442, 38141, 26362, 32161, 28124, 24366,
        33505, 31941, 28814, 25713, 40017, 33934, 39475, 28176, 25893, 40029, 26243, 25710, 34390,
        29634, 40075, 36258, 35131, 24021, 23985, 35295, 36207, 29479, 23595, 23917, 36203, 37854,
        27055, 28283, 26010, 25415, 37236, 35291, 26925, 26072, 35908, 39681, 38757, 28290, 36314,
        36283,
    ],
    &[
        25455, 30383, 37943, 37865, 28293, 39700, 32025, 36794, 39866, 24604, 32753, 27265, 22453,
        31901, 32533, 27041, 24970, 38256, 35588, 38783, 27677, 25383, 38246, 24558, 24975, 35109,
        30279, 38321, 37965, 34393, 22219, 22159, 33593, 37927, 31273, 23075, 22202, 37896, 36151,
        28886, 27730, 25376, 26108, 36480, 33594, 28693, 25452, 37612, 38100, 39393, 27722, 36856,
        37937,
    ],
];

const CHARACTER_SEGMENTS: &[&[(&str, u32, i64, bool, [f32; 4], &[(i64, u32)])]] = &[
    &[
        (
            "Hair",
            0,
            59,
            true,
            [
                0.2745097875595093,
                0.1921568512916565,
                0.12549015879631042,
                1.0,
            ],
            &[(58557, 2)],
        ),
        (
            "Eyebrows",
            0,
            57,
            true,
            [
                0.501960813999176,
                0.44705885648727417,
                0.3490195870399475,
                1.0,
            ],
            &[(58557, 6)],
        ),
        (
            "Eyes",
            0,
            58,
            false,
            [
                0.5098038911819458,
                0.31372547149658203,
                0.07843134552240372,
                1.0,
            ],
            &[(58557, 4), (43612, 1), (44815, 0)],
        ),
        (
            "Mustache",
            0,
            60,
            true,
            [
                0.501960813999176,
                0.44705885648727417,
                0.3490195870399475,
                1.0,
            ],
            &[(58557, 7)],
        ),
        (
            "Skin",
            0,
            61,
            false,
            [
                0.9528301358222961,
                0.7646114826202393,
                0.5842826962471008,
                1.0,
            ],
            &[(58557, 5)],
        ),
        (
            "Shirt",
            0,
            75,
            false,
            [
                0.5550907254219055,
                0.6712933778762817,
                0.7264150977134705,
                1.0,
            ],
            &[(58557, 0)],
        ),
        (
            "Pants",
            0,
            74,
            false,
            [
                0.6698113083839417,
                0.5059859752655029,
                0.3159486949443817,
                1.0,
            ],
            &[(58557, 1)],
        ),
        (
            "Shoes",
            0,
            73,
            false,
            [
                0.5283018946647644,
                0.29349106550216675,
                0.0647917166352272,
                1.0,
            ],
            &[(58557, 8)],
        ),
    ],
    &[
        (
            "Hair",
            101,
            68,
            true,
            [
                0.5999999642372131,
                0.4469999670982361,
                0.14999999105930328,
                1.0,
            ],
            &[(58529, 0)],
        ),
        (
            "Eyebrows",
            102,
            65,
            true,
            [
                0.4399999976158142,
                0.31679999828338623,
                0.13199999928474426,
                1.0,
            ],
            &[(58487, 0)],
        ),
        (
            "Eyelashes",
            429,
            66,
            true,
            [
                0.5099999904632568,
                0.3129359781742096,
                0.08160001039505005,
                1.0,
            ],
            &[(58545, 0)],
        ),
        (
            "Eyes",
            104,
            67,
            false,
            [
                0.5099999904632568,
                0.3129359781742096,
                0.08160001039505005,
                1.0,
            ],
            &[(58491, 0), (42694, 1), (41622, 0)],
        ),
        (
            "Skin",
            108,
            69,
            false,
            [
                0.949999988079071,
                0.7622799873352051,
                0.5889999866485596,
                1.0,
            ],
            &[(58512, 0)],
        ),
        (
            "Shirt",
            105,
            72,
            false,
            [
                0.8700000047683716,
                0.4948560297489166,
                0.44370001554489136,
                1.0,
            ],
            &[(58478, 0)],
        ),
        (
            "Pants",
            106,
            71,
            false,
            [
                0.5224999785423279,
                0.7960999608039856,
                0.949999988079071,
                1.0,
            ],
            &[(58525, 0)],
        ),
        (
            "Shoes",
            107,
            70,
            false,
            [
                0.5999999642372131,
                0.5999999642372131,
                0.5999999642372131,
                1.0,
            ],
            &[(58531, 0)],
        ),
    ],
];

impl CharacterReference {
    /// Source-independent links: local renderers, material dependencies, skins and segment slots.
    pub fn validate_structure(
        &self,
        m: &ReferenceManifest,
        local: impl Fn(NodeId) -> bool,
    ) -> Result<(), ReferenceError> {
        let c = self;
        ids(
            c.renderers.iter().map(|r| r.node),
            "duplicate character renderer",
        )?;
        ids(c.skins.iter().map(|s| s.index), "duplicate character skin")?;
        require(!c.renderers.is_empty(), "character renderers")?;
        for r in &c.renderers {
            require(
                local(r.node) && r.gltf_mesh < c.gltf_mesh_count && !r.materials.is_empty(),
                "character renderer links",
            )?;
            r.state.validate()?;
            for material in &r.materials {
                require(
                    m.rendering
                        .materials
                        .iter()
                        .any(|v| v.asset == c.asset && &v.source == material),
                    "character material dependency",
                )?;
            }
        }
        for s in &c.skins {
            let r = c
                .renderers
                .iter()
                .find(|r| r.node == s.node)
                .ok_or_else(|| ReferenceError("skin renderer".into()))?;
            require(
                r.skin == Some(s.index)
                    && local(s.skeleton)
                    && s.joints.iter().all(|n| local(*n))
                    && s.inverse_bind.index < c.gltf_accessor_count
                    && s.inverse_bind.count as usize == s.joints.len()
                    && s.attributes.len() == r.materials.len(),
                "character skin links",
            )?;
            s.inverse_bind.validate("MAT4", &[5126])?;
            for a in &s.attributes {
                require(
                    a.joints.index < c.gltf_accessor_count
                        && a.weights.index < c.gltf_accessor_count
                        && a.joints.count == a.weights.count,
                    "skin attribute index",
                )?;
                a.joints.validate("VEC4", &[5121, 5123])?;
                a.weights.validate("VEC4", &[5126])?;
            }
        }
        for segment in &c.segments {
            require(
                finite(&segment.default_color)
                    && finite(&segment.color)
                    && segment.renderers.iter().all(|b| {
                        c.renderers
                            .iter()
                            .any(|r| r.node == b.node && (b.slot as usize) < r.materials.len())
                    }),
                "character segment slot",
            )?;
        }
        Ok(())
    }

    pub fn validate(&self, m: &ReferenceManifest) -> Result<(), ReferenceError> {
        let c = self;
        let i = match (c.source.file.as_str(), c.source.path_id) {
            ("level1", 71215) => 0,
            ("level1", 67763) => 1,
            _ => return Err(ReferenceError("character source".into())),
        };
        let node = |id| m.nodes.iter().find(|n| n.id == id);
        require(
            c.source == scene([71215, 67763][i])
                && node(c.root)
                    .is_some_and(|n| n.source == scene([29438, 31145][i]) && n.asset == c.asset)
                && m.assets
                    .iter()
                    .any(|a| a.id == c.asset && a.usage == AssetUsage::PlayerPrototype),
            "character identity",
        )?;
        let local = |id| node(id).is_some_and(|n| n.asset == c.asset) && descendant(m, id, c.root);
        require(
            c.renderers.len() == CHARACTER_RENDERERS[i].len()
                && c.skins.len() == [2, 16][i]
                && c.segments.len() == 8,
            "complete character contract",
        )?;
        ids(
            c.renderers.iter().map(|r| r.node),
            "duplicate character renderer",
        )?;
        ids(c.skins.iter().map(|s| s.index), "duplicate character skin")?;
        for (r, (source, transform_id, mesh, materials)) in
            c.renderers.iter().zip(CHARACTER_RENDERERS[i])
        {
            require(
                r.source == scene(*source)
                    && local(r.node)
                    && node(r.node).is_some_and(|n| n.source == scene(*transform_id))
                    && r.gltf_mesh < c.gltf_mesh_count
                    && r.mesh
                        == SourceId {
                            file: "sharedassets0.assets".into(),
                            path_id: *mesh,
                        }
                    && r.enabled
                    && r.materials.len() == materials.len()
                    && r.materials
                        .iter()
                        .zip(*materials)
                        .all(|(m, p)| m.file == "sharedassets0.assets" && m.path_id == *p),
                "character renderer source bindings",
            )?;
            r.state.validate()?;
            let shadow = matches!(source, 58502 | 58511 | 58500 | 58523 | 58489 | 58535);
            let skinned = !matches!(source, 43612 | 44815 | 42694 | 41622);
            require(
                r.state.source == r.source
                    && r.state.cast_shadows
                        == if shadow {
                            3
                        } else if skinned {
                            0
                        } else {
                            1
                        }
                    && r.state.receive_shadows == !shadow
                    && r.state.reflection_probe_usage == if skinned { 0 } else { 1 }
                    && r.update_when_offscreen == skinned.then_some(true)
                    && r.skinned_motion_vectors == skinned.then_some(true)
                    && r.quality == skinned.then_some(0)
                    && r.skin.is_some() == skinned,
                "character renderer flags",
            )?;
            for material in &r.materials {
                require(
                    m.rendering
                        .materials
                        .iter()
                        .any(|v| v.asset == c.asset && &v.source == material),
                    "character material dependency",
                )?;
            }
        }
        for s in &c.skins {
            let r = c
                .renderers
                .iter()
                .find(|r| r.node == s.node)
                .ok_or_else(|| ReferenceError("skin renderer".into()))?;
            require(
                s.index < 18
                    && r.skin == Some(s.index)
                    && local(s.skeleton)
                    && node(s.skeleton).is_some_and(|n| n.source == scene([26068, 25455][i]))
                    && s.joints.len() == 53
                    && s.joints.iter().zip(CHARACTER_JOINTS[i]).all(|(n, p)| {
                        local(*n)
                            && descendant(m, *n, s.skeleton)
                            && node(*n).is_some_and(|v| v.source == scene(*p))
                    }),
                "ordered character joints",
            )?;
            require(
                s.inverse_bind.index < c.gltf_accessor_count,
                "inverse bind accessor index",
            )?;
            s.inverse_bind.validate("MAT4", &[5126])?;
            require(
                s.inverse_bind.count == 53 && s.attributes.len() == r.materials.len(),
                "skin inverse binds and primitive count",
            )?;
            for a in &s.attributes {
                require(
                    a.joints.index < c.gltf_accessor_count
                        && a.weights.index < c.gltf_accessor_count,
                    "skin attribute index",
                )?;
                a.joints.validate("VEC4", &[5121, 5123])?;
                a.weights.validate("VEC4", &[5126])?;
                require(a.joints.count == a.weights.count, "skin joint weight count")?;
            }
        }
        for ((segment, (name, label, material, optional, color, pairs)), hsv) in c
            .segments
            .iter()
            .zip(CHARACTER_SEGMENTS[i])
            .zip(CHARACTER_HSV[i])
        {
            require(
                segment.name == *name
                    && segment.language_name == *label
                    && segment.default_material
                        == SourceId {
                            file: "sharedassets0.assets".into(),
                            path_id: *material,
                        }
                    && segment.default_enabled == *optional
                    && segment.disable_able == *optional
                    && !segment.enabled
                    && segment.default_color == *color
                    && segment.color == if i == 0 { [0.; 4] } else { [0., 0., 0., 1.] }
                    && segment
                        .default_hsv
                        .iter()
                        .zip(hsv)
                        .all(|(a, b)| (a - b).abs() < 1e-6)
                    && finite(&segment.default_hsv)
                    && segment.default_hsv.iter().all(|v| (0. ..=1.).contains(v))
                    && segment.renderers.len() == pairs.len(),
                "character appearance defaults",
            )?;
            for (binding, (renderer, slot)) in segment.renderers.iter().zip(*pairs) {
                require(
                    binding.slot == *slot
                        && c.renderers.iter().any(|r| {
                            r.node == binding.node
                                && r.source == scene(*renderer)
                                && (binding.slot as usize) < r.materials.len()
                        }),
                    "character appearance slot",
                )?;
            }
        }
        Ok(())
    }
}

const CHARACTER_HSV: &[&[[f32; 3]]] = &[
    &[
        [0.07456142192883357, 0.5428572514227523, 0.2745097875595093],
        [0.10683763015434052, 0.30468758256392414, 0.501960813999176],
        [0.09090909483450015, 0.84615388999765, 0.5098038911819458],
        [0.10683763015434052, 0.30468758256392414, 0.501960813999176],
        [0.08154933260016389, 0.3867923837832201, 0.9528301358222961],
        [0.5536231546236953, 0.23584913478648908, 0.7264150977134705],
        [0.08950614970498984, 0.5283019396810823, 0.6698113083839417],
        [0.08223456560658511, 0.8773585381965344, 0.5283018946647644],
    ],
    &[
        [0.10999999768204145, 0.75, 0.5999999642372131],
        [0.09999999999999999, 0.7, 0.4399999976158142],
        [0.08999999165201016, 0.8399999766256291, 0.5099999904632568],
        [0.08999999165201016, 0.8399999766256291, 0.5099999904632568],
        [0.08, 0.3800000062741732, 0.949999988079071],
        [0.02000000605880368, 0.4899999849275612, 0.8700000047683716],
        [0.5600000092950711, 0.450000015685433, 0.949999988079071],
        [0.0, 0.0, 0.5999999642372131],
    ],
];
