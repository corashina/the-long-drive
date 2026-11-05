//! Pipelines that compiled during play in an earlier run (see `play_pipelines.rs`).
//! Loading compiles each listed pipeline. The cache then returns it to the specializer that asks for it.
//! To update the list, play, call `window.gamePrewarmList()`, and save the result as `prewarm_pipelines.json`.
use bevy::asset::{AssetServer, Handle, RecursiveDependencyLoadState, uuid::Uuid};
use bevy::mesh::VertexBufferLayout;
use bevy::render::{
    extract_resource::{ExtractResource, ExtractResourcePlugin},
    render_resource::{
        BindGroupLayoutDescriptor, CachedComputePipelineId, CachedPipelineState,
        CachedRenderPipelineId, ComputePipelineDescriptor, FragmentState, PipelineCache,
        RenderPipelineDescriptor, VertexState,
    },
};
use bevy::shader::{Shader, ShaderCacheError, ShaderDefVal};
use bevy::{ecs as bevy_ecs, prelude::*, render as bevy_render};
use serde::{Deserialize, Serialize};
use std::borrow::Cow;
use std::sync::{Arc, Mutex};

const BUNDLED: &str = include_str!("prewarm_pipelines.json");

#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Entry {
    Render(RenderEntry),
    Compute(ComputeEntry),
}
#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
pub(crate) struct RenderEntry {
    label: Option<String>,
    layout: Vec<Layout>,
    immediate_size: u32,
    vertex: Stage,
    buffers: Vec<Buffer>,
    primitive: wgpu::PrimitiveState,
    depth_stencil: Option<wgpu::DepthStencilState>,
    multisample: wgpu::MultisampleState,
    fragment: Option<Fragment>,
    zero_initialize_workgroup_memory: bool,
}
#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
pub(crate) struct ComputeEntry {
    label: Option<String>,
    layout: Vec<Layout>,
    immediate_size: u32,
    stage: Stage,
    zero_initialize_workgroup_memory: bool,
}
#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
struct Layout {
    label: String,
    entries: Vec<wgpu::BindGroupLayoutEntry>,
}
#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
struct Stage {
    shader: ShaderSource,
    defs: Vec<ShaderDefVal>,
    entry_point: Option<String>,
}
#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
struct Fragment {
    #[serde(flatten)]
    stage: Stage,
    targets: Vec<Option<wgpu::ColorTargetState>>,
}
#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
struct Buffer {
    array_stride: u64,
    step_mode: wgpu::VertexStepMode,
    attributes: Vec<wgpu::VertexAttribute>,
}
#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
#[serde(rename_all = "snake_case")]
enum ShaderSource {
    Path(String),
    Uuid(String),
}

fn owned(value: &Cow<'static, str>) -> String {
    value.to_string()
}
fn layouts(layout: &[BindGroupLayoutDescriptor]) -> Vec<Layout> {
    layout
        .iter()
        .map(|layout| Layout {
            label: owned(&layout.label),
            entries: layout.entries.clone(),
        })
        .collect()
}
fn source(shader: &Handle<Shader>) -> Option<ShaderSource> {
    match shader {
        Handle::Uuid(uuid, _) => Some(ShaderSource::Uuid(uuid.to_string())),
        Handle::Strong(_) => shader
            .path()
            .map(|path| ShaderSource::Path(path.to_string())),
    }
}
fn stage(
    shader: &Handle<Shader>,
    defs: &[ShaderDefVal],
    entry_point: &Option<Cow<'static, str>>,
) -> Option<Stage> {
    Some(Stage {
        shader: source(shader)?,
        defs: defs.to_vec(),
        entry_point: entry_point.as_ref().map(owned),
    })
}

impl Entry {
    /// `None` when a shader has no path or uuid, so a later run cannot find it.
    pub(crate) fn render(descriptor: &RenderPipelineDescriptor) -> Option<Self> {
        let vertex = &descriptor.vertex;
        let fragment = match &descriptor.fragment {
            Some(fragment) => Some(Fragment {
                stage: stage(
                    &fragment.shader,
                    &fragment.shader_defs,
                    &fragment.entry_point,
                )?,
                targets: fragment.targets.clone(),
            }),
            None => None,
        };
        Some(Self::Render(RenderEntry {
            label: descriptor.label.as_ref().map(owned),
            layout: layouts(&descriptor.layout),
            immediate_size: descriptor.immediate_size,
            vertex: stage(&vertex.shader, &vertex.shader_defs, &vertex.entry_point)?,
            buffers: vertex
                .buffers
                .iter()
                .map(|buffer| Buffer {
                    array_stride: buffer.array_stride,
                    step_mode: buffer.step_mode,
                    attributes: buffer.attributes.clone(),
                })
                .collect(),
            primitive: descriptor.primitive,
            depth_stencil: descriptor.depth_stencil.clone(),
            multisample: descriptor.multisample,
            fragment,
            zero_initialize_workgroup_memory: descriptor.zero_initialize_workgroup_memory,
        }))
    }
    pub(crate) fn compute(descriptor: &ComputePipelineDescriptor) -> Option<Self> {
        Some(Self::Compute(ComputeEntry {
            label: descriptor.label.as_ref().map(owned),
            layout: layouts(&descriptor.layout),
            immediate_size: descriptor.immediate_size,
            stage: stage(
                &descriptor.shader,
                &descriptor.shader_defs,
                &descriptor.entry_point,
            )?,
            zero_initialize_workgroup_memory: descriptor.zero_initialize_workgroup_memory,
        }))
    }
}

/// Bevy descriptor of one entry, with the shader handles that must load first.
#[derive(Clone)]
enum Pipeline {
    Render(RenderPipelineDescriptor),
    Compute(ComputePipelineDescriptor),
}
fn descriptor(
    entry: &Entry,
    server: &AssetServer,
) -> Result<(Pipeline, Vec<Handle<Shader>>), String> {
    let mut shaders = Vec::new();
    let mut shader = |source: &ShaderSource| -> Result<Handle<Shader>, String> {
        let handle = match source {
            ShaderSource::Path(path) => server.load(path.clone()),
            ShaderSource::Uuid(uuid) => Handle::Uuid(
                Uuid::parse_str(uuid).map_err(|error| format!("shader uuid {uuid}: {error}"))?,
                default(),
            ),
        };
        shaders.push(handle.clone());
        Ok(handle)
    };
    let layout = |layout: &[Layout]| -> Vec<BindGroupLayoutDescriptor> {
        layout
            .iter()
            .map(|layout| BindGroupLayoutDescriptor::new(layout.label.clone(), &layout.entries))
            .collect()
    };
    let pipeline = match entry {
        Entry::Render(entry) => Pipeline::Render(RenderPipelineDescriptor {
            label: entry.label.clone().map(Cow::Owned),
            layout: layout(&entry.layout),
            immediate_size: entry.immediate_size,
            vertex: VertexState {
                shader: shader(&entry.vertex.shader)?,
                shader_defs: entry.vertex.defs.clone(),
                entry_point: entry.vertex.entry_point.clone().map(Cow::Owned),
                buffers: entry
                    .buffers
                    .iter()
                    .map(|buffer| VertexBufferLayout {
                        array_stride: buffer.array_stride,
                        step_mode: buffer.step_mode,
                        attributes: buffer.attributes.clone(),
                    })
                    .collect(),
            },
            primitive: entry.primitive,
            depth_stencil: entry.depth_stencil.clone(),
            multisample: entry.multisample,
            fragment: match &entry.fragment {
                Some(fragment) => Some(FragmentState {
                    shader: shader(&fragment.stage.shader)?,
                    shader_defs: fragment.stage.defs.clone(),
                    entry_point: fragment.stage.entry_point.clone().map(Cow::Owned),
                    targets: fragment.targets.clone(),
                }),
                None => None,
            },
            zero_initialize_workgroup_memory: entry.zero_initialize_workgroup_memory,
        }),
        Entry::Compute(entry) => Pipeline::Compute(ComputePipelineDescriptor {
            label: entry.label.clone().map(Cow::Owned),
            layout: layout(&entry.layout),
            immediate_size: entry.immediate_size,
            shader: shader(&entry.stage.shader)?,
            shader_defs: entry.stage.defs.clone(),
            entry_point: entry.stage.entry_point.clone().map(Cow::Owned),
            zero_initialize_workgroup_memory: entry.zero_initialize_workgroup_memory,
        }),
    };
    Ok((pipeline, shaders))
}

enum State {
    /// Shaders load in the main world.
    Waiting(Vec<Handle<Shader>>),
    Loaded,
    Render(CachedRenderPipelineId),
    Compute(CachedComputePipelineId),
    /// Ready, or failed with a warning. A stale entry does not block loading.
    Done,
}
struct Listed {
    label: String,
    pipeline: Pipeline,
    state: State,
}
#[derive(Default)]
struct ListData {
    listed: Vec<Listed>,
    /// Every entry is `Done`.
    done: bool,
}
#[derive(Resource, Clone, Default, ExtractResource)]
struct PrewarmList(Arc<Mutex<ListData>>);

fn bundled() -> Vec<Entry> {
    serde_json::from_str(BUNDLED).expect("prewarm_pipelines.json is a list of pipeline entries")
}

pub(crate) fn install(app: &mut App) {
    app.init_resource::<PrewarmList>()
        .add_plugins(ExtractResourcePlugin::<PrewarmList>::default())
        .add_systems(Startup, load_list)
        .add_systems(Update, load_shaders);
}

fn load_list(server: Res<AssetServer>, list: Res<PrewarmList>) {
    let mut data = list.0.lock().unwrap();
    for entry in bundled() {
        let label = match &entry {
            Entry::Render(entry) => entry.label.clone(),
            Entry::Compute(entry) => entry.label.clone(),
        }
        .unwrap_or_else(|| "unlabeled".into());
        match descriptor(&entry, &server) {
            Ok((pipeline, shaders)) => data.listed.push(Listed {
                label,
                pipeline,
                state: State::Waiting(shaders),
            }),
            Err(error) => warn!("Prewarm list entry {label:?} skipped: {error}"),
        }
    }
    data.done = data.listed.is_empty();
}

fn load_shaders(server: Res<AssetServer>, list: Res<PrewarmList>) {
    let mut data = list.0.lock().unwrap();
    if data.done {
        return;
    }
    for listed in &mut data.listed {
        let State::Waiting(shaders) = &listed.state else {
            continue;
        };
        let mut loaded = true;
        for shader in shaders {
            match server.get_recursive_dependency_load_state(shader.id()) {
                // Uuid shaders are added by plugins and have no load state.
                None | Some(RecursiveDependencyLoadState::Loaded) => {}
                Some(RecursiveDependencyLoadState::Failed(error)) => {
                    warn!(
                        "Prewarm list entry {:?} skipped: shader {:?} failed: {error}",
                        listed.label,
                        shader.path().map(ToString::to_string),
                    );
                    listed.state = State::Done;
                    break;
                }
                Some(_) => loaded = false,
            }
        }
        if loaded && matches!(listed.state, State::Waiting(_)) {
            listed.state = State::Loaded;
        }
    }
}

/// Queues the listed pipelines whose shaders loaded. True when each one is ready or failed.
/// Call it in the render world while loading.
pub(crate) fn ready(world: &World) -> bool {
    let Some(list) = world.get_resource::<PrewarmList>() else {
        return true;
    };
    let mut data = list.0.lock().unwrap();
    if data.done {
        return true;
    }
    let cache = world.resource::<PipelineCache>();
    let mut done = true;
    for listed in &mut data.listed {
        let state = match listed.state {
            State::Loaded => {
                listed.state = match &listed.pipeline {
                    Pipeline::Render(descriptor) => {
                        State::Render(cache.prewarm_render_pipeline(descriptor.clone()))
                    }
                    Pipeline::Compute(descriptor) => {
                        State::Compute(cache.prewarm_compute_pipeline(descriptor.clone()))
                    }
                };
                done = false;
                continue;
            }
            State::Render(id) => cache.get_render_pipeline_state(id),
            State::Compute(id) => cache.get_compute_pipeline_state(id),
            State::Waiting(_) => {
                done = false;
                continue;
            }
            State::Done => continue,
        };
        match state {
            CachedPipelineState::Ok(_) => listed.state = State::Done,
            CachedPipelineState::Err(
                ShaderCacheError::ShaderNotLoaded(_)
                | ShaderCacheError::ShaderImportNotYetAvailable,
            ) => done = false,
            CachedPipelineState::Err(error) => {
                warn!("Prewarm list entry {:?} failed: {error}", listed.label);
                listed.state = State::Done;
            }
            _ => done = false,
        }
    }
    if done {
        info!("Prewarm list pipelines ready: {}", data.listed.len());
        data.done = true;
    }
    done
}

/// Entries of the pipelines that compiled during play in this run.
static PLAYED: Mutex<Vec<Entry>> = Mutex::new(Vec::new());

pub(crate) fn record(entry: Entry) {
    PLAYED.lock().unwrap().push(entry);
}

/// The bundled list and this run's play pipelines, as the next `prewarm_pipelines.json`.
#[wasm_bindgen::prelude::wasm_bindgen]
pub fn prewarm_pipeline_list() -> String {
    let mut list = bundled();
    for entry in PLAYED.lock().unwrap().iter() {
        if !list.contains(entry) {
            list.push(entry.clone());
        }
    }
    serde_json::to_string_pretty(&list).expect("prewarm entries serialize")
}
