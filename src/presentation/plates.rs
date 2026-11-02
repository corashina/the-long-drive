//! Shared source TMP surface and retained text-local meshes.
use super::{
    materials::{RenderingState, SourceExtension, SourceMaterials},
    reference::ReferenceVisuals,
};
use crate::{
    app::RuntimeReference,
    plates::layout::{GlyphMesh, layout_text, sdf_parameters},
};
use bevy::{
    asset as bevy_asset, ecs as bevy_ecs, image as bevy_image, reflect as bevy_reflect,
    render as bevy_render,
};
use bevy::{
    asset::RenderAssetUsages,
    ecs::system::SystemParamItem,
    mesh::{Indices, MeshVertexBufferLayoutRef},
    pbr::{MaterialPipeline, MaterialPipelineKey},
    prelude::*,
    render::{render_resource::*, renderer::RenderDevice},
    shader::ShaderRef,
};

#[derive(Clone, Copy, Debug, ShaderType)]
pub struct PlateUniform {
    face: Vec4,
    outline: Vec4,
    sdf: Vec4,
    scale: Vec4,
    bevel: Vec4,
    bump: Vec4,
    specular: Vec4,
}
#[derive(Asset, TypePath, AsBindGroup, Clone, Debug)]
pub struct PlateSurface {
    #[uniform(0)]
    values: PlateUniform,
    #[texture(1)]
    #[sampler(2)]
    atlas: Handle<Image>,
    #[texture(3)]
    #[sampler(4)]
    face: Handle<Image>,
    #[texture(5)]
    #[sampler(6)]
    bump: Handle<Image>,
}
#[derive(Asset, TypePath, Clone, Debug)]
pub struct PlateTextMaterial {
    surface: PlateSurface,
    lighting: SourceExtension,
}
impl AsBindGroup for PlateTextMaterial {
    type Data = ();
    type Param = (
        <PlateSurface as AsBindGroup>::Param,
        <SourceExtension as AsBindGroup>::Param,
    );
    fn label() -> &'static str {
        "source plate text"
    }
    fn bind_group_data(&self) {}
    fn unprepared_bind_group(
        &self,
        layout: &BindGroupLayout,
        device: &RenderDevice,
        p: &mut SystemParamItem<Self::Param>,
        _: bool,
    ) -> Result<UnpreparedBindGroup, AsBindGroupError> {
        let mut result = self
            .surface
            .unprepared_bind_group(layout, device, &mut p.0, true)?;
        result.bindings.0.extend(
            self.lighting
                .unprepared_bind_group(layout, device, &mut p.1, true)?
                .bindings
                .0,
        );
        Ok(result)
    }
    fn bind_group_layout_entries(device: &RenderDevice, _: bool) -> Vec<BindGroupLayoutEntry> {
        let mut entries = PlateSurface::bind_group_layout_entries(device, true);
        entries.extend(SourceExtension::bind_group_layout_entries(device, true));
        entries
    }
}
impl Material for PlateTextMaterial {
    fn vertex_shader() -> ShaderRef {
        "shaders/source_plate_text.wgsl".into()
    }
    fn fragment_shader() -> ShaderRef {
        "shaders/source_plate_text.wgsl".into()
    }
    fn alpha_mode(&self) -> AlphaMode {
        AlphaMode::Blend
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
        _: MaterialPipelineKey<Self>,
    ) -> Result<(), SpecializedMeshPipelineError> {
        descriptor.layout[0] = super::source_lighting::source_camera_layout();
        descriptor.layout[1] = super::source_lighting::surface_lighting_layout();
        descriptor.vertex.buffers = vec![layout.0.get_layout(&[
            Mesh::ATTRIBUTE_POSITION.at_shader_location(0),
            Mesh::ATTRIBUTE_NORMAL.at_shader_location(1),
            Mesh::ATTRIBUTE_UV_0.at_shader_location(2),
            Mesh::ATTRIBUTE_UV_1.at_shader_location(3),
            Mesh::ATTRIBUTE_COLOR.at_shader_location(4),
        ])?];
        descriptor.primitive.cull_mode = None;
        if let Some(depth) = &mut descriptor.depth_stencil {
            depth.format = super::render_targets::DEPTH_FORMAT;
            depth.depth_write_enabled = Some(false);
            depth.depth_compare = Some(CompareFunction::GreaterEqual);
        }
        for target in descriptor
            .fragment
            .as_mut()
            .unwrap()
            .targets
            .iter_mut()
            .flatten()
        {
            target.write_mask = ColorWrites::RED | ColorWrites::GREEN | ColorWrites::BLUE;
            target.blend = Some(BlendState::ALPHA_BLENDING);
        }
        Ok(())
    }
}
#[derive(Resource)]
pub struct PlateTextStatus {
    state: RenderingState,
    /// A sample label mesh. It warms the shared material pipeline.
    sample: Option<Handle<Mesh>>,
    /// Root entities of the source instances of world car copies that show their plates.
    world: Vec<Entity>,
    material: Option<Handle<PlateTextMaterial>>,
}
impl Default for PlateTextStatus {
    fn default() -> Self {
        Self {
            state: RenderingState::Loading,
            sample: None,
            world: Vec::new(),
            material: None,
        }
    }
}
impl PlateTextStatus {
    pub fn ready(&self) -> bool {
        self.state == RenderingState::Ready
    }
    pub(crate) fn bound(&self) -> bool {
        self.material.is_some()
    }
    pub fn failure(&self) -> Option<&str> {
        if let RenderingState::Failed(e) = &self.state {
            Some(e)
        } else {
            None
        }
    }
}
pub struct PlateTextPlugin;
impl Plugin for PlateTextPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins(MaterialPlugin::<PlateTextMaterial>::default())
            .init_resource::<PlateTextStatus>()
            .add_systems(
                Update,
                (bind_plate_text, bind_world_plate_text)
                    .chain()
                    .after(super::materials::material_readiness),
            );
    }
}
fn mesh(glyphs: GlyphMesh) -> Mesh {
    let mut result = Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::MAIN_WORLD | RenderAssetUsages::RENDER_WORLD,
    );
    result.insert_attribute(
        Mesh::ATTRIBUTE_POSITION,
        glyphs
            .vertices
            .iter()
            .map(|v| v.position)
            .collect::<Vec<_>>(),
    );
    result.insert_attribute(
        Mesh::ATTRIBUTE_NORMAL,
        glyphs.vertices.iter().map(|v| v.normal).collect::<Vec<_>>(),
    );
    result.insert_attribute(
        Mesh::ATTRIBUTE_UV_0,
        glyphs.vertices.iter().map(|v| v.uv0).collect::<Vec<_>>(),
    );
    result.insert_attribute(
        Mesh::ATTRIBUTE_UV_1,
        glyphs.vertices.iter().map(|v| v.uv1).collect::<Vec<_>>(),
    );
    result.insert_attribute(
        Mesh::ATTRIBUTE_COLOR,
        glyphs.vertices.iter().map(|v| v.color).collect::<Vec<_>>(),
    );
    result.insert_indices(Indices::U32(glyphs.indices));
    result
}
/// Source sort data of a plate label with the shared plate material.
fn sort_data(
    record: &crate::reference::PlateRecord,
    sorting_layer: i16,
    sorting_order: i16,
) -> Result<super::renderer_visibility::SourceSortData, String> {
    let material_identity =
        0x80000000 | u32::try_from(record.material.source.path_id).map_err(|e| e.to_string())?;
    Ok(super::renderer_visibility::SourceSortData {
        material_identity,
        material_key: crate::reference::sorting::material_sort_key(material_identity, false, 0),
        glass_source: 0,
        shader_identity: i32::try_from(record.material.shader.source.path_id)
            .map_err(|e| e.to_string())?,
        source_material_index: 0,
        layer_and_order: (u32::from((i32::from(sorting_layer) + 0x8000) as u16) << 16)
            | u32::from((i32::from(sorting_order) + 0x8000) as u16),
        queue: if record.material.state.render_queue < 0 {
            3000
        } else {
            record.material.state.render_queue as i16
        },
        additive: false,
        glass: false,
        grab: false,
    })
}
pub fn bind_plate_text(world: &mut World) {
    if !world.resource::<ReferenceVisuals>().bound
        || world.resource::<PlateTextStatus>().failure().is_some()
    {
        return;
    }
    let result = bind(world);
    if let Err(e) = result {
        world.resource_mut::<PlateTextStatus>().state = RenderingState::Failed(e);
    }
}
/// The first plate label of the car prototypes: its layout and source layer. Reference
/// validation gives every label the shared font, material and layer.
fn sample_plate(
    reference: &crate::reference::ReferenceManifest,
) -> Option<(&crate::reference::PlateLayoutReference, usize)> {
    reference.world.prototypes.iter().find_map(|prototype| {
        let plate = prototype.vehicle.as_ref()?.plates.first()?;
        let layer = prototype.nodes[plate.text_node as usize].layer;
        Some((&plate.layout, usize::from(layer)))
    })
}
/// Builds the shared plate material and a sample label that warms its pipeline.
fn bind(world: &mut World) -> Result<(), String> {
    if !world.resource::<PlateTextStatus>().bound() {
        let reference = &world.resource::<RuntimeReference>().0;
        let record = &reference.plates;
        let Some((layout, layer)) = sample_plate(reference) else {
            world.resource_mut::<PlateTextStatus>().state = RenderingState::Ready;
            return Ok(());
        };
        if layer >= 32 {
            return Err("Missing plate source layer".into());
        }
        let source = world.resource::<SourceMaterials>();
        let texture = |slot: &str| {
            record
                .material
                .textures
                .iter()
                .find(|t| t.slot == slot)
                .and_then(|t| source.texture(t.file()))
        };
        let (Some(atlas), Some(face), Some(bump)) = (
            texture("_MainTex"),
            texture("_FaceTex"),
            texture("_BumpMap"),
        ) else {
            return Ok(());
        };
        let p = sdf_parameters(&record.material, layout);
        let f = &record.material.floats;
        let surface = PlateSurface {
            atlas,
            face,
            bump,
            values: PlateUniform {
                face: Vec4::from_array(p.face_color),
                outline: Vec4::from_array(p.outline_color),
                sdf: Vec4::new(
                    p.face_dilate,
                    p.outline_width,
                    p.outline_softness,
                    p.scale_ratio_a,
                ),
                scale: Vec4::new(
                    p.gradient_scale,
                    p.perspective_filter,
                    p.weight,
                    f["_Sharpness"],
                ),
                bevel: Vec4::new(
                    f["_Bevel"],
                    f["_BevelOffset"],
                    f["_BevelWidth"],
                    f["_BevelRoundness"],
                ),
                bump: Vec4::new(
                    f["_BumpFace"],
                    f["_BumpOutline"],
                    f["_FaceShininess"],
                    f["_OutlineShininess"],
                ),
                specular: Vec4::from_array(record.material.colors["_SpecColor"]),
            },
        };
        let glyphs = layout_text(&record.font, layout, &record.material, "AAA-000")
            .map_err(|e| e.to_string())?;
        let material = world
            .resource_mut::<Assets<PlateTextMaterial>>()
            .add(PlateTextMaterial {
                surface,
                lighting: SourceExtension {
                    lod_fade: 1.,
                    receiver_mask: 1 << layer,
                    surface: Default::default(),
                },
            });
        let sample = world.resource_mut::<Assets<Mesh>>().add(mesh(glyphs));
        super::materials::warm_retained_material(
            world,
            sample.id(),
            material.id().untyped(),
            "source plate text".into(),
        );
        let mut status = world.resource_mut::<PlateTextStatus>();
        status.sample = Some(sample);
        status.material = Some(material);
    }
    if !world.resource::<PlateTextStatus>().ready() {
        super::materials::seal_plate_materials(world);
        if world.resource::<SourceMaterials>().state == RenderingState::Ready {
            world.resource_mut::<PlateTextStatus>().state = RenderingState::Ready;
        }
    }
    Ok(())
}
/// Shows the plate labels of each shown world car copy. A copy binds once; its labels leave
/// with its instance. The sample label of `bind` warmed the shared pipeline.
pub fn bind_world_plate_text(world: &mut World) {
    if !world.resource::<PlateTextStatus>().ready() {
        return;
    }
    if let Err(e) = bind_world(world) {
        world.resource_mut::<PlateTextStatus>().state = RenderingState::Failed(e);
    }
}
fn bind_world(world: &mut World) -> Result<(), String> {
    world.resource_scope(|world, mut status: Mut<PlateTextStatus>| {
        let Some(WorldLabels {
            root,
            material,
            labels,
        }) = world_labels(world, &mut status)?
        else {
            return Ok(());
        };
        for WorldLabel {
            node,
            mesh,
            layer,
            sort,
            text,
        } in labels
        {
            let handle = world.resource_mut::<Assets<Mesh>>().add(mesh);
            world.spawn((
                Name::new(format!("Plate text {text}")),
                Mesh3d(handle),
                MeshMaterial3d(material.clone()),
                Transform::IDENTITY,
                Visibility::Inherited,
                bevy::camera::visibility::RenderLayers::layer(layer),
                ChildOf(node),
                sort,
                bevy::render::batching::NoAutomaticBatching,
            ));
        }
        status.world.push(root);
        Ok(())
    })
}
/// One plate label of a world car copy, ready to spawn.
struct WorldLabel {
    node: Entity,
    mesh: Mesh,
    layer: usize,
    sort: super::renderer_visibility::SourceSortData,
    text: String,
}
/// The labels of one car copy: its instance root and the shared material.
struct WorldLabels {
    root: Entity,
    material: Handle<PlateTextMaterial>,
    labels: Vec<WorldLabel>,
}
/// Drops the bindings of removed instances. Returns the labels of one shown car copy that has
/// none yet: its root, the shared material and each label's node, mesh, layer, sort and text.
fn world_labels(
    world: &World,
    status: &mut PlateTextStatus,
) -> Result<Option<WorldLabels>, String> {
    let Some(instances) = world.get_resource::<super::world::instances::SourceInstances>() else {
        return Ok(None);
    };
    // Labels of a removed instance despawned with its hierarchy.
    if status.world.iter().any(|root| !instances.has_root(*root)) {
        status.world.retain(|root| instances.has_root(*root));
    }
    let reference = &world.resource::<RuntimeReference>().0;
    let plate_texts = &world
        .resource::<super::sim_frame::SimFrameResource>()
        .plates;
    let Some(material) = status.material.clone() else {
        return Ok(None);
    };
    let Some((root, prototype, nodes, texts)) =
        instances
            .shown()
            .find_map(|(root, placement, prototype, nodes)| {
                let plates = &reference.world.prototypes[prototype]
                    .vehicle
                    .as_ref()?
                    .plates;
                if plates.is_empty() || status.world.contains(&root) {
                    return None;
                }
                let texts = plate_texts.get(&placement.id)?;
                Some((root, prototype, nodes, texts))
            })
    else {
        return Ok(None);
    };
    let record = &reference.plates;
    let prototype = &reference.world.prototypes[prototype];
    let receiver_mask = world
        .resource::<Assets<PlateTextMaterial>>()
        .get(&material)
        .ok_or("Missing plate material asset")?
        .lighting
        .receiver_mask;
    let plates = &prototype.vehicle.as_ref().unwrap().plates;
    if plates.len() != texts.len() {
        return Err("Car plate texts differ from its record".into());
    }
    let mut labels = Vec::with_capacity(texts.len());
    for (plate, text) in plates.iter().zip(texts) {
        let node = &prototype.nodes[plate.text_node as usize];
        let layer = usize::from(node.layer);
        if layer >= 32 || 1u32 << layer != receiver_mask {
            return Err("Shared plate source layer mismatch".into());
        }
        let glyphs = layout_text(&record.font, &plate.layout, &record.material, text)
            .map_err(|e| e.to_string())?;
        labels.push(WorldLabel {
            node: nodes[node.gltf_node as usize].ok_or("Plate node is not spawned")?,
            mesh: mesh(glyphs),
            layer,
            sort: sort_data(record, plate.sorting[0], plate.sorting[1])?,
            text: text.clone(),
        });
    }
    Ok(Some(WorldLabels {
        root,
        material,
        labels,
    }))
}
