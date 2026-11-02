//! Source controller and clip inputs of the melee weapons.
//! Every controller gets structure checks. The shovel controller also keeps its checked values.
use super::*;
use crate::reference::acquisition::{required_field, source_is};
macro_rules! record {
    ($name:ident {$($field:ident: $ty:ty),* $(,)?}) => {
        #[derive(Clone,Debug,Serialize,Deserialize,PartialEq)]
        #[serde(deny_unknown_fields)]
        pub struct $name {$(#[serde(deserialize_with="required_field")] pub $field:$ty),*}
    };
}
record!(MeleeMotion {
    controller:SourceId, handler:SourceId, animator:SourceId, target:NodeId, coordinate_space:String,
    clip_slots:Vec<SourceId>, parameters:Vec<MotionParameter>, default_state:u32, layer_binding:u32,
    layer_weight:f32, layer_blending_mode:u32, layer_ik_pass:bool, states:Vec<MotionState>,
    any_state_transitions:Vec<MotionTransition>, selectors:Vec<MotionSelector>, clips:Vec<MotionClip>
});
record!(MotionParameter {id:u32,name:String,kind:String,index:u32,default_float:Option<f32>,default_bool:Option<bool>});
record!(MotionState {
    id:u32,name_id:u32,name:String,path_id:u32,tag_id:u32,clip_slot:u32,speed:f32,speed_parameter:u32,
    cycle_offset:f32,cycle_offset_parameter:u32,time_parameter:u32,mirror_parameter:u32,mirror:bool,
    r#loop:bool,ik_on_feet:bool,write_default_values:bool,clip_duration:f32,clip_cycle_offset:f32,
    clip_mirror:bool,callback:Option<SourceId>,transitions:Vec<MotionTransition>
});
record!(MotionTransition {
    id:u32,full_path_id:u32,user_id:u32,destination:u32,duration:f32,offset:f32,exit_time:f32,
    has_exit_time:bool,fixed_duration:bool,can_transition_to_self:bool,interruption_source:u32,
    ordered_interruption:bool,conditions:Vec<MotionCondition>
});
record!(MotionCondition {
    mode: u32,
    parameter: u32,
    threshold: f32,
    exit_time: f32
});
record!(MotionSelector {id:u32,entry:bool,destinations:Vec<u32>});
record!(MotionReceipt {
    bytes: u64,
    file: String,
    sha256: String
});
record!(MotionClip {
    source:SourceId,source_tracks:MotionReceipt,name:String,start_time:f32,stop_time:f32,sample_rate:f32,
    r#loop:bool,cycle_offset:f32,tracks:Vec<MotionTrack>,events:Vec<MotionEvent>
});
// These clips have no events. State callbacks carry their attack events.
record!(MotionEvent {
    time: f32,
    function: String
});
// Transform attribute 1 position, 2 quaternion, 3 scale, 4 euler degrees. Values are Unity parent-local.
// A key row has one value per component: 4 for a quaternion, else 3.
// The node is a manifest node id for the shovel and a world prototype node index for the flyer.
record!(MotionTrack {
    node:u32,attribute:u32,path_hash:u32,custom_type:u32,curve_offset:u32,storage:String,interpolation:String,
    times:Vec<f32>,values:Vec<Vec<f32>>,in_tangents:Vec<Vec<f32>>,out_tangents:Vec<Vec<f32>>,coefficients:Vec<Vec<[f32;3]>>
});
impl MotionTrack {
    /// Components of one key row.
    pub fn width(&self) -> usize {
        if self.attribute == 2 { 4 } else { 3 }
    }
    /// Finite, ordered keys in [0, stop] with one row of `width` per key.
    pub fn keys_valid(&self, stop_time: f32) -> bool {
        let n = self.times.len();
        let w = self.width();
        n > 0
            && (1..=4).contains(&self.attribute)
            && self.times.windows(2).all(|p| p[1] > p[0])
            && self
                .times
                .iter()
                .all(|v| v.is_finite() && *v >= 0. && *v <= stop_time + 1e-6)
            && [&self.values, &self.in_tangents, &self.out_tangents]
                .iter()
                .all(|rows| rows.len() == n && rows.iter().all(|r| r.len() == w && finite(r)))
            && (self.coefficients.is_empty() || self.coefficients.len() == n)
            && self
                .coefficients
                .iter()
                .all(|k| k.len() == w && k.iter().all(|c| finite(c)))
    }
}
/// Parameter names in source order: five attack triggers, Speed, Hit.
const PARAMETER_NAMES: [&str; 7] = [
    "AttackLeft",
    "AttackRight",
    "AttackForward",
    "AttackBack",
    "AttackStab",
    "Speed",
    "Hit",
];
/// (attribute, custom_type): position and rotation tracks.
const TRACK_TYPES: [(u32, u32); 2] = [(1, 0), (4, 4)];
impl MeleeMotion {
    pub fn is_shovel(&self) -> bool {
        source_is(&self.controller, "sharedassets1.assets", 4515)
    }
    pub fn validate(&self, m: &ReferenceManifest) -> Result<(), ReferenceError> {
        self.validate_structure(m)?;
        if self.is_shovel() {
            self.validate_shovel()?;
        }
        Ok(())
    }
    fn validate_structure(&self, m: &ReferenceManifest) -> Result<(), ReferenceError> {
        require(
            source_is(&self.handler, "level1", 74298)
                && source_is(&self.animator, "level1", 56797)
                && self.coordinate_space == "unity_parent_local"
                && m.nodes
                    .iter()
                    .any(|n| n.id == self.target && source_is(&n.source, "level1", 23118)),
            "melee motion owners",
        )?;
        require(
            self.parameters
                .iter()
                .map(|p| p.name.as_str())
                .eq(PARAMETER_NAMES),
            "melee parameter names",
        )?;
        for (i, p) in self.parameters.iter().enumerate() {
            require(
                self.parameters[..i].iter().all(|q| q.id != p.id),
                "melee parameter identity",
            )?;
            require(
                if p.name == "Speed" {
                    p.kind == "float" && p.default_float.is_some() && p.default_bool.is_none()
                } else {
                    p.kind == "trigger" && p.default_float.is_none() && p.default_bool.is_some()
                },
                "melee parameter type",
            )?;
        }
        let speed = self.parameters[5].id;
        let states = self.states.len() as u32;
        require(self.default_state < states, "melee default state")?;
        let mut triggered = [false; 5];
        let mut transition = |t: &MotionTransition| -> Result<(), ReferenceError> {
            require(
                t.destination < states
                    && t.duration.is_finite()
                    && t.duration >= 0.
                    && t.offset.is_finite()
                    && t.exit_time.is_finite()
                    && t.interruption_source == 0
                    && t.user_id == 0,
                "melee transition",
            )?;
            for c in &t.conditions {
                let p = self
                    .parameters
                    .iter()
                    .position(|p| p.id == c.parameter && p.kind == "trigger");
                require(
                    c.mode == 1 && c.threshold == 0. && c.exit_time == 0. && p.is_some(),
                    "melee transition condition",
                )?;
                if let Some(slot) = p.and_then(|i| triggered.get_mut(i)) {
                    *slot = true;
                }
            }
            Ok(())
        };
        for (i, s) in self.states.iter().enumerate() {
            require(
                (s.clip_slot as usize) < self.clip_slots.len()
                    && s.path_id == s.id
                    && (s.speed_parameter == 0 || s.speed_parameter == speed),
                "melee state clip or speed binding",
            )?;
            require(
                !s.mirror
                    && !s.ik_on_feet
                    && !s.clip_mirror
                    && s.write_default_values
                    && s.tag_id == 0
                    && s.cycle_offset == 0.
                    && s.cycle_offset_parameter == 0
                    && s.time_parameter == 0
                    && s.mirror_parameter == 0
                    && s.clip_cycle_offset == 0.
                    && s.speed.is_finite()
                    && s.clip_duration.is_finite()
                    && s.clip_duration >= 0.,
                "melee state operation",
            )?;
            // Attack states call AttackStart on enter and AttackEnd on exit. The default state has no callback.
            require(
                s.callback.is_none() == (i as u32 == self.default_state),
                "melee attack state callback",
            )?;
            for t in &s.transitions {
                transition(t)?;
            }
        }
        for t in &self.any_state_transitions {
            transition(t)?;
        }
        require(
            triggered.iter().all(|t| *t),
            "melee attack triggers lack transitions",
        )?;
        require(
            self.clips.len() == self.clip_slots.len(),
            "melee complete clips",
        )?;
        for (c, slot) in self.clips.iter().zip(&self.clip_slots) {
            require(
                c.source == *slot
                    && c.start_time >= 0.
                    && c.stop_time.is_finite()
                    && c.stop_time >= c.start_time
                    && c.sample_rate > 0.
                    && c.cycle_offset == 0.
                    && c.events.is_empty()
                    && !c.tracks.is_empty(),
                "melee clip timing or events",
            )?;
            for (j, t) in c.tracks.iter().enumerate() {
                require(
                    c.tracks[..j].iter().all(|u| u.attribute != t.attribute)
                        && t.node == self.target.0
                        && TRACK_TYPES.contains(&(t.attribute, t.custom_type))
                        && t.path_hash == 0,
                    "melee track binding",
                )?;
                let constant = t.storage == "constant";
                require(
                    (constant && t.interpolation == "CONSTANT")
                        || (t.storage == "streamed" && t.interpolation == "CUBIC"),
                    "melee track interpolation",
                )?;
                require(t.keys_valid(c.stop_time), "melee track keys")?;
                require(
                    t.coefficients.len() == if constant { 0 } else { t.times.len() },
                    "melee track polynomials",
                )?;
            }
        }
        Ok(())
    }
    /// Checked source values of the shovel controller.
    fn validate_shovel(&self) -> Result<(), ReferenceError> {
        require(
            self.clip_slots.len() == 6
                && self
                    .clip_slots
                    .iter()
                    .zip(CLIPS)
                    .all(|(s, id)| source_is(s, "sharedassets1.assets", id)),
            "shovel clip slots",
        )?;
        for (p, (name, id, index)) in self.parameters.iter().zip(PARAMETERS) {
            require(
                p.name == name
                    && p.id == id
                    && p.index == index
                    && p.default_float.unwrap_or(0.) == 0.
                    && p.default_bool != Some(true),
                "shovel parameter identity",
            )?;
        }
        require(
            self.default_state == 0
                && self.layer_binding == 756556552
                && self.layer_blending_mode == 0
                && self.layer_weight == 0.
                && !self.layer_ik_pass,
            "shovel layer",
        )?;
        require(self.states.len() == 6, "shovel states")?;
        for (i, s) in self.states.iter().enumerate() {
            require(
                s.id == STATES[i]
                    && s.clip_slot == [1, 0, 2, 3, 4, 5][i]
                    && s.speed == 1.
                    && s.r#loop == (i == 0)
                    && s.speed_parameter == if i == 0 { 0 } else { 3471299058 },
                "shovel state binding",
            )?;
            require(
                (s.name.as_str(), s.name_id, s.clip_duration) == STATE_BINDINGS[i],
                "shovel state source fields",
            )?;
            require(
                if i == 0 {
                    s.callback.is_none()
                } else {
                    s.callback.as_ref().is_some_and(|c| {
                        source_is(
                            c,
                            "sharedassets1.assets",
                            [130091, 130154, 130160, 130158, 130156][i - 1],
                        )
                    })
                },
                "shovel state callback",
            )?;
            require(
                s.transitions.len() == if i == 0 { 5 } else { 0 },
                "shovel state transitions",
            )?;
        }
        require(
            self.any_state_transitions == checked_any_transitions()
                && self.states[0].transitions == checked_idle_transitions(),
            "shovel checked transitions",
        )?;
        require(
            self.selectors.len() == 2
                && self.selectors[0].id == 756556552
                && self.selectors[0].entry
                && self.selectors[0].destinations == [0]
                && self.selectors[1].id == 756556552
                && !self.selectors[1].entry
                && self.selectors[1].destinations == [30000],
            "shovel selectors",
        )?;
        for (i, c) in self.clips.iter().enumerate() {
            let r = &c.source_tracks;
            require(
                !c.name.is_empty()
                    && r.bytes == RECEIPTS[i].0
                    && r.file == RECEIPTS[i].1
                    && r.sha256 == RECEIPTS[i].2,
                "shovel track receipt",
            )?;
            require(
                c.sample_rate == 60.
                    && c.start_time == 0.
                    && c.stop_time == CLIP_TIMES[i]
                    && c.r#loop == (i == 1)
                    && c.tracks.len() == 2,
                "shovel source clip length",
            )?;
            for (j, t) in c.tracks.iter().enumerate() {
                require(
                    t.attribute == [1, 4][j]
                        && t.curve_offset == j as u32 * 3
                        && (t.storage == "constant") == (i == 1),
                    "shovel track binding",
                )?;
                require(
                    t.times.len() == [5, 1, 5, 4, 3, 3][i]
                        && t.times.first()
                            == Some(&[0.16666667, 0., 0.16666667, 0.16666667, 0., 0.16666667][i])
                        && t.times.last() == Some(&c.stop_time),
                    "shovel source key extent",
                )?;
            }
        }
        Ok(())
    }
}
const CLIPS: [i64; 6] = [3998, 4001, 3999, 3997, 4002, 4003];
const STATES: [u32; 6] = [
    2008834259, 2530234843, 3863879828, 1326377276, 1368685456, 3895914711,
];
const PARAMETERS: [(&str, u32, u32); 7] = [
    ("AttackLeft", 2717246184, 0),
    ("AttackRight", 1504273457, 1),
    ("AttackForward", 1708274416, 2),
    ("AttackBack", 3059532983, 3),
    ("AttackStab", 268324848, 4),
    ("Speed", 3471299058, 0),
    ("Hit", 1654612129, 5),
];
const RECEIPTS: [(u64, &str, &str); 6] = [
    (
        9870,
        "animations/clips/sharedassets1.assets/3998/tracks.json",
        "8851780dbb8517494a90bac266283eedb8acae717148c979e0f6e7fb34c1d239",
    ),
    (
        6835,
        "animations/clips/sharedassets1.assets/4001/tracks.json",
        "7035332bc89364a4a1f52db9a6c4455a813f4922ac6f600773930a9703363239",
    ),
    (
        9832,
        "animations/clips/sharedassets1.assets/3999/tracks.json",
        "a70bbc9445f598dcf3a9cb55f482a00e17cfa232cd7dd5bb471c4016090efc89",
    ),
    (
        9084,
        "animations/clips/sharedassets1.assets/3997/tracks.json",
        "3eb3053001e6920fd0912df821ed2504996cddfc3891bbf2626ab8d544beeaa6",
    ),
    (
        8314,
        "animations/clips/sharedassets1.assets/4002/tracks.json",
        "3adf64bf7bfa84c12d5a54a42b9735424bb0744e28ed9caa21ccc27f360f3704",
    ),
    (
        8344,
        "animations/clips/sharedassets1.assets/4003/tracks.json",
        "7f94cbd4c86427963acbb8e5e08d7d5d3b26444e4fca2369fdea340c5449eb31",
    ),
];

const CLIP_TIMES: [f32; 6] = [
    0.6666666865348816,
    0.0,
    0.6666666865348816,
    0.6666666865348816,
    0.5833333134651184,
    0.6666666865348816,
];
const STATE_BINDINGS: [(&str, u32, f32); 6] = [
    ("katana_idle", 3361416310, 1.0),
    ("AttackLeft", 2717246184, 1.0),
    ("AttackRight", 1504273457, 1.0),
    ("katana_AttackForward", 1634851761, 1.0),
    ("shovel_AttackBack", 815580648, 1.0),
    ("shovel_AttackStab", 2302561967, 1.0),
];
fn checked_any_transitions() -> Vec<MotionTransition> {
    vec![
        MotionTransition {
            id: 1699225119,
            full_path_id: 666786194,
            user_id: 0,
            destination: 2,
            duration: 0.10000000149011612,
            offset: 0.0,
            exit_time: 0.800000011920929,
            has_exit_time: true,
            fixed_duration: true,
            can_transition_to_self: true,
            interruption_source: 0,
            ordered_interruption: true,
            conditions: vec![MotionCondition {
                mode: 1,
                parameter: 1504273457,
                threshold: 0.0,
                exit_time: 0.0,
            }],
        },
        MotionTransition {
            id: 3233637805,
            full_path_id: 834792901,
            user_id: 0,
            destination: 3,
            duration: 0.10000000149011612,
            offset: 0.0,
            exit_time: 0.800000011920929,
            has_exit_time: true,
            fixed_duration: true,
            can_transition_to_self: true,
            interruption_source: 0,
            ordered_interruption: true,
            conditions: vec![MotionCondition {
                mode: 1,
                parameter: 1708274416,
                threshold: 0.0,
                exit_time: 0.0,
            }],
        },
        MotionTransition {
            id: 1131093452,
            full_path_id: 129301733,
            user_id: 0,
            destination: 1,
            duration: 0.10000000149011612,
            offset: 0.0,
            exit_time: 0.800000011920929,
            has_exit_time: true,
            fixed_duration: true,
            can_transition_to_self: true,
            interruption_source: 0,
            ordered_interruption: true,
            conditions: vec![MotionCondition {
                mode: 1,
                parameter: 2717246184,
                threshold: 0.0,
                exit_time: 0.0,
            }],
        },
        MotionTransition {
            id: 4105854552,
            full_path_id: 3058440661,
            user_id: 0,
            destination: 0,
            duration: 0.10000000149011612,
            offset: 0.0,
            exit_time: 9.427516545201797e-09,
            has_exit_time: false,
            fixed_duration: true,
            can_transition_to_self: false,
            interruption_source: 0,
            ordered_interruption: true,
            conditions: vec![MotionCondition {
                mode: 1,
                parameter: 1654612129,
                threshold: 0.0,
                exit_time: 0.0,
            }],
        },
        MotionTransition {
            id: 4259996521,
            full_path_id: 1694822627,
            user_id: 0,
            destination: 4,
            duration: 0.10000000149011612,
            offset: 0.0,
            exit_time: 0.800000011920929,
            has_exit_time: true,
            fixed_duration: true,
            can_transition_to_self: true,
            interruption_source: 0,
            ordered_interruption: true,
            conditions: vec![MotionCondition {
                mode: 1,
                parameter: 3059532983,
                threshold: 0.0,
                exit_time: 0.0,
            }],
        },
        MotionTransition {
            id: 1145628718,
            full_path_id: 3701898148,
            user_id: 0,
            destination: 5,
            duration: 0.10000000149011612,
            offset: 0.0,
            exit_time: 0.800000011920929,
            has_exit_time: true,
            fixed_duration: true,
            can_transition_to_self: true,
            interruption_source: 0,
            ordered_interruption: true,
            conditions: vec![MotionCondition {
                mode: 1,
                parameter: 268324848,
                threshold: 0.0,
                exit_time: 0.0,
            }],
        },
        MotionTransition {
            id: 4105854552,
            full_path_id: 3058440661,
            user_id: 0,
            destination: 0,
            duration: 0.10000000149011612,
            offset: 0.0,
            exit_time: 0.800000011920929,
            has_exit_time: true,
            fixed_duration: true,
            can_transition_to_self: false,
            interruption_source: 0,
            ordered_interruption: true,
            conditions: vec![],
        },
    ]
}
fn checked_idle_transitions() -> Vec<MotionTransition> {
    vec![
        MotionTransition {
            id: 3869188192,
            full_path_id: 156787784,
            user_id: 0,
            destination: 1,
            duration: 0.10000000149011612,
            offset: 0.0,
            exit_time: 0.0,
            has_exit_time: false,
            fixed_duration: true,
            can_transition_to_self: true,
            interruption_source: 0,
            ordered_interruption: true,
            conditions: vec![MotionCondition {
                mode: 1,
                parameter: 2717246184,
                threshold: 0.0,
                exit_time: 0.0,
            }],
        },
        MotionTransition {
            id: 3129804257,
            full_path_id: 2413259359,
            user_id: 0,
            destination: 2,
            duration: 0.10000000149011612,
            offset: 0.0,
            exit_time: 0.0,
            has_exit_time: false,
            fixed_duration: true,
            can_transition_to_self: true,
            interruption_source: 0,
            ordered_interruption: true,
            conditions: vec![MotionCondition {
                mode: 1,
                parameter: 1504273457,
                threshold: 0.0,
                exit_time: 0.0,
            }],
        },
        MotionTransition {
            id: 3256176142,
            full_path_id: 2557134833,
            user_id: 0,
            destination: 3,
            duration: 0.10000000149011612,
            offset: 0.0,
            exit_time: 0.0,
            has_exit_time: false,
            fixed_duration: true,
            can_transition_to_self: true,
            interruption_source: 0,
            ordered_interruption: true,
            conditions: vec![MotionCondition {
                mode: 1,
                parameter: 1708274416,
                threshold: 0.0,
                exit_time: 0.0,
            }],
        },
        MotionTransition {
            id: 33870250,
            full_path_id: 3216411534,
            user_id: 0,
            destination: 4,
            duration: 0.10000000149011612,
            offset: 0.0,
            exit_time: 0.0,
            has_exit_time: false,
            fixed_duration: true,
            can_transition_to_self: true,
            interruption_source: 0,
            ordered_interruption: true,
            conditions: vec![MotionCondition {
                mode: 1,
                parameter: 3059532983,
                threshold: 0.0,
                exit_time: 0.0,
            }],
        },
        MotionTransition {
            id: 3148241645,
            full_path_id: 101974217,
            user_id: 0,
            destination: 5,
            duration: 0.10000000149011612,
            offset: 0.0,
            exit_time: 4.299437961208241e-08,
            has_exit_time: false,
            fixed_duration: true,
            can_transition_to_self: true,
            interruption_source: 0,
            ordered_interruption: true,
            conditions: vec![MotionCondition {
                mode: 1,
                parameter: 268324848,
                threshold: 0.0,
                exit_time: 0.0,
            }],
        },
    ]
}
