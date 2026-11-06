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
Spec: Clear, Cloudy, Rain, Thunderstorm, Fog. Each kind holds 8 to 25 game minutes and blends over 90 s.
Test on the PC after all rows: force each kind with F4, check the look, the rain cost (under 0.3 ms GPU), wet grip and MP sync.

| ID | Task | Size | Remaining work |
| --- | --- | --- | --- |
| WT2 | Sky, fog, cloud and light per weather | M | Apply the `WeatherMix` look in `compose_weather` before the sandstorm. |
| WT3 | Rain render pass | M | One instanced GPU streak pass, preset caps, shelter fade. Bench compare. |
| WT4 | Rain audio and shelter | S | CC0 rain loops and thunder clips. Inside loop when sheltered. |
| WT5 | Lightning and thunder | S | Strikes in Thunderstorm, 0.15 s flash, thunder at distance / 343 s. |
| WT7 | Multiplayer weather sync | S | Host sends the cycle and strikes. Guests do not run the cycle. |
