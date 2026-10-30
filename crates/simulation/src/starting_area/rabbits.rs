//! Rabbit deaths: `aiscript.Die` turns the live body into ragdoll bones that the player can
//! carry, and `nyulscript.Die` plays a die clip. Evil rabbit clips and attack triggers.
use super::GarageRuntime;
use crate::creatures::ragdoll::Bone;
use crate::creatures::stimuli::SoundStimulus;
use crate::interaction::InteractionState;
use crate::interaction::items::ItemStore;
use crate::reference::{ItemId, RabbitReference, ReferenceError, ReferenceManifest};
use crate::save::RabbitSnapshot;
use crate::vehicle::{from_rapier, to_rapier};
use rapier3d::math::Vector;

/// Audio instances of rabbit clips. Flyers share the voice and use item ids.
const AUDIO_INSTANCE: u32 = 1 << 31;
/// Sound stimulus keys of radio voices: the item id and the voice.
const RADIO_STIMULUS: u64 = 2 << 32;

fn allocate(r: &RabbitReference, items: &mut ItemStore) -> Result<Vec<ItemId>, ReferenceError> {
    (0..r.ragdoll.bodies.len())
        .map(|_| items.allocate_item_id())
        .collect()
}

/// Each bone is an ordinary pickup item. The rabbit scene shows it.
fn register(
    r: &RabbitReference,
    bones: &[Bone],
    physics: &crate::vehicle::PhysicsWorld,
    items: &mut ItemStore,
    interaction: &mut InteractionState,
) -> Result<(), ReferenceError> {
    for (bone, pickup) in bones.iter().zip(&r.ragdoll.pickups) {
        let pose = from_rapier(physics.rapier().bodies[bone.body].position());
        items.insert_spawned(bone.item, "", pose, bone.body, None)?;
        interaction.register_spawned(bone.item, pickup.clone(), &bone.colliders);
    }
    Ok(())
}

impl GarageRuntime {
    /// `stimulscript.CheckAudioSources` inputs: engine and horn loops of this step and the
    /// radio voices. Without audio there are none.
    pub(super) fn bind_rabbit_sounds(&mut self) {
        let bound = self.creatures.bound_sounds();
        bound.clear();
        let Some(audio) = &self.audio else {
            return;
        };
        bound.extend_from_slice(&audio.cars.stimuli);
        let cutoff = audio.reference.radio.electronics_cutoff;
        for (item, script, state, output, _) in self.stream.portable_radios() {
            let Some(pose) = self
                .area
                .items
                .get(item)
                .filter(|i| state.on && i.inactive_pose.is_none())
                .map(|i| i.pose)
            else {
                continue;
            };
            let frame = audio
                .broadcasts
                .frame(state, audio.game_seconds, output, cutoff);
            // `SradioSoundStatic`, `SradioSound` and `SradioSound2`.
            let gains = [frame.static_gain, frame.primary.gain, frame.secondary.gain];
            let pose = to_rapier(pose);
            for s in audio
                .reference
                .radio_stimuli
                .iter()
                .filter(|s| &s.radio == script)
            {
                bound.push(SoundStimulus {
                    key: RADIO_STIMULUS | u64::from(item.0) << 2 | u64::from(s.voice),
                    position: pose * Vector::from_array(s.offset),
                    value: gains[s.voice as usize] * s.max_sound_value,
                });
            }
        }
    }

    /// Turns the rabbits that died this step into ragdolls, plays the evil rabbit clips and
    /// updates the attack trigger roles.
    pub(super) fn settle_rabbits(&mut self, reference: &ReferenceManifest) {
        let r = &reference.creatures.rabbit;
        while let Some((id, sound)) = self.creatures.take_death() {
            if self.kill_rabbit(r, id, sound).is_err() {
                self.notice = "Cannot drop the rabbit ragdoll.";
            }
        }
        while let Some(sound) = self.creatures.take_sound() {
            let position = self.stream.origin.absolute(sound.position.to_array());
            self.play_rabbit_clip(r, sound.rabbit, sound.clip, position, sound.pitch);
        }
        let roles = self.stream.trigger_roles_mut();
        while let Some((handle, role)) = self.creatures.take_trigger(r) {
            match role {
                Some(role) => roles.insert(handle, role),
                None => roles.remove(handle),
            }
        }
    }

    /// `PlayClipAtPoint` of a rabbit clip. It makes a sound stimulus for the clip length.
    fn play_rabbit_clip(
        &mut self,
        r: &RabbitReference,
        id: u32,
        clip: u32,
        position: [f64; 3],
        pitch: f32,
    ) {
        let Some(audio) = &mut self.audio else {
            return;
        };
        let seconds = audio
            .reference
            .clips
            .iter()
            .find(|c| c.id == clip)
            .map_or(0., |c| c.source_duration_seconds as f32);
        self.creatures
            .sound(position, 1., self.simulation_seconds, seconds);
        audio.clip_at(r.die_voice, AUDIO_INSTANCE | id, clip, position, pitch);
    }

    fn kill_rabbit(
        &mut self,
        r: &RabbitReference,
        id: u32,
        sound: bool,
    ) -> Result<(), ReferenceError> {
        let area = &mut self.area;
        let items = allocate(r, &mut area.items)?;
        let interaction = &mut area.interaction;
        let (position, bones) =
            self.creatures
                .ragdoll(r, &mut self.physics, id, &items, &mut || {
                    interaction.allocate_generated_collider()
                })?;
        register(r, bones, &self.physics, &mut area.items, interaction)?;
        if !sound {
            return Ok(());
        }
        let position = self.stream.origin.absolute(position.to_array());
        let clip = self.creatures.die_clip(r);
        self.play_rabbit_clip(r, id, clip, position, 1.);
        Ok(())
    }

    /// Saved dead rabbits with their bones.
    pub(super) fn restore_dead_rabbits(
        &mut self,
        reference: &ReferenceManifest,
        saved: &[RabbitSnapshot],
    ) -> Result<(), ReferenceError> {
        let r = &reference.creatures.rabbit;
        for s in saved.iter().filter(|s| !s.alive) {
            let area = &mut self.area;
            let items = allocate(r, &mut area.items)?;
            let interaction = &mut area.interaction;
            let bones = self.creatures.restore_dead(
                r,
                &mut self.physics,
                &self.stream.origin,
                s,
                self.simulation_seconds,
                &items,
                &mut || interaction.allocate_generated_collider(),
            )?;
            register(r, bones, &self.physics, &mut area.items, interaction)?;
        }
        Ok(())
    }
}
