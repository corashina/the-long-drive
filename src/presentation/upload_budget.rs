//! One GPU upload budget per frame. World meshes and texture batches spend it.
//! A large set of uploads goes over several frames, so streaming does not spike a frame.
use bevy::{ecs as bevy_ecs, prelude::*};
use std::collections::VecDeque;
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

/// Bytes that one frame may upload during play.
pub const FRAME_UPLOAD_BYTES: usize = 4 * 1024 * 1024;

struct Spend {
    spent: AtomicUsize,
    limit: AtomicUsize,
}
impl Default for Spend {
    fn default() -> Self {
        Self {
            spent: AtomicUsize::new(0),
            limit: AtomicUsize::new(usize::MAX),
        }
    }
}
/// Bytes spent this frame. The main and render steps of a frame share it.
#[derive(Clone, Default)]
pub struct UploadSpend(Arc<Spend>);
impl UploadSpend {
    /// Spends `bytes` if the budget is not used up. One item always fits a fresh frame.
    pub fn take(&self, bytes: usize) -> bool {
        let limit = self.0.limit.load(Ordering::Relaxed);
        self.0
            .spent
            .try_update(Ordering::Relaxed, Ordering::Relaxed, |spent| {
                (spent < limit).then(|| spent.saturating_add(bytes))
            })
            .is_ok()
    }
    pub fn reset(&self, limit: usize) {
        self.0.spent.store(0, Ordering::Relaxed);
        self.0.limit.store(limit, Ordering::Relaxed);
    }
}

/// World meshes wait here until the frame budget holds them.
#[derive(Resource, Default)]
pub struct UploadBudget {
    pub spend: UploadSpend,
    queue: VecDeque<(Handle<Mesh>, Mesh)>,
}
impl UploadBudget {
    /// The handle is valid at once. The mesh enters `meshes` when the budget holds it.
    pub fn add(&mut self, meshes: &Assets<Mesh>, mesh: Mesh) -> Handle<Mesh> {
        let handle = meshes.reserve_handle();
        self.queue.push_back((handle.clone(), mesh));
        handle
    }
    /// Inserts queued meshes in order while the budget holds them.
    pub fn flush(&mut self, meshes: &mut Assets<Mesh>) {
        while let Some((handle, mesh)) = self.queue.front() {
            // The owner dropped its handle. The mesh retired before its upload.
            if let Handle::Strong(strong) = handle
                && Arc::strong_count(strong) == 1
            {
                self.queue.pop_front();
                continue;
            }
            if !self.spend.take(mesh_bytes(mesh)) {
                return;
            }
            let (handle, mesh) = self.queue.pop_front().unwrap();
            meshes
                .insert(handle.id(), mesh)
                .expect("reserved mesh handle");
        }
    }
}

fn mesh_bytes(mesh: &Mesh) -> usize {
    mesh.get_vertex_buffer_size() + mesh.get_index_buffer_bytes().map_or(0, <[u8]>::len)
}

/// Frame systems. The loading screen hides upload spikes, so loading has no limit.
#[cfg(target_arch = "wasm32")]
mod frame {
    use super::{FRAME_UPLOAD_BYTES, UploadBudget};
    use crate::presentation::readiness::RendererStateHandle;
    use crate::render_readiness::RendererStatus;
    use bevy::prelude::*;

    fn reset(budget: Res<UploadBudget>, state: Res<RendererStateHandle>) {
        let play = state.lock().proof.status() == RendererStatus::Ready;
        budget
            .spend
            .reset(if play { FRAME_UPLOAD_BYTES } else { usize::MAX });
    }

    fn flush(mut budget: ResMut<UploadBudget>, mut meshes: ResMut<Assets<Mesh>>) {
        budget.flush(&mut meshes);
    }

    pub(crate) fn install(app: &mut App) {
        app.init_resource::<UploadBudget>()
            .add_systems(First, reset)
            .add_systems(PostUpdate, flush);
    }
}
#[cfg(target_arch = "wasm32")]
pub(crate) use frame::install;

#[cfg(test)]
mod tests {
    use super::*;
    use bevy::{asset::RenderAssetUsages, mesh::PrimitiveTopology};

    fn mesh(vertices: usize) -> Mesh {
        Mesh::new(
            PrimitiveTopology::TriangleList,
            RenderAssetUsages::MAIN_WORLD,
        )
        .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, vec![[0.0f32; 3]; vertices])
    }

    #[test]
    fn flush_spreads_meshes_over_frames() {
        let mut meshes = Assets::<Mesh>::default();
        let mut budget = UploadBudget::default();
        // 12 bytes per vertex: each mesh is 3 MiB.
        let size = 3 * 1024 * 1024 / 12;
        let a = budget.add(&meshes, mesh(size));
        let b = budget.add(&meshes, mesh(size));
        let retired = budget.add(&meshes, mesh(size));
        drop(retired);
        let c = budget.add(&meshes, mesh(size));

        budget.spend.reset(FRAME_UPLOAD_BYTES);
        budget.flush(&mut meshes);
        // The second mesh starts below the limit, so the frame takes two.
        assert!(meshes.contains(a.id()) && meshes.contains(b.id()));
        assert!(!meshes.contains(c.id()));
        assert!(!budget.spend.take(1));

        budget.spend.reset(FRAME_UPLOAD_BYTES);
        budget.flush(&mut meshes);
        assert!(meshes.contains(c.id()));
        assert_eq!(meshes.len(), 3);
        assert!(budget.queue.is_empty());
    }
}
