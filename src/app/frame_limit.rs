//! Source `Application.targetFrameRate`. With a limit, a frame runs only when `web/frame-pacer.js` calls `wake_frame`.
//! The pacer wakes in step with animation frames. Timed updates run off the display refresh and lose frames.
use crate::browser::game_frame_limit;
use bevy::winit::{EventLoopProxyWrapper, UpdateMode, WinitSettings, WinitUserEvent};
use bevy::{ecs as bevy_ecs, prelude::*};
use std::cell::OnceCell;
use wasm_bindgen::prelude::*;
use winit::event_loop::EventLoopProxy;

thread_local! {
    static PROXY: OnceCell<EventLoopProxy<WinitUserEvent>> = const { OnceCell::new() };
}

pub(super) fn keep_proxy(proxy: Res<EventLoopProxyWrapper>) {
    PROXY.with(|cell| {
        let _ = cell.set((**proxy).clone());
    });
}

/// The pacer calls this when a limited frame is due. Without a limit, the wake has no effect.
#[wasm_bindgen]
pub fn wake_frame() {
    PROXY.with(|cell| {
        if let Some(proxy) = cell.get() {
            let _ = proxy.send_event(WinitUserEvent::WakeUp);
        }
    });
}

/// The frame time target of Auto scene scale. Without an FPS limit, the target is 60 FPS.
#[derive(Resource)]
pub(crate) struct FrameBudget {
    pub ms: f32,
}
impl Default for FrameBudget {
    fn default() -> Self {
        Self { ms: 1000. / 60. }
    }
}

pub(super) fn update(mut winit: ResMut<WinitSettings>, mut budget: ResMut<FrameBudget>) {
    let Some(limit) = game_frame_limit() else {
        return;
    };
    *budget = if limit > 0. {
        FrameBudget { ms: 1000. / limit }
    } else {
        FrameBudget::default()
    };
    let mode = if limit > 0. {
        UpdateMode::Reactive {
            wait: std::time::Duration::MAX,
            react_to_device_events: false,
            react_to_user_events: true,
            react_to_window_events: false,
        }
    } else {
        UpdateMode::Continuous
    };
    winit.focused_mode = mode;
    winit.unfocused_mode = mode;
}
