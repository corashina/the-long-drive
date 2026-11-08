//! Remote player figures of a multiplayer session (source `snTempPlayerSync`, `mpplayerscript`).
//! Each figure is one more player asset instance. It takes the remote rows of the frame and the outfit of its peer.
//! A dead player shows as its own ragdoll instance at the remote ragdoll rows.
//! The browser draws the name labels (source `helptextscript`) at the positions that `sync` reports.
use super::camera::DriveCamera;
use super::materials::SourceMaterial;
use super::outfit::{MODEL_OFF, Outfit};
use super::player_body::{
    BoundBody, BoundQuery, NodeQuery, OutfitQuery, bind_instance, dress, materials_bound,
    pose_instance, spawn_instance,
};
use super::ragdoll::{self, RagdollInstance, Style};
use super::reference::ReferenceVisuals;
use super::scene_nodes::SceneGraph;
use super::sim_frame::SimFrameResource;
use crate::app::RuntimeReference;
use bevy::ecs as bevy_ecs;
use bevy::prelude::*;
use bevy::world_serialization::WorldInstanceReady;
use simulation::boundary::RemoteRow;
use std::collections::HashMap;

/// Port choice. The label stands this far above the Player Transform.
const LABEL_HEIGHT: f32 = 2.1;

#[derive(Component)]
pub struct RemoteFigure {
    slot: u32,
    bound: Option<BoundBody>,
    shown: bool,
    dressed: bool,
    /// Ragdoll instance, spawned at the first death.
    ragdoll: Option<Entity>,
}

/// Outfits of the peers, by slot. A peer without a look wears the defaults.
#[derive(Resource, Default)]
pub struct RemoteLooks {
    outfits: HashMap<u32, Outfit>,
    revision: u32,
}

/// Figure entities by slot, and the looks revision each one wears.
#[derive(Resource, Default)]
pub struct RemoteFigures {
    figures: Vec<(u32, Entity, u32)>,
}

fn bind(
    event: On<WorldInstanceReady>,
    mut commands: Commands,
    reference: Res<RuntimeReference>,
    frame: Res<SimFrameResource>,
    mut figures: Query<&mut RemoteFigure>,
    graph: SceneGraph,
) {
    let Ok(mut figure) = figures.get_mut(event.entity) else {
        return;
    };
    figure.bound = bind_instance(
        figure.slot as u8 + 1,
        event.entity,
        &mut commands,
        &reference.0,
        frame.rig(),
        &graph,
    );
}

#[cfg(target_arch = "wasm32")]
/// Reads the peer outfits that the browser sends after each change: JSON `[[slot, outfit], ...]`.
pub(crate) fn poll(mut looks: ResMut<RemoteLooks>, reference: Res<RuntimeReference>) {
    let Some(json) = crate::browser::game_remote_looks() else {
        return;
    };
    match serde_json::from_str::<Vec<(u32, Outfit)>>(&json) {
        Ok(values) => {
            let characters = &reference.0.clothing.characters;
            looks.outfits = values
                .into_iter()
                .filter(|(_, outfit)| outfit.valid(characters))
                .collect();
            looks.revision = looks.revision.wrapping_add(1);
        }
        Err(error) => warn!("Remote looks: {error}"),
    }
}

pub(crate) fn sync(
    mut commands: Commands,
    reference: Res<RuntimeReference>,
    frame: Res<SimFrameResource>,
    visuals: Res<ReferenceVisuals>,
    gltfs: Res<Assets<bevy::gltf::Gltf>>,
    materials: Res<Assets<SourceMaterial>>,
    looks: Res<RemoteLooks>,
    slots: OutfitQuery,
    bound: BoundQuery,
    mut state: ResMut<RemoteFigures>,
    mut figures: Query<&mut RemoteFigure>,
    mut ragdolls: Query<&mut RagdollInstance>,
    mut nodes: NodeQuery,
    camera: Query<(&Camera, &GlobalTransform), With<DriveCamera>>,
    mut defaults: Local<Option<Outfit>>,
) {
    if !visuals.bound {
        return;
    }
    let rows = &frame.frame.remotes;
    state.figures.retain(|(slot, entity, _)| {
        let kept = rows.iter().any(|r| r.slot == *slot);
        if !kept {
            if let Some(ragdoll) = figures.get(*entity).ok().and_then(|f| f.ragdoll) {
                commands.entity(ragdoll).despawn();
            }
            commands.entity(*entity).despawn();
            #[cfg(target_arch = "wasm32")]
            crate::browser::game_remote_label(*slot, -1., -1.);
            #[cfg(target_arch = "wasm32")]
            crate::browser::game_remote_voice(*slot, 0., 0., 0., false);
        }
        kept
    });
    let defaults =
        defaults.get_or_insert_with(|| Outfit::defaults(&reference.0.clothing.characters));
    let joint_count = frame.rig().len();
    let camera = camera.single().ok();
    let bodies = reference.0.clothing.ragdoll.bodies.len();
    let mut ragdoll_rows = frame.frame.remote_ragdolls.as_slice();
    for (index, row) in rows.iter().enumerate() {
        let dead = row.flags & RemoteRow::RAGDOLL != 0;
        let ragdoll_rows = if dead {
            let (own, rest) = ragdoll_rows.split_at(bodies.min(ragdoll_rows.len()));
            ragdoll_rows = rest;
            own
        } else {
            &[]
        };
        let Some(at) = state
            .figures
            .iter()
            .position(|(slot, ..)| *slot == row.slot)
        else {
            if let Some(scene) = spawn_instance(&mut commands, &reference.0, &visuals, &gltfs) {
                commands
                    .entity(scene)
                    .insert(RemoteFigure {
                        slot: row.slot,
                        bound: None,
                        shown: false,
                        dressed: false,
                        ragdoll: None,
                    })
                    .observe(bind);
                state.figures.push((row.slot, scene, looks.revision));
            }
            continue;
        };
        let (_, scene, revision) = &mut state.figures[at];
        let Ok(mut figure) = figures.get_mut(*scene) else {
            continue;
        };
        let figure = &mut *figure;
        let Some(body) = &figure.bound else {
            continue;
        };
        let outfit = looks.outfits.get(&row.slot).unwrap_or(defaults);
        // Show the figure after its source materials replace the glTF materials.
        if !figure.shown {
            if !materials_bound(
                &body.primitives,
                |p| nodes.get(p).is_ok_and(|(_, _, source)| source),
                &bound,
            ) {
                continue;
            }
            figure.shown = true;
        }
        let redress = *revision != looks.revision;
        if dead && figure.ragdoll.is_none() {
            figure.ragdoll = ragdoll::spawn(
                &mut commands,
                &reference,
                &visuals,
                &gltfs,
                row.slot as u8 + 1,
            );
        }
        if let Some(scene) = figure.ragdoll
            && let Ok(mut instance) = ragdolls.get_mut(scene)
        {
            let style = Style {
                outfit,
                redress,
                slots: &slots,
                materials: &materials,
                bound: &bound,
            };
            ragdoll::show(
                &mut commands,
                &reference,
                scene,
                &mut instance,
                ragdoll_rows,
                &style,
                &mut nodes,
                |row| super::transform(row.pose()),
            );
        }
        if !figure.dressed || redress {
            dress(
                &mut commands,
                outfit,
                &body.primitives,
                false,
                &slots,
                &materials,
            );
            figure.dressed = true;
            *revision = looks.revision;
        }
        let visible = row.flags & RemoteRow::VISIBLE != 0 && outfit.model != MODEL_OFF;
        if let Ok((_, mut visibility, _)) = nodes.get_mut(*scene) {
            visibility.set_if_neq(if visible {
                Visibility::Inherited
            } else {
                Visibility::Hidden
            });
        }
        let joints = frame
            .frame
            .remote_joints
            .get(index * joint_count..(index + 1) * joint_count)
            .unwrap_or_default();
        let pose = row.body_pose.pose();
        pose_instance(
            body,
            &mut nodes,
            pose,
            row.body_root.pose(),
            frame.frame.state.joint_scale,
            joints,
            outfit.character,
        );
        // The voice plays at the label point above the head (source `mpplayerscript.SVoice`).
        let head = Vec3::from_array(pose.position) + Vec3::Y * LABEL_HEIGHT;
        let label = camera
            .filter(|_| visible)
            .and_then(|(camera, transform)| camera.world_to_viewport(transform, head).ok())
            .unwrap_or(Vec2::splat(-1.));
        #[cfg(target_arch = "wasm32")]
        {
            crate::browser::game_remote_label(row.slot, label.x, label.y);
            crate::browser::game_remote_voice(row.slot, head.x, head.y, head.z, true);
        }
        #[cfg(not(target_arch = "wasm32"))]
        let _ = (label, head);
    }
}
