//! Retained source condition choices. Each owner has separate live and candidate material banks.
use super::{
    materials::{self, SourceMaterial, SourceMaterials},
    reference::ReferenceVisuals,
    renderer_visibility::{SourceMaterialWarmup, SourceRenderer, SourceSortData},
    sim_frame::SimFrameResource,
};
use crate::{
    app::RuntimeReference,
    conditions::ConditionState,
    reference::{ConditionId, ConditionSurface, ReferenceManifest},
    save::SaveError,
};
use bevy::math::Affine3A;
use bevy::{
    camera::primitives::{Aabb, MeshAabb},
    ecs as bevy_ecs,
    prelude::*,
};
use std::collections::HashMap;

struct Choice {
    material: Handle<SourceMaterial>,
    sort: Option<SourceSortData>,
    color: Option<[f32; 4]>,
    dynamic_color: bool,
}
struct Receiver {
    entity: Entity,
    meshes: Vec<Handle<Mesh>>,
    bounds: Vec<(Aabb, SourceRenderer)>,
    banks: [Vec<Choice>; 2],
}
struct Owner {
    id: ConditionId,
    receivers: Vec<Receiver>,
    objects: Vec<Entity>,
}
#[derive(Resource, Default)]
pub struct ConditionVisuals {
    pub prepared: bool,
    bank: usize,
    owners: Vec<Owner>,
    indices: HashMap<ConditionId, usize>,
    washes: Vec<(crate::reference::WashId, Vec<Entity>)>,
    tools: Vec<(ConditionId, Option<Entity>, Option<Entity>)>,
    sprays: Vec<(crate::reference::ItemId, Vec<Receiver>)>,
}
struct OwnerUpdate {
    id: ConditionId,
    state: ConditionState,
}
pub struct PreparedConditions {
    bank: usize,
    owners: Vec<OwnerUpdate>,
    visibility: Vec<(Entity, Visibility)>,
    sprays: Vec<(crate::reference::ItemId, [f32; 4])>,
}
pub struct ConditionPlugin;
impl Plugin for ConditionPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<ConditionVisuals>().add_systems(
            Update,
            (prepare_conditions, sync_conditions)
                .chain()
                .after(materials::bind_authored_materials)
                .after(crate::app::sync_imported)
                .before(super::plates::bind_plate_text),
        );
    }
}
fn fail(text: impl Into<String>) -> SaveError {
    SaveError::MissingAssets(text.into())
}
fn color_rule(surface: &ConditionSurface, state: usize) -> bool {
    !surface.disable_color
        && ((!surface.dont_use_worn && (state < 3 || surface.force_color))
            || (surface.use_mat_list && surface.force_color))
}
fn prepare_conditions(world: &mut World) {
    if world.resource::<ConditionVisuals>().prepared || !world.resource::<SourceMaterials>().bound {
        return;
    }
    if matches!(
        world.resource::<SourceMaterials>().state,
        materials::RenderingState::Failed(_)
    ) {
        return;
    }
    let reference = world.resource::<RuntimeReference>().0.clone();
    match prepare(&reference, world) {
        Ok(visuals) => {
            world.insert_resource(visuals);
        }
        Err(error) => {
            world.resource_mut::<SourceMaterials>().state =
                materials::RenderingState::Failed(error.to_string())
        }
    }
}
fn prepare(
    reference: &ReferenceManifest,
    world: &mut World,
) -> Result<ConditionVisuals, SaveError> {
    let mut result = ConditionVisuals::default();
    let far = Transform::from_translation(Vec3::splat(1_000_000.));
    for surface in &reference.conditions.surfaces {
        let mut owner = Owner {
            id: surface.id,
            receivers: Vec::new(),
            objects: surface
                .objects
                .iter()
                .map(|id| world.resource::<ReferenceVisuals>().nodes[id])
                .collect(),
        };
        for renderer in &surface.renderers {
            let node = world.resource::<ReferenceVisuals>().nodes[&renderer.node];
            let entities: Vec<_> = world
                .query::<(Entity, &ChildOf)>()
                .iter(world)
                .filter(|(_, parent)| parent.parent() == node)
                .map(|(e, _)| e)
                .filter(|e| world.get::<MeshMaterial3d<SourceMaterial>>(*e).is_some())
                .collect();
            if entities.is_empty() {
                return Err(fail(format!(
                    "Condition {} renderer {} has no receivers",
                    surface.id.0, renderer.node.0
                )));
            }
            for entity in entities {
                let metadata = world
                    .get::<SourceRenderer>(entity)
                    .ok_or_else(|| fail("Condition renderer metadata"))?
                    .clone();
                let primitive = world
                    .get::<bevy::gltf::GltfExtras>(entity)
                    .ok_or_else(|| fail("Condition primitive metadata"))?
                    .clone();
                let slot = serde_json::from_str::<serde_json::Value>(&primitive.value)
                    .map_err(|e| fail(e.to_string()))?["submesh"]
                    .as_u64()
                    .ok_or_else(|| fail("Condition submesh"))? as usize;
                let base_mesh = world
                    .get::<Mesh3d>(entity)
                    .ok_or_else(|| fail("Condition mesh"))?
                    .0
                    .clone();
                let global = world
                    .get::<GlobalTransform>(entity)
                    .ok_or_else(|| fail("Condition transform"))?;
                let inverted = global.affine().matrix3.determinant() < 0.;
                let mask = world
                    .get::<bevy::camera::visibility::RenderLayers>(entity)
                    .map_or(1, |layers| {
                        layers.iter().fold(0u32, |bits, layer| {
                            bits | 1u32.checked_shl(layer as u32).unwrap_or(0)
                        })
                    });
                let mut receiver = Receiver {
                    entity,
                    meshes: vec![base_mesh],
                    bounds: vec![(
                        Aabb {
                            center: Vec3::from_array(metadata.0.bounds_center).into(),
                            half_extents: Vec3::from_array(metadata.0.bounds_extents).into(),
                        },
                        metadata.clone(),
                    )],
                    banks: std::array::from_fn(|_| Vec::with_capacity(5)),
                };
                if surface.use_mesh_list {
                    receiver.meshes.clear();
                    receiver.bounds.clear();
                    for mesh_ref in &renderer.meshes {
                        let visuals = world.resource::<ReferenceVisuals>();
                        let handle = &visuals
                            .gltfs
                            .iter()
                            .find(|(id, _)| *id == mesh_ref.asset)
                            .ok_or_else(|| fail("Condition mesh asset"))?
                            .1;
                        let gltf = world
                            .resource::<Assets<bevy::gltf::Gltf>>()
                            .get(handle)
                            .ok_or_else(|| fail("Condition glTF"))?;
                        let mesh = world
                            .resource::<Assets<bevy::gltf::GltfMesh>>()
                            .get(&gltf.meshes[mesh_ref.gltf_mesh as usize])
                            .ok_or_else(|| fail("Condition mesh variant"))?;
                        let raw = mesh
                            .primitives
                            .get(slot)
                            .ok_or_else(|| fail("Condition mesh submesh"))?
                            .mesh
                            .clone();
                        let mut mesh = world
                            .resource::<Assets<Mesh>>()
                            .get(&raw)
                            .ok_or_else(|| fail("Condition mesh pixels"))?
                            .clone();
                        let vertices = mesh.count_vertices();
                        let bounds = mesh
                            .compute_aabb()
                            .ok_or_else(|| fail("Condition mesh bounds"))?;
                        let mut renderer = metadata.clone();
                        renderer.0.bounds_center = bounds.center.to_array();
                        renderer.0.bounds_extents = bounds.half_extents.to_array();
                        let [center, extents] = renderer.0.attributes();
                        receiver.bounds.push((bounds, renderer));
                        mesh.insert_attribute(
                            super::renderers::SOURCE_BOUNDS_CENTER,
                            vec![center; vertices],
                        );
                        mesh.insert_attribute(
                            super::renderers::SOURCE_BOUNDS_EXTENTS,
                            vec![extents; vertices],
                        );
                        receiver
                            .meshes
                            .push(world.resource_mut::<Assets<Mesh>>().add(mesh));
                    }
                }
                for bank in &mut receiver.banks {
                    for (state_index, state) in renderer.states.iter().enumerate() {
                        let binding = state
                            .materials
                            .get(slot)
                            .or_else(|| state.materials.first())
                            .ok_or_else(|| fail("Condition material slot"))?;
                        let key = (binding.asset, binding.gltf_material);
                        let source = world.resource::<SourceMaterials>();
                        let mut material =
                            materials::authored_material(key, inverted, mask, source);
                        let dynamic_color = color_rule(surface, state_index);
                        if let Some(color) = if dynamic_color {
                            Some(surface.startup.color)
                        } else {
                            state.color
                        } {
                            materials::set_source_color(&mut material, color);
                        }
                        let sort = source
                            .sort_data(key, &metadata.0, slot as u32)
                            .map_err(|e| fail(e.to_string()))?;
                        materials::retain_color(&mut material, world);
                        let material = world.resource_mut::<Assets<SourceMaterial>>().add(material);
                        for mesh in &receiver.meshes {
                            materials::warm_retained_material(
                                world,
                                mesh.id(),
                                material.id().untyped(),
                                format!("condition {}", surface.id.0),
                            );
                            let mut warm = world.spawn((
                                Mesh3d(mesh.clone()),
                                MeshMaterial3d(material.clone()),
                                SourceMaterialWarmup,
                                far,
                                Visibility::Visible,
                                bevy::light::NotShadowCaster,
                                bevy::camera::visibility::NoFrustumCulling,
                            ));
                            if let Some(sort) = sort {
                                warm.insert((sort, bevy::render::batching::NoAutomaticBatching));
                            }
                            let id = warm.id();
                            world
                                .resource_mut::<super::lighting::SourceLighting>()
                                .warmups
                                .push(id);
                        }
                        bank.push(Choice {
                            material,
                            sort,
                            color: state.color,
                            dynamic_color,
                        });
                    }
                }
                owner.receivers.push(receiver);
            }
        }
        result.indices.insert(surface.id, result.owners.len());
        result.owners.push(owner);
    }
    for wash in &reference.conditions.washes {
        let mut nodes = Vec::with_capacity(wash.objects.len() + 1);
        nodes.push(world.resource::<ReferenceVisuals>().nodes[&wash.node]);
        for node in &wash.objects {
            let entity = world.resource::<ReferenceVisuals>().nodes[node];
            if !nodes.contains(&entity) {
                nodes.push(entity);
            }
        }
        result.washes.push((wash.id, nodes));
    }
    for tool in &reference.conditions.brushes {
        result.tools.push((
            tool.own_condition,
            tool.nodes
                .model
                .map(|n| world.resource::<ReferenceVisuals>().nodes[&n]),
            tool.nodes
                .scrub_node
                .map(|n| world.resource::<ReferenceVisuals>().nodes[&n]),
        ));
    }
    for tool in &reference.conditions.sprays {
        let node = world.resource::<ReferenceVisuals>().nodes[&tool.renderer_node];
        let entities: Vec<_> = world
            .query::<(Entity, &ChildOf)>()
            .iter(world)
            .filter(|(_, parent)| parent.parent() == node)
            .map(|(e, _)| e)
            .filter(|e| world.get::<MeshMaterial3d<SourceMaterial>>(*e).is_some())
            .collect();
        if entities.is_empty() {
            return Err(fail("Spray color renderer"));
        }
        let mut receivers = Vec::new();
        for entity in entities {
            let mesh = world.get::<Mesh3d>(entity).unwrap().0.clone();
            let handle = &world
                .get::<MeshMaterial3d<SourceMaterial>>(entity)
                .unwrap()
                .0;
            let original = world
                .resource::<Assets<SourceMaterial>>()
                .get(handle)
                .unwrap()
                .clone();
            let sort = world.get::<SourceSortData>(entity).copied();
            let mut receiver = Receiver {
                entity,
                meshes: vec![mesh.clone()],
                bounds: Vec::new(),
                banks: std::array::from_fn(|_| Vec::with_capacity(1)),
            };
            for bank in &mut receiver.banks {
                let mut material = original.clone();
                materials::set_source_color(&mut material, tool.tool.startup_color);
                materials::retain_color(&mut material, world);
                let material = world.resource_mut::<Assets<SourceMaterial>>().add(material);
                materials::warm_retained_material(
                    world,
                    mesh.id(),
                    material.id().untyped(),
                    "source spray label".into(),
                );
                let mut warm = world.spawn((
                    Mesh3d(mesh.clone()),
                    MeshMaterial3d(material.clone()),
                    SourceMaterialWarmup,
                    far,
                    Visibility::Visible,
                    bevy::light::NotShadowCaster,
                    bevy::camera::visibility::NoFrustumCulling,
                ));
                if let Some(sort) = sort {
                    warm.insert((sort, bevy::render::batching::NoAutomaticBatching));
                }
                let id = warm.id();
                world
                    .resource_mut::<super::lighting::SourceLighting>()
                    .warmups
                    .push(id);
                bank.push(Choice {
                    material,
                    sort,
                    color: None,
                    dynamic_color: true,
                });
            }
            receivers.push(receiver);
        }
        result.sprays.push((tool.item, receivers));
    }
    result.prepared = true;
    world
        .resource_mut::<SimFrameResource>()
        .changed_conditions
        .clear();
    for owner in &result.owners {
        let state = *world.resource::<SimFrameResource>().condition(owner.id);
        apply_owner(owner, &state, result.bank, world);
    }
    for (index, (_, receivers)) in result.sprays.iter().enumerate() {
        let color = world.resource::<SimFrameResource>().frame.spray_colors[index];
        apply_spray(receivers, color, result.bank, world, true);
    }
    result.apply_visibility(world);
    Ok(result)
}
fn update_colors(owner: &Owner, state: &ConditionState, bank: usize, world: &mut World) {
    let index = usize::from(state.state.min(4));
    for receiver in &owner.receivers {
        let choice = &receiver.banks[bank][index];
        if let Some(color) = if choice.dynamic_color {
            Some(state.color)
        } else {
            choice.color
        } {
            materials::update_retained_color(
                world
                    .resource::<Assets<SourceMaterial>>()
                    .get(&choice.material)
                    .expect("validated condition material"),
                color,
            );
        }
    }
}
fn bind_owner(owner: &Owner, state: &ConditionState, bank: usize, world: &mut World) {
    for receiver in &owner.receivers {
        let choice = &receiver.banks[bank][usize::from(state.state.min(4))];
        let mesh_index = if receiver.meshes.len() == 1 {
            0
        } else {
            state.state2 as usize
        };
        let mesh = &receiver.meshes[mesh_index];
        let (bounds, renderer) = &receiver.bounds[mesh_index];
        let mut target = world.entity_mut(receiver.entity);
        target.insert((
            Mesh3d(mesh.clone()),
            MeshMaterial3d(choice.material.clone()),
            *bounds,
        ));
        {
            let mut metadata = target
                .get_mut::<SourceRenderer>()
                .expect("condition renderer metadata");
            metadata.0.bounds_center = renderer.0.bounds_center;
            metadata.0.bounds_extents = renderer.0.bounds_extents;
        }
        if let Some(sort) = choice.sort {
            target.insert((sort, bevy::render::batching::NoAutomaticBatching));
        } else {
            target.remove::<SourceSortData>();
        }
    }
    for (index, &entity) in owner.objects.iter().enumerate() {
        world
            .entity_mut(entity)
            .insert(if index == state.state3 as usize {
                Visibility::Inherited
            } else {
                Visibility::Hidden
            });
    }
}
fn apply_owner(owner: &Owner, state: &ConditionState, bank: usize, world: &mut World) {
    update_colors(owner, state, bank, world);
    bind_owner(owner, state, bank, world);
}
impl ConditionVisuals {
    pub fn validate_assets(&self, world: &World) -> Result<(), SaveError> {
        if !self.prepared {
            return Err(fail("Condition bindings are not ready"));
        }
        for receiver in self
            .owners
            .iter()
            .flat_map(|o| &o.receivers)
            .chain(self.sprays.iter().flat_map(|(_, r)| r))
        {
            for mesh in &receiver.meshes {
                if !world.resource::<Assets<Mesh>>().contains(mesh.id()) {
                    return Err(fail("Condition mesh variant"));
                }
            }
            for bank in &receiver.banks {
                for choice in bank {
                    if !world
                        .resource::<Assets<SourceMaterial>>()
                        .contains(choice.material.id())
                    {
                        return Err(fail("Condition material variant"));
                    }
                }
            }
        }
        Ok(())
    }
    fn apply_visibility(&self, world: &mut World) {
        // Retained identities are hidden; source destruction never removes a binding.
        for (index, (_, objects)) in self.washes.iter().enumerate() {
            let visible = world.resource::<SimFrameResource>().frame.washes[index] != 0;
            for &entity in objects {
                if let Some(mut v) = world.get_mut::<Visibility>(entity) {
                    v.set_if_neq(if visible {
                        Visibility::Inherited
                    } else {
                        Visibility::Hidden
                    });
                }
            }
        }
        for (id, model, disabled) in &self.tools {
            let worn = world.resource::<SimFrameResource>().condition(*id).state == 5;
            for (entity, show) in [(model, true), (disabled, !worn)] {
                if let Some(entity) = entity {
                    if let Some(mut v) = world.get_mut::<Visibility>(*entity) {
                        v.set_if_neq(if show {
                            Visibility::Inherited
                        } else {
                            Visibility::Hidden
                        });
                    }
                }
            }
        }
    }
}
impl PreparedConditions {
    pub fn commit(self, world: &mut World) {
        world.resource_scope(|world, mut visuals: Mut<ConditionVisuals>| {
            visuals.bank = self.bank;
            for (id, color) in &self.sprays {
                let receivers = &visuals
                    .sprays
                    .iter()
                    .find(|(item, _)| item == id)
                    .unwrap()
                    .1;
                apply_spray(receivers, *color, self.bank, world, true);
            }
            for update in &self.owners {
                bind_owner(
                    &visuals.owners[visuals.indices[&update.id]],
                    &update.state,
                    self.bank,
                    world,
                );
            }
        });
        for (entity, visibility) in self.visibility {
            world.entity_mut(entity).insert(visibility);
        }
    }
}
pub(crate) fn sync_conditions(world: &mut World) {
    if !world.resource::<ConditionVisuals>().prepared {
        return;
    }
    world.resource_scope(|world, visuals: Mut<ConditionVisuals>| {
        loop {
            let id = world
                .resource_mut::<SimFrameResource>()
                .changed_conditions
                .pop();
            let Some(id) = id else {
                break;
            };
            let state = *world.resource::<SimFrameResource>().condition(id);
            apply_owner(
                &visuals.owners[visuals.indices[&id]],
                &state,
                visuals.bank,
                world,
            );
        }
        for (index, (_, receivers)) in visuals.sprays.iter().enumerate() {
            let color = world.resource::<SimFrameResource>().frame.spray_colors[index];
            apply_spray(receivers, color, visuals.bank, world, false);
        }
        visuals.apply_visibility(world);
        for (index, (_, model, _)) in visuals.tools.iter().enumerate() {
            let Some(model) = model else { continue };
            let pose = world.resource::<SimFrameResource>().frame.brush_poses[index].pose();
            let mut parent = world.get::<ChildOf>(*model).map(|p| p.parent());
            let mut basis = Affine3A::IDENTITY;
            while let Some(entity) = parent {
                if let Some(transform) = world.get::<Transform>(entity) {
                    basis = transform.compute_affine() * basis;
                }
                parent = world.get::<ChildOf>(entity).map(|p| p.parent());
            }
            let desired = Transform::from_translation(Vec3::from_array(pose.position))
                .with_rotation(Quat::from_array(pose.rotation));
            let local =
                Transform::from_matrix(Mat4::from(basis.inverse() * desired.compute_affine()));
            if let Some(mut transform) = world.get_mut::<Transform>(*model) {
                let next = Transform {
                    translation: local.translation,
                    rotation: local.rotation,
                    ..*transform
                };
                transform.set_if_neq(next);
            }
        }
    });
}

fn apply_spray(
    receivers: &[Receiver],
    color: [f32; 4],
    bank: usize,
    world: &mut World,
    bind: bool,
) {
    for receiver in receivers {
        let choice = &receiver.banks[bank][0];
        materials::update_retained_color(
            world
                .resource::<Assets<SourceMaterial>>()
                .get(&choice.material)
                .expect("retained spray material"),
            color,
        );
        if bind {
            world
                .entity_mut(receiver.entity)
                .insert(MeshMaterial3d(choice.material.clone()));
        }
    }
}
