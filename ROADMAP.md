# Project roadmap

Goal: recreate The Long Drive for the web.
Behavior authority: Steam build `11140890` and its decompiled code in `.cache/reference-library/records/managed/Assembly-CSharp.cs`.
Save format authority: [save schema](crates/simulation/src/save/schema.rs).

## Frame rate rule

The game runs at about 60 FPS now. Every new feature must keep it there.
Check the F3 overlay before and after each change. Report the frame-time cost.
If a feature drops the frame rate, fix the cost before merge or agree a cut.
A PR that changes render, shader, worker tick or world stream code must have a bench compare.
Run `node tests/bench/fps.mjs` on the base and on the branch, then `compare` the two JSON files.
Put the compare table in the PR. If the PC run waits for the section test pass, list the PR there.

## Task rules

Keep the game playable after each task.
Recheck the code and the original before each task. Complete missing behavior and verify existing behavior.
Remove a row when its task is done. Do not mark rows done.

Size: S is up to a day. M is a few days. L is about a week. XL is several weeks.

## Open tasks

| ID | Task | Size | Remaining work |
| --- | --- | --- | --- |

### Multiplayer

Match the source Steam multiplayer (`sns`, `snl`, `snTempPlayerSync`). Peers connect directly through WebRTC with PeerJS. The host browser runs the world. Guests talk only to the host. The host forwards.

Not planned: VR, stereo 3D, mod events, Steam features.

### Weather

Weather types are a new feature, not parity. The source has only the sandstorm. It stays as in the source and spawns only in Clear and Cloudy.
Spec: Clear, Cloudy, Rain, Thunderstorm, Fog. Regional weather cells move over the world. The weather at the player is the sum of the cells there.
Test on the PC after all rows: force each kind with F4, check the look, the rain cost (under 0.3 ms GPU), wet grip and MP sync.

| ID | Task | Size | Remaining work |
| --- | --- | --- | --- |

### Weather v2

Weather is a place, not a global filter. Spec and plan: `plans/weather-v2` (spec.md, plan.md) in the project files.
Test on the PC after all rows: the section test pass in plan.md, with a bench compare of Clear, Thunderstorm and Snow on High.

| ID | Task | Size | Remaining work |
| --- | --- | --- | --- |
| WX3 | Snow | M | Snow kind in the snow biome, flakes, snow cover, snow grip. |
| WX7 | Car cabin shelter | S | A roofed car cabin is a shelter. Rain-on-roof loop. |
| WX8 | Spray and glass drops | M | Wheel spray on wet roads. Drops on the windshield of the seated car. |
| WX9 | Lightning v2 | S | Visible bolt at the strike point. Close strikes play a sharp crack. |
| WX10 | F4 panel v2 | S | Cell map, wind, temperature, spawn a cell at the player. |
| WX11 | Multiplayer and save v2 | S | Wind and snow cover in the save and the cells message. |
