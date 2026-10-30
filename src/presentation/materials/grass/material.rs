use super::settings::settings_from_source;
use crate::{
    presentation::materials::SourceReceiver,
    reference::{ReferenceError, SourceId, WorldSourceMaterial},
};
use bevy::{
    asset as bevy_asset,
    ecs::system::{SystemParamItem, lifetimeless::SRes},
    mesh::{MeshVertexAttribute, MeshVertexBufferLayoutRef},
    pbr::{MaterialPipeline, MaterialPipelineKey},
    prelude::*,
    reflect as bevy_reflect,
    render::{render_resource::*, renderer::RenderDevice},
    shader::ShaderRef,
};
use grass_mesh::GrassView;
pub const BLADE_ATTRIBUTE: MeshVertexAttribute =
    MeshVertexAttribute::new("SourceGrassBlade", 984632517, VertexFormat::Float32x4);
#[derive(Clone, Copy, Debug, ShaderType)]
pub struct GrassUniform {
    pub lod: Vec4,
    pub camera: Vec4,
    pub shape: Vec4,
    pub fade_lod: Vec4,
    pub wind: Vec4,
    pub options: Vec4,
    pub color: Vec4,
    pub secondary: Vec4,
    pub bottom: Vec4,
    pub burn: Vec4,
    pub smoothing: UVec4,
}
#[derive(Asset, TypePath, Clone, Debug)]
pub struct GrassMaterial {
    pub preparation: crate::presentation::gpu_prepare::GpuPreparation,
    pub session: u64,
    pub source: SourceId,
    pub uniform: GrassUniform,
    pub receiver_mask: u32,
    pub cull: u8,
}
impl GrassMaterial {
    pub fn displacement_radius(&self) -> f32 {
        let bend = 2f32.sqrt() * (self.uniform.wind.x.abs() + self.uniform.wind.z.abs())
            + 1.8 * self.uniform.options.y.abs();
        let height = self.uniform.shape.y.abs().max(self.uniform.shape.z.abs());
        height * (1. + 2. * (1. + bend) * self.uniform.shape.w.abs()) + self.uniform.shape.x.abs()
    }
    pub fn from_source(
        session: u64,
        record: &WorldSourceMaterial,
        view: GrassView,
        receiver_mask: u32,
    ) -> Result<Self, ReferenceError> {
        if record.shader.name != "Stix Games/Grass" {
            return Err(ReferenceError("Expected checked Grass material".into()));
        }
        let settings = settings_from_source(record)?;
        for required in [
            "SIMPLE_GRASS",
            "UNIFORM_DENSITY",
            "GRASS_UNSHADED_LIGHTING",
            "GRASS_CALC_GLOBAL_WIND",
            "GRASS_TOP_VIEW_COMPENSATION",
        ] {
            if !record.valid_keywords.iter().any(|x| x == required) {
                return Err(ReferenceError(format!(
                    "Unsupported Grass variant: missing {required}"
                )));
            }
        }
        for keyword in &record.valid_keywords {
            if !matches!(
                keyword.as_str(),
                "SIMPLE_GRASS"
                    | "UNIFORM_DENSITY"
                    | "GRASS_UNSHADED_LIGHTING"
                    | "GRASS_CALC_GLOBAL_WIND"
                    | "GRASS_TOP_VIEW_COMPENSATION"
                    | "GRASS_HEIGHT_SMOOTHING"
                    | "GRASS_WIDTH_SMOOTHING"
            ) {
                return Err(ReferenceError(format!(
                    "Unsupported Grass keyword {keyword}"
                )));
            }
        }
        if record
            .textures
            .iter()
            .any(|link| link.property == "_ColorMap")
        {
            return Err(ReferenceError(
                "Grass selected ColorMap is no longer the checked white default".into(),
            ));
        }
        let get = |name: &str| {
            record
                .floats
                .get(name)
                .copied()
                .ok_or_else(|| ReferenceError(format!("Grass missing {name}")))
        };
        let vector = |name: &str| {
            record
                .colors
                .get(name)
                .copied()
                .map(Vec4::from_array)
                .ok_or_else(|| ReferenceError(format!("Grass missing {name}")))
        };
        let color = |name: &str| {
            vector(name).map(|v| {
                Vec4::from_array(Color::srgba(v.x, v.y, v.z, v.w).to_linear().to_f32_array())
            })
        };
        let enabled = |name: &str| u32::from(record.valid_keywords.iter().any(|x| x == name));
        let cull = get("_CullMode")?;
        if !matches!(cull, 0.0 | 1.0 | 2.0) {
            return Err(ReferenceError("Unsupported Grass culling state".into()));
        }
        Ok(Self {
            preparation: default(),
            session,
            source: record.source.clone(),
            receiver_mask,
            cull: cull as u8,
            uniform: GrassUniform {
                lod: Vec4::X,
                camera: view.position.extend(view.screen_height),
                shape: Vec4::new(
                    get("_Width00")?,
                    get("_MinHeight00")?,
                    settings.max_height,
                    get("_Softness00")?,
                ),
                fade_lod: Vec4::new(
                    get("_GrassFadeStart")? * super::settings::quality_scale().1,
                    settings.fade_end,
                    settings.lod_start,
                    settings.lod_end,
                ),
                wind: vector("_WindParams")?,
                options: Vec4::new(
                    get("_WindRotation")?,
                    get("_Disorder")?,
                    get("_BurnCutoff")?,
                    0.,
                ),
                color: color("_Color00")?,
                secondary: color("_SecColor00")?,
                bottom: color("_GrassBottomColor")?,
                burn: color("_BurnColor")?,
                smoothing: UVec4::new(
                    enabled("GRASS_HEIGHT_SMOOTHING"),
                    enabled("GRASS_WIDTH_SMOOTHING"),
                    0,
                    0,
                ),
            },
        })
    }
}
impl Material for GrassMaterial {
    fn vertex_shader() -> ShaderRef {
        "shaders/source_grass.wgsl".into()
    }
    fn fragment_shader() -> ShaderRef {
        "shaders/source_grass.wgsl".into()
    }
    fn prepass_vertex_shader() -> ShaderRef {
        "shaders/source_grass.wgsl".into()
    }
    fn prepass_fragment_shader() -> ShaderRef {
        "shaders/source_grass.wgsl".into()
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
            Mesh::ATTRIBUTE_UV_0.at_shader_location(1),
            BLADE_ATTRIBUTE.at_shader_location(2),
        ])?];
        descriptor.primitive.cull_mode = match key.bind_group_data {
            0 => None,
            1 => Some(Face::Front),
            _ => Some(Face::Back),
        };
        Ok(())
    }
}
impl AsBindGroup for GrassMaterial {
    type Data = u8;
    type Param = (
        <SourceReceiver as AsBindGroup>::Param,
        SRes<super::displacement::GrassGpu>,
    );
    fn label() -> &'static str {
        "source Grass"
    }
    fn bind_group_data(&self) -> u8 {
        self.cull
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
        let gpu = params
            .1
            .0
            .get(&self.session)
            .ok_or(AsBindGroupError::RetryNextUpdate)?;
        let extension = SourceReceiver {
            lod_fade: 1.,
            receiver_mask: self.receiver_mask,
        };
        let mut result = extension.unprepared_bind_group(layout, device, &mut params.0, true)?;
        let mut data = encase::UniformBuffer::new(Vec::new());
        data.write(&self.uniform).expect("Grass uniform");
        result.bindings.0.push((
            0,
            OwnedBindingResource::Buffer(device.create_buffer_with_data(&BufferInitDescriptor {
                label: Some("source Grass parameters"),
                contents: data.as_ref(),
                usage: BufferUsages::UNIFORM,
            })),
        ));
        result.bindings.0.push((
            1,
            OwnedBindingResource::TextureView(TextureViewDimension::D2, gpu.color_view.clone()),
        ));
        result.bindings.0.push((
            2,
            OwnedBindingResource::Sampler(SamplerBindingType::Filtering, gpu.sampler.clone()),
        ));
        result
            .bindings
            .0
            .push((3, OwnedBindingResource::Buffer(gpu.uniform.clone())));
        Ok(result)
    }
    fn bind_group_layout_entries(device: &RenderDevice, force: bool) -> Vec<BindGroupLayoutEntry> {
        use binding_types::*;
        let mut result = SourceReceiver::bind_group_layout_entries(device, force);
        result.extend(
            BindGroupLayoutEntries::sequential(
                ShaderStages::VERTEX_FRAGMENT,
                (
                    uniform_buffer::<GrassUniform>(false),
                    texture_2d(TextureSampleType::Float { filterable: true }),
                    sampler(SamplerBindingType::Filtering),
                    uniform_buffer::<super::displacement::GrassFrameUniform>(false),
                ),
            )
            .to_vec(),
        );
        result
    }
}
