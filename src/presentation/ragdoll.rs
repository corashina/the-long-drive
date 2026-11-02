//! Player ragdoll: the ragdoll prototype (`fpscontroller.prefabRagdol`) with the outfit character (`CopyFrom`).
//! It shows while the frame has ragdoll rows. Each body node takes its body pose under the prefab root.
//! Character 1 nodes take the body pose of the character 0 joint with the same name.
//! Nodes without a body keep their local pose under their parent. The GPU skins the meshes.
use super::materials::SourceMaterial;
use super::outfit::Outfit;
use super::player_body::{
    BoundQuery, Identity, OutfitQuery, character_nodes, character_renderer, dress, materials_bound,
    outfit_slot,
};
use super::reference::{ReferenceVisuals, bind_node};
use super::renderers::AuthoredRenderer;
use super::sim_frame::SimFrameResource;
use crate::app::{Drive, RuntimeReference};
use crate::reference::NodeId;
use bevy::ecs as bevy_ecs;
use bevy::gltf::GltfExtras;
use bevy::prelude::*;
use bevy::world_serialization::{WorldAssetRoot, WorldInstanceReady};
use simulation::boundary::PoseRow;
use std::collections::HashMap;

#[derive(Resource, Default)]
pub struct RagdollBody {
    scene: Option<Entity>,
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
    mut ragdoll: ResMut<RagdollBody>,
    children: Query<&Children>,
    extras: Query<&GltfExtras>,
    primitives: Query<(Entity, &ChildOf), With<Mesh3d>>,
) {
    let manifest = &reference.0;
    let record = &manifest.clothing.ragdoll;
    let characters = &record.characters;
    let male: Vec<NodeId> = record.bodies.iter().map(|b| b.node).collect();
    let Some(female) = character_nodes(manifest, characters, &male) else {
        error!("Ragdoll: character 1 lacks a joint name of a character 0 body");
        return;
    };
    let mut bodies = HashMap::<NodeId, Entity>::new();
    for entity in children.iter_descendants(event.entity) {
        let Ok(extra) = extras.get(entity) else {
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
        for (primitive, parent) in &primitives {
            if parent.parent() != entity {
                continue;
            }
            let Some((character, renderer)) = drawn else {
                commands.entity(primitive).despawn();
                removed = true;
                continue;
            };
            commands
                .entity(primitive)
                .insert(AuthoredRenderer(renderer.state.clone()));
            let slot = extras
                .get(primitive)
                .map_err(|e| e.to_string())
                .and_then(|e| {
                    outfit_slot(
                        0,
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
        if let Err(error) = bind_node(&mut commands, node, entity, extra, &primitives) {
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
    mut nodes: Query<(
        &mut Transform,
        &mut Visibility,
        Has<MeshMaterial3d<SourceMaterial>>,
    )>,
) {
    if !visuals.bound {
        return;
    }
    let record = &reference.0.clothing.ragdoll;
    let Some(scene) = ragdoll.scene else {
        let Some(gltf) = visuals
            .gltfs
            .iter()
            .find(|(id, _)| *id == record.asset)
            .and_then(|(_, handle)| gltfs.get(handle))
        else {
            return;
        };
        let scene = commands
            .spawn((
                WorldAssetRoot(gltf.scenes[0].clone()),
                Transform::default(),
                Visibility::Hidden,
            ))
            .observe(bind)
            .id();
        ragdoll.scene = Some(scene);
        return;
    };
    let Some(root) = ragdoll.root.filter(|_| !ragdoll.bodies[0].is_empty()) else {
        return;
    };
    // Show the ragdoll only after its source materials replace the glTF materials.
    if !ragdoll.ready {
        if !materials_bound(
            &ragdoll.primitives,
            |p| nodes.get(p).is_ok_and(|(_, _, source)| source),
            &bound,
        ) {
            return;
        }
        ragdoll.ready = true;
    }
    // `SpawnRagdol` enables every ragdoll renderer. `CopyFrom` keeps those of the outfit character.
    if !ragdoll.dressed || outfit.is_changed() {
        dress(
            &mut commands,
            &outfit,
            &ragdoll.primitives,
            false,
            &slots,
            &materials,
        );
        ragdoll.dressed = true;
    }
    let rows = &frame.frame.ragdoll;
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
    // The ragdoll moves with the camera interpolation, as the player body does.
    let world = |row: &PoseRow| super::transform(drive.rendered_body_pose(row.pose()));
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
    for ((entity, row), b) in ragdoll.bodies[outfit.character]
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
