# ROADMAP test pass rows

Headless scenario tests of the ROADMAP test pass rows. One file per section.
They read the installed export (`web/assets/reference/manifest.json`) and are `#[ignore]`.
Use the kit in `world/stream_tests/kit.rs`: `use crate::world::stream::tests::kit::*;`.

Run a section:

```
cargo +1.99 test --release -p simulation --lib -- --ignored roadmap_tests::<file>::
```

Run one row: `cargo +1.99 test --release -p simulation --lib -- --ignored tp_<row id>_`, for example `tp_cr3_`.

Release only. Debug builds overflow the stack.

Name each test `tp_<row id>_<claim>`, for example `tp_cr3_rabbits_spawn_and_unload`.
Each test stays under 60 s in release. The kit uses the default game seed and the source fixed step.

| File | Rows | Section |
| --- | --- | --- |
| `starting_area` | SA | Starting area |
| `car_parts` | CP | Car parts and repair |
| `world` | W | World and terrain |
| `more_cars` | MC | More cars |
| `car_camera` | CC | Car camera and crash damage |
| `saves` | S | Saves |
| `settings` | ST | Settings |
| `languages` | L | Languages and help text |
| `start_menu` | SM | Start menu figure |
| `player_body` | PB | Player body and outfit |
| `player_movement` | PM | Player movement |
| `weapons` | WD | Weapons and damage |
| `creatures` | CR | Creatures |
| `small_effects` | SE | Small effects |
| `advanced_rendering` | AR | Advanced rendering |
| `multiplayer` | MP | Multiplayer |
| `known_failures` | K | Known failures |
| `blocked` | SA, CP, PB, WD | Rows from the blocked checks |
| `outfit` | PB | Hands and glasses |
