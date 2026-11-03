//! The scene scale setting as a Bevy resource. Auto steps the scale on the F3 GPU time.
use crate::{
    display_settings::{AUTO_SCALE, DisplaySettings},
    presentation::gpu_timing,
};
use bevy::{ecs as bevy_ecs, prelude::*};
use simulation::render_scale::AutoScale;
pub(crate) use simulation::render_scale::SceneScale;
use std::sync::atomic::{AtomicU32, Ordering};

#[derive(Resource, Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct SceneScaleSetting(pub SceneScale);

/// The scale percent that F3 shows.
static PERCENT: AtomicU32 = AtomicU32::new(100);

pub(crate) fn percent() -> u32 {
    PERCENT.load(Ordering::Relaxed)
}

#[derive(Default)]
pub(crate) struct AutoState {
    /// `None` while a fixed scale is set.
    auto: Option<AutoScale>,
    window: u32,
}

pub(crate) fn update(
    mut scale: ResMut<SceneScaleSetting>,
    mut state: Local<AutoState>,
    budget: Res<crate::app::FrameBudget>,
    display: Res<DisplaySettings>,
) {
    let value = if display.scene_scale == AUTO_SCALE {
        auto_scale(&mut state, budget.ms)
    } else {
        state.auto = None;
        SceneScale::from_percent(display.scene_scale).expect("DisplaySettings::parse checks it")
    };
    if scale.0 != value {
        scale.0 = value;
        PERCENT.store(value.percent(), Ordering::Relaxed);
    }
}

/// Without timestamp queries, Auto stays at 100%.
fn auto_scale(state: &mut AutoState, budget_ms: f32) -> SceneScale {
    let window = gpu_timing::windows();
    let auto = state.auto.get_or_insert_with(|| {
        // A window from before Auto does not count.
        state.window = window;
        AutoScale::default()
    });
    if window == state.window {
        return auto.scale();
    }
    state.window = window;
    match gpu_timing::stage_ms() {
        Some(stages) => auto.window(stages.iter().sum(), budget_ms),
        None => auto.scale(),
    }
}
