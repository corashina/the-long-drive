use super::*;
use crate::world::{AttachmentSnapshot, GeneratedId, ObjectKey, SpawnContext};

impl SaveSnapshot {
    pub(super) fn validate_world(&self, reference: &ReferenceManifest) -> Result<(), SaveError> {
        self.world
            .generation
            .validate_identity(&reference.content_base)
            .map_err(|e| invalid(&e.0))?;
        let roads = crate::world::RoadIndex::new(
            // The biome does not change road states or bounds.
            crate::world::RoadSequence::new(&reference.world, self.world.seed, None)
                .map_err(|e| invalid(&e.0))?,
            crate::world::terrain_evaluator(&reference.world, self.world.seed),
        );
        roads
            .validate_index(&self.world.generation.roads)
            .map_err(|e| invalid(&e.0))?;
        crate::world::PlacementIndex::new(&reference.world, self.world.seed)
            .validate_index(&self.world.generation.placements)
            .map_err(|e| invalid(&e.0))?;
        let sources: HashSet<_> =
            reference
                .world
                .records
                .iter()
                .map(|r| &r.source)
                .chain(
                    reference
                        .world
                        .initialization
                        .components
                        .iter()
                        .map(|r| &r.source),
                )
                .chain(
                    reference
                        .world
                        .initialization
                        .nodes
                        .iter()
                        .flat_map(|r| [&r.source, &r.owner]),
                )
                .chain(reference.world.prototypes.iter().flat_map(|p| {
                    std::iter::once(&p.source).chain(p.nodes.iter().map(|n| &n.source))
                }))
                .collect();
        let source = |s: &r::SourceId| links(sources.contains(s), "Unknown source-world identity");
        let generated = |id: &GeneratedId| -> Result<(), SaveError> {
            source(&id.generator)?;
            for (child, _) in &id.children {
                source(child)?;
            }
            data(
                id.unit
                    .iter()
                    .all(|v| v.unsigned_abs() <= crate::world::MAX_ABSOLUTE_COORDINATE as u64),
                "Generated unit",
            )
        };
        let object = |key: &ObjectKey| -> Result<(), SaveError> {
            match key {
                ObjectKey::Imported(id) => links(
                    reference.items.iter().any(|i| i.id == *id),
                    "Unknown imported world item",
                ),
                ObjectKey::Generated(id) => generated(id),
            }
        };
        let exists = |key: &ObjectKey| match key {
            ObjectKey::Imported(id) => reference.items.iter().any(|i| i.id == *id),
            ObjectKey::Generated(_) => self
                .world
                .changes
                .get(key)
                .is_some_and(|c| c.snapshot.is_some()),
        };
        if let Some(SeatSnapshot {
            owner: SeatOwnerSnapshot::Item { item },
            ..
        }) = &self.player.seat
        {
            object(item)?;
            links(exists(item), "Removed or missing seat item")?;
        }
        let attachment = |a: &AttachmentSnapshot| -> Result<(), SaveError> {
            local(&LocalPose {
                position: a.local_pose.position.map(|v| v as f32),
                rotation: a.local_pose.rotation,
            })?;
            data(
                a.local_pose
                    .position
                    .iter()
                    .all(|v| v.is_finite() && v.abs() <= f64::from(crate::world::REBASE_LIMIT)),
                "Attachment local position",
            )?;
            if let Some(owner) = &a.owner {
                object(owner)?;
                links(exists(owner), "Removed or missing attachment owner")?;
            }
            if let Some(slot) = &a.slot {
                links(
                    sources.contains(slot)
                        || reference.part_slots.iter().any(|s| &s.source == slot),
                    "Unknown attachment slot",
                )?;
            }
            // The owner prototype is known when the owner record is its own placement.
            let prototype = a.owner.as_ref().and_then(|owner| match owner {
                ObjectKey::Generated(id) => self
                    .world
                    .changes
                    .get(owner)
                    .and_then(|c| c.replay.as_ref())
                    .filter(|p| &p.id == id)
                    .map(|p| &p.prototype),
                ObjectKey::Imported(_) => None,
            });
            storage_volume(&reference.storage, a, prototype)?;
            links(a.owner.is_some() || a.slot.is_some(), "Empty attachment")
        };
        let pose = |p: &crate::world::AbsolutePose| {
            absolute(&AbsolutePose {
                position: p.position,
                rotation: p.rotation,
            })
        };
        for (index, release) in self.world.boxes.iter().enumerate() {
            generated(&release.id)?;
            pose(&release.origin)?;
            pose(&release.pose)?;
            links(
                self.world
                    .changes
                    .get(ObjectKey::Generated(release.id.clone()))
                    .is_some_and(|c| c.snapshot.is_none())
                    && !self.world.boxes[..index].iter().any(|r| r.id == release.id),
                "Broken box without one removal",
            )?;
        }
        let mut edges = HashMap::new();
        let mut slots = HashSet::new();
        let mut held = None;
        for (key, change) in self.world.changes.iter() {
            let ObjectKey::Generated(id) = key else {
                return Err(invalid(
                    "Imported state has a separate canonical save owner",
                ));
            };
            generated(id)?;
            let replay = change
                .replay
                .as_ref()
                .ok_or_else(|| invalid("Missing original placement"))?;
            let instance = change
                .instance
                .as_ref()
                .ok_or_else(|| invalid("Missing generated instance"))?;
            generated(&replay.id)?;
            generated(instance)?;
            links(
                reference
                    .world
                    .prototypes
                    .iter()
                    .any(|p| p.source == replay.prototype),
                "Unknown replay prototype",
            )?;
            links(
                descendant(id, instance) && descendant(instance, &replay.id),
                "Replay identity ancestry",
            )?;
            pose(&replay.pose)?;
            data(
                replay.scale.iter().all(|v| v.is_finite() && *v != 0.),
                "Replay scale",
            )?;
            data(
                (0..3).all(|i| {
                    replay.bounds.min[i].is_finite()
                        && replay.bounds.max[i].is_finite()
                        && replay.bounds.min[i] <= replay.bounds.max[i]
                        && replay.bounds.min[i].abs() <= crate::world::MAX_ABSOLUTE_COORDINATE
                        && replay.bounds.max[i].abs() <= crate::world::MAX_ABSOLUTE_COORDINATE
                }),
                "Replay bounds",
            )?;
            let Some(s) = &change.snapshot else {
                links(
                    id == instance,
                    "Removal must identify a complete generated instance",
                )?;
                continue;
            };
            pose(&s.pose)?;
            data(
                s.temperature.is_none_or(f32::is_finite) && s.valve.is_none_or(unit),
                "Generated scalar state",
            )?;
            data(
                bounded_local(&s.linear_velocity) && bounded_local(&s.angular_velocity),
                "Generated velocity",
            )?;
            data(
                s.fluids.as_ref().is_none_or(|entries| {
                    crate::fluids::validate_fluid_entries(entries.as_slice())
                }),
                "Generated fluids",
            )?;
            if let Some(food) = &s.food {
                let recipe = reference
                    .survival
                    .foods
                    .iter()
                    .find(|f| f.source == food.source)
                    .ok_or_else(|| invalid("Unknown edible recipe"))?;
                data(
                    food.bite <= recipe.bites
                        && !(food.bite == recipe.bites && recipe.destroy_on_end),
                    "Edible bite state",
                )?;
            }
            if let Some(m) = &s.mechanism {
                data(finite(&m.degrees) && unit(m.slide), "Generated mechanism")?;
            }
            for (i, c) in s.conditions.iter().enumerate() {
                data(
                    c.state <= 4
                        && c.color.iter().all(|v| v.is_finite())
                        && c.repair_progress.is_finite()
                        && !s.conditions[..i].iter().any(|o| o.source == c.source),
                    "Generated part condition",
                )?;
            }
            if s.held {
                links(
                    held.replace(key).is_none() && s.attachment.is_none(),
                    "Generated held ownership",
                )?;
            }
            if let Some(a) = &s.attachment {
                attachment(a)?;
                if let Some(owner) = &a.owner {
                    edges.insert(key.clone(), owner.clone());
                }
                if let Some(slot) = &a.slot {
                    links(
                        slots.insert((a.owner.clone(), slot.clone())),
                        "Duplicate generated slot owner",
                    )?;
                }
            }
        }
        let ids = IdentityMap::new(reference)?;
        for item in &self.items {
            let key = ObjectKey::Imported(ids.item(&item.instance)?);
            match &item.location {
                ItemLocationSave::GeneratedAttachment { attachment: a } => {
                    attachment(a)?;
                    links(
                        matches!(a.owner, Some(ObjectKey::Generated(_))),
                        "Generated attachment owner",
                    )?;
                    if let Some(owner) = &a.owner {
                        edges.insert(key, owner.clone());
                    }
                    if let Some(slot) = &a.slot {
                        links(
                            slots.insert((a.owner.clone(), slot.clone())),
                            "Duplicate cross-source slot",
                        )?;
                    }
                }
                ItemLocationSave::Attached { owner } => {
                    edges.insert(key, ObjectKey::Imported(ids.item(owner)?));
                }
                _ => (),
            }
        }
        for start in edges.keys() {
            let mut seen = HashSet::new();
            let mut next = Some(start);
            while let Some(key) = next {
                links(seen.insert(key), "World attachment cycle")?;
                next = edges.get(key);
            }
        }
        links(
            held.map(|key| match key {
                ObjectKey::Generated(id) => InstanceId::Generated { id: id.clone() },
                _ => unreachable!(),
            }) == self.player.carry.as_ref().and_then(|c| {
                matches!(c.item, InstanceId::Generated { .. }).then(|| c.item.clone())
            }),
            "Generated carry record",
        )?;
        links(
            self.world.start_car.current_car(reference).ok()
                == Some(self.world.spawn.context.current_car),
            "Selected source spawn car",
        )?;
        for record in &self.attachments.parts {
            if record.part.is_some() {
                let slot_id = ids
                    .part_slots
                    .get(&record.slot)
                    .ok_or_else(|| invalid("Unknown source part slot"))?;
                let slot = reference
                    .part_slots
                    .iter()
                    .find(|s| s.id == *slot_id)
                    .unwrap();
                links(
                    slots.insert((slot.owner.map(ObjectKey::Imported), slot.source.clone())),
                    "Duplicate cross-source part slot",
                )?;
            }
        }
        // The browser starts a new game on the PC date, so any calendar date is valid.
        let context = |c: SpawnContext| -> Result<(), SaveError> {
            let [_, month, day] = c.date;
            data(
                (1..=12).contains(&month)
                    && (1..=31).contains(&day)
                    && c.chance_bits.is_none_or(|v| f32::from_bits(v).is_finite()),
                "Source first-call context",
            )
        };
        context(self.world.spawn.context)?;
        data(
            self.world.spawn.live.words != [0; 4],
            "Source live random state",
        )?;
        let mut boundaries = HashSet::new();
        for b in &self.world.spawn.boundaries {
            generated(&b.parent)?;
            source(&b.caller)?;
            context(b.context)?;
            data(b.random.words != [0; 4], "Source boundary random state")?;
            links(
                boundaries.insert((&b.parent, &b.caller, b.ordinal, b.phase as u8)),
                "Duplicate source first-call boundary",
            )?;
            for p in &b.painting_pools {
                source(&p.source)?;
            }
            let init = &b.initialization;
            for (id, s, _) in &init.paintings {
                generated(id)?;
                source(s)?;
            }
            for (id, s, scale) in &init.scales {
                generated(id)?;
                source(s)?;
                data(finite(scale), "Source initialization scale")?;
            }
            for (id, s, _) in &init.lamps {
                generated(id)?;
                source(s)?;
            }
            for (id, s) in &init.initialized {
                generated(id)?;
                source(s)?;
            }
            for (id, s, _) in &init.selectors {
                generated(id)?;
                source(s)?;
            }
            for (id, s, _) in &init.activations {
                generated(id)?;
                source(s)?;
            }
            for (id, s, part) in &init.slot_parts {
                generated(id)?;
                source(s)?;
                source(part)?;
            }
            for (id, s, speed) in &init.door_speeds {
                generated(id)?;
                source(s)?;
                data(speed.is_finite(), "Source door speed")?;
            }
            for c in &init.conditions {
                generated(&c.parent)?;
                source(&c.source)?;
                data(finite(&c.color), "Source condition color")?;
            }
        }
        for p in &self.world.spawn.painting_pools {
            source(&p.source)?;
        }
        crate::world::SourceSpawnState::restore_history(self.world.spawn.clone())?;
        Ok(())
    }
}
/// A storage lock has an owner and no slot. Its volume is a prototype volume of a generated owner. `prototype` is the generated owner prototype if known.
fn storage_volume(
    storage: &r::StorageRecord,
    a: &AttachmentSnapshot,
    prototype: Option<&r::SourceId>,
) -> Result<(), SaveError> {
    let Some(volume) = &a.volume else {
        return Ok(());
    };
    links(a.slot.is_none(), "Storage lock with a slot")?;
    match a.owner.as_ref() {
        Some(ObjectKey::Imported(_)) => Err(invalid("Storage volume owner")),
        Some(ObjectKey::Generated(_)) => links(
            storage
                .prototype_volumes
                .iter()
                .any(|v| v.source == *volume && prototype.is_none_or(|p| v.prototype == *p)),
            "Storage volume prototype",
        ),
        None => Err(invalid("Storage lock without owner")),
    }
}
fn descendant(child: &GeneratedId, parent: &GeneratedId) -> bool {
    child.layer == parent.layer
        && child.generator == parent.generator
        && child.unit == parent.unit
        && child.ordinal == parent.ordinal
        && child.children.starts_with(&parent.children)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::world::AbsolutePose;

    fn id(path_id: i64) -> r::SourceId {
        r::SourceId {
            file: "world".into(),
            path_id,
        }
    }
    fn storage() -> r::StorageRecord {
        r::StorageRecord {
            policy: r::StoragePolicy {
                settings_source: id(1),
                default_setting: 2,
                mount_mask: 1,
                drop_delay_seconds: 1.0,
                scan_interval_seconds: 1.5,
            },
            prototype_volumes: [10, 20]
                .map(|path_id| r::PrototypeVolumeReference {
                    prototype: id(path_id),
                    source: id(path_id + 1),
                    node: 0,
                    center: [0.; 3],
                    half_extents: [1.; 3],
                    rotation: [0., 0., 0., 1.],
                    storage_type: 0,
                    only_manual: false,
                    usables: Vec::new(),
                    slots: Vec::new(),
                    dont_lock_if_owner_picked_up: false,
                })
                .into(),
            flags: Vec::new(),
            prototype_flags: Vec::new(),
        }
    }
    fn lock(volume: i64, slot: Option<r::SourceId>) -> AttachmentSnapshot {
        AttachmentSnapshot {
            owner: Some(ObjectKey::Generated(GeneratedId {
                layer: crate::world::WorldLayer::Building,
                generator: id(1),
                unit: [0, 0],
                ordinal: 0,
                children: Vec::new(),
            })),
            slot,
            volume: Some(id(volume)),
            local_pose: AbsolutePose {
                position: [0.; 3],
                rotation: [0., 0., 0., 1.],
            },
        }
    }

    #[test]
    fn storage_lock_needs_no_slot_and_a_volume_of_the_owner_prototype() {
        let storage = storage();
        assert!(storage_volume(&storage, &lock(11, None), Some(&id(10))).is_ok());
        assert!(storage_volume(&storage, &lock(21, None), None).is_ok());
        assert!(storage_volume(&storage, &lock(11, Some(id(5))), Some(&id(10))).is_err());
        assert!(storage_volume(&storage, &lock(21, None), Some(&id(10))).is_err());
        assert!(storage_volume(&storage, &lock(30, None), None).is_err());
    }
}
