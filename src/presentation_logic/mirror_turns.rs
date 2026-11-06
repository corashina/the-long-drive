//! Mirror skip rules and draw turns. Live mirrors take turns to draw. Other slots keep their last image.

/// The maximum number of mirror slots that draw in one frame: 2 on High, 1 on Low.
pub(crate) fn draws_per_frame(mirrors: u32) -> usize {
    if mirrors == 0 { 2 } else { 1 }
}

/// Source `MirrorsUpdate` skip rules: out of draw range, facing away from the camera, or off screen.
/// `facing` is `dot(T.up, v)`, as in the source `Angle(T.up, v) < 90` test.
pub(crate) fn skipped(far: f32, near: f32, facing: f32, visible: bool) -> bool {
    // Near the max distance, far drops to the near plane.
    // Then that camera has no frustum and fails the shadow fit. Treat it as out of range.
    far <= near || facing <= 0. || !visible
}

/// Live slots to draw this frame, at most `budget`.
/// Fresh slots have no image of their mirror yet and draw first. The others take turns from `cursor`.
pub(crate) fn pick_draws<const N: usize>(
    live: [bool; N],
    fresh: [bool; N],
    mut budget: usize,
    cursor: &mut usize,
) -> [bool; N] {
    let mut draw = [false; N];
    let start = *cursor;
    for slot in 0..N {
        if budget > 0 && live[slot] && fresh[slot] {
            draw[slot] = true;
            budget -= 1;
        }
    }
    for step in 0..N {
        if budget == 0 {
            break;
        }
        let slot = (start + step) % N;
        if live[slot] && !draw[slot] {
            draw[slot] = true;
            budget -= 1;
            *cursor = (slot + 1) % N;
        }
    }
    draw
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn draws_take_turns_and_skip_rules_hold() {
        assert!(!skipped(10., 0.3, 1., true));
        assert!(skipped(0.3, 0.3, 1., true), "out of draw range");
        assert!(skipped(10., 0.3, 0., true), "faces away from the camera");
        assert!(skipped(10., 0.3, 1., false), "off screen");

        let live = [true, true, true, false, true];
        let none = [false; 5];
        let mut cursor = 0;
        assert_eq!(
            pick_draws(live, none, 2, &mut cursor),
            [true, true, false, false, false]
        );
        assert_eq!(
            pick_draws(live, none, 2, &mut cursor),
            [false, false, true, false, true]
        );
        assert_eq!(
            pick_draws(live, none, 2, &mut cursor),
            [true, true, false, false, false]
        );

        // A fresh slot draws first. The turn continues after it.
        let fresh = [false, false, false, false, true];
        assert_eq!(
            pick_draws(live, fresh, 2, &mut cursor),
            [false, false, true, false, true]
        );
        assert_eq!(
            pick_draws(live, none, 2, &mut cursor),
            [true, false, false, false, true]
        );
        // With no live slot, nothing draws.
        assert_eq!(pick_draws([false; 5], none, 2, &mut cursor), [false; 5]);

        // Low draws one slot per frame, in turns.
        let mut cursor = 0;
        assert_eq!(
            pick_draws(live, none, draws_per_frame(1), &mut cursor),
            [true, false, false, false, false]
        );
        assert_eq!(
            pick_draws(live, none, draws_per_frame(1), &mut cursor),
            [false, true, false, false, false]
        );
    }
}
