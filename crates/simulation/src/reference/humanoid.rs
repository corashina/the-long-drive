//! Source humanoid Animator of the human monster: the `munkas01Avatar` muscle space, the
//! `ThirdPersonAnimatorController` states that `newAiAnim` drives, and the `newAiScript` voice.
//! Unlike other records, humanoid poses use the source (left-handed) space under the Animator
//! Transform. Angles are in radians unless named in degrees.
use super::*;

/// Mecanim human bones: 25 body bones, then 15 left and 15 right finger bones.
pub const HUMAN_BONES: usize = 55;
/// Mecanim muscles.
pub const MUSCLES: usize = 95;
/// Humanoid clip attributes: `RootT` 7..10, `RootQ` 10..14, muscles 42..137.
pub const ROOT_T: u32 = 7;
pub const ROOT_Q: u32 = 10;
pub const FIRST_MUSCLE: u32 = 42;

/// Avatar `m_AxesArray` entry of one human bone.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HumanoidAxes {
    pub pre: [f32; 4],
    pub post: [f32; 4],
    pub sign: [f32; 3],
    pub min: [f32; 3],
    pub max: [f32; 3],
}

/// One mapped human bone.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HumanoidBone {
    pub node: NodeId,
    pub axes: HumanoidAxes,
}

/// One clip child of a 2D freeform cartesian blend tree.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HumanoidChild {
    /// Index into `HumanoidReference.clips`.
    pub clip: u32,
    /// (`Turn`, `Forward`).
    pub position: [f32; 2],
    /// Child `timeScale`.
    pub speed: f32,
}

/// A looping humanoid clip. `tracks` hold `RootT`, `RootQ` and the muscles, one value per key.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HumanoidClip {
    pub source: SourceId,
    pub name: String,
    pub stop_time: f32,
    /// `m_OrientationOffsetY` in degrees.
    pub orientation_offset: f32,
    /// `m_Level`.
    pub level: f32,
    /// `m_Mirror` and `m_CycleOffset`.
    pub mirror: bool,
    pub cycle_offset: f32,
    /// `m_LoopBlendOrientation`, `m_LoopBlendPositionXZ` and `m_LoopBlendPositionY`: bake the yaw,
    /// the ground position and the height into the pose.
    pub bake_orientation: bool,
    pub bake_position: bool,
    pub bake_height: bool,
    pub tracks: Vec<MotionTrack>,
}

/// `eye.L` and `eye.R` (`newAiScript.bones`) copy `LEyeT` and `REyeT` (`bonesTarget`) in `LateUpdate`.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HumanoidCopy {
    pub node: NodeId,
    /// Target pose under the Animator Transform. Its parents are not animated.
    pub target: Transform,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HumanoidReference {
    pub controller: SourceId,
    pub avatar: SourceId,
    /// The Animator Transform `munkas01`.
    pub root: NodeId,
    /// `HUMAN_BONES` entries in Mecanim order. Missing bones are null.
    pub bones: Vec<Option<HumanoidBone>>,
    /// `m_Human.m_Scale`: `RootT` is in units of it.
    pub scale: f32,
    /// `m_HumanBoneMass` of the 25 body bones.
    pub masses: Vec<f32>,
    /// Head-local direction that faces the avatar forward (+Z) in the T-pose.
    pub head_forward: [f32; 3],
    pub clips: Vec<HumanoidClip>,
    /// `Grounded` and `Crouching` blend trees on `Turn` and `Forward`.
    pub grounded: Vec<HumanoidChild>,
    pub crouching: Vec<HumanoidChild>,
    /// Transition durations in source state normalized time: to `Crouching`, to `Grounded`.
    pub fades: [f32; 2],
    /// `newAiAnim` `forwardLerp`, `turnLerp`, `lookLerp`.
    pub forward_lerp: f32,
    pub turn_lerp: f32,
    pub look_lerp: f32,
    /// `handWeightUp`, `handWeightDown`, `handWeightDownMinTime`.
    pub hand_weight_up: f32,
    pub hand_weight_down: f32,
    pub hand_weight_down_delay: f32,
    /// `newAiScript.head` and `headRotLimit` in degrees.
    pub head: NodeId,
    pub head_limit: f32,
    pub copies: Vec<HumanoidCopy>,
}

/// One voice clip and its length in seconds.
#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HumanVoiceClip {
    pub clip: u32,
    pub seconds: f32,
}

/// `newAiScript` voice: `SourceVoice`, the clip arrays, the voice times and `TVoice`.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HumanVoiceReference {
    /// Car voice template of `SourceVoice` and its position under the prefab root.
    pub voice: u32,
    pub position: [f32; 3],
    pub idle: Vec<HumanVoiceClip>,
    pub chase: Vec<HumanVoiceClip>,
    pub notice: Vec<HumanVoiceClip>,
    pub attack: Vec<HumanVoiceClip>,
    pub death: Vec<HumanVoiceClip>,
    pub idle_time: [f32; 2],
    pub chase_time: [f32; 2],
    pub seeing_time: [f32; 2],
    /// `TVoice`, and the `TVoiceClose` and `TVoiceOpen` poses in the parent space of `TVoice`.
    pub mouth: NodeId,
    pub closed: Transform,
    pub open: Transform,
}

impl HumanoidReference {
    pub fn validate(&self, nodes: &[NodeRecord]) -> Result<(), ReferenceError> {
        let fail = |text: &str| Err(ReferenceError(format!("Human monster Animator: {text}")));
        let known = |id: NodeId| nodes.iter().any(|n| n.id == id);
        if self.bones.len() != HUMAN_BONES || self.bones[0].is_none() {
            return fail("bone count or hips");
        }
        for bone in self.bones.iter().flatten() {
            let a = &bone.axes;
            if !known(bone.node)
                || !finite(&a.pre)
                || !finite(&a.post)
                || !finite(&a.sign)
                || !finite(&a.min)
                || !finite(&a.max)
            {
                return fail("bone axes");
            }
        }
        if self.masses.len() != 25 || !finite(&self.masses) || self.masses.iter().sum::<f32>() <= 0.
        {
            return fail("bone masses");
        }
        if !known(self.root) || !known(self.head) || !(self.scale > 0.) {
            return fail("root, head or scale");
        }
        for c in &self.clips {
            let valid = c.stop_time > 0.
                && c.tracks.iter().all(|t| {
                    !t.times.is_empty()
                        && t.times.len() == t.values.len()
                        && t.values.iter().all(|v| v.len() == 1 && v[0].is_finite())
                        && t.times.windows(2).all(|w| w[0] <= w[1])
                });
            if !valid {
                return fail("clip keys");
            }
        }
        let children = self.grounded.iter().chain(&self.crouching);
        if self.grounded.is_empty()
            || self.crouching.is_empty()
            || children
                .clone()
                .any(|c| c.clip as usize >= self.clips.len() || !(c.speed > 0.))
        {
            return fail("blend trees");
        }
        if self.copies.iter().any(|c| !known(c.node)) {
            return fail("bone copies");
        }
        Ok(())
    }
}
