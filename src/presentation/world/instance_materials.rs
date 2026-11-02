//! Generated renderer materials retain source identity and per-instance LOD inputs.
use super::{
    gpu_props::linear_color,
    pipelines::{PipelineTicket, WorldPipelines},
    shared_materials::{MaterialIdentity, SharedMaterialKey},
};
use crate::reference::prototype_plan::PlanPrimitive;
use crate::{
    presentation::materials::{
        self, RenderingState, SourceMaterial, SourceMaterials, grass::material::GrassMaterial,
        impostor::ImpostorMaterial, specular::SpecularMaterial,
    },
    reference::{ReferenceError, ReferenceManifest, WorldPrototypeReference},
    world::RendererWorld,
};
use bevy::{ecs as bevy_ecs, ecs::system::SystemParam, prelude::*};
use grass_mesh::GrassView;
#[derive(Clone)]
pub enum InstanceMaterial {
    Primary(Handle<SourceMaterial>),
    Specular(Handle<SpecularMaterial>),
    Impostor(Handle<ImpostorMaterial>),
    Grass(Handle<GrassMaterial>),
}
#[derive(Clone)]
pub struct RendererMaterial {
    pub visible: InstanceMaterial,
    pub sort: Option<crate::presentation::renderer_visibility::SourceSortData>,
    shared: Option<SharedMaterialKey>,
}
#[derive(Clone)]
pub(crate) struct SharedBinding {
    visible: InstanceMaterial,
}
#[derive(SystemParam)]
pub struct InstanceMaterials<'w> {
    pub cache: ResMut<'w, SourceMaterials>,
    pub primary: ResMut<'w, Assets<SourceMaterial>>,
    pub specular: ResMut<'w, Assets<SpecularMaterial>>,
    pub impostors: ResMut<'w, Assets<ImpostorMaterial>>,
    pub grass: ResMut<'w, Assets<GrassMaterial>>,
}
impl InstanceMaterials<'_> {
    pub fn admit(
        &mut self,
        stream: &RendererWorld,
        ledger: &mut crate::ownership::PresentationLedger,
        reference: &ReferenceManifest,
        prototype: &WorldPrototypeReference,
        placement: &crate::world::Placement,
        plan: &crate::reference::prototype_plan::PrototypePlan,
    ) -> Result<bool, ReferenceError> {
        let mut ready = true;
        for (node_index, source_node) in prototype.nodes.iter().enumerate() {
            if !source_node.renderer_enabled {
                continue;
            }
            let node = &plan.nodes[source_node.gltf_node as usize];
            for primitive in plan.primitives.iter().filter(|p| Some(p.mesh) == node.mesh) {
                let (initial, selected) = biome_material(
                    stream,
                    reference,
                    prototype,
                    placement,
                    node_index,
                    primitive,
                    selected_material(stream, reference, prototype, placement, &source_node.source),
                );
                let selected = selected.cloned();
                ready &= self.admit_binding(
                    ledger,
                    reference,
                    prototype,
                    primitive,
                    initial,
                    selected.as_ref(),
                )?;
                let variants = stream
                    .lamp_materials(&prototype.source, &source_node.source)?
                    .len();
                for index in 0..variants {
                    let source = stream.lamp_materials(&prototype.source, &source_node.source)?
                        [index]
                        .clone();
                    let primary = source_primary(reference, prototype, &source);
                    ready &= self.admit_binding(
                        ledger,
                        reference,
                        prototype,
                        primitive,
                        primary,
                        Some(&source),
                    )?;
                }
            }
        }
        Ok(ready)
    }
    pub(crate) fn admit_binding(
        &mut self,
        ledger: &mut crate::ownership::PresentationLedger,
        reference: &ReferenceManifest,
        prototype: &WorldPrototypeReference,
        primitive: &PlanPrimitive,
        initial: Option<(crate::reference::AssetId, u32)>,
        selected: Option<&crate::reference::SourceId>,
    ) -> Result<bool, ReferenceError> {
        if initial.is_none() && (selected.is_some() || primitive.has_source_material) {
            let source = selected
                .or(primitive.source_material.as_ref())
                .ok_or_else(|| ReferenceError("Unresolved source null material draw".into()))?;
            let record = prototype
                .deferred_materials
                .iter()
                .find(|m| m.source == *source)
                .ok_or_else(|| ReferenceError("Missing generated material record".into()))?;
            self.cache.admit_world_material(record, reference, ledger)?;
            Ok(self.cache.world_textures.ready(source))
        } else {
            let identity = initial
                .or_else(|| {
                    primitive
                        .material
                        .as_ref()
                        .map(|m| (m.asset, m.gltf_material))
                })
                .ok_or_else(|| {
                    ReferenceError("Missing generated primary material identity".into())
                })?;
            self.cache
                .admit_world_primary(identity, reference, ledger)?;
            Ok(self.cache.world_primary_ready(identity))
        }
    }
    pub fn bind(
        &mut self,
        session: u64,
        prototype: &WorldPrototypeReference,
        primitive: &PlanPrimitive,
        renderer: &crate::reference::render_lighting::RendererRecord,
        initial: Option<(crate::reference::AssetId, u32)>,
        selected: Option<&crate::reference::SourceId>,
        layer: u8,
        receive: bool,
        reflected: bool,
        source_position: Vec3,
        view: GrassView,
        gpu: bool,
    ) -> Result<Option<RendererMaterial>, ReferenceError> {
        let mask = 1u32 << layer;
        let mut sort = None;
        let mut shared = None;
        let key = |material, gpu| SharedMaterialKey {
            session,
            material,
            layer,
            receive,
            reflected,
            gpu,
        };
        let visible = if initial.is_none() && (selected.is_some() || primitive.has_source_material)
        {
            let Some(id) = selected.or(primitive.source_material.as_ref()) else {
                return Err(ReferenceError(
                    "Unresolved source null material draw".into(),
                ));
            };
            let record = prototype
                .deferred_materials
                .iter()
                .find(|m| m.source == *id)
                .ok_or_else(|| ReferenceError("Missing generated material record".into()))?;
            // Admission checked readiness. A texture that is not ready here waits for a later frame.
            if !self.cache.world_textures.ready(id) {
                warn!(
                    "Source texture not ready at bind: {}:{}",
                    id.file, id.path_id
                );
                return Ok(None);
            }
            match record.shader.name.as_str() {
                "Stix Games/Grass" => InstanceMaterial::Grass(
                    self.grass
                        .add(GrassMaterial::from_source(session, record, view, mask)?),
                ),
                "Hidden/Amplify Impostors/Octahedron Impostor"
                | "Impostors/Custom/Colored Barrels" => {
                    // An entity impostor holds its fade and source position, so only rows share it.
                    if gpu {
                        let key = key(MaterialIdentity::Impostor(id.clone()), true);
                        if let Some(binding) = self.cache.generated.acquire(&key) {
                            return Ok(Some(RendererMaterial {
                                visible: binding.visible,
                                sort,
                                shared: Some(key),
                            }));
                        }
                        shared = Some(key);
                    }
                    let mut material = ImpostorMaterial::from_source(
                        record,
                        &self.cache.world_textures,
                        mask,
                        receive,
                        reflected,
                        if gpu { Vec3::ZERO } else { source_position },
                    )?;
                    if gpu {
                        material.use_gpu_props();
                    }
                    InstanceMaterial::Impostor(self.impostors.add(material))
                }
                "Standard (Specular setup)" => {
                    let key = key(MaterialIdentity::Specular(id.clone()), gpu);
                    if let Some(binding) = self.cache.generated.acquire(&key) {
                        return Ok(Some(RendererMaterial {
                            visible: binding.visible,
                            sort,
                            shared: Some(key),
                        }));
                    }
                    shared = Some(key);
                    let mut material = SpecularMaterial::from_source(
                        record,
                        &self.cache.world_textures,
                        mask,
                        reflected,
                    )?;
                    if gpu {
                        material.use_gpu_props();
                    }
                    InstanceMaterial::Specular(self.specular.add(material))
                }
                name => {
                    return Err(ReferenceError(format!(
                        "Unresolved generated shader {name}"
                    )));
                }
            }
        } else {
            let identity = initial
                .or_else(|| {
                    primitive
                        .material
                        .as_ref()
                        .map(|m| (m.asset, m.gltf_material))
                })
                .ok_or_else(|| {
                    ReferenceError("Missing generated primary material identity".into())
                })?;
            sort = self
                .cache
                .sort_data(identity, renderer, primitive.submesh)?;
            if sort.is_none() && self.cache.shareable_generated_primary(identity) {
                let key = key(MaterialIdentity::Primary(identity.0, identity.1), gpu);
                if let Some(binding) = self.cache.generated.acquire(&key) {
                    return Ok(Some(RendererMaterial {
                        visible: binding.visible,
                        sort,
                        shared: Some(key),
                    }));
                }
                shared = Some(key);
            }
            let mut material = materials::authored_material(identity, reflected, mask, &self.cache);
            if shared.as_ref().is_some_and(|key| key.gpu) {
                material.extension.use_gpu_props();
            }
            InstanceMaterial::Primary(self.primary.add(material))
        };
        match &visible {
            InstanceMaterial::Primary(h) => {
                self.primary.get_mut(h).unwrap().preparation = self.cache.preparation.clone()
            }
            InstanceMaterial::Specular(h) => {
                self.specular.get_mut(h).unwrap().preparation = self.cache.preparation.clone()
            }
            InstanceMaterial::Impostor(h) => {
                self.impostors.get_mut(h).unwrap().preparation = self.cache.preparation.clone()
            }
            InstanceMaterial::Grass(h) => {
                self.grass.get_mut(h).unwrap().preparation = self.cache.preparation.clone()
            }
        }
        if let Some(key) = &shared {
            self.cache.generated.insert(
                key.clone(),
                SharedBinding {
                    visible: visible.clone(),
                },
            );
        }
        Ok(Some(RendererMaterial {
            visible,
            sort,
            shared,
        }))
    }
    /// Isolate before pipeline tickets or entities retain these handles.
    pub fn make_private(&mut self, binding: &mut RendererMaterial) {
        let Some(key) = binding.shared.take() else {
            return;
        };
        assert!(!key.gpu, "GPU prop materials stay shared");
        let Self {
            cache,
            primary,
            specular,
            ..
        } = self;
        let private = cache.generated.make_private(&key, |shared| {
            let visible = match &shared.visible {
                InstanceMaterial::Primary(handle) => {
                    let material = primary.get(handle).unwrap().clone();
                    InstanceMaterial::Primary(primary.add(material))
                }
                InstanceMaterial::Specular(handle) => {
                    let material = specular.get(handle).unwrap().clone();
                    InstanceMaterial::Specular(specular.add(material))
                }
                _ => unreachable!("Only entity primary and specular bindings become private"),
            };
            SharedBinding { visible }
        });
        binding.visible = private.visible;
    }
    pub fn color(
        &mut self,
        binding: &mut RendererMaterial,
        color: [f32; 4],
    ) -> Result<(), ReferenceError> {
        self.make_private(binding);
        let linear = linear_color(color);
        let color = Color::srgba(color[0], color[1], color[2], color[3]);
        match &binding.visible {
            InstanceMaterial::Primary(h) => {
                if let Some(mut m) = self.primary.get_mut(h) {
                    m.base.base_color = color;
                    m.extension.surface.set_color(linear);
                }
            }
            InstanceMaterial::Specular(h) => {
                if let Some(mut m) = self.specular.get_mut(h) {
                    m.set_color(color, linear);
                }
            }
            _ => {
                return Err(ReferenceError(
                    "Condition color requires its checked shader property".into(),
                ));
            }
        }
        Ok(())
    }
    /// Source `SetColor("_EmissionColor")` and, with `map`, `SetTexture("_EmissionMap")`.
    /// `None` keeps the current map. Only a changed value touches the material.
    /// A shared binding becomes private: bind it private before an entity retains it.
    pub fn emission(
        &mut self,
        binding: &mut RendererMaterial,
        color: [f32; 4],
        map: Option<Handle<Image>>,
    ) -> Result<(), ReferenceError> {
        assert!(
            !binding.gpu_props(),
            "An emission override needs an entity renderer"
        );
        self.make_private(binding);
        let linear = linear_color(color);
        match &binding.visible {
            InstanceMaterial::Primary(h) => {
                materials::write_emission(&mut self.primary, h, linear, map).map_err(ReferenceError)
            }
            InstanceMaterial::Specular(_) if map.is_some() => Err(ReferenceError(
                "Specular texture arrays cannot take an _EmissionMap image".into(),
            )),
            InstanceMaterial::Specular(h) => {
                if self.specular.get(h).is_some_and(|m| m.emission() != linear)
                    && let Some(mut m) = self.specular.get_mut(h)
                {
                    m.set_emission(linear);
                }
                Ok(())
            }
            _ => Err(ReferenceError(
                "Emission requires its checked shader property".into(),
            )),
        }
    }
    /// `mirrorscript.FStart`: the mirror image is `_MainTex` and `_EmissionMap`.
    /// The first call makes the binding private and returns true: the entity must take it.
    /// Only a changed image touches the material.
    pub fn mirror_image(
        &mut self,
        binding: &mut RendererMaterial,
        image: &Handle<Image>,
    ) -> Result<bool, ReferenceError> {
        let private = binding.shared.is_some();
        self.make_private(binding);
        let InstanceMaterial::Primary(handle) = &binding.visible else {
            return Err(ReferenceError(
                "A live mirror needs a primary source material".into(),
            ));
        };
        let Some(material) = self.primary.get(handle) else {
            return Ok(private);
        };
        if material.slot_map(0) == Some(image) && material.emission_map() == Some(image) {
            return Ok(private);
        }
        let mut material = self
            .primary
            .get_mut(handle)
            .expect("checked source material");
        material
            .set_main_map(Some(image.clone()))
            .and_then(|()| material.set_emission_map(Some(image.clone())))
            .map_err(ReferenceError)?;
        Ok(private)
    }
    /// Source `SetTexture("_EmissionMap", null)`: the white default.
    pub fn clear_emission_map(
        &mut self,
        binding: &mut RendererMaterial,
    ) -> Result<(), ReferenceError> {
        assert!(
            !binding.gpu_props(),
            "An emission override needs an entity renderer"
        );
        self.make_private(binding);
        match &binding.visible {
            InstanceMaterial::Primary(h) => {
                if self
                    .primary
                    .get(h)
                    .is_some_and(|m| m.emission_map().is_some())
                    && let Some(mut m) = self.primary.get_mut(h)
                {
                    m.set_emission_map(None).map_err(ReferenceError)?;
                }
                Ok(())
            }
            // The checked specular variant has no emission map.
            InstanceMaterial::Specular(_) => Ok(()),
            _ => Err(ReferenceError(
                "Emission requires its checked shader property".into(),
            )),
        }
    }
    /// Linear row color of a GPU prop: the condition color, else the shared material color.
    pub fn row_color(
        &self,
        binding: &RendererMaterial,
        color: Option<[f32; 4]>,
    ) -> Result<Vec4, ReferenceError> {
        let material = match &binding.visible {
            InstanceMaterial::Primary(h) => {
                self.primary.get(h).map(|m| m.extension.surface.color())
            }
            InstanceMaterial::Specular(h) => self.specular.get(h).map(SpecularMaterial::color),
            // The impostor shaders do not read a color.
            InstanceMaterial::Impostor(_) if color.is_none() => Some(Vec4::ONE),
            _ => {
                return Err(ReferenceError(
                    "Condition color requires its checked shader property".into(),
                ));
            }
        };
        Ok(color.map_or_else(|| material.expect("bound GPU prop material"), linear_color))
    }
    /// Entity LOD fade. `sync` keeps the impostor source position.
    pub fn fade(&mut self, binding: &RendererMaterial, fade: f32) {
        if binding.shared.is_some() {
            assert_eq!(
                fade, 1.,
                "LOD bindings must be private before pipeline warmup"
            );
            return;
        }
        match &binding.visible {
            InstanceMaterial::Primary(h) => {
                if let Some(mut m) = self.primary.get_mut(h) {
                    m.extension.lod_fade = fade;
                }
            }
            InstanceMaterial::Specular(h) => {
                if let Some(mut m) = self.specular.get_mut(h) {
                    m.lod_fade = fade;
                }
            }
            InstanceMaterial::Impostor(h) => {
                if let Some(mut m) = self.impostors.get_mut(h) {
                    m.uniform.lod.x = fade;
                }
            }
            InstanceMaterial::Grass(h) => {
                if let Some(mut m) = self.grass.get_mut(h) {
                    m.uniform.lod.x = fade;
                }
            }
        }
    }
    pub fn release(&mut self, binding: RendererMaterial) {
        if let Some(key) = &binding.shared {
            if !self.cache.generated.release(key) {
                return;
            }
        }
        match binding.visible {
            InstanceMaterial::Primary(h) => {
                self.primary.remove(h.id());
            }
            InstanceMaterial::Specular(h) => {
                self.specular.remove(h.id());
            }
            InstanceMaterial::Impostor(h) => {
                self.impostors.remove(h.id());
            }
            InstanceMaterial::Grass(h) => {
                self.grass.remove(h.id());
            }
        }
    }
}
impl InstanceMaterial {
    pub fn replace(&self, entity: Entity, commands: &mut Commands) {
        commands.entity(entity).remove::<(
            MeshMaterial3d<SourceMaterial>,
            MeshMaterial3d<SpecularMaterial>,
            MeshMaterial3d<ImpostorMaterial>,
            MeshMaterial3d<GrassMaterial>,
        )>();
        self.insert(entity, commands);
    }

    pub fn insert(&self, entity: Entity, commands: &mut Commands) {
        match self {
            Self::Primary(h) => {
                commands.entity(entity).insert(MeshMaterial3d(h.clone()));
            }
            Self::Specular(h) => {
                commands.entity(entity).insert(MeshMaterial3d(h.clone()));
            }
            Self::Impostor(h) => {
                commands.entity(entity).insert(MeshMaterial3d(h.clone()));
            }
            Self::Grass(h) => {
                commands.entity(entity).insert(MeshMaterial3d(h.clone()));
            }
        }
    }
    /// Requests the pipelines of this material. A parent continuation batches them with its structure.
    pub(crate) fn warm(
        &self,
        stream: &RendererWorld,
        ledger: &mut crate::ownership::PresentationLedger,
        pipelines: &mut WorldPipelines,
        parent: Option<&mut crate::ownership::renderer_pending::PipelineContinuation>,
        mesh: &Handle<Mesh>,
        shadow: bool,
    ) -> Result<Option<PipelineTicket>, ReferenceError> {
        match self {
            Self::Primary(h) => pipelines.request(stream, ledger, parent, mesh, h, false),
            Self::Specular(h) => pipelines.request(stream, ledger, parent, mesh, h, false),
            Self::Impostor(h) => pipelines.request(stream, ledger, parent, mesh, h, shadow),
            Self::Grass(h) => pipelines.request(stream, ledger, parent, mesh, h, shadow),
        }
    }
}
/// The primary material of `source`. A material exported with another asset only is found there.
pub fn source_primary(
    reference: &ReferenceManifest,
    prototype: &WorldPrototypeReference,
    source: &crate::reference::SourceId,
) -> Option<(crate::reference::AssetId, u32)> {
    let materials = &reference.rendering.materials;
    materials
        .iter()
        .find(|m| m.source == *source && m.asset == prototype.asset)
        .or_else(|| materials.iter().find(|m| m.source == *source))
        .map(|m| (m.asset, m.gltf_material))
}
pub fn ready(ticket: &Option<PipelineTicket>) -> Result<bool, ReferenceError> {
    match ticket.as_ref().map(PipelineTicket::state) {
        Some(RenderingState::Ready) => Ok(true),
        Some(RenderingState::Failed(error)) => Err(ReferenceError(error)),
        _ => Ok(false),
    }
}

impl RendererMaterial {
    /// The renderer is a GPU prop row: its shared material reads the row buffers.
    pub fn gpu_props(&self) -> bool {
        self.shared.as_ref().is_some_and(|key| key.gpu)
    }
    pub fn apply_sort(&self, entity: Entity, commands: &mut Commands) {
        if let Some(sort) = self.sort {
            commands
                .entity(entity)
                .insert((sort, bevy::render::batching::NoAutomaticBatching));
        } else {
            commands.entity(entity).remove::<(
                crate::presentation::renderer_visibility::SourceSortData,
                bevy::render::batching::NoAutomaticBatching,
            )>();
        }
    }
}

/// Prototypes whose renderers take a runtime emission override. Their renderers are
/// private entities, never GPU prop rows.
/// ginlampscript.R and displayscript.R take a private material and a runtime emission.
pub fn emissive_renderer(
    reference: &ReferenceManifest,
    renderer: &crate::reference::SourceId,
) -> bool {
    *renderer == reference.small_items.genie_lamp.renderer
        || *renderer == reference.computer.display.renderer
}

pub fn selected_material<'a>(
    stream: &'a RendererWorld,
    reference: &'a ReferenceManifest,
    prototype: &WorldPrototypeReference,
    placement: &crate::world::Placement,
    node: &crate::reference::SourceId,
) -> (
    Option<(crate::reference::AssetId, u32)>,
    Option<&'a crate::reference::SourceId>,
) {
    let appearance = stream.appearance(&placement.id).iter().rev().find(|a| {
        a.node == *node
            && (a.primary.is_some()
                || a.material.is_some()
                || a.color.is_some()
                || a.color_material.is_some())
    });
    let selected = appearance.and_then(|a| a.material.as_ref()).or_else(|| stream.prop_material(placement).filter(|(renderer,_)| {
                                        let owner = reference.world.records.iter().find(|r| &r.source == *renderer)
                                            .and_then(|r| r.fields.get("m_GameObject"));
                                        let node_owner = reference.world.records.iter().find(|r| r.source == *node)
                                            .and_then(|r| r.fields.get("m_GameObject"));
                                        matches!((owner,node_owner), (Some(crate::reference::WorldValue::Link(a)),Some(crate::reference::WorldValue::Link(b))) if a == b)
                                    }).map(|(_,material)| material));
    let selected_primary = selected.and_then(|id| source_primary(reference, prototype, id));
    let initial = stream
        .appearance(&placement.id)
        .iter()
        .rev()
        .find(|a| a.node == *node && a.primary.is_some())
        .and_then(|a| a.primary)
        .or(selected_primary)
        .or_else(|| {
            reference
                .world
                .initial_materials
                .iter()
                .find(|m| m.node == *node)
                .and_then(|binding| {
                    reference
                        .rendering
                        .materials
                        .iter()
                        .find(|m| m.source == binding.material)
                })
                .map(|m| (m.asset, m.gltf_material))
        });
    (initial, selected)
}

/// Source `materialChangeScript` swaps the first material slot only: submesh 0.
/// It compares the material that painting and condition already selected.
pub fn biome_material<'a>(
    stream: &'a RendererWorld,
    reference: &ReferenceManifest,
    prototype: &WorldPrototypeReference,
    placement: &crate::world::Placement,
    node: usize,
    primitive: &PlanPrimitive,
    chosen: (
        Option<(crate::reference::AssetId, u32)>,
        Option<&'a crate::reference::SourceId>,
    ),
) -> (
    Option<(crate::reference::AssetId, u32)>,
    Option<&'a crate::reference::SourceId>,
) {
    let (initial, selected) = chosen;
    if primitive.submesh != 0 {
        return (initial, selected);
    }
    let Some(biome) = stream.renderer_biome(prototype, placement, node) else {
        return (initial, selected);
    };
    let current = if initial.is_none() && (selected.is_some() || primitive.has_source_material) {
        selected.or(primitive.source_material.as_ref())
    } else {
        initial
            .or_else(|| {
                primitive
                    .material
                    .as_ref()
                    .map(|m| (m.asset, m.gltf_material))
            })
            .and_then(|(asset, gltf)| {
                reference
                    .rendering
                    .materials
                    .iter()
                    .find(|m| m.asset == asset && m.gltf_material == gltf)
            })
            .map(|m| &m.source)
    };
    let Some(variant) = current.and_then(|current| stream.biome_material(current, biome)) else {
        return (initial, selected);
    };
    let primary = source_primary(reference, prototype, variant);
    (primary, Some(variant))
}
