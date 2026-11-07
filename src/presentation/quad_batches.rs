//! Instanced quad batches with one uniform buffer each. Particles and decals use them.
//! The main world marks the batches it draws. Extract copies and upload writes only those.
use bevy::asset::RenderAssetUsages;
use bevy::ecs as bevy_ecs;
use bevy::mesh::{Indices, PrimitiveTopology};
use bevy::prelude::*;
use bevy::render::{
    Extract, ExtractSchedule, Render, RenderApp, RenderSystems,
    render_resource::{Buffer, ShaderType, UniformBuffer, encase::internal::WriteInto},
    renderer::{RenderDevice, RenderQueue},
};
use std::marker::PhantomData;

/// Main world batch uniforms.
#[derive(Resource)]
pub struct QuadBatches<U> {
    uniforms: Vec<U>,
    /// Batches to upload this frame: new and drawn ones.
    marked: Vec<usize>,
}
impl<U> Default for QuadBatches<U> {
    fn default() -> Self {
        Self {
            uniforms: Vec::new(),
            marked: Vec::new(),
        }
    }
}
impl<U> QuadBatches<U> {
    pub fn len(&self) -> usize {
        self.uniforms.len()
    }
    /// Adds a batch. Its first upload makes the buffer that the bind group needs.
    pub fn push(&mut self, uniform: U) -> usize {
        let index = self.uniforms.len();
        self.uniforms.push(uniform);
        self.marked.push(index);
        index
    }
    /// Unmarked access. A hidden batch keeps its old buffer.
    pub fn uniforms_mut(&mut self) -> &mut [U] {
        &mut self.uniforms
    }
    /// Marks a drawn batch for upload.
    pub fn mark(&mut self, index: usize) -> &mut U {
        self.marked.push(index);
        &mut self.uniforms[index]
    }
}

/// Render world copy of the marked batches.
#[derive(Resource)]
pub struct ExtractedQuadBatches<U> {
    pub len: usize,
    marked: Vec<(usize, U)>,
}
impl<U> Default for ExtractedQuadBatches<U> {
    fn default() -> Self {
        Self {
            len: 0,
            marked: Vec::new(),
        }
    }
}

#[derive(Resource)]
pub struct QuadBatchGpu<U: ShaderType> {
    buffers: Vec<UniformBuffer<U>>,
}
impl<U: ShaderType> Default for QuadBatchGpu<U> {
    fn default() -> Self {
        Self {
            buffers: Vec::new(),
        }
    }
}
impl<U: ShaderType + WriteInto> QuadBatchGpu<U> {
    pub fn buffer(&self, index: usize) -> Option<&Buffer> {
        self.buffers.get(index).and_then(UniformBuffer::buffer)
    }
}

pub struct QuadBatchPlugin<U>(PhantomData<fn(U)>);
impl<U> Default for QuadBatchPlugin<U> {
    fn default() -> Self {
        Self(PhantomData)
    }
}
impl<U: ShaderType + WriteInto + Default + Copy + Send + Sync + 'static> Plugin
    for QuadBatchPlugin<U>
{
    fn build(&self, app: &mut App) {
        app.init_resource::<QuadBatches<U>>()
            .add_systems(First, clear::<U>);
        app.sub_app_mut(RenderApp)
            .init_resource::<ExtractedQuadBatches<U>>()
            .init_resource::<QuadBatchGpu<U>>()
            .add_systems(ExtractSchedule, extract::<U>)
            .add_systems(Render, upload::<U>.in_set(RenderSystems::PrepareResources));
    }
}

fn clear<U: Send + Sync + 'static>(mut batches: ResMut<QuadBatches<U>>) {
    batches.marked.clear();
}

fn extract<U: Copy + Send + Sync + 'static>(
    batches: Extract<Res<QuadBatches<U>>>,
    mut extracted: ResMut<ExtractedQuadBatches<U>>,
) {
    extracted.len = batches.uniforms.len();
    extracted.marked.clear();
    extracted
        .marked
        .extend(batches.marked.iter().map(|&i| (i, batches.uniforms[i])));
}

fn upload<U: ShaderType + WriteInto + Default + Copy + Send + Sync + 'static>(
    extracted: Res<ExtractedQuadBatches<U>>,
    mut gpu: ResMut<QuadBatchGpu<U>>,
    device: Res<RenderDevice>,
    queue: Res<RenderQueue>,
) {
    gpu.buffers
        .resize_with(extracted.len, UniformBuffer::default);
    for &(i, value) in &extracted.marked {
        gpu.buffers[i].set(value);
        gpu.buffers[i].write_buffer(&device, &queue);
    }
}

/// `count` unit quads. Vertex z is the instance index.
pub fn quad_batch(count: usize) -> Mesh {
    let mut positions = Vec::with_capacity(count * 4);
    let mut uv = Vec::with_capacity(count * 4);
    let mut indices = Vec::with_capacity(count * 6);
    for i in 0..count {
        let z = i as f32;
        positions.extend([
            [-0.5, -0.5, z],
            [0.5, -0.5, z],
            [0.5, 0.5, z],
            [-0.5, 0.5, z],
        ]);
        uv.extend([[0., 1.], [1., 1.], [1., 0.], [0., 0.]]);
        let k = i as u32 * 4;
        indices.extend([k, k + 1, k + 2, k, k + 2, k + 3]);
    }
    Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::RENDER_WORLD,
    )
    .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, positions)
    .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, uv)
    .with_inserted_indices(Indices::U32(indices))
}
