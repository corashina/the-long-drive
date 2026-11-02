# The Long Slop

Prepare a fixed Lada 2101 in the starting garage. Drive onto the desert road.
Read [vehicles](docs/vehicles.md) for the selected car and source records.
The home, car, and supplies come from installed Steam build 11140890.
Rust, Bevy, and Rapier run the browser game through WebAssembly.

## Export, build, and run

Start Docker Desktop. Install Node.js and Python 3.12 or later. Run `npm ci` once for the web lint.
Keep Steam build 11140890 installed.
Prepare [host PowerShell and pinned ILSpy](tools/reference/README.md#host-managed-analysis-tools) before export.
Build the toolchain image, then export the complete library and selected runtime:

```powershell
python -m venv tools/reference/.venv
./tools/reference/.venv/Scripts/python.exe -m pip install -r tools/reference/requirements.txt
docker build --tag the-long-slop-toolchain --file tools/Dockerfile .
./tools/reference/.venv/Scripts/python.exe -m tools.reference.export --steam-root 'C:/Program Files (x86)/Steam/steamapps/common/The Long Drive'
./scripts/build.ps1
node scripts/serve.mjs
```

Open [the local game](http://127.0.0.1:4173). Wait for the scene. Select New game, then Continue to capture the mouse.
Press Ctrl+C in the server terminal to stop it.
The exporter does not change Steam files. Git excludes exported assets and `web/pkg`.
A missing or invalid export stops loading.

Run routine formatting and WASM compile checks:

```powershell
./scripts/check.ps1
```

Run selected native tests by integration target, name filter, or both:

```powershell
./scripts/check.ps1 -Test texture_upload
./scripts/check.ps1 -Filter roadside_
./scripts/check.ps1 -Test texture_upload -Filter refill
```

Native test commands also check formatting and WASM compilation.
Run the web lint and selected web suites by filename stem. This command skips Docker and Rust checks:

```powershell
./scripts/check.ps1 -WebTest loading,save-bridge
```

Run all retained web and native tests, formatting, and WASM compile checks only when requested or agreed:

```powershell
./scripts/check.ps1 -Full
```

Checks do not create a browser package. WASM compile checks do not prove runtime behavior.
Run a release build only when requested or at an agreed milestone:

```powershell
./scripts/build.ps1
```

The build script writes the browser package to `web/pkg`.
To iterate on a bug, use `./scripts/build.ps1 -Fast`. It skips LTO, wasm-opt and Brotli, so a rebuild takes about 1 minute.
Fast packages are larger and slower. Do not use them for F3 numbers or test passes.
It also prepares Brotli downloads in `.cache/http-compression`. Keep that directory with the server deployment.
After replacing an export without rebuilding WASM, prepare its downloads and restart the server:

```powershell
node scripts/prepare-http-compression.mjs
```

The server negotiates Brotli for WASM and current package JSON. It keeps images and package receipts unchanged.

## Development asset library

The export holds the full build 11140890 library. Startup loads only the selected runtime assets.
Read [export stages](tools/reference/README.md) and [support limits](tools/reference/README.md#development-support-limits) before you use an asset.
Library assets do not add gameplay.

## Controls

| Control | On foot | In the driver seat |
| --- | --- | --- |
| WASD | Move | Accelerate, brake, steer |
| Mouse | Look | Look |
| Shift | Run | — |
| Space | Jump | Toggle handbrake |
| E | Use cap, switch, or seat | Use control or leave seat |
| Hold LMB + mouse | Move an aimed mechanism | Move an aimed mechanism |
| F | Carry or remove a part; release to install; tap to drop | — |
| Hold F, then release | Charge and throw a held item | — |
| Hold F at the car body | Push while held | — |
| F at the steering grip | Latch grip; WASD pushes and steers; F releases | — |
| Mouse wheel / Tab | Change carry distance / recenter | — |
| Hold MMB / RMB + mouse | Rotate / move held item | — |
| Hold MMB and RMB + mouse | Roll held item | — |
| X | Split a wheel | — |
| Hold LMB | Fill an open container | Use an aimed switch |
| I | — | Tap ignition; hold starter |
| Left Ctrl / Right Ctrl / Numpad 0 | Crouch with Ctrl | Clutch |
| Up arrow / Down arrow | — | Shift up / down |
| R / Numpad + | — | Select reverse |
| 0 / Numpad - / N | — | Select neutral |
| 1–6 / Numpad 1–6 | — | Select a forward gear, when available |
| L | — | Cycle headlights: off, low beam, high beam |
| H / Numpad 8 | — | Horn |
| Hold V | Sleep, when eligible | Sleep, when eligible |
| Hold P | Urinate | Urinate |
| B | Burp or fart | Burp or fart |
| C | — | Switch first person / chase view |
| Backspace | Hide or show feedback | Hide or show feedback |
| Esc | Pause menu | Pause menu |

Remove the hubcap before removing its wheel. Removed parts enter your hands.
Move a held part close to a compatible empty slot. Tap F when the install prompt appears.
Aim at an empty wheel slot to install a wheel.
Some source items, including the fuel can, cannot rotate.

Carry a fuel can. Open the fuel cap. Aim at the opening and hold LMB to fill it.
Direct fill moves fluid from the carried container into the selected open container.
Aim at an open supply valve to fill the supply from your carried container.
Open containers spill when tilted. Supplies keep their current mixture and stock.
The Lada starts with 0.5 L gasoline, 2.5 L oil, and 3.6 L water.
Its capacities are 39 L fuel, 5 L oil, and 5 L coolant.
The Skoda starts with the same fluids. Its capacities are 32 L fuel, 5 L oil, and 5 L coolant.

Controls contains Automatic clutch and Automatic gears. Automatic gears requires Automatic clutch.
With Automatic gears, S requests reverse after the car stops. W requests forward travel.
Disable Automatic gears to select gears. Without Automatic clutch, press the clutch past halfway before a shift.
The gear display shows R, N, or the forward gear number while seated.
Save stores both cars' gears and engine state, plus the two assist choices. Load restores them.
New game restores neutral and enables both assists. Recovery and input clearing keep the selected gears.

Use Recover in the pause menu. Use Controls for bindings and Fullscreen.
Release the wheel before tire removal. Aim at its tire and press F to remove it.
Hold a compatible tire beside the loose rim. Release F at the tire slot to install it.
Remove the hubcap before installing the wheel.
Focus loss pauses play and clears input. Continue captures the mouse again.
Recovery preserves fluids, parts, condition, trip distance, cycle time, and lamp state.
An empty fuel tank requests the nearest supported physical supply. Water supplies do not provide gasoline.
Recovery waits for destination geometry, materials, collision, and clearance before moving the car.
Restart restores the garage, supplies, car, origin, trip distance, source cycle time, and lamp state.
