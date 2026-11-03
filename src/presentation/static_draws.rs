//! Static mesh draws of the camera pass run as one render bundle per view.
//! A static draw is an unskinned mesh without morph targets. Its commands repeat each frame.
//! GPU preprocessing writes its transform, so a moving mesh is static too.
//! Bevy creates new mesh bind groups each frame. Static draws bind a model-only group that
//! changes only with the instance buffer of its phase.
use bevy::ecs::{
    query::ROQueryItem,
    system::{SystemParamItem, lifetimeless::SRes},
};
use bevy::pbr::{
    MeshInputUniform, MeshPipeline, MeshUniform, MorphIndices, RenderMeshInstances,
    SetMeshBindGroup, SkinUniforms,
};
use bevy::platform::collections::HashMap;
use bevy::render::{
    batching::gpu_preprocessing::BatchedInstanceBuffers,
    mesh::{RenderMesh, allocator::MeshAllocator},
    render_asset::RenderAssets,
    render_phase::{
        PhaseItem, PhaseItemExtraIndex, RenderCommand, RenderCommandResult, TrackedRenderPass,
    },
    render_resource::{BindGroup, BufferId, PipelineCache},
    renderer::RenderDevice,
    sync_world::MainEntity,
};
use bevy::{ecs as bevy_ecs, prelude::*};
use std::any::TypeId;
use std::sync::Mutex;

/// Decides which draws are static. The project has no lightmapped meshes.
pub struct StaticDraws<'w> {
    meshes: &'w RenderMeshInstances,
    skins: &'w SkinUniforms,
    morphs: &'w MorphIndices,
    allocator: &'w MeshAllocator,
}
impl<'w> StaticDraws<'w> {
    pub fn new(world: &'w World) -> Self {
        Self {
            meshes: world.resource(),
            skins: world.resource(),
            morphs: world.resource(),
            allocator: world.resource(),
        }
    }
    pub fn contains(&self, entity: MainEntity) -> bool {
        let Some(mesh) = self.meshes.mesh_asset_id(entity) else {
            return false;
        };
        let morphed = match self.morphs {
            MorphIndices::Uniform { current, .. } => current.contains_key(&entity),
            MorphIndices::Storage { .. } => self
                .allocator
                .mesh_slabs(&mesh)
                .is_some_and(|slabs| slabs.morph_target_slab_id.is_some()),
        };
        !morphed && self.skins.skin_byte_offset(entity).is_none()
    }
}

/// The model-only mesh bind group of each phase, by its instance buffer.
#[derive(Resource, Default)]
pub struct StaticMeshGroups(HashMap<TypeId, (BufferId, BindGroup)>);

pub(crate) fn prepare_groups(
    mut groups: ResMut<StaticMeshGroups>,
    buffers: Option<Res<BatchedInstanceBuffers<MeshUniform, MeshInputUniform>>>,
    pipeline: Res<MeshPipeline>,
    device: Res<RenderDevice>,
    cache: Res<PipelineCache>,
) {
    let Some(buffers) = buffers else {
        groups.0.clear();
        return;
    };
    groups
        .0
        .retain(|phase, _| buffers.phase_instance_buffers.contains_key(phase));
    for (phase, phase_buffers) in &buffers.phase_instance_buffers {
        let Some(buffer) = phase_buffers.data_buffer.buffer() else {
            groups.0.remove(phase);
            continue;
        };
        if groups
            .0
            .get(phase)
            .is_some_and(|(id, _)| *id == buffer.id())
        {
            continue;
        }
        let group = pipeline
            .mesh_layouts
            .model_only(&device, &cache, &buffer.as_entire_binding());
        groups.0.insert(*phase, (buffer.id(), group));
    }
}

/// True when the mesh pipeline of `mesh` uses the skinned mesh layout. It is the same test as in Bevy.
fn skinned_layout(mesh: &RenderMesh) -> bool {
    mesh.layout.0.contains(Mesh::ATTRIBUTE_JOINT_INDEX)
        && mesh.layout.0.contains(Mesh::ATTRIBUTE_JOINT_WEIGHT)
}

/// The mesh group of a source draw. Static draws bind the stable group, others the Bevy group.
/// The pipeline takes the skinned layout from the mesh joints. The bind group takes it from a
/// live skin. When they differ, it skips the draw, because a WebGPU validation error stops the game.
pub struct SetSourceMeshBindGroup<const I: usize>;
impl<P: PhaseItem, const I: usize> RenderCommand<P> for SetSourceMeshBindGroup<I> {
    type Param = (
        SRes<StaticMeshGroups>,
        SRes<RenderMeshInstances>,
        SRes<SkinUniforms>,
        SRes<MorphIndices>,
        SRes<MeshAllocator>,
        SRes<RenderAssets<RenderMesh>>,
        <SetMeshBindGroup<I> as RenderCommand<P>>::Param,
    );
    type ViewQuery = <SetMeshBindGroup<I> as RenderCommand<P>>::ViewQuery;
    type ItemQuery = ();
    fn render<'w>(
        item: &P,
        view: ROQueryItem<'w, '_, Self::ViewQuery>,
        _: Option<()>,
        (groups, meshes, skins, morphs, allocator, render_meshes, bevy): SystemParamItem<
            'w,
            '_,
            Self::Param,
        >,
        pass: &mut TrackedRenderPass<'w>,
    ) -> RenderCommandResult {
        let entity = item.main_entity();
        if let Some(mesh) = meshes
            .mesh_asset_id(entity)
            .and_then(|id| render_meshes.into_inner().get(id))
        {
            let skin = skins.skin_byte_offset(entity).is_some();
            if skinned_layout(mesh) != skin {
                warn_once!(
                    "Skipped a source draw: its mesh joints and its skin differ \
                     (entity {entity:?}, mesh joints {}, live skin {skin})",
                    !skin
                );
                return RenderCommandResult::Skip;
            }
        }
        let draws = StaticDraws {
            meshes: meshes.into_inner(),
            skins: skins.into_inner(),
            morphs: morphs.into_inner(),
            allocator: allocator.into_inner(),
        };
        if draws.contains(entity)
            && let Some((_, group)) = groups.into_inner().0.get(&TypeId::of::<P>())
        {
            match item.extra_index() {
                PhaseItemExtraIndex::DynamicOffset(offset) => {
                    pass.set_bind_group(I, group, &[offset]);
                }
                _ => pass.set_bind_group(I, group, &[]),
            }
            return RenderCommandResult::Success;
        }
        <SetMeshBindGroup<I> as RenderCommand<P>>::render(item, view, None, bevy, pass)
    }
}

/// The static draw bundle of each view, and the depth prepass bundle of each world view.
#[derive(Resource)]
pub struct StaticBundles {
    pub views: [Mutex<super::render_bundles::DrawBundle>; super::surface_views::VIEW_COUNT],
    pub depth: [Mutex<super::render_bundles::DrawBundle>; super::surface_views::WORLD_VIEWS],
}
impl Default for StaticBundles {
    fn default() -> Self {
        Self {
            views: std::array::from_fn(|_| Mutex::default()),
            depth: std::array::from_fn(|_| Mutex::default()),
        }
    }
}

pub fn install(app: &mut SubApp) {
    use bevy::render::{Render, RenderSystems};
    app.init_resource::<StaticMeshGroups>()
        .init_resource::<StaticBundles>()
        .add_systems(
            Render,
            prepare_groups
                .in_set(RenderSystems::PrepareBindGroups)
                .after(bevy::pbr::prepare_mesh_bind_groups),
        );
}
