//! Start menu figure (`mainmenuscript.menuCharacter`, `menuIkScript`) in the level0 menu scene.
//! All poses are local to the figure root `MenuCharacter`.
use super::*;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MenuFigureReference {
    /// `menuIkScript.cam`.
    pub camera: PreviewCamera,
    /// Camera background color (gamma). The menu scene skybox and scenery are not ported.
    pub background: [f32; 3],
    /// Flat scene ambient color times intensity (gamma).
    pub ambient: [f32; 3],
    /// The scene sun.
    pub lights: Vec<PreviewLight>,
    /// `menuIkScript.outfit`.
    pub figure: PreviewFigure,
    /// `menuIkScript.Body`: the figure root that follows `B`.
    pub body: Transform,
    /// The `Player` capsule on `MenuCharacter`. Its axis is Y.
    pub collider: MenuFigureCollider,
    pub ik: MenuIkReference,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MenuFigureCollider {
    /// Source space (not converted).
    pub center: [f32; 3],
    pub radius: f32,
    pub height: f32,
}

/// A hand goal. It is a child of a figure joint.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MenuHandGoal {
    pub parent: String,
    pub pose: Transform,
}

/// Joint names of the humanoid bones (`HumanBodyBones`).
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MenuIkBones {
    pub hips: String,
    pub neck: String,
    pub head: String,
    pub left_upper_arm: String,
    pub left_lower_arm: String,
    pub left_hand: String,
    pub right_upper_arm: String,
    pub right_lower_arm: String,
    pub right_hand: String,
    pub left_upper_leg: String,
    pub left_lower_leg: String,
    pub left_foot: String,
    pub right_upper_leg: String,
    pub right_lower_leg: String,
    pub right_foot: String,
}

/// `menuIkScript` fields. Goals are the start poses of `B`, `H`, `LL`, `RL`, `LH` and `RH`.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MenuIkReference {
    pub mouse_forward: f32,
    pub mouse_forward_l: f32,
    pub mouse_forward_h: f32,
    pub max_lerp: f32,
    pub lerp_speed: f32,
    pub rot_lerp_speed: f32,
    pub body_stance: f32,
    pub b: Transform,
    pub h: Transform,
    pub ll: Transform,
    pub rl: Transform,
    pub lh: MenuHandGoal,
    pub rh: MenuHandGoal,
    /// `LLU` and `RHU`: joints that give the up vector of the kick and reach rotations.
    pub llu: String,
    pub rhu: String,
    pub bones: MenuIkBones,
}

impl MenuIkBones {
    pub fn names(&self) -> [&str; 15] {
        [
            &self.hips,
            &self.neck,
            &self.head,
            &self.left_upper_arm,
            &self.left_lower_arm,
            &self.left_hand,
            &self.right_upper_arm,
            &self.right_lower_arm,
            &self.right_hand,
            &self.left_upper_leg,
            &self.left_lower_leg,
            &self.left_foot,
            &self.right_upper_leg,
            &self.right_lower_leg,
            &self.right_foot,
        ]
    }
}

impl MenuFigureReference {
    /// `joint_names` are the skin joint names of each player character.
    pub fn validate(&self, joint_names: &[HashSet<&str>]) -> Result<(), ReferenceError> {
        PreviewReference {
            camera: self.camera.clone(),
            lights: self.lights.clone(),
            figure: self.figure.clone(),
        }
        .validate(joint_names)?;
        require(
            finite(&self.background) && finite(&self.ambient),
            "menu figure colors",
        )?;
        let c = &self.collider;
        require(
            finite(&c.center) && c.radius > 0. && c.height > 0.,
            "menu figure collider",
        )?;
        let ik = &self.ik;
        require(
            [
                ik.mouse_forward,
                ik.mouse_forward_l,
                ik.mouse_forward_h,
                ik.max_lerp,
                ik.lerp_speed,
                ik.rot_lerp_speed,
                ik.body_stance,
            ]
            .iter()
            .all(|v| v.is_finite() && *v > 0.),
            "menu IK scalars",
        )?;
        require(
            self.body == ik.b
                && [&ik.b, &ik.h, &ik.ll, &ik.rl, &ik.lh.pose, &ik.rh.pose]
                    .into_iter()
                    .all(transform),
            "menu IK goals",
        )?;
        let joints: HashSet<&str> = self.figure.joints.iter().map(|j| j.name.as_str()).collect();
        require(
            [
                ik.lh.parent.as_str(),
                ik.rh.parent.as_str(),
                ik.llu.as_str(),
                ik.rhu.as_str(),
            ]
            .into_iter()
            .chain(ik.bones.names())
            .all(|n| joints.contains(n)),
            "menu IK bones",
        )
    }
}
