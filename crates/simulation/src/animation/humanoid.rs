//! Source humanoid Animator of the human monster: `ThirdPersonAnimatorController` on the
//! `munkas01Avatar`, driven by `newAiAnim`, then the `LateUpdate` of `newAiScript`.
//! Poses use the source (left-handed) space under the Animator Transform.
//!
//! Port choices, from the avatar data, not from Unity code:
//! - A bone rotation is `preQ * SwingTwist(sign * angles) * inverse(postQ)`. A muscle `m` gives
//!   `m * max` for `m >= 0`, else `-m * min`. Swing turns about (0, y, z), twist about x.
//! - The body keeps no root motion (`applyRootMotion` is off). The root follows the body on the
//!   ground plane and turns with its yaw plus `m_OrientationOffsetY`. A baked yaw keeps the clip
//!   start yaw. A baked ground position moves the body from the clip start position. A height
//!   that is not baked keeps the clip start height. The port does not tell feet
//!   and original height bases apart.
//! - `m_Mirror` swaps the left and right muscles and negates the midline left-right and twist
//!   muscles, `RootT.x`, and the `RootQ` yaw and roll.
//! - The body rotation spans the upper arms and upper legs. Its right axis points from the
//!   left side to the right side, and its up axis from the legs to the arms. The body position
//!   is the mass center of the bone middles. The hips move so that the body takes its clip
//!   pose. Both match `m_RootX` within 1 cm.
//! - Feet take the clip muscles. The port does not use the clip foot IK goals or redistribute
//!   twist.
//! - Hand IK is a two-bone solve that keeps the bend plane and the hand rotation.
//!   Look-at turns the neck and the head half each, up to 90 degrees (`clampWeight` 0.5).
use super::clip::sample_track;
use crate::player::body::ik::Frame;
use crate::reference::{
    FIRST_MUSCLE, HUMAN_BONES, HumanReference, HumanoidChild, MUSCLES, MotionTrack, NodeId,
    NodeRecord, ROOT_T, ReferenceError, Transform,
};
use rapier3d::math::{Pose, Rotation, Vector};
use std::collections::HashMap;

/// Human bone and axis (0 twist x, 1 y, 2 z) of each muscle, in Mecanim order.
const MUSCLE_MAP: [(u8, u8); MUSCLES] = muscle_map();

const fn muscle_map() -> [(u8, u8); MUSCLES] {
    let mut m = [(0u8, 0u8); MUSCLES];
    let mut i = 0;
    // Spine, Chest, UpperChest, Neck, Head: front-back, left-right, twist.
    let body = [7u8, 8, 9, 10, 11];
    let mut b = 0;
    while b < 5 {
        m[i] = (body[b], 2);
        m[i + 1] = (body[b], 1);
        m[i + 2] = (body[b], 0);
        i += 3;
        b += 1;
    }
    // Eyes: down-up, in-out. Jaw: close, left-right.
    let face = [22u8, 23, 24];
    b = 0;
    while b < 3 {
        m[i] = (face[b], 2);
        m[i + 1] = (face[b], 1);
        i += 2;
        b += 1;
    }
    // Legs: upper leg (3), lower leg stretch and twist, foot up-down and twist, toes.
    let mut side = 0;
    while side < 2 {
        let s = side;
        let leg = [
            (1 + s, 2),
            (1 + s, 1),
            (1 + s, 0),
            (3 + s, 2),
            (3 + s, 0),
            (5 + s, 2),
            (5 + s, 0),
            (20 + s, 2),
        ];
        let mut k = 0;
        while k < 8 {
            m[i] = leg[k];
            i += 1;
            k += 1;
        }
        side += 1;
    }
    // Arms: shoulder (2), upper arm (3), forearm stretch and twist, hand down-up and in-out.
    side = 0;
    while side < 2 {
        let s = side;
        let arm = [
            (12 + s, 2),
            (12 + s, 1),
            (14 + s, 2),
            (14 + s, 1),
            (14 + s, 0),
            (16 + s, 2),
            (16 + s, 0),
            (18 + s, 2),
            (18 + s, 1),
        ];
        let mut k = 0;
        while k < 9 {
            m[i] = arm[k];
            i += 1;
            k += 1;
        }
        side += 1;
    }
    // Fingers: thumb, index, middle, ring, little. Proximal stretch and spread, then two stretches.
    side = 0;
    while side < 2 {
        let mut f = 0;
        while f < 5 {
            let base = 25 + side * 15 + f * 3;
            m[i] = (base, 2);
            m[i + 1] = (base, 1);
            m[i + 2] = (base + 1, 2);
            m[i + 3] = (base + 2, 2);
            i += 4;
            f += 1;
        }
        side += 1;
    }
    m
}

/// Mirror source muscle and sign of each muscle.
const MIRROR_MAP: [(u8, f32); MUSCLES] = mirror_map();

const fn mirror_map() -> [(u8, f32); MUSCLES] {
    let mut m = [(0u8, 1f32); MUSCLES];
    let mut i = 0;
    while i < MUSCLES {
        m[i] = match i {
            // Spine to head: front-back keeps, left-right and twist negate.
            0..15 => (i as u8, if i % 3 == 0 { 1. } else { -1. }),
            15..17 => (i as u8 + 2, 1.),
            17..19 => (i as u8 - 2, 1.),
            19 => (19, 1.),
            20 => (20, -1.),
            21..29 => (i as u8 + 8, 1.),
            29..37 => (i as u8 - 8, 1.),
            37..46 => (i as u8 + 9, 1.),
            46..55 => (i as u8 - 9, 1.),
            55..75 => (i as u8 + 20, 1.),
            _ => (i as u8 - 20, 1.),
        };
        i += 1;
    }
    m
}

/// The body mass center takes the middle of each bone and its next bone, or the bone itself.
/// Bones 25 and up have no mass.
const MASS_NEXT: [&[u8]; 25] = [
    &[7],
    &[3],
    &[4],
    &[5],
    &[6],
    &[20],
    &[21],
    &[8],
    &[9, 10],
    &[10],
    &[11],
    &[],
    &[14],
    &[15],
    &[16],
    &[17],
    &[18],
    &[19],
    &[],
    &[],
    &[],
    &[],
    &[],
    &[],
    &[],
];

const HIPS: usize = 0;
const NECK: usize = 10;
const HEAD: usize = 11;
/// Upper arm, forearm and hand of the left and right arm.
const ARMS: [[usize; 3]; 2] = [[14, 16, 18], [15, 17, 19]];
/// `SetLookAtWeight(1)` gives `clampWeight` 0.5, which is half of the 180 degree range.
const LOOK_CLAMP: f32 = std::f32::consts::FRAC_PI_2;

/// Converted space and source space differ by a Z mirror. The map is its own inverse.
pub fn mirror_point(v: Vector) -> Vector {
    Vector::new(v.x, v.y, -v.z)
}
pub fn mirror_rotation(r: Rotation) -> Rotation {
    Rotation::from_xyzw(-r.x, -r.y, r.z, r.w)
}
fn mirror_frame(f: &Frame) -> Frame {
    Frame {
        position: mirror_point(f.position),
        rotation: mirror_rotation(f.rotation),
        scale: f.scale,
    }
}
fn frame(t: &Transform) -> Frame {
    Frame {
        position: Vector::from_array(t.translation),
        rotation: Rotation::from_array(t.rotation).normalize(),
        scale: Vector::from_array(t.scale),
    }
}

#[derive(Clone, Copy)]
struct Bone {
    node: usize,
    pre: Rotation,
    post_inverse: Rotation,
    sign: Vector,
    min: Vector,
    max: Vector,
}

struct Clip {
    length: f32,
    /// `m_OrientationOffsetY` in radians.
    offset: f32,
    level: f32,
    mirror: bool,
    cycle_offset: f32,
    /// Root yaw when baked, ground position when baked and height when not baked, at the clip start.
    start_yaw: Option<f32>,
    start_ground: Option<[f32; 2]>,
    start_height: Option<f32>,
    /// `RootT` and `RootQ` tracks.
    root: [Option<usize>; 7],
    muscles: [Option<usize>; MUSCLES],
    tracks: Vec<MotionTrack>,
}

impl Clip {
    fn value(&self, track: Option<usize>, t: f32) -> f32 {
        track.map_or(0., |i| sample_track(&self.tracks[i], t)[0])
    }
    /// `RootT` on the ground plane, its height and `RootQ` at clip time `t`, mirrored when the
    /// clip mirrors.
    fn root_at(&self, t: f32) -> ([f32; 2], f32, Rotation) {
        let [x, y, z, w] = [3, 4, 5, 6].map(|i| self.value(self.root[i], t));
        let [tx, ty, tz] = [0, 1, 2].map(|i| self.value(self.root[i], t));
        if self.mirror {
            ([-tx, tz], ty, Rotation::from_xyzw(x, -y, -z, w).normalize())
        } else {
            ([tx, tz], ty, Rotation::from_xyzw(x, y, z, w).normalize())
        }
    }
}

/// A muscle pose: the body under the root and the muscle values.
#[derive(Clone, Copy)]
struct MusclePose {
    body: Frame,
    muscles: [f32; MUSCLES],
}
impl MusclePose {
    const ZERO: Self = Self {
        body: Frame {
            position: Vector::ZERO,
            rotation: Rotation::from_xyzw(0., 0., 0., 0.),
            scale: Vector::ONE,
        },
        muscles: [0.; MUSCLES],
    };
    /// Adds `other` with `weight`. Rotations add on the same hemisphere.
    fn add(&mut self, other: &Self, weight: f32) {
        let mut q = other.body.rotation;
        if self.body.rotation.dot(q) < 0. {
            q = -q;
        }
        self.body.position += other.body.position * weight;
        self.body.rotation += q * weight;
        for (a, b) in self.muscles.iter_mut().zip(&other.muscles) {
            *a += b * weight;
        }
    }
}

/// The shared Animator data. Index 0 is the Animator Transform.
pub struct Humanoid {
    nodes: Vec<NodeId>,
    index: HashMap<NodeId, usize>,
    parent: Vec<Option<usize>>,
    /// Prefab parent-local frames.
    rest: Vec<Frame>,
    bones: [Option<Bone>; HUMAN_BONES],
    /// `m_HumanBoneMass` of the 25 body bones.
    masses: [f32; 25],
    scale: f32,
    head_forward: Vector,
    clips: Vec<Clip>,
    states: [Vec<HumanoidChild>; 2],
    fades: [f32; 2],
    head: usize,
    head_limit: f32,
    /// Copied rig node and its target rig node.
    copies: Vec<(usize, usize)>,
    mouth: Option<(usize, Frame, Frame)>,
    forward_lerp: f32,
    turn_lerp: f32,
    look_lerp: f32,
    hand_up: f32,
    hand_down: f32,
    hand_delay: f32,
}

/// `newAiAnim.handik`: a `Hold` target under the Animator Transform.
#[derive(Clone, Copy, Debug)]
struct Hand {
    target: Vector,
    weight: f32,
    last: f64,
}

/// `Crouching` transition in progress.
#[derive(Clone, Copy, Debug)]
struct Fade {
    to: usize,
    time: f32,
    elapsed: f32,
    duration: f32,
}

/// Animator and `newAiAnim` state of one human monster.
pub struct HumanoidPose {
    state: usize,
    time: f32,
    fade: Option<Fade>,
    forward: f32,
    turn: f32,
    /// `lookTarget` in converted world space. None before the first update.
    look: Option<Vector>,
    hands: [Hand; 2],
    /// Parent-local frames, source space.
    pub local: Vec<Frame>,
    /// Frames under the Animator Transform, source space.
    pub model: Vec<Frame>,
    weights: Vec<f32>,
}

/// What `newAiScript` sets this frame.
#[derive(Clone, Copy, Debug)]
pub struct HumanoidInput {
    pub now: f64,
    pub dt: f32,
    /// World pose of the Animator Transform, converted space.
    pub root: Pose,
    pub target_forward: f32,
    pub target_turn: f32,
    pub crouch: bool,
    /// `ai.target.position`, converted world space.
    pub look: Vector,
    /// `Hold` points of the left and right hand this frame, converted world space.
    pub holds: [Option<Vector>; 2],
    /// `currentVoiceMove`.
    pub mouth: f32,
}

/// Yaw of the forward (+Z) axis of `rotation`.
fn yaw(rotation: Rotation) -> f32 {
    let forward = rotation * Vector::Z;
    forward.x.atan2(forward.z)
}

fn swing_twist(angles: Vector) -> Rotation {
    let swing = Vector::new(0., angles.y, angles.z);
    let angle = swing.length();
    let swing = if angle > 1e-9 {
        Rotation::from_axis_angle(swing / angle, angle)
    } else {
        Rotation::IDENTITY
    };
    swing * Rotation::from_rotation_x(angles.x)
}

/// Unity `Quaternion.FromToRotation`.
fn from_to(from: Vector, to: Vector) -> Rotation {
    match (from.try_normalize(), to.try_normalize()) {
        (Some(a), Some(b)) => Rotation::from_rotation_arc(a, b),
        _ => Rotation::IDENTITY,
    }
}

fn move_towards(current: f32, target: f32, delta: f32) -> f32 {
    if (target - current).abs() <= delta {
        target
    } else {
        current + (target - current).signum() * delta
    }
}

/// Gradient band weights of a 2D freeform cartesian blend tree, normalized.
fn cartesian_weights(children: &[HumanoidChild], point: [f32; 2], out: &mut Vec<f32>) {
    out.clear();
    let p = Vector::new(point[0], point[1], 0.);
    for (i, a) in children.iter().enumerate() {
        let pi = Vector::new(a.position[0], a.position[1], 0.);
        let mut w = 1f32;
        for (j, b) in children.iter().enumerate() {
            if i == j {
                continue;
            }
            let pij = Vector::new(b.position[0], b.position[1], 0.) - pi;
            let length = pij.length_squared();
            if length > 0. {
                w = w.min(1. - (p - pi).dot(pij) / length);
            }
        }
        out.push(w.max(0.));
    }
    let sum: f32 = out.iter().sum();
    if sum > 0. {
        out.iter_mut().for_each(|w| *w /= sum);
    } else if let Some(first) = out.first_mut() {
        *first = 1.;
    }
}

impl Humanoid {
    pub fn new(nodes: &[NodeRecord], human: &HumanReference) -> Result<Self, ReferenceError> {
        let (r, voice) = (&human.animator, &human.voice);
        let fail = |text: &str| ReferenceError(format!("Human monster Animator: {text}"));
        let asset = nodes
            .iter()
            .find(|n| n.id == r.root)
            .ok_or_else(|| fail("missing root"))?
            .asset;
        let records: HashMap<_, _> = nodes
            .iter()
            .filter(|n| n.asset == asset)
            .map(|n| (n.id, n))
            .collect();
        let depth = |mut id: NodeId| {
            let mut depth = 0;
            while id != r.root {
                id = records.get(&id)?.parent?;
                depth += 1;
            }
            Some(depth)
        };
        let mut ids: Vec<(usize, NodeId)> = records
            .keys()
            .filter_map(|id| Some((depth(*id)?, *id)))
            .collect();
        ids.sort_by_key(|(depth, id)| (*depth, id.0));
        let ids: Vec<NodeId> = ids.into_iter().map(|(_, id)| id).collect();
        let index: HashMap<_, _> = ids.iter().enumerate().map(|(i, id)| (*id, i)).collect();
        let parent = ids
            .iter()
            .enumerate()
            .map(|(i, id)| {
                (i > 0)
                    .then(|| records[id].parent.and_then(|p| index.get(&p).copied()))
                    .flatten()
            })
            .collect();
        let rest = ids
            .iter()
            .enumerate()
            .map(|(i, id)| {
                if i == 0 {
                    Frame::IDENTITY
                } else {
                    mirror_frame(&frame(&records[id].transform))
                }
            })
            .collect();
        let at = |id: NodeId| {
            index
                .get(&id)
                .copied()
                .ok_or_else(|| fail("node outside the rig"))
        };
        let mut bones = [None; HUMAN_BONES];
        for (slot, bone) in bones.iter_mut().zip(&r.bones) {
            let Some(b) = bone else { continue };
            let a = &b.axes;
            *slot = Some(Bone {
                node: at(b.node)?,
                pre: Rotation::from_array(a.pre).normalize(),
                post_inverse: Rotation::from_array(a.post).normalize().inverse(),
                sign: Vector::from_array(a.sign),
                min: Vector::from_array(a.min),
                max: Vector::from_array(a.max),
            });
        }
        if bones[HIPS].is_none() || bones[HEAD].is_none() || bones[NECK].is_none() {
            return Err(fail("missing hips, neck or head"));
        }
        if ARMS.iter().flatten().any(|b| bones[*b].is_none()) {
            return Err(fail("missing arm bones"));
        }
        let mut clips = Vec::with_capacity(r.clips.len());
        for c in &r.clips {
            let find = |attribute: u32| c.tracks.iter().position(|t| t.attribute == attribute);
            let mut clip = Clip {
                length: c.stop_time,
                offset: c.orientation_offset.to_radians(),
                level: c.level,
                mirror: c.mirror,
                cycle_offset: c.cycle_offset,
                start_yaw: None,
                start_ground: None,
                start_height: None,
                root: std::array::from_fn(|i| find(ROOT_T + i as u32)),
                muscles: std::array::from_fn(|i| find(FIRST_MUSCLE + i as u32)),
                tracks: c.tracks.clone(),
            };
            if clip.root.iter().any(Option::is_none) {
                return Err(fail("clip without RootT or RootQ"));
            }
            let (ground, height, q) = clip.root_at(0.);
            clip.start_yaw = c.bake_orientation.then_some(yaw(q));
            clip.start_ground = c.bake_position.then_some(ground);
            clip.start_height = (!c.bake_height).then_some(height);
            clips.push(clip);
        }
        let copies = r
            .copies
            .iter()
            .map(|c| Ok((at(c.node)?, at(c.target)?)))
            .collect::<Result<_, ReferenceError>>()?;
        let mouth = Some((at(voice.mouth)?, frame(&voice.closed), frame(&voice.open)));
        let head = at(human.head)?;
        Ok(Self {
            nodes: ids,
            index,
            parent,
            rest,
            bones,
            masses: std::array::from_fn(|i| r.masses[i]),
            scale: r.scale,
            head_forward: Vector::from_array(r.head_forward),
            clips,
            states: [r.grounded.clone(), r.crouching.clone()],
            fades: r.fades,
            head,
            head_limit: r.head_limit.to_radians(),
            copies,
            mouth,
            forward_lerp: r.forward_lerp,
            turn_lerp: r.turn_lerp,
            look_lerp: r.look_lerp,
            hand_up: r.hand_weight_up,
            hand_down: r.hand_weight_down,
            hand_delay: r.hand_weight_down_delay,
        })
    }

    pub fn len(&self) -> usize {
        self.nodes.len()
    }
    pub fn is_empty(&self) -> bool {
        self.nodes.is_empty()
    }
    pub fn node(&self, index: usize) -> NodeId {
        self.nodes[index]
    }
    pub fn index(&self, node: NodeId) -> Option<usize> {
        self.index.get(&node).copied()
    }

    /// The rest pose at `Start`. `newAiAnim.Start` makes the hand targets.
    pub fn pose(&self) -> HumanoidPose {
        let hand = Hand {
            target: Vector::ZERO,
            weight: 0.,
            last: f64::NEG_INFINITY,
        };
        let mut pose = HumanoidPose {
            state: 0,
            time: 0.,
            fade: None,
            forward: 0.,
            turn: 0.,
            look: None,
            hands: [hand; 2],
            local: self.rest.clone(),
            model: self.rest.clone(),
            weights: Vec::with_capacity(self.states.iter().map(Vec::len).max().unwrap_or(0)),
        };
        self.forward_kinematics(&mut pose);
        pose
    }

    /// Converted frame of rig node `index` under the Animator Transform.
    pub fn converted(&self, pose: &HumanoidPose, index: usize) -> Frame {
        mirror_frame(&pose.model[index])
    }

    /// Converted pose of `newAiScript.head` under the Animator Transform.
    pub fn head(&self, pose: &HumanoidPose) -> Pose {
        let f = self.converted(pose, self.head);
        Pose::from_parts(f.position, f.rotation)
    }

    fn forward_kinematics(&self, pose: &mut HumanoidPose) {
        for i in 0..self.nodes.len() {
            pose.model[i] = match self.parent[i] {
                Some(p) => pose.model[p].mul(&pose.local[i]),
                None => pose.local[i],
            };
        }
    }

    /// Sets a model rotation. Children update in the next forward kinematics.
    fn set_model_rotation(&self, pose: &mut HumanoidPose, i: usize, rotation: Rotation) {
        let parent = self.parent[i].map_or(Rotation::IDENTITY, |p| pose.model[p].rotation);
        pose.local[i].rotation = parent.inverse() * rotation;
        pose.model[i].rotation = rotation;
    }

    fn state_length(&self, state: usize, weights: &[f32]) -> f32 {
        self.states[state]
            .iter()
            .zip(weights)
            .map(|(c, w)| w * self.clips[c.clip as usize].length / c.speed)
            .sum::<f32>()
            .max(1e-4)
    }

    /// Clip muscle pose at clip time `t`.
    fn sample(&self, clip: &Clip, t: f32) -> MusclePose {
        let (ground, height, q) = clip.root_at(t);
        let height = clip.start_height.unwrap_or(height);
        let root_yaw = clip.start_yaw.unwrap_or_else(|| yaw(q)) + clip.offset;
        let [x, z] = clip
            .start_ground
            .map_or([0.; 2], |s| [ground[0] - s[0], ground[1] - s[1]]);
        let offset = Rotation::from_rotation_y(-root_yaw) * Vector::new(x, 0., z) * self.scale;
        MusclePose {
            body: Frame {
                position: offset + Vector::Y * (height * self.scale + clip.level),
                rotation: Rotation::from_rotation_y(-root_yaw) * q,
                scale: Vector::ONE,
            },
            muscles: std::array::from_fn(|i| {
                if clip.mirror {
                    let (j, sign) = MIRROR_MAP[i];
                    sign * clip.value(clip.muscles[j as usize], t)
                } else {
                    clip.value(clip.muscles[i], t)
                }
            }),
        }
    }

    /// Blend tree pose of `state` at normalized time `time`.
    fn evaluate(&self, state: usize, time: f32, weights: &[f32]) -> MusclePose {
        let mut out = MusclePose::ZERO;
        for (child, w) in self.states[state].iter().zip(weights) {
            if *w <= 0. {
                continue;
            }
            let clip = &self.clips[child.clip as usize];
            let p = self.sample(
                clip,
                (time + clip.cycle_offset).rem_euclid(1.) * clip.length,
            );
            out.add(&p, *w);
        }
        out.body.rotation = out.body.rotation.normalize();
        out
    }

    /// `newAiAnim.Update`, the Animator update with `OnAnimatorIK`, then `newAiScript.LateUpdate`.
    pub fn update(&self, pose: &mut HumanoidPose, input: &HumanoidInput) {
        let dt = input.dt;
        // `newAiAnim.Update` and `Hold`.
        pose.forward +=
            (input.target_forward - pose.forward) * (dt * self.forward_lerp).clamp(0., 1.);
        pose.turn += (input.target_turn - pose.turn) * (dt * self.turn_lerp).clamp(0., 1.);
        let to_model = |p: Vector| mirror_point(input.root.inverse_transform_point(p));
        for (hand, hold) in pose.hands.iter_mut().zip(input.holds) {
            if let Some(p) = hold {
                hand.target = to_model(p);
                hand.last = input.now;
                hand.weight = move_towards(hand.weight, 1., dt * self.hand_up);
            }
            if input.now - hand.last > f64::from(self.hand_delay) {
                hand.weight = move_towards(hand.weight, 0., dt * self.hand_down);
            }
        }
        // In the state machine, a true `Crouch` leaves `Grounded`.
        // A false `Crouch` leaves `Crouching`.
        let point = [pose.turn, pose.forward];
        cartesian_weights(&self.states[pose.state], point, &mut pose.weights);
        let length = self.state_length(pose.state, &pose.weights);
        let wanted = usize::from(input.crouch);
        if pose.fade.is_none() && wanted != pose.state {
            pose.fade = Some(Fade {
                to: wanted,
                time: 0.,
                elapsed: 0.,
                duration: self.fades[wanted] * length,
            });
        }
        pose.time += dt / length;
        let mut current = self.evaluate(pose.state, pose.time, &pose.weights);
        if let Some(mut fade) = pose.fade {
            cartesian_weights(&self.states[fade.to], point, &mut pose.weights);
            fade.time += dt / self.state_length(fade.to, &pose.weights);
            fade.elapsed += dt;
            let next = self.evaluate(fade.to, fade.time, &pose.weights);
            let w = (fade.elapsed / fade.duration.max(1e-4)).clamp(0., 1.);
            if w >= 1. {
                current = next;
                pose.state = fade.to;
                pose.time = fade.time;
                pose.fade = None;
            } else {
                let mut blended = MusclePose::ZERO;
                blended.add(&current, 1. - w);
                blended.add(&next, w);
                blended.body.rotation = blended.body.rotation.normalize();
                current = blended;
                pose.fade = Some(fade);
            }
        }
        self.muscle_pose(pose, &current);
        // `OnAnimatorIK`: look-at, then the hands.
        let look = pose.look.get_or_insert(input.look);
        *look = look.lerp(input.look, (dt * self.look_lerp).clamp(0., 1.));
        let look = to_model(*look);
        self.look_at(pose, look);
        for (side, arm) in ARMS.iter().enumerate() {
            let hand = pose.hands[side];
            if hand.weight > 0. {
                self.solve_arm(pose, arm, hand);
            }
        }
        self.late_update(pose, input.mouth);
    }

    /// Muscles to bone rotations, the hips from the body, then forward kinematics.
    fn muscle_pose(&self, pose: &mut HumanoidPose, p: &MusclePose) {
        pose.local.copy_from_slice(&self.rest);
        let mut angles = [Vector::ZERO; HUMAN_BONES];
        for (m, (bone, axis)) in p.muscles.iter().zip(MUSCLE_MAP) {
            let Some(b) = &self.bones[bone as usize] else {
                continue;
            };
            let limit = if *m >= 0. { b.max } else { -b.min };
            angles[bone as usize][axis as usize] = m * limit[axis as usize];
        }
        for (bone, a) in self.bones.iter().zip(angles).skip(1) {
            let Some(b) = bone else { continue };
            pose.local[b.node].rotation = b.pre * swing_twist(b.sign * a) * b.post_inverse;
        }
        let hips = self.bones[HIPS].unwrap().node;
        self.forward_kinematics(pose);
        // The body in the hips frame, then the hips that put the body at its clip pose.
        let frame = pose.model[hips];
        let at =
            |bone: usize| self.bones[bone].map(|b| frame.local_point(pose.model[b.node].position));
        let point = |bone: usize| at(bone).unwrap_or(Vector::ZERO);
        let right = (point(15) + point(2) - point(14) - point(1)) * 0.5;
        let up = (point(14) + point(15) - point(1) - point(2)) * 0.5;
        let forward = right.cross(up).normalize_or_zero();
        let up = forward.cross(right).normalize_or_zero();
        let rotation = Rotation::from_mat3(&rapier3d::glamx::Mat3::from_cols(
            right.normalize_or_zero(),
            up,
            forward,
        ));
        let mut center = Vector::ZERO;
        let mut total = 0.;
        for (bone, mass) in self.masses.iter().enumerate() {
            let Some(p) = at(bone).filter(|_| *mass > 0.) else {
                continue;
            };
            let next = MASS_NEXT[bone]
                .iter()
                .find_map(|n| at(*n as usize))
                .unwrap_or(p);
            center += (p + next) * 0.5 * *mass;
            total += mass;
        }
        let center = center / total.max(1e-6);
        let hips_rotation = p.body.rotation * rotation.inverse();
        let model = Frame {
            position: p.body.position - hips_rotation * center,
            rotation: hips_rotation,
            scale: frame.scale,
        };
        let parent = self.parent[hips].map_or(Frame::IDENTITY, |i| pose.model[i]);
        pose.local[hips].position = parent.local_point(model.position);
        pose.local[hips].rotation = parent.rotation.inverse() * model.rotation;
        self.forward_kinematics(pose);
    }

    /// `SetLookAtPosition` with weight 1. The neck and the head turn toward `target`.
    fn look_at(&self, pose: &mut HumanoidPose, target: Vector) {
        let head = self.bones[HEAD].unwrap().node;
        let neck = self.bones[NECK].unwrap().node;
        let h = pose.model[head];
        let current = h.rotation * self.head_forward;
        let (axis, angle) = from_to(current, target - h.position).to_axis_angle();
        if !angle.is_finite() || angle <= 0. {
            return;
        }
        let half = Rotation::from_axis_angle(axis, angle.min(LOOK_CLAMP) * 0.5);
        let n = pose.model[neck].rotation;
        self.set_model_rotation(pose, neck, half * n);
        self.set_model_rotation(pose, head, half * half * h.rotation);
        self.forward_kinematics(pose);
    }

    /// Two-bone arm IK toward the weighted hand target. The elbow keeps its bend plane.
    fn solve_arm(&self, pose: &mut HumanoidPose, arm: &[usize; 3], hand: Hand) {
        let [upper, mid, end] = arm.map(|b| self.bones[b].unwrap().node);
        let (a, b, c) = (
            pose.model[upper].position,
            pose.model[mid].position,
            pose.model[end].position,
        );
        let goal = c.lerp(hand.target, hand.weight.clamp(0., 1.));
        let hand_rotation = pose.model[end].rotation;
        let (l1, l2) = (a.distance(b), b.distance(c));
        let to = goal - a;
        let Some(dir) = to.try_normalize() else {
            return;
        };
        let reach = to.length().clamp((l1 - l2).abs() + 1e-4, l1 + l2 - 1e-4);
        let bend = b - a;
        let side = (bend - dir * bend.dot(dir))
            .try_normalize()
            .unwrap_or_else(|| dir.any_orthonormal_vector());
        let cos = ((l1 * l1 + reach * reach - l2 * l2) / (2. * l1 * reach)).clamp(-1., 1.);
        let elbow = a + (dir * cos + side * (1. - cos * cos).sqrt()) * l1;
        let swing = from_to(b - a, elbow - a);
        self.set_model_rotation(pose, upper, swing * pose.model[upper].rotation);
        self.forward_kinematics(pose);
        let joint = pose.model[mid].position;
        let swing_mid = from_to(pose.model[end].position - joint, a + dir * reach - joint);
        self.set_model_rotation(pose, mid, swing_mid * pose.model[mid].rotation);
        self.forward_kinematics(pose);
        self.set_model_rotation(pose, end, hand_rotation);
        self.forward_kinematics(pose);
    }

    /// `newAiScript.LateUpdate`: `TVoice`, the eye copies, then the head limit.
    fn late_update(&self, pose: &mut HumanoidPose, mouth: f32) {
        if let Some((node, closed, open)) = self.mouth {
            pose.local[node] = Frame {
                position: closed.position.lerp(open.position, mouth),
                rotation: closed.rotation.slerp(open.rotation, mouth),
                scale: closed.scale.lerp(open.scale, mouth),
            };
        }
        self.forward_kinematics(pose);
        // Position, rotation and `localScale` of the target.
        for &(node, target) in &self.copies {
            let parent = self.parent[node].map_or(Frame::IDENTITY, |p| pose.model[p]);
            let mut local = parent.inverse_mul(&pose.model[target]);
            local.scale = pose.local[target].scale;
            pose.local[node] = local;
            pose.model[node] = parent.mul(&local);
        }
        // `RotateTowards(transform.rotation, head.rotation, headRotLimit)`. The prefab root
        // and the Animator Transform share their rotation.
        let head = pose.model[self.head].rotation;
        let angle = Rotation::IDENTITY.angle_between(head);
        if angle > self.head_limit {
            let limited = Rotation::IDENTITY.slerp(head, self.head_limit / angle);
            self.set_model_rotation(pose, self.head, limited);
        }
        self.forward_kinematics(pose);
    }
}
