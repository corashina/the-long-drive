# The Long Slop

Prepare a fixed Lada 2101 in the starting garage. Drive onto the desert road.
Read [vehicles](docs/vehicles.md) for the selected car and source records.
The home, car, and supplies come from installed Steam build 11140890.
Rust, Bevy, and Rapier run the browser game through WebAssembly.
Play in desktop Chrome or Edge 113 or later, with a keyboard and mouse. The game needs WebGPU.

## Export, build, and run

Start Docker Desktop. Install Node.js and Python 3.12 or later. Run `npm ci` once for the web lint.
Keep Steam build 11140890 installed.
Prepare [host PowerShell and pinned ILSpy](tools/reference/README.md#host-managed-analysis-tools) before export.
Build the toolchain image, then export the complete library and selected runtime:

```powershell
python -m venv tools/reference/.venv
./tools/reference/.venv/Scripts/python.exe -m pip install -r tools/reference/requirements.txt
docker build --tag thelongdrive-toolchain --file tools/Dockerfile .
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
./scripts/check.ps1 -Test world_frontiers
./scripts/check.ps1 -Filter checkpoint
./scripts/check.ps1 -Test world_frontiers -Filter frontier
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
It first runs `validate-reference` natively on `web/assets/reference/manifest.bin.gz`. The game does not check the manifest, so a manifest that fails a new rule stops the build.
To iterate on a bug, use `./scripts/build.ps1 -Fast`. It skips LTO, wasm-opt and Brotli, so a rebuild takes about 1 minute.
Fast packages are larger and slower. Do not use them for F3 numbers or test passes.
The build script also prepares Brotli downloads in `.cache/http-compression`. Keep that directory with the server deployment.
After replacing an export without rebuilding WASM, prepare its downloads and restart the server:

```powershell
node scripts/prepare-http-compression.mjs
```

The server negotiates Brotli for WASM and current package JSON. It keeps images and package receipts unchanged.

## Host on Cloudflare Pages

Cloudflare Pages serves the game from a free global CDN. Multiplayer still uses the public PeerJS servers.
Make a release build first. Enable R2 in the Cloudflare dashboard. Then prepare the account once:

```powershell
npx wrangler@4.147.0 login
npx wrangler@4.147.0 pages project create thelongdrive --production-branch master
npx wrangler@4.147.0 r2 bucket create thelongdrive-large
npx wrangler@4.147.0 d1 create thelongdrive-events
```

Put the database ID in [wrangler.toml](wrangler.toml) if you make a new account.

If Wrangler offers to make a Worker instead of a Pages project, refuse. Add `--force` to make the Pages project.
Upload the game:

```powershell
node scripts/deploy.mjs
```

The script stages `web/` in `.cache/deploy`. It keeps the app, the reference manifest, and the catalog files of the current runtime package.
It copies each WASM package to `pkg/<hash>/` and rewrites the `./pkg/` imports to that path. Browsers cache `pkg/` and `assets/runtime/` as immutable.
Pages accepts files up to 25 MiB. The script puts larger files, such as the main WASM, in the R2 bucket under their SHA-256. It stops if a larger file is outside these two folders.
It also puts the Brotli files of the 4 WASM files in R2. It makes them first if a fast build skipped them.
It makes a Brotli 11 file of each runtime package file in `.cache/package-brotli` and keeps the files that shrink by 10% or more.
Cloudflare does not compress `.bc`, `.bin`, `.smip` and `.gltf` files, so these Brotli files go to R2 too.
The first run compresses the full package and takes several minutes. Later runs reuse the files of unchanged sources.
The function sends Brotli files with `Content-Encoding: br` when the browser accepts Brotli. Other browsers get the raw file.
`_routes.json` sends all of `pkg/` and `assets/runtime/` to the function, because Pages accepts only 100 route rules.
So each runtime file download is one function request. The free plan allows 100,000 function requests per day and 10 million R2 reads per month.
The script uploads each R2 key once and records it in `.cache/r2-uploaded.json`. Delete that file after you empty the bucket.
Each deployment has an `r2.json` map from path to R2 keys. So a rollback in the dashboard also restores its R2 files.
Old R2 files stay. Each upload of a changed main WASM adds about 33 MiB raw plus its Brotli file to the 10 GB free storage.
`functions/[[path]].js` serves them from the same site. [wrangler.toml](wrangler.toml) binds the bucket.
The script stops if the site has more than 20,000 files.
The site opens at `https://thelongdrive.pages.dev`.
The site is public. `web/og.png` is the link preview image.

### Analytics

The game sends a few events to `/api/event` on the hosted site only: start, ready (load time, GPU, quality), a heartbeat every 5 minutes (play time, FPS, worst frame, multiplayer role), end and start-up failure.
`functions/api/event.js` adds the country and user agent, and stores each event in the D1 database `thelongdrive-events`.
The deploy script applies [migrations](migrations) before the upload. Print a summary of the last 30 days:

```powershell
node scripts/analytics.mjs 30
```

For page visits and load timings, enable Web Analytics once: Workers & Pages, `thelongdrive`, Metrics, Web Analytics, Enable.

## FPS bench

The bench records F3 values in fixed scenes: garage at noon, road 2 km out at noon, first road building at night.
Serve a release build on port 4173. Run `npm ci` once. Then run:

```powershell
node tests/bench/fps.mjs --label master
node tests/bench/fps.mjs compare tests/bench/out/A.json tests/bench/out/B.json
```

It opens headed Chrome on the real GPU with a fresh profile, so all settings are defaults.
Chrome runs without vsync and frame rate limit, so FPS can go above the display rate.
Each scene prints frame ms of all frames in play: mean, p50, p99 and max. Compare frame ms and p99, not FPS.
Options: `--seed`, `--scenes garage,road,town`, `--town NAME` (a building name part), `--settle 10`, `--seconds 20`, `--trace`.
`--trace` writes a Chrome trace of the last scene. Results go to `tests/bench/out/`.
`draws` is the mean count of WebGPU draws per frame, for all views and shadows. `drawsMax` is the most in one frame.
The bench fails when a scene has a p90 of `draws` above `--draw-budget` (default 2700, the top of the GTA V range). `0` turns the check off.
A `?sandbox` page has `window.gameSandbox(actions)` with the actions of `crates/simulation/src/sandbox.rs`.

## Development asset library

The export holds the full build 11140890 library. Startup loads only the selected runtime assets.
Read [export stages](tools/reference/README.md) and [support limits](tools/reference/README.md#development-support-limits) before you use an asset.
Library assets do not add gameplay.

## Controls

| Control | On foot | In the driver seat |
| --- | --- | --- |
| WASD | Move | Accelerate, brake, steer |
| Mouse | Look | Look |
| Shift | Run |  |
| Space | Jump | Toggle handbrake |
| E | Use cap, switch, or seat | Use control or leave seat |
| Hold LMB + mouse | Move an aimed mechanism | Move an aimed mechanism |
| F | Carry or remove a part. Release F to install. Tap F to drop |  |
| Hold F, then release | Charge and throw a held item |  |
| Hold F at the car body | Push while held |  |
| F at the steering grip | Hold the grip. WASD pushes and steers. F releases |  |
| Mouse wheel / Tab | Change carry distance / recenter |  |
| Hold MMB / RMB + mouse | Rotate / move held item |  |
| Hold MMB and RMB + mouse | Roll held item |  |
| X | Split a wheel |  |
| Hold LMB | Fill an open container | Use an aimed switch |
| I |  | Tap for ignition. Hold for the starter |
| Left Ctrl / Right Ctrl / Numpad 0 | Crouch with Ctrl | Clutch |
| Up arrow / Down arrow |  | Shift up / down |
| R / Numpad + |  | Select reverse |
| 0 / Numpad - / N |  | Select neutral |
| 1 to 6 / Numpad 1 to 6 |  | Select a forward gear, when available |
| L |  | Cycle headlights: off, low beam, high beam |
| H / Numpad 8 |  | Horn |
| Hold V | Sleep, when eligible | Sleep, when eligible |
| Hold P | Urinate | Urinate |
| B | Burp or fart | Burp or fart |
| C |  | Switch first person / chase view |
| Backspace | Hide or show feedback | Hide or show feedback |
| Esc | Pause menu | Pause menu |

Remove the hubcap before removing its wheel. Removed parts enter your hands.
Move a held part close to a compatible empty slot. Tap F when the install prompt appears.
Aim at an empty wheel slot to install a wheel.
Some source items, including the fuel can, cannot rotate.

Carry a fuel can. Open the fuel cap. Aim at the opening and hold LMB to fill it.
Direct fill moves fluid from the carried container into the selected open container.
Aim at an open supply valve to fill the supply from your carried container.
An open container spills when it tilts. Supplies keep their current mixture and stock.
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
