# Vehicles

The world loader spawns cars from car prototypes. The export allows the Lada, the bus pool, the bike pool and the V02 car types.

## Car types

| Car type | Source root | Start field | Engine |
| --- | --- | --- | --- |
| Lada 2101 (car02full; bare body car02) | `sharedassets1` 82556; car02 in `CAR_ITEMS` | `StartCar02` (`level1` GO 17687) | Front, petrol |
| car01, car02k, car04, car05 (bare and full) | Catalog items in `CAR_ITEMS` | `StartCar01`, `StartCar02k`, `StartCar04`, `StartCar05` | Petrol |
| car06 (bare and full) | Catalog item in `CAR_ITEMS` | `StartCar06` | `randomTypeSelector`: petrol or diesel, 50/50 |
| car03, car03k, car10, car10k (bare and full) | Catalog items in `CAR_ITEMS` | `StartCar03`, `StartCar03k`, `StartCar10`, `StartCar10k` | Two-stroke |
| car08, car08c (bare and full) | Catalog items in `CAR_ITEMS` | `StartCar08`, `StartCar08c` | Petrol |
| car09 (bare, full, `car09fullS`) | Catalog items in `CAR_ITEMS` | `StartCar09`, `StartCar09S` (`level1` GO 12382, `Car09full_slide`) | Two-stroke |
| car09P (bare and full) | Catalog items in `CAR_ITEMS` | `StartCar09P` | Two-stroke |
| car07 (bare and full) | Catalog items in `CAR_ITEMS` | None (pools only) | Petrol, forced automatic |
| Bus01, Bus02, Bus03 (bare and full) | Catalog category `bus` items with `carscript` | World spawn pools | Diesel |
| Bike01 (bare and full) | Catalog category `bike01` items with `carscript` | World spawn pools | Motorcycle, kick start |
| Bike03 (bare and full) | Catalog category `bike03` items with `carscript` | World spawn pools | Moped, pedals and engine |

## Car names

The game uses its own vehicle names, not source labels. `interaction::labels::vehicle_name` holds the table.
The hover text and the Game mode start car list show them. Saves and sandbox commands keep the source names.

- Each `vehicle` record has the source `carscript.carType`.
- `CAR_ITEMS` also spawn from the pools `kaszni` (bare body) and `carfull` (full car). Car06P and Car09T are V03.

Trailers (`utanfutoscript`) export as world prototypes with a `trailer` record. See [Trailers](#trailers).

## Model

- `car_prototypes` in `tools/reference/world.py` is the allow list. It reads the catalog at export time.
- The list holds the Lada, each `CAR_CATEGORIES` item root with `carscript`, and each `CAR_ITEMS` item root. Other car roots stay `other_vehicle`. Pools do not spawn them.
- A `randomTypeSelector` engine part (car06) must be a node of the car prototype. Each engine part then has its engine record.
- A `randomTypeSelector` variant `tank` replaces that tank's fluids after the random fluids. The `StartCar06` diesel pick puts 0.5 L diesel in the fuel tank. Variant `Move` and `R` are not ported; no V02 car uses them.
- Pool car copies run the loot steps: `RandomSpawned` (fluids, condition `ir1`/`ir2`), then component Starts (`randomTypeSelector`, `partconditionscript`), then deferred `partslotscript` parts. A start car skips `RandomSpawned`.
- An allowed car root exports as a world prototype, as loot does. Its `vehicle` record holds the `carscript` data.
- The record mass is dry `massScript` mass. Wheel parts, mounted parts and tank fluid (1 kg per liter) add at runtime.
- Engine and radiator records are on their parts. The car uses the engine part that is mounted on it.
- A car can have any wheel count. The radiator and the engine oil tank are optional.
- Engine records have `two_stroke` and the mix tolerances `oil_toleration_min`, `oil_toleration_max` (source `oilTolerationMin`, `oilTolerationMax`).
- The vehicle record has `oil_led_two_stroke` (source `carscript.ledOilTestTwoStrokeDefault`).
- `colliderscaleapplyscript` (Car10k) is an editor bake helper. The export stops if its `run` flag is set.
- The fuel tank is the root `tankscript`. Without it, the tank is the part in `carscript.tankslot` (Bike03).
- A `clockscript` on an active child node (Car07 dashboard clock) binds like the item clock, on the car root item. It reads only the meter face with `tipus` "time".
- Built-in gauges come from `carscript.meters`. Each meter adds needles, lamps and a backlight. The odometer is optional.
- Gauge parts (Bus01) are `meterscript` on part nodes. `carscript.kmhmeters` updates the gauge in each listed slot.
- A `kmhmeters` row reads its `types`, times `modifier`. With `force`, every needle of the gauge reads the speed (source behavior).
- A gauge adds the driven distance to its `kilometerszamlalo` and `napiszamlalo` counters. A removed gauge keeps its values.
- Diesel is data only. Each engine part has its `fuel_consumption` mix. Fuel suitability compares the tank mix with it.
- Pumps and cans take their fluids from the source tanks. Fluid 5 is diesel.
- `crates/simulation/src/world/content_vehicles.rs` binds a car copy: chassis, `VehicleRuntime`, wheel slots, engine and radiator parts.
- `ObjectKey` keys each car copy. `GarageRuntime.vehicles`, `PhysicsWorld` and wheel slots use this key.
- One Rapier world holds every chassis. Every engine steps each frame. Only the seated driver's car reads keys.
- `GarageRuntime.vehicle` is the last seated car. The camera, HUD, and recovery follow it.
- Parts fit only slots with a matching source part type. The source rules decide cross-car fit.
- The world generator can place any number of copies at any pose. Each copy has its own key.

## Start car

- `start_placements` reads each `START_CARS` field of `TerrainGenerationSettings` (`level1` 64898). The field links the start car group.
- The group holds one `carscript` root, a scene copy. The entry has its scene pose, the prefab of the catalog item, and `start_car` set.
- The stream spawns only the picked start car, as a fixed placement on the `Startup` layer.
- The start pack of a group spawns only with its start car (`StartCar01`, `StartCar02`).
- The Game mode screen has a `#start-car` list: Random, then each manifest start placement. The settings store keeps it (`startCar`, -1 = Random). The menu default is Random.
- `crates/simulation/src/world/start_car.rs` picks the start car. Random follows `mainmenuscript.RandomStartCar`:

| Field | Chance |
| --- | --- |
| `StartCar01` | 100 |
| `StartCar02` | 75 |
| `StartCar02k`, `StartCar03` | 65 |
| `StartCar03k`, `StartCar04` to `StartCar08c` | 55 |
| `StartCar09`, `StartCar09P` | 30 |
| `StartCar10`, `StartCar10k` | 55 |

- The pick draws `Random.Range(0, sum)` and takes the first cumulative chance above it.
- A field without an exported placement is skipped with its chance. V03 adds `StartCar06p`.
- Car09 then takes `StartCar09` or `StartCar09S`, 50/50 (source `startscript`).
- The pick uses a menu random from browser entropy. It does not touch the world random state.
- The spawn context current car is source `itemdatabase.carTypeToCarID[carType]` of the picked car. The Lada gives 15.

## Save

- On retire, the car, engine, radiator and wheel slot states go to the world change records.
- On return, the copy respawns and takes its states back. A moved car returns at its last pose.
- The save keeps these records per key: pose, gear, ignition, handbrake, trip, headlights, engine state and fuel.
- The save keeps gauge part values per key.
- The save keeps bus door state (`Bopened`, `Bsigned`) per bus.
- The save keeps the start car field. Load spawns its placement and ignores the menu choice. Validation checks the current car against it.
- Save schema 25. Older saves are rejected.
- `crates/simulation/src/world/stream_tests/car_types.rs` drives, saves and loads each start car. It needs the export.

## Bus doors

`crates/simulation/src/world/content_busdoors.rs` binds `busdoorscripts`, `busdooropenscript` and `dinamiccolliderforbus` on a car copy.

- Each door has `Usign` and `Uopen` usables. Their colliders are switch knobs. One usable can serve more doors.
- `Usign` sets the signal. `Uopen` toggles the door and clears the signal.
- Lamps: open = green off, red on. Signal = green on, red off. Else both off. `Rbs` and `Rbc` take the `greenON`/`greenOFF` and `redON`/`redOFF` materials.
- Buttons work only when the `electronics` output is above `mainscript.lightTurnOffValue`. With no electronics they always work.
- Leaves lerp their euler angles to `opened` or `closed`. The speed is drawn in `Start` order from `minspeed`..`maxspeed`.
- A leaf plays a random sound when `minTime` passed since its last use. Colliders below a leaf turn with it.
- `dinamiccolliderforbus`: each box length follows the `t1`-`t2` distance when a leaf moves.
- `detachIfOpen` is null on every bus in the source build. A set link stops the bind with an error.
- A usable that also slides (world mechanism) slides and works as a door button.

## Trailers

`crates/simulation/src/interaction/hitch.rs` and `crates/simulation/src/world/content_trailers.rs` port `utanfutoscript`, `attachgripscript`, `attachablescript` (onlyToPoint) and `attachTargetscript`.

- A prototype root with `utanfutoscript` exports a `trailer` record: body, wheels, both centers of mass, brake force, brake switch, grip and rear lamps.
- `attachTargetscript` entries with type `utanfuto` export as `hitches`: the target node and the one collider on the script node.
- `attachgripscript.Start` spawns the grip prefab at `VgripPos` as a child content. A loose spherical joint holds it to the trailer.
- Attach: the player puts the held grip on a hitch collider. One spherical joint links the car body and the trailer at the grip point.
- A hitched grip stays a world item. Its body is disabled. Its shapes ride on the car without contacts or mass.
- Picking up a hitched grip detaches it. The grip keeps the car speed at the hitch.
- The trailer uses `CenterOfMassAttached` while hitched, else `CenterOfMass`.
- Brakes: each wheel gets `brakeForce` while the `brake` flashlightscript is on. Only the Bus02 trailers have one (`brakeForce` 100). It is a switch on the trailer, not the car brake lamp.
- Lamps: `rearlights` follow the towing car: headlights, ignition and brake. With no car, the lamps are off.
- Save: the grip attachment holds the car key and the target source. No schema change. Load waits for the target and the trailer.
- The trailer, its grip and the towing car load and unload as one group.
- Not ported: rope towing (B16 is not in the original), `PlusWheelToKeepAwake`, trailer wheel mesh spin.

## Physics fix

Rapier 0.36 CCD sweeps disabled colliders. The Skoda has disabled source colliders below the ground.
CCD then clamped the chassis each step. The car moved at a fraction of its speed. The Lada lost half its motion.
Disabled colliders now have empty solver groups. CCD ignores them.

## Driving controls

The driving branch uses source bindings for every car. Gear keys act with Automatic gears disabled.

| Action | Keys |
| --- | --- |
| Clutch | Left Ctrl, Right Ctrl, Numpad 0 |
| Shift up / down | Up arrow / Down arrow |
| Reverse | R, Numpad + |
| Neutral | 0, Numpad -, N |
| Forward gear | 1–6, Numpad 1–6, within the car's gear range |

Without Automatic clutch, depress the clutch past halfway before a shift.
Automatic clutch permits manual shifts. Its RPM clutch ramp runs inside automatic shifting.
Automatic gears requires Automatic clutch. In that mode, S requests reverse after stopping; W requests forward travel.
Every car uses neutral and both assists at New game startup.
At exact zero driven-wheel RPM, the endpoint-clamped port can retain neutral.
Source CalcTorque evaluates its curve without that wrapper. Native curve extrapolation remains unmeasured.
The serialized infinity value does not establish the public WrapMode mapping. Source boundary parity remains conditional.
Normal browser launch in both cars remains unverified.

The save stores each car's selected gear and engine state, plus both shared assist choices.
Load restores those values. Pause, capture loss, seat changes, and recovery clear input while keeping selected gears.
Seat entry suppresses pending shift edges. Seat exit clears ignition holds without a release toggle.
Automatic rules require a mounted engine; manual selection retains its source conditions.
Suspension validation accepts target `[0,1]` and nonnegative spring and damping values.
The gear display uses R, N, or a forward number. Browser acceptance of the seated display remains open.

## Wheel adapter limits

The drivetrain applies clutch engagement in the engine owner and each gear/final-drive ratio once.
The physics owner distributes axle torque among active driven wheels, then converts by installed wheel radius.
It supplies engine, service, and handbrake contributions to eligible wheels.
The stock Rapier controller excludes simultaneous motor force and brake impulse on one wheel.
Input sums therefore do not prove source combined-brake response.

Source tire stiffness and Rapier friction describe different models.
Tire/rim differences, empty hubs, suspension settling, and wheel spin require native measurements before further adapter work.
Task 4 lacks qualifying source observations. Task 5 remains unscheduled.

## Verification

The final fix on `codex/correct-driving-physics` follows review base `6f8118162ece6d9ee24c811719c0935af433487a`.
The controller owns its scoped re-review. Earlier full-suite results describe `05bbf7b`.
Native critical checks cover shift rules, clutch attenuation, signed RPM/stall, both-car seat transitions, and saved gears/assists.
The full native suite retains 108 known failures. These checks do not establish source handling parity.

The controller verified normal menu actions, paused save, full reload, restored choices, and visible New game assist defaults.
IAB refused pointer capture. Both-car driving, normal launch, recovery, restart, and handling-rate acceptance remain open.
The older `production_skoda_drives_switches_recovers_and_rebases` test describes a controlled Rust route with exported assets.
The current run passes Skoda movement checks, then fails the inactive Lada trip bound.
Parked drift and preparation sleep checks also fail. These ignored cases lack a complete prior baseline.
See [current verification](verification.md#driving-branch-verification-2026-10-14) for command results and retained evidence.

## Add the next car

1. Add the car's catalog items to `CAR_ITEMS` in `tools/reference/world.py`. Its roots then export with a `vehicle` record and spawn from pools.
2. For a start car, add its field and full item to `START_CARS`. Check the pose against home colliders and the terrain.
3. Run the runtime export. The exporter stops when a source record is missing. Car `mountStuff` volumes export by themselves.
4. Check the prototype node count against the stream limits.
5. Run the export-based tests with the new manifest.

Expected work for other cars:

- Two-strokes (car03, car03k, car09, car09P, car10, car10k): the engine needs two-stroke mixed fuel. They have no oil tank.
- Car03, car03k, car08 and car08c have no radiator.
- Car06P: diesel works through its engine `fuel_consumption`. Add its category.
- Bikes: add balance physics. The four-wheel controller does not fit.
