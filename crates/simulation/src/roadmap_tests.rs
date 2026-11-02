//! Headless tests of the ROADMAP test pass rows, one module per section. See `roadmap_tests/README.md`.
#[path = "roadmap_tests/known_failures.rs"]
mod known_failures;
#[path = "roadmap_tests/multiplayer.rs"]
mod multiplayer;
