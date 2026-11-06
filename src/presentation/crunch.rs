//! Crunch textures unpack to BC blocks in workers (`web/crunch.js`).
use std::io::{Error, ErrorKind};
use wasm_bindgen::prelude::*;
#[wasm_bindgen]
extern "C" {
    /// Copies `file` and returns a promise of the BC blocks.
    #[wasm_bindgen(catch, js_namespace=window, js_name=crunchUnpack)]
    fn crunch_unpack(file: &[u8], first: u32) -> Result<js_sys::Promise, JsValue>;
}
/// Writes the BC blocks of the levels in a stored crunch file to `out`.
/// `first` is the level of the first data after the tables.
pub async fn unpack(file: &[u8], first: u32, out: &mut [u8]) -> Result<(), Error> {
    let failed = |e: JsValue| Error::new(ErrorKind::InvalidData, format!("Crunch unpack: {e:?}"));
    let promise = crunch_unpack(file, first).map_err(failed)?;
    let blocks: js_sys::Uint8Array = wasm_bindgen_futures::JsFuture::from(promise)
        .await
        .map_err(failed)?
        .unchecked_into();
    if blocks.length() as usize != out.len() {
        return Err(Error::new(
            ErrorKind::InvalidData,
            "Crunch BC bytes differ from the texture levels",
        ));
    }
    blocks.copy_to(out);
    Ok(())
}
