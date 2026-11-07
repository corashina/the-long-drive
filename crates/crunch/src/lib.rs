//! Unity crunch textures (DXT1, DXT5) to BC1 and BC3 blocks.
//!
//! A crunch file is a header, the tables and palettes, then the data of each level.
//! A stored file is the file start before level 0, then the data of the levels `first..`.
//! It can end after any level. The exporter stores level 0 alone, so it can load later.
mod crn;
use crn::crn_header::CrnHeader;
use crn::crn_unpacker::CrnUnpacker;

/// Header byte offset of the level 0 data offset.
const LEVEL_OFFSETS: usize = 70;

/// Byte count of the file start before the level 0 data: header, tables and palettes.
pub fn tables_bytes(file: &[u8]) -> Result<usize, String> {
    let bytes = file
        .get(LEVEL_OFFSETS..LEVEL_OFFSETS + 4)
        .ok_or("Crunch header is truncated")?;
    let tables = u32::from_be_bytes(bytes.try_into().unwrap()) as usize;
    if tables > file.len() {
        return Err("Crunch tables are truncated".into());
    }
    Ok(tables)
}

/// BC blocks of the levels in a stored file, in level order.
/// `first` is the level of the first data after the tables.
pub fn unpack(file: &[u8], first: u32) -> Result<Vec<u8>, String> {
    let tables = tables_bytes(file)?;
    let mut header = CrnHeader::default();
    let header_bytes = (header_size(file)? as usize).min(tables);
    if header_bytes < LEVEL_OFFSETS + 4 * file[16] as usize
        || !header.crnd_get_header(&file[..header_bytes], u32::MAX)
    {
        return Err("Invalid crunch header".into());
    }
    let levels = header.levels.cast_to_uint();
    let format = header.format.cast_to_uint();
    let (width, height) = (header.width.cast_to_uint(), header.height.cast_to_uint());
    let data_size = header.data_size.cast_to_uint() as usize;
    if header.faces.cast_to_uint() != 1
        || !matches!(format, 0 | 2)
        || levels == 0
        || first >= levels
    {
        return Err("Unsupported crunch texture".into());
    }
    // Each table section must end before level 0.
    for (offset, size) in [
        (
            &mut header.color_endpoints.ofs,
            &mut header.color_endpoints.size,
        ),
        (
            &mut header.color_selectors.ofs,
            &mut header.color_selectors.size,
        ),
        (
            &mut header.alpha_endpoints.ofs,
            &mut header.alpha_endpoints.size,
        ),
        (
            &mut header.alpha_selectors.ofs,
            &mut header.alpha_selectors.size,
        ),
    ] {
        if (offset.cast_to_uint() + size.cast_to_uint()) as usize > tables {
            return Err("Crunch palette is outside the tables".into());
        }
    }
    if (header.tables_ofs.cast_to_uint() + header.tables_size.cast_to_uint()) as usize > tables {
        return Err("Crunch tables are outside the file start".into());
    }
    let offsets: Vec<usize> = (0..levels as usize)
        .map(|i| header.level_ofs[i].cast_to_uint() as usize)
        .chain([data_size])
        .collect();
    if offsets[0] != tables || offsets.windows(2).any(|w| w[1] <= w[0]) {
        return Err("Invalid crunch level offsets".into());
    }
    let block = if format == 0 { 8 } else { 16 };
    let mut unpacker = CrnUnpacker::default();
    if !unpacker.init(&file[..tables], data_size as u32) {
        return Err("Invalid crunch tables".into());
    }
    let mut out = Vec::new();
    let mut at = tables;
    for level in first..levels {
        if at == file.len() && level > first {
            break;
        }
        let size = offsets[level as usize + 1] - offsets[level as usize];
        let data = file
            .get(at..at + size)
            .ok_or("Crunch level data is truncated")?;
        let blocks_x = (width >> level).max(1).div_ceil(4);
        let blocks_y = (height >> level).max(1).div_ceil(4);
        let pitch = blocks_x * block;
        let blocks = unpacker.unpack_level_2(data, size as u32, pitch * blocks_y, pitch, level)?;
        out.extend_from_slice(&blocks);
        at += size;
    }
    if at != file.len() {
        return Err("Crunch file has trailing data".into());
    }
    Ok(out)
}

fn header_size(file: &[u8]) -> Result<u16, String> {
    let bytes = file.get(2..4).ok_or("Crunch header is truncated")?;
    Ok(u16::from_be_bytes(bytes.try_into().unwrap()))
}

#[cfg(test)]
mod tests;
