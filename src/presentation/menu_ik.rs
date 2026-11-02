//! Source `menuIkScript.OnAnimatorIK` on the start menu figure, with a port of the humanoid IK it drives.
//! All frames are local to the figure root `MenuCharacter`, in converted space: the figure faces -Z.
//! Port choices: the base pose is the exported scene pose, not the `HumanoidIdle` clip.
//! Limb IK is a two-bone solve with a fixed bend direction. Look-at turns the neck and head; `SetLookAtWeight(1)` keeps the body still.
use crate::reference::{MenuFigureReference, Transform as SourcePose};
use bevy::math::{Quat, Vec2, Vec3};
use bevy::prelude::Transform;

/// `SetLookAtWeight(1)` gives `clampWeight` 0.5: half of the 180 degree range each side.
const LOOK_CLAMP: f32 = std::f32::consts::FRAC_PI_2;
/// Bend directions in figure space: elbows point back and out, knees point forward.
const ELBOW_LEFT: Vec3 = Vec3::new(-0.3, 0., 1.);
const ELBOW_RIGHT: Vec3 = Vec3::new(0.3, 0., 1.);
const KNEE: Vec3 = Vec3::new(0., 0., -1.);

/// The click ray. The stage holds only the figure.
const RAY_LENGTH: f32 = 100.;

pub const LEFT_MOUSE: u32 = 1;
pub const RIGHT_MOUSE: u32 = 2;

/// Mouse state for one frame.
#[derive(Clone, Copy, Default)]
pub struct MenuMouse {
    /// Cursor in window fractions, from the top left.
    pub position: Vec2,
    /// Held buttons.
    pub buttons: u32,
    /// Buttons pressed since the last frame.
    pub pressed: u32,
}

/// The stage camera, in figure space.
#[derive(Clone, Copy)]
pub struct MenuView {
    pub pose: Transform,
    /// Vertical field of view in radians.
    pub fov: f32,
    /// Width over height.
    pub aspect: f32,
}
impl MenuView {
    /// Unity `ScreenToWorldPoint` with z = `depth`.
    fn point(&self, mouse: Vec2, depth: f32) -> Vec3 {
        let t = (self.fov * 0.5).tan();
        let ndc = Vec2::new(mouse.x * 2. - 1., 1. - mouse.y * 2.);
        self.pose
            .transform_point(Vec3::new(ndc.x * t * self.aspect, ndc.y * t, -1.) * depth)
    }
}

#[derive(Clone, Copy)]
struct Goal {
    position: Vec3,
    rotation: Quat,
}
impl Goal {
    fn of(p: &SourcePose) -> Self {
        Self {
            position: Vec3::from_array(p.translation),
            rotation: Quat::from_array(p.rotation),
        }
    }
}

#[derive(Clone, Copy)]
struct Limb {
    upper: usize,
    mid: usize,
    end: usize,
    bend: Vec3,
    /// End bone rotation relative to its goal rotation. Set on the first solve.
    offset: Option<Quat>,
}

pub struct MenuIk {
    parents: Vec<Option<usize>>,
    /// Parents before children.
    order: Vec<usize>,
    /// Rest pose of each joint, local to its parent joint or to the figure root.
    rest: Vec<Transform>,
    local: Vec<Transform>,
    world: Vec<Transform>,
    neck: usize,
    head: usize,
    /// Head-local direction that faces forward in the rest pose.
    head_forward: Vec3,
    /// Left hand, right hand, left foot, right foot.
    limbs: [Limb; 4],
    llu: usize,
    rhu: usize,
    lh_parent: usize,
    rh_parent: usize,
    // `B`, `H`, `LL` and `RL` are children of the figure root. `LH` and `RH` are local to their parent joint.
    b: Goal,
    h: Goal,
    ll: Goal,
    rl: Goal,
    lh: Goal,
    rh: Goal,
    def_b: Goal,
    def_ll: Goal,
    def_lh: Goal,
    def_rh: Goal,
    /// `Body` follows `B`. Its start pose equals `B`.
    body: Vec3,
    clicked: bool,
    collider: (Vec3, Vec3, f32),
    mouse_forward: f32,
    mouse_forward_l: f32,
    mouse_forward_h: f32,
    max_lerp: f32,
    lerp_speed: f32,
    rot_lerp_speed: f32,
    body_stance: f32,
}

fn pose(p: &SourcePose) -> Transform {
    Transform {
        translation: Vec3::from_array(p.translation),
        rotation: Quat::from_array(p.rotation),
        scale: Vec3::from_array(p.scale),
    }
}
/// `child` in the space of `parent`. All rig scales are uniform.
fn relative(parent: &Transform, child: &Transform) -> Transform {
    let inverse = parent.rotation.inverse();
    Transform {
        translation: inverse * (child.translation - parent.translation) / parent.scale,
        rotation: inverse * child.rotation,
        scale: child.scale / parent.scale,
    }
}
fn clamp_magnitude(v: Vec3, max: f32) -> Vec3 {
    v.clamp_length_max(max)
}
/// Unity `Vector3.MoveTowards`.
fn move_towards(from: Vec3, to: Vec3, step: f32) -> Vec3 {
    let d = to - from;
    let length = d.length();
    if length <= step || length == 0. {
        to
    } else {
        from + d / length * step
    }
}
/// Unity `Quaternion.LookRotation`: source +Z is converted -Z, which is the Bevy forward.
fn look_rotation(forward: Vec3, up: Vec3) -> Quat {
    if forward.length_squared() < 1e-12 {
        return Quat::IDENTITY;
    }
    Transform::IDENTITY.looking_to(forward, up).rotation
}
fn from_to(from: Vec3, to: Vec3) -> Quat {
    match (from.try_normalize(), to.try_normalize()) {
        (Some(a), Some(b)) => Quat::from_rotation_arc(a, b),
        _ => Quat::IDENTITY,
    }
}

/// Distance between segments `p1 q1` and `p2 q2` (Ericson, `ClosestPtSegmentSegment`).
fn segment_distance(p1: Vec3, q1: Vec3, p2: Vec3, q2: Vec3) -> f32 {
    const EPSILON: f32 = 1e-9;
    let (d1, d2, r) = (q1 - p1, q2 - p2, p1 - p2);
    let (a, e, f) = (d1.dot(d1), d2.dot(d2), d2.dot(r));
    let (s, t) = if a <= EPSILON && e <= EPSILON {
        (0., 0.)
    } else if a <= EPSILON {
        (0., (f / e).clamp(0., 1.))
    } else {
        let c = d1.dot(r);
        if e <= EPSILON {
            ((-c / a).clamp(0., 1.), 0.)
        } else {
            let b = d1.dot(d2);
            let denominator = a * e - b * b;
            let s = if denominator > EPSILON {
                ((b * f - c * e) / denominator).clamp(0., 1.)
            } else {
                0.
            };
            let t = (b * s + f) / e;
            if t < 0. {
                ((-c / a).clamp(0., 1.), 0.)
            } else if t > 1. {
                (((b - c) / a).clamp(0., 1.), 1.)
            } else {
                (s, t)
            }
        }
    };
    (p1 + d1 * s).distance(p2 + d2 * t)
}

impl MenuIk {
    /// `parents` are the nearest joint ancestors of the figure joints. `FStart` keeps the default goals.
    pub fn new(record: &MenuFigureReference, parents: &[Option<usize>]) -> Option<Self> {
        let joints = &record.figure.joints;
        let find = |name: &str| joints.iter().position(|j| j.name == name);
        let rest_world: Vec<Transform> = joints.iter().map(|j| pose(&j.pose)).collect();
        let rest: Vec<Transform> = (0..joints.len())
            .map(|i| match parents[i] {
                Some(p) => relative(&rest_world[p], &rest_world[i]),
                None => rest_world[i],
            })
            .collect();
        let depth = |mut i: usize| {
            let mut d = 0;
            while let Some(p) = parents[i] {
                i = p;
                d += 1;
            }
            d
        };
        let mut order: Vec<usize> = (0..joints.len()).collect();
        order.sort_by_key(|i| depth(*i));
        let ik = &record.ik;
        let b = &ik.bones;
        let limb = |upper: &str, mid: &str, end: &str, bend: Vec3| {
            let (upper, mid, end) = (find(upper)?, find(mid)?, find(end)?);
            // The solve sets each bone from its parent in the chain.
            (parents[mid] == Some(upper) && parents[end] == Some(mid)).then_some(Limb {
                upper,
                mid,
                end,
                bend,
                offset: None,
            })
        };
        let lh_parent = find(&ik.lh.parent)?;
        let rh_parent = find(&ik.rh.parent)?;
        let head = find(&b.head)?;
        let local = |parent: usize, goal: Goal| {
            let r = relative(
                &rest_world[parent],
                &Transform::from_translation(goal.position).with_rotation(goal.rotation),
            );
            Goal {
                position: r.translation,
                rotation: r.rotation,
            }
        };
        let def_lh = local(lh_parent, Goal::of(&ik.lh.pose));
        let def_rh = local(rh_parent, Goal::of(&ik.rh.pose));
        let c = &record.collider;
        // Source capsule: Y axis, `height` includes both caps. Converted space negates Z.
        let center = Vec3::new(c.center[0], c.center[1], -c.center[2]);
        let half = Vec3::Y * (c.height * 0.5 - c.radius).max(0.);
        let rest_head = rest_world[head].rotation;
        Some(Self {
            local: rest.clone(),
            world: rest_world.clone(),
            rest,
            parents: parents.to_vec(),
            order,
            neck: find(&b.neck)?,
            head,
            head_forward: rest_head.inverse() * Vec3::NEG_Z,
            limbs: [
                limb(
                    &b.left_upper_arm,
                    &b.left_lower_arm,
                    &b.left_hand,
                    ELBOW_LEFT,
                )?,
                limb(
                    &b.right_upper_arm,
                    &b.right_lower_arm,
                    &b.right_hand,
                    ELBOW_RIGHT,
                )?,
                limb(&b.left_upper_leg, &b.left_lower_leg, &b.left_foot, KNEE)?,
                limb(&b.right_upper_leg, &b.right_lower_leg, &b.right_foot, KNEE)?,
            ],
            llu: find(&ik.llu)?,
            rhu: find(&ik.rhu)?,
            lh_parent,
            rh_parent,
            b: Goal::of(&ik.b),
            h: Goal::of(&ik.h),
            ll: Goal::of(&ik.ll),
            rl: Goal::of(&ik.rl),
            lh: def_lh,
            rh: def_rh,
            def_b: Goal::of(&ik.b),
            def_ll: Goal::of(&ik.ll),
            def_lh,
            def_rh,
            body: Vec3::from_array(record.body.translation),
            clicked: false,
            collider: (center - half, center + half, c.radius),
            mouse_forward: ik.mouse_forward,
            mouse_forward_l: ik.mouse_forward_l,
            mouse_forward_h: ik.mouse_forward_h,
            max_lerp: ik.max_lerp,
            lerp_speed: ik.lerp_speed,
            rot_lerp_speed: ik.rot_lerp_speed,
            body_stance: ik.body_stance,
        })
    }

    /// Joint poses in figure space, in figure record order.
    pub fn poses(&self) -> &[Transform] {
        &self.world
    }

    fn forward_kinematics(&mut self) {
        let offset = self.b.position - self.body;
        for &i in &self.order {
            self.world[i] = match self.parents[i] {
                Some(p) => self.world[p].mul_transform(self.local[i]),
                None => {
                    let mut t = self.local[i];
                    t.translation += offset;
                    t
                }
            };
        }
    }

    fn set_world_rotation(&mut self, i: usize, rotation: Quat) {
        let parent = self.parents[i].map_or(Quat::IDENTITY, |p| self.world[p].rotation);
        self.local[i].rotation = parent.inverse() * rotation;
        self.world[i].rotation = rotation;
    }

    /// World goal of a joint-local goal.
    fn world_goal(&self, parent: usize, goal: Goal) -> Goal {
        let t = self.world[parent]
            .mul_transform(Transform::from_translation(goal.position).with_rotation(goal.rotation));
        Goal {
            position: t.translation,
            rotation: t.rotation,
        }
    }
    fn local_goal(&self, parent: usize, goal: Goal) -> Goal {
        let r = relative(
            &self.world[parent],
            &Transform::from_translation(goal.position).with_rotation(goal.rotation),
        );
        Goal {
            position: r.translation,
            rotation: r.rotation,
        }
    }

    /// `OnAnimatorIK`, then the humanoid IK solve. `dt` is `Time.deltaTime`.
    pub fn update(&mut self, dt: f32, mouse: MenuMouse, view: &MenuView) {
        let m_pos = view.point(mouse.position, self.mouse_forward);
        let m_pos_l = view.point(mouse.position, self.mouse_forward_l);
        let m_pos_h = view.point(mouse.position, self.mouse_forward_h);
        if mouse.pressed & (LEFT_MOUSE | RIGHT_MOUSE) != 0 && self.hit(view, mouse.position) {
            self.clicked = true;
        }
        let lerp = (dt * self.lerp_speed).clamp(0., 1.);
        let rot_lerp = (dt * self.rot_lerp_speed).clamp(0., 1.);
        let left = mouse.buttons & LEFT_MOUSE != 0 && self.clicked;
        let right = mouse.buttons & RIGHT_MOUSE != 0 && self.clicked;
        // `B` first: the hand goals are children of the pelvis, which follows it.
        if left && right {
            let forward = self.def_b.rotation * Vec3::NEG_Z;
            let v = m_pos + Vec3::NEG_Y * self.body_stance * 2.5 - self.def_b.position;
            let planar = v - forward * v.dot(forward) / forward.length_squared();
            let target = self.def_b.position + clamp_magnitude(planar, self.body_stance);
            self.b.position = self.b.position.lerp(target, lerp);
        } else if !left && !right {
            self.b.position = self.b.position.lerp(self.def_b.position, lerp);
        }
        self.local.copy_from_slice(&self.rest);
        self.forward_kinematics();
        let mut rh = self.world_goal(self.rh_parent, self.rh);
        let mut lh = self.world_goal(self.lh_parent, self.lh);
        if left && !right {
            self.h.position = move_towards(self.h.position, m_pos, dt * self.lerp_speed * 2.);
            rh.position = rh.position.lerp(
                rh.position + clamp_magnitude(m_pos_h - rh.position, self.max_lerp),
                lerp,
            );
            let up = -(self.world[self.rhu].rotation * Vec3::Y);
            rh.rotation = rh
                .rotation
                .slerp(look_rotation(m_pos_h - rh.position, up), rot_lerp);
        } else if right && !left {
            self.ll.position = self.ll.position.lerp(
                self.ll.position + clamp_magnitude(m_pos_l - self.ll.position, self.max_lerp),
                lerp,
            );
            let up = -(self.world[self.llu].rotation * Vec3::Y);
            self.ll.rotation = self
                .ll
                .rotation
                .slerp(look_rotation(m_pos_l - self.ll.position, up), rot_lerp);
        } else if !left && !right {
            self.clicked = false;
            self.h.position = move_towards(self.h.position, m_pos, dt * self.lerp_speed * 2.);
            self.ll.position = self.ll.position.lerp(
                self.ll.position
                    + clamp_magnitude(self.def_ll.position - self.ll.position, self.max_lerp),
                lerp,
            );
            let def_rh = self.world_goal(self.rh_parent, self.def_rh);
            rh.position = rh.position.lerp(
                rh.position + clamp_magnitude(def_rh.position - rh.position, self.max_lerp),
                lerp,
            );
            self.ll.rotation = self.ll.rotation.slerp(self.def_ll.rotation, rot_lerp);
            rh.rotation = rh.rotation.slerp(def_rh.rotation, rot_lerp);
        }
        let def_lh = self.world_goal(self.lh_parent, self.def_lh);
        lh.position = lh.position.lerp(def_lh.position, lerp);
        self.rh = self.local_goal(self.rh_parent, rh);
        self.lh = self.local_goal(self.lh_parent, lh);
        let goals = [lh, rh, self.ll, self.rl];
        for (i, goal) in goals.into_iter().enumerate() {
            self.solve_limb(i, goal);
        }
        self.look_at(self.h.position);
        self.forward_kinematics();
    }

    /// `Physics.Raycast` against the `Player` capsule.
    fn hit(&self, view: &MenuView, mouse: Vec2) -> bool {
        let origin = view.pose.translation;
        let dir = (view.point(mouse, 1.) - origin).normalize_or_zero();
        let (a, b, radius) = self.collider;
        segment_distance(origin, origin + dir * RAY_LENGTH, a, b) <= radius
    }

    /// Two-bone limb IK: the end reaches the goal position and takes the goal rotation.
    fn solve_limb(&mut self, index: usize, goal: Goal) {
        let Limb {
            upper,
            mid,
            end,
            bend,
            ..
        } = self.limbs[index];
        let a = self.world[upper].translation;
        let b = self.world[mid].translation;
        let c = self.world[end].translation;
        let (l1, l2) = (a.distance(b), b.distance(c));
        let to = goal.position - a;
        let Some(dir) = to.try_normalize() else {
            return;
        };
        let reach = to.length().clamp((l1 - l2).abs() + 1e-4, l1 + l2 - 1e-4);
        let side = (bend - dir * bend.dot(dir))
            .try_normalize()
            .unwrap_or_else(|| dir.any_orthonormal_vector());
        let cos = ((l1 * l1 + reach * reach - l2 * l2) / (2. * l1 * reach)).clamp(-1., 1.);
        let knee = a + (dir * cos + side * (1. - cos * cos).sqrt()) * l1;
        let swing = from_to(b - a, knee - a);
        self.set_world_rotation(upper, swing * self.world[upper].rotation);
        self.world[mid] = self.world[upper].mul_transform(self.local[mid]);
        let c = knee + swing * (c - b);
        let joint = self.world[mid].translation;
        let swing_mid = from_to(c - joint, a + dir * reach - joint);
        self.set_world_rotation(mid, swing_mid * self.world[mid].rotation);
        // The end bone keeps its swung rotation relative to the start goal rotation.
        let swung = self.world[mid].rotation * self.local[end].rotation;
        let offset = *self.limbs[index]
            .offset
            .get_or_insert(goal.rotation.inverse() * swung);
        self.set_world_rotation(end, goal.rotation * offset);
    }

    /// `SetLookAtPosition`: turn the neck and head toward `target`, half each.
    fn look_at(&mut self, target: Vec3) {
        let head = self.world[self.head];
        let current = head.rotation * self.head_forward;
        let Some(want) = (target - head.translation).try_normalize() else {
            return;
        };
        let (axis, angle) = from_to(current, want).to_axis_angle();
        let half = Quat::from_axis_angle(axis, angle.min(LOOK_CLAMP) * 0.5);
        let neck = self.world[self.neck].rotation;
        self.set_world_rotation(self.neck, half * neck);
        self.set_world_rotation(self.head, half * half * head.rotation);
    }
}
