//! Source `bulgingeyesScript` on the head of each player character (`fpscontroller` and `MPPlayer`).
use super::*;

/// The player and `MPPlayer` scripts have the same values.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BulgingEyesReference {
    /// The script on character 0 of the player.
    pub source: SourceId,
    /// `T` of each character: the left eye and the right eye renderer nodes.
    pub eyes: Vec<[NodeId; 2]>,
    pub start_scale: [f32; 3],
    pub end_scale: [f32; 3],
    /// `animx`, `animy`, `animz`.
    pub curves: [TorqueCurve; 3],
    /// `speedF`, `speedB`.
    pub speed_forward: f32,
    pub speed_back: f32,
    pub evidence: ManagedEvidence,
}

impl BulgingEyesReference {
    #[cfg(feature = "validate")]
    pub fn validate(&self, characters: &[CharacterReference]) -> Result<(), ReferenceError> {
        let renderer = |c: &CharacterReference, n: NodeId| c.renderers.iter().any(|r| r.node == n);
        require(
            self.eyes.len() == characters.len()
                && self
                    .eyes
                    .iter()
                    .zip(characters)
                    .all(|(e, c)| e[0] != e[1] && e.iter().all(|n| renderer(c, *n))),
            "bulging eyes are character renderers",
        )?;
        // `crate::simulation::engine::torque` reads unweighted keys only.
        let curve = |c: &TorqueCurve| {
            c.keys.first().is_some_and(|k| k.time <= 0.)
                && c.keys.last().is_some_and(|k| k.time >= 1.)
                && c.keys.windows(2).all(|k| k[0].time < k[1].time)
                && c.keys.iter().all(|k| {
                    k.weighted_mode == 0
                        && [k.time, k.value, k.in_tangent, k.out_tangent]
                            .iter()
                            .all(|v| v.is_finite())
                })
        };
        require(
            self.curves.iter().all(curve)
                && self
                    .start_scale
                    .iter()
                    .chain(&self.end_scale)
                    .all(|v| v.is_finite())
                && self.speed_forward > 0.
                && self.speed_back > 0.
                && self.evidence.valid(),
            "bulging eyes values",
        )
    }
}
