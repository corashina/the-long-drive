# Performance

Goal: keep the game at about 60 FPS (see the frame rate rule in [ROADMAP.md](ROADMAP.md)).
Performance work must not change game behavior.
Add only items that increase render FPS or remove hitches. Remove an item when it is done.

## Measure

- F3 overlay: frame time, "Worker tick avg", "Worker physics / world CPU avg", "WASM main / worker", draw counts, GPU prop rows, ECS entity count.
- F3 "GPU ms": GPU time of the world view per stage, average of 30 frames. Needs the browser `timestamp-query` feature. Probe faces, sky and prop culling are not in the stages.
- CPU profile (`web/cpu-profile.js`): stages Physics, RenderPreparation, Input and others.
- Measure with a release build (`scripts/build.ps1`), not `-Fast`.

## Architecture facts

- The simulation runs in a Web Worker (`crates/simulation-worker`). Render FPS depends on the main thread and the GPU. Worker-only work does not change render FPS.
- The worker sends only changed pose rows. The renderer interpolates poses.
- Physics steps at 50 Hz, at most 2 steps per frame (`STEP_BUDGET` in `crates/simulation/src/simulation.rs`).
- Terrain tiles and grass meshes are built in their own workers.
- WebGPU is the only renderer. MSAA is 4x. The "Antialiasing" setting turns it off after a reload (`src/presentation/antialiasing.rs`). TAA runs with it. No camera-depth pass.
- Motion vectors run when antialiasing or "Motion blur" is on. Motion blur adds 7 fullscreen draws (`postprocessing/motion_blur.rs`); off removes the pass.
- Display settings for weak GPUs, default at full quality: scene resolution 100% to 50% (`render_scale.rs`), "Shadows" high, medium, low (world view atlas side halved per step, `shadow_quality.rs`), "Ambient occlusion" on or off (`ambient_occlusion.rs`), "Grass" high, medium, low (blade spacing and fade distance, after a reload, `materials/grass/settings.rs`).
- World props draw through the own GPU path: compute cull and indirect draws (`src/presentation/prop_draws.rs`). Each view pass runs its prop draws as one render bundle.
- Static draws use render bundles (`src/presentation/render_bundles.rs`). A bundle records again only when its commands change. In the camera pass, unskinned meshes without morph targets run as one bundle per view (`static_draws.rs`); skinned meshes draw directly. The prop draws of each pass and cascade run as bundles.
- Shadow cascades 0 and 1 render each frame, 2 and 3 every 2nd and 4th frame (`crates/simulation/src/reference/shadow_cadence.rs`). A rendered cascade does not draw its tile when its projection, the held sun and its casters did not change. The shadow sun moves in steps of about one texel (`SUN_STEP` in `src/presentation/shadow_scene.rs`). Grass and skinned casters draw their cascades each frame.
- The FPS limit is off by default: source VSync on makes Unity ignore it. When on, each frame runs in an animation frame (`web/frame-pacer.js`).
- Release build: fat LTO, simd128, wasm-opt -O3.

## Open items

Size: S is up to a day. M is a few days. L is about a week.
Impact is an estimate from the code. No item has F3 numbers yet.

### No visible change

| # | Item | Size | Impact | Notes |
| --- | --- | --- | --- | --- |
| 2 | Skip unchanged item visuals | S | Low | The renderer still does a pose lookup and interpolation for every item visual each frame (`src/presentation/reference.rs`, `spawned.rs`). Use the changed pose rows to skip the rest. |

Rejected: async compute, bindless, Hi-Z occlusion, f16, subgroups, GPU grass, own QueryBounds BVH removal.

## Final test pass

Do on Tomasz's PC after the open items are done: F3 frame time and "GPU ms" before and after each item, in a town and in a grass area.
Also check that a part condition change, such as rust, shows the new material without a frame freeze. The old material shows until the new pipelines compile.
Also check the shadow tile reuse: F3 shadow atlas "GPU ms" standing still versus walking. No stale shadows under moving cars, the player, doors or fading LOD props. No visible shadow step when the sun moves.
Also check the render bundles: no WebGPU errors in the console, and terrain, roads, buildings, cars, props and their shadows draw as before, also with Antialiasing off and in reflection probes. F3 "WASM main" should drop in a town.
