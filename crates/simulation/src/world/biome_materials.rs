//! Source `materialChangeScript`: a renderer under the script takes the biome material of its set.
use crate::reference::{SourceId, WorldBiomeMaterialSet, WorldPrototypeReference};

/// Desert material to winter and grass materials, sorted by desert material.
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

#[cfg(test)]
mod tests {
    use super::*;
    fn id(path_id: i64) -> SourceId {
        SourceId {
            file: "level1".into(),
            path_id,
        }
    }
    #[test]
    fn variant_picks_biome_and_keeps_desert() {
        let table = BiomeMaterials::new(&[
            WorldBiomeMaterialSet {
                desert: id(20),
                variants: [None, Some(id(22))],
            },
            WorldBiomeMaterialSet {
                desert: id(10),
                variants: [Some(id(11)), Some(id(12))],
            },
        ]);
        assert_eq!(table.variant(&id(10), 1), Some(&id(11)));
        assert_eq!(table.variant(&id(10), 2), Some(&id(12)));
        assert_eq!(table.variant(&id(10), 0), None);
        assert_eq!(table.variant(&id(20), 1), None);
        assert_eq!(table.variant(&id(20), 2), Some(&id(22)));
        assert_eq!(table.variant(&id(30), 1), None);
    }
    #[test]
    fn marked_node_holds_its_descendants() {
        use crate::reference::*;
        let transform = Transform {
            translation: [0.; 3],
            rotation: [0., 0., 0., 1.],
            scale: [1.; 3],
        };
        let node = |parent, biome_materials| WorldPrototypeNode {
            source: id(1),
            name: String::new(),
            parent,
            active: true,
            renderer_enabled: true,
            layer: 0,
            transform: transform.clone(),
            gltf_node: 0,
            biome_materials,
        };
        let prototype = WorldPrototypeReference {
            source: id(1),
            role: WorldPrototypeRole::Prop,
            asset: AssetId(0),
            nodes: vec![
                node(None, false),
                node(Some(0), true),
                node(Some(1), false),
                node(Some(0), false),
            ],
            controls: Vec::new(),
            excluded_actions: Vec::new(),
            deferred_materials: Vec::new(),
            skins: Vec::new(),
            bodies: Vec::new(),
            colliders: Vec::new(),
            prefab_pose: transform.clone(),
            interiors: Vec::new(),
            vehicle: None,
            engines: Vec::new(),
            radiators: Vec::new(),
            gauges: Vec::new(),
            sled: None,
            trailer: None,
            followers: None,
            hitches: Vec::new(),
            help_texts: Vec::new(),
            pushes: Vec::new(),
        };
        let marked: Vec<_> = (0..4).map(|n| under_mark(&prototype, n)).collect();
        assert_eq!(marked, [false, true, true, false]);
    }
}
