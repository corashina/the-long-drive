//! Browser audio calls of one rendered frame.
use super::GarageRuntime;
use crate::boundary::{AudioCall, RenderInput};
use crate::reference::ReferenceManifest;

impl GarageRuntime {
    /// Advances the audio mixers and writes the audio calls of this frame.
    pub(super) fn fill_audio(
        &mut self,
        reference: &ReferenceManifest,
        alpha: f32,
        input: RenderInput,
        out: &mut Vec<AudioCall>,
    ) {
        out.clear();
        let Self {
            audio,
            area,
            stream,
            physics,
            survival,
            hearing,
            vehicles,
            started,
            paused,
            audio_motion_revision,
            ..
        } = self;
        let Some(audio) = audio else {
            return;
        };
        out.push(AudioCall::Begin {
            session: audio.session,
            started: *started,
            paused: *paused,
            seconds: audio.game_seconds,
        });
        let needs = survival.output();
        let level = if needs.dead {
            (1. - needs.death_seconds as f32 / reference.survival.death_seconds).clamp(0.001, 1.)
        } else {
            1.
        };
        out.push(AudioCall::SurvivalMaster {
            gain: 10_f32.powf(level.ln()),
            revision: *audio_motion_revision,
            seconds: audio.game_seconds + f64::from(alpha) / 60.,
        });
        out.push(AudioCall::PlayerEar {
            db: hearing.ear_db(),
            clip: hearing.clip,
            ring: hearing.ring_gain(),
            pan: hearing.dir,
        });
        let shift = audio.take_origin_shift();
        if shift != [0.; 3] {
            out.push(AudioCall::OriginShift(shift));
        }
        out.push(AudioCall::Listener);
        let body = crate::vehicle::to_rapier(area.player.body_pose(physics));
        for b in &audio.reference.survival {
            let p = body * rapier3d::math::Vector::from_array(b.offset);
            out.push(AudioCall::SurvivalEmitter {
                kind: b.kind,
                position: p.to_array(),
            });
        }
        let origin = stream.origin.offset;
        let local = |p: [f64; 3]| std::array::from_fn(|i| (p[i] - origin[i]) as f32);
        for e in audio.survival_loops.iter().chain(&audio.survival_events) {
            out.push(AudioCall::Survival {
                kind: e.kind as u32,
                emitter: e.emitter,
                action: e.action as u32,
                clip: e.clip,
                gain: e.gain,
                pitch: e.pitch,
                position: local(e.position),
            });
        }
        audio.survival_events.clear();
        for emitter in &audio.reference.emitters {
            let alive = audio.owner_alive(emitter.id)
                && emitter.item.is_none_or(|id| {
                    area.items
                        .get(id)
                        .is_some_and(|item| item.inactive_pose.is_none())
                });
            out.push(AudioCall::NodeEmitter {
                id: emitter.id,
                node: emitter.node,
                alive,
            });
        }
        if *started && !*paused {
            audio.update_mixers(area, stream, input.real_dt);
        }
        for (id, db) in audio.mixer_outputs() {
            out.push(AudioCall::Mixer { id, db });
        }
        for (index, route) in audio.reference.routes.iter().enumerate() {
            for &id in &route.voices {
                out.push(AudioCall::Route {
                    id,
                    mixer: audio.route_mixers[index],
                });
            }
        }
        for signal in &audio.signals {
            out.push(AudioCall::Voice {
                id: signal.id,
                playing: signal.playing,
                gain: signal.gain,
                pitch: signal.pitch,
                looping: signal.looping,
                clip: signal.clip,
            });
        }
        // Car voices play through the survival path with one record per car voice template.
        for &(instance, mixer) in &audio.cars.routes {
            out.push(AudioCall::Route {
                id: crate::audio::cars::CAR_ROUTE + instance,
                mixer,
            });
        }
        for e in audio
            .cars
            .loops
            .iter()
            .chain(&audio.cars.events)
            .chain(&audio.voice_loops)
            .chain(&audio.tool_loops)
        {
            out.push(AudioCall::Survival {
                kind: e.kind,
                emitter: e.instance,
                action: e.action as u32,
                clip: e.clip,
                gain: e.gain,
                pitch: e.pitch,
                position: local(e.position),
            });
        }
        audio.cars.events.clear();
        while let Some(event) = audio.pop_event() {
            out.push(AudioCall::Event {
                session: event.session,
                sequence: event.sequence,
                binding: event.binding,
                emitter: event.emitter,
                position: event.position,
                gain: event.gain,
                pitch: event.pitch,
                seconds: event.time_seconds,
            });
        }
        let cutoff = audio.reference.radio.electronics_cutoff;
        let items = &area.items;
        let radios = stream
            .portable_radios()
            .filter_map(|(id, script, state, output, _)| {
                items
                    .get(id)
                    .filter(|item| item.inactive_pose.is_none())
                    .map(|item| (id, script, item.pose.position, state.on && output > cutoff))
            });
        if let Some(listener) = input.listener {
            audio.assign_portable_radios(listener, radios);
        }
        let slots = audio.portable_radio_slots.iter().flatten();
        for (slot, id) in slots.enumerate() {
            let Some((item, (_, _, state, output, parent))) = id.and_then(|id| {
                items
                    .get(id)
                    .zip(stream.portable_radios().find(|r| r.0 == id))
            }) else {
                continue;
            };
            let emitter = crate::audio::PORTABLE_RADIO_EMITTER + slot as u32;
            out.push(AudioCall::Emitter {
                id: emitter,
                position: item.pose.position,
                rotation: item.pose.rotation,
                alive: true,
            });
            out.push(AudioCall::Radio {
                emitter,
                frame: audio
                    .broadcasts
                    .frame(state, audio.game_seconds, output, cutoff),
            });
            // Source `audioBasedOnInteriorsensscript`: a car radio plays through its car mixer.
            let car = vehicles.iter().find_map(|(key, car)| match key {
                crate::world::ObjectKey::Generated(id) if id == parent => {
                    car.record(reference).audio.as_ref().map(|a| &a.route)
                }
                _ => None,
            });
            let group = slot / crate::audio::PORTABLE_RADIO_SLOTS;
            for (k, voice) in audio.reference.portable_radios[group]
                .voices
                .iter()
                .enumerate()
            {
                let mixer = car.map_or(voice.mixer, |route| audio.world_car_mixer(route, parent));
                out.push(AudioCall::Route {
                    id: crate::audio::PORTABLE_RADIO_EMITTER + slot as u32 * 4 + k as u32 + 1,
                    mixer,
                });
            }
        }
        out.push(AudioCall::End);
    }
}
