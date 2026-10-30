# Test sandbox scripts

Claude runs the ROADMAP test pass with these in Chrome. They are not automatic tests.

1. Open the game with `?sandbox`. Add `&date=11-01` for a fixed date. New games use seed 42; add `&seed=N` for another world.
2. Paste `helpers.js` into the game tab. Start a New game. Then paste one area file, for example `buses.js`.
3. Call the steps. `window.sandbox` has `teleport(x, z)`, `garage()`, `building(name)`, `spawn(name, forward)`, `hand(name)`, `fill(fluid, liters)`, `cycle(seconds)` and `state(radius)`.
   Setup helpers use the game paths, so the checks after them stay valid:
   - `seat(car, seat)`: sit on the nearest loaded seat. `car` is part of a root name (`'Car02Full'`), a game name (`'Volna'`) or `'Lada'`, or `null` for any seat, such as a sled or the WC. `seat` is `'driver'`, `'passenger'` or `null`.
   - `stand()`: get out to the first clear exit.
   - `wear(name)`: spawn a wearable or the cigarette and put it on. A bad name lists the names.
   - `equip(name)`: place a gun, wait until the stream loads it, then put it in the hand with full ammo. A bad name lists the gun names.
   - `aim(x, y, z)`: turn the head toward an absolute world point. Positions come from `state()`.
   - `spawn('rabbit', forward)` and `spawn('evil rabbit', forward)`: a small rabbit, or a big one that gets evil eyes unless Peaceful is on.
   - `storm(distance)`: a sandstorm `distance` metres ahead. Default 50.
   - `hand(name)` also takes world prototype items, such as `binoculars`. It waits until the stream loads the item.
   - `state()` lists `creatures` (rabbits, bats and planes), `storms` and `storm_influence`.
4. `__run` holds each step with the sandbox state and F3. Write it to the run report with one screenshot per step.

`spawn` takes a catalog item name (`sunglasses`) or a world prototype root name (`Bus02Full`, a gun, loot). A world prototype appears when the stream loads it, Save and load with a world spawn is not checked yet.
