//! CPU-only batches. Upload owners retain these assets through GPU acknowledgement.
use crate::reference::texture_batch::{
    MAX_BATCH_BYTES, MAX_DESCRIPTOR_BYTES, TextureBatchDescriptor, TextureBatchRegion,
};
use bevy::tasks::futures_lite::AsyncReadExt;
use bevy::{
    asset as bevy_asset,
    asset::{AssetLoader, LoadContext, io::Reader},
    prelude::*,
    reflect as bevy_reflect,
};
use std::{
    io::{Error, ErrorKind},
    sync::Arc,
};

#[derive(Asset, TypePath, Clone)]
pub struct SourceTextureBatch {
    pub regions: Arc<[TextureBatchRegion]>,
    pub payload: Arc<Vec<u8>>,
}
#[derive(TypePath)]
pub struct SourceTextureBatchLoader {
    catalog: Arc<crate::reference::FileCatalog>,
}
impl FromWorld for SourceTextureBatchLoader {
    fn from_world(world: &mut World) -> Self {
        Self {
            catalog: Arc::clone(
                &world
                    .resource::<crate::app::RuntimeReference>()
                    .0
                    .file_catalog,
            ),
        }
    }
}
#[derive(Default, serde::Serialize, serde::Deserialize)]
pub struct SourceTextureBatchSettings {
    pub descriptor_bytes: usize,
    pub decoded_bytes: usize,
}
impl AssetLoader for SourceTextureBatchLoader {
    type Asset = SourceTextureBatch;
    type Settings = SourceTextureBatchSettings;
    type Error = Error;
    async fn load(
        &self,
        reader: &mut dyn Reader,
        settings: &Self::Settings,
        context: &mut LoadContext<'_>,
    ) -> Result<Self::Asset, Self::Error> {
        let invalid = |message: String| Error::new(ErrorKind::InvalidData, message);
        if settings.descriptor_bytes == 0
            || settings.descriptor_bytes > MAX_DESCRIPTOR_BYTES
            || settings.decoded_bytes == 0
            || settings.decoded_bytes > MAX_BATCH_BYTES
        {
            return Err(invalid(
                "Texture batch requires bounded admitted bytes".into(),
            ));
        }
        let mut bytes = vec![0; settings.descriptor_bytes];
        reader.read_exact(&mut bytes).await?;
        let mut extra = [0; 1];
        if reader.read(&mut extra).await? != 0 {
            return Err(invalid("Texture batch descriptor exceeds receipt".into()));
        }
        let descriptor = TextureBatchDescriptor::parse(&bytes, settings.decoded_bytes)
            .map_err(|e| invalid(format!("{}: {e}", context.path())))?;
        drop(bytes);
        for region in &descriptor.regions {
            let receipt = self.catalog.file(&region.input.file).map_err(|e| {
                invalid(format!(
                    "{}:{} {}: {e}",
                    descriptor.source.file, descriptor.source.path_id, region.input.file
                ))
            })?;
            if receipt != &region.input {
                return Err(invalid(format!(
                    "{}:{} {}: descriptor input receipt differs from catalog",
                    descriptor.source.file, descriptor.source.path_id, region.input.file
                )));
            }
        }
        let mut payload = vec![0; settings.decoded_bytes];
        for (index, region) in descriptor.regions.iter().enumerate() {
            let identity = || {
                format!(
                    "{}:{} {}",
                    descriptor.source.file, descriptor.source.path_id, region.input.file
                )
            };
            // The default source reader checks the catalog hash and byte count.
            let input = context
                .read_asset_bytes(&region.input.file)
                .await
                .map_err(|e| invalid(format!("{}: {e}", identity())))?;
            descriptor
                .copy_input(index, &input, &mut payload)
                .map_err(|e| invalid(format!("{}: {e}", identity())))?;
            // Only one raw input remains live at a time.
        }
        Ok(SourceTextureBatch {
            regions: descriptor.regions.into(),
            payload: Arc::new(payload),
        })
    }
    fn extensions(&self) -> &[&str] {
        &["stexbatch"]
    }
}
