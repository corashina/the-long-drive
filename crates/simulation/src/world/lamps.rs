//! Checked source pole tilt, shared by source road placement.
use super::SourceRandom;
use rapier3d::math::{Rotation, Vector};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RoadLampId(pub i64);

pub fn flatten_then_tilt(incoming: Rotation, angles: [i32; 3]) -> Rotation {
    let forward = incoming * Vector::NEG_Z;
    let yaw = (-forward.x).atan2(-forward.z);
    let [x, y, z] = angles.map(|angle| (angle as f32).to_radians());
    // Converted Unity Euler applies Z, X, then Y. FStart runs before this local tilt.
    Rotation::from_rotation_y(yaw)
        * Rotation::from_rotation_y(-y)
        * Rotation::from_rotation_x(-x)
        * Rotation::from_rotation_z(z)
}

pub fn source_angles(
    seed: u64,
    id: RoadLampId,
    chances: [f32; 2],
    angles: [[i32; 3]; 2],
) -> [i32; 3] {
    // Source seed and segment index are signed integers. Preserve their low bits and wrapping sum.
    let mut random = SourceRandom::new((seed as i32).wrapping_add(id.0 as i32));
    let [chance1, chance2] = chances;
    let chance = random.sample() as f32 * (chance1 + chance2);
    let angles = if chance < chance1 {
        angles[0]
    } else if chance - chance1 < chance2 {
        angles[1]
    } else {
        return [0; 3];
    };
    angles.map(|limit| (random.sample() * (limit * 2) as f64) as i32 - limit)
}
