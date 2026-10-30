# Bevy render ownership patch

This directory contains the cached crates.io `bevy_render` 0.19.1 package.
No package was downloaded. Its version, features, and dependency versions are unchanged.
The package retains its MIT and Apache 2.0 licenses.

Upstream crate SHA256: fc86a32b2eef9859e1e7d323c6d1a9150fe77aa17c30fd05abebc12f645929dc.
Upstream Git commit: b56fc29d3016e641754765244b5ba3f9cc504671.
Upstream path: crates/bevy_render.

The changed upstream Rust files are `src/render_resource/pipeline_cache.rs` and the render bundle files below.

`src/render_resource/pipeline_cache.rs`:
It adds held render IDs, per-ID synchronous preparation, safe retirement, and release.
Held IDs cannot enter automatic processing, including shader reload retries.
An owned queue guard admits descriptor storage before material specialization.
Descriptor accounting uses vector and string capacities. It also reserves cloned keys and retry links.
Rejected metadata stays held as a terminal error. Normal active-cache processing stays unchanged.

The product owns candidate scopes, cancellation, readiness, promotion, and the persistent metadata budget.
The patch does not change the mesh allocator, shader language, renderer backend, or normal active-cache queue.

`src/render_phase/draw_state.rs`: `TrackedRenderPass::begin_capture` sends the next commands to a list, not the pass.
The list holds only commands that a render bundle can hold. Other commands mark the capture as unsupported.
Capture start and end reset the pass state tracking, as a render bundle does.
`src/render_phase/mod.rs`: `BinnedRenderPhase::render_filtered` draws only the items whose entity passes a filter.
A batch passes when its representative entity passes. `render` uses it with a filter that passes every item.
The product owns the bundles (`src/presentation/render_bundles.rs`).

When upgrading Bevy, review this file against the new PipelineCache API and preserve these ownership rules.

The local copy omits unused package metadata and the manifest's reference to the original manifest.
The local .gitattributes disables line-ending conversion within this crate to preserve upstream bytes.
