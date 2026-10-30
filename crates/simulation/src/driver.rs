//! Runs the simulation from renderer commands. It owns the runtime, the input queue and the step clock.
use crate::boundary::{Change, HudBlock, LookBlock, RenderInput, SimCommand, SimFrame, SimStartup};
use crate::car_camera::{CameraInput, CameraSeat, CarCamera};
use crate::reference::{ReferenceError, ReferenceManifest};
use crate::simulation::{FIXED_DT, FrameInput, StepClock, keys};
use crate::starting_area::{GarageRuntime, WorldInput};
use crate::vehicle::VehiclePose;
use glam::{Quat, Vec3};

/// World step milliseconds of a loading frame. The loading panel covers the screen.
pub const LOADING_BUDGET_MS: f64 = 12.;

pub struct SimDriver {
    pub runtime: GarageRuntime,
    /// Input of the next fixed step.
    input: FrameInput,
    /// Accepted frame input that no frame consumed yet.
    frame_input: FrameInput,
    clock: StepClock,
    /// Player camera of the last two steps.
    look: LookBlock,
    /// Player camera at the interpolation of the last frame.
    rendered: VehiclePose,
    /// Command mouse in `input` that no step consumed yet.
    raw_mouse: [f32; 2],
    /// Sequence of the last accepted command.
    accepted_seq: u64,
    /// Sequence of the last command in `frame_input`.
    frame_seq: u64,
    /// Sequence of the last command in `input`.
    input_seq: u64,
    /// Sequence of the last command whose input a step consumed or the simulation dropped.
    consumed_seq: u64,
    /// Source seated views: outside view, drop cam and map view.
    pub camera: CarCamera,
    /// Rig input of accepted commands that no frame used yet.
    camera_input: CameraInput,
    /// Camera seat of the last two steps.
    camera_seat: [Option<CameraSeat>; 2],
    /// Fixed steps of the last frame.
    pub(crate) frame_steps: u32,
    /// Milliseconds of the last frame steps.
    pub physics_ms: f64,
    /// Milliseconds of the last world step.
    world_ms: f64,
    pub(crate) world: WorldInput,
    /// Answer of the last world step to the renderer report.
    pub(crate) grant: Option<crate::world::PresentationGrant>,
    skip_elapsed: bool,
    /// The renderer shows the garage.
    pub(crate) ready: bool,
    delta_seconds: f64,
    pub(crate) calendar: [u8; 2],
    /// The browser asked for a world measurement.
    measure: bool,
    /// A failure that the next frame reports. The driver stops the storage startup after it.
    failure: Option<String>,
    failed: bool,
    pub(crate) save: crate::save_requests::SaveState,
    /// The renderer world copy of the last frame.
    pub(crate) sent: crate::world::RendererWorld,
    /// Camera position of the last rendered frame.
    listener: Option<[f32; 3]>,
    /// Sandbox replies that the next frame sends.
    sandbox: Vec<String>,
    /// Multiplayer peers. A load keeps them.
    pub net: crate::net::Net,
}

impl SimDriver {
    /// `source` stamps the saves of this build.
    pub fn new(runtime: GarageRuntime, source: crate::save::SourceStamp) -> Self {
        let sent = crate::world::RendererWorld::new(runtime.stream.world_reference().clone());
        let camera = CarCamera::new(&runtime.area.player.reference().car_camera);
        let mut driver = Self {
            sent,
            runtime,
            input: FrameInput::default(),
            frame_input: FrameInput::default(),
            clock: StepClock::default(),
            look: LookBlock {
                gain: 1.,
                ..LookBlock::default()
            },
            rendered: VehiclePose {
                position: [0.; 3],
                rotation: [0., 0., 0., 1.],
            },
            raw_mouse: [0.; 2],
            accepted_seq: 0,
            frame_seq: 0,
            input_seq: 0,
            consumed_seq: 0,
            camera,
            camera_input: CameraInput::default(),
            camera_seat: [None; 2],
            frame_steps: 0,
            physics_ms: 0.,
            world_ms: 0.,
            world: WorldInput::default(),
            grant: None,
            skip_elapsed: false,
            ready: false,
            delta_seconds: 0.,
            calendar: [1, 1],
            measure: false,
            failure: None,
            failed: false,
            save: crate::save_requests::SaveState::new(source),
            listener: None,
            sandbox: Vec::new(),
            net: crate::net::Net::default(),
        };
        driver.reset_look();
        driver
    }

    /// Renderer setup values of the runtime.
    pub fn startup(&self) -> SimStartup {
        let runtime = &self.runtime;
        let view = runtime.area.player.view(&runtime.physics);
        SimStartup {
            view: view.camera_pose,
            fov_degrees: view.fov_degrees,
            far_plane: runtime.distances.cam2_far_plane,
            session: runtime.stream.frame().request.session,
        }
    }

    /// One simulation frame: accepts `command`, steps and fills `frame`.
    pub fn tick(
        &mut self,
        reference: &ReferenceManifest,
        command: SimCommand,
        frame: &mut SimFrame,
    ) {
        self.accept(reference, command);
        self.advance(reference);
        self.advance_world(reference);
        self.fill(reference, frame);
    }

    /// Applies the menu actions and settings of `command` and queues its input.
    /// Save requests run after the input, so a pause in the same command counts.
    pub fn accept(&mut self, reference: &ReferenceManifest, mut command: SimCommand) {
        let saves = std::mem::take(&mut command.save);
        self.accept_input(reference, command);
        self.accept_saves(reference, saves);
    }

    fn accept_input(&mut self, reference: &ReferenceManifest, command: SimCommand) {
        self.delta_seconds = command.delta_seconds;
        self.listener = command.listener;
        self.calendar = command.calendar;
        self.ready = command.ready;
        self.accepted_seq = command.seq;
        self.measure |= command.measure;
        self.world.accept(command.world);
        if let Some(json) = &command.sandbox
            && !command.hold
        {
            crate::sandbox::run(&mut self.runtime, reference, json, &mut self.sandbox);
        }
        if command.hold {
            clear_profile_input();
            self.drop_frame_input();
            return;
        }
        let actions = command.actions;
        if actions & SimCommand::PAUSE != 0 {
            self.runtime.paused = true;
            self.clear_input();
        }
        if actions & SimCommand::START != 0 && self.ready && !self.runtime.needs_output().menu_due {
            self.runtime.started = true;
            self.runtime.paused = false;
            self.clear_input();
        }
        if actions & SimCommand::RECOVER != 0 {
            self.runtime.recover(reference);
            self.clear_input();
        }
        if let Some(assists) = command.assists {
            let _ = self.runtime.set_driving_assists(assists);
        }
        if let Some(options) = command.options {
            self.runtime.set_game_options(options);
        }
        // The browser validates stored settings. A refused value keeps the current settings.
        if let Some(settings) = command.settings
            && settings.valid(&reference.driving)
        {
            self.runtime.driving_input.settings = settings;
        }
        self.runtime.old_heat_haze = command.old_heat_haze;
        if let Some(settings) = command.player_settings
            && settings.valid(&reference.settings)
        {
            self.runtime.set_player_settings(settings);
        }
        let frame = command.input;
        if !self.ready || !self.runtime.started || self.runtime.paused {
            self.input.clear();
            self.raw_mouse = [0.; 2];
            clear_profile_input();
            self.drop_frame_input();
            self.runtime.clear_input();
            // The Controls screen shows the pad axes while the game is paused.
            self.runtime.driving_input.preview(&frame.gamepad);
            return;
        }
        if actions != 0 {
            return;
        }
        if !self.runtime.needs_output().dead && !self.runtime.sleeping() {
            self.camera_input.merge(&frame);
        }
        self.frame_input.merge(if self.camera.other_view() {
            frame.without_view_input()
        } else {
            frame
        });
        self.frame_seq = command.seq;
    }

    /// Runs the fixed steps of the accepted frame time.
    pub fn advance(&mut self, reference: &ReferenceManifest) {
        self.physics_ms = 0.;
        self.frame_steps = 0;
        if !self.ready && !self.failed {
            // Source storage startup runs once per loading frame.
            if let Err(error) = self
                .runtime
                .initialize_storage_frame(self.delta_seconds as f32)
            {
                self.failure = Some(error.to_string());
                self.failed = true;
            }
        }
        if !self.ready || !self.runtime.started || self.runtime.paused {
            return;
        }
        let blocked = self.skip_elapsed
            && (self.runtime.recovering
                || !self.runtime.world_supported()
                || self.runtime.sleeping()
                || self.runtime.needs_output().dead);
        let Some(raw) = self.frame_input.take_frame(&mut self.skip_elapsed, blocked) else {
            if blocked {
                self.input.clear();
                self.raw_mouse = [0.; 2];
                self.consumed_seq = self.accepted_seq;
                self.runtime.clear_input();
            }
            return;
        };
        for (pending, delta) in self.raw_mouse.iter_mut().zip(raw.mouse) {
            if delta.is_finite() {
                *pending += delta;
            }
        }
        self.input_seq = self.frame_seq;
        let started = now();
        #[cfg(target_arch = "wasm32")]
        let _profile = crate::cpu_profile::Timer::new(crate::cpu_profile::Stage::Physics);
        if let Some(audio) = &mut self.runtime.audio {
            let [month, day] = self.calendar;
            audio.broadcasts.calendar(month, day);
        }
        let real = self.delta_seconds as f32;
        let delta = real * self.runtime.clock_factors(reference).time_scale;
        self.runtime.area.interaction.conditions.update(delta, true);
        let frame = self
            .runtime
            .advance_inventory_frame(reference, raw, delta, self.rendered);
        frame.merge_fixed_input(&mut self.input);
        self.clock.accept_frame(self.delta_seconds);
        loop {
            let scale = self.runtime.clock_factors(reference).time_scale;
            if !self.clock.take_step(scale) {
                break;
            }
            self.frame_steps += 1;
            let old_origin = self.runtime.stream.origin.offset;
            let input = self.input.take_step();
            self.runtime.area.player.take_look_applied();
            self.runtime.step(reference, input);
            let applied = self.runtime.area.player.take_look_applied();
            let raw = std::mem::take(&mut self.raw_mouse);
            // The gain covers look scale, zoom and every rule that keeps the mouse from the look.
            let axis = usize::from(raw[1].abs() > raw[0].abs());
            if raw[axis] != 0. {
                self.look.gain = applied[axis] / raw[axis];
            }
            self.consumed_seq = self.input_seq;
            let new_origin = self.runtime.stream.origin.offset;
            let mut previous = self.look.position[1];
            for i in 0..3 {
                previous[i] -= (new_origin[i] - old_origin[i]) as f32;
            }
            let parent = self.look.parent[1];
            self.capture_look();
            self.look.position[0] = previous;
            self.look.parent[0] = parent;
            let shift = Vec3::from_array(std::array::from_fn(|i| {
                (new_origin[i] - old_origin[i]) as f32
            }));
            let mut seat = self.camera_seat[1];
            if let Some(seat) = &mut seat {
                seat.shift(shift);
            }
            self.camera_seat = [seat, self.camera_seat()];
            self.camera.shift(shift);
            if self.runtime.paused {
                self.clock.reset();
                break;
            }
        }
        let scale = self.runtime.clock_factors(reference).time_scale;
        self.rendered = self.look.pose(self.clock.alpha(scale), [0.; 2]);
        self.physics_ms = now() - started;
    }

    /// Runs the world step of the accepted renderer input, after the fixed steps.
    pub fn advance_world(&mut self, reference: &ReferenceManifest) {
        self.world_ms = 0.;
        if let Some(failure) = self.world.take_failure() {
            self.runtime.stream.fail(ReferenceError(failure));
            self.runtime.clear_input();
        }
        let started = now();
        let budget_ms = if self.ready { 0. } else { LOADING_BUDGET_MS };
        let runtime = &mut self.runtime;
        runtime.stream.advance_generation();
        let was_aligned = runtime.startup_aligned;
        let result = runtime
            .step_world(reference, &mut self.world, budget_ms)
            .and_then(|grant| {
                self.grant = grant;
                runtime.align_startup(reference, self.calendar)?;
                if runtime.has_deferred_save_links() {
                    runtime
                        .advance_saved_restore(reference)
                        .map_err(|e| ReferenceError(e.to_string()))?;
                }
                runtime
                    .sync_world_activity(reference)
                    .map_err(|e| ReferenceError(e.to_string()))?;
                runtime.advance_recovery()
            });
        match result {
            Ok(moved) => {
                if moved || (!was_aligned && self.runtime.startup_aligned) {
                    self.clear_input();
                }
            }
            Err(error) => {
                self.failure = Some(error.to_string());
                self.runtime.stream.fail(error);
                self.runtime.clear_input();
            }
        }
        self.world_ms = now() - started;
    }

    /// Writes the runtime state into `frame`.
    pub fn fill(&mut self, reference: &ReferenceManifest, frame: &mut SimFrame) {
        let input = RenderInput {
            dt: self.frame_steps as f32 * FIXED_DT,
            real_dt: self.delta_seconds as f32,
            listener: self.listener,
        };
        self.runtime.advance_render_frame(reference, input);
        let alpha = self
            .clock
            .alpha(self.runtime.clock_factors(reference).time_scale);
        if self.ready && self.runtime.started && !self.runtime.paused {
            self.update_camera(reference, alpha);
        }
        self.runtime.fill_frame(reference, alpha, frame);
        self.net
            .fill(reference, &self.runtime, self.delta_seconds as f32, frame);
        if self.input.held & keys::GRAB != 0 {
            frame.hud.flags &= !HudBlock::AUTOSAVE_SAFE;
        }
        self.runtime
            .stream
            .sync_renderer(&mut self.sent, &mut frame.world);
        frame.seq_consumed = self.consumed_seq;
        frame.state.look = self.look;
        frame.state.perf.physics_ms = self.physics_ms;
        frame.state.perf.frame_steps = self.frame_steps;
        frame.state.perf.world_ms = self.world_ms;
        if let Some(grant) = self.grant.take() {
            frame.changes.push(Change::Grant(grant));
        }
        frame
            .changes
            .extend(self.sandbox.drain(..).map(Change::Sandbox));
        if std::mem::take(&mut self.measure) {
            frame.changes.push(Change::Measurements(
                self.runtime.measurements(reference).to_string(),
            ));
        }
        if let Some(failure) = self.failure.take() {
            frame.changes.push(Change::Failure(failure));
        }
        self.camera.fill(&mut frame.state);
        self.fill_saves(frame);
    }

    /// Drops queued input and restarts the step clock at the player view.
    pub fn clear_input(&mut self) {
        clear_profile_input();
        self.input.clear();
        self.frame_input.clear();
        self.runtime.clear_input();
        self.raw_mouse = [0.; 2];
        self.camera_input = CameraInput::default();
        self.consumed_seq = self.accepted_seq;
        self.reset_look();
        self.clock.reset();
        self.runtime.area.begin_step();
        self.skip_elapsed = true;
    }

    /// Starts a loaded runtime: paused, with no queued input.
    pub fn restart(&mut self) {
        self.camera.leave();
        self.runtime.other_view = false;
        self.camera_input = CameraInput::default();
        self.reset_look();
        self.raw_mouse = [0.; 2];
        self.consumed_seq = self.accepted_seq;
        self.input.clear();
        self.frame_input.clear();
        self.clock = StepClock::default();
        self.frame_steps = 0;
        self.ready = false;
        self.runtime.paused = true;
        self.skip_elapsed = true;
    }

    /// Drops the frame input. Its mouse is consumed when no step input waits.
    fn drop_frame_input(&mut self) {
        self.frame_input.clear();
        if self.raw_mouse == [0.; 2] {
            self.consumed_seq = self.accepted_seq;
        }
    }

    /// Sets the current step look from the player. Keeps the gain.
    fn capture_look(&mut self) {
        let player = &self.runtime.area.player;
        let gain = self.look.gain;
        self.look = player.look(&self.runtime.physics);
        self.look.gain = gain;
        self.look.position[1] = player.view(&self.runtime.physics).camera_pose.position;
    }

    /// Both steps of the look at the player view.
    fn reset_look(&mut self) {
        self.capture_look();
        self.look.position[0] = self.look.position[1];
        self.look.parent[0] = self.look.parent[1];
        self.rendered = self.look.pose(1., [0.; 2]);
        self.camera_seat = [self.camera_seat(); 2];
    }

    fn camera_seat(&self) -> Option<CameraSeat> {
        let runtime = &self.runtime;
        if runtime.needs_output().dead {
            return None;
        }
        runtime.area.player.camera_seat(&runtime.physics)
    }

    /// Runs the seated view rig once per frame at the interpolated seat.
    fn update_camera(&mut self, reference: &ReferenceManifest, alpha: f32) {
        let seat = match self.camera_seat {
            [Some(previous), Some(current)] => Some(previous.lerp(&current, alpha)),
            [_, current] => current,
        };
        let head = self.look.pose(alpha, [0.; 2]);
        let dt = self.delta_seconds as f32 * self.runtime.clock_factors(reference).time_scale;
        let other_view = self.camera.other_view();
        self.camera.frame(
            &self.runtime.area.player.reference().car_camera,
            &mut self.camera_input,
            seat.as_ref(),
            (
                Vec3::from_array(head.position),
                Quat::from_array(head.rotation),
            ),
            dt,
        );
        if other_view != self.camera.other_view() {
            self.runtime.audio_motion_revision = self.runtime.audio_motion_revision.wrapping_add(1);
        }
        self.runtime.other_view = self.camera.other_view();
    }
}

pub fn interpolate(a: VehiclePose, b: VehiclePose, alpha: f32) -> VehiclePose {
    VehiclePose {
        position: Vec3::from_array(a.position)
            .lerp(Vec3::from_array(b.position), alpha)
            .to_array(),
        rotation: Quat::from_array(a.rotation)
            .slerp(Quat::from_array(b.rotation), alpha)
            .to_array(),
    }
}

#[cfg(target_arch = "wasm32")]
fn now() -> f64 {
    crate::host::game_now()
}

#[cfg(not(target_arch = "wasm32"))]
fn now() -> f64 {
    0.
}

fn clear_profile_input() {
    #[cfg(target_arch = "wasm32")]
    if crate::cpu_profile::enabled() {
        crate::host::game_cpu_clear_input();
    }
}
