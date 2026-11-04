//! Sends the audio calls of a frame to `web/audio.js` in one JS call.
//! Each call is an opcode and fixed arguments in one `Float64Array`. f64 keeps u32 ids and times exact.
use super::Drive;
use crate::browser::game_audio_frame;
use crate::presentation::camera::DriveCamera;
use crate::presentation::reference::ReferenceVisuals;
use crate::presentation::sim_frame::SimFrameResource;
use bevy::prelude::*;
use simulation::boundary::AudioCall;

/// Opcodes of the batch. Keep in step with `OP` in `web/audio.js`.
const BEGIN: f64 = 0.0;
const SURVIVAL_MASTER: f64 = 1.0;
const PLAYER_EAR: f64 = 2.0;
const ORIGIN_SHIFT: f64 = 3.0;
const LISTENER: f64 = 4.0;
const SURVIVAL_EMITTER: f64 = 5.0;
const SURVIVAL: f64 = 6.0;
const EMITTER: f64 = 7.0;
const VOICE: f64 = 8.0;
const EVENT: f64 = 9.0;
const MIXER: f64 = 10.0;
const ROUTE: f64 = 11.0;
const RADIO: f64 = 12.0;
const END: f64 = 13.0;

fn flag(value: bool) -> f64 {
    if value { 1.0 } else { 0.0 }
}

fn push_emitter(batch: &mut Vec<f64>, id: u32, p: Vec3, q: Quat, alive: bool) {
    batch.extend([
        EMITTER,
        id.into(),
        p.x.into(),
        p.y.into(),
        p.z.into(),
        q.x.into(),
        q.y.into(),
        q.z.into(),
        q.w.into(),
        flag(alive),
    ]);
}

pub(super) fn publish_audio(
    mut drive: ResMut<Drive>,
    mut batch: Local<Vec<f64>>,
    sim: Res<SimFrameResource>,
    visuals: Res<ReferenceVisuals>,
    globals: Query<(&GlobalTransform, &InheritedVisibility)>,
    camera: Query<&GlobalTransform, With<DriveCamera>>,
) {
    let camera = camera
        .single()
        .ok()
        .map(|c| c.to_scale_rotation_translation());
    drive.listener = camera.map(|(_, _, p)| p.to_array());
    if sim.frame.audio.is_empty() {
        return;
    }
    batch.clear();
    for call in &sim.frame.audio {
        match *call {
            AudioCall::Begin {
                session,
                started,
                paused,
                seconds,
            } => batch.extend([BEGIN, session.into(), flag(started), flag(paused), seconds]),
            AudioCall::SurvivalMaster {
                gain,
                revision,
                seconds,
            } => batch.extend([SURVIVAL_MASTER, gain.into(), revision.into(), seconds]),
            AudioCall::PlayerEar {
                db,
                clip,
                ring,
                pan,
            } => batch.extend([PLAYER_EAR, db.into(), clip.into(), ring.into(), pan.into()]),
            AudioCall::OriginShift([x, y, z]) => {
                batch.extend([ORIGIN_SHIFT, x.into(), y.into(), z.into()])
            }
            AudioCall::Listener => {
                if let Some((_, q, p)) = camera {
                    batch.extend([
                        LISTENER,
                        p.x.into(),
                        p.y.into(),
                        p.z.into(),
                        q.x.into(),
                        q.y.into(),
                        q.z.into(),
                        q.w.into(),
                    ]);
                }
            }
            AudioCall::SurvivalEmitter {
                kind,
                position: [x, y, z],
            } => batch.extend([SURVIVAL_EMITTER, kind.into(), x.into(), y.into(), z.into()]),
            AudioCall::Survival {
                kind,
                emitter,
                action,
                clip,
                gain,
                pitch,
                position: [x, y, z],
            } => batch.extend([
                SURVIVAL,
                kind.into(),
                emitter.into(),
                action.into(),
                clip.into(),
                gain.into(),
                pitch.into(),
                x.into(),
                y.into(),
                z.into(),
            ]),
            AudioCall::NodeEmitter { id, node, alive } => {
                if let Some((global, visibility)) = visuals
                    .nodes
                    .get(&node)
                    .and_then(|&entity| globals.get(entity).ok())
                {
                    let (_, q, p) = global.to_scale_rotation_translation();
                    push_emitter(&mut batch, id, p, q, alive && visibility.get());
                }
            }
            AudioCall::Emitter {
                id,
                position,
                rotation,
                alive,
            } => push_emitter(
                &mut batch,
                id,
                Vec3::from_array(position),
                Quat::from_array(rotation),
                alive,
            ),
            AudioCall::Mixer { id, db } => batch.extend([MIXER, id.into(), db.into()]),
            AudioCall::Route { id, mixer } => batch.extend([ROUTE, id.into(), mixer.into()]),
            AudioCall::Voice {
                id,
                playing,
                gain,
                pitch,
                looping,
                clip,
            } => batch.extend([
                VOICE,
                id.into(),
                flag(playing),
                gain.into(),
                pitch.into(),
                flag(looping),
                clip.into(),
            ]),
            AudioCall::Event {
                session,
                sequence,
                binding,
                emitter,
                position: [x, y, z],
                gain,
                pitch,
                seconds,
            } => batch.extend([
                EVENT,
                session.into(),
                sequence.into(),
                binding.into(),
                emitter.into(),
                x.into(),
                y.into(),
                z.into(),
                gain.into(),
                pitch.into(),
                seconds,
            ]),
            AudioCall::Radio { emitter, frame } => batch.extend([
                RADIO,
                emitter.into(),
                flag(frame.power),
                flag(frame.am),
                frame.frequency.into(),
                frame.volume.into(),
                frame.static_gain.into(),
                frame.primary.clip.unwrap_or(0).into(),
                frame.primary.offset_seconds,
                frame.primary.gain.into(),
                frame.primary.next_clip.unwrap_or(0).into(),
                frame.secondary.clip.unwrap_or(0).into(),
                frame.secondary.offset_seconds,
                frame.secondary.gain.into(),
                frame.secondary.next_clip.unwrap_or(0).into(),
            ]),
            AudioCall::End => batch.push(END),
        }
    }
    game_audio_frame(&batch);
}
