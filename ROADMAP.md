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
| X03f | Human monster AI | L | `newAiScript.Update`: sight by `fov` and ray, chase time, patrol (`GetNewPatrolPos`), rigidbody move, `AvoidWalls`, jump when stuck, ladders (`ladderV`), doors (`Use`), `peacefulEnemies`, `rizseshusi` target. |
| X03g | Human monster animation and voice | M | Needs X03f. `newAiAnim` animator, look target and hand IK. Voices: idle, notice, chase, attack and death lists with pitch, and the `TVoice` mouth move. |
| X03h | Human monster attack and death | M | Needs X03f. `Attack`: ray to the player, `survival.DamageInstant(damage * dt)`, both hands hold. `breakablescript.DamageFrom` wakes it and turns its target to the hit. Shot, collision and explosion break it. `Die`: ragdoll and death sound. |
| X03i | UFO seat, colliders and stimulus | M | `fedoscript` seat: `PlayerInput`, the `AutoPilot`, `SetSpeed` and `SetHeight` usables, `seatedObj`. Fedo01 rigidbody and colliders. Its `stimulscript` (kind 3, value 50) for rabbits. The UFO itself flies since X03d. |

### Multiplayer

Match the source Steam multiplayer (`sns`, `snl`, `snTempPlayerSync`). WebRTC peer to peer through PeerJS. The host browser runs the world. Guests talk only to the host. The host forwards.

Not planned: VR, stereo 3D, mod events, Steam features.

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

### Creatures

Re-export the reference first: X03d adds `sky.ground_ufo` and `sky.prefabs[].emp`, X03b adds `ambient.player_electronics`.

1. X03d: Sleep through nights until a UFO comes (after 2 to 6 nights, 75 % chance, host only). It comes from behind, 15 m over the road, with its hum and spin. It passes the player and flies on. Check F3 while it is near.
2. X03b: Let the UFO pass near the start car at night. The headlights and rear lights dim and go out, the engine does not start and the radio fades to static. All come back within about 10 s after it leaves. Check F3 while it is near.
3. X03b: Hold a compass and a switched-on metal detector under the UFO. The compass points at the UFO, then spins when it is close. The detector beeps without metal.
4. X03b: Near road lamps and a building with lamps, the lamps dim and go out under the UFO and come back after. The ambient sound drops and recovers. A bus door button does nothing while the bus is jammed.
5. X03e: Enter an old house, a mansion or the Vizmu building with human spawns. Some have a human monster in its rest pose. It does not move before X03f. Saves keep it, and a placement does not spawn a second one.
