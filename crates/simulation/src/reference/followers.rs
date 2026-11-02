//! Source node followers of world prototypes: `followspringscript`, `followavgposrot` and
//! `followposrotscript`. Every `u32` is a prototype node index.
use super::*;

/// Followers of one prototype, in hierarchy order.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorldFollowersReference {
    /// `wheelgraphicsscript.T` of each vehicle or trailer wheel, in wheel order.
    pub wheels: Vec<u32>,
    pub followers: Vec<FollowerReference>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum FollowerReference {
    /// `followspringscript`: `node` moves on the line `from`-`to` by the distance `start`-`target`
    /// over the spawn distance `start`-`end`.
    Spring {
        node: u32,
        target: u32,
        from: u32,
        to: u32,
        start: u32,
        end: u32,
    },
    /// `followavgposrot`: the middle of `first` and `second`, and a look at `look`.
    Average {
        node: u32,
        position: bool,
        rotation: bool,
        offset: [f32; 3],
        axis: [f32; 3],
        first: Option<u32>,
        second: Option<u32>,
        look: Option<u32>,
    },
    /// `followposrotscript` without clamp.
    Pose {
        node: u32,
        copy_position: bool,
        lerp: bool,
        copy_rotation: bool,
        look: bool,
        lerp_amount: f32,
        look_rotation: [f32; 4],
        position_target: Option<u32>,
        from: Option<u32>,
        to: Option<u32>,
        rotation_target: Option<u32>,
        look_target: Option<u32>,
    },
}

impl FollowerReference {
    pub fn node(&self) -> u32 {
        match self {
            Self::Spring { node, .. } | Self::Average { node, .. } | Self::Pose { node, .. } => {
                *node
            }
        }
    }
    fn valid(&self, nodes: usize) -> bool {
        let n = |v: u32| (v as usize) < nodes && v != 0;
        let set = |flag: bool, v: Option<u32>| !flag || v.is_some_and(n);
        let any = |v: Option<u32>| v.is_none_or(n);
        match *self {
            Self::Spring {
                node,
                target,
                from,
                to,
                start,
                end,
            } => [node, target, from, to, start, end].into_iter().all(n),
            Self::Average {
                node,
                position,
                rotation,
                offset,
                axis,
                first,
                second,
                look,
            } => {
                n(node)
                    && finite(&offset)
                    && finite(&axis)
                    && set(position, first)
                    && set(position, second)
                    && set(rotation, look)
                    && [first, second, look].into_iter().all(any)
            }
            Self::Pose {
                node,
                copy_position,
                lerp,
                copy_rotation,
                look,
                lerp_amount,
                look_rotation,
                position_target,
                from,
                to,
                rotation_target,
                look_target,
            } => {
                n(node)
                    && lerp_amount.is_finite()
                    && finite(&look_rotation)
                    && set(copy_position, position_target)
                    && set(lerp, from)
                    && set(lerp, to)
                    && set(copy_rotation, rotation_target)
                    && set(look, look_target)
                    && [position_target, from, to, rotation_target, look_target]
                        .into_iter()
                        .all(any)
            }
        }
    }
}

impl WorldFollowersReference {
    pub(super) fn valid(&self, p: &WorldPrototypeReference) -> bool {
        let wheels = p
            .vehicle
            .as_ref()
            .map(|v| v.wheels.len())
            .or(p.trailer.as_ref().map(|t| t.wheels.len()));
        !self.followers.is_empty()
            && wheels == Some(self.wheels.len())
            && self
                .wheels
                .iter()
                .all(|&n| n != 0 && (n as usize) < p.nodes.len())
            && self.followers.iter().all(|f| f.valid(p.nodes.len()))
    }
}
