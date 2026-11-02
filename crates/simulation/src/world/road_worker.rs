//! One road calculation and unread result, owned by one world.
use super::WorldSurface;
use crate::reference::ReferenceError;

// scripts/build.ps1 gives the road package this initial linear-memory capacity.
pub const INITIAL_WASM_BYTES: usize = 2 * 1024 * 1024;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RoadJobId {
    pub session: u64,
    pub revision: u64,
    pub owner: u32,
    pub sequence: u64,
}
impl RoadJobId {
    #[cfg(target_arch = "wasm32")]
    fn bytes(self) -> [u8; 28] {
        let mut bytes = [0; 28];
        bytes[..8].copy_from_slice(&self.session.to_le_bytes());
        bytes[8..16].copy_from_slice(&self.revision.to_le_bytes());
        bytes[16..20].copy_from_slice(&self.owner.to_le_bytes());
        bytes[20..].copy_from_slice(&self.sequence.to_le_bytes());
        bytes
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RoadPoll {
    Idle,
    Busy,
    Ready(RoadJobId),
}
#[derive(Clone, Copy)]
pub struct RoadLayout {
    pub vertices: usize,
    pub normals: usize,
    pub tangents: usize,
    pub uv: usize,
    pub triangles: usize,
    pub groups: usize,
    pub cache_bytes: usize,
    pub scratch_bytes: usize,
}
impl RoadLayout {
    pub fn new(p: &road_mesh::RoadPrototype) -> Self {
        Self {
            vertices: p.skins.iter().map(|s| s.positions.len()).sum(),
            normals: p.skins.iter().map(|s| s.normals.len()).sum(),
            tangents: p.skins.iter().map(|s| s.tangents.len()).sum(),
            uv: p.skins.iter().map(|s| s.uv.len()).sum(),
            triangles: p
                .skins
                .iter()
                .flat_map(|s| &s.triangles)
                .map(Vec::len)
                .sum(),
            groups: p.skins.iter().map(|s| s.triangles.len()).sum(),
            cache_bytes: p.retained_bytes(),
            scratch_bytes: p.nodes.len() * std::mem::size_of::<road_mesh::RoadFrame>(),
        }
    }
    pub fn surface_bytes(self) -> usize {
        self.vertices * 12
            + self.normals * 12
            + self.tangents * 16
            + self.uv * 8
            + self.triangles * 12
            + self.groups * std::mem::size_of::<std::ops::Range<usize>>()
    }
    /// Main output, worker output, transferred output, packed ranges, and both input copies.
    pub fn job_bytes(self) -> usize {
        self.surface_bytes() * 3
            + self.groups * 16
            + 48
            + self.scratch_bytes
            + std::mem::size_of::<road_mesh::RoadMesh>()
            + 16
            + 156 * 3
    }
    #[cfg(target_arch = "wasm32")]
    fn words(self) -> [u32; 9] {
        [
            self.vertices,
            self.normals,
            self.tangents,
            self.uv,
            self.triangles,
            self.groups,
            self.cache_bytes,
            self.job_bytes(),
            self.scratch_bytes,
        ]
        .map(|v| u32::try_from(v).expect("admitted road allocation"))
    }
}
#[derive(Clone, Copy)]
struct Installed {
    session: u64,
    revision: u64,
    layout: RoadLayout,
}

#[derive(Default)]
pub struct RoadWorker {
    next: u64,
    last_submitted: u64,
    installed: [Option<Installed>; 3],
    job: Option<RoadJobId>,
    canceled: bool,
    #[cfg(target_arch = "wasm32")]
    transport: Option<crate::host::RoadTransport>,
    #[cfg(target_arch = "wasm32")]
    wasm_bytes: usize,
    #[cfg(not(target_arch = "wasm32"))]
    prototypes: [Option<road_mesh::RoadPrototype>; 3],
    #[cfg(not(target_arch = "wasm32"))]
    result: Option<road_mesh::RoadMesh>,
}
impl RoadWorker {
    pub fn next_id(
        &mut self,
        session: u64,
        revision: u64,
        owner: u32,
    ) -> Result<RoadJobId, ReferenceError> {
        self.next = self
            .next
            .checked_add(1)
            .ok_or_else(|| error("Job sequence exhausted"))?;
        Ok(RoadJobId {
            session,
            revision,
            owner,
            sequence: self.next,
        })
    }
    pub fn installed(&self, session: u64, revision: u64, owner: usize) -> bool {
        self.installed[owner].is_some_and(|s| s.session == session && s.revision == revision)
    }
    pub fn cache_bytes(&self) -> usize {
        self.installed
            .iter()
            .flatten()
            .map(|s| s.layout.cache_bytes)
            .sum()
    }
    pub fn wasm_bytes(&self) -> usize {
        #[cfg(target_arch = "wasm32")]
        {
            self.wasm_bytes
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            0
        }
    }
    pub fn initialize(&mut self, capacity: usize) -> Result<bool, ReferenceError> {
        #[cfg(target_arch = "wasm32")]
        {
            let transport = self.transport()?;
            transport
                .road_capacity_limit(capacity as f64)
                .map_err(js_error)?;
            transport.road_initialized().map_err(js_error)
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            let _ = capacity;
            Ok(true)
        }
    }
    #[cfg(target_arch = "wasm32")]
    fn transport(&mut self) -> Result<&crate::host::RoadTransport, ReferenceError> {
        if self.transport.is_none() {
            self.transport = Some(crate::host::game_road_create().map_err(js_error)?);
        }
        Ok(self.transport.as_ref().unwrap())
    }
    pub fn install(
        &mut self,
        identity: RoadJobId,
        input: &[u8],
        layout: RoadLayout,
    ) -> Result<(), ReferenceError> {
        if self.job.is_some()
            || identity.owner >= 3
            || self.installed[identity.owner as usize].is_some()
        {
            return Err(error("Invalid source installation"));
        }
        #[cfg(target_arch = "wasm32")]
        self.transport()?
            .road_install(&identity.bytes(), input, &layout.words())
            .map_err(js_error)?;
        #[cfg(not(target_arch = "wasm32"))]
        {
            self.prototypes[identity.owner as usize] =
                Some(road_mesh::decode_prototype(input).map_err(ReferenceError)?);
        }
        self.installed[identity.owner as usize] = Some(Installed {
            session: identity.session,
            revision: identity.revision,
            layout,
        });
        Ok(())
    }
    /// Input starts with the source slot and its exact revision, followed by the pose.
    pub fn submit(&mut self, identity: RoadJobId, input: &[u8]) -> Result<(), ReferenceError> {
        if self.job.is_some()
            || input.len() != 156
            || identity.sequence != self.next
            || identity.sequence <= self.last_submitted
        {
            return Err(error("Invalid submission or worker busy"));
        }
        let owner = u32::from_le_bytes(input[..4].try_into().unwrap()) as usize;
        let revision = u64::from_le_bytes(input[4..12].try_into().unwrap());
        if owner >= 3 || !self.installed(identity.session, revision, owner) {
            return Err(error("Road source is not installed"));
        }
        #[cfg(target_arch = "wasm32")]
        self.transport()?
            .road_submit(&identity.bytes(), input)
            .map_err(js_error)?;
        #[cfg(not(target_arch = "wasm32"))]
        {
            let pose = road_mesh::decode_pose(&input[12..]).map_err(ReferenceError)?;
            self.result = Some(road_mesh::deform(
                self.prototypes[owner].as_ref().unwrap(),
                &pose,
            ));
        }
        self.last_submitted = identity.sequence;
        self.job = Some(identity);
        self.canceled = false;
        Ok(())
    }
    pub fn poll(&mut self) -> Result<RoadPoll, ReferenceError> {
        #[cfg(target_arch = "wasm32")]
        {
            let Some(transport) = &self.transport else {
                return Ok(RoadPoll::Idle);
            };
            let result = transport.road_poll();
            self.wasm_bytes = transport.road_wasm_bytes() as usize;
            match result.map_err(js_error)? {
                -1 => {
                    self.job = None;
                    self.canceled = false;
                    Ok(RoadPoll::Idle)
                }
                -2 => Ok(RoadPoll::Busy),
                1 if !self.canceled => self
                    .job
                    .map(RoadPoll::Ready)
                    .ok_or_else(|| error("Result has no owner")),
                _ => Err(error("Invalid transport state")),
            }
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            Ok(match self.job {
                Some(id) if !self.canceled => RoadPoll::Ready(id),
                _ => {
                    self.job = None;
                    self.result = None;
                    self.canceled = false;
                    RoadPoll::Idle
                }
            })
        }
    }
    pub fn read(&mut self, output: &mut WorldSurface) -> Result<(), ReferenceError> {
        let RoadPoll::Ready(_) = self.poll()? else {
            return Err(error("No readable result"));
        };
        let _timer = super::stream_profile::Timer::new(super::stream_profile::Stage::RoadResult);
        #[cfg(target_arch = "wasm32")]
        {
            let transport = self.transport.as_ref().unwrap();
            // The bridge validates all lengths against the installed immutable source.
            let owner = transport.road_source_owner() as usize;
            let layout = self
                .installed
                .get(owner)
                .and_then(|s| *s)
                .ok_or_else(|| error("Invalid result source"))?
                .layout;
            output.vertices = vec![[0.; 3]; layout.vertices];
            output.normals = vec![[0.; 3]; layout.normals];
            output.tangents = vec![[0.; 4]; layout.tangents];
            output.uv = vec![[0.; 2]; layout.uv];
            output.triangles = vec![[0; 3]; layout.triangles];
            output.submeshes = Vec::with_capacity(layout.groups);
            let mut groups = vec![0; layout.groups * 2];
            transport
                .road_read(
                    &mut output.origin,
                    output.vertices.as_flattened_mut(),
                    output.normals.as_flattened_mut(),
                    output.tangents.as_flattened_mut(),
                    output.uv.as_flattened_mut(),
                    output.triangles.as_flattened_mut(),
                    &mut groups,
                )
                .map_err(js_error)?;
            output
                .submeshes
                .extend(groups.chunks_exact(2).map(|g| g[0] as usize..g[1] as usize));
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            let mesh = self
                .result
                .take()
                .ok_or_else(|| error("No readable result"))?;
            *output = WorldSurface {
                origin: mesh.origin,
                vertices: mesh.vertices,
                normals: mesh.normals,
                tangents: mesh.tangents,
                uv: mesh.uv,
                triangles: mesh.triangles,
                submeshes: mesh.submeshes,
                colors: Vec::new(),
            };
        }
        self.job = None;
        Ok(())
    }
    /// Cancellation retains physical ownership until a reply or termination.
    pub fn discard(&mut self) {
        self.canceled = true;
        #[cfg(target_arch = "wasm32")]
        if let Some(t) = &self.transport {
            t.road_discard();
        }
    }
    pub fn dispose(&mut self) {
        #[cfg(target_arch = "wasm32")]
        {
            if let Some(t) = self.transport.take() {
                t.road_dispose();
            }
            self.wasm_bytes = 0;
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            self.prototypes = [const { None }; 3];
            self.result = None;
        }
        self.installed = [None; 3];
        self.job = None;
        self.canceled = false;
        // Keep the sequence across transport replacement. Never reuse an identity.
    }
    pub fn packing_started(&mut self) -> Result<(), ReferenceError> {
        #[cfg(target_arch = "wasm32")]
        self.transport()?.road_packing_started();
        Ok(())
    }
    pub fn packing_ended(&self) {
        #[cfg(target_arch = "wasm32")]
        if let Some(t) = &self.transport {
            t.road_packing_ended();
        }
    }
    pub fn publication_started(&self) {
        #[cfg(target_arch = "wasm32")]
        if let Some(t) = &self.transport {
            t.road_publication_started();
        }
    }
    pub fn publication_ended(&self, accepted: bool) {
        #[cfg(target_arch = "wasm32")]
        if let Some(t) = &self.transport {
            t.road_publication_ended(accepted);
        }
        #[cfg(not(target_arch = "wasm32"))]
        let _ = accepted;
    }
}
impl Drop for RoadWorker {
    fn drop(&mut self) {
        self.dispose();
    }
}
fn error(message: &str) -> ReferenceError {
    ReferenceError(format!("Road worker: {message}"))
}
#[cfg(target_arch = "wasm32")]
fn js_error(value: wasm_bindgen::JsValue) -> ReferenceError {
    use wasm_bindgen::JsCast;
    error(
        &value
            .dyn_ref::<js_sys::Error>()
            .map_or_else(|| format!("{value:?}"), |e| String::from(e.message())),
    )
}
