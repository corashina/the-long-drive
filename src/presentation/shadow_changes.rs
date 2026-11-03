//! Main-world caster mutation inputs. ViewVisibility is refreshed separately every frame.
//! Asset edits come from asset events, not from `Assets` change ticks: any `&mut Assets`
//! access marks the whole resource changed, and world preparation takes one every frame.
use super::{
    materials::{
        SourceMaterial, grass::material::GrassMaterial, impostor::ImpostorMaterial,
        specular::SpecularMaterial, terrain::TerrainMaterial,
    },
    renderer_visibility::{SourceMaterialWarmup, SourceRenderer},
};
use bevy::ecs as bevy_ecs;
use bevy::{
    camera::{
        primitives::Aabb,
        visibility::{InheritedVisibility, RenderLayers, ViewVisibility},
    },
    ecs::{query::QueryData, system::SystemParam},
    mesh::skinning::SkinnedMesh,
    prelude::*,
};

#[derive(QueryData)]
pub(super) struct ShadowMesh {
    pub entity: Entity,
    pub bounds: &'static Aabb,
    pub transform: &'static GlobalTransform,
    pub inherited: &'static InheritedVisibility,
    pub record: &'static SourceRenderer,
    pub mesh: Option<&'static Mesh3d>,
    pub layers: Option<&'static RenderLayers>,
    pub skinned: Has<SkinnedMesh>,
    pub material: Option<&'static MeshMaterial3d<SourceMaterial>>,
    pub specular: Option<&'static MeshMaterial3d<SpecularMaterial>>,
    pub terrain: Option<&'static MeshMaterial3d<TerrainMaterial>>,
    pub grass: Option<&'static MeshMaterial3d<GrassMaterial>>,
    pub impostor: Option<&'static MeshMaterial3d<ImpostorMaterial>>,
}

impl ShadowMeshItem<'_, '_> {
    /// Grass sways with time and skinned meshes follow their joints. Their tiles draw each frame.
    pub fn animated(&self) -> bool {
        self.grass.is_some() || self.skinned
    }
    /// Mesh and material identity, for the lamp face pose hash.
    pub fn hash_assets(&self, hasher: &mut impl std::hash::Hasher) {
        use std::hash::Hash;
        self.mesh.map(|m| m.id()).hash(hasher);
        self.material.map(|m| m.id()).hash(hasher);
        self.specular.map(|m| m.id()).hash(hasher);
        self.terrain.map(|m| m.id()).hash(hasher);
        self.grass.map(|m| m.id()).hash(hasher);
        self.impostor.map(|m| m.id()).hash(hasher);
    }
}

#[derive(SystemParam)]
pub(super) struct ShadowMaterials<'w> {
    source: Res<'w, Assets<SourceMaterial>>,
    specular: Res<'w, Assets<SpecularMaterial>>,
    terrain: Res<'w, Assets<TerrainMaterial>>,
    grass: Res<'w, Assets<GrassMaterial>>,
    impostors: Res<'w, Assets<ImpostorMaterial>>,
}
impl ShadowMaterials<'_> {
    /// Caster state of a GPU prop row material, as `has_caster` gives it for an entity.
    pub fn row_caster(&self, id: bevy::asset::UntypedAssetId) -> Result<bool, &'static str> {
        use std::any::TypeId;
        let available = match id.type_id() {
            t if t == TypeId::of::<SourceMaterial>() => {
                return self
                    .source
                    .get(id.typed::<SourceMaterial>())
                    .map(|material| material.extension.has_caster())
                    .ok_or("Missing source shadow material");
            }
            t if t == TypeId::of::<SpecularMaterial>() => (
                self.specular.contains(id.typed::<SpecularMaterial>()),
                "Missing source specular material",
            ),
            t if t == TypeId::of::<ImpostorMaterial>() => (
                self.impostors.contains(id.typed::<ImpostorMaterial>()),
                "Missing source impostor material",
            ),
            _ => return Err("Unsupported GPU prop material"),
        };
        match available {
            (true, _) => Ok(true),
            (false, error) => Err(error),
        }
    }
    pub fn has_caster(&self, mesh: &ShadowMeshItem<'_, '_>) -> Result<Option<bool>, &'static str> {
        if let Some(handle) = mesh.material {
            return self
                .source
                .get(handle.0.id())
                .map(|material| Some(material.extension.has_caster()))
                .ok_or("Missing source shadow material");
        }
        let available = if let Some(handle) = mesh.specular {
            Some((
                self.specular.contains(handle.0.id()),
                "Missing source specular material",
            ))
        } else if let Some(handle) = mesh.terrain {
            Some((
                self.terrain.contains(handle.0.id()),
                "Missing source terrain material",
            ))
        } else if let Some(handle) = mesh.grass {
            Some((
                self.grass.contains(handle.0.id()),
                "Missing source grass material",
            ))
        } else {
            mesh.impostor.map(|handle| {
                (
                    self.impostors.contains(handle.0.id()),
                    "Missing source impostor material",
                )
            })
        };
        match available {
            Some((true, _)) => Ok(Some(true)),
            Some((false, error)) => Err(error),
            None => Ok(None),
        }
    }
}

#[derive(SystemParam)]
pub(super) struct ShadowChanges<'w, 's> {
    pub changed: Query<
        'w,
        's,
        Entity,
        (
            With<SourceRenderer>,
            Or<(
                Changed<Aabb>,
                Changed<GlobalTransform>,
                Changed<InheritedVisibility>,
                Changed<SourceRenderer>,
                Changed<RenderLayers>,
                Changed<Mesh3d>,
                Added<ViewVisibility>,
                Changed<MeshMaterial3d<SourceMaterial>>,
                Changed<MeshMaterial3d<SpecularMaterial>>,
                Changed<MeshMaterial3d<TerrainMaterial>>,
                Changed<MeshMaterial3d<GrassMaterial>>,
                Changed<MeshMaterial3d<ImpostorMaterial>>,
            )>,
        ),
    >,
    pub warmup: Query<'w, 's, Entity, (With<SourceRenderer>, Added<SourceMaterialWarmup>)>,
    /// Renderers whose mesh or material asset was edited.
    pub edited: Query<
        'w,
        's,
        Entity,
        (
            With<SourceRenderer>,
            Or<(
                AssetChanged<Mesh3d>,
                AssetChanged<MeshMaterial3d<SourceMaterial>>,
                AssetChanged<MeshMaterial3d<SpecularMaterial>>,
                AssetChanged<MeshMaterial3d<TerrainMaterial>>,
                AssetChanged<MeshMaterial3d<GrassMaterial>>,
                AssetChanged<MeshMaterial3d<ImpostorMaterial>>,
            )>,
        ),
    >,
    /// GPU prop batch materials: source, specular and impostor.
    source_events: MessageReader<'w, 's, AssetEvent<SourceMaterial>>,
    specular_events: MessageReader<'w, 's, AssetEvent<SpecularMaterial>>,
    impostor_events: MessageReader<'w, 's, AssetEvent<ImpostorMaterial>>,
    bounds: RemovedComponents<'w, 's, Aabb>,
    transforms: RemovedComponents<'w, 's, GlobalTransform>,
    inherited: RemovedComponents<'w, 's, InheritedVisibility>,
    visible: RemovedComponents<'w, 's, ViewVisibility>,
    renderers: RemovedComponents<'w, 's, SourceRenderer>,
    layers: RemovedComponents<'w, 's, RenderLayers>,
    source: RemovedComponents<'w, 's, MeshMaterial3d<SourceMaterial>>,
    specular: RemovedComponents<'w, 's, MeshMaterial3d<SpecularMaterial>>,
    terrain: RemovedComponents<'w, 's, MeshMaterial3d<TerrainMaterial>>,
    grass: RemovedComponents<'w, 's, MeshMaterial3d<GrassMaterial>>,
    impostors: RemovedComponents<'w, 's, MeshMaterial3d<ImpostorMaterial>>,
    excluded: RemovedComponents<'w, 's, SourceMaterialWarmup>,
}
impl ShadowChanges<'_, '_> {
    /// A GPU prop batch material can change its caster state.
    pub fn batch_materials_changed(&mut self) -> bool {
        // Consume every stream.
        self.source_events.read().count()
            + self.specular_events.read().count()
            + self.impostor_events.read().count()
            != 0
    }
    pub fn removed(&mut self) -> bool {
        // Consume every stream, including later removals after the first match.
        self.bounds
            .read()
            .chain(self.transforms.read())
            .chain(self.inherited.read())
            .chain(self.visible.read())
            .chain(self.renderers.read())
            .chain(self.layers.read())
            .chain(self.source.read())
            .chain(self.specular.read())
            .chain(self.terrain.read())
            .chain(self.grass.read())
            .chain(self.impostors.read())
            .chain(self.excluded.read())
            .count()
            != 0
    }
}
