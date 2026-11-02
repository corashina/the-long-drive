//! JS host functions. They are globals in the window and in a worker.
use wasm_bindgen::prelude::*;
#[wasm_bindgen]
extern "C" {
    #[wasm_bindgen(js_name = gameNow)]
    pub fn game_now() -> f64;
    #[wasm_bindgen(js_name = gameCpuInputConsumed)]
    pub fn game_cpu_input_consumed();
    #[wasm_bindgen(js_name = gameCpuClearInput)]
    pub fn game_cpu_clear_input();
    #[wasm_bindgen(js_name = Object)]
    pub type RoadTransport;
    #[wasm_bindgen(catch, js_name = gameRoadCreate)]
    pub fn game_road_create() -> Result<RoadTransport, JsValue>;
    #[wasm_bindgen(method, structural, catch, js_name = initialized)]
    pub fn road_initialized(this: &RoadTransport) -> Result<bool, JsValue>;
    #[wasm_bindgen(method, structural, catch, js_name = capacityLimit)]
    pub fn road_capacity_limit(this: &RoadTransport, bytes: f64) -> Result<(), JsValue>;
    #[wasm_bindgen(method, structural, catch, js_name = installRaw)]
    pub fn road_install(
        this: &RoadTransport,
        identity: &[u8],
        input: &[u8],
        layout: &[u32],
    ) -> Result<(), JsValue>;
    #[wasm_bindgen(method, structural, catch, js_name = submitRaw)]
    pub fn road_submit(this: &RoadTransport, identity: &[u8], input: &[u8]) -> Result<(), JsValue>;
    #[wasm_bindgen(method, structural, catch, js_name = poll)]
    pub fn road_poll(this: &RoadTransport) -> Result<i32, JsValue>;
    #[wasm_bindgen(method, structural, js_name = wasmBytes)]
    pub fn road_wasm_bytes(this: &RoadTransport) -> f64;
    #[wasm_bindgen(method, structural, js_name = sourceOwner)]
    pub fn road_source_owner(this: &RoadTransport) -> u32;
    #[wasm_bindgen(method, structural, catch, js_name = readRaw)]
    pub fn road_read(
        this: &RoadTransport,
        origin: &mut [f64],
        vertices: &mut [f32],
        normals: &mut [f32],
        tangents: &mut [f32],
        uv: &mut [f32],
        triangles: &mut [u32],
        submeshes: &mut [u32],
    ) -> Result<(), JsValue>;
    #[wasm_bindgen(method, structural, js_name = discard)]
    pub fn road_discard(this: &RoadTransport);
    #[wasm_bindgen(method, structural, js_name = dispose)]
    pub fn road_dispose(this: &RoadTransport);
    #[wasm_bindgen(method, structural, js_name = packingStarted)]
    pub fn road_packing_started(this: &RoadTransport);
    #[wasm_bindgen(method, structural, js_name = packingEnded)]
    pub fn road_packing_ended(this: &RoadTransport);
    #[wasm_bindgen(method, structural, js_name = publicationStarted)]
    pub fn road_publication_started(this: &RoadTransport);
    #[wasm_bindgen(method, structural, js_name = publicationEnded)]
    pub fn road_publication_ended(this: &RoadTransport, accepted: bool);
    #[wasm_bindgen(js_name = gameTerrainSubmit)]
    pub fn game_terrain_submit(input: &[u8]) -> u32;
    /// -1 idle, -2 busy, else the id of the ready result. Worker failures throw.
    #[wasm_bindgen(catch, js_name = gameTerrainPoll)]
    pub fn game_terrain_poll() -> Result<i32, JsValue>;
    #[wasm_bindgen(js_name = gameTerrainVertexCount)]
    pub fn game_terrain_vertex_count() -> u32;
    #[wasm_bindgen(js_name = gameTerrainTriangleCount)]
    pub fn game_terrain_triangle_count() -> u32;
    #[wasm_bindgen(js_name = gameTerrainRead)]
    pub fn game_terrain_read(
        vertices: &mut [f32],
        normals: &mut [f32],
        tangents: &mut [f32],
        uv: &mut [f32],
        colors: &mut [f32],
        triangles: &mut [u32],
    );
    #[wasm_bindgen(js_name = gameTerrainDiscard)]
    pub fn game_terrain_discard();
}
