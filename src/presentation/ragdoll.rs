//! The player ragdoll is the ragdoll prototype (`fpscontroller.prefabRagdol`) with the outfit character (`CopyFrom`).
//! It shows while the frame has ragdoll rows. A dead remote player has its own instance. Each body node takes its body pose under the prefab root.
//! Character 1 nodes take the body pose of the character 0 joint with the same name.
//! Nodes without a body keep their local pose under their parent. The GPU skins the meshes.
use super::materials::SourceMaterial;
use super::outfit::Outfit;
use super::player_body::{
    BoundQuery, Identity, NodeQuery, OutfitQuery, character_nodes, character_renderer, dress,
    materials_bound, outfit_slot,
};
use super::reference::ReferenceVisuals;
use super::renderers::AuthoredRenderer;
use super::scene_nodes::{SceneGraph, bind_node};
use super::sim_frame::SimFrameResource;
use crate::app::{Drive, RuntimeReference};
use crate::reference::NodeId;
use bevy::ecs as bevy_ecs;
use bevy::prelude::*;
use bevy::world_serialization::{WorldAssetRoot, WorldInstanceReady};
use simulation::boundary::PoseRow;
use std::collections::HashMap;

/// The local player ragdoll instance.
#[derive(Resource, Default)]
pub struct RagdollBody {
    scene: Option<Entity>,
}

/// One ragdoll asset instance, on its scene entity.
#[derive(Component, Default)]
pub struct RagdollInstance {
    /// `OutfitSlot::figure`.
    figure: u8,
    /// Prefab root. It follows the root body.
    root: Option<Entity>,
    /// Body node entities of each character in `RagdollReference::bodies` order.
    bodies: [Vec<Entity>; 2],
    /// The first body without a parent body.
    root_body: usize,
    /// Renderer primitives and their character.
    primitives: Vec<(Entity, usize)>,
    ready: bool,
    /// The outfit is applied. It applies again after each change.
    dressed: bool,
}

/// Bind the ragdoll nodes. Remove the primitives of all renderers except those of the characters.
fn bind(
    event: On<WorldInstanceReady>,
    mut commands: Commands,
    reference: Res<RuntimeReference>,
    mut instances: Query<&mut RagdollInstance>,
    graph: SceneGraph,
) {
    let Ok(mut ragdoll) = instances.get_mut(event.entity) else {
        return;
    };
    let manifest = &reference.0;
    let record = &manifest.clothing.ragdoll;
    let characters = &record.characters;
    let male: Vec<NodeId> = record.bodies.iter().map(|b| b.node).collect();
    let Some(female) = character_nodes(manifest, characters, &male) else {
        error!("Ragdoll: character 1 lacks a joint name of a character 0 body");
        return;
    };
    let mut bodies = HashMap::<NodeId, Entity>::new();
    for entity in graph.children.iter_descendants(event.entity) {
        let Ok(extra) = graph.extras.get(entity) else {
            continue;
        };
        let Some(node) = serde_json::from_str::<Identity>(&extra.value)
            .ok()
            .and_then(|identity| identity.reference_id)
            .and_then(|source| {
                manifest
                    .nodes
                    .iter()
                    .find(|n| n.source == source && n.asset == record.asset)
            })
        else {
            continue;
        };
        if node.id == record.root {
            ragdoll.root = Some(entity);
        }
        if male.contains(&node.id) || female.contains(&node.id) {
            bodies.insert(node.id, entity);
        }
        let drawn = character_renderer(characters, node.id);
        let mut removed = false;
        for primitive in graph.primitives(entity) {
            let Some((character, renderer)) = drawn else {
                commands.entity(primitive).despawn();
                removed = true;
                continue;
            };
            commands
                .entity(primitive)
                .insert(AuthoredRenderer(renderer.state.clone()));
            let slot = graph
                .extras
                .get(primitive)
                .map_err(|e| e.to_string())
                .and_then(|e| {
                    outfit_slot(
                        ragdoll.figure,
                        manifest,
                        characters,
                        &record.invisible_material,
                        character,
                        node.id,
                        e,
                    )
                });
            match slot {
                Ok(Some(slot)) => {
                    commands.entity(primitive).insert(slot);
                }
                Ok(None) => {}
                Err(error) => error!("Ragdoll: {error}"),
            }
            ragdoll.primitives.push((primitive, character));
        }
        if removed {
            continue;
        }
        if let Err(error) = bind_node(&mut commands, node, entity, extra, &graph) {
            error!("Ragdoll: {error}");
        }
    }
    let ordered = |nodes: &[NodeId]| -> Option<Vec<Entity>> {
        nodes.iter().map(|n| bodies.get(n).copied()).collect()
    };
    let root_body = record.bodies.iter().position(|b| b.parent_body.is_none());
    let (Some(root), Some(male), Some(female), Some(root_body)) =
        (ragdoll.root, ordered(&male), ordered(&female), root_body)
    else {
        error!("Ragdoll glTF lacks its root or body nodes");
        return;
    };
    if ragdoll.primitives.is_empty() {
        error!("Ragdoll glTF lacks the character renderers");
        return;
    }
    // Body poses are in prefab root space, so each body node is a direct child of the root.
    for body in male.iter().chain(&female).filter(|b| **b != root) {
        commands.entity(*body).insert(ChildOf(root));
    }
    ragdoll.bodies = [male, female];
    ragdoll.root_body = root_body;
}

/// Spawns a hidden ragdoll instance. None until the ragdoll glTF loads.
pub(super) fn spawn(
    commands: &mut Commands,
    reference: &RuntimeReference,
    visuals: &ReferenceVisuals,
    gltfs: &Assets<bevy::gltf::Gltf>,
    figure: u8,
) -> Option<Entity> {
    let asset = reference.0.clothing.ragdoll.asset;
    let gltf = visuals
        .gltfs
        .iter()
        .find(|(id, _)| *id == asset)
        .and_then(|(_, handle)| gltfs.get(handle))?;
    let scene = commands
        .spawn((
            WorldAssetRoot(gltf.scenes[0].clone()),
            Transform::default(),
            Visibility::Hidden,
            RagdollInstance {
                figure,
                ..default()
            },
        ))
        .observe(bind)
        .id();
    Some(scene)
}

pub(crate) fn sync(
    mut commands: Commands,
    reference: Res<RuntimeReference>,
    drive: Res<Drive>,
    frame: Res<SimFrameResource>,
    visuals: Res<ReferenceVisuals>,
    gltfs: Res<Assets<bevy::gltf::Gltf>>,
    outfit: Res<Outfit>,
    materials: Res<Assets<SourceMaterial>>,
    slots: OutfitQuery,
    bound: BoundQuery,
    mut ragdoll: ResMut<RagdollBody>,
    mut instances: Query<&mut RagdollInstance>,
    mut nodes: NodeQuery,
) {
    if !visuals.bound {
        return;
    }
    let Some(scene) = ragdoll.scene else {
        ragdoll.scene = spawn(&mut commands, &reference, &visuals, &gltfs, 0);
        return;
    };
    let Ok(mut instance) = instances.get_mut(scene) else {
        return;
    };
    let style = Style {
        outfit: &outfit,
        redress: outfit.is_changed(),
        slots: &slots,
        materials: &materials,
        bound: &bound,
    };
    // The ragdoll moves with the camera interpolation, as the player body does.
    show(
        &mut commands,
        &reference,
        scene,
        &mut instance,
        &frame.frame.ragdoll,
        &style,
        &mut nodes,
        |row| super::transform(drive.rendered_body_pose(row.pose())),
    );
}

/// The outfit of one ragdoll instance and the queries that dress it.
pub(super) struct Style<'a, 'w, 's> {
    pub outfit: &'a Outfit,
    /// The outfit changed since the last dress.
    pub redress: bool,
    pub slots: &'a OutfitQuery<'w, 's>,
    pub materials: &'a Assets<SourceMaterial>,
    pub bound: &'a BoundQuery<'w, 's>,
}

/// Shows `scene` at `rows`, or hides it when `rows` does not hold every body.
#[allow(clippy::too_many_arguments)]
pub(super) fn show(
    commands: &mut Commands,
    reference: &RuntimeReference,
    scene: Entity,
    ragdoll: &mut RagdollInstance,
    rows: &[PoseRow],
    style: &Style,
    nodes: &mut NodeQuery,
    world: impl Fn(&PoseRow) -> Transform,
) {
    let record = &reference.0.clothing.ragdoll;
    let Some(root) = ragdoll.root.filter(|_| !ragdoll.bodies[0].is_empty()) else {
        return;
    };
    // Show the ragdoll only after its source materials replace the glTF materials.
    if !ragdoll.ready {
        if !materials_bound(
            &ragdoll.primitives,
            |p| nodes.get(p).is_ok_and(|(_, _, source)| source),
            style.bound,
        ) {
            return;
        }
        ragdoll.ready = true;
    }
    // `SpawnRagdol` enables every ragdoll renderer. `CopyFrom` keeps those of the outfit character.
    if !ragdoll.dressed || style.redress {
        dress(
            commands,
            style.outfit,
            &ragdoll.primitives,
            false,
            style.slots,
            style.materials,
        );
        ragdoll.dressed = true;
    }
    let active = rows.len() == record.bodies.len();
    if let Ok((_, mut visibility, _)) = nodes.get_mut(scene) {
        visibility.set_if_neq(if active {
            Visibility::Inherited
        } else {
            Visibility::Hidden
        });
    }
    let visible = |row: &PoseRow| row.flags & PoseRow::VISIBLE != 0;
    if !active || !visible(&rows[ragdoll.root_body]) {
        return;
    }
    // The root keeps the root body at its rest pose in root space.
    let rest = &record.bodies[ragdoll.root_body].pose;
    let body = world(&rows[ragdoll.root_body]);
    let Ok((mut transform, _, _)) = nodes.get_mut(root) else {
        return;
    };
    let scale = transform.scale;
    let rotation = body.rotation * Quat::from_array(rest.rotation).inverse();
    let translation = body.translation - rotation * (scale * Vec3::from_array(rest.translation));
    transform.set_if_neq(Transform {
        translation,
        rotation,
        scale,
    });
    let inverse = rotation.inverse();
    for ((entity, row), b) in ragdoll.bodies[style.outfit.character]
        .iter()
        .zip(rows)
        .zip(&record.bodies)
    {
        if *entity == root || !visible(row) {
            continue;
        }
        let pose = world(row);
        if let Ok((mut transform, _, _)) = nodes.get_mut(*entity) {
            transform.set_if_neq(Transform {
                translation: inverse * (pose.translation - translation) / scale,
                rotation: inverse * pose.rotation,
                scale: Vec3::from_array(b.pose.scale),
            });
        }
    }
}
