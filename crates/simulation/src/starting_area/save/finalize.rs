use super::StartingArea;
use crate::interaction::{items::ItemStore, wheels::WheelAssemblies};
use crate::reference::{BodyId, ReferenceManifest};
use crate::save::{
    BodyIdentity, BodySnapshot, IdentityMap, ItemSnapshot, PersistentWorld, SaveError, SaveSnapshot,
};
use crate::vehicle::PhysicsWorld;
use crate::world::{ChangeRecords, WorldStream};
use rapier3d::prelude::RigidBodyHandle;
use std::collections::HashMap;

pub(super) struct RestoreFinalizer<'a> {
    pub(super) area: &'a mut StartingArea,
    pub(super) physics: &'a mut PhysicsWorld,
    pub(super) wheels: &'a mut WheelAssemblies,
    pub(super) persistent: &'a mut PersistentWorld,
    pub(super) stream: &'a mut WorldStream,
}

impl RestoreFinalizer<'_> {
    pub(super) fn initial(
        &mut self,
        reference: &ReferenceManifest,
        ids: &IdentityMap,
        saved: &SaveSnapshot,
    ) -> Result<(), SaveError> {
        self.persistent
            .sync(reference, self.area, self.wheels, self.stream, self.physics)?;
        self.area.interaction.sync_mechanisms(self.physics);
        self.area.sync_attachments(self.physics, false);
        self.area
            .interaction
            .sync_parts(&mut self.area.items, self.physics)?;
        self.area.sync_portable_mass(self.physics);
        self.wheels.sync(&mut self.area.items, self.physics)?;
        self.area.items.restore_save(ids, &saved.items, reference)?;
        self.area.items.cleanup_loaded_fluids();
        self.area
            .interaction
            .sync_parts(&mut self.area.items, self.physics)?;
        self.area.sync_portable_mass(self.physics);
        self.wheels.sync(&mut self.area.items, self.physics)?;
        self.area.items.sync_poses(self.physics);
        recompute_dynamic_mass(self.physics);
        restore_body_records(
            saved.bodies.iter(),
            BodyLookup::Required,
            ids,
            &self.area.bodies,
            &self.area.items,
            self.physics,
        )?;
        self.area
            .interaction
            .cleaning
            .sync_wash_colliders(&self.area.interaction.conditions, self.physics);
        self.physics.refresh_queries();
        Ok(())
    }

    pub(super) fn deferred(
        &mut self,
        reference: &ReferenceManifest,
        ids: &IdentityMap,
        motion: &mut Vec<BodySnapshot>,
        unresolved: &[ItemSnapshot],
    ) -> Result<(), SaveError> {
        self.persistent
            .sync(reference, self.area, self.wheels, self.stream, self.physics)?;
        self.area
            .interaction
            .sync_parts(&mut self.area.items, self.physics)?;
        self.wheels
            .refresh_conditions(&self.area.interaction.conditions);
        self.wheels.sync(&mut self.area.items, self.physics)?;
        self.area.sync_equipment(reference, self.physics);
        self.area.items.sync_poses(self.physics);
        self.area.commit_poses(self.physics, true);
        recompute_dynamic_mass(self.physics);
        restore_body_records(
            motion.iter(),
            BodyLookup::Available,
            ids,
            &self.area.bodies,
            &self.area.items,
            self.physics,
        )?;
        restore_world_motion(
            motion,
            self.stream.changes(),
            &self.area.items,
            self.physics,
        );
        self.stream
            .release_saved_item_locations(&self.area.items, unresolved);
        self.physics.refresh_queries();
        Ok(())
    }
}

enum BodyLookup {
    Required,
    Available,
}

fn recompute_dynamic_mass(physics: &mut PhysicsWorld) {
    // Rapier defers collider and additional-mass updates until a step or recomputation.
    // Commit them before restoring motion and sleep, since mass changes may wake bodies.
    physics.edit_bodies(|_, body, colliders| {
        if body.is_dynamic() {
            body.recompute_mass_properties_from_colliders(colliders);
        }
    });
}

fn restore_body_records<'a>(
    records: impl Iterator<Item = &'a BodySnapshot>,
    lookup: BodyLookup,
    ids: &IdentityMap,
    bodies: &HashMap<BodyId, RigidBodyHandle>,
    items: &ItemStore,
    physics: &mut PhysicsWorld,
) -> Result<(), SaveError> {
    for record in records {
        let handle = match &record.identity {
            BodyIdentity::Source { source } => match lookup {
                BodyLookup::Required => Some(bodies[&ids.bodies[source]]),
                BodyLookup::Available => bodies.get(&ids.bodies[source]).copied(),
            },
            BodyIdentity::Assembly { root } => {
                let handle = items.get(ids.item(root)?).and_then(|item| item.body);
                match lookup {
                    BodyLookup::Required => Some(handle.ok_or_else(|| {
                        SaveError::ReconstructionFailure("Missing assembly body".into())
                    })?),
                    BodyLookup::Available => handle,
                }
            }
        };
        if let Some(handle) = handle.filter(|handle| physics.body_active(*handle)) {
            physics.edit_body(handle, |body| {
                body.set_linvel(record.linear_velocity.into(), false);
                body.set_angvel(record.angular_velocity.into(), false);
                if record.sleeping {
                    body.sleep();
                } else {
                    body.wake_up(true);
                }
            });
        }
    }
    Ok(())
}

fn restore_world_motion(
    motion: &mut Vec<BodySnapshot>,
    changes: &ChangeRecords,
    items: &ItemStore,
    physics: &mut PhysicsWorld,
) {
    if !motion.is_empty() {
        for (key, change) in changes.iter() {
            if let (Some(id), Some(snapshot)) = (items.resolve_key(key), &change.snapshot) {
                if snapshot.attachment.is_none() {
                    crate::world::changes::restore_motion(id, snapshot, items, physics);
                }
            }
        }
    }
    motion.clear();
}
