//! Bind the source nodes of scene instances. Each walk reads only the instance subtree.
use crate::reference::{AssetId, NodeId, NodeRecord, ReferenceManifest, SourceId};
use bevy::camera::visibility::RenderLayers;
use bevy::ecs as bevy_ecs;
use bevy::ecs::system::SystemParam;
use bevy::gltf::GltfExtras;
use bevy::prelude::*;

#[derive(serde::Deserialize)]
struct NodeExtras {
    active: bool,
    renderer_enabled: Option<bool>,
    layer: Option<u8>,
    cast_shadows: Option<u8>,
    receive_shadows: Option<bool>,
}
#[derive(serde::Deserialize)]
struct Identity {
    reference_id: Option<SourceId>,
}

/// Source identity of a glTF node, from its extras.
pub(crate) fn source_id(extra: &GltfExtras) -> Result<Option<SourceId>, serde_json::Error> {
    serde_json::from_str::<Identity>(&extra.value).map(|identity| identity.reference_id)
}

/// Scene graph reads for node binding.
#[derive(SystemParam)]
pub(crate) struct SceneGraph<'w, 's> {
    pub children: Query<'w, 's, &'static Children>,
    pub extras: Query<'w, 's, &'static GltfExtras>,
    meshes: Query<'w, 's, (), With<Mesh3d>>,
}

/// Source nodes and mesh primitives of one scene instance.
#[derive(Default)]
pub(crate) struct SceneBinding {
    pub nodes: Vec<(NodeId, Entity)>,
    pub primitives: Vec<Entity>,
    /// Nodes of the asset in the manifest.
    pub expected: usize,
    /// The first node that failed to bind. It is not in `nodes`.
    pub error: Option<String>,
}
impl SceneBinding {
    /// Every node of the asset has its entity.
    pub fn complete(&self) -> bool {
        self.nodes.len() == self.expected
    }
    /// Why the binding is not complete.
    pub fn failure(&self) -> String {
        let bound = format!("bound {} of {} nodes", self.nodes.len(), self.expected);
        match &self.error {
            Some(error) => format!("{bound}: {error}"),
            None => bound,
        }
    }
}

impl SceneGraph<'_, '_> {
    /// Mesh primitives directly under `node`.
    pub fn primitives(&self, node: Entity) -> impl Iterator<Item = Entity> + '_ {
        self.children
            .get(node)
            .into_iter()
            .flat_map(|children| children.iter())
            .filter(|child| self.meshes.contains(*child))
    }

    /// Bind the nodes of `asset` under `root` and collect its mesh primitives.
    /// Run once, when the instance is ready. A node that fails to bind is left out.
    pub fn bind_instance(
        &self,
        commands: &mut Commands,
        manifest: &ReferenceManifest,
        root: Entity,
        asset: AssetId,
    ) -> SceneBinding {
        let records: Vec<&NodeRecord> = manifest
            .nodes
            .iter()
            .filter(|node| node.asset == asset)
            .collect();
        let mut binding = SceneBinding {
            expected: records.len(),
            ..default()
        };
        for entity in self.children.iter_descendants(root) {
            if self.meshes.contains(entity) {
                binding.primitives.push(entity);
            }
            let Ok(extra) = self.extras.get(entity) else {
                continue;
            };
            let Ok(Some(source)) = source_id(extra) else {
                continue;
            };
            let Some(node) = records.iter().find(|node| node.source == source) else {
                continue;
            };
            if binding.nodes.iter().any(|(id, _)| *id == node.id) {
                continue;
            }
            match bind_node(commands, node, entity, extra, self) {
                Ok(()) => binding.nodes.push((node.id, entity)),
                Err(error) => {
                    binding.error.get_or_insert(error);
                }
            }
        }
        binding
    }
}

/// Apply checked source renderer flags without global node ownership.
pub(crate) fn bind_node(
    commands: &mut Commands,
    node: &NodeRecord,
    entity: Entity,
    extra: &GltfExtras,
    graph: &SceneGraph,
) -> Result<(), String> {
    let data: NodeExtras = serde_json::from_str(&extra.value)
        .map_err(|error| format!("Invalid glTF node {} extras: {error}", node.id.0))?;
    if !data.active {
        commands.entity(entity).insert(Visibility::Hidden);
    }
    for primitive in graph.primitives(entity) {
        let enabled = data
            .renderer_enabled
            .ok_or_else(|| format!("Missing renderer_enabled for glTF node {}", node.id.0))?;
        let layer = data
            .layer
            .filter(|layer| *layer < 32)
            .ok_or_else(|| format!("Missing source layer for glTF node {}", node.id.0))?;
        let cast = data
            .cast_shadows
            .filter(|mode| *mode <= 3)
            .ok_or_else(|| format!("Missing shadow mode for glTF node {}", node.id.0))?;
        commands.entity(primitive).insert((
            RenderLayers::layer(layer as usize),
            super::materials::SourceShadowMode(cast),
        ));
        if data.receive_shadows == Some(false) {
            commands
                .entity(primitive)
                .insert(bevy::light::NotShadowReceiver);
        }
        if !enabled {
            commands.entity(primitive).insert(Visibility::Hidden);
        }
    }
    Ok(())
}
