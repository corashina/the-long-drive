//! Source octahedron atlas materials. Pose inputs use the frozen generation frame.
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
    shader::ShaderRef,
};
#[derive(Clone, Copy, Debug, ShaderType)]
pub struct ImpostorUniform {
    pub lod: Vec4,
    shape: Vec4,
    surface: Vec4,
    offset: Vec4,
    size_offset: Vec4,
    renderer: UVec4,
    pub source_position: Vec4,
    bindings: [UVec4; 4],
    sampling: [Vec4; 4],
}
#[derive(Asset, TypePath, Clone, Debug)]
pub struct ImpostorMaterial {
    pub preparation: crate::presentation::gpu_prepare::GpuPreparation,
    pub source: SourceId,
    pub uniform: ImpostorUniform,
    arrays: [Handle<Image>; 4],
    array_count: u8,
    receiver_mask: u32,
    reflected: bool,
    /// Static rows draw this material from the GPU prop buffers. Rows hold the fade and source position.
    gpu_props: bool,
    uploaded:
        std::sync::Arc<std::sync::Mutex<std::collections::HashSet<bevy::asset::AssetId<Image>>>>,
}
impl ImpostorMaterial {
    /// Only a shared material can draw GPU prop rows.
    pub fn use_gpu_props(&mut self) {
        self.gpu_props = true;
    }
    pub fn from_source(
        record: &WorldSourceMaterial,
        textures: &WorldTextures,
        mask: u32,
        receive_shadows: bool,
        reflected: bool,
        source_position: Vec3,
    ) -> Result<Self, ReferenceError> {
        let colored = match record.shader.name.as_str() {
            "Hidden/Amplify Impostors/Octahedron Impostor" => false,
            "Impostors/Custom/Colored Barrels" => true,
            _ => return Err(ReferenceError("Expected source impostor shader".into())),
        };
        if record
            .valid_keywords
            .iter()
            .any(|x| colored || x != "_HEMI_ON")
        {
            return Err(ReferenceError("Unsupported source impostor keyword".into()));
        }
        let hemi = record.valid_keywords.iter().any(|x| x == "_HEMI_ON");
        let number = |name: &str| {
            record
                .floats
                .get(name)
                .copied()
                .ok_or_else(|| ReferenceError(format!("Impostor missing {name}")))
        };
        let vector = |name: &str| {
            record
                .colors
                .get(name)
                .copied()
                .map(Vec4::from_array)
                .ok_or_else(|| ReferenceError(format!("Impostor missing {name}")))
        };
        let parameter =
            |name: &str| number(&format!("_{}{name}", if colored { "AI_" } else { "" }));
        let frames = parameter("Frames")?;
        if frames != 8. {
            return Err(ReferenceError(
                "Unsupported source impostor frame count".into(),
            ));
        }
        if !colored && number("_AI_AlphaToCoverage")? != 0. {
            return Err(ReferenceError(
                "Unsupported impostor alpha-to-coverage".into(),
            ));
        }
        let mut banks: Vec<(usize, Handle<Image>)> = Vec::with_capacity(4);
        let mut bindings = [UVec4::ZERO; 4];
        let mut sampling = [Vec4::ZERO; 4];
        let slots = if colored {
            ["_Normals", "_Albedo", "_Mask", ""]
        } else {
            ["_Normals", "_Albedo", "_Emission", "_Specular"]
        };
        for (slot, name) in slots.iter().enumerate() {
            if name.is_empty() {
                continue;
            }
            let (binding, image) = textures
                .binding(&record.source, name)
                .ok_or_else(|| ReferenceError(format!("Impostor texture not ready: {name}")))?;
            let bank = match banks.iter().position(|(id, _)| *id == binding.array) {
                Some(index) => index,
                None => {
                    banks.push((binding.array, image.clone()));
                    banks.len() - 1
                }
            };
            if bank >= 4 {
                return Err(ReferenceError("Impostor exceeds four exact arrays".into()));
            }
            bindings[slot] = UVec4::new(bank as u32, binding.layer, 0, 0);
            sampling[slot] = textures
                .sampling(binding.array)
                .ok_or_else(|| ReferenceError("Missing impostor sampler".into()))?;
        }
        let array_count = banks.len() as u8;
        let arrays = std::array::from_fn(|i| banks.get(i).unwrap_or(&banks[0]).1.clone());
        Ok(Self {
            preparation: default(),
            source: record.source.clone(),
            arrays,
            array_count,
            receiver_mask: mask,
            reflected,
            gpu_props: false,
            uploaded: textures.upload_receipts(),
            uniform: ImpostorUniform {
                lod: Vec4::X,
                shape: Vec4::new(
                    frames,
                    parameter("ImpostorSize")?,
                    parameter("Parallax")?,
                    parameter("DepthSize")?,
                ),
                surface: Vec4::new(
                    parameter("TextureBias")?,
                    number(if colored { "_AI_Clip" } else { "_ClipMask" })?,
                    number("_AI_ShadowBias")?,
                    number("_AI_ShadowView")?,
                ),
                offset: vector(if colored { "_AI_Offset" } else { "_Offset" })?,
                size_offset: vector("_AI_SizeOffset")?,
                renderer: UVec4::new(hemi as u32, colored as u32, receive_shadows as u32, 0),
                source_position: source_position.extend(0.),
                bindings,
                sampling,
            },
        })
    }
}
impl Material for ImpostorMaterial {
    fn vertex_shader() -> ShaderRef {
        "shaders/source_impostor.wgsl".into()
    }
    fn fragment_shader() -> ShaderRef {
        "shaders/source_impostor.wgsl".into()
    }
    fn prepass_vertex_shader() -> ShaderRef {
        "shaders/source_impostor.wgsl".into()
    }
    fn prepass_fragment_shader() -> ShaderRef {
        "shaders/source_impostor.wgsl".into()
    }
    fn enable_prepass() -> bool {
        false
    }
    fn specialize(
        _: &MaterialPipeline,
        descriptor: &mut RenderPipelineDescriptor,
        layout: &MeshVertexBufferLayoutRef,
        key: MaterialPipelineKey<Self>,
    ) -> Result<(), SpecializedMeshPipelineError> {
        let (reflected, gpu_props) = key.bind_group_data;
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
                    reflected,
                ));
            }
            descriptor.layout[0] = crate::presentation::source_lighting::source_camera_layout();
            descriptor.layout[1] = crate::presentation::source_lighting::surface_lighting_layout();
            if let Some(depth) = &mut descriptor.depth_stencil {
                depth.format = crate::presentation::render_targets::DEPTH_FORMAT;
            }
        }
        descriptor.vertex.buffers = vec![
            layout
                .0
                .get_layout(&[Mesh::ATTRIBUTE_POSITION.at_shader_location(0)])?,
        ];
        if gpu_props {
            descriptor.layout[2] = crate::presentation::prop_draws::draw_layout();
            // Depth-only passes have no fragment stage.
            let fragment = descriptor.fragment.as_mut().map(|f| &mut f.shader_defs);
            let vertex = &mut descriptor.vertex.shader_defs;
            for definitions in std::iter::once(vertex).chain(fragment) {
                definitions.push(bevy::shader::ShaderDefVal::Bool(
                    "SOURCE_GPU_PROPS".into(),
                    true,
                ));
            }
        }
        descriptor.primitive.cull_mode = Some(Face::Back);
        if reflected {
            descriptor.primitive.front_face = FrontFace::Cw;
        }
        Ok(())
    }
}
impl AsBindGroup for ImpostorMaterial {
    /// Reflected transform and GPU prop rows.
    type Data = (bool, bool);
    type Param = (
        <SourceReceiver as AsBindGroup>::Param,
        SRes<RenderAssets<GpuImage>>,
    );
    fn label() -> &'static str {
        "source impostor"
    }
    fn bind_group_data(&self) -> (bool, bool) {
        (self.reflected, self.gpu_props)
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
        data.write(&self.uniform).expect("impostor uniform layout");
        result.bindings.0.push((
            0,
            OwnedBindingResource::Buffer(device.create_buffer_with_data(&BufferInitDescriptor {
                label: Some("impostor parameters"),
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
                    uniform_buffer::<ImpostorUniform>(false),
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
