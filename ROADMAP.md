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
- CR2: `tp_cr2_halloween_bats_near_buildings` was flaky: load time depends on the machine, and the one tracked bat could break before it squeaked. It now collects bats from the first frame of each visit and checks squeaks, flight and save over all bats of the building (#TP CR2). Changed without a run.
- Export field added (#PB): `player.seat_push_force` (source `fpscontroller.seatPushForce`). Export ee6554d lacks it, so re-export before the next run.
- Export field added (#M01): `player.remote` (source `mpplayerscript` of `snTempPlayerSync.playerPrefab`). Export ee6554d lacks it, so re-export before the next run. `tp_mp5_the_other_figure_turns_solid_after_3_s` then checks the remote figure body.
- Export format changed (#X06c): each world prototype has `probes`, and `vehicle.probe` is gone. Re-export before the next run. Then stand inside the cabin, the turret house and a petrol station. Building probes take slots (F3), metal surfaces reflect the room, and F3 frame time stays in the frame rate rule.
- Export field added (#M04): the `voicechat` input binding (source `inputscript.IN.voicechat`). Until the re-export, voice chat has no push-to-talk key. The stored key bindings reset once.
- Save schema 41 (#M04): rabbits keep their id (`RabbitSnapshot.id`). Old saves do not load.
- Save schema 42 (#TP W02): spawn boundaries are compact. They store sources, contexts and pool states once in tables, and each parent as a compact key. Old saves do not load. The 3fd48db run measured 158,021 bytes/km (790 MB for 5,000 km). On 6fae264 it measured 24,128 bytes/km (120.6 MB), below the 200 MB limit.

Prepare once:

1. Pull master. Re-export, then make a release build. Old exports and old saves do not load. Only the current save schema loads.
2. Paste the acquisition hash that the export prints into `CONTRACT_SHA256` if it changed.
3. Run all ignored tests: `cargo test --release --lib -- --ignored --nocapture`.
4. In that output, note the `w02_` bytes per km and the 5,000 km estimate. Above 200 MB, compact spawn boundaries.
5. Claude runs the steps in Chrome with the test sandbox: see `tests/sandbox/README.md`. First check the sandbox itself: `spawn('Bus02Full', 10)` places a bus, and `teleport`, `building`, `spawn`, `hand`, `fill` and `cycle` work. Use `tests/sandbox/buses.js` for More cars steps 16-20.

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
