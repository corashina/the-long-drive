//! Original decoded source mip loader.
use bevy::tasks::futures_lite::AsyncReadExt;
use bevy::{
    asset::{AssetLoader, LoadContext, RenderAssetUsages, io::Reader},
    image::{ImageAddressMode, ImageFilterMode, ImageSampler, ImageSamplerDescriptor},
    prelude::*,
    reflect as bevy_reflect,
    render::render_resource::*,
};
#[derive(Default, TypePath)]
pub struct SourceImageLoader;
#[derive(Default, serde::Serialize, serde::Deserialize)]
pub struct SourceImageSettings {
    pub srgb: bool,
    pub filter: u8,
    pub wrap_u: u8,
    pub wrap_v: u8,
    #[serde(default)]
    pub main_world_only: bool,
    #[serde(default)]
    pub decoded_bytes: usize,
}
impl AssetLoader for SourceImageLoader {
    type Asset = Image;
    type Settings = SourceImageSettings;
    type Error = std::io::Error;
    async fn load(
        &self,
        reader: &mut dyn Reader,
        settings: &Self::Settings,
        _: &mut LoadContext<'_>,
    ) -> Result<Image, Self::Error> {
        let capacity = settings.decoded_bytes.checked_add(16).ok_or_else(|| {
            std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "Source image size overflow",
            )
        })?;
        if settings.decoded_bytes == 0 {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "Source image requires admitted bytes",
            ));
        }
        let mut data = vec![0; capacity];
        reader.read_exact(&mut data).await?;
        let mut extra = [0_u8; 1];
        if reader.read(&mut extra).await? != 0 {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "Source mip exceeds admitted bytes",
            ));
        }
        let invalid = || {
            std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "Invalid source mip payload",
            )
        };
        let _profile =
            crate::app::cpu_profile::Timer::new(crate::app::cpu_profile::Stage::AssetDecode);
        if data.len() < 16 || &data[..4] != b"SMIP" {
            return Err(invalid());
        }
        let width = u32::from_le_bytes(data[4..8].try_into().unwrap());
        let height = u32::from_le_bytes(data[8..12].try_into().unwrap());
        let count = u32::from_le_bytes(data[12..16].try_into().unwrap());
        if width == 0 || height == 0 || count == 0 || count > 32 - width.max(height).leading_zeros()
        {
            return Err(invalid());
        }
        let bytes = (0..count)
            .try_fold(0usize, |sum, level| {
                let pixels = ((width >> level).max(1) as usize)
                    .checked_mul((height >> level).max(1) as usize)?
                    .checked_mul(4)?;
                sum.checked_add(pixels)
            })
            .ok_or_else(invalid)?;
        if bytes != settings.decoded_bytes {
            return Err(invalid());
        }
        data.drain(..16);
        Ok(source_image(width, height, count, data, settings))
    }
    fn extensions(&self) -> &[&str] {
        &["smip"]
    }
}

pub(crate) fn source_image(
    width: u32,
    height: u32,
    count: u32,
    data: Vec<u8>,
    settings: &SourceImageSettings,
) -> Image {
    let mut image = Image::new_uninit(
        Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        if settings.srgb {
            TextureFormat::Rgba8UnormSrgb
        } else {
            TextureFormat::Rgba8Unorm
        },
        if settings.main_world_only {
            RenderAssetUsages::MAIN_WORLD
        } else {
            RenderAssetUsages::MAIN_WORLD | RenderAssetUsages::RENDER_WORLD
        },
    );
    image.texture_descriptor.mip_level_count = count;
    image.data = Some(data);
    let address = |value| match value {
        0 => ImageAddressMode::Repeat,
        2 => ImageAddressMode::MirrorRepeat,
        _ => ImageAddressMode::ClampToEdge,
    };
    let filter = if settings.filter == 0 {
        ImageFilterMode::Nearest
    } else {
        ImageFilterMode::Linear
    };
    image.sampler = ImageSampler::Descriptor(ImageSamplerDescriptor {
        mag_filter: filter,
        min_filter: filter,
        mipmap_filter: if settings.filter == 2 {
            ImageFilterMode::Linear
        } else {
            ImageFilterMode::Nearest
        },
        address_mode_u: address(settings.wrap_u),
        address_mode_v: address(settings.wrap_v),
        ..default()
    });
    image
}
