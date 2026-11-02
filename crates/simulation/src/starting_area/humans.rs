//! Human monsters: buildings place them, `itemPlaceRemoveScript` unloads and places them, and
//! `Die` turns the body into ragdoll bone items.
use super::GarageRuntime;
use super::rabbits::{allocate, register};
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
        self.creatures.humans.step();
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

    /// Source `itemPlaceRemoveScript` for humans: far ones unload, and saved or far ones near an
    /// important object come back where the world is loaded.
    pub(super) fn place_remove_humans(
        &mut self,
        reference: &ReferenceManifest,
        distances: crate::creatures::PlaceDistances,
    ) {
        let (stream, origin) = (&self.stream, self.stream.origin);
        let mut bones = Vec::new();
        self.creatures.humans.unload(
            &mut self.physics,
            &origin,
            |p| stream.important_distance(origin.absolute(p.to_array()), None) >= distances.remove,
            &mut bones,
        );
        self.release_bones(bones);
        let stream = &self.stream;
        let near = |p: [f64; 3]| {
            stream.important_distance(p, None) < distances.spawn
                && stream.supports(crate::world::WorldBounds {
                    min: p.map(|v| v - 2.),
                    max: p.map(|v| v + 2.),
                })
        };
        let mut placed = Vec::new();
        while let Some(s) = self.creatures.humans.take_far(near) {
            placed.push(s);
        }
        for s in &placed {
            if self.restore_human(reference, s).is_err() {
                self.notice = "Cannot place the human.";
            }
        }
        if !placed.is_empty() {
            self.physics.refresh_queries();
        }
    }

    /// `save_newai.SetTosave`: a saved dead human calls `Die` with its saved bone poses.
    fn restore_human(
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
