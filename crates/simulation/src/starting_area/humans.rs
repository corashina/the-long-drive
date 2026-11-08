//! Human monsters. Buildings place them, `itemPlaceRemoveScript` unloads and places them, and
//! `Die` turns the body into ragdoll bone items.
use super::GarageRuntime;
use super::population::{allocate, register};
use crate::reference::{ReferenceError, ReferenceManifest};
use crate::save::HumanSnapshot;
use crate::world::ObjectKey;
use rapier3d::math::{Pose, Rotation, Vector};

impl GarageRuntime {
    /// New human placements of the world contents. The host alone spawns them, as rabbits.
    /// A placement that spawned a human before does not spawn another.
    pub(super) fn place_humans(&mut self, reference: &ReferenceManifest) {
        let r = &reference.creatures.human;
        while let Some((placement, roll)) = self.stream.take_human() {
            let key = ObjectKey::Generated(placement.id);
            if self.net_guest || self.creatures.humans.known(&key) {
                continue;
            }
            let pose = Pose::from_parts(
                Vector::from_array(self.stream.origin.local(placement.pose.position)),
                Rotation::from_array(placement.pose.rotation),
            );
            if self
                .creatures
                .humans
                .place(r, &mut self.physics, key, pose, roll)
                .is_err()
            {
                self.notice = "Cannot place the human.";
            }
        }
    }

    /// `newAiScript.Update` of the live humans. Host only, as rabbits.
    pub(super) fn step_humans(&mut self, reference: &ReferenceManifest, dt: f32) {
        if self.net_guest {
            return;
        }
        let player = &self.area.player;
        let view = crate::creatures::human_ai::PlayerView {
            position: crate::vehicle::to_rapier(player.body_pose(&self.physics)).translation,
            head: Vector::from_array(player.focus_position(&self.physics)),
        };
        let interaction = &self.area.interaction;
        let mouth = (!player.dead()).then(|| {
            (
                interaction.mouth_center(player, &self.physics),
                rapier3d::parry::shape::Ball::new(interaction.mouth_radius()),
            )
        });
        let usable =
            |handle, physics: &crate::vehicle::PhysicsWorld| interaction.ai_usable(handle, physics);
        let mut doors = Vec::new();
        let r = &reference.creatures.human;
        let now = self.simulation_seconds;
        self.creatures.humans.voice_block(r, now, dt);
        let damage = self.creatures.humans.step_ai(
            r,
            &mut self.physics,
            &crate::creatures::human::AiInput {
                now: self.simulation_seconds,
                dt,
                player: Some((player.body_handle(), view)),
                mouth,
                peaceful: self.creatures.peaceful,
                roles: self.stream.trigger_roles(),
                usable: &usable,
            },
            &mut doors,
        );
        // `Attack`: `currentPlayer.survival.DamageInstant(damage * dt, show: true)`.
        if damage > 0. {
            self.survival.damage_instant(&reference.survival, damage);
            self.announce_death(reference);
        }
        self.creatures.humans.animate(r, &self.physics, now, dt);
        for door in doors {
            self.area
                .interaction
                .mechanisms
                .push(door.item, r.ai.door_speed * dt, dt);
        }
    }

    /// `breakablescript.Break` of the humans that broke this step: `Die(_playSound: true)`.
    pub(crate) fn settle_humans(&mut self, reference: &ReferenceManifest) {
        while let Some(id) = self.creatures.humans.take_death() {
            if self.kill_human(reference, id, None).is_err() {
                self.notice = "Cannot drop the human ragdoll.";
            }
            let r = &reference.creatures.human;
            self.creatures
                .humans
                .death_voice(r, id, self.simulation_seconds);
        }
    }

    /// `Die` of human `id`. `poses` gives saved bone poses.
    pub(crate) fn kill_human(
        &mut self,
        reference: &ReferenceManifest,
        id: u32,
        poses: Option<Vec<Pose>>,
    ) -> Result<(), ReferenceError> {
        let r = &reference.creatures.human;
        let area = &mut self.area;
        let items = allocate(&r.ragdoll, &mut area.items)?;
        let interaction = &mut area.interaction;
        let bones =
            self.creatures
                .humans
                .die(r, &mut self.physics, id, poses, &items, &mut || {
                    interaction.allocate_generated_collider()
                })?;
        register(
            &r.ragdoll,
            bones,
            &self.physics,
            &mut area.items,
            interaction,
        )
    }

    /// `save_newai.SetTosave`. A saved dead human calls `Die` with its saved bone poses.
    pub(super) fn restore_human(
        &mut self,
        reference: &ReferenceManifest,
        saved: &HumanSnapshot,
    ) -> Result<(), ReferenceError> {
        let origin = self.stream.origin;
        let id = self.creatures.humans.restore(
            &reference.creatures.human,
            &mut self.physics,
            &origin,
            saved,
        )?;
        if saved.died {
            let poses = saved
                .bones
                .iter()
                .map(|p| {
                    Pose::from_parts(
                        Vector::from_array(origin.local(p.position)),
                        Rotation::from_array(p.rotation),
                    )
                })
                .collect();
            self.kill_human(reference, id, Some(poses))?;
        }
        Ok(())
    }
}
