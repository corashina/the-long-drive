//! Authored pose links. Startup IK chains remain separate from node parents.
use super::*;
use std::collections::BTreeMap;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PoseAnchorReference {
    pub source: SourceId,
    pub node: NodeId,
    pub transform: Transform,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PlayerFollowReference {
    pub source: SourceId,
    pub owner: NodeId,
    pub enabled: bool,
    pub inverse: bool,
    pub nodes: Vec<NodeId>,
    pub targets: Vec<NodeId>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct IkChainReference {
    pub source: SourceId,
    pub owner: NodeId,
    pub target: NodeId,
    pub pole: Option<NodeId>,
    pub end_bone: NodeId,
    pub bones: Vec<NodeId>,
    pub chain_length: u32,
    pub use_pole: bool,
    pub force_end: bool,
    pub stretch: bool,
    pub offsets: [f32; 4],
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PlayerPoseReference {
    pub player_source: SourceId,
    pub animation_source: SourceId,
    pub root: NodeId,
    pub anchors: BTreeMap<String, PoseAnchorReference>,
    pub targets: BTreeMap<String, NodeId>,
    pub walk_paths: [Vec<NodeId>; 2],
    pub sit_poses: [Vec<NodeId>; 4],
    pub wearable_anchors: Vec<NodeId>,
    pub character_meshes: Vec<NodeId>,
    pub runtime_targets: BTreeMap<String, Option<NodeId>>,
    pub walk_offsets: [f32; 2],
    pub method_inputs: BTreeMap<String, f32>,
    pub scalars: BTreeMap<String, f32>,
    pub animation_scalars: BTreeMap<String, f32>,
    pub right_hand_rotation: [f32; 4],
    pub use_override_sit_pose: bool,
    pub override_sit_pose_index: u32,
    pub ik_chains: Vec<IkChainReference>,
    pub follow: PlayerFollowReference,
    pub evidence: ManagedEvidence,
}
fn node(m: &ReferenceManifest, id: NodeId) -> Result<&NodeRecord, ReferenceError> {
    m.nodes
        .iter()
        .find(|n| n.id == id)
        .ok_or_else(|| ReferenceError("missing player node".into()))
}
fn unit(q: &[f32; 4]) -> bool {
    finite(q) && (q.iter().map(|v| v * v).sum::<f32>() - 1.).abs() < 0.002
}
pub(super) fn same_pose(a: &Transform, b: &Transform) -> bool {
    a.translation == b.translation && a.rotation == b.rotation && a.scale == b.scale && transform(a)
}
impl PlayerPoseReference {
    pub fn validate(&self, m: &ReferenceManifest) -> Result<(), ReferenceError> {
        let root = node(m, self.root)?;
        let asset = m
            .assets
            .iter()
            .find(|a| a.id == root.asset)
            .ok_or_else(|| ReferenceError("player asset".into()))?;
        require(
            self.player_source == scene(80125)
                && self.animation_source == scene(68762)
                && root.source == scene(39843)
                && asset.usage == AssetUsage::PlayerPrototype
                && asset.root == scene(39843)
                && asset.path == "reference/player.gltf"
                && m.nodes.iter().filter(|n| n.asset == root.asset).count() == 384,
            "complete player hierarchy",
        )?;
        let matches = |id: NodeId, pid: i64| {
            m.nodes
                .iter()
                .any(|n| n.id == id && n.source == scene(pid) && n.asset == root.asset)
                && descendant(m, id, self.root)
        };
        let links = |values: &[NodeId], expected: &[i64]| {
            values.len() == expected.len()
                && values.iter().zip(expected).all(|(n, p)| matches(*n, *p))
        };
        require(
            self.anchors.len() == ANCHOR_FIELDS.len() && self.targets.len() == TARGET_FIELDS.len(),
            "complete player pose fields",
        )?;
        for (field, pid) in ANCHOR_FIELDS {
            let a = self
                .anchors
                .get(*field)
                .ok_or_else(|| ReferenceError("missing player anchor".into()))?;
            require(
                a.source == scene(*pid)
                    && matches(a.node, *pid)
                    && same_pose(&a.transform, &node(m, a.node)?.transform),
                "player anchor pose",
            )?;
        }
        for (field, pid) in TARGET_FIELDS {
            require(
                self.targets.get(*field).is_some_and(|n| matches(*n, *pid)),
                "player target",
            )?;
        }
        for (values, expected) in self.walk_paths.iter().zip(WALK_IDS) {
            require(links(values, expected), "player walk path")?;
        }
        for (values, expected) in self.sit_poses.iter().zip(SIT_IDS) {
            require(links(values, expected), "player sit pose")?;
        }
        require(
            links(&self.wearable_anchors, &[26477, 28766, 35477, 36615, 39162]),
            "player wearable nodes",
        )?;
        require(
            self.runtime_targets.len() == NULL_FIELDS.len()
                && NULL_FIELDS
                    .iter()
                    .all(|f| self.runtime_targets.get(*f) == Some(&None)),
            "authored null player targets",
        )?;
        require(
            self.walk_offsets == [0.5, 0.] && self.scalars.len() == 7,
            "player walk inputs",
        )?;
        for (field, value) in [
            ("walkLerp", 5.),
            ("walkRotLerp", 1.),
            ("walkSpeedModifier", 0.1),
            ("FHeadLerp", 15.),
            ("FduckLerp", 5.),
            ("onehandLimit", 10.),
            ("pickedUpLerp", 6.),
        ] {
            require(
                self.scalars.get(field) == Some(&value),
                "player pose scalar",
            )?;
        }
        require(self.method_inputs.len() == 4, "player method inputs")?;
        for (field, value) in [
            ("VLerpSpeedHand", 20.),
            ("VLerpSpeedLeg", 20.),
            ("VMoveLockTime", 0.5),
            ("handSnapDelay", 0.25),
        ] {
            require(
                self.method_inputs.get(field) == Some(&value),
                "player method input",
            )?;
        }
        require(
            self.right_hand_rotation
                .iter()
                .zip(HAND_ROTATION)
                .all(|(a, b)| (a - b).abs() < 1e-6),
            "exact player hand rotation",
        )?;
        require(self.animation_scalars.len() == 7, "player head inputs")?;
        require(
            self.animation_scalars
                .get("sendInterval")
                .is_some_and(|v| v.is_finite() && *v > 0.),
            "player send interval",
        )?;
        for (field, value) in [
            ("headBackM", 0.1),
            ("headBackLerp", 10.),
            ("headForwardLerp", 4.),
            ("maxHeadBobX", 0.2),
            ("maxHeadBobY", 0.1),
            ("maxHeadBobZ", 0.3),
        ] {
            require(
                self.animation_scalars.get(field) == Some(&value),
                "player head scalar",
            )?;
        }
        require(
            !self.use_override_sit_pose
                && self.override_sit_pose_index == 0
                && self.character_meshes.len() == 12
                && self
                    .character_meshes
                    .iter()
                    .zip([
                        32922, 38614, 23205, 27836, 22144, 30679, 23000, 31080, 39486, 32302,
                        30051, 29368,
                    ])
                    .all(|(n, p)| matches(*n, p))
                && unit(&self.right_hand_rotation),
            "player view and sit inputs",
        )?;
        require(self.ik_chains.len() == 5, "complete player IK chains")?;
        for (i, c) in self.ik_chains.iter().enumerate() {
            require(
                c.offsets
                    .iter()
                    .zip(IK_ROTATIONS[i])
                    .all(|(a, b)| (a - b).abs() < 1e-6),
                "exact player IK offsets",
            )?;
            let expected = IK_BONES[i];
            let (owner, target, pole) = IK_LINKS[i];
            require(
                c.source == scene(IK_IDS[i])
                    && c.chain_length as usize + 1 == expected.len()
                    && links(&c.bones, expected)
                    && matches(c.owner, owner)
                    && matches(c.target, target)
                    && c.pole.is_some_and(|p| matches(p, pole))
                    && c.end_bone == *c.bones.last().unwrap()
                    && c.use_pole
                    && c.force_end == (i == 2)
                    && c.stretch == (i >= 2)
                    && unit(&c.offsets),
                "player IK links and flags",
            )?;
            for (j, bone) in c.bones.iter().enumerate() {
                let n = node(m, *bone)?;
                require(
                    n.parent == Some(if j == 0 { c.owner } else { c.bones[j - 1] }),
                    "authored IK parent",
                )?;
                if j > 0 {
                    require(
                        n.transform.translation.iter().map(|v| v * v).sum::<f32>() > 1e-16,
                        "zero-length IK segment",
                    )?;
                }
            }
        }
        let f = &self.follow;
        require(
            f.source == scene(74950)
                && matches(f.owner, 31145)
                && f.enabled
                && !f.inverse
                && links(&f.nodes, FOLLOW_NODES)
                && links(&f.targets, FOLLOW_TARGETS),
            "player pose-copy links",
        )?;
        require(self.evidence.valid(), "player pose source evidence")
    }
}

const ANCHOR_FIELDS: &[(&str, i64)] = &[
    ("CharacterRoot", 29438),
    ("CharacterStandPos", 36982),
    ("CharacterDuckPos", 26032),
    ("defLHPos", 34234),
    ("defRHPos", 30124),
    ("defLHPosStand", 32632),
    ("defRHPosStand", 28934),
    ("defLHPosDuck", 25084),
    ("defRHPosDuck", 28428),
    ("defLLPos", 36417),
    ("defRLPos", 36921),
    ("defdefLLPos", 37888),
    ("defdefRLPos", 25884),
    ("RH", 23118),
    ("TpickedUpParent", 39812),
    ("TpickedUpParent2", 23837),
    ("TpickedUpPos", 39814),
    ("usingHand", 32168),
    ("pointAt", 35402),
    ("CamParent", 22249),
    ("THeadDir", 37709),
    ("ghostHead", 28987),
    ("TStandHeadPos", 39823),
    ("TDuckHeadPos", 39849),
    ("TProneHeadPos", 23339),
    ("THeadBob", 39808),
    ("THeadLean", 39829),
    ("Th", 39811),
];

const TARGET_FIELDS: &[(&str, i64)] = &[
    ("HHTarget", 25362),
    ("LHTarget", 32234),
    ("RHTarget", 32762),
    ("LLTarget", 36286),
    ("RLTarget", 26378),
    ("Pelvis", 29799),
];

const WALK_IDS: &[&[i64]] = &[
    &[22742, 32651, 34295, 22427, 37543, 24385],
    &[28142, 26891, 36507, 36405, 33777, 27003],
];

const SIT_IDS: &[&[i64]] = &[
    &[34069, 34069, 34069],
    &[26796, 26796, 26796],
    &[24726, 25212, 25409],
    &[28329, 30177, 36962],
];

const IK_BONES: &[&[i64]] = &[
    &[26925, 26072, 30531],
    &[38757, 28290, 34137],
    &[36277, 36234, 27719, 37236, 29226],
    &[32657, 37442, 37971],
    &[34390, 29634, 31204],
];

const IK_LINKS: &[(i64, i64, i64)] = &[
    (29799, 36286, 23301),
    (29799, 26378, 38357),
    (29799, 25362, 37866),
    (38049, 32234, 28800),
    (25710, 32762, 38405),
];

const IK_IDS: &[i64] = &[69323, 79008, 76627, 70472, 76733];

const FOLLOW_NODES: &[i64] = &[
    25455, 30383, 37943, 37865, 28293, 39700, 32025, 36794, 39866, 24604, 32753, 27265, 24960,
    22453, 31901, 32533, 33876, 27041, 24970, 38256, 24859, 35588, 38783, 27677, 31650, 25383,
    38246, 24558, 35551, 24975, 35109, 30279, 38321, 37965, 34393, 22219, 38189, 22159, 33593,
    37927, 31634, 31273, 23075, 22202, 38190, 37896, 36151, 28886, 22054, 27730, 25376, 26108,
    36646, 36480, 33594, 27782, 28693, 25452, 37612, 38100, 36137, 39393, 27722, 36856, 37937,
    32014,
];

const FOLLOW_TARGETS: &[i64] = &[
    26068, 29799, 36277, 36234, 27719, 38049, 32657, 37442, 38141, 26362, 32161, 28124, 25617,
    24366, 33505, 31941, 35609, 28814, 25713, 40017, 26804, 33934, 39475, 28176, 33387, 25893,
    40029, 26243, 33814, 25710, 34390, 29634, 40075, 36258, 35131, 24021, 39954, 23985, 35295,
    36207, 33366, 29479, 23595, 23917, 39951, 36203, 37854, 27055, 23832, 28283, 26010, 25415,
    37352, 37236, 35291, 28365, 26925, 26072, 35908, 39681, 37746, 38757, 28290, 36314, 36283,
    32635,
];

const NULL_FIELDS: &[&str] = &[
    "nearestHand",
    "nearestLeg",
    "TpickedUp",
    "VCurrentLHT",
    "VCurrentRHT",
    "VCurrentLLT",
    "VCurrentRLT",
];

const HAND_ROTATION: &[f32] = &[-0.0, -0.0, -0.7071067811865476, 0.7071067811865476];

const IK_ROTATIONS: &[&[f32]] = &[
    &[-0.0, 0.7071067811865476, 0.0, 0.7071067811865476],
    &[-0.0, 0.7071067811865476, 0.0, 0.7071067811865476],
    &[
        -0.12278780396897285,
        -0.696364240320019,
        0.12278780396897285,
        0.696364240320019,
    ],
    &[-0.0, -0.0, 0.0, 1.0],
    &[-0.0, -0.0, 0.0, 1.0],
];
