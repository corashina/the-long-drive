//! Appearance editor preview stage (`settingsplayercustomscript.camStuff`).
//! All poses are local to `camStuff`. The presentation places `camStuff` away from the world.
use super::*;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PreviewReference {
    pub camera: PreviewCamera,
    pub lights: Vec<PreviewLight>,
    pub figure: PreviewFigure,
}

/// `settingsplayercustomscript.cam`.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PreviewCamera {
    pub pose: Transform,
    /// Vertical field of view in degrees.
    pub fov: f32,
    pub near: f32,
    pub far: f32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PreviewLightKind {
    Spot,
    Directional,
    Point,
}

/// An active `Light` under `camStuff`.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PreviewLight {
    pub kind: PreviewLightKind,
    /// Source color (gamma space).
    pub color: [f32; 4],
    pub intensity: f32,
    pub range: f32,
    /// Full cone angle in degrees. Only a spot light uses it.
    pub spot_angle: f32,
    pub pose: Transform,
}

/// `settingsplayercustomscript.outfit`: the preview figure.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PreviewFigure {
    pub root_pose: Transform,
    /// The figure character that gives the joint poses.
    pub character: u32,
    pub joints: Vec<PreviewJoint>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PreviewJoint {
    /// Node name. Both player characters use the same joint names.
    pub name: String,
    pub pose: Transform,
}

impl PreviewReference {
    /// `joint_names` are the skin joint names of each player character.
    pub fn validate(&self, joint_names: &[HashSet<&str>]) -> Result<(), ReferenceError> {
        let c = &self.camera;
        require(
            transform(&c.pose)
                && c.fov > 0.
                && c.fov < 180.
                && c.near > 0.
                && c.far > c.near
                && c.far.is_finite(),
            "preview camera",
        )?;
        for l in &self.lights {
            require(
                transform(&l.pose)
                    && finite(&l.color)
                    && l.intensity.is_finite()
                    && l.intensity >= 0.
                    && l.range.is_finite()
                    && l.range > 0.
                    && l.spot_angle.is_finite(),
                "preview light",
            )?;
        }
        let f = &self.figure;
        let names = joint_names
            .get(f.character as usize)
            .ok_or_else(|| ReferenceError("preview figure character".into()))?;
        require(transform(&f.root_pose), "preview figure root pose")?;
        ids(
            f.joints.iter().map(|j| j.name.as_str()),
            "duplicate preview joint",
        )?;
        require(
            f.joints.len() == names.len()
                && f.joints
                    .iter()
                    .all(|j| names.contains(j.name.as_str()) && transform(&j.pose)),
            "preview figure joints",
        )
    }
}
