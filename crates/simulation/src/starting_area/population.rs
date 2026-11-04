//! Creature population: source `itemPlaceRemoveScript` for rabbits and humans, and the ragdoll
//! bone items of dead creatures. Far creatures unload, and far ones near an important object
//! come back where the world is loaded.
use super::GarageRuntime;
use crate::creatures::PlaceDistances;
use crate::creatures::ragdoll::Bone;
use crate::interaction::InteractionState;
use crate::interaction::items::{ItemLocation, ItemStore};
use crate::reference::{CreatureRagdoll, ItemId, ReferenceError, ReferenceManifest};
use crate::save::RabbitSnapshot;
use crate::vehicle::{PhysicsWorld, from_rapier};
use crate::world::WorldBounds;
use rapier3d::prelude::RigidBodyHandle;

/// Item ids for the bones of one ragdoll.
pub(super) fn allocate(
    r: &CreatureRagdoll,
    items: &mut ItemStore,
) -> Result<Vec<ItemId>, ReferenceError> {
    (0..r.bodies.len())
        .map(|_| items.allocate_item_id())
        .collect()
}

/// Each bone is an ordinary pickup item. The creature scene shows it.
pub(super) fn register(
    r: &CreatureRagdoll,
    bones: &[Bone],
    physics: &PhysicsWorld,
    items: &mut ItemStore,
    interaction: &mut InteractionState,
) -> Result<(), ReferenceError> {
    for (bone, pickup) in bones.iter().zip(&r.pickups) {
        let pose = from_rapier(physics.rapier().bodies[bone.body].position());
        items.insert_spawned(bone.item, "", pose, bone.body, None)?;
        interaction.register_spawned(bone.item, pickup.clone(), &bone.colliders);
    }
    Ok(())
}

impl GarageRuntime {
    /// `itemPlaceRemoveScript.DoUpd` for rabbits, then humans. The rabbit spawner then refills
    /// its slots.
    pub(super) fn place_remove_creatures(
        &mut self,
        reference: &ReferenceManifest,
        distances: PlaceDistances,
    ) {
        let (stream, origin) = (&self.stream, self.stream.origin);
        let remove = |p: rapier3d::math::Vector| {
            stream.important_distance(origin.absolute(p.to_array()), None) >= distances.remove
        };
        let mut bones = Vec::new();
        self.creatures
            .unload(&mut self.physics, &origin, remove, &mut bones);
        self.creatures
            .humans
            .unload(&mut self.physics, &origin, remove, &mut bones);
        self.release_bones(bones);

        let stream = &self.stream;
        let near = |p: [f64; 3]| {
            stream.important_distance(p, None) < distances.spawn
                && stream.supports(WorldBounds {
                    min: p.map(|v| v - 2.),
                    max: p.map(|v| v + 2.),
                })
        };
        let mut rabbits = Vec::new();
        while let Some(s) = self.creatures.take_far(near) {
            rabbits.push(s);
        }
        let mut humans = Vec::new();
        while let Some(s) = self.creatures.humans.take_far(near) {
            humans.push(s);
        }
        if rabbits.is_empty() && humans.is_empty() {
            return;
        }
        let now = self.simulation_seconds;
        if !rabbits.is_empty()
            && self
                .creatures
                .restore(
                    &reference.creatures.rabbit,
                    &mut self.physics,
                    &origin,
                    &rabbits,
                    now,
                )
                .and_then(|()| self.restore_dead_rabbits(reference, &rabbits))
                .is_err()
        {
            self.notice = "Cannot place the rabbit.";
        }
        for s in &humans {
            if self.restore_human(reference, s).is_err() {
                self.notice = "Cannot place the human.";
            }
        }
        self.physics.refresh_queries();
    }

    /// Removes the bone items and bodies of unloaded creatures.
    fn release_bones(&mut self, bones: Vec<(ItemId, RigidBodyHandle)>) {
        let area = &mut self.area;
        for (item, body) in bones {
            if area.interaction.held() == Some(item) {
                area.interaction
                    .drop_held(&mut area.items, &mut self.physics);
            }
            area.inventory.forget(item);
            area.interaction.forget_item(item, Some(body));
            self.physics.remove_body(body);
            if let Some(state) = area.items.set_body(item, None) {
                state.location = ItemLocation::World;
            }
            area.items.remove_spawned(item);
            area.item_render_poses.retain(|(i, _, _)| *i != item);
        }
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
            let items = allocate(&r.ragdoll, &mut area.items)?;
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
            register(
                &r.ragdoll,
                bones,
                &self.physics,
                &mut area.items,
                interaction,
            )?;
        }
        Ok(())
    }
}
