//! Grass worker entry. Immutable input and planned output belong to this WASM instance.
use grass_mesh::{GrassDomain, GrassExpansion, GrassMeshData, GrassPatch, GrassSettings};
use std::{cell::RefCell, collections::BTreeMap, sync::Arc};
use wasm_bindgen::prelude::*;

struct Dataset {
    patches: Arc<[GrassPatch]>,
    settings: GrassSettings,
    domains: Arc<[GrassDomain; 4]>,
}
#[derive(Default)]
struct State {
    datasets: BTreeMap<u64, Dataset>,
    domains: Option<Arc<[GrassDomain; 4]>>,
    owner: Option<u64>,
    task: Option<GrassExpansion>,
}
thread_local! {
    static STATE: RefCell<State> = RefCell::new(State::default());
}
fn error(message: String) -> JsError {
    JsError::new(&message)
}

#[wasm_bindgen]
pub fn install(owner: u64, input: &[u8]) -> Result<(), JsError> {
    STATE.with(|state| {
        let mut state = state.borrow_mut();
        if state.datasets.contains_key(&owner) {
            return Err(JsError::new("Grass source is already installed"));
        }
        let (patches, settings) = grass_mesh::decode_dataset(input).map_err(error)?;
        if state.domains.is_none() {
            let domains = [
                GrassDomain::new(2).map_err(error)?,
                GrassDomain::new(4).map_err(error)?,
                GrassDomain::new(8).map_err(error)?,
                GrassDomain::new(16).map_err(error)?,
            ];
            state.domains = Some(Arc::new(domains));
        }
        let domains = Arc::clone(state.domains.as_ref().unwrap());
        state.datasets.insert(
            owner,
            Dataset {
                patches: patches.into(),
                settings,
                domains,
            },
        );
        Ok(())
    })
}
/// JavaScript reads the exact shared reservation before acknowledging emission.
#[wasm_bindgen]
pub struct GrassAllocation(grass_mesh::GrassAllocation);
#[wasm_bindgen]
impl GrassAllocation {
    pub fn vertices(&self) -> usize {
        self.0.vertices
    }
    pub fn indices(&self) -> usize {
        self.0.indices
    }
    pub fn scratch_bytes(&self) -> usize {
        self.0.scratch_bytes
    }
    pub fn output_bytes(&self) -> usize {
        self.0.output_bytes
    }
    pub fn gpu_bytes(&self) -> usize {
        self.0.gpu_bytes
    }
}
#[wasm_bindgen]
pub fn plan(owner: u64, input: &[u8]) -> Result<GrassAllocation, JsError> {
    STATE.with(|state| {
        let mut state = state.borrow_mut();
        if state.task.is_some() {
            return Err(JsError::new("Grass plan is already owned"));
        }
        let view = grass_mesh::decode_view(input).map_err(error)?;
        let dataset = state
            .datasets
            .get(&owner)
            .ok_or_else(|| JsError::new("Grass source is not installed"))?;
        // Main reserves planning scratch before dispatch, then reserves output after this reply.
        let mut task = GrassExpansion::new(
            view,
            dataset.settings,
            Arc::clone(&dataset.patches),
            Arc::clone(&dataset.domains),
            |_| Ok(()),
        )
        .map_err(error)?;
        let allocation = loop {
            if let Some(allocation) = task.plan().map_err(error)? {
                break allocation;
            }
        };
        state.task = Some(task);
        state.owner = Some(owner);
        Ok(GrassAllocation(allocation))
    })
}
/// Each getter moves its attribute out once.
#[wasm_bindgen]
pub struct GrassSurface(GrassMeshData);
#[wasm_bindgen]
impl GrassSurface {
    pub fn anchors(&mut self) -> Vec<f32> {
        std::mem::take(&mut self.0.anchors).into_flattened()
    }
    pub fn uvs(&mut self) -> Vec<f32> {
        std::mem::take(&mut self.0.uvs).into_flattened()
    }
    pub fn blades(&mut self) -> Vec<f32> {
        std::mem::take(&mut self.0.blades).into_flattened()
    }
    pub fn patches(&mut self) -> Vec<u32> {
        std::mem::take(&mut self.0.patches)
    }
    pub fn indices(&mut self) -> Vec<u32> {
        std::mem::take(&mut self.0.indices)
    }
}
/// Calling evaluate acknowledges main-thread admission of this worker's current plan.
#[wasm_bindgen]
pub fn evaluate() -> Result<GrassSurface, JsError> {
    STATE.with(|state| {
        let mut state = state.borrow_mut();
        state.owner = None;
        let mut task = state
            .task
            .take()
            .ok_or_else(|| JsError::new("Grass plan is not awaiting approval"))?;
        task.approve().map_err(error)?;
        while !task.prepare().map_err(error)? {}
        let (_, mesh, _) = task.finish();
        Ok(GrassSurface(mesh))
    })
}

#[wasm_bindgen]
pub fn discard() {
    STATE.with(|state| {
        let mut s = state.borrow_mut();
        s.task = None;
        s.owner = None;
    });
}
#[wasm_bindgen]
pub fn retire(owner: u64) -> Result<(), JsError> {
    STATE.with(|state| {
        let mut s = state.borrow_mut();
        if s.owner == Some(owner) {
            return Err(JsError::new("Grass dataset has an owned plan"));
        }
        s.datasets
            .remove(&owner)
            .ok_or_else(|| JsError::new("Grass dataset is not installed"))?;
        Ok(())
    })
}
