//! Source `IKscript`. `new` is `Start`, `solve` is `LateUpdate`.
use rapier3d::math::{Rotation, Vector};

/// The longest source chain has 5 bones plus the end bone.
pub const MAX_BONES: usize = 6;

/// Position, rotation and lossy scale. All scales in the rig are uniform.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Frame {
    pub position: Vector,
    pub rotation: Rotation,
    pub scale: Vector,
}
impl Frame {
    pub const IDENTITY: Self = Self {
        position: Vector::ZERO,
        rotation: Rotation::IDENTITY,
        scale: Vector::ONE,
    };
    /// Child frame from its local frame.
    pub fn mul(&self, local: &Self) -> Self {
        Self {
            position: self.position + self.rotation * (self.scale * local.position),
            rotation: self.rotation * local.rotation,
            scale: self.scale * local.scale,
        }
    }
    /// Unity `InverseTransformPoint`.
    pub fn local_point(&self, point: Vector) -> Vector {
        self.rotation.inverse() * (point - self.position) / self.scale
    }
    /// Local frame of `world` under this frame.
    pub fn inverse_mul(&self, world: &Self) -> Self {
        Self {
            position: self.local_point(world.position),
            rotation: self.rotation.inverse() * world.rotation,
            scale: world.scale / self.scale,
        }
    }
}

/// Unity `Quaternion.FromToRotation`.
fn from_to(from: Vector, to: Vector) -> Rotation {
    let (from, to) = (from.normalize_or_zero(), to.normalize_or_zero());
    if from == Vector::ZERO || to == Vector::ZERO {
        return Rotation::IDENTITY;
    }
    Rotation::from_rotation_arc(from, to)
}

/// `Start` data. Bones are re-parented to the owner, so they are siblings.
#[derive(Clone, Debug)]
pub struct IkChain {
    /// `chainLength`. `bones` has one more entry: the end bone.
    count: usize,
    lengths: [f32; MAX_BONES - 1],
    sum_length: f32,
    start_dirs: [Vector; MAX_BONES - 1],
    /// `startRots[i] * Quaternion.Euler(offsets)`.
    start_rots: [Rotation; MAX_BONES - 1],
    end_bone_offset: Rotation,
    force_end: bool,
    stretch: bool,
}
impl IkChain {
    /// `start` holds the bones in world space at `Start`, owner first child to end bone.
    pub fn new(
        start: &[Frame],
        target: &Frame,
        offsets: Rotation,
        force_end: bool,
        stretch: bool,
    ) -> Self {
        assert!((2..=MAX_BONES).contains(&start.len()), "IK chain length");
        let count = start.len() - 1;
        let mut chain = Self {
            count,
            lengths: [0.; MAX_BONES - 1],
            sum_length: 0.,
            start_dirs: [Vector::ZERO; MAX_BONES - 1],
            start_rots: [Rotation::IDENTITY; MAX_BONES - 1],
            end_bone_offset: target.rotation * start[count].rotation.inverse(),
            force_end,
            stretch,
        };
        for i in 0..count {
            chain.lengths[i] = (start[i].position - start[i + 1].position).length();
            chain.sum_length += chain.lengths[i];
            chain.start_dirs[i] = start[i + 1].position - start[i].position;
            chain.start_rots[i] = start[i].rotation * offsets;
        }
        chain
    }
    pub fn bone_count(&self) -> usize {
        self.count + 1
    }
    /// `bones` are owner-local frames. `pole` runs first, as in `LateUpdate`.
    pub fn solve(&self, owner: &Frame, target: &Frame, pole: Option<&Frame>, bones: &mut [Frame]) {
        let mut p = [Vector::ZERO; MAX_BONES];
        for (p, bone) in p.iter_mut().zip(&*bones) {
            *p = owner.mul(bone).position;
        }
        if let Some(pole) = pole {
            self.do_stuff(owner, pole, target.position, &mut p, bones);
        }
        self.do_stuff(owner, target, target.position, &mut p, bones);
    }
    /// `DoStuff(_target)`. `target_position` is the `target` field, which stretch reads.
    fn do_stuff(
        &self,
        owner: &Frame,
        goal: &Frame,
        target_position: Vector,
        p: &mut [Vector; MAX_BONES],
        bones: &mut [Frame],
    ) {
        let n = self.count;
        let len = &self.lengths;
        p[n] = goal.position;
        for i in (1..n).rev() {
            p[i] = p[i + 1] + (p[i] - p[i + 1]).normalize_or_zero() * len[i];
        }
        for i in 1..=n {
            p[i] = p[i - 1] + (p[i] - p[i - 1]).normalize_or_zero() * len[i - 1];
        }
        p[n] = goal.position;
        for i in (2..n).rev() {
            p[i] = p[i + 1] + (p[i] - p[i + 1]).normalize_or_zero() * len[i];
        }
        if self.stretch
            && (target_position - owner.position).length_squared()
                > self.sum_length * self.sum_length
        {
            for i in 1..=n {
                let t = (len[i - 1] / self.sum_length).clamp(0., 1.);
                p[i] = p[i].lerp(target_position, t);
            }
        }
        if self.force_end {
            p[n] = goal.position;
        }
        for (bone, p) in bones.iter_mut().zip(&*p) {
            bone.position = owner.local_point(*p);
        }
        bones[n].rotation = owner.rotation.inverse() * goal.rotation * self.end_bone_offset;
        for i in (0..n).rev() {
            bones[i].rotation = from_to(
                self.start_dirs[i],
                bones[i + 1].position - bones[i].position,
            ) * self.start_rots[i];
        }
    }
}
