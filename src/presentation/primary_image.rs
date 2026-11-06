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

#[derive(Asset, TypePath)]
pub struct SourcePrimaryImage(pub Image);
#[derive(Default, TypePath)]
pub struct SourcePrimaryImageLoader;
#[derive(Default, serde::Serialize, serde::Deserialize)]
pub struct SourcePrimaryImageSettings {
    pub record: Option<Arc<TextureRecord>>,
    pub input_bytes: usize,
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
        let (byte_count, format, width, height, input) = match &record.storage {
            TextureStorage::NativeBc {
                format,
                width,
                height,
                input,
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
                (bytes, format, *width, *height, input)
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
                (bytes, format, *width, *height, input)
            }
        };
        if settings.input_bytes != byte_count {
            return Err(invalid("Primary input reservation differs"));
        }
        // Read directly into the final level vector. The checked reader retains its own input.
        let mut bytes = vec![0; byte_count];
        reader.read_exact(&mut bytes).await?;
        let mut extra = [0];
        if reader.read(&mut extra).await? != 0 {
            return Err(invalid(format!("{}: input exceeds receipt", input.file)));
        }
        let mut image = Image::new_uninit(
            Extent3d {
                width,
                height,
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
        Ok(SourcePrimaryImage(image))
    }
}
