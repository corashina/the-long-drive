//! One grass actor belongs to one presentation world. No Bevy ownership enters this client.
use crate::reference::ReferenceError;
use grass_mesh::{GrassAllocation, GrassMeshData, GrassView};
#[cfg(not(target_arch = "wasm32"))]
use grass_mesh::{GrassDomain, GrassExpansion, GrassPatch, GrassSettings};
#[cfg(not(target_arch = "wasm32"))]
use std::{collections::BTreeMap, sync::Arc};

// scripts/build.ps1 sets the grass package linker initial memory to this value.
pub const INITIAL_WASM_BYTES: usize = 2 * 1024 * 1024;
pub const DOMAIN_BYTES: usize =
    std::mem::size_of::<[grass_mesh::GrassDomain; 4]>() + 304 * 12 + 510 * 6 + 32;
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GrassJobId {
    pub session: u64,
    pub revision: u64,
    pub entity: u64,
}
impl GrassJobId {
    #[cfg(target_arch = "wasm32")]
    fn bytes(self) -> [u8; 24] {
        let mut b = [0; 24];
        b[..8].copy_from_slice(&self.session.to_le_bytes());
        b[8..16].copy_from_slice(&self.revision.to_le_bytes());
        b[16..].copy_from_slice(&self.entity.to_le_bytes());
        b
    }
}
#[derive(Clone, Copy, Debug)]
pub enum GrassPoll {
    Idle,
    Busy,
    Planned(GrassJobId, GrassAllocation),
    Ready(GrassJobId),
}
/// One job at a time. State names match `web/grass.js`.
#[derive(Clone, Copy, Debug, Default)]
enum GrassJob {
    #[default]
    Idle,
    Planning(GrassJobId),
    Planned(GrassJobId, GrassAllocation),
    Emitting(GrassJobId, GrassAllocation),
    Ready(GrassJobId, GrassAllocation),
    Canceling(GrassJobId),
}
impl GrassJob {
    fn id(self) -> Option<GrassJobId> {
        match self {
            Self::Idle => None,
            Self::Planning(id)
            | Self::Planned(id, _)
            | Self::Emitting(id, _)
            | Self::Ready(id, _)
            | Self::Canceling(id) => Some(id),
        }
    }
}
#[derive(Default)]
pub struct GrassWorker {
    job: GrassJob,
    last_revision: u64,
    #[cfg(target_arch = "wasm32")]
    transport: Option<crate::browser::GrassTransport>,
    #[cfg(not(target_arch = "wasm32"))]
    datasets: BTreeMap<u64, (u64, Arc<[GrassPatch]>, GrassSettings)>,
    #[cfg(not(target_arch = "wasm32"))]
    domains: Option<Arc<[GrassDomain; 4]>>,
    #[cfg(not(target_arch = "wasm32"))]
    task: Option<GrassExpansion>,
}
impl GrassWorker {
    pub fn busy(&self) -> bool {
        !matches!(self.job, GrassJob::Idle)
    }
    pub fn owns(&self, id: GrassJobId) -> bool {
        self.job.id() == Some(id)
    }
    pub fn initialize(&mut self) -> Result<bool, ReferenceError> {
        #[cfg(target_arch = "wasm32")]
        {
            if self.transport.is_none() {
                self.transport = Some(crate::browser::game_grass_create().map_err(js_error)?);
            }
            self.transport
                .as_ref()
                .unwrap()
                .grass_initialized()
                .map_err(js_error)
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            Ok(true)
        }
    }
    pub fn capacity_limit(&self, bytes: usize) -> Result<(), ReferenceError> {
        #[cfg(target_arch = "wasm32")]
        {
            self.transport
                .as_ref()
                .unwrap()
                .grass_capacity_limit(bytes as f64)
                .map_err(js_error)?;
        }
        #[cfg(not(target_arch = "wasm32"))]
        let _ = bytes;
        Ok(())
    }
    pub fn wasm_bytes(&self) -> usize {
        #[cfg(target_arch = "wasm32")]
        {
            self.transport
                .as_ref()
                .map_or(0, |t| t.grass_wasm_bytes() as usize)
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            0
        }
    }
    pub fn install(
        &mut self,
        id: GrassJobId,
        input: &[u8],
        scratch: usize,
    ) -> Result<(), ReferenceError> {
        #[cfg(target_arch = "wasm32")]
        self.transport
            .as_ref()
            .unwrap()
            .grass_install(&id.bytes(), input, scratch as u32)
            .map_err(js_error)?;
        #[cfg(not(target_arch = "wasm32"))]
        {
            let _ = scratch;
            let (p, s) = grass_mesh::decode_dataset(input).map_err(ReferenceError)?;
            if self.datasets.contains_key(&id.entity) {
                return Err(error("Duplicate dataset"));
            }
            if self.domains.is_none() {
                self.domains = Some(Arc::new(
                    [2, 4, 8, 16].map(|f| GrassDomain::new(f).unwrap()),
                ));
            }
            self.datasets.insert(id.entity, (id.session, p.into(), s));
        }
        Ok(())
    }
    pub fn installed(&self, entity: u64) -> Result<bool, ReferenceError> {
        #[cfg(target_arch = "wasm32")]
        {
            self.transport
                .as_ref()
                .unwrap()
                .grass_installed(&entity.to_le_bytes())
                .map_err(js_error)
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            Ok(self.datasets.contains_key(&entity))
        }
    }
    pub fn retiring(&self, entity: u64) -> Result<bool, ReferenceError> {
        #[cfg(target_arch = "wasm32")]
        {
            self.transport
                .as_ref()
                .unwrap()
                .grass_retire(&entity.to_le_bytes())
                .map_err(js_error)
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            let _ = entity;
            Ok(true)
        }
    }
    pub fn remove(&mut self, entity: u64) {
        #[cfg(not(target_arch = "wasm32"))]
        self.datasets.remove(&entity);
        #[cfg(target_arch = "wasm32")]
        let _ = entity;
    }
    pub fn submit(&mut self, id: GrassJobId, view: GrassView) -> Result<(), ReferenceError> {
        if !matches!(self.job, GrassJob::Idle) || id.revision <= self.last_revision {
            return Err(error("Actor is busy"));
        }
        #[cfg(target_arch = "wasm32")]
        {
            self.packing_started()?;
            let mut input = Vec::with_capacity(80);
            grass_mesh::encode_view(view, &mut input);
            self.packing_ended();
            self.transport
                .as_ref()
                .unwrap()
                .grass_submit(&id.bytes(), &input)
                .map_err(js_error)?;
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            let (session, p, s) = self
                .datasets
                .get(&id.entity)
                .ok_or_else(|| error("Dataset is absent"))?;
            if *session != id.session {
                return Err(error("Dataset belongs to another session"));
            }
            self.task = Some(
                GrassExpansion::new(
                    view,
                    *s,
                    Arc::clone(p),
                    Arc::clone(self.domains.as_ref().unwrap()),
                    |_| Ok(()),
                )
                .map_err(ReferenceError)?,
            );
        }
        self.last_revision = id.revision;
        self.job = GrassJob::Planning(id);
        Ok(())
    }
    pub fn poll(&mut self) -> Result<GrassPoll, ReferenceError> {
        #[cfg(target_arch = "wasm32")]
        {
            let Some(t) = &self.transport else {
                return Ok(GrassPoll::Idle);
            };
            match t.grass_poll().map_err(js_error)? {
                -1 => {
                    self.job = GrassJob::Idle;
                    Ok(GrassPoll::Idle)
                }
                -2 => Ok(GrassPoll::Busy),
                0 => {
                    let mut words = [0; 5];
                    t.grass_plan(&mut words).map_err(js_error)?;
                    let p = GrassAllocation {
                        vertices: words[0] as usize,
                        indices: words[1] as usize,
                        scratch_bytes: words[2] as usize,
                        output_bytes: words[3] as usize,
                        gpu_bytes: words[4] as usize,
                    };
                    let (GrassJob::Planning(id) | GrassJob::Planned(id, _)) = self.job else {
                        return Err(error("Plan has no owner"));
                    };
                    self.job = GrassJob::Planned(id, p);
                    Ok(GrassPoll::Planned(id, p))
                }
                1 => {
                    let (GrassJob::Emitting(id, p) | GrassJob::Ready(id, p)) = self.job else {
                        return Err(error("Output has no owner"));
                    };
                    self.job = GrassJob::Ready(id, p);
                    Ok(GrassPoll::Ready(id))
                }
                _ => Err(error("Invalid actor state")),
            }
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            match self.job {
                GrassJob::Idle => Ok(GrassPoll::Idle),
                GrassJob::Canceling(_) => {
                    self.task = None;
                    self.job = GrassJob::Idle;
                    Ok(GrassPoll::Idle)
                }
                GrassJob::Planning(id) | GrassJob::Planned(id, _) => Ok(
                    if let Some(p) = self.task.as_mut().unwrap().plan().map_err(ReferenceError)? {
                        self.job = GrassJob::Planned(id, p);
                        GrassPoll::Planned(id, p)
                    } else {
                        GrassPoll::Busy
                    },
                ),
                GrassJob::Emitting(id, p) | GrassJob::Ready(id, p) => Ok(
                    if self
                        .task
                        .as_mut()
                        .unwrap()
                        .prepare()
                        .map_err(ReferenceError)?
                    {
                        self.job = GrassJob::Ready(id, p);
                        GrassPoll::Ready(id)
                    } else {
                        GrassPoll::Busy
                    },
                ),
            }
        }
    }
    pub fn approve(&mut self, id: GrassJobId, bytes: usize) -> Result<(), ReferenceError> {
        let GrassJob::Planned(owner, plan) = self.job else {
            return Err(error("Invalid plan approval"));
        };
        if owner != id || plan.output_bytes != bytes {
            return Err(error("Invalid plan approval"));
        }
        #[cfg(target_arch = "wasm32")]
        self.transport
            .as_ref()
            .unwrap()
            .grass_approve(&id.bytes(), bytes as f64)
            .map_err(js_error)?;
        #[cfg(not(target_arch = "wasm32"))]
        self.task
            .as_mut()
            .unwrap()
            .approve()
            .map_err(ReferenceError)?;
        self.job = GrassJob::Emitting(id, plan);
        Ok(())
    }
    pub fn consume(&mut self, id: GrassJobId) -> Result<GrassMeshData, ReferenceError> {
        let GrassJob::Ready(owner, p) = self.job else {
            return Err(error("Invalid output owner"));
        };
        if owner != id {
            return Err(error("Invalid output owner"));
        }
        #[cfg(target_arch = "wasm32")]
        let output = {
            let mut o = GrassMeshData {
                anchors: vec![[0.; 3]; p.vertices],
                uvs: vec![[0.; 2]; p.vertices],
                blades: vec![[0.; 4]; p.vertices],
                patches: vec![0; p.vertices],
                indices: vec![0; p.indices],
            };
            self.transport
                .as_ref()
                .unwrap()
                .grass_read(
                    o.anchors.as_flattened_mut(),
                    o.uvs.as_flattened_mut(),
                    o.blades.as_flattened_mut(),
                    &mut o.patches,
                    &mut o.indices,
                )
                .map_err(js_error)?;
            o
        };
        #[cfg(not(target_arch = "wasm32"))]
        let output = {
            let _ = p;
            self.task.take().unwrap().finish().1
        };
        self.job = GrassJob::Idle;
        Ok(output)
    }
    pub fn discard(&mut self) {
        if let Some(id) = self.job.id() {
            self.job = GrassJob::Canceling(id);
        }
        #[cfg(target_arch = "wasm32")]
        if let Some(t) = &self.transport {
            t.grass_discard();
        }
    }
    pub fn packing_started(&self) -> Result<(), ReferenceError> {
        #[cfg(target_arch = "wasm32")]
        if let Some(t) = &self.transport {
            t.grass_packing_started();
        }
        Ok(())
    }
    pub fn packing_ended(&self) {
        #[cfg(target_arch = "wasm32")]
        if let Some(t) = &self.transport {
            t.grass_packing_ended();
        }
    }
    pub fn publication_started(&self, entity: u64) {
        #[cfg(not(target_arch = "wasm32"))]
        let _ = entity;
        #[cfg(target_arch = "wasm32")]
        if let Some(t) = &self.transport {
            t.grass_publication_started(&entity.to_le_bytes());
        }
    }
    pub fn publication_ended(&self, entity: u64, accepted: bool) {
        #[cfg(target_arch = "wasm32")]
        if let Some(t) = &self.transport {
            t.grass_publication_ended(&entity.to_le_bytes(), accepted);
        }
        #[cfg(not(target_arch = "wasm32"))]
        let _ = (entity, accepted);
    }
    pub fn dispose(&mut self) {
        #[cfg(target_arch = "wasm32")]
        if let Some(t) = self.transport.take() {
            t.grass_dispose();
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            self.datasets.clear();
            self.domains = None;
            self.task = None;
        }
        self.job = GrassJob::Idle;
    }
}
impl Drop for GrassWorker {
    fn drop(&mut self) {
        self.dispose();
    }
}
fn error(s: &str) -> ReferenceError {
    ReferenceError(format!("Grass worker: {s}"))
}
#[cfg(target_arch = "wasm32")]
fn js_error(v: wasm_bindgen::JsValue) -> ReferenceError {
    use wasm_bindgen::JsCast;
    error(
        &v.dyn_ref::<js_sys::Error>()
            .map_or_else(|| format!("{v:?}"), |e| String::from(e.message())),
    )
}
#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use super::*;
    #[test]
    fn approve_and_consume_need_their_state() {
        let settings = GrassSettings {
            edge_length: 1.0,
            max_tessellation: 2,
            max_height: 1.0,
            fade_end: 10.0,
            lod_start: 1.0,
            lod_end: 5.0,
            lod_max: 3,
            smooth_domain: false,
            thin_start: 5.0,
            thin_floor: 0.5,
        };
        let mut input = Vec::new();
        grass_mesh::encode_dataset(&[], settings, &mut input);
        let id = GrassJobId {
            session: 1,
            revision: 1,
            entity: 1,
        };
        let mut worker = GrassWorker::default();
        worker.install(id, &input, 0).unwrap();
        worker.submit(id, GrassView::default()).unwrap();
        assert!(worker.approve(id, 0).is_err());
        let plan = loop {
            if let GrassPoll::Planned(_, plan) = worker.poll().unwrap() {
                break plan;
            }
        };
        assert!(worker.consume(id).is_err());
        worker.approve(id, plan.output_bytes).unwrap();
        assert!(worker.consume(id).is_err());
    }
}
