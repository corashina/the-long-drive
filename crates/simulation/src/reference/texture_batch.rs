//! Checked row and mip descriptors. Raw bytes use the checked source asset reader.
use super::{FileCatalog, FileReceipt, LightingTextureRecord, ReferenceError, SourceId, require};
use serde::{Deserialize, Serialize};

pub const MAX_BATCH_BYTES: usize = 1024 * 1024;
pub const MAX_BATCH_ENTRIES: usize = 32;
pub const MAX_DESCRIPTOR_BYTES: usize = 64 * 1024;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TextureBatchReference {
    pub descriptor: FileReceipt,
    pub decoded_bytes: usize,
    pub largest_raw_input_bytes: usize,
    pub region_count: usize,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TextureBatchRegion {
    pub input: FileReceipt,
    pub mip: u32,
    pub row: u32,
    pub width: u32,
    pub rows: u32,
    pub texture_offset: usize,
    pub decoded_offset: usize,
    pub decoded_bytes: usize,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TextureBatchDescriptor {
    pub source: SourceId,
    pub width: u32,
    pub height: u32,
    pub mip_count: u32,
    pub decoded_bytes: usize,
    pub regions: Vec<TextureBatchRegion>,
}

fn invalid() -> ReferenceError {
    ReferenceError("Texture batch coverage or size".into())
}
fn pixels(width: u32, rows: u32) -> Result<usize, ReferenceError> {
    (width as usize)
        .checked_mul(rows as usize)
        .and_then(|n| n.checked_mul(4))
        .ok_or_else(invalid)
}
impl TextureBatchDescriptor {
    pub fn parse(input: &[u8], decoded_bytes: usize) -> Result<Self, ReferenceError> {
        require(
            !input.is_empty() && input.len() <= MAX_DESCRIPTOR_BYTES,
            "texture batch descriptor bound",
        )?;
        let descriptor: Self = serde_json::from_slice(input)
            .map_err(|e| ReferenceError(format!("Texture batch JSON: {e}")))?;
        descriptor.validate()?;
        require(
            descriptor.decoded_bytes == decoded_bytes,
            "texture batch admitted payload",
        )?;
        Ok(descriptor)
    }
    pub fn validate(&self) -> Result<(), ReferenceError> {
        require(
            self.width > 0
                && self.height > 0
                && self.mip_count > 0
                && self.mip_count <= 32 - self.width.max(self.height).leading_zeros()
                && !self.regions.is_empty()
                && self.regions.len() <= MAX_BATCH_ENTRIES
                && self.decoded_bytes > 0
                && self.decoded_bytes <= MAX_BATCH_BYTES,
            "texture batch dimensions and bounds",
        )?;
        let mut decoded = 0usize;
        let mut next_texture = None;
        let mut files = std::collections::HashSet::new();
        for region in &self.regions {
            region.input.validate()?;
            require(
                region.input.file.starts_with("reference/")
                    && region.input.file.ends_with(".smip")
                    && files.insert(&region.input.file)
                    && region.mip < self.mip_count,
                "texture batch input identity",
            )?;
            let width = (self.width >> region.mip).max(1);
            let height = (self.height >> region.mip).max(1);
            require(
                region.width == width
                    && region.row < height
                    && region.rows > 0
                    && region.rows <= height - region.row,
                "texture batch region dimensions",
            )?;
            let size = pixels(width, region.rows)?;
            let mut offset = pixels(width, region.row)?;
            for mip in 0..region.mip {
                offset = offset
                    .checked_add(pixels(
                        (self.width >> mip).max(1),
                        (self.height >> mip).max(1),
                    )?)
                    .ok_or_else(invalid)?;
            }
            require(
                size <= MAX_BATCH_BYTES
                    && region.decoded_bytes == size
                    && region.decoded_offset == decoded
                    && region.texture_offset == offset
                    && next_texture.is_none_or(|next| next == offset)
                    && region.input.bytes as usize == size + 16,
                "texture batch region offsets and receipt",
            )?;
            decoded = decoded.checked_add(size).ok_or_else(invalid)?;
            next_texture = Some(offset.checked_add(size).ok_or_else(invalid)?);
        }
        require(decoded == self.decoded_bytes, "texture batch decoded total")
    }
    /// Source IO verifies each receipt hash before this check and copy.
    pub fn copy_input(
        &self,
        index: usize,
        input: &[u8],
        output: &mut [u8],
    ) -> Result<(), ReferenceError> {
        let region = self.regions.get(index).ok_or_else(invalid)?;
        require(
            input.len() == region.input.bytes as usize
                && input.len() >= 16
                && input[..4] == *b"SMIP",
            "texture batch SMIP header and size",
        )?;
        let word = |start| u32::from_le_bytes(input[start..start + 4].try_into().unwrap());
        require(
            word(4) == region.width
                && word(8) == region.rows
                && word(12) == 1
                && input.len() - 16 == region.decoded_bytes
                && output.len() == self.decoded_bytes,
            "texture batch SMIP dimensions",
        )?;
        let end = region
            .decoded_offset
            .checked_add(region.decoded_bytes)
            .ok_or_else(invalid)?;
        output
            .get_mut(region.decoded_offset..end)
            .ok_or_else(invalid)?
            .copy_from_slice(&input[16..]);
        Ok(())
    }
}

impl LightingTextureRecord {
    /// Batch metadata must partition the existing ordered checked chunks exactly.
    pub fn validate_batch_receipts(&self, catalog: &FileCatalog) -> Result<(), ReferenceError> {
        self.validate_chunk_receipts(catalog)?;
        let mut index = 0;
        for batch in &self.batches {
            batch.descriptor.validate()?;
            require(
                batch.descriptor.bytes > 0
                    && batch.descriptor.bytes as usize <= MAX_DESCRIPTOR_BYTES
                    && batch.descriptor.file
                        == format!("reference/{}.stexbatch", batch.descriptor.sha256)
                    && catalog.file(&batch.descriptor.file)? == &batch.descriptor
                    && batch.region_count > 0
                    && batch.region_count <= MAX_BATCH_ENTRIES,
                "texture batch descriptor receipt",
            )?;
            let mut decoded = 0;
            let mut largest = 0;
            for _ in 0..batch.region_count {
                let chunk = self.chunks.get(index).ok_or_else(invalid)?;
                let bytes = pixels(chunk.width, chunk.rows)?;
                decoded += bytes;
                largest = largest.max(bytes + 16);
                index += 1;
            }
            require(
                decoded > 0
                    && decoded <= MAX_BATCH_BYTES
                    && batch.decoded_bytes == decoded
                    && batch.largest_raw_input_bytes == largest,
                "texture batch reference totals",
            )?;
        }
        require(
            index == self.chunks.len(),
            "texture batch complete coverage",
        )
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TextureArrayKey {
    pub width: u32,
    pub height: u32,
    pub srgb: bool,
    pub mips: u32,
    pub filter: u8,
    pub wrap_u: u8,
    pub wrap_v: u8,
    pub aniso: u32,
    pub bias: u32,
}
impl From<&LightingTextureRecord> for TextureArrayKey {
    fn from(t: &LightingTextureRecord) -> Self {
        Self {
            width: t.width,
            height: t.height,
            srgb: t.color_space != 0,
            mips: t.mip_count,
            filter: t.filter_mode,
            wrap_u: t.wrap_u,
            wrap_v: t.wrap_v,
            aniso: t.aniso,
            bias: t.mip_bias.to_bits(),
        }
    }
}
impl TextureArrayKey {
    pub fn layer_bytes(self) -> Result<usize, ReferenceError> {
        if self.width == 0
            || self.height == 0
            || self.mips == 0
            || self.mips > 32 - self.width.max(self.height).leading_zeros()
        {
            return Err(ReferenceError("Invalid source texture dimensions".into()));
        }
        let bytes: u64 = (0..self.mips)
            .map(|level| {
                u64::from((self.width >> level).max(1))
                    * u64::from((self.height >> level).max(1))
                    * 4
            })
            .sum();
        usize::try_from(bytes).map_err(|_| ReferenceError("Source texture size overflow".into()))
    }
}

/// Plan exact banks before allocation. Every material in this set must already be required.
pub fn texture_bank_groups<'a>(
    materials: &[super::WorldSourceMaterial],
    textures: &'a [LightingTextureRecord],
) -> Result<Vec<(TextureArrayKey, Vec<&'a LightingTextureRecord>)>, ReferenceError> {
    let mut groups: Vec<(TextureArrayKey, Vec<&LightingTextureRecord>)> = Vec::new();
    for material in materials {
        for link in &material.textures {
            let record = textures
                .iter()
                .find(|t| t.id == link.texture)
                .ok_or_else(|| {
                    ReferenceError(format!("Missing source texture {}", link.texture))
                })?;
            record.validate_chunks()?;
            let key = TextureArrayKey::from(record);
            let index = if let Some(index) = groups.iter().position(|(old, _)| *old == key) {
                index
            } else {
                groups.push((key, Vec::new()));
                groups.len() - 1
            };
            if let Some(old) = groups[index]
                .1
                .iter()
                .find(|old| old.source == record.source)
            {
                if old.mip_data.path != record.mip_data.path
                    || old.mip_data.sha256 != record.mip_data.sha256
                {
                    return Err(ReferenceError(format!(
                        "Conflicting texture bank source {}:{}",
                        record.source.file, record.source.path_id
                    )));
                }
            } else {
                groups[index].1.push(record);
            }
        }
    }
    for (_, records) in &mut groups {
        records.sort_by(|a, b| {
            (&a.source.file, a.source.path_id).cmp(&(&b.source.file, b.source.path_id))
        });
    }
    Ok(groups)
}
