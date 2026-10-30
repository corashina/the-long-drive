//! Receipt checks complete before the existing Bevy decoders receive input.
use crate::reference::FileCatalog;
use async_lock::Semaphore;
use bevy::{
    asset::{
        AssetApp,
        io::{
            AssetReader, AssetReaderError, AssetSourceBuilder, AssetSourceId, PathStream, Reader,
            VecReader,
        },
    },
    prelude::{App, Plugin},
};
use std::{path::Path, sync::Arc};
use wasm_bindgen::prelude::*;
#[wasm_bindgen]
extern "C" {
    #[wasm_bindgen(catch, js_namespace=window, js_name=sourceReadAsset)]
    async fn read_asset(base: &str, receipt: &str) -> Result<js_sys::Uint8Array, JsValue>;
}
pub(crate) struct SourceIoPlugin {
    content_base: String,
    catalog: Arc<FileCatalog>,
}
impl SourceIoPlugin {
    pub fn new(content_base: String, catalog: Arc<FileCatalog>) -> Self {
        Self {
            content_base,
            catalog,
        }
    }
}
impl Plugin for SourceIoPlugin {
    fn build(&self, app: &mut App) {
        let base = self.content_base.clone();
        let catalog = Arc::clone(&self.catalog);
        app.register_asset_source(
            AssetSourceId::Default,
            AssetSourceBuilder::new(move || {
                Box::new(SourceAssetReader {
                    base: base.clone(),
                    catalog: Arc::clone(&catalog),
                    requests: Semaphore::new(8),
                })
            }),
        );
    }
}
struct SourceAssetReader {
    base: String,
    catalog: Arc<FileCatalog>,
    requests: Semaphore,
}
impl AssetReader for SourceAssetReader {
    async fn read<'a>(&'a self, path: &'a Path) -> Result<impl Reader + 'a, AssetReaderError> {
        let invalid = |message: String| {
            AssetReaderError::Io(
                std::io::Error::new(std::io::ErrorKind::InvalidData, message).into(),
            )
        };
        let path = path
            .to_str()
            .ok_or_else(|| invalid("Invalid source path".into()))?;
        // Build shaders never come from the exported package.
        if let Some(source) = crate::reference::fixed_reservation::shader_source(path) {
            return Ok(VecReader::new(source.as_bytes().to_vec()));
        }
        let receipt = self
            .catalog
            .file(path)
            .map_err(|e| invalid(e.to_string()))?;
        let json = serde_json::to_string(receipt).map_err(|e| invalid(e.to_string()))?;
        // The asset owner keeps byte/job reservations through decode and upload.
        // This permit covers only the physical response, never dependent decoder work.
        let permit = self.requests.acquire().await;
        let bytes = read_asset(&self.base, &json)
            .await
            .map_err(|e| invalid(format!("Checked source input: {e:?}")))?
            .to_vec();
        drop(permit);
        Ok(VecReader::new(bytes))
    }
    async fn read_meta<'a>(&'a self, path: &'a Path) -> Result<impl Reader + 'a, AssetReaderError> {
        // The package has no Bevy .meta files; AssetPlugin disables metadata probing.
        Err::<VecReader, _>(AssetReaderError::NotFound(path.to_path_buf()))
    }
    async fn read_directory<'a>(
        &'a self,
        _path: &'a Path,
    ) -> Result<Box<PathStream>, AssetReaderError> {
        Ok(Box::new(bevy::tasks::futures_lite::stream::empty()))
    }
    async fn is_directory<'a>(&'a self, _path: &'a Path) -> Result<bool, AssetReaderError> {
        Ok(false)
    }
}
