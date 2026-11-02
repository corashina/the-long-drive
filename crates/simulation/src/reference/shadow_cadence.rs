//! World view cascade cadence. Far cascades render less often and keep their last fit.
use super::shadows::ShadowCascade;

/// A change of these values renders all cascades.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CascadeEpoch {
    pub session: u64,
    pub scene: u32,
    pub origin: [u64; 3],
    /// Depth bias, normal bias, strength and split fractions, as bits.
    pub settings: [u32; 6],
}
pub struct CascadeInputs {
    pub epoch: CascadeEpoch,
    pub cascades: [ShadowCascade; 4],
    /// Held shadow sun rotation, as bits.
    pub sun: [u32; 4],
    /// Caster content counter of each cascade. It changes when the tile content would change.
    pub contents: [u64; 4],
    /// The tile holds a grass or skinned caster. These change each frame.
    pub animated: [bool; 4],
    /// The atlas is new, or its last planned frame was not encoded.
    pub forced: bool,
}
/// The fit of each cascade at its last render. The atlas tile holds this projection.
#[derive(Clone, Copy, Debug, Default)]
pub struct CascadeHistory {
    pub epoch: Option<CascadeEpoch>,
    pub cascades: [ShadowCascade; 4],
    pub suns: [[u32; 4]; 4],
    /// Caster content counter of each tile at its last draw.
    pub contents: [u64; 4],
    /// The tile holds animated casters since its last refresh.
    pub animated: [bool; 4],
}
impl CascadeHistory {
    /// Keep the current fit for each rendered cascade. Returns true when a value changed.
    pub fn commit(&mut self, plan: &CascadePlan, current: &CascadeInputs) -> bool {
        let mut changed = self.epoch != Some(current.epoch);
        self.epoch = Some(current.epoch);
        for (index, cascade) in current.cascades.iter().enumerate() {
            if plan.render[index] {
                changed |= !same(&self.cascades[index], cascade) || self.suns[index] != current.sun;
                self.cascades[index] = *cascade;
                self.suns[index] = current.sun;
            }
            if plan.draw[index] {
                self.contents[index] = current.contents[index];
            }
            if plan.refresh[index] {
                self.animated[index] = current.animated[index];
            }
        }
        changed
    }
}
/// `render`: the cascade takes the current fit. Else it keeps its last fit and tile.
/// `draw`: the static casters draw into the static tile. Else it has the same projection and content.
/// `refresh`: the atlas tile copies the static tile, then the animated casters draw.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CascadePlan {
    pub render: [bool; 4],
    pub draw: [bool; 4],
    pub refresh: [bool; 4],
}
impl CascadePlan {
    pub const ALL: Self = Self {
        render: [true; 4],
        draw: [true; 4],
        refresh: [true; 4],
    };
}
fn same(a: &ShadowCascade, b: &ShadowCascade) -> bool {
    let words = |c: &ShadowCascade| {
        c.sphere_center
            .into_iter()
            .chain([c.sphere_radius])
            .chain(c.snapped_center)
            .chain(c.light_position)
            .chain(c.width_height)
            .chain(c.texel_size)
            .chain([c.near, c.far, c.min_view_distance, c.max_view_distance])
            .map(f32::to_bits)
    };
    words(a).eq(words(b))
}
/// Values that set the tile depth: the projection and the normal bias texel size.
fn same_projection(a: &ShadowCascade, b: &ShadowCascade) -> bool {
    let words = |c: &ShadowCascade| {
        c.light_position
            .into_iter()
            .chain(c.width_height)
            .chain(c.texel_size)
            .chain([c.near, c.far])
            .map(f32::to_bits)
    };
    words(a).eq(words(b))
}
/// Cascades 0 and 1 render each frame, cascade 2 on even frames, cascade 3 when frame % 4 == 1.
/// All render after an epoch change, a forced refresh, or a cascade 2 or 3 sphere move above 25% of its radius.
/// A rendered tile does not draw when its projection, sun and static content did not change.
/// A rendered tile refreshes when it draws, or when it holds animated casters now or since its last refresh.
pub fn plan_cascades(frame: u32, last: &CascadeHistory, current: &CascadeInputs) -> CascadePlan {
    let render = cascades_to_render(frame, last, current);
    if current.forced || last.epoch != Some(current.epoch) {
        return CascadePlan::ALL;
    }
    let draw = std::array::from_fn(|i| {
        render[i]
            && (last.suns[i] != current.sun
                || last.contents[i] != current.contents[i]
                || !same_projection(&last.cascades[i], &current.cascades[i]))
    });
    let refresh =
        std::array::from_fn(|i| draw[i] || render[i] && (current.animated[i] || last.animated[i]));
    CascadePlan {
        render,
        draw,
        refresh,
    }
}
fn cascades_to_render(frame: u32, last: &CascadeHistory, current: &CascadeInputs) -> [bool; 4] {
    // Cascades 0 and 1 render each frame, so only the skipped cascades can hold a stale fit.
    let moved = last.cascades[2..]
        .iter()
        .zip(&current.cascades[2..])
        .any(|(old, new)| {
            let distance = (0..3)
                .map(|i| (new.sphere_center[i] - old.sphere_center[i]).powi(2))
                .sum::<f32>();
            distance > (0.25 * old.sphere_radius).powi(2)
        });
    if current.forced || last.epoch != Some(current.epoch) || moved {
        return [true; 4];
    }
    [true, true, frame.is_multiple_of(2), frame % 4 == 1]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn inputs(contents: [u64; 4]) -> CascadeInputs {
        CascadeInputs {
            epoch: CascadeEpoch {
                session: 1,
                scene: 1,
                origin: [0; 3],
                settings: [0; 6],
            },
            cascades: [ShadowCascade {
                sphere_radius: 10.,
                width_height: [20.; 2],
                texel_size: [0.01; 2],
                far: 20.,
                ..ShadowCascade::default()
            }; 4],
            sun: [0, 0, 0, 1f32.to_bits()],
            contents,
            animated: [false; 4],
            forced: false,
        }
    }

    #[test]
    fn unchanged_tiles_skip_the_draw() {
        let mut history = CascadeHistory::default();
        let first = inputs([0; 4]);
        let plan = plan_cascades(0, &history, &first);
        assert_eq!(plan, CascadePlan::ALL);
        history.commit(&plan, &first);

        // Same fit and content: rendered cascades keep the tile. A sphere move does not draw.
        let mut same = inputs([0; 4]);
        same.cascades[0].sphere_center = [0.5, 0., 0.];
        let plan = plan_cascades(1, &history, &same);
        assert_eq!(plan.render, [true, true, false, true]);
        assert_eq!(plan.draw, [false; 4]);
        assert_eq!(plan.refresh, [false; 4]);
        history.commit(&plan, &same);

        // An animated caster refreshes its tile without a static draw, also once after it leaves.
        let mut animated = inputs([0; 4]);
        animated.animated[0] = true;
        let plan = plan_cascades(3, &history, &animated);
        assert_eq!(plan.draw, [false; 4]);
        assert_eq!(plan.refresh, [true, false, false, false]);
        history.commit(&plan, &animated);
        let plan = plan_cascades(5, &history, &same);
        assert_eq!(plan.refresh, [true, false, false, false]);
        history.commit(&plan, &same);
        assert_eq!(plan_cascades(5, &history, &same).refresh, [false; 4]);

        // A content change or a projection change draws only that cascade.
        let mut next = inputs([0, 1, 0, 0]);
        next.cascades[0].light_position = [0.1, 0., 0.];
        let plan = plan_cascades(2, &history, &next);
        assert_eq!(plan.draw, [true, true, false, false]);

        // A content change of a stale cascade waits for its render, then draws.
        let next = inputs([0, 0, 0, 1]);
        assert_eq!(plan_cascades(2, &history, &next).draw, [false; 4]);
        assert_eq!(
            plan_cascades(1, &history, &next).draw,
            [false, false, false, true]
        );

        // A sun step or a forced refresh draws.
        let mut sun = inputs([0; 4]);
        sun.sun[0] = 1;
        assert_eq!(
            plan_cascades(2, &history, &sun).draw,
            [true, true, true, false]
        );
        let mut forced = inputs([0; 4]);
        forced.forced = true;
        assert_eq!(plan_cascades(2, &history, &forced), CascadePlan::ALL);
    }
}
