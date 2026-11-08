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

| ID | Task | Size | Remaining work |
| --- | --- | --- | --- |
| A01 | Player damage between players | M | Source `meleeweaponscript` and `weaponscript` send `SDmgPlayer` for a hit on a remote figure (layer 9). Add the message so melee and gun hits hurt other players. |
| A02 | Zoom eyes and zoom sync | S | Source `bulgingeyesScript` scales the eyes while `zooming`. `SPlayerAnim` sends `zooming`. Add both. |
| A03 | Take an item from another player's hands | M | The source allows it. `interaction/in_use.rs` blocks it. |
| A04 | Sync the remaining item state messages | M | Check `color`, `rndType`, `licenseplate`, `localPos`, `itemDataUpd` and `itemRBUpd` in the source. Add each one whose state changes in play. |
| A05 | Starter pulse on remote cars | S | `net/car.rs` does not pulse the starter of `UpdMultiIgnition`. |

### Cleanup

| ID | Task | Size | Remaining work |
| --- | --- | --- | --- |
| A06 | Steering wheel settings reset | S | `web/settings-screen.js` does not port the source reset. |
| A10 | Remove web exports with no importer | S | About 45 exports in `web/*.js` are used only in their own file. Remove the `export` word, or the code if it is dead. |

