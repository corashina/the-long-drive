//! Selected Standard (Specular setup) with authored mip arrays and explicit reflectance.
use super::{SourceReceiver, SourceSurface, SourceUniform, world_textures::WorldTextures};
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
#[derive(Asset, TypePath, Clone, Debug)]
pub struct SpecularMaterial {
    pub preparation: crate::presentation::gpu_prepare::GpuPreparation,
    pub lod_fade: f32,
    pub source: SourceId,
    base: StandardMaterial,
    uniform: SourceUniform,
    arrays: [Handle<Image>; 4],
    array_count: u8,
    receiver_mask: u32,
    reflected: bool,
    /// Static rows draw this material from the GPU prop buffers.
    gpu_props: bool,
    uploaded:
        std::sync::Arc<std::sync::Mutex<std::collections::HashSet<bevy::asset::AssetId<Image>>>>,
}
impl SpecularMaterial {
    pub(crate) fn shadow_batch_state(&self) -> crate::presentation::shadow_batch::CasterBatchState {
        crate::presentation::shadow_batch::CasterBatchState::opaque_specular(
            self.uniform.flags.x,
            self.lod_fade,
        )
    }

    /// Only a shared material can draw GPU prop rows.
    pub fn use_gpu_props(&mut self) {
        self.gpu_props = true;
    }
    /// Linear color.
    pub fn color(&self) -> Vec4 {
        self.uniform.color
    }
    pub fn set_color(&mut self, color: Color, linear: Vec4) {
        self.base.base_color = color;
        self.uniform.color = linear;
    }
    /// Linear `_EmissionColor`.
    pub fn emission(&self) -> Vec4 {
        self.uniform.emission
    }
    pub fn set_emission(&mut self, linear: Vec4) {
        self.uniform.emission = linear;
    }
    pub fn from_source(
        record: &WorldSourceMaterial,
        textures: &WorldTextures,
        mask: u32,
        reflected: bool,
    ) -> Result<Self, ReferenceError> {
        // Emission is always on. A normal map is optional.
        let normal = record.valid_keywords.iter().any(|x| x == "_NORMALMAP");
        if record.shader.name != "Standard (Specular setup)"
            || record.valid_keywords.len() != 1 + usize::from(normal)
            || !record.valid_keywords.iter().any(|x| x == "_EMISSION")
        {
            return Err(ReferenceError("Unsupported source specular variant".into()));
        }
        let number = |name: &str| {
            record
                .floats
                .get(name)
                .copied()
                .ok_or_else(|| ReferenceError(format!("Specular material missing {name}")))
        };
        for (name, expected) in [
            ("_Mode", 0.),
            ("_ZWrite", 1.),
            ("_SrcBlend", 1.),
            ("_DstBlend", 0.),
            ("_SpecularHighlights", 1.),
            ("_GlossyReflections", 1.),
        ] {
            if number(name)? != expected {
                return Err(ReferenceError(format!(
                    "Unsupported source specular state {name}"
                )));
            }
        }
        let color = |name: &str| {
            record
                .colors
                .get(name)
                .copied()
                .map(|v| {
                    Vec4::from_array(
                        Color::srgba(v[0], v[1], v[2], v[3])
                            .to_linear()
                            .to_f32_array(),
                    )
                })
                .ok_or_else(|| ReferenceError(format!("Specular material missing {name}")))
        };
        let mut uniform = SourceSurface::default().data;
        uniform.color = color("_Color")?;
        uniform.emission = color("_EmissionColor")?;
        // _SpecColor is a linear reflectance value in the checked native constant buffer.
        let specular = record
            .colors
            .get("_SpecColor")
            .ok_or_else(|| ReferenceError("Missing _SpecColor".into()))?;
        uniform.specular = Vec4::new(specular[0], specular[1], specular[2], 1.);
        uniform.factors = Vec4::new(
            0.,
            number("_Glossiness")?,
            number("_GlossMapScale")?,
            number("_Cutoff")?,
        );
        uniform.scales = Vec4::new(number("_BumpScale")?, 0., number("_OcclusionStrength")?, 0.);
        uniform.flags = UVec4::new(u32::from(normal) | (1 << 3), 0, 0, 0);
        let uv = *record
            .texture_coordinates
            .get("_MainTex")
            .ok_or_else(|| ReferenceError("Missing specular _MainTex_ST".into()))?;
        uniform.uv = [Vec4::from_array(uv); 9];
        uniform.defaults[1] = Vec4::new(0.5, 0.5, 1., 1.);
        // Checked shader's null emission default is white; its source emission color is zero.
        if uniform.emission.truncate() != Vec3::ZERO {
            return Err(ReferenceError(
                "Unresolved nonzero null specular emission".into(),
            ));
        }
        let mut banks: Vec<(usize, Handle<Image>)> = Vec::with_capacity(4);
        for (slot, name) in [(0, "_MainTex"), (1, "_BumpMap"), (3, "_OcclusionMap")] {
            if slot == 1 && !normal {
                continue;
            }
            let (binding, image) = textures
                .binding(&record.source, name)
                .ok_or_else(|| ReferenceError(format!("Specular texture not ready: {name}")))?;
            let bank = match banks.iter().position(|(id, _)| *id == binding.array) {
                Some(index) => index,
                None => {
                    banks.push((binding.array, image.clone()));
                    banks.len() - 1
                }
            };
            if bank >= 4 {
                return Err(ReferenceError(
                    "Specular material exceeds four exact arrays".into(),
                ));
            }
            uniform.banks[slot / 4][slot % 4] = bank as u32;
            uniform.layers[slot / 4][slot % 4] = binding.layer;
            uniform.flags.y |= 1 << slot;
            let sampling = textures
                .sampling(binding.array)
                .ok_or_else(|| ReferenceError("Missing source specular sampler".into()))?;
            uniform.sampling[slot] = Vec4::new(0., sampling.x, sampling.y, sampling.z);
        }
        let array_count = banks.len() as u8;
        let arrays = std::array::from_fn(|i| banks.get(i).unwrap_or(&banks[0]).1.clone());
        Ok(Self {
            preparation: default(),
            lod_fade: 1.,
            source: record.source.clone(),
            base: StandardMaterial::default(),
            uniform,
            arrays,
            array_count,
            receiver_mask: mask,
            reflected,
            gpu_props: false,
            uploaded: textures.upload_receipts(),
        })
    }
}
impl Material for SpecularMaterial {
    fn vertex_shader() -> ShaderRef {
        "shaders/source_mesh.wgsl".into()
    }
    fn fragment_shader() -> ShaderRef {
        "shaders/source_surface.wgsl".into()
    }
    fn enable_prepass() -> bool {
        false
    }
    fn enable_shadows() -> bool {
        false
    }
    fn specialize(
        pipeline: &MaterialPipeline,
        descriptor: &mut RenderPipelineDescriptor,
        layout: &MeshVertexBufferLayoutRef,
        key: MaterialPipelineKey<Self>,
    ) -> Result<(), SpecializedMeshPipelineError> {
        StandardMaterial::specialize(
            pipeline,
            descriptor,
            layout,
            MaterialPipelineKey::<StandardMaterial> {
                mesh_key: key.mesh_key,
                bind_group_data: StandardMaterial::default().bind_group_data(),
            },
        )?;
        descriptor.layout[0] = crate::presentation::source_lighting::source_camera_layout();
        descriptor.layout[1] = crate::presentation::source_lighting::surface_lighting_layout();
        let mut attributes = Vec::new();
        for (location, attribute) in [
            Mesh::ATTRIBUTE_POSITION,
            Mesh::ATTRIBUTE_NORMAL,
            Mesh::ATTRIBUTE_UV_0,
            Mesh::ATTRIBUTE_UV_1,
            Mesh::ATTRIBUTE_TANGENT,
            Mesh::ATTRIBUTE_COLOR,
            // Bevy reads joints at 6 and weights at 7 for a skinned mesh, such as the siphon hose.
            Mesh::ATTRIBUTE_JOINT_INDEX,
            Mesh::ATTRIBUTE_JOINT_WEIGHT,
        ]
        .into_iter()
        .enumerate()
        {
            if layout.0.contains(attribute) {
                attributes.push(attribute.at_shader_location(location as u32));
            }
        }
        attributes.push(crate::presentation::renderers::SOURCE_BOUNDS_CENTER.at_shader_location(8));
        attributes
            .push(crate::presentation::renderers::SOURCE_BOUNDS_EXTENTS.at_shader_location(9));
        descriptor.vertex.buffers = vec![layout.0.get_layout(&attributes)?];
        if let Some(depth) = &mut descriptor.depth_stencil {
            depth.format = crate::presentation::render_targets::DEPTH_FORMAT;
        }
        let (reflected, gpu_props) = key.bind_group_data;
        descriptor.vertex.shader_defs.push(ShaderDefVal::Bool(
            "SOURCE_OBJECT_REFLECTED".into(),
            reflected,
        ));
        if reflected {
            descriptor.primitive.front_face = FrontFace::Cw;
        }
        if gpu_props {
            descriptor.layout[2] = crate::presentation::prop_draws::draw_layout();
            descriptor
                .vertex
                .shader_defs
                .push(ShaderDefVal::Bool("SOURCE_GPU_PROPS".into(), true));
        }
        if let Some(fragment) = &mut descriptor.fragment {
            fragment.shader_defs.extend([
                ShaderDefVal::Bool("SOURCE_SURFACE".into(), true),
                ShaderDefVal::Bool("SOURCE_STANDARD".into(), true),
                ShaderDefVal::Bool("SOURCE_OBJECT_REFLECTED".into(), reflected),
                ShaderDefVal::Bool("WORLD_TEXTURE_ARRAYS".into(), true),
            ]);
            if gpu_props {
                fragment
                    .shader_defs
                    .push(ShaderDefVal::Bool("SOURCE_GPU_PROPS".into(), true));
            }
        }
        Ok(())
    }
}
impl AsBindGroup for SpecularMaterial {
    /// Reflected transform and GPU prop rows.
    type Data = (bool, bool);
    type Param = (
        <SourceReceiver as AsBindGroup>::Param,
        SRes<RenderAssets<GpuImage>>,
        <StandardMaterial as AsBindGroup>::Param,
    );
    fn label() -> &'static str {
        "source specular surface"
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
            lod_fade: self.lod_fade,
            receiver_mask: self.receiver_mask,
        };
        let mut result = extension.unprepared_bind_group(layout, device, &mut params.0, true)?;
        let base = self
            .base
            .unprepared_bind_group(layout, device, &mut params.2, true)?;
        result
            .bindings
            .0
            .extend(base.bindings.0.into_iter().filter(|(index, _)| *index == 0));
        let mut data = encase::UniformBuffer::new(Vec::new());
        data.write(&self.uniform)
            .expect("specular surface uniform layout");
        result.bindings.0.push((
            120,
            OwnedBindingResource::Buffer(device.create_buffer_with_data(&BufferInitDescriptor {
                label: Some("specular surface parameters"),
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
            StandardMaterial::bind_group_layout_entries(device, true)
                .into_iter()
                .filter(|entry| entry.binding == 0),
        );
        result.extend(
            BindGroupLayoutEntries::with_indices(
                ShaderStages::FRAGMENT,
                ((120, uniform_buffer::<SourceUniform>(false)),),
            )
            .to_vec(),
        );
        result.extend(
            BindGroupLayoutEntries::sequential(
                ShaderStages::VERTEX_FRAGMENT,
                (
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
            .to_vec()
            .into_iter()
            .enumerate()
            .map(|(index, mut entry)| {
                entry.binding = 1 + index as u32;
                entry
            }),
        );
        result
    }
}
