//! Shadow draws share the surface alpha data, without surface lighting textures.
use super::{
    gpu_prepare::GpuPreparation,
    materials::{SourceMaterial, SourceUniform, specular::SpecularMaterial},
};
use bevy::asset::{AssetEvent, AssetId, UntypedAssetId};
use bevy::ecs::{
    self as bevy_ecs,
    query::ROQueryItem,
    system::{SystemParamItem, lifetimeless::SRes},
};
use bevy::pbr::{PreparedMaterial, RenderMaterialInstances, SetMaterialBindGroup};
use bevy::prelude::*;
use bevy::render::{
    Extract, ExtractSchedule, Render, RenderApp, RenderSystems,
    erased_render_asset::ErasedRenderAssets,
    render_asset::RenderAssets,
    render_phase::{PhaseItem, RenderCommand, RenderCommandResult, TrackedRenderPass},
    render_resource::{binding_types::*, *},
    renderer::{RenderDevice, RenderQueue},
    texture::{FallbackImage, GpuImage},
};
use std::collections::BTreeMap;

struct Input {
    revision: u64,
    preparation: GpuPreparation,
    data: SourceUniform,
    batch: super::shadow_batch::CasterBatchState,
    texture: Option<AssetId<Image>>,
    color: Option<super::materials::RetainedColor>,
}
struct SpecularInput {
    revision: u64,
    preparation: GpuPreparation,
    batch: super::shadow_batch::CasterBatchState,
}
#[derive(Resource, Default)]
struct Inputs {
    revision: u64,
    materials: BTreeMap<AssetId<SourceMaterial>, Input>,
}
#[derive(Resource, Default)]
struct SpecularInputs {
    revision: u64,
    materials: BTreeMap<AssetId<SpecularMaterial>, SpecularInput>,
}
struct Material {
    revision: u64,
    color_revision: u64,
    image: (TextureViewId, SamplerId),
    uniform: UniformBuffer<SourceUniform>,
    group: BindGroup,
}
#[derive(Resource, Default)]
pub struct CasterMaterials {
    revision: u64,
    materials: BTreeMap<AssetId<SourceMaterial>, Material>,
}
impl CasterMaterials {
    pub fn group(&self, id: AssetId<SourceMaterial>) -> Option<&BindGroup> {
        self.materials.get(&id).map(|material| &material.group)
    }
}
pub fn layout() -> BindGroupLayoutDescriptor {
    BindGroupLayoutDescriptor::new(
        "source_caster_material",
        &BindGroupLayoutEntries::with_indices(
            ShaderStages::VERTEX_FRAGMENT,
            (
                (1, texture_2d(TextureSampleType::Float { filterable: true })),
                (2, sampler(SamplerBindingType::Filtering)),
                (120, uniform_buffer::<SourceUniform>(false)),
            ),
        ),
    )
}
fn extract(
    assets: Extract<Res<Assets<SourceMaterial>>>,
    mut events: Extract<MessageReader<AssetEvent<SourceMaterial>>>,
    mut inputs: ResMut<Inputs>,
) {
    for event in events.read() {
        match *event {
            AssetEvent::Added { id } | AssetEvent::Modified { id } => {
                let Some(material) = assets.get(id) else {
                    continue;
                };
                inputs.revision += 1;
                let revision = inputs.revision;
                inputs.materials.insert(
                    id,
                    Input {
                        revision,
                        preparation: material.preparation.clone(),
                        data: material.caster_data(),
                        batch: material.shadow_batch_state(),
                        color: material.retained_color.clone(),
                        texture: material.base.base_color_texture.as_ref().map(Handle::id),
                    },
                );
            }
            AssetEvent::Removed { id } => {
                inputs.revision += 1;
                inputs.materials.remove(&id);
            }
            _ => {}
        }
    }
}
fn extract_specular(
    assets: Extract<Res<Assets<SpecularMaterial>>>,
    mut events: Extract<MessageReader<AssetEvent<SpecularMaterial>>>,
    mut inputs: ResMut<SpecularInputs>,
) {
    for event in events.read() {
        match *event {
            AssetEvent::Added { id } | AssetEvent::Modified { id } => {
                let Some(material) = assets.get(id) else {
                    continue;
                };
                inputs.revision += 1;
                let revision = inputs.revision;
                inputs.materials.insert(
                    id,
                    SpecularInput {
                        revision,
                        preparation: material.preparation.clone(),
                        batch: material.shadow_batch_state(),
                    },
                );
            }
            AssetEvent::Removed { id } | AssetEvent::Unused { id } => {
                inputs.materials.remove(&id);
            }
            _ => {}
        }
    }
}
fn prepare(
    inputs: Res<Inputs>,
    mut output: ResMut<CasterMaterials>,
    images: Res<RenderAssets<GpuImage>>,
    white: Res<FallbackImage>,
    device: Res<RenderDevice>,
    queue: Res<RenderQueue>,
    pipelines: Res<PipelineCache>,
) {
    if output.revision != inputs.revision {
        output
            .materials
            .retain(|id, _| inputs.materials.contains_key(id));
        output.revision = inputs.revision;
    }
    for (&id, input) in &inputs.materials {
        if input.preparation.cancelled() {
            continue;
        }
        let image = match input.texture {
            Some(id) => {
                let Some(image) = images.get(id) else {
                    continue;
                };
                image
            }
            None => &white.d2,
        };
        let identity = (image.texture_view.id(), image.sampler.id());
        let (color_revision, data) = input.color.as_ref().map_or((0, input.data), |c| {
            let (revision, color) = c.revision_color();
            (revision, input.data.with_color(color))
        });
        if output.materials.get(&id).is_some_and(|m| {
            m.revision == input.revision
                && m.image == identity
                && m.color_revision == color_revision
        }) {
            continue;
        }
        let _scope = input.preparation.scope(&device);
        if let Some(material) = output.materials.get_mut(&id) {
            if material.revision != input.revision || material.color_revision != color_revision {
                material.uniform.set(data);
                material.uniform.write_buffer(&device, &queue);
                material.revision = input.revision;
                material.color_revision = color_revision;
            }
            if material.image == identity {
                if input.preparation.isolated() {
                    queue.submit([]);
                    input.preparation.submitted(&queue);
                }
                continue;
            }
        }
        let mut uniform = output
            .materials
            .remove(&id)
            .map_or_else(|| UniformBuffer::from(data), |material| material.uniform);
        uniform.write_buffer(&device, &queue);
        let group = device.create_bind_group(
            "source_caster_material",
            &pipelines.get_bind_group_layout(&layout()),
            &BindGroupEntries::with_indices((
                (1, &image.texture_view),
                (2, &image.sampler),
                (120, uniform.binding().unwrap()),
            )),
        );
        if input.preparation.isolated() {
            queue.submit([]);
            input.preparation.submitted(&queue);
        }
        output.materials.insert(
            id,
            Material {
                revision: input.revision,
                color_revision,
                image: identity,
                uniform,
                group,
            },
        );
    }
}
// Each generated primitive reserves this additional caster allocation before material creation.
pub const CPU_BYTES: usize = 4096;
pub fn gpu_bytes() -> usize {
    SourceUniform::min_size().get() as usize
}
pub fn ready(world: &World, id: AssetId<SourceMaterial>) -> bool {
    let inputs = world.resource::<Inputs>();
    let output = world.resource::<CasterMaterials>();
    inputs.materials.get(&id).is_some_and(|input| {
        output
            .materials
            .get(&id)
            .is_some_and(|material| material.revision == input.revision)
    })
}
/// Queue only current prepared opaque casters into material-independent shadow bins.
pub fn shadow_material(world: &World, id: UntypedAssetId) -> Option<UntypedAssetId> {
    if id.type_id() == std::any::TypeId::of::<SpecularMaterial>() {
        let Some(input) = world
            .resource::<SpecularInputs>()
            .materials
            .get(&id.typed::<SpecularMaterial>())
        else {
            return Some(id);
        };
        if input.preparation.cancelled() {
            return Some(id);
        }
        // Bevy removes the old PreparedMaterial before preparing an extracted update.
        // A present material therefore represents this revision, including after retries.
        let prepared = world
            .resource::<ErasedRenderAssets<PreparedMaterial>>()
            .get(id)
            .map(|_| input.revision);
        return input.batch.material_key(id, input.revision, prepared);
    }
    if id.type_id() != std::any::TypeId::of::<SourceMaterial>() {
        return Some(id);
    }
    let typed = id.typed::<SourceMaterial>();
    let Some(input) = world.resource::<Inputs>().materials.get(&typed) else {
        return Some(id);
    };
    if input.preparation.cancelled() {
        return Some(id);
    }
    let prepared = world
        .resource::<CasterMaterials>()
        .materials
        .get(&typed)
        .map(|material| material.revision);
    input.batch.material_key(id, input.revision, prepared)
}
pub struct SetCasterMaterial;
impl<P: PhaseItem> RenderCommand<P> for SetCasterMaterial {
    type Param = (
        SRes<CasterMaterials>,
        SRes<RenderMaterialInstances>,
        <SetMaterialBindGroup<3> as RenderCommand<P>>::Param,
    );
    type ViewQuery = ();
    type ItemQuery = ();
    fn render<'w>(
        item: &P,
        _: ROQueryItem<'w, '_, Self::ViewQuery>,
        _: Option<()>,
        (materials, instances, full): SystemParamItem<'w, '_, Self::Param>,
        pass: &mut TrackedRenderPass<'w>,
    ) -> RenderCommandResult {
        let Some(instance) = instances.into_inner().instances.get(&item.main_entity()) else {
            return RenderCommandResult::Skip;
        };
        if instance.asset_id.type_id() != std::any::TypeId::of::<SourceMaterial>() {
            return <SetMaterialBindGroup<3> as RenderCommand<P>>::render(
                item,
                (),
                None,
                full,
                pass,
            );
        }
        let Some(material) = materials
            .into_inner()
            .materials
            .get(&instance.asset_id.typed::<SourceMaterial>())
        else {
            return RenderCommandResult::Skip;
        };
        pass.set_bind_group(3, &material.group, &[]);
        RenderCommandResult::Success
    }
}
pub fn install(app: &mut App) {
    app.sub_app_mut(RenderApp)
        .init_resource::<Inputs>()
        .init_resource::<SpecularInputs>()
        .init_resource::<CasterMaterials>()
        .add_systems(ExtractSchedule, (extract, extract_specular))
        .add_systems(Render, prepare.in_set(RenderSystems::PrepareBindGroups));
}
