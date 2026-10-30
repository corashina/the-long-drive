use wasm_bindgen::prelude::*;
#[wasm_bindgen]
extern "C" {
    #[wasm_bindgen(js_namespace=window,js_name=gameGrassCreate,catch)]
    pub fn game_grass_create() -> Result<GrassTransport, JsValue>;
    #[wasm_bindgen(js_name = Object)]
    pub type GrassTransport;
    #[wasm_bindgen(method,structural,catch,js_name=initialized)]
    pub fn grass_initialized(this: &GrassTransport) -> Result<bool, JsValue>;
    #[wasm_bindgen(method,structural,catch,js_name=capacityLimit)]
    pub fn grass_capacity_limit(this: &GrassTransport, bytes: f64) -> Result<(), JsValue>;
    #[wasm_bindgen(method,structural,js_name=wasmBytes)]
    pub fn grass_wasm_bytes(this: &GrassTransport) -> f64;
    #[wasm_bindgen(method,structural,catch,js_name=installRaw)]
    pub fn grass_install(
        this: &GrassTransport,
        id: &[u8],
        input: &[u8],
        scratch: u32,
    ) -> Result<(), JsValue>;
    #[wasm_bindgen(method,structural,catch,js_name=installedRaw)]
    pub fn grass_installed(this: &GrassTransport, entity: &[u8]) -> Result<bool, JsValue>;
    #[wasm_bindgen(method,structural,catch,js_name=retireRaw)]
    pub fn grass_retire(this: &GrassTransport, entity: &[u8]) -> Result<bool, JsValue>;
    #[wasm_bindgen(method,structural,catch,js_name=submitRaw)]
    pub fn grass_submit(this: &GrassTransport, id: &[u8], input: &[u8]) -> Result<(), JsValue>;
    #[wasm_bindgen(method,structural,catch,js_name=poll)]
    pub fn grass_poll(this: &GrassTransport) -> Result<i32, JsValue>;
    #[wasm_bindgen(method,structural,catch,js_name=planRaw)]
    pub fn grass_plan(this: &GrassTransport, out: &mut [u32]) -> Result<(), JsValue>;
    #[wasm_bindgen(method,structural,catch,js_name=approveRaw)]
    pub fn grass_approve(this: &GrassTransport, id: &[u8], bytes: f64) -> Result<(), JsValue>;
    #[wasm_bindgen(method,structural,catch,js_name=readRaw)]
    pub fn grass_read(
        this: &GrassTransport,
        anchors: &mut [f32],
        uvs: &mut [f32],
        blades: &mut [f32],
        patches: &mut [u32],
        indices: &mut [u32],
    ) -> Result<(), JsValue>;
    #[wasm_bindgen(method,structural,js_name=packingStarted)]
    pub fn grass_packing_started(this: &GrassTransport);
    #[wasm_bindgen(method,structural,js_name=packingEnded)]
    pub fn grass_packing_ended(this: &GrassTransport);
    #[wasm_bindgen(method,structural,js_name=publicationStartedRaw)]
    pub fn grass_publication_started(this: &GrassTransport, entity: &[u8]);
    #[wasm_bindgen(method,structural,js_name=publicationEndedRaw)]
    pub fn grass_publication_ended(this: &GrassTransport, entity: &[u8], accepted: bool);
    #[wasm_bindgen(method,structural,js_name=discard)]
    pub fn grass_discard(this: &GrassTransport);
    #[wasm_bindgen(method,structural,js_name=dispose)]
    pub fn grass_dispose(this: &GrassTransport);

    #[wasm_bindgen(js_namespace = window, js_name = gameSceneScale)]
    pub fn game_scene_scale() -> u32;
    /// "Shadows" setting: 0 high, 1 medium, 2 low.
    #[wasm_bindgen(js_namespace = window, js_name = gameShadowQuality)]
    pub fn game_shadow_quality() -> u32;
    #[wasm_bindgen(js_name = gameAudioError)]
    pub fn game_audio_error(message: &str);
    #[wasm_bindgen(js_namespace = window, js_name = gameAudioSurvival)]
    pub fn game_audio_survival(
        kind: u32,
        emitter: u32,
        action: u32,
        clip: u32,
        gain: f32,
        pitch: f32,
        x: f32,
        y: f32,
        z: f32,
    );
    #[wasm_bindgen(js_namespace = window, js_name = gameAudioSurvivalEmitter)]
    pub fn game_audio_survival_emitter(kind: u32, x: f32, y: f32, z: f32);
    #[wasm_bindgen(js_namespace = window, js_name = gameAudioSurvivalMaster)]
    pub fn game_audio_survival_master(gain: f32, revision: u32, seconds: f64);
    #[wasm_bindgen(js_namespace = window, js_name = gameAudioPlayerEar)]
    pub fn game_audio_player_ear(db: f32, clip: u32, ring: f32, pan: f32);

    #[wasm_bindgen(js_namespace = window, js_name = gameAudioBegin)]
    pub fn game_audio_begin(session: u32, started: bool, paused: bool, time: f64);
    #[wasm_bindgen(js_namespace = window, js_name = gameAudioOriginShift)]
    pub fn game_audio_origin_shift(x: f32, y: f32, z: f32);
    #[wasm_bindgen(js_namespace = window, js_name = gameAudioListener)]
    pub fn game_audio_listener(x: f32, y: f32, z: f32, qx: f32, qy: f32, qz: f32, qw: f32);
    #[wasm_bindgen(js_namespace = window, js_name = gameAudioEmitter)]
    pub fn game_audio_emitter(
        id: u32,
        x: f32,
        y: f32,
        z: f32,
        qx: f32,
        qy: f32,
        qz: f32,
        qw: f32,
        alive: bool,
    );
    #[wasm_bindgen(js_namespace = window, js_name = gameAudioVoice)]
    pub fn game_audio_voice(
        id: u32,
        playing: bool,
        gain: f32,
        pitch: f32,
        looping: bool,
        clip: u32,
    );
    #[wasm_bindgen(js_namespace = window, js_name = gameAudioEvent)]
    pub fn game_audio_event(
        session: u32,
        sequence: u32,
        binding: u32,
        emitter: u32,
        x: f32,
        y: f32,
        z: f32,
        gain: f32,
        pitch: f32,
        time: f64,
    );
    #[wasm_bindgen(js_namespace = window, js_name = gameAudioMixer)]
    pub fn game_audio_mixer(id: u32, db: f32);
    #[wasm_bindgen(js_namespace = window, js_name = gameAudioRoute)]
    pub fn game_audio_route(id: u32, mixer: u32);
    #[wasm_bindgen(js_namespace = window, js_name = gameAudioRadio)]
    pub fn game_audio_radio(
        emitter: u32,
        power: bool,
        am: bool,
        frequency: f32,
        volume: f32,
        static_gain: f32,
        primary_clip: u32,
        primary_offset: f64,
        primary_gain: f32,
        primary_next: u32,
        secondary_clip: u32,
        secondary_offset: f64,
        secondary_gain: f32,
        secondary_next: u32,
    );
    #[wasm_bindgen(js_namespace = window, js_name = gameAudioCalendar)]
    pub fn game_audio_calendar() -> u32;
    #[wasm_bindgen(js_namespace = window, js_name = gameAudioEnd)]
    pub fn game_audio_end();
    #[wasm_bindgen(js_namespace = window, js_name = gameHasSaveRequest)]
    pub fn game_has_save_request() -> bool;
    #[wasm_bindgen(js_namespace = window, js_name = gameSaveRequest)]
    pub fn game_save_request() -> String;
    #[wasm_bindgen(js_namespace = window, js_name = gameSaveResult)]
    pub fn game_save_result(id: u32, kind: &str, payload: &str, error: &str);

    #[wasm_bindgen(js_namespace = window, js_name = gameLoading)]
    pub fn game_loading();
    #[wasm_bindgen(js_namespace = window, js_name = gameLoadingProgress)]
    pub fn game_loading_progress(roads: u32, total: u32, rendering: bool, world: bool);
    #[wasm_bindgen(js_namespace = window, js_name = gameLoadingBudget)]
    pub fn game_loading_budget(name: &str, used: f64, limit: f64, unit: &str);
    #[wasm_bindgen(js_namespace = window, js_name = gameStartupStage)]
    pub fn game_startup_stage(stage: &str, started: bool);
    #[wasm_bindgen(js_namespace = window, js_name = gameStartupFailureDetails)]
    pub fn game_startup_failure_details(json: &str);
    #[wasm_bindgen(js_namespace=window,js_name=gameSurvival)]
    pub fn game_survival(
        hp: f32,
        food: f32,
        water: f32,
        bowel: f32,
        bladder: f32,
        opacity: f32,
        dead: bool,
        look_scale: f32,
        menu_due: bool,
    );
    #[wasm_bindgen(js_namespace=window,js_name=gameSleep)]
    pub fn game_sleep(
        phase: u32,
        eligible: bool,
        show_hint: bool,
        charge: f32,
        cooldown_remaining: f64,
        progress: f32,
    );
    /// Posts one encoded command to the simulation worker.
    #[wasm_bindgen(js_namespace = window, js_name = gameSimSend)]
    pub fn game_sim_send(command: &[u8]);
    /// Byte length of the received frame. -1: no frame.
    #[wasm_bindgen(js_namespace = window, js_name = gameSimFrameLength)]
    pub fn game_sim_frame_length() -> i32;
    /// Copies the received frame into `out` and drops it.
    #[wasm_bindgen(js_namespace = window, js_name = gameSimRead)]
    pub fn game_sim_read(out: &mut [u8]);
    /// Called in the task that presented the frame, while the canvas still holds it.
    #[wasm_bindgen(js_namespace = window, js_name = gameThumbnailFrame)]
    pub fn game_thumbnail_frame();
    #[wasm_bindgen(js_namespace = window, js_name = gameCpuSubmitted)]
    pub fn game_cpu_submitted(token: u32);
    #[wasm_bindgen(js_namespace = window, js_name = gamePerformanceFrame)]
    pub fn game_performance_frame(physics_ms: f64, world_ms: f64) -> bool;
    #[wasm_bindgen(js_namespace = window, js_name = gamePerformanceCounters)]
    pub fn game_performance_counters(
        steps: u32,
        wheel_contacts: u32,
        bodies: u32,
        colliders: u32,
        roads: u32,
        terrain: u32,
        objects: u32,
        jobs: u32,
        pending: u32,
        cpu_bytes: f64,
        gpu_bytes: f64,
        storms: u32,
        storm_influence: f32,
        storm_particles: u32,
        entities: u32,
    );
    #[wasm_bindgen(js_namespace = window, js_name = gameMeasureRequested)]
    pub fn game_measure_requested() -> bool;
    #[wasm_bindgen(js_namespace = window, js_name = gameMeasurements)]
    pub fn game_measurements(json: &str);
    /// Queued sandbox commands as one JSON array. `None` without commands.
    #[wasm_bindgen(js_namespace = window, js_name = gameSandboxTake)]
    pub fn game_sandbox_take() -> Option<String>;
    #[wasm_bindgen(js_namespace = window, js_name = gameSandboxReply)]
    pub fn game_sandbox_reply(json: &str);

    #[wasm_bindgen(js_namespace = window, js_name = gameReady)]
    pub fn game_ready();
    #[wasm_bindgen(js_namespace = window, js_name = gameFailure)]
    pub fn game_failure(message: &str);
    #[wasm_bindgen(js_namespace = window, js_name = gameKeys)]
    pub fn game_keys() -> u32;
    /// Held player keys in bits 0-15, pressed in bits 16-31.
    #[wasm_bindgen(js_namespace = window, js_name = gamePlayerKeys)]
    pub fn game_player_keys() -> u32;
    #[wasm_bindgen(js_namespace = window, js_name = gamePlayerReleased)]
    pub fn game_player_released() -> u32;
    #[wasm_bindgen(js_namespace = window, js_name = gamePressed)]
    pub fn game_pressed() -> u32;
    #[wasm_bindgen(js_namespace = window, js_name = gameReleased)]
    pub fn game_released() -> u32;
    #[wasm_bindgen(js_namespace = window, js_name = gameInventoryKeys)]
    pub fn game_inventory_keys() -> u32;
    #[wasm_bindgen(js_namespace = window, js_name = gameMouseX)]
    pub fn game_mouse_x() -> f32;
    #[wasm_bindgen(js_namespace = window, js_name = gameMouseY)]
    pub fn game_mouse_y() -> f32;
    #[wasm_bindgen(js_namespace = window, js_name = gameWheel)]
    pub fn game_wheel() -> f32;
    /// Source `Mouse X` and `Mouse Y` axes for mouse driving, outside the look bindings.
    #[wasm_bindgen(js_namespace = window, js_name = gameDriveMouseX)]
    pub fn game_drive_mouse_x() -> f32;
    #[wasm_bindgen(js_namespace = window, js_name = gameDriveMouseY)]
    pub fn game_drive_mouse_y() -> f32;
    #[wasm_bindgen(js_namespace = window, js_name = gameActions)]
    pub fn game_actions() -> u32;
    #[wasm_bindgen(js_namespace = window, js_name = gameDrivingAssists)]
    pub fn game_driving_assists() -> u32;
    /// Pending `DrivingSettings` JSON, or none.
    #[wasm_bindgen(js_namespace = window, js_name = gameDrivingSettings)]
    pub fn game_driving_settings() -> Option<String>;
    /// Pending `PlayerSettings` JSON, or none.
    #[wasm_bindgen(js_namespace = window, js_name = gamePlayerSettings)]
    pub fn game_player_settings() -> Option<String>;
    /// Pending frame limit in frames per second, or none. 0 is no limit. `web/frame-pacer.js` paces the frames.
    #[wasm_bindgen(js_namespace = window, js_name = gameFrameLimit)]
    pub fn game_frame_limit() -> Option<f32>;
    /// Pending `Outfit` JSON, or none.
    #[wasm_bindgen(js_namespace = window, js_name = gameOutfit)]
    pub fn game_outfit() -> Option<String>;
    /// Pending peer outfits JSON `[[slot, outfit], ...]`, or none.
    #[wasm_bindgen(js_namespace = window, js_name = gameRemoteLooks)]
    pub fn game_remote_looks() -> Option<String>;
    /// Name label of a remote figure in canvas pixels. Negative: hidden.
    #[wasm_bindgen(js_namespace = window, js_name = gameRemoteLabel)]
    pub fn game_remote_label(slot: u32, x: f32, y: f32);
    /// "Antialiasing" setting at page load.
    #[wasm_bindgen(js_namespace = window, js_name = gameAntialiasing)]
    pub fn game_antialiasing() -> bool;
    /// "Grass" setting at page load: 0 high, 1 medium, 2 low.
    #[wasm_bindgen(js_namespace = window, js_name = gameGrassQuality)]
    pub fn game_grass_quality() -> u32;
    /// Pending "Ambient occlusion" setting, or none.
    #[wasm_bindgen(js_namespace = window, js_name = gameAmbientOcclusion)]
    pub fn game_ambient_occlusion() -> Option<bool>;
    /// Pending "Motion blur" setting, or none.
    #[wasm_bindgen(js_namespace = window, js_name = gameMotionBlur)]
    pub fn game_motion_blur() -> Option<bool>;
    /// Pending "Depth of field" setting, or none.
    #[wasm_bindgen(js_namespace = window, js_name = gameDepthOfField)]
    pub fn game_depth_of_field() -> Option<bool>;
    /// Pending "Sunglasses effects" setting, or none.
    #[wasm_bindgen(js_namespace = window, js_name = gameSunglassesEffects)]
    pub fn game_sunglasses_effects() -> Option<bool>;
    /// Pending `ScreenEffects` JSON, or none.
    #[wasm_bindgen(js_namespace = window, js_name = gameScreenEffects)]
    pub fn game_screen_effects() -> Option<String>;
    /// Pending "Lamp shadows" setting, or none.
    #[wasm_bindgen(js_namespace = window, js_name = gameLampShadows)]
    pub fn game_lamp_shadows() -> Option<bool>;
    /// Pending "Reflections" setting, or none.
    #[wasm_bindgen(js_namespace = window, js_name = gameReflections)]
    pub fn game_reflections() -> Option<bool>;
    /// Fills the pending "Mirrors" setting: on (0 or 1) and the draw distance in meters.
    /// False when nothing changed.
    #[wasm_bindgen(js_namespace = window, js_name = gameMirrors)]
    pub fn game_mirrors(setting: &mut [f32]) -> bool;
    /// Fills the appearance preview rect: x, y, width and height in canvas fractions.
    /// False while the appearance screen is closed.
    #[wasm_bindgen(js_namespace = window, js_name = gameAppearancePreview)]
    pub fn game_appearance_preview(rect: &mut [f32]) -> bool;
    /// Fills the start menu mouse: x and y in canvas fractions, held buttons and buttons pressed since the last call.
    /// Bit 1 is the left button, bit 2 the right. False while the start menu is closed.
    #[wasm_bindgen(js_namespace = window, js_name = gameMenuFigure)]
    pub fn game_menu_figure(input: &mut [f32]) -> bool;
    /// Fills 4 pads × 8 axes and 4 pads × 32 button bits (`GamepadFrame`).
    #[wasm_bindgen(js_namespace = window, js_name = gameGamepad)]
    pub fn game_gamepad(axes: &mut [f32], buttons: &mut [u32]);
    #[wasm_bindgen(js_namespace = window, js_name = gameBiome)]
    pub fn game_biome() -> u32;
    #[wasm_bindgen(js_namespace = window, js_name = gameStartCar)]
    pub fn game_start_car() -> i32;
    #[wasm_bindgen(js_namespace = window, js_name = gameSeed)]
    pub fn game_seed() -> i32;
    /// Changed game option bits (1 peaceful, 2 survival), or none.
    #[wasm_bindgen(js_namespace = window, js_name = gameOptions)]
    pub fn game_options() -> Option<u32>;
    /// `steer` to `handbrake` are the last `DrivingControls` (steer positive left).
    #[wasm_bindgen(js_namespace = window, js_name = gameDrivingState)]
    pub fn game_driving_state(
        gear: i32,
        assists: u32,
        seated: bool,
        steer: f32,
        throttle: f32,
        brake: f32,
        clutch: f32,
        handbrake: f32,
    );
    #[wasm_bindgen(js_namespace = window, js_name = gameUpdate)]
    pub fn game_update(
        speed: f32,
        camera: u32,
        x: f32,
        y: f32,
        z: f32,
        contacts: u32,
        steps: u32,
        surfaces: u32,
        colliders: u32,
        absolute_x: f64,
        absolute_y: f64,
        absolute_z: f64,
        origin_x: f64,
        origin_y: f64,
        origin_z: f64,
        rebases: u32,
        travel: f64,
        prepared_roads: f64,
        fuel: f32,
        paused: bool,
        started: bool,
        autosave_safe: bool,
        seated: bool,
    );
    #[wasm_bindgen(js_namespace=window,js_name=gameInventory)]
    pub fn game_inventory(selected: u32, a: u32, b: u32, c: u32, d: u32, progress: f32);
    #[wasm_bindgen(js_namespace = window, js_name = gameWeaponCrosshair)]
    pub fn game_weapon_crosshair(icon: u32, color: u32);
    #[wasm_bindgen(js_namespace = window, js_name = gameRadioFeedback)]
    pub fn game_radio_feedback(volume: f32, frequency: f32, am: bool);
    #[wasm_bindgen(js_namespace = window, js_name = gameTypingKeys)]
    pub fn game_typing_keys() -> u32;
    #[wasm_bindgen(js_namespace = window, js_name = gameTypedText)]
    pub fn game_typed_text(buffer: &mut [u8]) -> u32;
    #[wasm_bindgen(js_namespace = window, js_name = gameFeedback)]
    pub fn game_feedback(
        actions: u32,
        context: u32,
        name: &str,
        notice: &str,
        carrying: bool,
        amount: f32,
        capacity: f32,
        has_name: bool,
        no_text: bool,
        fluid_word: u32,
        held: &str,
        throw_charge: f32,
        throw_active: bool,
        rpm: f32,
        ignition: bool,
        running: bool,
        starter: bool,
        seated: bool,
        handbrake: bool,
        gas: f32,
        oil: f32,
        alcohol: f32,
        water: f32,
        blood: f32,
        diesel: f32,
        fluid_order: u32,
        control: u32,
        lmb_use: bool,
        prompts: u32,
        computer: u32,
        crosshair: u32,
        weight: f32,
        help: u32,
    );
}
/// The local month and day.
pub fn local_calendar() -> [u8; 2] {
    let calendar = game_audio_calendar();
    [(calendar / 100) as u8, (calendar % 100) as u8]
}
