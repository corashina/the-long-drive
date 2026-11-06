//! Opt-in CPU spans. Disabled timers do not read clocks or allocate.
use std::sync::{
    Mutex,
    atomic::{AtomicBool, Ordering},
};

const CAPACITY: usize = 4096;
#[derive(Clone, Copy)]
pub enum Stage {
    Startup,
    AssetDecode,
    SavePrepare,
    SaveCommit,
    LoadPrepare,
    Physics,
    RenderPreparation,
    Input,
    TerrainInputPreparation,
    TerrainResultAllocation,
    /// `First` to `RunFixedMainLoop`.
    MainPreUpdate,
    /// `Update` and `SpawnScene`.
    MainUpdate,
    /// `PostUpdate` to the end of visibility: transforms, bounds, frusta, visibility.
    MainVisibility,
    /// The rest of `PostUpdate`, and `Last`.
    MainPostUpdate,
    Extract,
    Specialize,
    Queue,
    Prepare,
    Encode,
    Submit,
    Cleanup,
}
const COUNT: usize = 21;
const NAMES: [&str; COUNT] = [
    "Startup",
    "AssetDecode",
    "SavePrepare",
    "SaveCommit",
    "LoadPrepare",
    "Physics",
    "RenderPreparation",
    "Input",
    "TerrainInputPreparation",
    "TerrainResultAllocation",
    "MainPreUpdate",
    "MainUpdate",
    "MainVisibility",
    "MainPostUpdate",
    "Extract",
    "Specialize",
    "Queue",
    "Prepare",
    "Encode",
    "Submit",
    "Cleanup",
];
#[derive(Clone, Copy)]
struct Measurement {
    samples: [f64; CAPACITY],
    calls: usize,
    total_ms: f64,
    max_ms: f64,
}
const EMPTY: Measurement = Measurement {
    samples: [0.; CAPACITY],
    calls: 0,
    total_ms: 0.,
    max_ms: 0.,
};
static ENABLED: AtomicBool = AtomicBool::new(false);
static VALUES: Mutex<[Measurement; COUNT]> = Mutex::new([EMPTY; COUNT]);

pub fn enabled() -> bool {
    ENABLED.load(Ordering::Relaxed)
}
pub struct Timer {
    stage: Stage,
    start: Option<f64>,
}
impl Timer {
    pub fn new(stage: Stage) -> Self {
        Self {
            stage,
            start: enabled().then(crate::host::game_now),
        }
    }
}
impl Drop for Timer {
    fn drop(&mut self) {
        let Some(start) = self.start else { return };
        record(self.stage, crate::host::game_now() - start);
    }
}

/// Adds one span of `duration` milliseconds to `stage`. Callers check `enabled` first.
pub fn record(stage: Stage, duration: f64) {
    let mut values = VALUES.lock().unwrap();
    let row = &mut values[stage as usize];
    let index = row.calls % CAPACITY;
    row.samples[index] = duration;
    row.calls += 1;
    row.total_ms += duration;
    row.max_ms = row.max_ms.max(duration);
}

#[wasm_bindgen::prelude::wasm_bindgen]
pub fn cpu_profile(value: bool) {
    ENABLED.store(false, Ordering::Relaxed);
    if value {
        for row in VALUES.lock().unwrap().iter_mut() {
            row.samples.fill(0.);
            row.calls = 0;
            row.total_ms = 0.;
            row.max_ms = 0.;
        }
    }
    crate::world::stream_profile::streaming_profile(value);
    ENABLED.store(value, Ordering::Relaxed);
}

#[wasm_bindgen::prelude::wasm_bindgen]
pub fn cpu_profile_audit() -> String {
    let values = VALUES.lock().unwrap();
    let measurements: Vec<_> = values
        .iter()
        .map(|row| {
            let mut samples = row.samples[..row.calls.min(CAPACITY)].to_vec();
            samples.sort_by(f64::total_cmp);
            let percentile = |p: f64| {
                samples
                    .get(
                        (samples.len() as f64 * p).ceil() as usize
                            - usize::from(!samples.is_empty()),
                    )
                    .copied()
            };
            serde_json::json!({
                "calls": row.calls, "total_ms": row.total_ms, "max_ms": row.max_ms,
                "mean_ms": (row.calls > 0).then(|| row.total_ms / row.calls as f64),
                "p95_ms": percentile(0.95), "p99_ms": percentile(0.99),
                "sample_count": samples.len(), "dropped": row.calls.saturating_sub(CAPACITY),
            })
        })
        .collect();
    let streaming: serde_json::Value =
        serde_json::from_str(&crate::world::stream_profile::streaming_profile_audit())
            .expect("streaming profile serializes");
    serde_json::json!({"enabled": enabled(), "stages": NAMES, "measurements": measurements,
        "nested_stages": ["AssetDecode", "TerrainInputPreparation", "TerrainResultAllocation"],
        "stage_totals_are_exclusive": false, "streaming": streaming,
        "retainedBytes": std::mem::size_of_val(&*values)})
    .to_string()
}
