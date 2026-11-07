//! Crunch worker entry. It unpacks stored crunch files to BC blocks off the main thread.
use wasm_bindgen::prelude::*;
use wasm_heap as _;

/// BC blocks of the levels in `file`. `first` is the level of the first data after the tables.
#[wasm_bindgen]
pub fn unpack(file: &[u8], first: u32) -> Result<Vec<u8>, JsError> {
    crunch::unpack(file, first).map_err(|error| JsError::new(&error))
}
