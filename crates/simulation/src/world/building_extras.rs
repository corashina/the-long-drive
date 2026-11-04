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

/// False: a building lookup does not fit `room`. It appends the ramps up to that building.
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
        // The source lerps rampMinRadius to itself, so this draw only advances the sequence.
        random.sample();
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
        let id = parent.id.with_child(&building.source, u32::MAX);
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
