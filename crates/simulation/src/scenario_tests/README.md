# Headless scenario tests

These headless scenario tests check whole game areas. Each area has one file.
The tests read the installed export (`web/assets/reference/manifest.bin.gz`) and are `#[ignore]`.
Use the kit in `world/stream_tests/kit.rs`: `use crate::world::stream::tests::kit::*;`.

Run an area:

```
cargo +1.99 test --release -p simulation --lib -- --ignored scenario_tests::<file>::
```

Run the tests in release builds only. Debug builds overflow the stack.

Name each test by its claim, for example `multiplayer::a_joining_guest_starts_in_the_garage`.
Do not put roadmap IDs in names.
Each test stays under 60 s in release. The kit uses the default game seed and the source fixed step.

| File | Area |
| --- | --- |
| `driving` | Driving and the streamed world |
| `interaction` | The use ray, the hand and seats |
| `multiplayer` | Multiplayer |
| `save` | Saves |
| `start_house` | Start house items |
