//! Base64 of little-endian triples. Large collider point lists use it in JSON.
//! The binary form (postcard) holds the bytes.
use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use serde::{Deserialize, Deserializer, Serializer, de};

fn encode<T: bytemuck::Pod, S: Serializer>(
    rows: &[[T; 3]],
    serializer: S,
) -> Result<S::Ok, S::Error> {
    // Every supported target is little-endian, as the WASM target.
    let bytes = bytemuck::cast_slice::<[T; 3], u8>(rows);
    if !serializer.is_human_readable() {
        return serializer.serialize_bytes(bytes);
    }
    serializer.serialize_str(&STANDARD.encode(bytes))
}
fn decode<'de, T: bytemuck::Pod, D: Deserializer<'de>>(
    deserializer: D,
) -> Result<Vec<[T; 3]>, D::Error> {
    let text;
    let bytes = if deserializer.is_human_readable() {
        text = STANDARD
            .decode(String::deserialize(deserializer)?)
            .map_err(de::Error::custom)?;
        text.as_slice()
    } else {
        <&[u8]>::deserialize(deserializer)?
    };
    let (rows, rest) = bytes.as_chunks::<12>();
    if !rest.is_empty() {
        return Err(de::Error::custom("Packed triples have a partial row"));
    }
    Ok(rows
        .iter()
        .map(|row| std::array::from_fn(|i| bytemuck::pod_read_unaligned(&row[4 * i..4 * i + 4])))
        .collect())
}

pub mod points {
    use super::*;
    pub fn serialize<S: Serializer>(rows: &[[f32; 3]], serializer: S) -> Result<S::Ok, S::Error> {
        encode(rows, serializer)
    }
    pub fn deserialize<'de, D: Deserializer<'de>>(
        deserializer: D,
    ) -> Result<Vec<[f32; 3]>, D::Error> {
        decode(deserializer)
    }
}
pub mod triangles {
    use super::*;
    pub fn serialize<S: Serializer>(rows: &[[u32; 3]], serializer: S) -> Result<S::Ok, S::Error> {
        encode(rows, serializer)
    }
    pub fn deserialize<'de, D: Deserializer<'de>>(
        deserializer: D,
    ) -> Result<Vec<[u32; 3]>, D::Error> {
        decode(deserializer)
    }
}

#[cfg(test)]
mod tests {
    use crate::reference::ColliderGeometry;

    #[test]
    fn trimesh_round_trips_as_base64() {
        // Python: base64 of struct.pack('<6f', 1, 2, 3, -0.5, 0, 4) and struct.pack('<3I', 0, 1, 2).
        let json = r#"{"kind":"trimesh","points":"AACAPwAAAEAAAEBAAAAAvwAAAAAAAIBA","triangles":"AAAAAAEAAAACAAAA"}"#;
        let g: ColliderGeometry = serde_json::from_str(json).unwrap();
        let ColliderGeometry::Trimesh { points, triangles } = &g else {
            panic!()
        };
        assert_eq!(points, &[[1., 2., 3.], [-0.5, 0., 4.]]);
        assert_eq!(triangles, &[[0, 1, 2]]);
        assert_eq!(serde_json::to_string(&g).unwrap(), json);
        assert!(
            serde_json::from_str::<ColliderGeometry>(r#"{"kind":"convex","points":"AACAPw=="}"#)
                .is_err()
        );
    }
}
