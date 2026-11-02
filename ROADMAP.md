# Project roadmap

Goal: recreate The Long Drive for the web.
Behavior authority: Steam build `11140890` and its decompiled code in `.cache/reference-library/records/managed/Assembly-CSharp.cs`.
Save format authority: [save schema](crates/simulation/src/save/schema.rs).

## Frame rate rule

The game runs at about 60 FPS now. Every new feature must keep it there.
Check the F3 overlay before and after each change. Report the frame-time cost.
If a feature drops the frame rate, fix the cost before merge or agree a cut.

## Task rules

Keep the game playable after each task.
Recheck the code and the original before each task. Complete missing behavior and verify existing behavior.
Remove a row when its task is done. Do not mark rows done.
Add the manual checks of each task to the test pass below. Remove them after they pass.

Size: S is up to a day. M is a few days. L is about a week. XL is several weeks.

## Open tasks

The starting area, car parts, world and more cars sections are done. Only their test pass remains.

| ID | Task | Size | Remaining work |
| --- | --- | --- | --- |
| X03 | Creatures | XL | Ground UFO, EMP and green-eyed evil rabbits, human monster (attacks hurt the player), breakablescript `DamageFrom` for AI. Rabbits do not yet sense the player hum. |

### Multiplayer

Match the source Steam multiplayer (`sns`, `snl`, `snTempPlayerSync`). WebRTC peer to peer through PeerJS. The host browser runs the world. Guests talk only to the host. The host forwards.

| ID | Task | Size | Remaining work |
| --- | --- | --- | --- |
| M01 | Join and see each other | S | Remote player collision: export the `mpplayerscript` capsule (`minBodyRadius`, `maxBodyRadius`, head and prone head nodes), add a kinematic capsule per figure, 3 s spawn delay and 1 s after standing up. |
| M02 | Shared cars | L | Driver input sync (`carfloats`, gears, ignition, lamps, handbrake), passengers. Seated figures follow the seat of the car by save id (source `UpdPos` sitting). |
| M03 | Shared items and world | XL | `claim`, item poses and velocity, inventory, tanks and caps, parts, usables, attachables, food, weapons. |
| M04 | Everything else | XL | Voice chat (M04b). After the M03 network keys: host-only creature, Fedo, random stuff and tumbleweed spawns, creature poses to guests, radio controls (`msgType.radio`). The source does not sync sandstorms or sleep. |

Not planned: VR, stereo 3D, mod events, Steam features.

## Test pass

Run these checks on Tomasz's PC in one run. Passed checks are removed.
Check F3 in each scenario below. Compare with the frame rate rule.
Look, listen and F3-only checks are on Tomasz's eyes list (project file `plans/tp-eyes-list.md`), not here.

Known failures on master e927770 (2026-10-31 headless run, export ee6554d), fix before the next run:

- K4 (browser): First page load can stall at the start of object loading with dependency staging full (7.7/7.8 MiB). A reload clears it.
- K5 (browser): In game, pause > Load > Load last once stayed on "Loading..." for over 6 minutes, with no console messages. The simulation and the save bridge finish the load (`tp_k5_s1_a_stored_save_starts_the_session_and_its_restore_finishes`), so the hang is in the browser.
- 32 of 169 ignored simulation tests failed in release. Debug overflows the stack. #KF removed 8 old tests that `roadmap_tests` covers, and changed 4 without a run. The lines below name each open one.
- WD3: `tp_wd3_each_melee_weapon_swings_with_each_key_and_breaks_a_bottle` needs the next export. The exporter now links melee weapons through `pickupable.melee` (#WD3), which adds the katana, fly swatter and leg. The test passes on ee6554d with those records added.
- WD20: `tp_wd20_a_hard_hit_breaks_a_tumbleweed_with_its_gib_and_it_stays_gone` needs the next export. The exporter now exports gib ParticleSystems (`behavior.gib_particles`, #WD20). The tumbleweed gib is particles only. The test passes on ee6554d with a stand-in particle record.
- Export field added (#PB): `player.seat_push_force` (source `fpscontroller.seatPushForce`). Export ee6554d lacks it, so re-export before the next run.
- Export format changed (#X06c): each world prototype has `probes`, and `vehicle.probe` is gone. Re-export before the next run. Then stand inside the cabin, the turret house and a petrol station. Building probes take slots (F3), metal surfaces reflect the room, and F3 frame time stays in the frame rate rule.

Prepare once:

1. Pull master. Re-export, then make a release build. Old exports and old saves do not load. Only the current save schema loads.
2. Paste the acquisition hash that the export prints into `CONTRACT_SHA256` if it changed.
3. Run all ignored tests: `cargo test --release --lib -- --ignored --nocapture`.
4. In that output, note the `w02_` bytes per km and the 5,000 km estimate. Above 200 MB, compact spawn boundaries.
5. Claude runs the steps in Chrome with the test sandbox: see `tests/sandbox/README.md`. First check the sandbox itself: `spawn('Bus02Full', 10)` places a bus, and `teleport`, `building`, `spawn`, `hand`, `fill` and `cycle` work. Use `tests/sandbox/buses.js` for More cars steps 16-20.

