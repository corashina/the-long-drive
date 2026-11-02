//! Checked source primary images. This asset never enters the render world.
use crate::reference::{NativeBcFormat, TextureColorSpace, TextureRecord, TextureStorage};
use bevy::tasks::futures_lite::AsyncReadExt;
use bevy::{asset as bevy_asset, reflect as bevy_reflect};
use bevy::{
    asset::{AssetLoader, LoadContext, RenderAssetUsages, io::Reader},
    image::{CompressedImageFormats, ImageSampler, ImageType},
    prelude::*,
};
use std::{
    io::{Error, ErrorKind},
    sync::Arc,
};

#[derive(Asset, TypePath)]
pub struct SourcePrimaryImage(pub Image);
#[derive(Default, TypePath)]
pub struct SourcePrimaryImageLoader;
#[derive(Default, serde::Serialize, serde::Deserialize)]
pub struct SourcePrimaryImageSettings {
    pub record: Option<Arc<TextureRecord>>,
    pub encoded_bytes: Vec<usize>,
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
        if let TextureStorage::NativeBc {
            format,
            width,
            height,
            input,
            ..
        } = &record.storage
        {
            let byte_count = crate::reference::fixed_reservation::validate_native_bc(record)
                .map_err(|e| invalid(e.to_string()))?;
            if settings.encoded_bytes.as_slice() != [byte_count] {
                return Err(invalid("Native BC input reservation differs"));
            }
            // Read directly into the final block vector. The checked reader retains its own input.
            let mut bytes = vec![0; byte_count];
            reader.read_exact(&mut bytes).await?;
            let mut extra = [0];
            if reader.read(&mut extra).await? != 0 {
                return Err(invalid(format!("{}: BC exceeds receipt", input.file)));
            }
            use bevy::render::render_resource::{
                Extent3d, TextureDataOrder, TextureDimension, TextureFormat,
            };
            let srgb = record.color_space == TextureColorSpace::Srgb;
            let format = match (format, srgb) {
                (NativeBcFormat::Bc1, false) => TextureFormat::Bc1RgbaUnorm,
                (NativeBcFormat::Bc1, true) => TextureFormat::Bc1RgbaUnormSrgb,
                (NativeBcFormat::Bc3, false) => TextureFormat::Bc3RgbaUnorm,
                (NativeBcFormat::Bc3, true) => TextureFormat::Bc3RgbaUnormSrgb,
            };
            let mut image = Image::new_uninit(
                Extent3d {
                    width: *width,
                    height: *height,
                    depth_or_array_layers: 1,
                },
                TextureDimension::D2,
                format,
                RenderAssetUsages::RENDER_WORLD,
            );
            image.data = Some(bytes);
            image.data_order = TextureDataOrder::MipMajor;
            image.texture_descriptor.mip_level_count = record.sampler.mip_count;
            super::source_images::apply_sampler(&mut image, &record.sampler);
            return Ok(SourcePrimaryImage(image));
        }
        let TextureStorage::PngChain { mips } = &record.storage else {
            unreachable!()
        };
        if mips.is_empty()
            || mips.len() != record.sampler.mip_count as usize
            || settings.encoded_bytes.len() != mips.len()
        {
            return Err(invalid("Invalid admitted PNG chain"));
        }
        let byte_count = crate::reference::fixed_reservation::png_pixels(mips)
            .map_err(|e| invalid(e.to_string()))?;
        let mut bytes = Vec::new();
        let mut assembled = None;
        for (index, mip) in mips.iter().enumerate() {
            // The base reader already checked its receipt. Never fetch it twice.
            let input = if index == 0 {
                let mut input = vec![0; settings.encoded_bytes[index]];
                reader.read_exact(&mut input).await?;
                let mut extra = [0];
                if reader.read(&mut extra).await? != 0 {
                    return Err(invalid(format!("{}: PNG exceeds receipt", mip.file)));
                }
                input
            } else {
                context
                    .read_asset_bytes(mip.file.clone())
                    .await
                    .map_err(|e| invalid(format!("{}: {e}", mip.file)))?
            };
            if input.len() != settings.encoded_bytes[index] {
                return Err(invalid(format!(
                    "{}: PNG byte count differs from receipt",
                    mip.file
                )));
            }
            let srgb = record.color_space == TextureColorSpace::Srgb;
            let _profile =
                crate::app::cpu_profile::Timer::new(crate::app::cpu_profile::Stage::AssetDecode);
            let mut image = Image::from_buffer(
                &input,
                ImageType::Extension("png"),
                CompressedImageFormats::NONE,
                srgb,
                ImageSampler::Default,
                RenderAssetUsages::MAIN_WORLD,
            )
            .map_err(|e| invalid(format!("{}: {e}", mip.file)))?;
            use bevy::render::render_resource::TextureFormat;
            let format = if srgb {
                TextureFormat::Rgba8UnormSrgb
            } else {
                TextureFormat::Rgba8Unorm
            };
            if image.width() != mip.width
                || image.height() != mip.height
                || image.texture_descriptor.format != format
                || image.texture_descriptor.mip_level_count != 1
                || image.data.as_ref().map(Vec::len)
                    != Some(mip.width as usize * mip.height as usize * 4)
            {
                return Err(invalid(format!("Invalid decoded mip {}", mip.file)));
            }
            // Allocate after the base decoder drops its RGB conversion buffers.
            if index == 0 {
                bytes.reserve_exact(byte_count);
            }
            // Taking drops the decoded capacity; the retained base Image keeps no pixel allocation.
            bytes.extend_from_slice(&image.data.take().expect("checked mip bytes"));
            if index == 0 {
                assembled = Some(image);
            }
            // Input and decoded level drop before the next checked read.
        }
        let mut image = assembled.expect("checked mip count");
        image.data = Some(bytes);
        image.texture_descriptor.mip_level_count = record.sampler.mip_count;
        image.asset_usage = RenderAssetUsages::RENDER_WORLD;
        super::source_images::apply_sampler(&mut image, &record.sampler);
        Ok(SourcePrimaryImage(image))
    }
}
