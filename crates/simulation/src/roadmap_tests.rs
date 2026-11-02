//! Headless tests of the ROADMAP test pass rows, one module per section. See `roadmap_tests/README.md`.
#[path = "roadmap_tests/advanced_rendering.rs"]
mod advanced_rendering;
#[path = "roadmap_tests/blocked.rs"]
mod blocked;
#[path = "roadmap_tests/car_camera.rs"]
mod car_camera;
#[path = "roadmap_tests/car_parts.rs"]
mod car_parts;
#[path = "roadmap_tests/creatures.rs"]
mod creatures;
#[path = "roadmap_tests/known_failures.rs"]
mod known_failures;
#[path = "roadmap_tests/languages.rs"]
mod languages;
#[path = "roadmap_tests/more_cars.rs"]
mod more_cars;
#[path = "roadmap_tests/multiplayer.rs"]
mod multiplayer;
#[path = "roadmap_tests/outfit.rs"]
mod outfit;
#[path = "roadmap_tests/player_body.rs"]
mod player_body;
#[path = "roadmap_tests/player_movement.rs"]
mod player_movement;
#[path = "roadmap_tests/saves.rs"]
mod saves;
#[path = "roadmap_tests/settings.rs"]
mod settings;
#[path = "roadmap_tests/small_effects.rs"]
mod small_effects;
#[path = "roadmap_tests/start_menu.rs"]
mod start_menu;
#[path = "roadmap_tests/starting_area.rs"]
mod starting_area;
#[path = "roadmap_tests/weapons.rs"]
mod weapons;
#[path = "roadmap_tests/world.rs"]
mod world;
