//! Authored glTF accessor loading. Bevy's loader changes absent normal/tangent streams.
use bevy::tasks::futures_lite::AsyncReadExt;
use bevy::{
    asset as bevy_asset,
    asset::{AssetLoader, LoadContext, RenderAssetUsages, io::Reader},
    mesh::{Indices, PrimitiveTopology, skinning::SkinnedMeshInverseBindposes},
    prelude::*,
    reflect as bevy_reflect,
};
#[derive(Asset, TypePath)]
pub struct SourceGltf {
    #[dependency]
    pub meshes: Vec<Handle<Mesh>>,
    pub primitives: Vec<(usize, usize, usize)>,
    #[dependency]
    pub inverse_bindposes: Vec<Handle<SkinnedMeshInverseBindposes>>,
    pub joints: Vec<Vec<usize>>,
    pub mesh_bytes: usize,
    pub skin_bytes: usize,
}
/// Typed loads select this loader. Leave glTF labels to Bevy's loader.
#[derive(Default, TypePath)]
pub struct SourceGltfLoader;
#[derive(Clone, Copy, Default, serde::Serialize, serde::Deserialize)]
pub struct SourceGltfSettings {
    pub json_bytes: usize,
    pub buffer_bytes: usize,
    pub mesh_bytes: usize,
}
impl AssetLoader for SourceGltfLoader {
    type Asset = SourceGltf;
    type Settings = SourceGltfSettings;
    type Error = std::io::Error;
    async fn load(
        &self,
        reader: &mut dyn Reader,
        settings: &Self::Settings,
        context: &mut LoadContext<'_>,
    ) -> Result<Self::Asset, Self::Error> {
        let invalid =
            |message: String| std::io::Error::new(std::io::ErrorKind::InvalidData, message);
        if settings.json_bytes == 0 || settings.json_bytes > 32 * 1024 * 1024 {
            return Err(invalid("Unadmitted prototype file".into()));
        }
        let mut bytes = Vec::with_capacity(settings.json_bytes);
        reader
            .take(settings.json_bytes as u64 + 1)
            .read_to_end(&mut bytes)
            .await?;
        if bytes.len() != settings.json_bytes {
            return Err(invalid("Prototype changed from the receipt".into()));
        }
        let document = gltf::Gltf::from_slice(&bytes).map_err(|e| invalid(e.to_string()))?;
        let directory = context
            .path()
            .path()
            .parent()
            .ok_or_else(|| invalid("Missing prototype directory".into()))?
            .to_path_buf();
        let mut buffers = Vec::with_capacity(document.buffers().len());
        let mut buffer_bytes = 0usize;
        for buffer in document.buffers() {
            let gltf::buffer::Source::Uri(uri) = buffer.source() else {
                return Err(invalid("Unexpected embedded prototype buffer".into()));
            };
            if uri.contains('/') || uri.contains('\\') || uri.contains(':') || uri.contains("..") {
                return Err(invalid("Invalid prototype buffer URI".into()));
            }
            buffer_bytes = buffer_bytes
                .checked_add(buffer.length())
                .ok_or_else(|| invalid("Prototype buffer overflow".into()))?;
            if buffer_bytes > settings.buffer_bytes {
                return Err(invalid("Unadmitted prototype buffer allocation".into()));
            }
            let data = context
                .read_asset_bytes(directory.join(uri))
                .await
                .map_err(|e| invalid(e.to_string()))?;
            if data.len() != buffer.length() {
                return Err(invalid(
                    "Prototype buffer length differs from the receipt".into(),
                ));
            }
            buffers.push(data);
        }
        let mut result = SourceGltf {
            meshes: Vec::new(),
            primitives: Vec::new(),
            inverse_bindposes: Vec::new(),
            joints: Vec::new(),
            mesh_bytes: 0,
            skin_bytes: 0,
        };
        for node in document.nodes() {
            let Some(source_mesh) = node.mesh() else {
                continue;
            };
            let extras: crate::reference::prototype_plan::NodeExtras = serde_json::from_str(
                node.extras()
                    .as_ref()
                    .ok_or_else(|| invalid("Missing source renderer extras".into()))?
                    .get(),
            )
            .map_err(|e| invalid(e.to_string()))?;
            let checked = extras
                .source_renderer
                .as_ref()
                .ok_or_else(|| invalid("Missing checked source renderer record".into()))?;
            if extras.cast_shadows != Some(checked.cast_shadows)
                || extras.receive_shadows != Some(checked.receive_shadows)
            {
                return Err(invalid("Conflicting source renderer flags".into()));
            }
            let renderer = extras
                .source_renderer
                .ok_or_else(|| invalid("Missing checked source renderer record".into()))?;
            renderer.validate().map_err(|e| invalid(e.to_string()))?;
            for primitive in source_mesh.primitives() {
                if primitive.mode() != gltf::mesh::Mode::Triangles
                    || primitive.morph_targets().len() != 0
                {
                    return Err(invalid(
                        "Unresolved prototype primitive topology or morph input".into(),
                    ));
                }
                for (semantic, _) in primitive.attributes() {
                    match semantic {
                        gltf::Semantic::Positions
                        | gltf::Semantic::Normals
                        | gltf::Semantic::Tangents
                        | gltf::Semantic::Colors(0)
                        | gltf::Semantic::Joints(0)
                        | gltf::Semantic::Weights(0)
                        | gltf::Semantic::TexCoords(0..=3) => {}
                        other => {
                            return Err(invalid(format!(
                                "Unresolved authored vertex stream {other:?}"
                            )));
                        }
                    }
                }
                let source = primitive.reader(|buffer| Some(buffers[buffer.index()].as_slice()));
                let mut mesh = Mesh::new(
                    PrimitiveTopology::TriangleList,
                    RenderAssetUsages::MAIN_WORLD | RenderAssetUsages::RENDER_WORLD,
                );
                let positions: Vec<_> = source
                    .read_positions()
                    .ok_or_else(|| invalid("Missing authored positions".into()))?
                    .collect();
                let mut size = positions.capacity() * 12;
                mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, positions);
                if let Some(values) = source.read_normals() {
                    let values: Vec<_> = values.collect();
                    size += values.capacity() * 12;
                    mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, values);
                }
                if let Some(values) = source.read_tangents() {
                    let values: Vec<_> = values.collect();
                    size += values.capacity() * 16;
                    mesh.insert_attribute(Mesh::ATTRIBUTE_TANGENT, values);
                }
                for (set, attribute) in [
                    (0, Mesh::ATTRIBUTE_UV_0),
                    (1, Mesh::ATTRIBUTE_UV_1),
                    (
                        2,
                        bevy::mesh::MeshVertexAttribute::new(
                            "SourceUv2",
                            984632518,
                            bevy::render::render_resource::VertexFormat::Float32x2,
                        ),
                    ),
                    (
                        3,
                        bevy::mesh::MeshVertexAttribute::new(
                            "SourceUv3",
                            984632519,
                            bevy::render::render_resource::VertexFormat::Float32x2,
                        ),
                    ),
                ] {
                    if let Some(values) = source.read_tex_coords(set) {
                        let values: Vec<_> = values.into_f32().collect();
                        size += values.capacity() * 8;
                        mesh.insert_attribute(attribute, values);
                    }
                }
                if let Some(values) = source.read_colors(0) {
                    let values: Vec<_> = values.into_rgba_f32().collect();
                    size += values.capacity() * 16;
                    mesh.insert_attribute(Mesh::ATTRIBUTE_COLOR, values);
                }
                if let Some(values) = source.read_joints(0) {
                    let values: Vec<_> = values.into_u16().collect();
                    size += values.capacity() * 8;
                    mesh.insert_attribute(
                        Mesh::ATTRIBUTE_JOINT_INDEX,
                        bevy::mesh::VertexAttributeValues::Uint16x4(values),
                    );
                }
                if let Some(values) = source.read_weights(0) {
                    let values: Vec<_> = values.into_f32().collect();
                    size += values.capacity() * 16;
                    mesh.insert_attribute(Mesh::ATTRIBUTE_JOINT_WEIGHT, values);
                }
                if let Some(values) = source.read_indices() {
                    let values: Vec<_> = values.into_u32().collect();
                    size += values.capacity() * 4;
                    mesh.insert_indices(Indices::U32(values));
                }
                let [center, extents] = renderer.attributes();
                let vertices = mesh.count_vertices();
                mesh.insert_attribute(
                    crate::presentation::renderers::SOURCE_BOUNDS_CENTER,
                    vec![center; vertices],
                );
                mesh.insert_attribute(
                    crate::presentation::renderers::SOURCE_BOUNDS_EXTENTS,
                    vec![extents; vertices],
                );
                size += vertices * 32;
                result.mesh_bytes = result
                    .mesh_bytes
                    .checked_add(size)
                    .ok_or_else(|| invalid("Prototype mesh allocation overflow".into()))?;
                if result.mesh_bytes > settings.mesh_bytes {
                    return Err(invalid(
                        "Prototype decoded mesh exceeded its reservation".into(),
                    ));
                }
                result
                    .primitives
                    .push((node.index(), source_mesh.index(), primitive.index()));
                result.meshes.push(context.add_labeled_asset(
                    format!(
                        "Node{}/Mesh{}/Primitive{}",
                        node.index(),
                        source_mesh.index(),
                        primitive.index()
                    ),
                    mesh,
                ));
            }
        }
        for skin in document.skins() {
            let source = skin.reader(|buffer| Some(buffers[buffer.index()].as_slice()));
            let matrices = source
                .read_inverse_bind_matrices()
                .ok_or_else(|| invalid("Missing authored bind poses".into()))?
                .map(|matrix| Mat4::from_cols_array_2d(&matrix))
                .collect::<Vec<_>>();
            let joints = skin.joints().map(|node| node.index()).collect::<Vec<_>>();
            if joints.len() != matrices.len() {
                return Err(invalid("Source skin bone/bind-pose count differs".into()));
            }
            result.skin_bytes +=
                matrices.capacity() * 64 + joints.capacity() * std::mem::size_of::<usize>();
            result.inverse_bindposes.push(context.add_labeled_asset(
                format!("Skin{}/InverseBindposes", skin.index()),
                SkinnedMeshInverseBindposes::from(matrices),
            ));
            result.joints.push(joints);
        }
        Ok(result)
    }
}
