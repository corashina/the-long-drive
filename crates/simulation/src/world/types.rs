//! Stable world values. These types contain no renderer or physics handles.
use crate::reference::{ItemId, SourceId};
use serde::{Deserialize, Serialize};

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

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GeneratedId {
    pub layer: WorldLayer,
    pub generator: SourceId,
    pub unit: [i64; 2],
    pub ordinal: u32,
    pub children: Vec<(SourceId, u32)>,
}
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(tag = "kind", content = "id", rename_all = "snake_case")]
pub enum ObjectKey {
    Imported(ItemId),
    Generated(GeneratedId),
}
impl ObjectKey {
    /// Stable 64-bit FNV-1a seed of the key. Each copy of one prototype gets its own seed.
    pub fn seed(&self) -> u64 {
        fn mix(hash: &mut u64, bytes: &[u8]) {
            for byte in bytes {
                *hash = (*hash ^ u64::from(*byte)).wrapping_mul(0x100000001b3);
            }
        }
        fn source(hash: &mut u64, id: &SourceId) {
            mix(hash, id.file.as_bytes());
            mix(hash, &id.path_id.to_le_bytes());
        }
        let mut hash = 0xcbf29ce484222325;
        match self {
            Self::Imported(id) => mix(&mut hash, &id.0.to_le_bytes()),
            Self::Generated(id) => {
                mix(&mut hash, &[id.layer as u8]);
                source(&mut hash, &id.generator);
                mix(&mut hash, &id.unit[0].to_le_bytes());
                mix(&mut hash, &id.unit[1].to_le_bytes());
                mix(&mut hash, &id.ordinal.to_le_bytes());
                for (child, ordinal) in &id.children {
                    source(&mut hash, child);
                    mix(&mut hash, &ordinal.to_le_bytes());
                }
            }
        }
        hash
    }
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
                if bytes.len() % size_of::<T>() != 0 {
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
mod surface_codec_tests {
    use super::WorldSurface;

    #[test]
    fn surface_arrays_encode_as_raw_bytes() {
        let surface = WorldSurface {
            origin: [1., 2., 3.],
            vertices: vec![[0.5, -1., 2.]; 100],
            normals: vec![[0., 1., 0.]; 100],
            triangles: vec![[0, 1, 2]; 50],
            submeshes: vec![0..50],
            ..Default::default()
        };
        let bytes = postcard::to_stdvec(&surface).unwrap();
        // Arrays cost their raw size plus a short length prefix each.
        assert!(bytes.len() < 100 * 12 * 2 + 50 * 12 + 64);
        let back: WorldSurface = postcard::from_bytes(&bytes).unwrap();
        assert_eq!(back.vertices, surface.vertices);
        assert_eq!(back.normals, surface.normals);
        assert_eq!(back.triangles, surface.triangles);
        assert_eq!(back.submeshes, surface.submeshes);
        assert!(postcard::from_bytes::<WorldSurface>(&bytes[..bytes.len() - 1]).is_err());
    }
}
