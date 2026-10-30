//! Web Worker entry for source terrain tile jobs. It owns its own WASM memory.
use wasm_bindgen::prelude::*;

/// Each getter moves one attribute out once, as a flat array.
#[wasm_bindgen]
pub struct TerrainSurface(terrain::TerrainMesh);

#[wasm_bindgen]
impl TerrainSurface {
    pub fn vertices(&mut self) -> Vec<f32> {
        std::mem::take(&mut self.0.vertices).into_flattened()
    }
    pub fn normals(&mut self) -> Vec<f32> {
        std::mem::take(&mut self.0.normals).into_flattened()
    }
    pub fn tangents(&mut self) -> Vec<f32> {
        std::mem::take(&mut self.0.tangents).into_flattened()
    }
    pub fn uv(&mut self) -> Vec<f32> {
        std::mem::take(&mut self.0.uv).into_flattened()
    }
    pub fn colors(&mut self) -> Vec<f32> {
        std::mem::take(&mut self.0.colors).into_flattened()
    }
    pub fn triangles(&mut self) -> Vec<u32> {
        std::mem::take(&mut self.0.triangles).into_flattened()
    }
}

#[wasm_bindgen]
pub fn evaluate(input: &[u8]) -> Result<TerrainSurface, JsError> {
    terrain::evaluate(input)
        .map(TerrainSurface)
        .map_err(|error| JsError::new(&error))
}
