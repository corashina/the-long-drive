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
| X03a | Player hum | M | Source `fpscontroller` hum on `IN.hum`: start and loop clips, scroll selects the tune, mouse sets volume and pitch. `humStimul.SetAudioValue(volume * humStimulValues[tune])` lets rabbits hear it. Export the clips, values and key binding. |
| X03b | EMP jamming | L | Live `electronicsscript.jamValue` (`mainscript.jamIncrease` 0.3, `jamDecrease` 0.1, `maxJamValue` 0.3, `TLastEmp`). Source `empscript`: `OverlapSphere`, Perlin range and value noise, `fallOff`, `ambientSoundModifier`. Jam dims car, headlights, flashlights, street lamps and bus doors, and affects compass, radio and metal detector. Today each output is fixed at spawn. |
| X03c | Green-eyed evil rabbits | S | Needs X03b. `empscript.makeRabbitsEvil`: rabbit `electronicsscript.nyul` output below 0.2 calls `GetEvil(natural: false)`. `Evil::get_evil` has the `Green` path. Link rabbit electronics and render `EyeGreen`. |
| X03d | Ground UFO | M | Needs X03b. Find and export the source prefabs that carry `empscript` (also check `sky_ufo`). Place the ground UFO with its model, sounds and EMP. |

### Multiplayer

Match the source Steam multiplayer (`sns`, `snl`, `snTempPlayerSync`). WebRTC peer to peer through PeerJS. The host browser runs the world. Guests talk only to the host. The host forwards.

Not planned: VR, stereo 3D, mod events, Steam features.

## Test pass

Run these checks on Tomasz's PC in one run. Passed checks are removed.
Check F3 in each scenario below. Compare with the frame rate rule.
Look, listen and F3-only checks are on Tomasz's eyes list (project file `plans/tp-eyes-list.md`), not here.

Headless run on 2026-11-02 (master ebed1d3, export 3fd48db): 174 of 179 ignored simulation tests passed, and 270 of 270 release workspace tests passed. #419 and #422-#425 fix the other 5. Each fix passed on Tomasz's PC before merge. W02 measured 24,128 bytes/km (120.6 MB for 5,000 km).

Open before the next run:

- X06c (browser): Stand inside the cabin, the turret house and a petrol station. Building probes take slots (F3), metal surfaces reflect the room, and F3 frame time stays in the frame rate rule.
- The stored key bindings reset once, because the export adds the `voicechat` binding (#M04).
- X03e (browser): Enter an old house, a mansion or the Vizmu building with human spawns. Some have a human monster in its rest pose. It does not move before X03f. Saves keep it, and a placement does not spawn a second one.
- X03f (browser): A human monster that sees the player chases them. It ducks and jumps over low walls, pushes doors open and climbs ladders. Out of sight it patrols. Peaceful mode stops it. Export check: `human.ai.col` is a capsule live collider and the five `sens` triggers export.
- X03g (browser): A live human monster walks, runs, turns and crouches with its Animator clips, looks at its target, and holds a door with its left hand. Feet stay on the ground. Export check: `human.animator` has 15 `grounded` and 4 `crouching` children, and `human.voice` has five clip lists. Port choices to check against the source: blend tree child speed is `1 / m_Duration` (crouch walks play twice as fast), clip foot IK goals and twist spreading are not used.
- X03h (browser): A human monster that reaches the player hurts the player and holds the player with both hands. A shot wakes a sleeping human and turns it to the shooter. Shots, explosions and hard hits kill it into a ragdoll with the death voice. Run `tp_x03h_human_shot_wakes_and_kills` after the re-export. The attack shows no damage flash yet.

Prepare once:

1. Pull master. Re-export, then make a release build. Old exports and old saves do not load. Only the current save schema loads.
2. Paste the acquisition hash that the export prints into `CONTRACT_SHA256` if it changed.
3. Run all ignored tests: `cargo test --release --lib -- --ignored --nocapture`.
4. In that output, note the `w02_` bytes per km and the 5,000 km estimate. Above 200 MB, compact spawn boundaries.
5. Claude runs the steps in Chrome with the test sandbox: see `tests/sandbox/README.md`. First check the sandbox itself: `spawn('Bus02Full', 10)` places a bus, and `teleport`, `building`, `spawn`, `hand`, `fill` and `cycle` work. Use `tests/sandbox/buses.js` for More cars steps 16-20.

### Startup

1. K4: Open the game 3 times in a new private window (empty cache). Each first load passes object loading and reaches the garage. The "Dependency staging" row on the loading screen does not stay full.

### Saves

1. K5: In game, pause > Load > Load last. The page reloads within 30 s and the last save loads. "Loading..." does not stay. Do the same with a named save from pause > Load.

### Multiplayer

Two browsers, host and guest in one room. Check F3 on the host before and after, with about 300 items near.

1. M03: The host kicks a loose can. It moves on the guest within 1 s and lands at the same place.
2. M03: The guest picks up a can and walks. The host sees the can move with the guest. The guest throws it. On both screens it lands at the same place.
3. M02: The guest drives the start car. The host copy steers, shifts and honks with it. The handbrake and ignition follow.
4. M02: Both see each passenger in their car seat, also at speed.
5. M03: The guest puts the can in the inventory. It goes away on the host. The guest takes it in the right hand. It shows in the guest hand on the host.
6. M03: The host walks 2 km away. The guest picks up a can there, drops it in another place and walks away too. The host comes back: the can is where the guest left it.
7. M03: The guest pours fuel from a canister into the start car and opens its fuel cap. The host sees the canister level drop, the car level rise and the cap open. The host opens a door; it opens on the guest.
8. M03: The guest takes off the start car hood and a wheel, and takes the tire off the rim. They come off on the host. The guest puts them back; they go back on at the host. The host hitches a trailer; it hitches on the guest.
9. M03: The guest rides a sled down a dune. The host sees the sled and the guest move together. The host switches a garage light and a building lamp, and opens a bus door; all change on the guest. The guest rusts a part with a crash; the host sees the rust.
10. M03: The guest eats half a sandwich, fires a gun and reloads it. The host sees the bites and hears the shot; the gun ammo matches on both. The guest pushes a wheelbarrow; it moves on the host. The guest breaks a bottle: the host sees it break and the shards on both match.
