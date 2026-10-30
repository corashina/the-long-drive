//! F3 GPU time of the world view stages, from the WebGPU `timestamp-query` feature.
//! An empty compute pass writes one timestamp at each stage edge. Probe faces are not timed.
//! Without the feature, F3 shows no GPU line.
use super::{
    surface_graph::{SourceSurface, SourceSurfaceStage},
    surface_views::{SourceSurfaceView, WORLD_VIEWS},
};
use bevy::render::{
    Render, RenderApp, RenderStartup, RenderSystems,
    renderer::{RenderContext, RenderDevice, RenderQueue, ViewQuery},
};
use bevy::{ecs as bevy_ecs, prelude::*};
use std::sync::{
    Arc,
    atomic::{AtomicU8, AtomicU32, Ordering},
};

/// Shadow atlas, opaque, after opaque (depth resolve, AO), transparent, postprocessing.
pub const STAGES: usize = 5;
const MARKS: u32 = STAGES as u32 + 1;
const ALL_MARKS: u8 = (1 << MARKS) - 1;
const BYTES: u64 = MARKS as u64 * wgpu::QUERY_SIZE as u64;
/// Frames in one F3 average.
const WINDOW: u32 = 30;
const IDLE: u8 = 0;
const COPIED: u8 = 1;
const MAPPING: u8 = 2;
const MAPPED: u8 = 3;

/// Average milliseconds of each stage. NaN bits until the first window completes.
static STAGE_MS: [AtomicU32; STAGES] = [const { AtomicU32::new(u32::MAX) }; STAGES];

/// The last F3 average of each stage, or `None` without timestamps.
pub fn stage_ms() -> Option<[f32; STAGES]> {
    let values = STAGE_MS
        .each_ref()
        .map(|value| f32::from_bits(value.load(Ordering::Relaxed)));
    values
        .iter()
        .all(|value| value.is_finite())
        .then_some(values)
}

#[derive(Resource)]
struct GpuTiming {
    queries: wgpu::QuerySet,
    resolve: wgpu::Buffer,
    readback: wgpu::Buffer,
    /// Marks written in this frame.
    written: AtomicU8,
    state: Arc<AtomicU8>,
    period_ns: f32,
    sums: [f64; STAGES],
    frames: u32,
}

pub fn install(app: &mut App) {
    use SourceSurfaceStage::*;
    app.sub_app_mut(RenderApp)
        .add_systems(RenderStartup, init)
        .add_systems(
            Render,
            (
                read.in_set(RenderSystems::PrepareResources),
                map.in_set(RenderSystems::Cleanup),
            ),
        )
        .add_systems(
            SourceSurface,
            (
                mark::<0>
                    .in_set(ShadowAtlas)
                    .before(super::shadow_atlas::render_atlas),
                mark::<1>.after(Collector).before(Opaque),
                mark::<2>.after(Opaque).before(AfterOpaque),
                mark::<3>.after(AfterOpaque).before(Transparent),
                mark::<4>.after(Transparent).before(Postprocessing),
                mark::<5>.after(AfterTransparent),
            ),
        );
}

fn init(mut commands: Commands, device: Res<RenderDevice>, queue: Res<RenderQueue>) {
    if !device.features().contains(wgpu::Features::TIMESTAMP_QUERY) {
        return;
    }
    let device = device.wgpu_device();
    let buffer = |label, usage| {
        device.create_buffer(&wgpu::BufferDescriptor {
            label: Some(label),
            size: BYTES,
            usage,
            mapped_at_creation: false,
        })
    };
    commands.insert_resource(GpuTiming {
        queries: device.create_query_set(&wgpu::QuerySetDescriptor {
            label: Some("source_gpu_timing"),
            ty: wgpu::QueryType::Timestamp,
            count: MARKS,
        }),
        resolve: buffer(
            "source_gpu_timing_resolve",
            wgpu::BufferUsages::QUERY_RESOLVE | wgpu::BufferUsages::COPY_SRC,
        ),
        readback: buffer(
            "source_gpu_timing_readback",
            wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
        ),
        written: AtomicU8::new(0),
        state: Arc::new(AtomicU8::new(IDLE)),
        period_ns: queue.get_timestamp_period(),
        sums: [0.; STAGES],
        frames: 0,
    });
}

/// Writes timestamp `N` of the world view. The last mark copies the frame to the readback.
fn mark<const N: u32>(
    view: ViewQuery<&SourceSurfaceView>,
    timing: Option<Res<GpuTiming>>,
    mut context: RenderContext,
) {
    let Some(timing) = timing else { return };
    if view.into_inner().index >= WORLD_VIEWS {
        return;
    }
    let encoder = context.command_encoder();
    drop(encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
        label: Some("source_gpu_timing_mark"),
        timestamp_writes: Some(wgpu::ComputePassTimestampWrites {
            query_set: &timing.queries,
            beginning_of_pass_write_index: Some(N),
            end_of_pass_write_index: None,
        }),
    }));
    let written = timing.written.fetch_or(1 << N, Ordering::Relaxed) | 1 << N;
    if N + 1 < MARKS {
        return;
    }
    timing.written.store(0, Ordering::Relaxed);
    // A frame that skipped a stage leaves an old timestamp in its slot.
    if written != ALL_MARKS || timing.state.load(Ordering::Acquire) != IDLE {
        return;
    }
    encoder.resolve_query_set(&timing.queries, 0..MARKS, &timing.resolve, 0);
    encoder.copy_buffer_to_buffer(&timing.resolve, 0, &timing.readback, 0, BYTES);
    timing.state.store(COPIED, Ordering::Release);
}

/// Maps the copied timestamps after submission.
fn map(timing: Option<Res<GpuTiming>>) {
    let Some(timing) = timing else { return };
    if timing.state.load(Ordering::Acquire) != COPIED {
        return;
    }
    timing.state.store(MAPPING, Ordering::Release);
    let state = timing.state.clone();
    timing
        .readback
        .slice(..)
        .map_async(wgpu::MapMode::Read, move |result| {
            state.store(
                if result.is_ok() { MAPPED } else { IDLE },
                Ordering::Release,
            );
        });
}

/// Adds a mapped frame to the window. A full window publishes its averages.
fn read(timing: Option<ResMut<GpuTiming>>) {
    let Some(mut timing) = timing else { return };
    if timing.state.load(Ordering::Acquire) != MAPPED {
        return;
    }
    let mut ticks = [0u64; MARKS as usize];
    {
        let range = timing.readback.slice(..).get_mapped_range();
        for (tick, bytes) in ticks.iter_mut().zip(range.as_chunks::<8>().0) {
            *tick = u64::from_le_bytes(*bytes);
        }
    }
    timing.readback.unmap();
    timing.state.store(IDLE, Ordering::Release);
    let timing = &mut *timing;
    for (stage, sum) in timing.sums.iter_mut().enumerate() {
        // Some drivers reorder or reset timestamps. Count a negative span as zero.
        let ticks = ticks[stage + 1].saturating_sub(ticks[stage]);
        *sum += ticks as f64 * f64::from(timing.period_ns) / 1.0e6;
    }
    timing.frames += 1;
    if timing.frames < WINDOW {
        return;
    }
    for (stage, sum) in timing.sums.iter_mut().enumerate() {
        let ms = (*sum / f64::from(timing.frames)) as f32;
        STAGE_MS[stage].store(ms.to_bits(), Ordering::Relaxed);
        *sum = 0.;
    }
    timing.frames = 0;
}
