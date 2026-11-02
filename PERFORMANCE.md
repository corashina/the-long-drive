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
- Shadow cascades 0 and 1 render each frame, 2 and 3 every 2nd and 4th frame (`crates/simulation/src/reference/shadow_cadence.rs`). A rendered cascade does not draw its tile when its projection, the held sun and its casters did not change. The shadow sun moves in steps of about one texel (`SUN_STEP` in `src/presentation/shadow_scene.rs`). Each atlas has a static copy (`static_atlas` in `render_targets.rs`). Static casters and props draw there. Grass and skinned casters do not change it: their tiles copy the static tile and draw only these casters each frame (`shadow_atlas.rs`).
- Lamp shadow faces use the same reuse (`LampHistory` in `crates/simulation/src/reference/lamp_shadows.rs`). A face draws only when its projection or the casters of its lamp change. Grass and skinned casters in a lamp draw its faces each frame.
- Reflection probes: only the camera of the face in the current capture job is active. Mip and publish jobs run no probe camera. Warmup runs all 6 face cameras (`sync_probe_cameras` in `src/presentation/view_frame.rs`).
- Shadow casters sit in a loose 32 m grid (`crates/simulation/src/reference/shadow_grid.rs`). Each moving view tests only the cells near its shadow hull, and the lamps only the cells near their spheres. Mask users read the sorted row list of each view (`ShadowScene.marked`), not all casters.
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
Also check the shadow caster grid: F3 "WASM main" while driving in a town, before and after. Shadows look the same in the world view, mirrors, probes and lamp shadows at night. Cars that drove far and props that stream in still cast shadows.
Also check the animated caster split: stand still in grass and in third person with outfit mode 1. F3 shadow "GPU ms" should drop against master. Grass and body shadows move, no trail stays where the body was, and no shadow acne differs from master.
Also check the render bundles: no WebGPU errors in the console, and terrain, roads, buildings, cars, props and their shadows draw as before, also with Antialiasing off and in reflection probes. F3 "WASM main" should drop in a town.
Also check the reflection probes: car reflections show all 6 cube faces as before. F3 "WASM main" should drop near cars with Reflections on.
