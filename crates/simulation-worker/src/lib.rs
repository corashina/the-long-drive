//! Web Worker entry for the simulation. It owns its own WASM memory and the game runtime.
use simulation::boundary::{SimInit, encode_startup};
use simulation::reference::ReferenceManifest;
use simulation::session::Session;
use simulation::world::{GameMode, GameOptions};
use std::cell::RefCell;
use std::sync::Arc;
use wasm_bindgen::prelude::*;

thread_local! {
    static SESSION: RefCell<Option<Session>> = const { RefCell::new(None) };
}

/// Starts the simulation. Returns the encoded `SimStartup`.
#[wasm_bindgen]
pub fn start(
    manifest: &[u8],
    manifest_sha256: String,
    draw_distance: f32,
    biome: u32,
    start_car: i32,
    seed: i32,
    options: u32,
    save: Vec<u8>,
) -> Result<Vec<u8>, JsValue> {
    console_error_panic_hook::set_once();
    let error = |e: &dyn std::fmt::Display| JsValue::from_str(&e.to_string());
    let reference =
        Arc::new(ReferenceManifest::from_runtime_json(manifest).map_err(|e| error(&e))?);
    let init = SimInit {
        mode: GameMode::from_menu(&reference.world, biome, start_car, seed),
        options: GameOptions::from_bits(options),
        draw_distance,
        manifest_sha256,
        save,
    };
    let (session, startup) = Session::new(reference, init).map_err(|e| error(&e))?;
    SESSION.with_borrow_mut(|s| *s = Some(session));
    Ok(encode_startup(&startup))
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
    SESSION.with_borrow_mut(|session| {
        let session = session
            .as_mut()
            .ok_or_else(|| JsValue::from_str("The simulation is not started."))?;
        session
            .tick(command)
            .map(<[u8]>::to_vec)
            .map_err(|e| JsValue::from_str(&e.0))
    })
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
