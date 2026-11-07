//! Web Worker entry for the simulation. It owns its own WASM memory and the game runtime.
use simulation::boundary::SimInit;
use simulation::reference::ReferenceManifest;
use simulation::session::Session;
use simulation::world::{GameMode, GameOptions};
use std::cell::RefCell;
use std::sync::Arc;
use wasm_bindgen::prelude::*;
use wasm_heap as _;

thread_local! {
    static REFERENCE: RefCell<Option<Arc<ReferenceManifest>>> = const { RefCell::new(None) };
    static SESSION: RefCell<Option<Session>> = const { RefCell::new(None) };
}

fn error(e: &dyn std::fmt::Display) -> JsValue {
    JsValue::from_str(&e.to_string())
}

/// Parses and checks the manifest. It runs while the main thread sets up the interface.
#[wasm_bindgen]
pub fn load(manifest: &[u8]) -> Result<(), JsValue> {
    console_error_panic_hook::set_once();
    let reference = ReferenceManifest::from_runtime(manifest).map_err(|e| error(&e))?;
    REFERENCE.set(Some(Arc::new(reference)));
    Ok(())
}

/// Starts the simulation on the manifest of `load`.
#[wasm_bindgen]
pub fn start(
    manifest_sha256: String,
    draw_distance: f32,
    biome: u32,
    start_car: i32,
    seed: i32,
    options: u32,
    save: Vec<u8>,
) -> Result<(), JsValue> {
    let reference = REFERENCE
        .take()
        .ok_or_else(|| JsValue::from_str("The manifest is not loaded."))?;
    let init = SimInit {
        mode: GameMode::from_menu(&reference.world, biome, start_car, seed),
        options: GameOptions::from_bits(options),
        draw_distance,
        manifest_sha256,
        save,
    };
    let session = Session::new(reference, init).map_err(|e| error(&e))?;
    SESSION.with_borrow_mut(|s| *s = Some(session));
    Ok(())
}

/// Why the save of `start` did not load. Empty: it loaded, or there was none.
#[wasm_bindgen]
pub fn load_error() -> String {
    SESSION.with_borrow(|s| {
        s.as_ref()
            .and_then(Session::load_error)
            .unwrap_or_default()
            .to_owned()
    })
}

/// Runs one encoded command. Returns the encoded frame.
#[wasm_bindgen]
pub fn tick(command: &[u8]) -> Result<Vec<u8>, JsValue> {
    with_session(|s| {
        s.tick(command)
            .map(<[u8]>::to_vec)
            .map_err(|e| JsValue::from_str(&e.0))
    })?
}

/// Runs a JSON array of sandbox actions (`simulation::sandbox`). Returns the JSON replies.
/// `?sandbox` pages offer all of it. The F4 weather panel uses it in every build.
#[wasm_bindgen]
pub fn sandbox(json: &str) -> Result<String, JsValue> {
    with_session(|s| s.sandbox(json))
}

fn with_session<T>(f: impl FnOnce(&mut Session) -> T) -> Result<T, JsValue> {
    SESSION.with_borrow_mut(|session| {
        session
            .as_mut()
            .map(f)
            .ok_or_else(|| JsValue::from_str("The simulation is not started."))
    })
}

/// Starts or ends a multiplayer session. `role`: 0 off, 1 host, 2 guest.
#[wasm_bindgen]
pub fn net_session(role: u8, slot: u8) -> Result<(), JsValue> {
    with_session(|s| s.net_session(role, slot))
}

/// Length-prefixed peer packets for the next tick.
#[wasm_bindgen]
pub fn net_receive(packets: &[u8]) -> Result<(), JsValue> {
    with_session(|s| s.net_receive(packets))
}

/// A peer left.
#[wasm_bindgen]
pub fn net_leave(slot: u8) -> Result<(), JsValue> {
    with_session(|s| s.net_leave(slot))
}

/// Length-prefixed packets of the last tick for peers.
#[wasm_bindgen]
pub fn net_send() -> Result<Vec<u8>, JsValue> {
    with_session(|s| s.net_send().to_vec())
}
