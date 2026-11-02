//! Player ragdoll prefab (`fpscontroller.prefabRagdol`).
//! Body poses are in ragdoll root space. Collider geometry, joint anchors and joint axes are in body space.
//! Joint axes are axial: a positive source angle about the source axis stays positive here. Limits are in degrees.
use super::*;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RagdollReference {
    /// Prefab root Transform.
    pub source: SourceId,
    pub asset: AssetId,
    pub root: NodeId,
    /// Ragdoll `playermodeloutfitscript`. `SpawnRagdol` copies the player outfit into it.
    pub outfit_source: SourceId,
    pub invisible_material: SourceId,
    /// Same order and segments as the player characters.
    pub characters: Vec<CharacterReference>,
    /// `SpawnRagdol` makes every body dynamic.
    pub bodies: Vec<BodyRecord>,
    /// `SpawnRagdol` enables every collider and ignores collisions with the player colliders.
    pub colliders: Vec<ColliderRecord>,
    pub joints: Vec<RagdollJoint>,
    pub pose: RagdollPose,
    pub evidence: ManagedEvidence,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RagdollJointKind {
    Character,
    Configurable,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct JointLimit {
    /// Degrees.
    pub limit: f32,
    pub bounciness: f32,
    pub contact_distance: f32,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct JointSpring {
    pub spring: f32,
    pub damper: f32,
}

/// Unbreakable source joint. Twist is about `axis`, swing1 about `swing_axis`,
/// swing2 about `axis x swing_axis`.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RagdollJoint {
    pub source: SourceId,
    pub kind: RagdollJointKind,
    /// Body on the joint node.
    pub body: BodyId,
    pub connected_body: BodyId,
    /// In `body` space.
    pub anchor: [f32; 3],
    /// In `connected_body` space.
    pub connected_anchor: [f32; 3],
    pub collide_connected: bool,
    /// Unit, in `body` space.
    pub axis: [f32; 3],
    /// Unit, in `body` space.
    pub swing_axis: [f32; 3],
    pub low_twist: JointLimit,
    pub high_twist: JointLimit,
    pub swing1: JointLimit,
    pub swing2: JointLimit,
    pub twist_spring: JointSpring,
    pub swing_spring: JointSpring,
    pub projection: bool,
    pub projection_distance: f32,
    /// Degrees.
    pub projection_angle: f32,
}

/// `ragdolPosScript`. At spawn each `pairs[i].ragdoll` takes the world pose of `pairs[i].live`.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RagdollPose {
    /// Ragdoll `ragdolPosScript`.
    pub source: SourceId,
    /// Player `fpscontroller.ragdolPos`.
    pub live_source: SourceId,
    /// `ragdolPosScript.head`: the camera follows it while dead.
    pub head: NodeId,
    pub pairs: Vec<RagdollPosePair>,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RagdollPosePair {
    /// Ragdoll asset node.
    pub ragdoll: NodeId,
    /// Player asset node.
    pub live: NodeId,
}

fn unit(v: &[f32; 3]) -> bool {
    finite(v) && (v.iter().map(|x| x * x).sum::<f32>() - 1.).abs() < 1e-4
}

impl JointLimit {
    fn valid(&self) -> bool {
        finite(&[self.limit, self.bounciness, self.contact_distance])
            && self.bounciness >= 0.
            && self.contact_distance >= 0.
    }
}

impl JointSpring {
    fn valid(&self) -> bool {
        finite(&[self.spring, self.damper]) && self.spring >= 0. && self.damper >= 0.
    }
}

impl RagdollReference {
    /// Ragdoll node in its asset, under its root.
    pub fn within(&self, m: &ReferenceManifest, node: NodeId) -> bool {
        m.nodes
            .iter()
            .any(|n| n.id == node && n.asset == self.asset)
            && descendant(m, node, self.root)
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

    pub fn validate(
        &self,
        m: &ReferenceManifest,
        player: &[CharacterReference],
    ) -> Result<(), ReferenceError> {
        require(
            m.assets
                .iter()
                .filter(|a| a.usage == AssetUsage::RagdollPrototype)
                .count()
                == 1
                && m.assets.iter().any(|a| {
                    a.id == self.asset
                        && a.usage == AssetUsage::RagdollPrototype
                        && a.root == self.source
                })
                && m.nodes.iter().any(|n| {
                    n.id == self.root
                        && n.asset == self.asset
                        && n.source == self.source
                        && n.parent.is_none()
                })
                && self.evidence.valid(),
            "ragdoll identity",
        )?;
        let bodies = ids(self.bodies.iter().map(|b| b.id), "duplicate ragdoll body")?;
        ids(
            self.bodies.iter().map(|b| b.node),
            "duplicate ragdoll body node",
        )?;
        require(
            !self.bodies.is_empty()
                && !self.colliders.is_empty()
                && self
                    .bodies
                    .iter()
                    .enumerate()
                    .all(|(i, b)| b.id.0 as usize == i + 1),
            "complete ragdoll physics",
        )?;
        for b in &self.bodies {
            let parent = m
                .nodes
                .iter()
                .find(|n| n.id == b.node)
                .and_then(|n| n.parent);
            require(
                self.within(m, b.node)
                    && b.dynamic
                    && b.mass_kg.is_finite()
                    && b.mass_kg > 0.
                    && b.linear_damping.is_finite()
                    && b.linear_damping >= 0.
                    && b.angular_damping.is_finite()
                    && b.angular_damping >= 0.
                    && transform(&b.pose)
                    && b.parent_body == self.body_for(m, parent),
                "ragdoll body",
            )?;
        }
        ids(
            self.colliders.iter().map(|c| c.id),
            "duplicate ragdoll collider",
        )?;
        for c in &self.colliders {
            require(
                self.within(m, c.node)
                    && bodies.contains(&c.body)
                    && self.body_for(m, Some(c.node)) == Some(c.body)
                    && c.enabled
                    && c.membership.is_power_of_two()
                    && finite(&[c.friction, c.restitution])
                    && c.friction >= 0.
                    && c.restitution >= 0.
                    && super::particles::valid_geometry(&c.geometry),
                "ragdoll collider",
            )?;
        }
        ids(self.joints.iter().map(|j| j.body), "ragdoll joint owner")?;
        for j in &self.joints {
            let dot: f32 = j.axis.iter().zip(&j.swing_axis).map(|(a, b)| a * b).sum();
            require(
                bodies.contains(&j.body)
                    && bodies.contains(&j.connected_body)
                    && j.body != j.connected_body
                    && finite(&j.anchor)
                    && finite(&j.connected_anchor)
                    && unit(&j.axis)
                    && unit(&j.swing_axis)
                    && dot.abs() < 0.999
                    && [j.low_twist, j.high_twist, j.swing1, j.swing2]
                        .iter()
                        .all(JointLimit::valid)
                    && j.twist_spring.valid()
                    && j.swing_spring.valid()
                    && j.low_twist.limit <= j.high_twist.limit
                    && j.swing1.limit >= 0.
                    && j.swing2.limit >= 0.
                    && finite(&[j.projection_distance, j.projection_angle]),
                "ragdoll joint",
            )?;
        }
        let live_asset = player.first().map(|c| c.asset);
        ids(
            self.pose.pairs.iter().map(|p| p.ragdoll),
            "duplicate ragdoll pose node",
        )?;
        require(
            !self.pose.pairs.is_empty()
                && self.pose.pairs.iter().all(|p| {
                    self.within(m, p.ragdoll)
                        && m.nodes
                            .iter()
                            .any(|n| n.id == p.live && Some(n.asset) == live_asset)
                })
                && self.within(m, self.pose.head)
                && self.body_for(m, Some(self.pose.head)).is_some(),
            "ragdoll pose map",
        )?;
        require(
            self.characters.len() == player.len(),
            "ragdoll character count",
        )?;
        for (c, p) in self.characters.iter().zip(player) {
            require(
                c.asset == self.asset
                    && self.within(m, c.root)
                    && c.segments
                        .iter()
                        .map(|s| &s.name)
                        .eq(p.segments.iter().map(|s| &s.name)),
                "ragdoll character contract",
            )?;
            c.validate_structure(m, |n| self.within(m, n))?;
        }
        require(
            m.rendering
                .materials
                .iter()
                .any(|v| v.asset == self.asset && v.source == self.invisible_material),
            "ragdoll invisible material",
        )
    }
}
