//! Source tank cap marker motion.
use bevy::prelude::Transform;

/// Source `tankcapscript.DoMove`: each frame moves `TCap` by `Lerp(current, target, capLerp)`.
/// `capLerp` starts at 0 and gains `deltaTime * 10`; at 1 the cap snaps to its target.
pub fn cap_motion(
    current: Transform,
    target: Transform,
    motion: &mut Option<f32>,
    dt: f32,
) -> Transform {
    let Some(lerp) = motion else {
        if current.translation != target.translation || current.rotation != target.rotation {
            *motion = Some(0.0);
        }
        return current;
    };
    *lerp += dt * 10.0;
    if *lerp >= 1.0 {
        *motion = None;
        return Transform {
            scale: current.scale,
            ..target
        };
    }
    Transform {
        translation: current.translation.lerp(target.translation, *lerp),
        rotation: current.rotation.lerp(target.rotation, *lerp).normalize(),
        scale: current.scale,
    }
}
