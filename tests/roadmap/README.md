# Roadmap runs

Scripts that run the ROADMAP test pass rows of one section in one game tab.
They use the `?sandbox` helpers of `tests/sandbox/`. One run loads the page once and starts one New game.

## Run

1. Run `npm install` once. It installs `playwright-core`. The run uses the installed Chrome.
2. Serve the game, for example on port 4174.
3. Run a section, or some rows of it:

```
node tests/roadmap/run.mjs creatures
node tests/roadmap/run.mjs creatures 3 9
node tests/roadmap/run.mjs car-camera
node tests/roadmap/run.mjs more-cars
node tests/roadmap/run.mjs saves --url=http://127.0.0.1:4173/?sandbox
```

`--chrome=PATH` uses another Chrome executable. Do not use the mouse or keyboard in the window during a run.

## Results

Each row prints one result with its evidence:

- `PASS`: the state checks pass.
- `FAIL`: a check failed, or the row stopped with an error. An error saves a screenshot.
- `LOOK`: the state checks pass. Look at the screenshot, or listen, for the rest of the row.
- `SKIP`: the row needs a manual check.

The run writes `report.md`, `report.json` and the screenshots to `tests/roadmap/out/<section>-<time>/`.
Remove a ROADMAP row after `PASS`, or after `LOOK` when the screenshot is good.

## Add a section

Add `<section>.mjs` with `{name, query, rows}` and add its name to `SECTIONS` in `run.mjs`.
`query` adds URL flags, such as `date=10-20`. Each row is `{title, seconds, run(game)}`. Row keys are the ROADMAP row numbers.
`run` answers `pass`, `fail`, `look` or `skip` from `game.mjs`. `Game` has the sandbox calls, `state()`, `saveLoad()`, keys, `press(/action/)` for bound keys and `shot()`.
