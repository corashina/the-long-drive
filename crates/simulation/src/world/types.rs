//! Stable world values. These types contain no renderer or physics handles.
use crate::reference::{ItemId, SourceId};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorldLayer {
    Road,
    Terrain,
    DistantTerrain,
    DistantTerrain2,
    Building,
    Prop,
    Mountain,
    Tower,
    DesertBuilding,
    Lamp,
    Nested,
    /// Fixed world prototype placements of the source startup list, such as the start car.
    Startup,
}

/// The fields of a `GeneratedId`.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GeneratedKey {
    pub layer: WorldLayer,
    pub generator: SourceId,
    pub unit: [i64; 2],
    pub ordinal: u32,
    pub children: Vec<(SourceId, u32)>,
}
/// A shared, immutable `GeneratedKey` with its cached seed.
/// Clone is a refcount increment. Hash writes the seed. Eq and Ord compare the fields.
#[derive(Clone, Deserialize)]
#[serde(from = "GeneratedKey")]
pub struct GeneratedId(Arc<Shared>);
struct Shared {
    key: GeneratedKey,
    seed: u64,
}
impl From<GeneratedKey> for GeneratedId {
    fn from(key: GeneratedKey) -> Self {
        let seed = key.seed();
        Self(Arc::new(Shared { key, seed }))
    }
}
impl Serialize for GeneratedId {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        self.0.key.serialize(serializer)
    }
}
impl std::ops::Deref for GeneratedId {
    type Target = GeneratedKey;
    fn deref(&self) -> &GeneratedKey {
        &self.0.key
    }
}
impl std::fmt::Debug for GeneratedId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.key.fmt(f)
    }
}
impl PartialEq for GeneratedId {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0) || (self.0.seed == other.0.seed && self.0.key == other.0.key)
    }
}
impl Eq for GeneratedId {}
impl std::hash::Hash for GeneratedId {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        state.write_u64(self.0.seed);
    }
}
impl PartialOrd for GeneratedId {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}
impl Ord for GeneratedId {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        if Arc::ptr_eq(&self.0, &other.0) {
            return std::cmp::Ordering::Equal;
        }
        self.0.key.cmp(&other.0.key)
    }
}
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(tag = "kind", content = "id", rename_all = "snake_case")]
pub enum ObjectKey {
    Imported(ItemId),
    Generated(GeneratedId),
}
const FNV_OFFSET: u64 = 0xcbf29ce484222325;
fn mix(hash: &mut u64, bytes: &[u8]) {
    for byte in bytes {
        *hash = (*hash ^ u64::from(*byte)).wrapping_mul(0x100000001b3);
    }
}
fn mix_source(hash: &mut u64, id: &SourceId) {
    mix(hash, id.file.as_bytes());
    mix(hash, &id.path_id.to_le_bytes());
}
impl GeneratedId {
    /// `ObjectKey::seed` of this key, cached when the id was built.
    pub fn seed(&self) -> u64 {
        self.0.seed
    }
    /// The seed of this key with one more child: `source` with ordinal `ordinal`.
    pub fn child_seed(&self, source: &SourceId, ordinal: u32) -> u64 {
        let mut hash = self.seed();
        mix_source(&mut hash, source);
        mix(&mut hash, &ordinal.to_le_bytes());
        hash
    }
    /// The seed of this key without its last child. None: the key has no children.
    pub fn parent_seed(&self) -> Option<u64> {
        let children = self.0.key.children.len().checked_sub(1)?;
        Some(self.0.key.prefix_seed(children))
    }
    /// The seeds of each proper prefix of this key, shortest first. Empty: the key has no children.
    pub fn ancestor_seeds(&self) -> impl Iterator<Item = u64> + '_ {
        let key = &self.0.key;
        let parents = key.children.len().saturating_sub(1);
        let root = (!key.children.is_empty()).then(|| key.prefix_seed(0));
        root.into_iter().chain(key.children[..parents].iter().scan(
            root.unwrap_or(FNV_OFFSET),
            |hash, (child, ordinal)| {
                mix_source(hash, child);
                mix(hash, &ordinal.to_le_bytes());
                Some(*hash)
            },
        ))
    }
    /// True when `self` is a proper prefix of `child`: the same root with fewer children.
    pub fn is_ancestor_of(&self, child: &GeneratedId) -> bool {
        let (parent, child) = (&self.0.key, &child.0.key);
        parent.layer == child.layer
            && parent.generator == child.generator
            && parent.unit == child.unit
            && parent.ordinal == child.ordinal
            && child.children.len() > parent.children.len()
            && child.children.starts_with(&parent.children)
    }
    /// This id with one more child: `source` with ordinal `ordinal`.
    pub fn with_child(&self, source: &SourceId, ordinal: u32) -> Self {
        let mut key = self.0.key.clone();
        key.children.push((source.clone(), ordinal));
        key.into()
    }
}
impl GeneratedKey {
    fn seed(&self) -> u64 {
        self.prefix_seed(self.children.len())
    }
    /// The seed of this key with only its first `children` children.
    fn prefix_seed(&self, children: usize) -> u64 {
        let mut hash = FNV_OFFSET;
        mix(&mut hash, &[self.layer as u8]);
        mix_source(&mut hash, &self.generator);
        mix(&mut hash, &self.unit[0].to_le_bytes());
        mix(&mut hash, &self.unit[1].to_le_bytes());
        mix(&mut hash, &self.ordinal.to_le_bytes());
        for (child, ordinal) in &self.children[..children] {
            mix_source(&mut hash, child);
            mix(&mut hash, &ordinal.to_le_bytes());
        }
        hash
    }
}
impl ObjectKey {
    /// Stable 64-bit FNV-1a seed of the key. Each copy of one prototype gets its own seed.
    pub fn seed(&self) -> u64 {
        match self {
            Self::Imported(id) => {
                let mut hash = FNV_OFFSET;
                mix(&mut hash, &id.0.to_le_bytes());
                hash
            }
            Self::Generated(id) => id.seed(),
        }
    }
}
/// Stable seed of a source object, the hash of `ObjectKey::seed`.
pub fn source_seed(id: &SourceId) -> u64 {
    let mut hash = FNV_OFFSET;
    mix_source(&mut hash, id);
    hash
}
/// FNV-1a of `bytes`, the hash of `ObjectKey::seed`.
pub fn fnv1a(bytes: &[u8]) -> u64 {
    let mut hash = FNV_OFFSET;
    mix(&mut hash, bytes);
    hash
}
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AbsolutePose {
    pub position: [f64; 3],
    pub rotation: [f32; 4],
}
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorldBounds {
    pub min: [f64; 3],
    pub max: [f64; 3],
}
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorldSurface {
    pub origin: [f64; 3],
    #[serde(with = "pod_bytes")]
    pub vertices: Vec<[f32; 3]>,
    #[serde(with = "pod_bytes")]
    pub normals: Vec<[f32; 3]>,
    #[serde(with = "pod_bytes")]
    pub tangents: Vec<[f32; 4]>,
    #[serde(with = "pod_bytes")]
    pub uv: Vec<[f32; 2]>,
    /// Each range retains one authored material slot.
    pub submeshes: Vec<std::ops::Range<usize>>,
    #[serde(with = "pod_bytes")]
    pub colors: Vec<[f32; 4]>,
    #[serde(with = "pod_bytes")]
    pub triangles: Vec<[u32; 3]>,
    /// Some: a regular terrain grid tile of this size (`terrain::regular_grid`). It has no
    /// triangles and no UV. `terrain::grid_triangles` and `terrain::grid_uv` rebuild them.
    pub grid: Option<u32>,
}

/// Mesh arrays as one byte string, so the frame codec copies them in one step.
mod pod_bytes {
    use bytemuck::Pod;
    use serde::{Deserializer, Serializer, de};

    pub fn serialize<T: Pod, S: Serializer>(rows: &[T], serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_bytes(bytemuck::cast_slice(rows))
    }

    pub fn deserialize<'de, T: Pod, D: Deserializer<'de>>(
        deserializer: D,
    ) -> Result<Vec<T>, D::Error> {
        struct Rows<T>(std::marker::PhantomData<T>);
        impl<T: Pod> de::Visitor<'_> for Rows<T> {
            type Value = Vec<T>;
            fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
                f.write_str("mesh array bytes")
            }
            fn visit_bytes<E: de::Error>(self, bytes: &[u8]) -> Result<Vec<T>, E> {
                if !bytes.len().is_multiple_of(size_of::<T>()) {
                    return Err(E::invalid_length(bytes.len(), &self));
                }
                let mut rows = vec![T::zeroed(); bytes.len() / size_of::<T>()];
                bytemuck::cast_slice_mut(&mut rows).copy_from_slice(bytes);
                Ok(rows)
            }
        }
        deserializer.deserialize_bytes(Rows(std::marker::PhantomData))
    }
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Placement {
    pub id: GeneratedId,
    pub prototype: SourceId,
    pub pose: AbsolutePose,
    pub scale: [f32; 3],
    pub bounds: WorldBounds,
    pub object_seed: i32,
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::hash::BuildHasher;

    fn key(ordinal: u32) -> GeneratedKey {
        GeneratedKey {
            layer: WorldLayer::Prop,
            generator: SourceId {
                file: "level0".into(),
                path_id: 7,
            },
            unit: [1, -2],
            ordinal,
            children: Vec::new(),
        }
    }

    #[test]
    fn separate_builds_of_one_key_match() {
        let (a, b): (GeneratedId, GeneratedId) = (key(3).into(), key(3).into());
        let state = std::hash::RandomState::new();
        assert_eq!(a, b);
        assert_eq!(state.hash_one(&a), state.hash_one(&b));
        assert_ne!(a, GeneratedId::from(key(4)));
        let source = SourceId {
            file: "level0".into(),
            path_id: 9,
        };
        let mut nested = key(3);
        nested.children.push((source.clone(), 1));
        let child = a.with_child(&source, 1);
        assert_eq!(child, GeneratedId::from(nested));
        assert_eq!(child.seed(), a.child_seed(&source, 1));
        assert_eq!(child.parent_seed(), Some(a.seed()));
        assert_eq!(a.parent_seed(), None);
        let json = serde_json::to_string(&child).unwrap();
        assert_eq!(serde_json::from_str::<GeneratedId>(&json).unwrap(), child);
        let grandchild = child.with_child(&source, 2);
        let seeds: Vec<u64> = grandchild.ancestor_seeds().collect();
        assert_eq!(seeds, [a.seed(), child.seed()]);
        assert!(a.is_ancestor_of(&grandchild) && child.is_ancestor_of(&grandchild));
        assert!(!grandchild.is_ancestor_of(&child) && !a.is_ancestor_of(&a));
        assert_eq!(a.ancestor_seeds().count(), 0);
    }
}
