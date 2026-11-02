//! Candidate GPU work owns its errors and completion receipts.
use bevy::{
    ecs::system::SystemParamItem,
    render::{
        render_resource::*,
        renderer::{RenderDevice, RenderQueue},
    },
};
use std::sync::{Arc, Mutex};
#[derive(Debug, Default)]
struct Progress {
    pending: usize,
    error: Option<String>,
    promoted: bool,
    cancelled: bool,
    pipelines: usize,
}
#[derive(Clone, Debug, Default)]
pub struct GpuPreparation(Option<Arc<Mutex<Progress>>>);
impl GpuPreparation {
    pub fn cancel(&self) {
        if let Some(p) = &self.0 {
            p.lock().unwrap().cancelled = true;
        }
    }
    pub fn cancelled(&self) -> bool {
        self.0.as_ref().is_some_and(|p| p.lock().unwrap().cancelled)
    }
    pub fn retain_pipeline(&self) {
        if let Some(p) = &self.0 {
            p.lock().unwrap().pipelines += 1;
        }
    }
    pub fn release_pipeline(&self) {
        if let Some(p) = &self.0 {
            p.lock().unwrap().pipelines -= 1;
        }
    }
    pub fn same_owner(&self, other: &Self) -> bool {
        match (&self.0, &other.0) {
            (Some(a), Some(b)) => Arc::ptr_eq(a, b),
            _ => false,
        }
    }
    pub(crate) fn fail(&self, message: String) {
        if let Some(p) = &self.0 {
            p.lock().unwrap().error.get_or_insert(message);
        }
    }
    pub fn isolated(&self) -> bool {
        self.0.as_ref().is_some_and(|s| !s.lock().unwrap().promoted)
    }
    pub fn promote(&self) {
        if let Some(p) = &self.0 {
            p.lock().unwrap().promoted = true;
        }
    }
    pub fn ready(&self) -> Result<bool, crate::reference::ReferenceError> {
        let Some(p) = &self.0 else { return Ok(true) };
        let p = p.lock().unwrap();
        if let Some(e) = &p.error {
            Err(crate::reference::ReferenceError(e.clone()))
        } else {
            Ok(p.pending == 0)
        }
    }
    pub fn drained(&self) -> bool {
        self.0.as_ref().is_none_or(|p| {
            let p = p.lock().unwrap();
            p.pending == 0 && (!p.cancelled || p.pipelines == 0)
        })
    }
    pub fn scope(&self, device: &RenderDevice) -> Option<GpuScope> {
        if !self.isolated() {
            return None;
        }
        let progress = self.0.as_ref().unwrap().clone();
        progress.lock().unwrap().pending += 1;
        Some(GpuScope {
            progress,
            validation: Some(
                device
                    .wgpu_device()
                    .push_error_scope(wgpu::ErrorFilter::Validation),
            ),
            allocation: Some(
                device
                    .wgpu_device()
                    .push_error_scope(wgpu::ErrorFilter::OutOfMemory),
            ),
        })
    }
    pub fn submitted(&self, queue: &RenderQueue) {
        if !self.isolated() {
            return;
        }
        let progress = self.0.as_ref().unwrap().clone();
        progress.lock().unwrap().pending += 1;
        queue.on_submitted_work_done(move || {
            progress.lock().unwrap().pending -= 1;
        });
    }
}
pub struct GpuScope {
    progress: Arc<Mutex<Progress>>,
    validation: Option<wgpu::ErrorScopeGuard>,
    allocation: Option<wgpu::ErrorScopeGuard>,
}
impl Drop for GpuScope {
    fn drop(&mut self) {
        let allocation = self.allocation.take().unwrap().pop();
        let validation = self.validation.take().unwrap().pop();
        let progress = self.progress.clone();
        wasm_bindgen_futures::spawn_local(async move {
            let allocation = allocation.await;
            let validation = validation.await;
            let mut p = progress.lock().unwrap();
            if let Some(error) = allocation.or(validation) {
                p.error
                    .get_or_insert_with(|| format!("Candidate GPU preparation: {error}"));
            }
            p.pending -= 1;
        });
    }
}
pub fn bind_group<M: AsBindGroup>(
    material: &M,
    owner: &GpuPreparation,
    descriptor: &BindGroupLayoutDescriptor,
    device: &RenderDevice,
    cache: &PipelineCache,
    param: &mut SystemParamItem<'_, '_, M::Param>,
) -> Result<PreparedBindGroup, AsBindGroupError> {
    if owner.cancelled() {
        return Err(AsBindGroupError::RetryNextUpdate);
    }
    let _scope = owner.scope(device);
    let layout = cache.get_bind_group_layout(descriptor);
    let UnpreparedBindGroup { mut bindings } =
        material.unprepared_bind_group(&layout, device, param, true)?;
    for (_, binding) in &mut bindings.0 {
        if let OwnedBindingResource::Data(data) = binding {
            *binding = OwnedBindingResource::Buffer(device.create_buffer_with_data(
                &BufferInitDescriptor {
                    label: Some(M::label()),
                    contents: &data.0,
                    usage: BufferUsages::UNIFORM,
                },
            ));
        }
    }
    let entries = bindings
        .iter()
        .map(|(index, binding)| BindGroupEntry {
            binding: *index,
            resource: binding.get_binding(),
        })
        .collect::<Vec<_>>();
    let bind_group = device.create_bind_group(M::label(), &layout, &entries);
    Ok(PreparedBindGroup {
        bindings,
        bind_group,
    })
}
