//! Source coordinate-seeded building children. These do not use Unity random state.
use super::spawn_rules::*;
use super::{AbsolutePose, Placement, SourceRandom, WorldBounds};
use crate::reference::{ReferenceError, SourceId, WorldReference};
use std::collections::HashMap;

/// Selected prototype identities. A new source graph starts with a new stream and memo.
#[derive(Default)]
pub struct BuildingComponents {
    entries: HashMap<SourceId, Option<RecordLocation>>,
    key_bytes: usize,
}
impl BuildingComponents {
    pub fn needs_lookup(&self, placements: &[Placement]) -> bool {
        placements
            .iter()
            .any(|p| p.id.children.is_empty() && !self.entries.contains_key(&p.prototype))
    }
    pub fn retained_bytes(&self) -> usize {
        super::metrics::map_bytes(
            self.entries.capacity(),
            std::mem::size_of::<(SourceId, Option<RecordLocation>)>(),
        ) + self.key_bytes
    }
    /// `None`: the new entry does not fit `room`. Nothing is retained.
    fn component<'a>(
        &mut self,
        reference: &'a WorldReference,
        rules: &'a SourceRules,
        source: &SourceId,
        room: &mut usize,
    ) -> Result<Option<Option<SourceRecord<'a>>>, ReferenceError> {
        if let Some(location) = self.entries.get(source) {
            return Ok(Some(location.map(|location| location.record(reference))));
        }
        // Keep the checked lookup and its errors before metadata admission.
        let record = rules.component(reference, source, "buildingscript")?;
        let growth =
            super::metrics::map_insert_bytes(&self.entries).max(if self.entries.capacity() == 0 {
                // The first std HashMap allocation has capacity three.
                super::metrics::map_bytes(
                    3,
                    std::mem::size_of::<(SourceId, Option<RecordLocation>)>(),
                )
            } else {
                0
            });
        let admission = growth.saturating_add(source.file.capacity());
        if admission > *room {
            return Ok(None);
        }
        let before = self.retained_bytes();
        let key = source.clone();
        self.key_bytes += key.file.capacity();
        self.entries.insert(key, record.map(|r| r.location));
        *room -= self.retained_bytes() - before;
        Ok(Some(record))
    }
}

/// False: a building lookup does not fit `room`. The ramps up to that building are appended.
pub fn append_ramps(
    reference: &WorldReference,
    rules: &SourceRules,
    components: &mut BuildingComponents,
    mut room: usize,
    seed: i32,
    placements: &mut Vec<Placement>,
    maximum: usize,
) -> Result<bool, ReferenceError> {
    append_ramps_with(reference, rules, seed, placements, maximum, |source| {
        components.component(reference, rules, source, &mut room)
    })
}

fn append_ramps_with<'a>(
    reference: &'a WorldReference,
    rules: &'a SourceRules,
    seed: i32,
    placements: &mut Vec<Placement>,
    maximum: usize,
    mut component: impl FnMut(&SourceId) -> Result<Option<Option<SourceRecord<'a>>>, ReferenceError>,
) -> Result<bool, ReferenceError> {
    let parents = placements.len();
    for index in 0..parents {
        let parent = &placements[index];
        if !parent.id.children.is_empty() {
            continue;
        }
        let Some(building) = component(&parent.prototype)? else {
            return Ok(false);
        };
        let Some(building) = building else {
            continue;
        };
        if placements.iter().any(|p| {
            p.id.layer == parent.id.layer
                && p.id.unit == parent.id.unit
                && p.id.ordinal == parent.id.ordinal
                && p.id.children.last().is_some_and(|(source, ordinal)| {
                    source == building.source && *ordinal == u32::MAX
                })
        }) {
            continue;
        }
        let f = building.fields;
        let position = [
            parent.pose.position[0],
            parent.pose.position[1],
            -parent.pose.position[2],
        ];
        // savedatascript.buildingRounding is the checked constant 20.0.
        let local_seed = position.into_iter().fold(seed, |seed, value| {
            seed.wrapping_add(source_i32((value / 20.).round_ties_even() * 20.))
        });
        let mut random = SourceRandom::new(local_seed);
        let chance = random.sample() as f32;
        let target = optional(f, "rampPrefab")?;
        let threshold = number(f, "rampChance")?;
        let Some(target) = target.filter(|_| chance < threshold) else {
            continue;
        };
        let angle = random.sample() as f32 * 360.;
        let radius = number(f, "rampMinRadius")?;
        // The source passes rampMinRadius twice and still consumes this draw.
        let fraction = random.sample() as f32;
        let radius = radius + (radius - radius) * fraction;
        let low = number(f, "rampMinOffsety")?;
        let high = number(f, "rampMaxOffsety")?;
        let height = low + (high - low) * (random.sample() as f32);
        let rotation = angle
            + number(f, "rampOffsetRY")?
            + if random.next_range(0, 2) == 0 {
                -90.
            } else {
                90.
            };
        let root = rules.root(reference, target)?;
        let prototype = reference
            .prototypes
            .iter()
            .find(|p| &p.source == root)
            .ok_or_else(|| ReferenceError("Missing checked source ramp prototype".into()))?;
        let offset = super::placement::rotate(
            super::spawn_building::source_euler([0., angle, 0.]),
            [0., 0., -radius],
        );
        let position = [
            parent.pose.position[0] + f64::from(offset[0]),
            parent.pose.position[1] + f64::from(height),
            parent.pose.position[2] + f64::from(offset[2]),
        ];
        let mut id = parent.id.clone();
        id.children.push((building.source.clone(), u32::MAX));
        if placements.iter().any(|p| p.id == id) {
            continue;
        }
        if placements.len() == maximum {
            return Err(ReferenceError(
                "Source ramp object admission exceeds capacity".into(),
            ));
        }
        let radius = rules
            .radius(root)
            .ok_or_else(|| ReferenceError("Missing source ramp bounds".into()))?;
        placements.push(Placement {
            id,
            prototype: root.clone(),
            pose: AbsolutePose {
                position,
                rotation: super::spawn_building::source_euler([0., rotation, 0.]),
            },
            scale: prototype.prefab_pose.scale,
            bounds: WorldBounds {
                min: position.map(|v| v - radius),
                max: position.map(|v| v + radius),
            },
            object_seed: local_seed,
        });
    }
    Ok(true)
}

fn source_i32(value: f64) -> i32 {
    if !value.is_finite() || value < -2147483648. || value >= 2147483648. {
        i32::MIN
    } else {
        value as i32
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::reference::{
        ReferenceManifest, WorldInitComponent, WorldInitField, WorldInitNode, WorldSourceRecord,
        WorldValue,
    };
    use crate::world::{GeneratedId, WorldLayer};

    fn source(index: i64) -> SourceId {
        SourceId {
            file: "stage-d-fixture".into(),
            path_id: index,
        }
    }
    fn parent(prototype: SourceId, ordinal: u32) -> Placement {
        let position = [f64::from(ordinal) * 24.5, 2.5, -f64::from(ordinal) * 17.25];
        Placement {
            id: GeneratedId {
                layer: WorldLayer::Building,
                generator: source(0),
                unit: [3, 4],
                ordinal,
                children: Vec::new(),
            },
            prototype,
            pose: AbsolutePose {
                position,
                rotation: [0., 0., 0., 1.],
            },
            scale: [1.; 3],
            bounds: WorldBounds {
                min: position.map(|x| x - 2.),
                max: position.map(|x| x + 2.),
            },
            object_seed: 9,
        }
    }
    fn fields(ramp: Option<&SourceId>, chance: f64) -> Vec<WorldInitField> {
        let mut values = vec![
            ("rampChance", WorldValue::Number(chance)),
            (
                "rampPrefab",
                ramp.map_or(WorldValue::Empty, |r| WorldValue::Link(r.clone())),
            ),
        ];
        if chance > 0. && ramp.is_some() {
            values.extend([
                ("rampMinRadius", WorldValue::Number(13.)),
                ("rampMinOffsety", WorldValue::Number(-2.)),
                ("rampMaxOffsety", WorldValue::Number(4.)),
                ("rampOffsetRY", WorldValue::Number(15.)),
            ]);
        } else {
            // These fields must remain unevaluated after absence or failed chance.
            values.push(("rampMinRadius", WorldValue::Empty));
        }
        values.sort_unstable_by_key(|(name, _)| *name);
        values
            .into_iter()
            .map(|(name, value)| WorldInitField {
                name: name.into(),
                value,
            })
            .collect()
    }
    #[test]
    #[ignore = "requires the installed source export"]
    fn selected_building_memo_matches_checked_ramps_and_admission() {
        let checked = ReferenceManifest::from_runtime_json(
            &std::fs::read(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../../web/assets/reference/manifest.json"
            ))
            .unwrap(),
        )
        .unwrap();
        let mut world = std::sync::Arc::try_unwrap(checked.world).unwrap();
        let ramp = world.prototypes[0].source.clone();
        // Keep the checked ramp geometry. Add selected full/compact metadata cases.
        for index in 1..=7 {
            let component = source(100 + index);
            let mut components = if index == 1 {
                Vec::new()
            } else {
                vec![component.clone()]
            };
            if index == 5 {
                components.push(source(999));
            }
            world.initialization.nodes.push(WorldInitNode {
                source: source(index),
                owner: source(200 + index),
                parent: None,
                children: Vec::new(),
                components,
                active: true,
                pose: world.generation_frame,
            });
            if matches!(index, 1 | 6) {
                continue;
            }
            let mut f = fields(
                (index != 2).then_some(&ramp),
                if index == 3 { 0. } else { 1. },
            );
            if index == 7 {
                f.iter_mut()
                    .find(|f| f.name == "rampMinRadius")
                    .unwrap()
                    .value = WorldValue::Empty;
            }
            if index == 4 {
                world.records.push(WorldSourceRecord {
                    source: component,
                    class_name: "buildingscript".into(),
                    fields: serde_json::from_value(serde_json::to_value(f).unwrap()).unwrap(),
                });
            } else {
                world.initialization.components.push(WorldInitComponent {
                    source: component,
                    owner: source(200 + index),
                    class_name: "buildingscript".into(),
                    execution_order: 7,
                    fields: f,
                });
            }
        }
        let mut other_file = world.initialization.nodes.last().unwrap().clone();
        other_file.source = source(1);
        other_file.source.file = "stage-d-other-file".into();
        other_file.components = vec![source(103)];
        let other_source = other_file.source.clone();
        world.initialization.nodes.push(other_file);
        let rules = SourceRules::new(&world);
        let initial: Vec<_> = [1, 2, 3, 4, 5, 4]
            .into_iter()
            .enumerate()
            .map(|(i, p)| parent(source(p), i as u32))
            .collect();
        let mut expected = initial.clone();
        append_ramps_with(&world, &rules, 42, &mut expected, 32, |s| {
            rules.component(&world, s, "buildingscript").map(Some)
        })
        .unwrap();
        assert_eq!(expected.len(), initial.len() + 3);
        let mut actual = initial.clone();
        let mut memo = BuildingComponents::default();
        // A lookup without room stops before its building and keeps nothing.
        let mut waiting = BuildingComponents::default();
        let mut partial = initial.clone();
        assert!(!append_ramps(&world, &rules, &mut waiting, 0, 42, &mut partial, 32).unwrap());
        assert_eq!(partial, initial);
        assert_eq!(waiting.retained_bytes(), 0);
        assert!(append_ramps(&world, &rules, &mut memo, usize::MAX, 42, &mut actual, 32).unwrap());
        assert_eq!(actual, expected);
        assert_eq!(
            serde_json::to_vec(&actual).unwrap(),
            serde_json::to_vec(&expected).unwrap()
        );
        assert_eq!(memo.entries.len(), 5);
        assert!(memo.entries[&source(1)].is_none());
        assert!(memo.entries[&source(2)].is_some());
        assert!(memo.entries[&source(3)].is_some());
        let retained = memo.retained_bytes();
        let capacity = memo.entries.capacity();
        for _ in 0..3 {
            append_ramps_with(&world, &rules, 42, &mut expected, 32, |s| {
                rules.component(&world, s, "buildingscript").map(Some)
            })
            .unwrap();
            assert!(append_ramps(&world, &rules, &mut memo, 0, 42, &mut actual, 32).unwrap());
            assert_eq!(actual, expected);
            assert_eq!(memo.retained_bytes(), retained);
            assert_eq!(memo.entries.capacity(), capacity);
        }
        assert_eq!(
            retained,
            super::super::metrics::map_bytes(
                capacity,
                std::mem::size_of::<(SourceId, Option<RecordLocation>)>()
            ) + memo
                .entries
                .keys()
                .map(|s| s.file.capacity())
                .sum::<usize>()
        );
        // Object capacity fails at the same parent after identical preceding placements.
        let mut expected = initial.clone();
        let mut actual = initial.clone();
        let old = append_ramps_with(&world, &rules, 42, &mut expected, initial.len() + 1, |s| {
            rules.component(&world, s, "buildingscript").map(Some)
        })
        .unwrap_err();
        let new = append_ramps(
            &world,
            &rules,
            &mut memo,
            0,
            42,
            &mut actual,
            initial.len() + 1,
        )
        .unwrap_err();
        assert_eq!(old.0, new.0);
        assert_eq!(actual, expected);
        // A broken compact component errors before metadata admission and is never cached.
        let mut rejected = BuildingComponents::default();
        for _ in 0..2 {
            let mut expected = vec![parent(source(6), 0)];
            let mut actual = expected.clone();
            let old = append_ramps_with(&world, &rules, 42, &mut expected, 32, |s| {
                rules.component(&world, s, "buildingscript").map(Some)
            })
            .unwrap_err();
            let new =
                append_ramps(&world, &rules, &mut rejected, 0, 42, &mut actual, 32).unwrap_err();
            assert_eq!(old.0, new.0);
            assert_eq!(actual, expected);
            assert!(rejected.entries.is_empty());
            assert_eq!(rejected.retained_bytes(), 0);
        }
        // Cache identity uses the complete source, including its file.
        let mut room = usize::MAX;
        assert!(
            memo.component(&world, &rules, &other_source, &mut room)
                .unwrap()
                .unwrap()
                .is_some()
        );
        assert!(memo.entries[&source(1)].is_none());
        // A successful component lookup retains its identity even when a selected field fails.
        for room in [usize::MAX, 0] {
            let mut expected = vec![parent(source(7), 0)];
            let mut actual = expected.clone();
            let old = append_ramps_with(&world, &rules, 42, &mut expected, 32, |s| {
                rules.component(&world, s, "buildingscript").map(Some)
            })
            .unwrap_err();
            let new =
                append_ramps(&world, &rules, &mut memo, room, 42, &mut actual, 32).unwrap_err();
            assert_eq!(old.0, new.0);
            assert_eq!(actual, expected);
            assert!(memo.entries[&source(7)].is_some());
        }
        // Reject both the first allocation and later table growth before retaining anything.
        let first = super::super::metrics::map_bytes(
            3,
            std::mem::size_of::<(SourceId, Option<RecordLocation>)>(),
        ) + source(1).file.capacity();
        let mut room = first - 1;
        assert!(
            rejected
                .component(&world, &rules, &source(1), &mut room)
                .unwrap()
                .is_none()
        );
        assert_eq!(rejected.retained_bytes(), 0);
        assert_eq!(room, first - 1);
        room = first;
        rejected
            .component(&world, &rules, &source(1), &mut room)
            .unwrap();
        assert_eq!(room + rejected.retained_bytes(), first);
        room = usize::MAX;
        for index in [2, 3] {
            rejected
                .component(&world, &rules, &source(index), &mut room)
                .unwrap();
        }
        assert_eq!(rejected.entries.len(), rejected.entries.capacity());
        let retained = rejected.retained_bytes();
        let capacity = rejected.entries.capacity();
        let growth =
            super::super::metrics::map_insert_bytes(&rejected.entries) + source(4).file.capacity();
        room = growth - 1;
        assert!(
            rejected
                .component(&world, &rules, &source(4), &mut room)
                .unwrap()
                .is_none()
        );
        assert_eq!(rejected.retained_bytes(), retained);
        assert_eq!(rejected.entries.capacity(), capacity);
        assert_eq!(room, growth - 1);
        room = growth;
        rejected
            .component(&world, &rules, &source(4), &mut room)
            .unwrap();
        assert_eq!(room + rejected.retained_bytes() - retained, growth);
    }
}
