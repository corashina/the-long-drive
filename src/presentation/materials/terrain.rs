//! TerrainBiome surface parameters and authored texture bindings.
use super::{SourceReceiver, world_textures::WorldTextures};
use crate::reference::{ReferenceError, SourceId, WorldSourceMaterial};
use bevy::{
    asset as bevy_asset,
    ecs::system::{SystemParamItem, lifetimeless::SRes},
    mesh::MeshVertexBufferLayoutRef,
    pbr::{MaterialPipeline, MaterialPipelineKey},
    prelude::*,
    reflect as bevy_reflect,
    render::{
        render_asset::RenderAssets, render_resource::*, renderer::RenderDevice, texture::GpuImage,
    },
    shader::{ShaderDefVal, ShaderRef},
};

const SLOTS: [&str; 12] = [
    "_SandNormal",
    "_GrassNormal",
    "_DirtNormal",
    "_SnowNormal",
    "_CliffNormal",
    "_NormalMap",
    "_NormalMap2",
    "_DirtTexture",
    "_GrassTexture",
    "_SandTexture",
    "_SnowTexture",
    "_CliffTexture",
];
#[derive(Clone, Copy, Debug, ShaderType)]
pub struct TerrainUniform {
    pub displacement: Vec4,
    pub renderer: UVec4,
    pub texcoord: Vec4,
    pub scale_slope: Vec4,
    pub layers: [Vec4; 5],
    pub detail: Vec4,
    pub colors: [Vec4; 10],
    pub bindings: [UVec4; 12],
    pub sampling: [Vec4; 12],
}
#[derive(Asset, TypePath, Clone, Debug)]
pub struct TerrainMaterial {
    pub preparation: crate::presentation::gpu_prepare::GpuPreparation,
    pub source: SourceId,
    uploaded:
        std::sync::Arc<std::sync::Mutex<std::collections::HashSet<bevy::asset::AssetId<Image>>>>,
    pub uniform: TerrainUniform,
    pub arrays: [Handle<Image>; 5],
    pub array_count: u8,
    pub receiver_mask: u32,
}
impl TerrainMaterial {
    pub fn from_source(
        record: &WorldSourceMaterial,
        textures: &WorldTextures,
        mask: u32,
        receive_shadows: bool,
    ) -> Result<Self, ReferenceError> {
        if record.shader.name != "TerrainBiome" {
            return Err(ReferenceError("Expected TerrainBiome source shader".into()));
        }
        let number = |name: &str| {
            record
                .floats
                .get(name)
                .copied()
                .ok_or_else(|| ReferenceError(format!("TerrainBiome missing {name}")))
        };
        let mut colors = [Vec4::ZERO; 10];
        for (index, name) in ["Dirt", "Grass", "Sand", "Snow", "Cliff"]
            .iter()
            .enumerate()
        {
            for color in 0..2 {
                let key = format!("_{name}Color{}", color + 1);
                let value = record
                    .colors
                    .get(&key)
                    .ok_or_else(|| ReferenceError(format!("TerrainBiome missing {key}")))?;
                colors[index * 2 + color] = Color::srgba(value[0], value[1], value[2], value[3])
                    .to_linear()
                    .to_f32_array()
                    .into();
            }
        }
        let mut layers = [Vec4::ZERO; 5];
        for (index, name) in ["Sand", "Grass", "Dirt", "Snow", "Cliff"]
            .iter()
            .enumerate()
        {
            layers[index] = Vec4::new(
                number(&format!("_{name}Scale"))?,
                number(&format!("_{name}NormalStrength"))?,
                number(&format!("_{name}Smooth"))?,
                0.,
            );
        }
        let mut banks = Vec::with_capacity(5);
        let mut bindings = [UVec4::ZERO; 12];
        let mut sampling = [Vec4::ZERO; 12];
        for (index, slot) in SLOTS.iter().enumerate() {
            let (binding, image) = textures
                .binding(&record.source, slot)
                .ok_or_else(|| ReferenceError(format!("TerrainBiome texture not ready: {slot}")))?;
            let bank = if let Some(bank) = banks.iter().position(|(id, _)| *id == binding.array) {
                bank
            } else {
                banks.push((binding.array, image.clone()));
                banks.len() - 1
            };
            if bank >= 5 {
                return Err(ReferenceError(
                    "TerrainBiome exceeds five exact texture arrays".into(),
                ));
            }
            bindings[index] = UVec4::new(bank as u32, binding.layer, 0, 0);
            // This compiled program uses uv_texcoord_ST only. Individual TexEnv scale
            // and offset values are retained in the source contract but never read.
            sampling[index] = textures
                .sampling(binding.array)
                .ok_or_else(|| ReferenceError("Missing terrain sampler".into()))?;
        }
        let array_count = banks.len() as u8;
        if array_count == 0 {
            return Err(ReferenceError("TerrainBiome has no source images".into()));
        }
        let arrays = std::array::from_fn(|index| banks.get(index).unwrap_or(&banks[0]).1.clone());
        Ok(Self {
            preparation: default(),
            source: record.source.clone(),
            uploaded: textures.upload_receipts(),
            uniform: TerrainUniform {
                texcoord: Vec4::from_array(
                    *record
                        .texture_coordinates
                        .get("_texcoord")
                        .ok_or_else(|| ReferenceError("Missing source _texcoord_ST".into()))?,
                ),
                renderer: UVec4::new(receive_shadows as u32, 0, 0, 0),
                displacement: Vec4::new(
                    number("_FarDist")?,
                    number("_NearDist")?,
                    number("_Amount")?,
                    number("_VertexColor")?,
                ),
                scale_slope: Vec4::new(
                    number("_MainScale")?,
                    number("_LocalScaling")?,
                    number("_CliffSlopeMin")?,
                    number("_CliffSlopeMax")?,
                ),
                detail: Vec4::new(
                    number("_NormalMapScale")?,
                    number("_NormalMapStrength")?,
                    number("_NormalMapScale2")?,
                    number("_NormalMapStrength2")?,
                ),
                colors,
                layers,
                bindings,
                sampling,
            },
            arrays,
            array_count,
            receiver_mask: mask,
        })
    }
}
impl Material for TerrainMaterial {
    fn vertex_shader() -> ShaderRef {
        "shaders/source_terrain.wgsl".into()
    }
    fn fragment_shader() -> ShaderRef {
        "shaders/source_terrain.wgsl".into()
    }
    fn enable_prepass() -> bool {
        false
    }
    fn enable_shadows() -> bool {
        false
    }
    fn specialize(
        _: &MaterialPipeline,
        descriptor: &mut RenderPipelineDescriptor,
        layout: &MeshVertexBufferLayoutRef,
        key: MaterialPipelineKey<Self>,
    ) -> Result<(), SpecializedMeshPipelineError> {
        if !key
            .mesh_key
            .contains(bevy::pbr::MeshPipelineKey::DEPTH_PREPASS)
        {
            for definitions in [
                &mut descriptor.vertex.shader_defs,
                &mut descriptor
                    .fragment
                    .as_mut()
                    .expect("source world fragment")
                    .shader_defs,
            ] {
                definitions.push(bevy::shader::ShaderDefVal::Bool(
                    "SOURCE_WORLD".into(),
                    true,
                ));
                definitions.push(bevy::shader::ShaderDefVal::Bool(
                    "SOURCE_OBJECT_REFLECTED".into(),
                    false,
                ));
            }
            descriptor.layout[0] = crate::presentation::source_lighting::source_camera_layout();
            descriptor.layout[1] = crate::presentation::source_lighting::surface_lighting_layout();
            if let Some(depth) = &mut descriptor.depth_stencil {
                depth.format = crate::presentation::render_targets::DEPTH_FORMAT;
            }
        }
        descriptor.vertex.buffers = vec![layout.0.get_layout(&[
            Mesh::ATTRIBUTE_POSITION.at_shader_location(0),
            Mesh::ATTRIBUTE_NORMAL.at_shader_location(1),
            Mesh::ATTRIBUTE_UV_0.at_shader_location(2),
            Mesh::ATTRIBUTE_COLOR.at_shader_location(3),
            Mesh::ATTRIBUTE_TANGENT.at_shader_location(4),
        ])?];
        descriptor.primitive.front_face = if key
            .mesh_key
            .contains(bevy::pbr::MeshPipelineKey::INVERT_CULLING)
        {
            FrontFace::Cw
        } else {
            FrontFace::Ccw
        };
        let definition =
            ShaderDefVal::UInt("TERRAIN_ARRAYS".into(), u32::from(key.bind_group_data));
        descriptor.vertex.shader_defs.push(definition.clone());
        if let Some(fragment) = &mut descriptor.fragment {
            fragment.shader_defs.push(definition);
        }
        Ok(())
    }
}
impl AsBindGroup for TerrainMaterial {
    type Data = u8;
    type Param = (
        <SourceReceiver as AsBindGroup>::Param,
        SRes<RenderAssets<GpuImage>>,
    );
    fn label() -> &'static str {
        "source TerrainBiome"
    }
    fn bind_group_data(&self) -> u8 {
        self.array_count
    }
    fn as_bind_group(
        &self,
        descriptor: &BindGroupLayoutDescriptor,
        device: &RenderDevice,
        cache: &PipelineCache,
        param: &mut SystemParamItem<Self::Param>,
    ) -> Result<PreparedBindGroup, AsBindGroupError> {
        crate::presentation::gpu_prepare::bind_group(
            self,
            &self.preparation,
            descriptor,
            device,
            cache,
            param,
        )
    }
    fn unprepared_bind_group(
        &self,
        layout: &BindGroupLayout,
        device: &RenderDevice,
        params: &mut SystemParamItem<Self::Param>,
        direct: bool,
    ) -> Result<UnpreparedBindGroup, AsBindGroupError> {
        if self.preparation.isolated() && !direct {
            return Err(AsBindGroupError::CreateBindGroupDirectly);
        }
        let extension = SourceReceiver {
            lod_fade: 1.,
            receiver_mask: self.receiver_mask,
        };
        let mut result = extension.unprepared_bind_group(layout, device, &mut params.0, true)?;
        let mut data = encase::UniformBuffer::new(Vec::new());
        data.write(&self.uniform)
            .expect("TerrainBiome uniform layout");
        result.bindings.0.push((
            0,
            OwnedBindingResource::Buffer(device.create_buffer_with_data(&BufferInitDescriptor {
                label: Some("TerrainBiome parameters"),
                contents: data.as_ref(),
                usage: BufferUsages::UNIFORM,
            })),
        ));
        super::world_textures::bind_arrays(&mut result, &self.arrays, &params.1)?;
        self.uploaded.lock().unwrap().extend(
            self.arrays[..self.array_count as usize]
                .iter()
                .map(Handle::id),
        );
        Ok(result)
    }
    fn bind_group_layout_entries(device: &RenderDevice, force: bool) -> Vec<BindGroupLayoutEntry> {
        use binding_types::*;
        let mut result = SourceReceiver::bind_group_layout_entries(device, force);
        result.extend(
            BindGroupLayoutEntries::sequential(
                ShaderStages::VERTEX_FRAGMENT,
                (
                    uniform_buffer::<TerrainUniform>(false),
                    texture_2d_array(TextureSampleType::Float { filterable: true }),
                    sampler(SamplerBindingType::Filtering),
                    texture_2d_array(TextureSampleType::Float { filterable: true }),
                    sampler(SamplerBindingType::Filtering),
                    texture_2d_array(TextureSampleType::Float { filterable: true }),
                    sampler(SamplerBindingType::Filtering),
                    texture_2d_array(TextureSampleType::Float { filterable: true }),
                    sampler(SamplerBindingType::Filtering),
                    texture_2d_array(TextureSampleType::Float { filterable: true }),
                    sampler(SamplerBindingType::Filtering),
                ),
            )
            .to_vec(),
        );
        result
    }
}
