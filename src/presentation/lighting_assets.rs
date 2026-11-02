//! Startup-only loading for the source lighting resources.
use crate::reference::{SourceId, lighting_scene::LightingRecord};
use bevy::asset::{AssetLoader, LoadContext, io::Reader};
use bevy::prelude::*;
use bevy::{asset as bevy_asset, ecs as bevy_ecs, reflect as bevy_reflect};
use std::{collections::HashMap, sync::Arc};

#[derive(Asset, TypePath)]
pub struct CasterDitherBytes(pub(crate) Vec<u8>);
#[derive(Default, TypePath)]
pub struct CasterDitherLoader;
impl AssetLoader for CasterDitherLoader {
    type Asset = CasterDitherBytes;
    type Settings = ();
    type Error = std::io::Error;
    async fn load(
        &self,
        reader: &mut dyn Reader,
        _: &(),
        _: &mut LoadContext<'_>,
    ) -> Result<Self::Asset, Self::Error> {
        let mut bytes = Vec::new();
        reader.read_to_end(&mut bytes).await?;
        if bytes.len() != 256 {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "Source caster dither requires 256 bytes",
            ));
        }
        Ok(CasterDitherBytes(bytes))
    }
    fn extensions(&self) -> &[&str] {
        &["bin"]
    }
}

struct LoadingTexture {
    image: Option<Handle<Image>>,
}
#[derive(Clone)]
pub struct SharedLightingRecord(pub Arc<crate::reference::ReferenceManifest>);
impl std::ops::Deref for SharedLightingRecord {
    type Target = LightingRecord;
    fn deref(&self) -> &Self::Target {
        &self.0.render_lighting
    }
}
#[derive(Resource)]
pub struct LightingAssets {
    pub record: SharedLightingRecord,
    textures: Vec<LoadingTexture>,
    texture_indices: HashMap<SourceId, usize>,
    pub cloud_mesh: Handle<Mesh>,
    pub(crate) dither_bytes: Handle<CasterDitherBytes>,
    pub caster_dither: Option<Handle<Image>>,
    pub shaders: LightingShaders,
    pub loaded: bool,
    pub texture_bytes: usize,
}
impl LightingAssets {
    pub fn texture(&self, source: &SourceId) -> Option<&Handle<Image>> {
        self.texture_indices
            .get(source)
            .and_then(|index| self.textures[*index].image.as_ref())
    }
}

pub fn load_lighting(
    commands: &mut Commands,
    _server: &AssetServer,
    reference: Arc<crate::reference::ReferenceManifest>,
) {
    let record = &reference.render_lighting;
    commands.insert_resource(LightingAssets {
        record: SharedLightingRecord(Arc::clone(&reference)),
        textures: record
            .textures
            .iter()
            .map(|_| LoadingTexture { image: None })
            .collect(),
        texture_indices: record
            .textures
            .iter()
            .enumerate()
            .map(|(i, t)| (t.source.clone(), i))
            .collect(),
        cloud_mesh: default(),
        dither_bytes: default(),
        caster_dither: None,
        shaders: default(),
        loaded: false,
        texture_bytes: 0,
    });
}

pub fn lighting_asset_readiness(
    assets: Option<ResMut<LightingAssets>>,
    server: Res<AssetServer>,
    materials: Res<super::materials::SourceMaterials>,
) {
    let Some(mut assets) = assets else { return };
    if assets.loaded {
        return;
    }
    let mut added = 0;
    for index in 0..assets.textures.len() {
        if assets.textures[index].image.is_some() {
            continue;
        }
        if let Some(handle) = materials.texture(&assets.record.textures[index].mips[0].file) {
            assets.textures[index].image = Some(handle);
            added += assets.record.textures[index]
                .mips
                .iter()
                .map(|m| m.width as usize * m.height as usize * 4)
                .sum::<usize>();
        }
    }
    assets.texture_bytes += added;
    assets.loaded = assets.textures.iter().all(|t| t.image.is_some())
        && assets.caster_dither.is_some()
        && server.is_loaded_with_dependencies(assets.cloud_mesh.id())
        && assets
            .shaders
            .handles()
            .iter()
            .all(|h| server.is_loaded_with_dependencies(h.id()));
}

/// Handles and source settings cross worlds once. GPU preparation waits for their uploads.
#[derive(Resource)]
pub struct LightingRenderAssets {
    pub record: SharedLightingRecord,
    pub textures: HashMap<SourceId, Handle<Image>>,
    pub cloud_mesh: Handle<Mesh>,
    pub caster_dither: Handle<Image>,
    pub shaders: LightingShaders,
}
pub fn extract_lighting_assets(
    mut commands: Commands,
    assets: bevy::render::Extract<Option<Res<LightingAssets>>>,
    existing: Option<Res<LightingRenderAssets>>,
) {
    if existing.is_some() {
        return;
    }
    let Some(assets) = assets.as_ref() else {
        return;
    };
    if !assets.loaded {
        return;
    }
    commands.insert_resource(LightingRenderAssets {
        record: assets.record.clone(),
        textures: assets
            .texture_indices
            .iter()
            .map(|(source, index)| {
                (
                    source.clone(),
                    assets.textures[*index]
                        .image
                        .as_ref()
                        .expect("loaded lighting texture")
                        .clone(),
                )
            })
            .collect(),
        cloud_mesh: assets.cloud_mesh.clone(),
        caster_dither: assets
            .caster_dither
            .as_ref()
            .expect("loaded caster dither")
            .clone(),
        shaders: assets.shaders.clone(),
    });
}
#[derive(Clone, Default)]
pub struct LightingShaders {
    pub lighting: Handle<Shader>,
    pub cloud: Handle<Shader>,
    pub sky: Handle<Shader>,
    pub float_store: Handle<Shader>,
    pub glass: Handle<Shader>,
    pub copy: Handle<Shader>,
    pub dof: Handle<Shader>,
    pub histogram: Handle<Shader>,
    pub exposure: Handle<Shader>,
    pub probe: Handle<Shader>,
    pub caster: Handle<Shader>,
    pub caster_math: Handle<Shader>,
    pub bloom: Handle<Shader>,
    pub lut: Handle<Shader>,
    pub lut_ldr: Handle<Shader>,
    pub uber: Handle<Shader>,
    pub heat_haze: Handle<Shader>,
    pub ao_depth: Handle<Shader>,
    pub ao_render: Handle<Shader>,
    pub ao_upsample: Handle<Shader>,
    pub ao_composite: Handle<Shader>,
    pub depth_resolve: Handle<Shader>,
    pub props: Handle<Shader>,
    pub props_cull: Handle<Shader>,
    pub motion: Handle<Shader>,
    pub taa: Handle<Shader>,
    pub motion_blur: Handle<Shader>,
}
impl LightingShaders {
    pub(crate) fn handles(&self) -> [&Handle<Shader>; 27] {
        [
            &self.lighting,
            &self.cloud,
            &self.sky,
            &self.float_store,
            &self.glass,
            &self.copy,
            &self.dof,
            &self.histogram,
            &self.exposure,
            &self.probe,
            &self.caster,
            &self.caster_math,
            &self.bloom,
            &self.lut,
            &self.lut_ldr,
            &self.uber,
            &self.heat_haze,
            &self.ao_depth,
            &self.ao_render,
            &self.ao_upsample,
            &self.ao_composite,
            &self.depth_resolve,
            &self.props,
            &self.props_cull,
            &self.motion,
            &self.taa,
            &self.motion_blur,
        ]
    }
}
