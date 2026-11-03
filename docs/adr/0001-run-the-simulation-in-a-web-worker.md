# ADR 0001: Run the simulation in a Web Worker

Status: Accepted
Date: 2026-11-02

## Context

The game runs in the browser through WebAssembly. Rust, Bevy and Rapier make the game.
The goal is about 60 FPS (see the frame rate rule in [ROADMAP.md](../../ROADMAP.md)).
One browser thread cannot do physics, game logic and WebGPU rendering in 16.7 ms.
Physics steps at a fixed 50 Hz, like the source (`FIXED_DT` in `crates/simulation/src/simulation.rs`).
Rendering runs at the display rate. The two rates are different.
Multiplayer (M01 to M04) makes the host do more simulation work for each peer.

## Decision

The simulation runs in its own Web Worker. The main thread only renders.

- `crates/simulation` holds all game state and behavior. It has no Bevy render code.
- `crates/simulation-worker` is the worker entry. It has its own WASM memory and one `Session`.
- The root crate is the renderer. It uses Bevy and WebGPU on the main thread.
- `crates/simulation/src/boundary.rs` is the only interface between the two.
  - The renderer sends one `SimCommand` for each rendered frame.
  - The worker sends one `SimFrame` for each tick.
  - Poses and state are `Pod` rows. Rare changes are postcard encoded.
  - The worker sends only changed pose rows. The renderer interpolates poses.
- One command is in flight at a time (`src/app/sim_link.rs`). Newer commands merge into the next one.
- Buffers move with `postMessage` transfer. Shared memory is not used.
- Physics does at most 2 steps for each frame (`STEP_BUDGET`).
- Peer packets go through the same boundary. `web/net.js` gives them to the worker with the next command.
- Terrain, road and grass meshes use their own workers on the same pattern.

## Consequences

- Worker work does not change render FPS. Render FPS depends on the main thread and the GPU.
- Native tests run the simulation crate without a browser or a GPU.
- The renderer shows state one tick late. Interpolation hides the step.
- Every new state that the renderer shows needs a row or a message in `boundary.rs`.
- The renderer must not read or change game state directly. It sends commands.
- Each worker loads its own WASM module (5 in total, see `scripts/build.ps1`). Startup downloads and memory are larger.
- `SharedArrayBuffer` is not needed, so the server does not need cross-origin isolation headers.

## Rejected options

- One thread for simulation and rendering. Physics and logic take frame time from rendering.
- Bevy multithreading in the browser. It needs `SharedArrayBuffer` and cross-origin isolation, and Bevy WASM threading is not stable.
- `SharedArrayBuffer` ring buffers between the threads. They add complexity, and transfer of changed rows is fast enough.
