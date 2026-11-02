//! First-person player body: the two characters of the player prototype. The outfit chooses one.
//! Outfit model mode (`enabledCharacter`): 0 no body, 1 shadow only, 2 visible.
//! The skin joints are children of `CharacterRoot` and take the `BodyState` pose. The GPU skins the meshes.
//! Character 1 joints take the pose of the character 0 joint with the same name.
//! The body hides while the ragdoll shows.
use super::materials::{OutfitMaterials, OutfitSlot, SourceMaterial, update_retained_color};
use super::outfit::{MODEL_OFF, MODEL_SHADOW, Outfit, map_by_name};
use super::reference::{ReferenceVisuals, bind_node};
use super::renderer_visibility::CameraHidden;
use super::renderers::AuthoredRenderer;
use super::sim_frame::SimFrameResource;
use crate::app::{Drive, RuntimeReference};
use crate::reference::{CharacterReference, NodeId, ReferenceManifest, SourceId};
use bevy::ecs as bevy_ecs;
use bevy::gltf::GltfExtras;
use bevy::prelude::*;
use bevy::world_serialization::{WorldAssetRoot, WorldInstanceReady};
use std::collections::{HashMap, HashSet};

#[derive(Resource, Default)]
pub struct PlayerBody {
    scene: Option<Entity>,
    bound: BoundBody,
    shown: bool,
    /// The outfit is applied. It applies again after each change.
    dressed: bool,
}
impl PlayerBody {
    /// Entity of the source Player Transform, once the body is bound.
    pub fn player(&self) -> Option<Entity> {
        self.bound.player
    }
    /// Entity of a skin joint, once the body is bound.
    pub fn joint(&self, node: NodeId) -> Option<Entity> {
        self.bound.joints.get(&node).copied()
    }
}

/// The bound nodes of one player asset instance: the local body or a remote figure.
#[derive(Default)]
pub(super) struct BoundBody {
    /// Source Player Transform. It follows the player pose.
    pub player: Option<Entity>,
    /// Source `CharacterRoot`. It follows `BodyState::root_local`.
    pub character_root: Option<Entity>,
    /// Skin joints of both characters.
    pub joints: HashMap<NodeId, Entity>,
    /// Skin joint entities of each character in `BodyState` joint order.
    pub posed: [Vec<Entity>; 2],
    /// Renderer primitives and their character.
    pub primitives: Vec<(Entity, usize)>,
}

pub(super) type NodeQuery<'w, 's> = Query<
    'w,
    's,
    (
        &'static mut Transform,
        &'static mut Visibility,
        Has<MeshMaterial3d<SourceMaterial>>,
    ),
>;

#[derive(serde::Deserialize)]
pub(super) struct Identity {
    pub reference_id: Option<SourceId>,
}

#[derive(serde::Deserialize)]
struct Submesh {
    submesh: u32,
}

pub(super) type OutfitQuery<'w, 's> =
    Query<'w, 's, (&'static OutfitSlot, &'static OutfitMaterials)>;
pub(super) type BoundQuery<'w, 's> =
    Query<'w, 's, Option<&'static OutfitMaterials>, With<OutfitSlot>>;

/// Character renderer of `node`, with the character index.
pub(super) fn character_renderer(
    characters: &[CharacterReference],
    node: NodeId,
) -> Option<(usize, &crate::reference::CharacterRendererReference)> {
    characters.iter().enumerate().find_map(|(i, c)| {
        c.renderers
            .iter()
            .find(|r| r.node == node)
            .map(|renderer| (i, renderer))
    })
}

/// Outfit slot of a renderer primitive: the segment that owns the material slot `submesh` of `node`.
pub(super) fn outfit_slot(
    figure: u8,
    manifest: &ReferenceManifest,
    characters: &[CharacterReference],
    invisible: &SourceId,
    character: usize,
    node: NodeId,
    primitive: &GltfExtras,
) -> Result<Option<OutfitSlot>, String> {
    let c = &characters[character];
    let slot = serde_json::from_str::<Submesh>(&primitive.value)
        .map_err(|error| format!("primitive submesh of node {}: {error}", node.0))?
        .submesh;
    let Some((index, segment)) = c
        .segments
        .iter()
        .enumerate()
        .find(|(_, s)| s.renderers.iter().any(|r| r.node == node && r.slot == slot))
    else {
        return Ok(None);
    };
    let material = |source: &SourceId| {
        manifest
            .rendering
            .materials
            .iter()
            .find(|m| m.asset == c.asset && m.source == *source)
            .map(|m| m.gltf_material)
            .ok_or_else(|| format!("outfit material {}:{}", source.file, source.path_id))
    };
    Ok(Some(OutfitSlot {
        figure,
        character: character as u8,
        segment: index as u8,
        material: material(&segment.default_material)?,
        invisible: if segment.disable_able {
            Some(material(invisible)?)
        } else {
            None
        },
    }))
}

/// Skin joints of a character with their node names.
pub(super) fn named_joints<'a>(
    manifest: &'a ReferenceManifest,
    character: &CharacterReference,
) -> Vec<(NodeId, &'a str)> {
    let joints: HashSet<NodeId> = character
        .skins
        .iter()
        .flat_map(|s| s.joints.iter().copied())
        .collect();
    manifest
        .nodes
        .iter()
        .filter(|n| joints.contains(&n.id))
        .map(|n| (n.id, n.name.as_str()))
        .collect()
}

/// Map character 0 `nodes` to character 1 by joint name.
pub(super) fn character_nodes(
    manifest: &ReferenceManifest,
    characters: &[CharacterReference],
    nodes: &[NodeId],
) -> Option<Vec<NodeId>> {
    map_by_name(
        nodes,
        &named_joints(manifest, &characters[0]),
        &named_joints(manifest, &characters[1]),
    )
}

/// `playermodeloutfitscript.Refresh`: show the chosen character and set the segment colors and materials.
/// `camera_hidden` hides the chosen character from the camera but keeps its shadow.
/// It runs once per outfit change, not per frame.
pub(super) fn dress(
    commands: &mut Commands,
    outfit: &Outfit,
    primitives: &[(Entity, usize)],
    camera_hidden: bool,
    slots: &OutfitQuery,
    materials: &Assets<SourceMaterial>,
) {
    for &(entity, character) in primitives {
        let chosen = character == outfit.character;
        let mut primitive = commands.entity(entity);
        primitive.insert(if chosen {
            Visibility::Inherited
        } else {
            Visibility::Hidden
        });
        if chosen && camera_hidden {
            primitive.insert(CameraHidden);
        } else {
            primitive.remove::<CameraHidden>();
        }
        let Ok((slot, owned)) = slots.get(entity) else {
            continue;
        };
        let segment =
            &outfit.characters[usize::from(slot.character)].segments[usize::from(slot.segment)];
        let [r, g, b] = segment.color();
        if let Some(material) = materials.get(&owned.segment) {
            update_retained_color(material, [r, g, b, 1.]);
        }
        let handle = match &owned.invisible {
            Some(invisible) if !segment.enabled => invisible,
            _ => &owned.segment,
        };
        primitive.insert(MeshMaterial3d(handle.clone()));
    }
}

/// True when every primitive has its source material and, for an outfit slot, its outfit materials.
pub(super) fn materials_bound(
    primitives: &[(Entity, usize)],
    sourced: impl Fn(Entity) -> bool,
    outfit: &BoundQuery,
) -> bool {
    primitives
        .iter()
        .all(|(p, _)| sourced(*p) && !matches!(outfit.get(*p), Ok(None)))
}

/// Bind the player nodes of the instance under `root`. Remove the primitives of all renderers except those of the characters.
/// `rig` is the `BodyState` joint order. `figure` is the `OutfitSlot::figure` of the instance.
pub(super) fn bind_instance(
    figure: u8,
    root: Entity,
    commands: &mut Commands,
    manifest: &ReferenceManifest,
    rig: &[NodeId],
    children: &Query<&Children>,
    extras: &Query<&GltfExtras>,
    primitives: &Query<(Entity, &ChildOf), With<Mesh3d>>,
) -> Option<BoundBody> {
    let clothing = &manifest.clothing;
    let characters = &clothing.characters;
    let mut body = BoundBody::default();
    for entity in children.iter_descendants(root) {
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
                    .find(|n| n.source == source && n.asset == characters[0].asset)
            })
        else {
            continue;
        };
        if node.id == clothing.pose.root {
            body.player = Some(entity);
        }
        if node.id == clothing.pose.anchors["CharacterRoot"].node {
            body.character_root = Some(entity);
        }
        if characters
            .iter()
            .any(|c| c.skins.iter().any(|s| s.joints.contains(&node.id)))
        {
            body.joints.insert(node.id, entity);
        }
        let drawn = character_renderer(characters, node.id);
        let mut removed = false;
        for (primitive, parent) in primitives {
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
                        figure,
                        manifest,
                        characters,
                        &clothing.invisible_material,
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
                Err(error) => error!("Player body: {error}"),
            }
            body.primitives.push((primitive, character));
        }
        if removed {
            continue;
        }
        if let Err(error) = bind_node(commands, node, entity, extra, primitives) {
            error!("Player body: {error}");
        }
    }
    let joints = characters
        .iter()
        .flat_map(|c| &c.skins)
        .flat_map(|s| &s.joints)
        .all(|id| body.joints.contains_key(id));
    let complete = body.player.is_some() && joints && !body.primitives.is_empty();
    let Some(root) = body.character_root.filter(|_| complete) else {
        error!("Player body glTF lacks its root, CharacterRoot, skin joints, or renderers");
        return None;
    };
    let Some(female) = character_nodes(manifest, characters, rig) else {
        error!("Player body: character 1 lacks a joint name of character 0");
        return None;
    };
    // BodyState poses every joint relative to CharacterRoot, so each joint is its direct child.
    let male: Vec<Entity> = rig.iter().map(|node| body.joints[node]).collect();
    let female: Vec<Entity> = female.iter().map(|node| body.joints[node]).collect();
    for joint in male.iter().chain(&female) {
        commands.entity(*joint).insert(ChildOf(root));
    }
    body.posed = [male, female];
    Some(body)
}

/// Bind the local player body.
fn bind(
    event: On<WorldInstanceReady>,
    mut commands: Commands,
    reference: Res<RuntimeReference>,
    frame: Res<SimFrameResource>,
    mut body: ResMut<PlayerBody>,
    children: Query<&Children>,
    extras: Query<&GltfExtras>,
    primitives: Query<(Entity, &ChildOf), With<Mesh3d>>,
) {
    if let Some(bound) = bind_instance(
        0,
        event.entity,
        &mut commands,
        &reference.0,
        frame.rig(),
        &children,
        &extras,
        &primitives,
    ) {
        body.bound = bound;
    }
}

/// Spawns the player asset scene, hidden until it binds.
pub(super) fn spawn_instance(
    commands: &mut Commands,
    manifest: &ReferenceManifest,
    visuals: &ReferenceVisuals,
    gltfs: &Assets<bevy::gltf::Gltf>,
) -> Option<Entity> {
    let asset = manifest.clothing.characters[0].asset;
    let gltf = visuals
        .gltfs
        .iter()
        .find(|(id, _)| *id == asset)
        .and_then(|(_, handle)| gltfs.get(handle))?;
    Some(
        commands
            .spawn((
                WorldAssetRoot(gltf.scenes[0].clone()),
                Transform::default(),
                Visibility::Hidden,
            ))
            .id(),
    )
}

/// Moves the Player Transform, `CharacterRoot` and the joints of the chosen character.
pub(super) fn pose_instance(
    body: &BoundBody,
    nodes: &mut NodeQuery,
    player: crate::vehicle::VehiclePose,
    root: crate::vehicle::VehiclePose,
    joint_scale: f32,
    joints: &[simulation::boundary::PoseRow],
    character: usize,
) {
    if let Some(entity) = body.player
        && let Ok((mut transform, _, _)) = nodes.get_mut(entity)
    {
        let scale = transform.scale;
        transform.set_if_neq(super::transform(player).with_scale(scale));
    }
    if let Some(entity) = body.character_root
        && let Ok((mut transform, _, _)) = nodes.get_mut(entity)
    {
        let scale = transform.scale;
        transform.set_if_neq(super::transform(root).with_scale(scale));
    }
    let scale = Vec3::splat(joint_scale);
    for (joint, row) in body.posed[character].iter().zip(joints) {
        if let Ok((mut transform, _, _)) = nodes.get_mut(*joint) {
            transform.set_if_neq(super::transform(row.pose()).with_scale(scale));
        }
    }
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
    mut body: ResMut<PlayerBody>,
    mut nodes: NodeQuery,
) {
    if !visuals.bound {
        return;
    }
    let Some(scene) = body.scene else {
        if let Some(scene) = spawn_instance(&mut commands, &reference.0, &visuals, &gltfs) {
            commands.entity(scene).observe(bind);
            body.scene = Some(scene);
        }
        return;
    };
    if body.bound.player.is_none() {
        return;
    }
    // Show the body after its source materials replace the glTF materials.
    if !body.shown {
        if !materials_bound(
            &body.bound.primitives,
            |p| nodes.get(p).is_ok_and(|(_, _, source)| source),
            &bound,
        ) {
            return;
        }
        body.shown = true;
    }
    if !body.dressed || outfit.is_changed() {
        dress(
            &mut commands,
            &outfit,
            &body.bound.primitives,
            outfit.model == MODEL_SHADOW,
            &slots,
            &materials,
        );
        body.dressed = true;
    }
    if let Ok((_, mut visibility, _)) = nodes.get_mut(scene) {
        // `enabledCharacter` 0 and death deactivate `CharacterRoot`.
        visibility.set_if_neq(
            if frame.frame.ragdoll.is_empty() && outfit.model != MODEL_OFF {
                Visibility::Inherited
            } else {
                Visibility::Hidden
            },
        );
    }
    let state = &frame.frame.state;
    pose_instance(
        &body.bound,
        &mut nodes,
        drive.rendered_body_pose(state.body_pose.pose()),
        state.body_root.pose(),
        state.joint_scale,
        &frame.frame.joints,
        outfit.character,
    );
}
