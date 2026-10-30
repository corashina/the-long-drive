//! Source clip sampling into converted parent-local transform parts.
use crate::reference::{MotionClip, MotionTrack};
use crate::simulation::dials::unity_euler;
use rapier3d::math::{Rotation, Vector};

/// One sampled transform part of a node, in converted parent-local space.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum LocalPart {
    Position(Vector),
    Rotation(Rotation),
    Scale(Vector),
}

/// Clip time for `elapsed` seconds of a looping clip.
pub fn loop_time(clip: &MotionClip, elapsed: f32) -> f32 {
    clip.start_time + elapsed.rem_euclid(clip.stop_time - clip.start_time)
}

/// Samples each track at clip `time` and gives its node and converted part.
pub fn sample_clip(clip: &MotionClip, time: f32, mut out: impl FnMut(u32, LocalPart)) {
    for track in &clip.tracks {
        let [x, y, z, w] = sample_track(track, time);
        let part = match track.attribute {
            // Source parent-local values: z negates.
            1 => LocalPart::Position(Vector::new(x, y, -z)),
            2 => LocalPart::Rotation(Rotation::from_xyzw(-x, -y, z, w).normalize()),
            3 => LocalPart::Scale(Vector::new(x, y, z)),
            _ => {
                let [x, y, z, w] = unity_euler([x, y, z]);
                LocalPart::Rotation(Rotation::from_xyzw(x, y, z, w))
            }
        };
        out(track.node, part);
    }
}

/// Unity streamed cubic keys hold polynomial coefficients from each key time.
/// Components past the track width stay 0.
pub(crate) fn sample_track(track: &MotionTrack, time: f32) -> [f32; 4] {
    let times = &track.times;
    let i = times.partition_point(|t| *t <= time).saturating_sub(1);
    let mut out = [0.; 4];
    let values = &track.values[i];
    out[..values.len()].copy_from_slice(values);
    if time <= times[0] || i == times.len() - 1 {
        return out;
    }
    let Some(coefficients) = track.coefficients.get(i) else {
        return out;
    };
    let elapsed = time - times[i];
    for (c, [a, b, d]) in coefficients.iter().enumerate() {
        if !track.out_tangents[i][c].is_infinite() {
            out[c] = ((a * elapsed + b) * elapsed + d) * elapsed + values[c];
        }
    }
    out
}
