//! Road worker entry. Source data belongs to this WASM instance.
use std::cell::RefCell;
use wasm_bindgen::prelude::*;

thread_local! {
    static PROTOTYPES: RefCell<[Option<road_mesh::RoadPrototype>; 3]> = const { RefCell::new([None, None, None]) };
}

#[wasm_bindgen]
pub fn install(owner: u32, input: &[u8]) -> Result<(), JsError> {
    if owner >= 3 {
        return Err(JsError::new("Invalid road source owner"));
    }
    if PROTOTYPES.with(|slots| slots.borrow()[owner as usize].is_some()) {
        return Err(JsError::new("Road source slot is already installed"));
    }
    let prototype = road_mesh::decode_prototype(input).map_err(|error| JsError::new(&error))?;
    PROTOTYPES.with(|slots| slots.borrow_mut()[owner as usize] = Some(prototype));
    Ok(())
}

/// Each getter moves its attribute out once.
#[wasm_bindgen]
pub struct RoadSurface(road_mesh::RoadMesh, f64, f64);

#[wasm_bindgen]
extern "C" {
    #[wasm_bindgen(js_name = roadCpuNow)]
    fn cpu_now() -> f64;
}

#[wasm_bindgen]
impl RoadSurface {
    pub fn decode_ms(&self) -> f64 {
        self.1
    }
    pub fn compute_ms(&self) -> f64 {
        self.2
    }
    pub fn origin(&mut self) -> Vec<f64> {
        std::mem::take(&mut self.0.origin).to_vec()
    }
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
    pub fn triangles(&mut self) -> Vec<u32> {
        std::mem::take(&mut self.0.triangles).into_flattened()
    }
    pub fn submeshes(&mut self) -> Vec<u32> {
        std::mem::take(&mut self.0.submeshes)
            .into_iter()
            .flat_map(|range| {
                [
                    u32::try_from(range.start).expect("admitted road range"),
                    u32::try_from(range.end).expect("admitted road range"),
                ]
            })
            .collect()
    }
}

#[wasm_bindgen]
pub fn evaluate(owner: u32, input: &[u8], profile: bool) -> Result<RoadSurface, JsError> {
    let started = if profile { cpu_now() } else { 0.0 };
    let pose = road_mesh::decode_pose(input).map_err(|error| JsError::new(&error))?;
    let decoded = if profile { cpu_now() } else { 0.0 };
    PROTOTYPES.with(|slots| {
        let slots = slots.borrow();
        let prototype = slots
            .get(owner as usize)
            .and_then(Option::as_ref)
            .ok_or_else(|| JsError::new("Road source is not installed"))?;
        let mesh = road_mesh::deform(prototype, &pose);
        let computed = if profile { cpu_now() } else { 0.0 };
        Ok(RoadSurface(mesh, decoded - started, computed - decoded))
    })
}
