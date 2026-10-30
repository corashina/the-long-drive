pub(crate) use simulation::cpu_profile;
mod frame_limit;
mod performance;
pub(crate) mod save;
mod sim_link;
use crate::browser::*;
use crate::interaction::labels::vehicle_name;
use crate::presentation::{
    self,
    camera::DriveCamera,
    reference::{BodyVisual, CapVisual, ItemVisual, ReferenceVisuals, ValveVisibility},
};
use crate::reference::ReferenceManifest;
use crate::simulation::FrameInput;
use crate::vehicle::VehiclePose;
use bevy::asset::{AssetMetaCheck, LoadState, RecursiveDependencyLoadState};
use bevy::ecs as bevy_ecs;
use bevy::gltf::GltfExtras;
use bevy::prelude::*;
use presentation::sim_frame::SimFrameResource;
use simulation::boundary::MouseLook;
use simulation::boundary::{AudioCall, HudBlock, PoseRow, SimCommand, StateBlock};
use simulation::host::game_now;
use simulation::world::metrics::BUDGET_LABELS;
use std::sync::Arc;
use wasm_bindgen::prelude::*;

#[derive(Resource)]
pub(crate) struct RuntimeReference(pub Arc<ReferenceManifest>);
#[derive(Resource)]
pub(crate) struct Drive {
    link: sim_link::SimLink,
    /// Renderer setup values of the simulation.
    startup: simulation::boundary::SimStartup,
    /// Sequence of the last sent command.
    seq: u64,
    /// Sequence of the last command with menu actions. Older frames have a stale pause flag.
    action_seq: u64,
    world_ms: f64,
    /// Camera position of the last rendered frame. The simulation assigns radio voices by it.
    pub listener: Option<[f32; 3]>,
    pub(crate) ready: bool,
    /// Mouse of commands that no step consumed yet.
    look: MouseLook,
    /// `PlayerSettings::look` of the last browser settings.
    look_scale: [f32; 2],
    /// Player camera of this frame, with the look of the latest mouse.
    view: VehiclePose,
    /// `view` position minus the current step camera position.
    view_offset: [f32; 3],
    last_inventory: Option<([u32; 4], u8, u32)>,
    last_crosshair: Option<(u32, u32)>,
    /// Renderer input of the next world step.
    world: simulation::boundary::WorldCommand,
}

impl Drive {
    /// Player body `pose` at the rendered camera. It moves with the camera interpolation.
    pub(crate) fn rendered_body_pose(&self, mut pose: VehiclePose) -> VehiclePose {
        for i in 0..3 {
            pose.position[i] += self.view_offset[i];
        }
        pose
    }
}

/// Manifest fields the browser shell reads. Copies them without parsing the world records.
#[derive(serde::Deserialize, serde::Serialize)]
struct ManifestShell<'a> {
    #[serde(borrow)]
    app_id: &'a serde_json::value::RawValue,
    #[serde(borrow)]
    build_id: &'a serde_json::value::RawValue,
    #[serde(borrow)]
    source_hashes: &'a serde_json::value::RawValue,
    #[serde(borrow)]
    driving: &'a serde_json::value::RawValue,
    #[serde(borrow)]
    settings: &'a serde_json::value::RawValue,
    #[serde(borrow)]
    language: &'a serde_json::value::RawValue,
    #[serde(borrow)]
    ui: &'a serde_json::value::RawValue,
    #[serde(borrow)]
    sleep: &'a serde_json::value::RawValue,
    #[serde(borrow)]
    audio: &'a serde_json::value::RawValue,
    #[serde(borrow)]
    content_base: &'a serde_json::value::RawValue,
    #[serde(borrow)]
    file_catalog: &'a serde_json::value::RawValue,
    #[serde(borrow)]
    items: &'a serde_json::value::RawValue,
    #[serde(borrow)]
    world: WorldShell<'a>,
    #[serde(borrow)]
    clothing: ClothingShell<'a>,
}
/// The outfit defaults: the segments of each player character.
#[derive(serde::Deserialize, serde::Serialize)]
struct ClothingShell<'a> {
    #[serde(borrow)]
    characters: Vec<CharacterShell<'a>>,
}
#[derive(serde::Deserialize, serde::Serialize)]
struct CharacterShell<'a> {
    #[serde(borrow)]
    segments: &'a serde_json::value::RawValue,
}
#[derive(serde::Deserialize, serde::Serialize)]
struct WorldShell<'a> {
    #[serde(borrow)]
    draw_distance: &'a serde_json::value::RawValue,
    #[serde(borrow)]
    mirrors: &'a serde_json::value::RawValue,
    #[serde(borrow, skip_serializing)]
    startup: Vec<StartupShell<'a>>,
    /// Start car names in export order. The Game mode start car list shows them.
    #[serde(skip_deserializing)]
    start_cars: Vec<String>,
}
#[derive(serde::Deserialize)]
struct StartupShell<'a> {
    group: &'a str,
    start_car: bool,
}

#[wasm_bindgen]
pub fn manifest_shell(reference_json: &[u8]) -> Result<String, JsValue> {
    let mut shell: ManifestShell =
        serde_json::from_slice(reference_json).map_err(|e| JsValue::from_str(&e.to_string()))?;
    // Car09 has two start fields. The second one with a name shows "(2)".
    for startup in shell.world.startup.iter().filter(|s| s.start_car) {
        let name = vehicle_name(startup.group).unwrap_or(startup.group);
        let name = if shell.world.start_cars.iter().any(|n| n == name) {
            format!("{name} (2)")
        } else {
            name.to_owned()
        };
        shell.world.start_cars.push(name);
    }
    serde_json::to_string(&shell).map_err(|e| JsValue::from_str(&e.to_string()))
}

/// The Game mode from the browser. Biome values above 3 read as Mixed.
/// The start car is -1 (Random) or an index into the manifest start placements.
fn game_mode(reference: &ReferenceManifest) -> crate::world::GameMode {
    crate::world::GameMode::from_menu(
        &reference.world,
        game_biome(),
        game_start_car(),
        game_seed(),
    )
}

#[wasm_bindgen]
/// `startup` is the encoded `SimStartup` of the simulation worker.
pub fn run(reference_json: &[u8], startup: &[u8]) -> Result<(), JsValue> {
    let _profile = cpu_profile::Timer::new(cpu_profile::Stage::Startup);
    console_error_panic_hook::set_once();
    let reference = Arc::new(
        ReferenceManifest::from_runtime_json(reference_json)
            .map_err(|e| JsValue::from_str(&e.to_string()))?,
    );
    if let Err(error) = crate::reference::AudioReference::from_value(&reference.audio, &reference) {
        game_audio_error(&error.to_string());
    }
    let startup =
        simulation::boundary::decode_startup(startup).map_err(|e| JsValue::from_str(&e.0))?;
    let mut ledger = crate::ownership::PresentationLedger::default();
    let (fixed, graph) = crate::ownership::fixed::admit_control(
        &mut ledger,
        presentation::readiness::control_bytes(),
        presentation::readiness::RendererStateHandle::default,
    )
    .map_err(|e| JsValue::from_str(&e.to_string()))?;
    let allocation = presentation::world::runtime::SourcePresentationAllocation::bootstrap(
        startup.session,
        fixed,
        ledger,
    );
    App::new()
        .insert_resource(graph)
        .insert_resource(allocation)
        .insert_resource(RuntimeReference(reference.clone()))
        .init_resource::<save::SaveRequests>()
        .insert_resource(Drive {
            view: startup.view,
            link: sim_link::SimLink::default(),
            startup,
            seq: 0,
            action_seq: 0,
            world_ms: 0.,
            listener: None,
            ready: false,
            look: MouseLook::default(),
            look_scale: simulation::reference::PlayerSettings::defaults(&reference).look(),
            view_offset: [0.; 3],
            last_inventory: None,
            last_crosshair: None,
            world: Default::default(),
        })
        .insert_resource(ClearColor(Color::BLACK))
        .init_resource::<crate::render_scale::SceneScaleSetting>()
        .add_systems(Startup, frame_limit::keep_proxy)
        .add_systems(PreUpdate, (update_scene_scale, frame_limit::update))
        .init_resource::<presentation::shadow_quality::ShadowQuality>()
        .add_systems(PreUpdate, presentation::shadow_quality::poll)
        .add_plugins(presentation::source_io::SourceIoPlugin::new(
            reference.content_base.clone(),
            Arc::clone(&reference.file_catalog),
        ))
        .add_plugins(
            DefaultPlugins
                .set(bevy::render::RenderPlugin {
                    render_creation: {
                        let settings = bevy::render::settings::WgpuSettings::default();
                        bevy::render::settings::WgpuSettings {
                            features: settings.features | bevy::render::render_resource::WgpuFeatures::TEXTURE_COMPRESSION_BC,
                            ..settings
                        }.into()
                    },
                    ..default()
                })
                .set(AssetPlugin {
                    file_path: reference.content_base.clone(),
                    meta_check: AssetMetaCheck::Never,
                    ..default()
                })
                .set(WindowPlugin {
                    primary_window: Some(Window {
                        title: "The Long Slop".into(),
                        canvas: Some("#game".into()),
                        fit_canvas_to_parent: true,
                        prevent_default_event_handling: true,
                        ..default()
                    }),
                    ..default()
                }),
        )
        .add_plugins(presentation::readiness::install)
        .add_plugins((
            presentation::lighting::SourceLightingPlugin,
            presentation::materials::SourceMaterialPlugin,
            presentation::plates::PlateTextPlugin,
            presentation::conditions::ConditionPlugin,
            presentation::source_lighting::SourceLightingPlugin,
            presentation::frame_graph::SourceFramePlugin,
            presentation::particles::SourceParticlePlugin,
            presentation::decals::SourceDecalPlugin,
            presentation::world::prototypes::SourcePrototypePlugin,
            presentation::world::pipelines::WorldPipelinePlugin,
            presentation::figure_stage::FigureStagePlugin,
        ))
        .init_resource::<presentation::outfit_preview::OutfitPreview>()
        .init_resource::<presentation::menu_figure::MenuFigure>()
        .init_resource::<presentation::world::SourceWorldVisuals>()
        .init_resource::<presentation::world::instances::SourceInstances>()
        .init_resource::<presentation::computer::ComputerScreens>()
        .init_resource::<SimFrameResource>()
        .init_resource::<presentation::world::grass_jobs::GrassJobs>()
        .init_resource::<crate::presentation::world_metrics::MeshMeasurements>()
        .add_systems(
            Last,
            (
                crate::presentation::world_metrics::collect_mesh_bytes,
                crate::presentation::world_metrics::publish,
                performance::publish,
            )
                .chain(),
        )
        .add_systems(Startup, setup)
        .add_systems(
            Update,
            (
                presentation::readiness::observe_main_readiness.before(check_ready),
                check_ready,
                select_world_views,
                read_input,
                presentation::lighting::bind_lighting,
                presentation::lighting::lighting_readiness,
                (fill_frame, prepare_world, sync_instances).chain(),
                save::dispatch,
                sync_imported,
                presentation::reference::sync_home_cable,
                (
                    presentation::consumables::warm_up,
                    presentation::consumables::sync,
                )
                    .chain(),
                (
                    presentation::spawned::sync,
                    presentation::cigarettes::sync,
                    presentation::light_dither::sync,
                    presentation::outfit::poll,
                    presentation::remote_players::poll,
                    presentation::player_body::sync,
                    presentation::remote_players::sync,
                    presentation::ragdoll::sync,
                    presentation::outfit_preview::sync,
                    presentation::menu_figure::sync,
                    presentation::sky_traffic::sync,
                    presentation::creatures::sync,
                )
                    .chain(),
                presentation::lighting::sync_control_visuals,
                presentation::reference::sync_inventory,
                update_camera,
                presentation::frame_graph::sync_sky_camera,
                publish_loading,
                update_hud,
            )
                .chain(),
        )
        .add_systems(
            PostUpdate,
            publish_audio
                .after(bevy::transform::TransformSystems::Propagate)
                .after(bevy::camera::visibility::VisibilitySystems::VisibilityPropagate),
        )
        .add_systems(
            PostUpdate,
            presentation::lighting::sync_lighting
                .after(bevy::transform::TransformSystems::Propagate)
                .before(bevy::light::SimulationLightSystems::AssignLightsToClusters)
                .before(bevy::light::SimulationLightSystems::UpdateDirectionalLightCascades)
                .before(bevy::light::SimulationLightSystems::UpdateLightFrusta),
        )
        .run();
    Ok(())
}

fn setup(
    mut commands: Commands,

    mut images: ResMut<Assets<Image>>,
    server: Res<AssetServer>,
    gltf_extensions: Res<bevy::gltf::extensions::GltfExtensionHandlers>,
    reference: Res<RuntimeReference>,
    drive: Res<Drive>,
    antialiasing: Res<presentation::antialiasing::Antialiasing>,
) {
    commands.insert_resource(presentation::consumables::ConsumableVisuals::default());
    commands.insert_resource(presentation::spawned::SpawnedVisuals::default());
    commands.insert_resource(presentation::player_body::PlayerBody::default());
    commands.insert_resource(presentation::remote_players::RemoteLooks::default());
    commands.insert_resource(presentation::remote_players::RemoteFigures::default());
    commands.insert_resource(presentation::ragdoll::RagdollBody::default());
    commands.insert_resource(presentation::outfit::Outfit::defaults(
        &reference.0.clothing.characters,
    ));
    commands.insert_resource(presentation::sky_traffic::SkyTrafficVisuals::default());
    commands.insert_resource(presentation::creatures::CreatureVisuals::default());
    if let Err(error) =
        presentation::materials::load_materials(&mut commands, &server, &reference.0)
    {
        game_failure(&error.to_string());
        return;
    }
    presentation::lighting::load_lighting(&mut commands, &server, &reference.0.lighting);
    presentation::lighting_assets::load_lighting(&mut commands, &server, Arc::clone(&reference.0));
    presentation::lighting::load_lamp_materials(&mut commands, &gltf_extensions, &reference.0);
    presentation::reference::load(&mut commands, &server, &reference.0);
    match presentation::fixed_assets::SourceFixedAssets::new(&reference.0) {
        Ok(fixed) => commands.insert_resource(fixed),
        Err(error) => {
            game_failure(&error.to_string());
            return;
        }
    };
    presentation::frame_graph::spawn_cameras(
        &mut commands,
        &mut images,
        &reference.0,
        presentation::transform(drive.startup.view),
        drive.startup.fov_degrees,
        drive.startup.far_plane,
        *antialiasing,
    );
}

fn update_scene_scale(
    mut scale: ResMut<crate::render_scale::SceneScaleSetting>,
    status: Res<presentation::readiness::RendererStateHandle>,
) {
    match crate::render_scale::SceneScale::from_percent(game_scene_scale()) {
        Ok(value) => {
            if scale.0 != value {
                scale.0 = value;
            }
        }
        Err(error) => {
            status.fail(error.into());
        }
    }
}

fn check_ready(
    mut commands: Commands,
    server: Res<AssetServer>,
    gltfs: Res<Assets<bevy::gltf::Gltf>>,
    reference: Res<RuntimeReference>,
    mut drive: ResMut<Drive>,
    mut sim: ResMut<SimFrameResource>,
    mut visuals: ResMut<ReferenceVisuals>,
    extras: Query<(Entity, &GltfExtras)>,
    primitives: Query<(Entity, &ChildOf), With<Mesh3d>>,
    mut renderer: presentation::readiness::RendererReadinessAccess,
    lighting: (
        Res<presentation::lighting::SourceLighting>,
        Res<presentation::fixed_assets::SourceFixedAssets>,
    ),
    materials: Res<presentation::materials::SourceMaterials>,
    plates: (
        Res<presentation::plates::PlateTextStatus>,
        Res<presentation::particles::ParticleRender>,
    ),
) {
    let (lighting, fixed) = lighting;
    let (plates, particles) = plates;
    if let Some(error) = sim.failure.take() {
        game_failure(&error);
        visuals.failed = true;
    }
    let state = sim.frame.state;
    let prepared = (|| -> bool {
        if renderer.failure().is_some() {
            return false;
        }
        if let Some(error) = plates.failure() {
            game_failure(error);
            visuals.failed = true;
        }
        if visuals.failed {
            return false;
        }
        if let presentation::lighting::RenderingState::Failed(error) = &lighting.state {
            game_failure(error);
            visuals.failed = true;
            return false;
        }
        if let presentation::materials::RenderingState::Failed(error) = &materials.state {
            game_failure(error);
            visuals.failed = true;
            return false;
        }
        if drive.ready {
            return true;
        }
        if !fixed.payloads_ready() || !materials.fixed_textures_ready() {
            return false;
        }
        if state.flags & StateBlock::STORAGE_READY == 0 {
            return false;
        }
        for (path, handle) in &visuals.handles {
            let Some((load, _, recursive)) = server.get_load_states(handle.id()) else {
                return false;
            };
            if let LoadState::Failed(error) = load {
                game_failure(&format!("Cannot load {path}: {error}"));
                visuals.failed = true;
                return false;
            }
            if let RecursiveDependencyLoadState::Failed(error) = recursive {
                game_failure(&format!("Cannot load dependencies for {path}: {error}"));
                visuals.failed = true;
                return false;
            }
            if !server.is_loaded_with_dependencies(handle.id()) || !gltfs.contains(handle.id()) {
                return false;
            }
            if gltfs.get(handle).unwrap().scenes.is_empty() {
                game_failure(&format!("Required source scene 0 is missing from {path}"));
                visuals.failed = true;
                return false;
            }
        }
        if !lighting.materials_ready {
            return false;
        }
        if !visuals.spawned {
            presentation::reference::spawn_scenes(
                &mut commands,
                &reference.0,
                &mut visuals,
                &gltfs,
            );
            return false;
        }
        match presentation::reference::bind(
            &mut commands,
            &reference.0,
            &mut visuals,
            &extras,
            &primitives,
        ) {
            Ok(true) => {
                match &lighting.state {
                    presentation::lighting::RenderingState::Loading => {
                        return false;
                    }
                    presentation::lighting::RenderingState::Failed(error) => {
                        game_failure(error);
                        visuals.failed = true;
                        return false;
                    }
                    presentation::lighting::RenderingState::Ready => {}
                }
                if materials.state != presentation::materials::RenderingState::Ready {
                    return false;
                }
                if state.flags & StateBlock::WORLD_READY == 0 || !lighting.road_lamps.ready() {
                    return false;
                }
                if !plates.ready() || !particles.prepared() {
                    return false;
                }
                true
            }
            Ok(false) => false,
            Err(error) => {
                game_failure(&error);
                visuals.failed = true;
                false
            }
        }
    })();
    let key = crate::render_readiness::ContentKey {
        session: state.session,
        selection_revision: state.selection_revision,
    };
    let preparation = if prepared {
        crate::render_readiness::ContentPreparation::Prepared(key)
    } else {
        crate::render_readiness::ContentPreparation::Pending(key)
    };
    match renderer.check(presentation::readiness::RendererContentCheck {
        preparation,
        history_revision: state.motion_revision,
    }) {
        crate::render_readiness::RendererStatus::Loading => {}
        crate::render_readiness::RendererStatus::Ready => {
            if !drive.ready {
                drive.ready = true;
                game_ready();
            }
        }
        crate::render_readiness::RendererStatus::Failed(error) => {
            game_failure(&error);
            visuals.failed = true;
        }
    }
}

fn read_input(
    reference: Res<RuntimeReference>,
    time: Res<Time<Real>>,
    mut sim: ResMut<SimFrameResource>,
    mut drive: ResMut<Drive>,
    mut requests: ResMut<save::SaveRequests>,
    screen: Res<presentation::screen_effects::ScreenEffects>,
) {
    let _profile = cpu_profile::Timer::new(cpu_profile::Stage::Input);
    for json in sim.sandbox.drain(..) {
        game_sandbox_reply(&json);
    }
    let hud = &sim.frame.hud;
    let [steer, throttle, brake, clutch, handbrake] = hud.controls;
    game_driving_state(
        hud.gear,
        hud.assists,
        hud.flags & HudBlock::DRIVING != 0,
        steer,
        throttle,
        brake,
        clutch,
        handbrake,
    );
    let actions = game_actions() & SimCommand::ACTIONS;
    let hold = requests.pending();
    // A pending save keeps the browser assists and settings for a later frame.
    let player_settings = if hold {
        None
    } else {
        game_player_settings()
            .and_then(|settings| {
                serde_json::from_str::<simulation::reference::PlayerSettings>(&settings).ok()
            })
            .filter(|settings| settings.valid(&reference.0.settings))
    };
    if let Some(settings) = player_settings {
        drive.look_scale = settings.look();
    }
    let held = game_keys();
    let player = game_player_keys();
    let mut input = FrameInput {
        held,
        player_held: player as u16,
        player_pressed: (player >> 16) as u16,
        player_released: game_player_released() as u16,
        pressed: game_pressed(),
        released: game_released(),
        inventory_held: game_inventory_keys() as u8 & 15,
        inventory_released: (game_inventory_keys() >> 4) as u8 & 15,
        mouse: [
            game_mouse_x() * drive.look_scale[0],
            game_mouse_y() * drive.look_scale[1],
        ],
        drive_mouse: [game_drive_mouse_x(), -game_drive_mouse_y()],
        wheel: game_wheel(),
        carry: crate::simulation::CarryDeltas::default(),
        typing: if hud.flags & HudBlock::TYPING != 0 {
            let mut typing = crate::simulation::TypingInput {
                keys: game_typing_keys(),
                ..Default::default()
            };
            typing.len = game_typed_text(&mut typing.text).min(typing.text.len() as u32) as u8;
            typing
        } else {
            crate::simulation::TypingInput::default()
        },
        gamepad: crate::simulation::GamepadFrame::default(),
    };
    game_gamepad(&mut input.gamepad.axes, &mut input.gamepad.buttons);
    // A pending save keeps the browser assists, settings and options for a later frame.
    let (assists, settings, options) = if hold {
        (None, None, None)
    } else {
        let requested = game_driving_assists();
        (
            (requested & 4 != 0).then_some(crate::reference::DrivingAssists {
                automatic_clutch: requested & 1 != 0,
                automatic_gears: requested & 2 != 0,
            }),
            game_driving_settings().and_then(|settings| serde_json::from_str(&settings).ok()),
            game_options().map(crate::world::GameOptions::from_bits),
        )
    };
    drive.seq += 1;
    let seq = drive.seq;
    if actions != 0 {
        drive.action_seq = seq;
    }
    drive.look.send(seq, input.mouse);
    let command = SimCommand {
        seq,
        delta_seconds: time.delta_secs_f64(),
        input,
        actions,
        ready: drive.ready,
        hold,
        assists,
        settings,
        options,
        player_settings,
        old_heat_haze: screen.old_heat_haze,
        calendar: local_calendar(),
        measure: game_measure_requested(),
        world: std::mem::take(&mut drive.world),
        save: requests.take_commands(),
        listener: drive.listener,
        // A pending save drops the input, so the commands wait in the browser.
        sandbox: if hold { None } else { game_sandbox_take() },
    };
    drive.link.send(command);
}
/// World camera of the last complete view snapshot. `None`: startup and load use the root camera.
fn world_snapshot<'a>(
    views: &'a presentation::view_frame::SourceViewFrame,
    epoch: &presentation::view_frame::SourceSceneEpoch,
    session: u64,
) -> Option<&'a presentation::view_frame::ViewSnapshot> {
    // Preparation consumes one completed pose/FOV pair.
    let coherent = views.session == session
        && views.scene == epoch.0
        && views.views[..presentation::surface_views::WORLD_VIEWS]
            .iter()
            .all(Option::is_some);
    coherent.then(|| {
        views.views[0]
            .as_ref()
            .expect("complete world camera snapshot")
    })
}
/// Direction of the main light, in source space.
pub(crate) fn light_forward(cycle: &simulation::lighting::CycleOutput) -> [f32; 3] {
    let [x, y, z] = cycle.directional_euler_degrees.map(f32::to_radians);
    let forward = Quat::from_euler(EulerRot::YXZ, -y, -x, z) * Vec3::NEG_Z;
    [forward.x, forward.y, -forward.z]
}
/// Writes the required camera views of the next world step into the world command.
fn select_world_views(
    reference: Res<RuntimeReference>,
    mut drive: ResMut<Drive>,
    sim: Res<SimFrameResource>,
    views: Res<presentation::view_frame::SourceViewFrame>,
    mut required_views: Local<Option<crate::world::RequiredViews>>,
    epoch: Res<presentation::view_frame::SourceSceneEpoch>,
) {
    let drive = &mut *drive;
    drive.world.views.clear();
    let state = &sim.frame.state;
    let Some(first) = world_snapshot(&views, &epoch, state.session) else {
        *required_views = None;
        return;
    };
    let forward = light_forward(&sim.frame.lighting.cycle);
    let origin = state.stream_origin;
    let mask = reference.0.render_lighting.lights[0].mask;
    let result = (|| -> Result<(), crate::reference::ReferenceError> {
        let first = presentation::view_frame::required_view(first, origin, forward, mask)?;
        let required =
            required_views.get_or_insert_with(|| crate::world::RequiredViews::new(first));
        required.reset(first);
        presentation::view_frame::append_probe_required_views(
            required,
            &reference.0.render_lighting.probes,
            &views.probe_position,
            &views.probe_enabled,
            origin,
            forward,
            mask,
        )?;
        drive.world.views.extend_from_slice(required.as_slice());
        Ok(())
    })();
    if let Err(error) = result {
        game_failure(&error.to_string());
        drive.world.failure.get_or_insert(error.to_string());
    }
}
/// One renderer step on the world copy after the world step of the frame.
fn prepare_world(
    reference: Res<RuntimeReference>,
    mut drive: ResMut<Drive>,
    mut sim: ResMut<SimFrameResource>,
    mut presentation: presentation::world::runtime::WorldPresentation,
    camera: Query<(&Projection, &Transform), With<DriveCamera>>,
    views: Res<presentation::view_frame::SourceViewFrame>,
    epoch: Res<presentation::view_frame::SourceSceneEpoch>,
    windows: Query<&Window>,
) {
    let drive = &mut *drive;
    drive.world_ms = sim.frame.state.perf.world_ms;
    let Ok((projection, camera_transform)) = camera.single() else {
        return;
    };
    // The renderer world exists after the first frame from the simulation worker.
    let Some(origin) = presentation.origin() else {
        return;
    };
    let started = game_now();
    let snapshot = world_snapshot(&views, &epoch, sim.frame.state.session);
    let camera_world = snapshot.map_or_else(
        || camera_transform.to_matrix(),
        |view| view.view_from_world.inverse(),
    );
    let clip_from_view = snapshot.map_or_else(
        || projection.get_clip_from_view(),
        |view| view.raster_projection,
    );
    let fov_degrees = snapshot.map_or_else(
        || match projection {
            Projection::Perspective(p) => p.fov.to_degrees(),
            _ => 0.,
        },
        |view| view.fov_degrees,
    );
    let position = Vec3::from_array(
        origin
            .absolute(camera_world.w_axis.truncate().to_array())
            .map(|v| v as f32),
    );
    let source_from_runtime = Mat4::from_scale(Vec3::new(1., 1., -1.));
    let mut camera_world = camera_world;
    camera_world.w_axis = position.extend(1.);
    let clip = clip_from_view * camera_world.inverse() * source_from_runtime;
    let rows = clip.transpose();
    let planes = [
        rows.w_axis + rows.x_axis,
        rows.w_axis - rows.x_axis,
        rows.w_axis + rows.y_axis,
        rows.w_axis - rows.y_axis,
    ]
    .map(|p| p / p.truncate().length());
    let view = grass_mesh::GrassView {
        position: position * Vec3::new(1., 1., -1.),
        screen_height: windows.single().map_or(1., |w| w.physical_height() as f32),
        clip_planes: planes,
    };
    let lod_view = presentation::world::lod::LodView {
        position: snapshot.map_or_else(
            || camera_transform.translation,
            |view| view.view_from_world.inverse().w_axis.truncate(),
        ),
        fov_degrees,
        ortho_size: match projection {
            Projection::Orthographic(p) => Some(p.area.height() * 0.5),
            _ => None,
        },
    };
    let grant = sim.grant.take();
    let frame = &sim.frame;
    if let Err(error) = presentation.prepare_frame(
        &reference.0,
        grant,
        view,
        frame.lighting.cycle.shader_time_seconds as f32,
        lod_view,
        frame.lighting.road_lamp.enabled,
        &frame.lights,
        &mut drive.world,
    ) {
        game_failure(&error.to_string());
        drive.world.failure.get_or_insert(error.to_string());
    }
    drive.world_ms += game_now() - started;
}

pub(crate) fn sync_imported(
    frame: Res<SimFrameResource>,
    mut query: Query<(
        &BodyVisual,
        &mut Transform,
        &mut Visibility,
        Option<&ValveVisibility>,
    )>,
    mut items: Query<
        (
            &ItemVisual,
            &mut Transform,
            &mut Visibility,
            Option<&ValveVisibility>,
        ),
        Without<BodyVisual>,
    >,
    time: Res<Time<Real>>,
    mut caps: Query<(&mut CapVisual, &mut Transform), (Without<BodyVisual>, Without<ItemVisual>)>,
    mut visibility: Query<
        (&ValveVisibility, &mut Visibility),
        (Without<BodyVisual>, Without<ItemVisual>),
    >,
) {
    presentation::reference::sync(&frame, &mut query);
    presentation::reference::sync_items(&frame, &mut items);
    // Source DoMove advances by scaled Time.deltaTime.
    let state = frame.frame.state;
    let dt = if state.flags & StateBlock::PAUSED != 0 {
        0.
    } else {
        time.delta_secs() * state.time_scale
    };
    presentation::reference::sync_caps(&frame.frame, dt, &mut caps, &mut visibility);
}

/// Applies the structure views of the new frame.
fn sync_instances(
    reference: Res<RuntimeReference>,
    mut drive: ResMut<Drive>,
    mut sim: ResMut<SimFrameResource>,
    mut presentation: presentation::world::runtime::WorldPresentation,
) {
    if let Err(error) = presentation.sync_instances(&reference.0, &mut sim) {
        game_failure(&error.to_string());
        drive.world.failure.get_or_insert(error.to_string());
    }
}
/// Takes the new simulation frame. Without one, the last state stays and its events go.
fn fill_frame(
    reference: Res<RuntimeReference>,
    mut drive: ResMut<Drive>,
    mut frame: ResMut<SimFrameResource>,
    mut allocation: ResMut<presentation::world::runtime::SourcePresentationAllocation>,
) {
    let drive = &mut *drive;
    match drive.link.receive(&mut frame.frame) {
        Ok(true) => {}
        Ok(false) => {
            frame.frame.clear_events();
            drive.view = drive.look.camera(&frame.frame);
            return;
        }
        Err(error) => {
            game_failure(&error.0);
            return;
        }
    }
    fill(drive, &mut frame);
    allocation.apply_world(&reference.0.world, &frame.frame.world);
}
/// Applies the changes of the new frame.
fn fill(drive: &mut Drive, frame: &mut SimFrameResource) {
    drive.view = drive.look.camera(&frame.frame);
    let current = frame.frame.state.look.position[1];
    drive.view_offset = std::array::from_fn(|i| drive.view.position[i] - current[i]);
    frame.apply_changes();
    frame.index_particles();
}
/// Camera of `state`: the sleep camera, the seated view camera, else the player `view`.
pub(crate) fn frame_camera(state: &StateBlock, view: VehiclePose) -> Transform {
    let pose = |row: PoseRow| presentation::transform(row.pose());
    if state.sleep_camera.flags & PoseRow::ACTIVE != 0 {
        pose(state.sleep_camera)
    } else if state.view_camera.flags & PoseRow::ACTIVE != 0 {
        pose(state.view_camera)
    } else {
        presentation::transform(view)
    }
}
/// `gameState.camera`: 0 first person, 1 outside view, 2 drop cam, 3 map view.
fn view_code(flags: u32) -> u32 {
    if flags & StateBlock::DROP_CAM != 0 {
        2
    } else if flags & StateBlock::MAP_VIEW != 0 {
        3
    } else {
        u32::from(flags & StateBlock::OTHER_VIEW != 0)
    }
}
fn update_camera(
    drive: Res<Drive>,
    sim: Res<SimFrameResource>,
    mut query: Query<(&mut Transform, &mut Projection), With<DriveCamera>>,
) {
    let state = &sim.frame.state;
    for (mut transform, mut projection) in &mut query {
        *transform = frame_camera(state, drive.view);
        presentation::camera_projection::sync_player_projection(
            &mut projection,
            true,
            state.fov_degrees,
        );
    }
}
fn publish_loading(
    reference: Res<RuntimeReference>,
    drive: Res<Drive>,
    sim: Res<SimFrameResource>,
    visuals: Res<ReferenceVisuals>,
    materials: Res<presentation::materials::SourceMaterials>,
    lighting: Res<presentation::lighting::SourceLighting>,
    renderer: presentation::readiness::RendererReadinessAccess,
    plates: Res<presentation::plates::PlateTextStatus>,
    time: Res<Time<Real>>,
    mut last: Local<f64>,
    mut traced: Local<u8>,
) {
    let now = time.elapsed_secs_f64();
    let state = &sim.frame.state;
    let rendering = visuals.bound
        && materials.state == presentation::materials::RenderingState::Ready
        && lighting.state == presentation::lighting::RenderingState::Ready
        && renderer.graph_ready(presentation::readiness::RendererContentCheck {
            preparation: crate::render_readiness::ContentPreparation::Prepared(
                crate::render_readiness::ContentKey {
                    session: state.session,
                    selection_revision: state.selection_revision,
                },
            ),
            history_revision: state.motion_revision,
        })
        && plates.ready();
    if state.flags & StateBlock::GENERATION_READY != 0 && *traced & 1 == 0 {
        game_startup_stage("generation", false);
        *traced |= 1;
    }
    if visuals.bound && *traced & 2 == 0 {
        game_startup_stage("decodeUpload", false);
        *traced |= 2;
    }
    if rendering && *traced & 4 == 0 {
        game_startup_stage("pipeline", false);
        *traced |= 4;
    }
    if drive.ready || visuals.failed || now - *last < 0.5 {
        return;
    }
    *last = now;
    game_loading_progress(
        state.prepared_roads,
        reference.0.world.road.gen_at_start,
        rendering,
        state.flags & StateBlock::WORLD_READY != 0,
    );
    for (budget, (name, unit)) in sim.frame.budgets.iter().zip(BUDGET_LABELS) {
        game_loading_budget(name, budget.used, budget.limit, unit);
    }
}

fn update_hud(mut drive: ResMut<Drive>, mut sim: ResMut<SimFrameResource>) {
    let hud = sim.frame.hud;
    let state = sim.frame.state;
    let stamp = (hud.slots, hud.selected as u8, hud.swap_progress.to_bits());
    if drive.last_inventory != Some(stamp) {
        game_inventory(
            hud.selected,
            hud.slots[0],
            hud.slots[1],
            hud.slots[2],
            hud.slots[3],
            hud.swap_progress,
        );
        drive.last_inventory = Some(stamp);
    }
    let crosshair = (hud.weapon_icon, hud.weapon_color);
    if drive.last_crosshair != Some(crosshair) {
        game_weapon_crosshair(crosshair.0, crosshair.1);
        drive.last_crosshair = Some(crosshair);
    }
    game_survival(
        hud.hp,
        hud.food,
        hud.water,
        hud.bowel,
        hud.bladder,
        hud.opacity,
        hud.flags & HudBlock::DEAD != 0,
        hud.look_scale,
        hud.flags & HudBlock::MENU_DUE != 0,
    );
    game_sleep(
        hud.sleep_phase,
        hud.flags & HudBlock::SLEEP_ELIGIBLE != 0,
        hud.flags & HudBlock::SLEEP_HINT != 0,
        hud.sleep_charge,
        hud.cooldown_remaining,
        hud.sleep_progress,
    );
    let seated = state.flags & StateBlock::SEATED != 0;
    game_update(
        hud.speed_kmh,
        view_code(state.flags),
        hud.focus[0],
        hud.focus[1],
        hud.focus[2],
        hud.contacts,
        state.perf.frame_steps,
        hud.surfaces,
        hud.colliders,
        hud.absolute[0],
        hud.absolute[1],
        hud.absolute[2],
        hud.origin[0],
        hud.origin[1],
        hud.origin[2],
        hud.rebases,
        hud.trip_m,
        f64::from(state.prepared_roads),
        hud.fuel,
        // A frame before the last Continue or pause must not pause the menu again.
        state.flags & StateBlock::PAUSED != 0 && sim.frame.seq_consumed >= drive.action_seq,
        state.flags & StateBlock::STARTED != 0,
        hud.flags & HudBlock::AUTOSAVE_SAFE != 0,
        seated,
    );
    if hud.flags & HudBlock::RADIO != 0 {
        game_radio_feedback(
            hud.radio_volume,
            hud.radio_frequency,
            hud.flags & HudBlock::RADIO_AM != 0,
        );
    }
    let Some(f) = sim.feedback.take() else {
        return;
    };
    game_feedback(
        f.actions,
        f.context,
        &f.name,
        &f.notice,
        f.carrying,
        f.amount,
        f.capacity,
        f.has_name,
        f.no_text,
        f.fluid_word,
        &f.held_name,
        f.throw_charge,
        f.throw_active,
        f.rpm,
        f.ignition,
        f.running,
        f.starter,
        f.seated,
        f.handbrake,
        f.quantities[0],
        f.quantities[1],
        f.quantities[2],
        f.quantities[3],
        f.quantities[4],
        f.quantities[5],
        f.order,
        f.light_control,
        f.lmb_use,
        f.prompts.into(),
        f.computer.into(),
        f.crosshair,
        f.weight,
        f.help.into(),
    );
}

fn publish_audio(
    mut drive: ResMut<Drive>,
    sim: Res<SimFrameResource>,
    visuals: Res<ReferenceVisuals>,
    globals: Query<(&GlobalTransform, &InheritedVisibility)>,
    camera: Query<&GlobalTransform, With<DriveCamera>>,
) {
    let camera = camera
        .single()
        .ok()
        .map(|c| c.to_scale_rotation_translation());
    drive.listener = camera.map(|(_, _, p)| p.to_array());
    for call in &sim.frame.audio {
        match *call {
            AudioCall::Begin {
                session,
                started,
                paused,
                seconds,
            } => game_audio_begin(session, started, paused, seconds),
            AudioCall::SurvivalMaster {
                gain,
                revision,
                seconds,
            } => game_audio_survival_master(gain, revision, seconds),
            AudioCall::PlayerEar {
                db,
                clip,
                ring,
                pan,
            } => game_audio_player_ear(db, clip, ring, pan),
            AudioCall::OriginShift([x, y, z]) => game_audio_origin_shift(x, y, z),
            AudioCall::Listener => {
                if let Some((_, q, p)) = camera {
                    game_audio_listener(p.x, p.y, p.z, q.x, q.y, q.z, q.w);
                }
            }
            AudioCall::SurvivalEmitter {
                kind,
                position: [x, y, z],
            } => game_audio_survival_emitter(kind, x, y, z),
            AudioCall::Survival {
                kind,
                emitter,
                action,
                clip,
                gain,
                pitch,
                position: [x, y, z],
            } => game_audio_survival(kind, emitter, action, clip, gain, pitch, x, y, z),
            AudioCall::NodeEmitter { id, node, alive } => {
                if let Some((global, visibility)) = visuals
                    .nodes
                    .get(&node)
                    .and_then(|&entity| globals.get(entity).ok())
                {
                    let (_, q, p) = global.to_scale_rotation_translation();
                    game_audio_emitter(
                        id,
                        p.x,
                        p.y,
                        p.z,
                        q.x,
                        q.y,
                        q.z,
                        q.w,
                        alive && visibility.get(),
                    );
                }
            }
            AudioCall::Emitter {
                id,
                position: [x, y, z],
                rotation: [qx, qy, qz, qw],
                alive,
            } => game_audio_emitter(id, x, y, z, qx, qy, qz, qw, alive),
            AudioCall::Mixer { id, db } => game_audio_mixer(id, db),
            AudioCall::Route { id, mixer } => game_audio_route(id, mixer),
            AudioCall::Voice {
                id,
                playing,
                gain,
                pitch,
                looping,
                clip,
            } => game_audio_voice(id, playing, gain, pitch, looping, clip),
            AudioCall::Event {
                session,
                sequence,
                binding,
                emitter,
                position: [x, y, z],
                gain,
                pitch,
                seconds,
            } => game_audio_event(
                session, sequence, binding, emitter, x, y, z, gain, pitch, seconds,
            ),
            AudioCall::Radio { emitter, frame } => game_audio_radio(
                emitter,
                frame.power,
                frame.am,
                frame.frequency,
                frame.volume,
                frame.static_gain,
                frame.primary.clip.unwrap_or(0),
                frame.primary.offset_seconds,
                frame.primary.gain,
                frame.primary.next_clip.unwrap_or(0),
                frame.secondary.clip.unwrap_or(0),
                frame.secondary.offset_seconds,
                frame.secondary.gain,
                frame.secondary.next_clip.unwrap_or(0),
            ),
            AudioCall::End => game_audio_end(),
        }
    }
}
