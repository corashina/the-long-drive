//! Checked little-endian inputs. Validate the complete prototype before allocating it.
use super::{RoadNode, RoadPose, RoadPrototype, RoadSkin};

fn count(out: &mut Vec<u8>, len: usize) {
    out.extend(
        u32::try_from(len)
            .expect("admitted road input length")
            .to_le_bytes(),
    );
}
fn floats<const N: usize>(out: &mut Vec<u8>, values: &[[f32; N]]) {
    count(out, values.len());
    for value in values.iter().flatten() {
        out.extend(value.to_le_bytes());
    }
}
fn indices<const N: usize>(out: &mut Vec<u8>, values: &[[u32; N]]) {
    count(out, values.len());
    for value in values.iter().flatten() {
        out.extend(value.to_le_bytes());
    }
}
pub fn encode_prototype(input: &RoadPrototype, out: &mut Vec<u8>) {
    out.clear();
    count(out, input.nodes.len());
    for node in &input.nodes {
        out.extend(node.parent.unwrap_or(u32::MAX).to_le_bytes());
        for value in node
            .translation
            .iter()
            .chain(&node.rotation)
            .chain(&node.scale)
        {
            out.extend(value.to_le_bytes());
        }
    }
    count(out, input.controls.len());
    for control in &input.controls {
        out.extend(control.to_le_bytes());
    }
    count(out, input.skins.len());
    for skin in &input.skins {
        count(out, skin.bones.len());
        for bone in &skin.bones {
            out.extend(bone.to_le_bytes());
        }
        floats(out, &skin.bind_poses);
        floats(out, &skin.positions);
        floats(out, &skin.normals);
        floats(out, &skin.tangents);
        floats(out, &skin.uv);
        indices(out, &skin.joints);
        floats(out, &skin.weights);
        count(out, skin.triangles.len());
        for group in &skin.triangles {
            indices(out, group);
        }
    }
}
pub fn encode_pose(input: &RoadPose, out: &mut Vec<u8>) {
    out.clear();
    for value in input.origin {
        out.extend(value.to_le_bytes());
    }
    for value in input.rotations.as_flattened() {
        out.extend(value.to_le_bytes());
    }
}

#[derive(Clone, Copy)]
struct Reader<'a>(&'a [u8]);
impl<'a> Reader<'a> {
    fn bytes<const N: usize>(&mut self) -> Result<[u8; N], String> {
        let (head, tail) = self
            .0
            .split_first_chunk::<N>()
            .ok_or("Road input is truncated")?;
        self.0 = tail;
        Ok(*head)
    }
    fn u32(&mut self) -> Result<u32, String> {
        self.bytes().map(u32::from_le_bytes)
    }
    fn f32(&mut self) -> Result<f32, String> {
        let value = f32::from_le_bytes(self.bytes()?);
        if !value.is_finite() {
            return Err("Road input contains a nonfinite value".into());
        }
        Ok(value)
    }
    fn f32s<const N: usize>(&mut self) -> Result<[f32; N], String> {
        let mut values = [0.0; N];
        for value in &mut values {
            *value = self.f32()?;
        }
        Ok(values)
    }
    fn count(&mut self, item: usize) -> Result<usize, String> {
        let count = self.u32()? as usize;
        if count
            .checked_mul(item)
            .is_none_or(|bytes| bytes > self.0.len())
        {
            return Err("Road count exceeds its input".into());
        }
        Ok(count)
    }
    fn block(&mut self, width: usize) -> Result<&'a [u8], String> {
        let count = self.count(width)?;
        let (head, tail) = self.0.split_at(count * width);
        self.0 = tail;
        Ok(head)
    }
    fn done(self) -> Result<(), String> {
        if !self.0.is_empty() {
            return Err("Road input has trailing bytes".into());
        }
        Ok(())
    }
}
fn valid(condition: bool) -> Result<(), String> {
    if !condition {
        return Err("Invalid road prototype counts or indices".into());
    }
    Ok(())
}
fn u32_values(bytes: &[u8]) -> impl Iterator<Item = u32> + '_ {
    bytes
        .chunks_exact(4)
        .map(|v| u32::from_le_bytes(v.try_into().unwrap()))
}
fn f32_values(bytes: &[u8]) -> impl Iterator<Item = f32> + '_ {
    bytes
        .chunks_exact(4)
        .map(|v| f32::from_le_bytes(v.try_into().unwrap()))
}
fn float_block<'a>(reader: &mut Reader<'a>, width: usize) -> Result<&'a [u8], String> {
    let bytes = reader.block(width)?;
    valid(f32_values(bytes).all(f32::is_finite))?;
    Ok(bytes)
}
fn validate_prototype(input: &[u8]) -> Result<(), String> {
    let mut reader = Reader(input);
    let nodes = reader.count(44)?;
    valid(nodes > 0)?;
    for index in 0..nodes {
        let parent = reader.u32()?;
        valid(if index == 0 {
            parent == u32::MAX
        } else {
            (parent as usize) < index
        })?;
        let _translation = reader.f32s::<3>()?;
        let rotation = reader.f32s::<4>()?;
        let scale = reader.f32s::<3>()?;
        valid(
            (rotation.iter().map(|v| v * v).sum::<f32>() - 1.0).abs() < 0.002
                && scale.iter().all(|v| *v != 0.0),
        )?;
    }
    let controls = reader.block(4)?;
    valid(controls.len() == 10 * 4)?;
    for (index, control) in u32_values(controls).enumerate() {
        valid(
            (control as usize) < nodes && !u32_values(&controls[..index * 4]).any(|v| v == control),
        )?;
    }
    let skins = reader.count(36)?;
    valid(skins > 0)?;
    let mut total_vertices = 0usize;
    let mut total_triangles = 0usize;
    for _ in 0..skins {
        let bones = reader.block(4)?;
        valid(!bones.is_empty() && u32_values(bones).all(|v| (v as usize) < nodes))?;
        let binds = float_block(&mut reader, 64)?;
        valid(binds.len() / 64 == bones.len() / 4)?;
        let positions = float_block(&mut reader, 12)?;
        let vertices = positions.len() / 12;
        total_vertices = total_vertices
            .checked_add(vertices)
            .ok_or("Road vertex count overflow")?;
        valid(vertices > 0 && total_vertices <= u32::MAX as usize)?;
        valid(float_block(&mut reader, 12)?.len() / 12 == vertices)?;
        let tangents = float_block(&mut reader, 16)?;
        valid(tangents.is_empty() || tangents.len() / 16 == vertices)?;
        valid(float_block(&mut reader, 8)?.len() / 8 == vertices)?;
        let joints = reader.block(16)?;
        valid(
            joints.len() / 16 == vertices
                && u32_values(joints).all(|v| (v as usize) < bones.len() / 4),
        )?;
        let weights = float_block(&mut reader, 16)?;
        valid(weights.len() / 16 == vertices)?;
        for weights in weights.chunks_exact(16) {
            valid(
                f32_values(weights).all(|v| (0.0..=1.0).contains(&v))
                    && (f32_values(weights).sum::<f32>() - 1.0).abs() < 0.0001,
            )?;
        }
        let groups = reader.count(4)?;
        valid(groups > 0)?;
        for _ in 0..groups {
            let triangles = reader.block(12)?;
            total_triangles = total_triangles
                .checked_add(triangles.len() / 12)
                .ok_or("Road triangle count overflow")?;
            valid(total_triangles <= u32::MAX as usize)?;
            valid(!triangles.is_empty() && u32_values(triangles).all(|v| (v as usize) < vertices))?;
        }
    }
    reader.done()
}
fn read_floats<const N: usize>(reader: &mut Reader<'_>) -> Result<Vec<[f32; N]>, String> {
    let bytes = reader.block(N * 4)?;
    Ok(bytes
        .chunks_exact(N * 4)
        .map(|bytes| {
            std::array::from_fn(|i| f32::from_le_bytes(bytes[i * 4..i * 4 + 4].try_into().unwrap()))
        })
        .collect())
}
fn read_indices<const N: usize>(reader: &mut Reader<'_>) -> Result<Vec<[u32; N]>, String> {
    let bytes = reader.block(N * 4)?;
    Ok(bytes
        .chunks_exact(N * 4)
        .map(|bytes| {
            std::array::from_fn(|i| u32::from_le_bytes(bytes[i * 4..i * 4 + 4].try_into().unwrap()))
        })
        .collect())
}
pub fn decode_prototype(input: &[u8]) -> Result<RoadPrototype, String> {
    validate_prototype(input)?;
    let mut reader = Reader(input);
    let node_count = reader.count(44)?;
    let mut nodes = Vec::with_capacity(node_count);
    for _ in 0..node_count {
        let parent = reader.u32()?;
        nodes.push(RoadNode {
            parent: (parent != u32::MAX).then_some(parent),
            translation: reader.f32s()?,
            rotation: reader.f32s()?,
            scale: reader.f32s()?,
        });
    }
    let controls = u32_values(reader.block(4)?).collect();
    let skin_count = reader.count(36)?;
    let mut skins = Vec::with_capacity(skin_count);
    for _ in 0..skin_count {
        skins.push(RoadSkin {
            bones: u32_values(reader.block(4)?).collect(),
            bind_poses: read_floats(&mut reader)?,
            positions: read_floats(&mut reader)?,
            normals: read_floats(&mut reader)?,
            tangents: read_floats(&mut reader)?,
            uv: read_floats(&mut reader)?,
            joints: read_indices(&mut reader)?,
            weights: read_floats(&mut reader)?,
            triangles: {
                let count = reader.count(4)?;
                let mut groups = Vec::with_capacity(count);
                for _ in 0..count {
                    groups.push(read_indices(&mut reader)?);
                }
                groups
            },
        });
    }
    reader.done()?;
    Ok(RoadPrototype {
        nodes,
        controls,
        skins,
    })
}
pub fn decode_pose(input: &[u8]) -> Result<RoadPose, String> {
    let mut reader = Reader(input);
    let mut origin = [0.0; 3];
    for value in &mut origin {
        *value = f64::from_le_bytes(reader.bytes()?);
        valid(value.is_finite())?;
    }
    let mut rotations = [[0.0; 3]; 10];
    for rotation in &mut rotations {
        *rotation = reader.f32s()?;
        valid(rotation.iter().all(|v| v.abs() <= 90.0))?;
    }
    reader.done()?;
    Ok(RoadPose { origin, rotations })
}

/// Encoded bytes, excluding transport ownership.
pub fn prototype_encoded_len(input: &RoadPrototype) -> usize {
    12 + input.nodes.len() * 44
        + input.controls.len() * 4
        + input
            .skins
            .iter()
            .map(|s| {
                36 + s.bones.len() * 4
                    + s.bind_poses.len() * 64
                    + s.positions.len() * 12
                    + s.normals.len() * 12
                    + s.tangents.len() * 16
                    + s.uv.len() * 8
                    + s.joints.len() * 16
                    + s.weights.len() * 16
                    + s.triangles.iter().map(|g| 4 + g.len() * 12).sum::<usize>()
            })
            .sum::<usize>()
}
