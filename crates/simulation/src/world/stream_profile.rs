//! Opt-in streaming CPU measurements. Disabled timers do not read the clock.
use std::sync::{
    Mutex,
    atomic::{AtomicBool, Ordering},
};

#[derive(Clone, Copy)]
#[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
pub enum Stage {
    Cleanup,
    FixedInputs,
    Pipelines,
    PrimaryTextures,
    DeferredTextures,
    Prototypes,
    Stream,
    MaterialRetirement,
    Descriptors,
    Bindings,
    Instances,
    Lods,
    Surface,
    Lamps,
    Publication,
    Supplies,
    SurfaceSync,
    InstanceSync,
    LampSync,
    Writers,
    Grass,
    Capture,
    FinalCleanup,
    StreamDescriptors,
    StreamGeometry,
    Terrain,
    Road,
    RoadPacking,
    RoadResult,
    RoadPublication,
}
const NAMES: [&str; 30] = [
    "Cleanup",
    "FixedInputs",
    "Pipelines",
    "PrimaryTextures",
    "DeferredTextures",
    "Prototypes",
    "Stream",
    "MaterialRetirement",
    "Descriptors",
    "Bindings",
    "Instances",
    "Lods",
    "Surface",
    "Lamps",
    "Publication",
    "Supplies",
    "SurfaceSync",
    "InstanceSync",
    "LampSync",
    "Writers",
    "Grass",
    "Capture",
    "FinalCleanup",
    "StreamDescriptors",
    "StreamGeometry",
    "Terrain",
    "Road",
    "RoadPacking",
    "RoadResult",
    "RoadPublication",
];
#[derive(Clone, Copy, Default, serde::Serialize)]
struct Measurement {
    calls: u64,
    total_ms: f64,
    max_ms: f64,
    over_2_ms: u64,
    // Work items, such as LOD groups evaluated.
    items: u64,
    max_items: u64,
}
static ENABLED: AtomicBool = AtomicBool::new(false);
const SAMPLE_CAPACITY: usize = 4096;
static SAMPLES: Mutex<[[f64; SAMPLE_CAPACITY]; 30]> = Mutex::new([[0.; SAMPLE_CAPACITY]; 30]);
static VALUES: Mutex<[Measurement; 30]> = Mutex::new(
    [Measurement {
        calls: 0,
        total_ms: 0.,
        max_ms: 0.,
        over_2_ms: 0,
        items: 0,
        max_items: 0,
    }; 30],
);

pub struct Timer {
    stage: Stage,
    #[cfg(target_arch = "wasm32")]
    start: Option<f64>,
    #[cfg(not(target_arch = "wasm32"))]
    start: Option<std::time::Instant>,
}
impl Timer {
    pub fn new(stage: Stage) -> Self {
        Self {
            stage,
            start: ENABLED.load(Ordering::Relaxed).then(now),
        }
    }
    pub fn next(&mut self, stage: Stage) {
        self.record();
        self.stage = stage;
        if self.start.is_some() {
            self.start = Some(now());
        }
    }
    fn record(&self) {
        let Some(start) = self.start else { return };
        #[cfg(target_arch = "wasm32")]
        let ms = now() - start;
        #[cfg(not(target_arch = "wasm32"))]
        let ms = start.elapsed().as_secs_f64() * 1000.;
        let mut values = VALUES.lock().unwrap();
        let value = &mut values[self.stage as usize];
        SAMPLES.lock().unwrap()[self.stage as usize][value.calls as usize % SAMPLE_CAPACITY] = ms;
        value.calls += 1;
        value.total_ms += ms;
        value.max_ms = value.max_ms.max(ms);
        value.over_2_ms += u64::from(ms > 2.);
    }
}
/// Adds work items of one stage call.
#[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
pub fn count(stage: Stage, items: usize) {
    if !ENABLED.load(Ordering::Relaxed) {
        return;
    }
    let mut values = VALUES.lock().unwrap();
    let value = &mut values[stage as usize];
    value.items += items as u64;
    value.max_items = value.max_items.max(items as u64);
}
impl Drop for Timer {
    fn drop(&mut self) {
        self.record();
    }
}
#[cfg(target_arch = "wasm32")]
fn now() -> f64 {
    crate::host::game_now()
}
#[cfg(not(target_arch = "wasm32"))]
fn now() -> std::time::Instant {
    std::time::Instant::now()
}

#[cfg_attr(target_arch = "wasm32", wasm_bindgen::prelude::wasm_bindgen)]
pub fn streaming_profile(enabled: bool) {
    ENABLED.store(false, Ordering::Relaxed);
    *VALUES.lock().unwrap() = [Measurement::default(); 30];
    if enabled {
        for samples in SAMPLES.lock().unwrap().iter_mut() {
            samples.fill(0.);
        }
    }
    ENABLED.store(enabled, Ordering::Relaxed);
}
#[cfg_attr(target_arch = "wasm32", wasm_bindgen::prelude::wasm_bindgen)]
pub fn streaming_profile_audit() -> String {
    let values = VALUES.lock().unwrap();
    let samples = SAMPLES.lock().unwrap();
    let measurements: Vec<_> = values.iter().enumerate().map(|(index, row)| {
        let count = (row.calls as usize).min(SAMPLE_CAPACITY);
        let mut sorted = samples[index][..count].to_vec();
        sorted.sort_by(f64::total_cmp);
        let percentile = |p: f64| sorted.get((count as f64 * p).ceil() as usize - usize::from(count > 0)).copied();
        serde_json::json!({"calls": row.calls, "total_ms": row.total_ms, "max_ms": row.max_ms,
            "over_2_ms": row.over_2_ms, "items": row.items, "max_items": row.max_items, "mean_ms": (row.calls > 0).then(|| row.total_ms / row.calls as f64),
            "p95_ms": percentile(0.95), "p99_ms": percentile(0.99), "sample_count": count,
            "dropped": row.calls.saturating_sub(SAMPLE_CAPACITY as u64)})
    }).collect();
    serde_json::to_string(&serde_json::json!({
        "enabled": ENABLED.load(Ordering::Relaxed),
        "stages": NAMES,
        "measurements": measurements,
        "retainedBytes": std::mem::size_of_val(&*samples) + std::mem::size_of_val(&*values),
        "nested_stages": ["StreamDescriptors", "StreamGeometry", "Terrain", "Road", "RoadPacking", "RoadResult", "RoadPublication"],
    }))
    .expect("streaming measurements serialize")
}
