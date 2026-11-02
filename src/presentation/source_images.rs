//! Shared assembly of authored PNG mip levels. Runs only during asset loading.
use crate::reference::SamplerRecord;
use bevy::{
    image::{ImageAddressMode, ImageFilterMode, ImageSampler, ImageSamplerDescriptor},
    prelude::*,
};

pub(crate) fn wrap(value: u8) -> ImageAddressMode {
    match value {
        0 => ImageAddressMode::Repeat,
        2 => ImageAddressMode::MirrorRepeat,
        _ => ImageAddressMode::ClampToEdge,
    }
}

pub(super) fn apply_sampler(image: &mut Image, settings: &SamplerRecord) {
    let filter = if settings.filter == 0 {
        ImageFilterMode::Nearest
    } else {
        ImageFilterMode::Linear
    };
    image.sampler = ImageSampler::Descriptor(ImageSamplerDescriptor {
        address_mode_u: wrap(settings.wrap_u),
        address_mode_v: wrap(settings.wrap_v),
        mag_filter: filter,
        min_filter: filter,
        mipmap_filter: if settings.filter == 2 || settings.aniso > 1 {
            ImageFilterMode::Linear
        } else {
            ImageFilterMode::Nearest
        },
        anisotropy_clamp: u16::from(settings.aniso.max(1)),
        lod_max_clamp: (settings.mip_count - 1) as f32,
        ..default()
    });
}
