# Headless scenario tests

Headless scenario tests from the former ROADMAP rows. One file per area.
They read the installed export (`web/assets/reference/manifest.json`) and are `#[ignore]`.
Use the kit in `world/stream_tests/kit.rs`: `use crate::world::stream::tests::kit::*;`.

Run a section:

```
cargo +1.99 test --release -p simulation --lib -- --ignored roadmap_tests::<file>::
```

Run one row: `cargo +1.99 test --release -p simulation --lib -- --ignored tp_<row id>_`, for example `tp_mp2_`.

Release only. Debug builds overflow the stack.

Name each test `tp_<row id>_<claim>`, for example `tp_mp2_the_guest_starts_where_the_host_stood_and_in_its_seat`.
Each test stays under 60 s in release. The kit uses the default game seed and the source fixed step.

| File | Rows | Section |
| --- | --- | --- |
| `multiplayer` | MP | Multiplayer |
| `known_failures` | K | Known failures |
| `start_items` | SI | Start house items |
