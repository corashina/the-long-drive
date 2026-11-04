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
/// Cascade 0 renders each frame, cascade 1 on odd frames, cascade 2 when frame % 4 == 0, cascade 3 when frame % 4 == 2.
/// So each frame renders cascade 0 and one other cascade.
/// All render after an epoch change, a forced refresh, or a cascade 1, 2 or 3 sphere move above 25% of its radius.
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
    // Cascade 0 renders each frame, so only the skipped cascades can hold a stale fit.
    let moved = last.cascades[1..]
        .iter()
        .zip(&current.cascades[1..])
        .any(|(old, new)| {
            let distance = (0..3)
                .map(|i| (new.sphere_center[i] - old.sphere_center[i]).powi(2))
                .sum::<f32>();
            distance > (0.25 * old.sphere_radius).powi(2)
        });
    if current.forced || last.epoch != Some(current.epoch) || moved {
        return [true; 4];
    }
    [true, frame % 2 == 1, frame % 4 == 0, frame % 4 == 2]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_frame_renders_cascade_0_and_one_other() {
        let epoch = CascadeEpoch {
            session: 1,
            scene: 1,
            origin: [0; 3],
            settings: [0; 6],
        };
        let cascades = std::array::from_fn(|_| ShadowCascade {
            sphere_radius: 10.,
            ..Default::default()
        });
        let current = CascadeInputs {
            epoch,
            cascades,
            sun: [0; 4],
            contents: [0; 4],
            animated: [false; 4],
            forced: false,
        };
        let mut history = CascadeHistory::default();
        history.commit(&CascadePlan::ALL, &current);
        let mut renders = [0; 4];
        for frame in 0..8 {
            let plan = plan_cascades(frame, &history, &current);
            assert!(plan.render[0]);
            assert_eq!(plan.render.iter().filter(|&&r| r).count(), 2);
            for (count, render) in renders.iter_mut().zip(plan.render) {
                *count += u32::from(render);
            }
        }
        assert_eq!(renders, [8, 4, 2, 2]);
        let mut moved = current.cascades;
        moved[1].sphere_center[0] = 3.;
        let current = CascadeInputs {
            cascades: moved,
            ..current
        };
        assert_eq!(plan_cascades(0, &history, &current).render, [true; 4]);
    }
}
