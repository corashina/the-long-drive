//! Selected plan assets use the same checked reader as their geometry.
pub mod selected;
use crate::reference::prototype_plan::PrototypePlan;
use crate::reference::{FileCatalog, ReferenceError, SourceId};
use bevy::{
    asset as bevy_asset,
    asset::{AssetLoader, LoadContext, io::Reader},
    prelude::*,
    reflect as bevy_reflect,
};
use wasm_bindgen::prelude::*;
#[wasm_bindgen]
extern "C" {
    #[wasm_bindgen(catch,js_namespace=window,js_name=sourceInputBytes)]
    fn input_bytes(path: &str) -> Result<u32, JsValue>;
}
pub fn checked_input_bytes(path: &str) -> Result<usize, ReferenceError> {
    input_bytes(path)
        .map(|bytes| bytes as usize)
        .map_err(|e| ReferenceError(format!("World input size: {e:?}")))
}
#[derive(Asset, TypePath)]
pub struct SourcePlan(pub std::sync::Arc<PrototypePlan>);
#[derive(Default, TypePath)]
pub struct SourcePlanLoader;
impl AssetLoader for SourcePlanLoader {
    type Asset = SourcePlan;
    type Settings = usize;
    type Error = std::io::Error;
    async fn load(
        &self,
        reader: &mut dyn Reader,
        size: &usize,
        _: &mut LoadContext<'_>,
    ) -> Result<SourcePlan, Self::Error> {
        use bevy::tasks::futures_lite::AsyncReadExt;
        let invalid =
            |message: String| std::io::Error::new(std::io::ErrorKind::InvalidData, message);
        if *size == 0 || *size > 32 * 1024 * 1024 {
            return Err(invalid("Unadmitted plan input".into()));
        }
        let mut bytes = Vec::with_capacity(*size);
        reader
            .take(*size as u64 + 1)
            .read_to_end(&mut bytes)
            .await?;
        if bytes.len() != *size {
            return Err(invalid("Plan byte count differs".into()));
        }
        let plan: PrototypePlan =
            serde_json::from_slice(&bytes).map_err(|e| invalid(e.to_string()))?;
        Ok(SourcePlan(std::sync::Arc::new(plan)))
    }
}
/// The caller owns metadata CPU, input staging, and one job until this handle settles.
pub fn load_plan(
    server: &AssetServer,
    catalog: &FileCatalog,
    asset: &str,
) -> Result<Handle<SourcePlan>, ReferenceError> {
    let input = catalog.plan(asset)?;
    let size = input.bytes as usize;
    Ok(server
        .load_builder()
        .with_settings(move |target: &mut usize| *target = size)
        .load(input.file.clone()))
}

pub fn poll_plan(
    server: &AssetServer,
    plans: &Assets<SourcePlan>,
    handle: &Handle<SourcePlan>,
) -> Result<Option<std::sync::Arc<PrototypePlan>>, ReferenceError> {
    if let bevy::asset::LoadState::Failed(error) = server.load_state(handle.id()) {
        return Err(ReferenceError(format!("Source plan: {error}")));
    }
    Ok(plans
        .get(handle.id())
        .map(|SourcePlan(plan)| std::sync::Arc::clone(plan)))
}

/// Only checked startup assignments may replace a serialized null material.
pub fn initial_material(
    reference: &crate::reference::ReferenceManifest,
    node: &SourceId,
) -> Option<(crate::reference::AssetId, u32)> {
    let binding = reference
        .world
        .initial_materials
        .iter()
        .find(|m| &m.node == node)?;
    let material = reference
        .rendering
        .materials
        .iter()
        .find(|m| m.source == binding.material)?;
    Some((material.asset, material.gltf_material))
}
