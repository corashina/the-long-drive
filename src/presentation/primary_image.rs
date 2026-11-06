//! Checked source primary images. This asset never enters the render world.
use crate::reference::{NativeBcFormat, TextureColorSpace, TextureRecord, TextureStorage};
use bevy::tasks::futures_lite::AsyncReadExt;
use bevy::{asset as bevy_asset, reflect as bevy_reflect};
use bevy::{
    asset::{AssetLoader, LoadContext, RenderAssetUsages, io::Reader},
    prelude::*,
    render::render_resource::{Extent3d, TextureDataOrder, TextureDimension, TextureFormat},
};
use std::{
    io::{Error, ErrorKind},
    sync::Arc,
};

/// The image, and the crunch tables of a texture whose top mip loads later.
#[derive(Asset, TypePath)]
pub struct SourcePrimaryImage(pub Image, pub Option<Arc<[u8]>>);
#[derive(Default, TypePath)]
pub struct SourcePrimaryImageLoader;
#[derive(Default, serde::Serialize, serde::Deserialize)]
pub struct SourcePrimaryImageSettings {
    pub record: Option<Arc<TextureRecord>>,
    pub input_bytes: usize,
    /// Fill the top mip from mip 1 and do not read the top file. The top mip loads after start.
    pub defer_top: bool,
    /// Half texture quality: the image has mips 1+ only. See `TextureRecord::levels`.
    pub half: bool,
}
fn invalid(message: impl Into<String>) -> Error {
    Error::new(ErrorKind::InvalidData, message.into())
}
impl AssetLoader for SourcePrimaryImageLoader {
    type Asset = SourcePrimaryImage;
    type Settings = SourcePrimaryImageSettings;
    type Error = Error;
    async fn load(
        &self,
        reader: &mut dyn Reader,
        settings: &Self::Settings,
        context: &mut LoadContext<'_>,
    ) -> Result<Self::Asset, Error> {
        let record = settings
            .record
            .as_ref()
            .ok_or_else(|| invalid("Missing admitted primary image"))?;
        if context.path().path() != std::path::Path::new(record.file()) {
            return Err(invalid("Primary image path differs from admitted storage"));
        }
        let srgb = record.color_space == TextureColorSpace::Srgb;
        let (byte_count, format, width, height, input, top, crunch) = match &record.storage {
            TextureStorage::NativeBc {
                format,
                source_format,
                width,
                height,
                input,
                top,
                ..
            } => {
                let bytes = crate::reference::fixed_reservation::validate_native_bc(record)
                    .map_err(|e| invalid(e.to_string()))?;
                let format = match (format, srgb) {
                    (NativeBcFormat::Bc1, false) => TextureFormat::Bc1RgbaUnorm,
                    (NativeBcFormat::Bc1, true) => TextureFormat::Bc1RgbaUnormSrgb,
                    (NativeBcFormat::Bc3, false) => TextureFormat::Bc3RgbaUnorm,
                    (NativeBcFormat::Bc3, true) => TextureFormat::Bc3RgbaUnormSrgb,
                };
                let crunch = crate::reference::fixed_reservation::native_crunch(*source_format);
                (bytes, format, *width, *height, input, top.as_ref(), crunch)
            }
            TextureStorage::Rgba8 {
                width,
                height,
                input,
            } => {
                let bytes = crate::reference::fixed_reservation::validate_rgba(record)
                    .map_err(|e| invalid(e.to_string()))?;
                let format = if srgb {
                    TextureFormat::Rgba8UnormSrgb
                } else {
                    TextureFormat::Rgba8Unorm
                };
                (bytes, format, *width, *height, input, None, false)
            }
        };
        if settings.input_bytes != input.bytes as usize {
            return Err(invalid("Primary input reservation differs"));
        }
        let levels = record
            .levels(settings.half)
            .map_err(|e| invalid(e.to_string()))?;
        let block = format.block_copy_size(None).unwrap() as usize;
        // Bytes of the top mip in the level vector, when it has its own file.
        let top_bytes = top.map_or(0, |_| (width as usize / 4) * (height as usize / 4) * block);
        // Half has no top mip. A split chain then reads only its input.
        let top = top.filter(|_| levels.skip == 0);
        let mut bytes = vec![0; levels.bytes];
        let mut tables = None;
        if crunch {
            let mut file = vec![0; input.bytes as usize];
            read_input(reader, &mut file, &input.file).await?;
            let split = crunch::tables_bytes(&file).map_err(invalid)?;
            match top {
                // One level, or Half: the file holds exactly the image levels.
                None => {
                    let first = u32::from(levels.skip > 0);
                    super::crunch::unpack(&file, first, &mut bytes).await?
                }
                Some(_) if settings.defer_top => {
                    let (head, tail) = bytes.split_at_mut(top_bytes);
                    super::crunch::unpack(&file, 1, tail).await?;
                    crate::reference::fixed_reservation::upscale_bc(
                        block, width, height, tail, head,
                    );
                    tables = Some(Arc::from(&file[..split]));
                }
                Some(top) => {
                    let data = read_top(context, top).await?;
                    // The original crunch file: tables, level 0, then levels 1+.
                    let whole = [&file[..split], &data, &file[split..]].concat();
                    super::crunch::unpack(&whole, 0, &mut bytes).await?;
                }
            }
        } else {
            // An RGBA8 input starts with the top mip. Half reads it in steps and drops it.
            let mut skip = levels.skip.saturating_sub(top_bytes);
            if skip > 0 {
                let mut scratch = vec![0; skip.min(64 * 1024)];
                while skip > 0 {
                    let step = skip.min(scratch.len());
                    reader.read_exact(&mut scratch[..step]).await?;
                    skip -= step;
                }
            }
            // Read directly into the final level vector, after the top mip of a split BC chain.
            // The checked reader retains its own input.
            let start = if top.is_some() { top_bytes } else { 0 };
            read_input(reader, &mut bytes[start..], &input.file).await?;
            if let Some(top) = top {
                if settings.defer_top {
                    let (head, tail) = bytes.split_at_mut(top_bytes);
                    crate::reference::fixed_reservation::upscale_bc(
                        block, width, height, tail, head,
                    );
                } else {
                    let data = read_top(context, top).await?;
                    if data.len() != top_bytes {
                        return Err(invalid(format!("{}: BC differs from receipt", top.file)));
                    }
                    bytes[..top_bytes].copy_from_slice(&data);
                }
            }
        }
        let mut image = Image::new_uninit(
            Extent3d {
                width: levels.width,
                height: levels.height,
                depth_or_array_layers: 1,
            },
            TextureDimension::D2,
            format,
            RenderAssetUsages::RENDER_WORLD,
        );
        image.data = Some(bytes);
        image.data_order = TextureDataOrder::MipMajor;
        image.texture_descriptor.mip_level_count = levels.mip_count;
        super::source_images::apply_sampler(&mut image, &record.sampler, levels.mip_count);
        Ok(SourcePrimaryImage(image, tables))
    }
}
/// Reads exactly the receipt bytes of a checked input.
async fn read_input(reader: &mut dyn Reader, bytes: &mut [u8], file: &str) -> Result<(), Error> {
    reader.read_exact(bytes).await?;
    if reader.read(&mut [0]).await? != 0 {
        return Err(invalid(format!("{file}: input exceeds receipt")));
    }
    Ok(())
}
async fn read_top(
    context: &mut LoadContext<'_>,
    top: &crate::reference::FileReceipt,
) -> Result<Vec<u8>, Error> {
    let data = context
        .read_asset_bytes(top.file.clone())
        .await
        .map_err(|e| invalid(format!("{}: {e}", top.file)))?;
    if data.len() != top.bytes as usize {
        return Err(invalid(format!("{}: input differs from receipt", top.file)));
    }
    Ok(data)
}
/// Top mip blocks of one native BC texture, loaded after start.
#[derive(Asset, TypePath)]
pub struct SourceTopMip(pub Vec<u8>);
#[derive(Default, TypePath)]
pub struct SourceTopMipLoader;
#[derive(Default, serde::Serialize, serde::Deserialize)]
pub struct SourceTopMipSettings {
    /// Receipt bytes of the top file.
    pub input_bytes: usize,
    /// BC bytes of the top mip.
    pub bytes: usize,
    /// Crunch tables of the texture. The top file then holds the level 0 crunch data.
    pub tables: Option<Arc<[u8]>>,
}
impl AssetLoader for SourceTopMipLoader {
    type Asset = SourceTopMip;
    type Settings = SourceTopMipSettings;
    type Error = Error;
    async fn load(
        &self,
        reader: &mut dyn Reader,
        settings: &Self::Settings,
        context: &mut LoadContext<'_>,
    ) -> Result<Self::Asset, Error> {
        let file = context.path().path().to_string_lossy();
        let mut input = vec![0; settings.input_bytes];
        read_input(reader, &mut input, &file).await?;
        let Some(tables) = &settings.tables else {
            if input.len() != settings.bytes {
                return Err(invalid(format!("{file}: BC differs from receipt")));
            }
            return Ok(SourceTopMip(input));
        };
        let mut bytes = vec![0; settings.bytes];
        super::crunch::unpack(&[&tables[..], &input[..]].concat(), 0, &mut bytes).await?;
        Ok(SourceTopMip(bytes))
    }
}
