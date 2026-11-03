//! Source `materialChangeScript`. A renderer under the script takes the biome material of its set.
use crate::reference::{SourceId, WorldBiomeMaterialSet, WorldPrototypeReference};

/// Maps each desert material to its winter and grass materials, sorted by desert material.
pub struct BiomeMaterials(Vec<(SourceId, [Option<SourceId>; 2])>);
impl BiomeMaterials {
    pub fn new(sets: &[WorldBiomeMaterialSet]) -> Self {
        let mut table: Vec<_> = sets
            .iter()
            .map(|s| (s.desert.clone(), s.variants.clone()))
            .collect();
        table.sort_unstable_by(|a, b| a.0.cmp(&b.0));
        Self(table)
    }
    /// Biome material for `material`. Desert (0), a null entry and an unknown material give `None`.
    pub fn variant(&self, material: &SourceId, biome: u8) -> Option<&SourceId> {
        let slot = (biome as usize).checked_sub(1)?;
        let i = self.0.binary_search_by(|e| e.0.cmp(material)).ok()?;
        self.0[i].1.get(slot)?.as_ref()
    }
}

/// The node is a marked node or under one. Activity is a separate check.
pub fn under_mark(prototype: &WorldPrototypeReference, mut node: usize) -> bool {
    for _ in 0..prototype.nodes.len() {
        let n = &prototype.nodes[node];
        if n.biome_materials {
            return true;
        }
        let Some(parent) = n.parent else {
            return false;
        };
        node = parent as usize;
    }
    false
}
