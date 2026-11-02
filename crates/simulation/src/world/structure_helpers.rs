//! Cached source terrain helper points for active off-road structures.
use super::spawn_rules::*;
use super::{Placement, TerrainFoundation, WorldLayer};
use crate::reference::{ReferenceError, SourceId, WorldReference};
pub struct StructureHelpers(Vec<(SourceId, Vec<TerrainFoundation>)>);
impl StructureHelpers {
    pub fn new(reference: &WorldReference, rules: &SourceRules) -> Result<Self, ReferenceError> {
        let mut groups = Vec::new();
        for prototype in &reference.prototypes {
            let mut helpers = Vec::new();
            for (index, node) in prototype.nodes.iter().enumerate() {
                if !super::placement::active(prototype, index) {
                    continue;
                }
                let transform = rules.get(reference, &node.source)?;
                let owner = optional(transform.fields, "m_GameObject")
                    .ok()
                    .flatten()
                    .ok_or_else(|| ReferenceError("Missing source helper GameObject".into()))?;
                let object = rules
                    .get(reference, owner)
                    .ok()
                    .filter(|r| matches!(r.fields, Fields::Record(_)))
                    .ok_or_else(|| ReferenceError("Missing source helper GameObject".into()))?;
                let components = match object.fields.get("m_Component") {
                    Some(crate::reference::WorldValue::Array(values)) => values.as_slice(),
                    _ => &[],
                };
                for source in components.iter().filter_map(|value| match value {
                    crate::reference::WorldValue::Record(fields) => match fields.get("component") {
                        Some(crate::reference::WorldValue::Link(source)) => Some(source),
                        _ => None,
                    },
                    _ => None,
                }) {
                    let record = rules.get(reference, source)?;
                    if record.class_name != "terrainHeightAlignToBuildingScript" {
                        continue;
                    }
                    let coords = array(record.fields, "helpPosCoords")?;
                    let ranges = array(record.fields, "range2")?;
                    for (i, coord) in coords.iter().enumerate() {
                        let target = value_link(coord)?;
                        let index = prototype
                            .nodes
                            .iter()
                            .position(|n| &n.source == target)
                            .ok_or_else(|| {
                                ReferenceError("Source terrain helper outside prototype".into())
                            })?;
                        let scale = prototype.nodes[index].transform.scale[0];
                        let factor = if ranges.len() < coords.len() {
                            0.
                        } else {
                            match &ranges[i] {
                                crate::reference::WorldValue::Number(v) => *v as f32,
                                crate::reference::WorldValue::Integer(v) => *v as f32,
                                _ => return Err(ReferenceError("Source helper range type".into())),
                            }
                        };
                        let factor = if factor == 0. { 4. } else { factor.max(0.) };
                        let inner = (f64::from(scale) * 0.5) as f32;
                        let outer = (f64::from(scale) * f64::from(factor) * 0.5) as f32;
                        helpers.push(TerrainFoundation {
                            position: super::placement::node_point(prototype, index, [0.; 3]),
                            range_squared: [
                                f64::from(inner).powi(2) as f32,
                                f64::from(outer).powi(2) as f32,
                            ],
                        });
                    }
                }
            }
            if !helpers.is_empty() {
                groups.push((prototype.source.clone(), helpers));
            }
        }
        Ok(Self(groups))
    }
    fn selected(placement: &Placement) -> bool {
        matches!(
            placement.id.layer,
            WorldLayer::Prop
                | WorldLayer::Mountain
                | WorldLayer::Tower
                | WorldLayer::DesertBuilding
        ) && placement.id.children.is_empty()
    }
    pub fn count(&self, placements: &[Placement]) -> usize {
        placements
            .iter()
            .filter(|p| Self::selected(p))
            .map(|p| {
                self.0
                    .iter()
                    .find(|(source, _)| source == &p.prototype)
                    .map_or(0, |(_, helpers)| helpers.len())
            })
            .sum()
    }
    pub fn append(&self, placements: &[Placement], output: &mut Vec<TerrainFoundation>) {
        output.reserve_exact(self.count(placements));
        for placement in placements.iter().filter(|p| Self::selected(p)) {
            let Some((_, helpers)) = self
                .0
                .iter()
                .find(|(source, _)| source == &placement.prototype)
            else {
                continue;
            };
            for helper in helpers {
                let local = std::array::from_fn(|i| helper.position[i] * placement.scale[i]);
                let offset = super::placement::rotate(placement.pose.rotation, local);
                let p = std::array::from_fn::<_, 3, _>(|i| {
                    (placement.pose.position[i] + f64::from(offset[i])) as f32
                });
                output.push(TerrainFoundation {
                    position: [p[0], p[1], -p[2]],
                    range_squared: helper.range_squared,
                });
            }
        }
    }
    pub fn retained_bytes(&self) -> usize {
        self.0.capacity() * std::mem::size_of::<(SourceId, Vec<TerrainFoundation>)>()
            + self
                .0
                .iter()
                .map(|(source, rows)| {
                    source.file.capacity()
                        + rows.capacity() * std::mem::size_of::<TerrainFoundation>()
                })
                .sum::<usize>()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    impl StructureHelpers {
        fn legacy_walk(
            reference: &WorldReference,
            rules: &SourceRules,
        ) -> Result<Self, ReferenceError> {
            let mut groups = Vec::new();
            for prototype in &reference.prototypes {
                let mut helpers = Vec::new();
                for (index, node) in prototype.nodes.iter().enumerate() {
                    if !crate::world::placement::active(prototype, index) {
                        continue;
                    }
                    let transform = rules.get(reference, &node.source)?;
                    let object = reference
                        .records
                        .iter()
                        .find(|r| {
                            Some(&r.source)
                                == optional(transform.fields, "m_GameObject").ok().flatten()
                        })
                        .ok_or_else(|| ReferenceError("Missing source helper GameObject".into()))?;
                    for source in object.components() {
                        let record = rules.get(reference, source)?;
                        if record.class_name != "terrainHeightAlignToBuildingScript" {
                            continue;
                        }
                        let coords = array(record.fields, "helpPosCoords")?;
                        let ranges = array(record.fields, "range2")?;
                        for (i, coord) in coords.iter().enumerate() {
                            let target = value_link(coord)?;
                            let index = prototype
                                .nodes
                                .iter()
                                .position(|n| &n.source == target)
                                .ok_or_else(|| {
                                    ReferenceError("Source terrain helper outside prototype".into())
                                })?;
                            let scale = prototype.nodes[index].transform.scale[0];
                            let factor = if ranges.len() < coords.len() {
                                0.
                            } else {
                                match &ranges[i] {
                                    crate::reference::WorldValue::Number(v) => *v as f32,
                                    crate::reference::WorldValue::Integer(v) => *v as f32,
                                    _ => {
                                        return Err(ReferenceError(
                                            "Source helper range type".into(),
                                        ));
                                    }
                                }
                            };
                            let factor = if factor == 0. { 4. } else { factor.max(0.) };
                            let inner = (f64::from(scale) * 0.5) as f32;
                            let outer = (f64::from(scale) * f64::from(factor) * 0.5) as f32;
                            helpers.push(TerrainFoundation {
                                position: crate::world::placement::node_point(
                                    prototype, index, [0.; 3],
                                ),
                                range_squared: [
                                    f64::from(inner).powi(2) as f32,
                                    f64::from(outer).powi(2) as f32,
                                ],
                            });
                        }
                    }
                }
                if !helpers.is_empty() {
                    groups.push((prototype.source.clone(), helpers));
                }
            }
            Ok(Self(groups))
        }
    }

    #[test]
    #[ignore = "requires the installed source export"]
    fn indexed_helpers_match_complete_ordered_legacy_walk() {
        let bytes = std::fs::read(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../web/assets/reference/manifest.json"
        ))
        .unwrap();
        #[derive(serde::Deserialize)]
        struct Manifest {
            world: WorldReference,
        }
        let reference: Manifest = serde_json::from_slice(&bytes).unwrap();
        let reference = &reference.world;
        let rules = SourceRules::new(reference);
        let start = std::time::Instant::now();
        let old = StructureHelpers::legacy_walk(reference, &rules).unwrap();
        let old_time = start.elapsed();
        let start = std::time::Instant::now();
        let new = StructureHelpers::new(reference, &rules).unwrap();
        let new_time = start.elapsed();
        assert_eq!(old.0.len(), new.0.len());
        let mut count = 0;
        for ((old_id, old), (new_id, new)) in old.0.iter().zip(&new.0) {
            assert_eq!(old_id, new_id);
            assert_eq!(old.len(), new.len());
            for (old, new) in old.iter().zip(new) {
                assert_eq!(
                    old.position.map(f32::to_bits),
                    new.position.map(f32::to_bits)
                );
                assert_eq!(
                    old.range_squared.map(f32::to_bits),
                    new.range_squared.map(f32::to_bits)
                );
                count += 1;
            }
        }
        println!(
            "helper constructor only: legacy={old_time:?} indexed={new_time:?}; groups={} helpers={count}",
            new.0.len()
        );
    }
}
