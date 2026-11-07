//! The fixed step of one car copy: drivetrain, ignition, engine, forces, pedals, lights,
//! exhaust and wheel effects, in source `carscript` order.
use super::{CarFrames, VehicleRuntime, apply_balance, car_sound, vehicle_steer, world_vehicle};
use crate::interaction::{InteractionState, items::ItemStore};
use crate::player::{PlayerState, body::BodyState};
use crate::reference::{DrivingAssists, ReferenceManifest};
use crate::simulation::{
    DriveInput, FIXED_DT, FrameInput,
    engine::{EngineInput, EngineOutput, EngineRecords, InstalledEngineInput},
    ignition::IgnitionInput,
    input::DrivingOutput,
    keys,
    powertrain::PowertrainParts,
};
use crate::starting_area::start_sample;
use crate::vehicle::{PhysicsWorld, to_rapier};
use crate::world::{DEFAULT_SEED, ObjectKey, WorldOrigin};

/// The world state that a car step reads and changes.
pub struct CarWorld<'a> {
    pub physics: &'a mut PhysicsWorld,
    pub items: &'a mut ItemStore,
    pub interaction: &'a InteractionState,
    pub player: &'a PlayerState,
    /// The player body. Car controls send reach moves to it.
    pub body: &'a mut BodyState,
    pub powertrain: &'a mut PowertrainParts,
    pub audio: &'a mut Option<crate::audio::AudioState>,
    pub origin: &'a WorldOrigin,
    pub assists: DrivingAssists,
    /// Engine input without car controls: ambient temperature and clock rate.
    pub climate: EngineInput,
    /// Ignition presses of all cars. Engine start pushes sample from it.
    pub attempts: &'a mut u64,
}

/// The controls of one car step.
#[derive(Clone, Copy)]
pub struct CarInput {
    pub frame: FrameInput,
    /// The local player drives this car.
    pub driven: bool,
    /// The driving controls. Only a driven car reads them.
    pub driving: DrivingOutput,
}

impl VehicleRuntime {
    /// Source `carscript.FixedUpdate` of car `key` for one fixed step.
    pub fn step(
        &mut self,
        reference: &ReferenceManifest,
        key: &ObjectKey,
        world: &mut CarWorld,
        input: CarInput,
    ) {
        step_drive(
            world,
            reference,
            key,
            self,
            input.frame,
            input.driven,
            input.driving,
        );
        advance_lights(world, reference, key, self);
        advance_exhaust(world, reference, key, self);
        advance_wheel_effects(world, reference, key, self);
    }
}

/// Steps the light controllers of a car.
fn advance_lights(
    world: &CarWorld,
    reference: &ReferenceManifest,
    key: &ObjectKey,
    vehicle: &mut VehicleRuntime,
) {
    let record = world_vehicle(reference, vehicle.prototype);
    let input = vehicle.light_input(reference);
    let (items, mechanisms) = (&world.items, &world.interaction.mechanisms);
    let root = items.resolve_key(key);
    vehicle.lights.advance(
        &record.lights,
        input,
        &vehicle.electronics,
        |item| root.is_some_and(|root| item == root || items.descends(item, root)),
        |item| mechanisms.value(item, false).unwrap_or(0.),
        |item| mechanisms.turn_state(item),
        FIXED_DT,
    );
}

/// Advances exhaust smoke every step, so old smoke ages out when the engine stops.
fn advance_exhaust(
    world: &mut CarWorld,
    reference: &ReferenceManifest,
    key: &ObjectKey,
    vehicle: &mut VehicleRuntime,
) {
    let engine = vehicle
        .powertrain
        .engine
        .and_then(|item| world.powertrain.engine(item));
    let output = engine.map_or_else(EngineOutput::default, |e| e.state.output());
    let active = engine.is_some_and(|engine| {
        world
            .items
            .get(engine.item)
            .and_then(|item| item.body)
            .is_some_and(|body| world.physics.body_active(body))
    });
    let running = active && output.running;
    // The chassis body frame is the car root node. Reference validation requires it.
    let root = world.physics.rapier().bodies[world.physics.chassis_handle(key)].position();
    vehicle.exhaust.advance(
        FIXED_DT,
        crate::exhaust::ExhaustInput {
            running,
            rpm: output.rpm,
            max_rpm: engine.map_or(0., |e| e.record.max_rpm),
            smoke_rate: engine.map_or(0., |e| e.record.smoke_rate),
            tank: vehicle
                .fuel_tank
                .and_then(|id| world.items.get(id))
                .and_then(|i| i.fluid.as_ref()),
            root_absolute: world.origin.absolute(root.translation.to_array()),
            rotation: root.rotation,
            origin: world.origin.offset,
        },
        &reference.fluid_rules.burn_colors,
        world.physics,
    );
    vehicle.coolant_steam.advance(
        FIXED_DT,
        if active { output.boil } else { 0. },
        world.origin.absolute(root.translation.to_array()),
        root.rotation,
        world.origin.offset,
        world.physics,
    );
}

/// Advances wheel effects every step, so old particles age out on a sleeping car.
fn advance_wheel_effects(
    world: &mut CarWorld,
    reference: &ReferenceManifest,
    key: &ObjectKey,
    vehicle: &mut VehicleRuntime,
) {
    let record = world_vehicle(reference, vehicle.prototype);
    let (references, gains) = (&record.wheel_effects, &record.effect_gains);
    let chassis = world.physics.chassis_handle(key);
    let body = &world.physics.rapier().bodies[chassis];
    // The chassis body frame is the car root node. Reference validation requires it.
    let root = body.position();
    let forward = root.rotation * -rapier3d::math::Vector::Z;
    let linear_velocity = body.linvel();
    let physics = &world.physics;
    let wheel = |w| {
        let (rpm, radius, grounded) = physics.wheel_motion(key, w);
        crate::wheel_effects::WheelInput {
            rpm,
            radius,
            forward,
            grounded,
        }
    };
    // Footsteps own the source sand layer list. Without audio, every hit is road.
    let sand_layers = world
        .audio
        .as_ref()
        .map_or(&[][..], |a| &a.reference.footsteps.sand_layers);
    vehicle.wheel_effects.advance(
        FIXED_DT,
        &crate::wheel_effects::WheelEffectsInput {
            root_absolute: world.origin.absolute(root.translation.to_array()),
            rotation: root.rotation,
            origin: world.origin.offset,
            chassis,
            linear_velocity,
            car_kmh: linear_velocity.dot(forward).abs() * 3.6,
        },
        wheel,
        references,
        gains,
        &reference.skid,
        sand_layers,
        world.physics,
    );
}

/// Steps one engine and applies its forces. Only the driven vehicle reads keys.
/// The engine and radiator are the parts mounted on this car. Torque needs an engine part.
fn step_drive(
    world: &mut CarWorld,
    reference: &ReferenceManifest,
    key: &ObjectKey,
    vehicle: &mut VehicleRuntime,
    input: FrameInput,
    driving: bool,
    driving_output: crate::simulation::input::DrivingOutput,
) {
    vehicle.net.sent = None;
    let received_gear = vehicle.net.gear.take();
    let received_rpm = vehicle.net.rpm.take();
    let chassis = world.physics.chassis_handle(key);
    let chassis_active = world.physics.body_active(chassis);
    let climate = world.climate;
    let powertrain = vehicle.powertrain;
    let engine = powertrain
        .engine
        .and_then(|item| world.powertrain.engine(item));
    let radiator = powertrain
        .radiator
        .and_then(|item| world.powertrain.radiator(item));
    let engine_active = engine
        .and_then(|engine| world.items.get(engine.item))
        .and_then(|item| item.body)
        .is_some_and(|body| world.physics.body_active(body));
    if !chassis_active && !engine_active {
        if let Some(gear) = received_gear {
            vehicle.drivetrain.restore_gear(gear);
        }
        return;
    }
    let gearbox: crate::simulation::drivetrain::Gearbox =
        world_vehicle(reference, vehicle.prototype).into();
    let snapshot = world.physics.snapshot(key);
    let speed = snapshot.speed_mps;
    let driving = driving && chassis_active;
    let crate::simulation::input::DrivingOutput {
        controls,
        gear_edges,
    } = if driving {
        driving_output
    } else {
        Default::default()
    };
    // Source `multiControlling`: the drivers of the other peers.
    let remote = vehicle.net.remote.filter(|_| chassis_active);
    let controls = remote.map_or(controls, |r| {
        crate::net::car::with_remote(controls, r, driving)
    });
    let control = if driving {
        crate::simulation::drivetrain::Control::Local
    } else if remote.is_some() {
        crate::simulation::drivetrain::Control::Remote
    } else {
        crate::simulation::drivetrain::Control::None
    };
    let direction = speed
        * snapshot
            .linear_velocity
            .iter()
            .map(|v| v * v)
            .sum::<f32>()
            .sqrt();
    let rolling_rpm = world.physics.driven_wheel_rpm(key);
    let last_gear = vehicle.drivetrain.gear();
    if let Some(gear) = received_gear {
        vehicle.drivetrain.restore_gear(gear);
    }
    let drivetrain = vehicle.drivetrain.step(
        controls,
        gear_edges,
        world.assists,
        engine.map(|engine| (engine.state.output(), &engine.record)),
        vehicle.ignition.on(),
        rolling_rpm,
        direction,
        gearbox,
        &reference.driving,
        control,
    );
    let gear = drivetrain.gear;
    if gear != last_gear
        && let Some(lever) = &mut vehicle.lever
    {
        lever.shift(&reference.driving.lever, last_gear, gear);
    }
    // Source `Shifted` plays a random `valtoSounds` clip unless the gears shift by themselves.
    // Source `ControlStuff` plays `rosszvaltoSound` at the grind gain.
    let audio = world_vehicle(reference, vehicle.prototype).audio.as_ref();
    let shift = audio
        .filter(|a| gear != last_gear && !drivetrain.automatic && !a.shift.is_empty())
        .map(|a| {
            let i = vehicle.shift_random.range_int(0, a.shift.len() as i32) as usize;
            a.shift[i]
        });
    let grind = drivetrain
        .grind
        .zip(audio.and_then(|a| a.grind))
        .map(|(gain, shot)| crate::reference::CarShotReference {
            gain: shot.gain * gain,
            ..shot
        });
    for shot in [shift, grind].into_iter().flatten() {
        car_sound(
            world.audio,
            world.physics,
            reference,
            key,
            vehicle.prototype,
            |_| Some(shot),
        );
    }
    let record = world_vehicle(reference, vehicle.prototype);
    let reach = &record.reach;
    let frames = CarFrames::new(
        reference,
        key,
        vehicle,
        world.powertrain,
        world.items,
        world.physics,
    );
    let body = &mut world.body;
    if gear != last_gear
        && driving
        && let Some(m) = crate::simulation::reach::shift(
            reach,
            frames.gear_stick(),
            drivetrain.automatic,
            reference.driving.lever.seconds,
        )
    {
        frames.send(body, m);
    }
    let drive = &record.drive_frames;
    vehicle.pedal_reach.step(
        reach,
        [drive.left_leg.is_some(), drive.right_leg.is_some()],
        crate::simulation::reach::Pedals {
            throttle: drivetrain.throttle,
            brake: drivetrain.brake,
            clutch: drivetrain.clutch,
        },
        driving,
        FIXED_DT,
        |m| frames.send(body, m),
    );
    // In source `carscript.ControlStuff`, the pedals are off while the engine runs.
    let pedaling = driving
        && record.pedals.is_some()
        && !engine.is_some_and(|engine| engine.state.output().running);
    let throttle = if record.no_gas_while_pedaling && pedaling {
        0.
    } else {
        drivetrain.throttle
    };
    let brake = drivetrain.brake;
    vehicle.brake = brake;
    let car_controls = chassis_active
        && (driving || (world.player.driving2() && world.player.seat_vehicle() == Some(key)));
    let ignition_held =
        car_controls && (input.held & keys::IGNITION != 0 || drivetrain.ignition_request);
    let ignition_pressed = (car_controls && input.pressed & keys::IGNITION != 0)
        || (ignition_held && !vehicle.ignition.held());
    let ignition_released = !ignition_held
        && (vehicle.ignition.held() || (car_controls && input.released & keys::IGNITION != 0));
    if ignition_pressed {
        *world.attempts = world.attempts.wrapping_add(1);
    }
    let radiator_condition = radiator.map(|radiator| {
        ((1.0
            - world
                .interaction
                .conditions
                .item_quality(radiator.item)
                .unwrap_or(1.0))
            * 4.0)
            .round() as u8
    });
    if car_controls {
        crate::simulation::reach::ignition(
            reach,
            frames.gear_stick(),
            crate::simulation::reach::IgnitionReach {
                held: ignition_held,
                last_held: vehicle.ignition.held(),
                starting: vehicle.ignition.cranking(),
                hold: vehicle.ignition.hold_seconds(),
                delay: vehicle.starter_delay_seconds,
            },
            |m| frames.send(world.body, m),
        );
        if input.held & keys::HORN != 0
            && let Some(m) = crate::simulation::reach::horn(reach)
        {
            frames.send(world.body, m);
        }
    }
    let was_ignition = vehicle.ignition.on();
    vehicle.ignition.step(
        IgnitionInput {
            held: ignition_held,
            pressed: ignition_pressed,
            released: ignition_released,
        },
        vehicle.starter_delay_seconds,
        vehicle.electronics_enabled(),
        FIXED_DT,
    );
    let was_running = engine.is_some_and(|engine| engine.state.output().running);
    let globals = &reference.world.vehicle_globals;
    if engine_active && let (Some(part), mut radiator_part) = world.powertrain.pair_mut(powertrain)
    {
        if let Some(rpm) = received_rpm {
            part.state.rpm = rpm;
        }
        let (fan_on, radiator, coolant_tank) = match radiator_part.as_deref_mut() {
            Some(r) => (Some(&mut r.fan_on), Some(&r.record), r.coolant_tank),
            None => (None, None, None),
        };
        part.state.step(
            EngineInput {
                installed: Some(InstalledEngineInput {
                    key: vehicle.ignition.key,
                    throttle,
                    clutch: drivetrain.engagement,
                    wheel_rpm: drivetrain.wheel_rpm,
                    free_run: drivetrain.free_run,
                    speed_kmh: speed * 3.6,
                    radiator_condition,
                    coolant_boil: world_vehicle(reference, vehicle.prototype)
                        .coolant_steam
                        .is_some(),
                    electronics_enabled: vehicle.electronics_enabled(),
                }),
                ..climate
            },
            world
                .items
                .engine_fluids([vehicle.fuel_tank, part.oil_tank, coolant_tank]),
            EngineRecords {
                engine: &part.record,
                radiator,
                globals,
            },
            fan_on,
            |draw| start_sample(DEFAULT_SEED, draw),
            FIXED_DT,
        );
        // Source `coolantTankscript.Cool`: Fan.Rotate(0, 0, selectedFanSpeed * dt).
        if let Some(r) = radiator_part {
            r.fan_angle = (r.fan_angle + part.state.output().fan_speed * FIXED_DT) % 360.;
        }
        world
            .items
            .get_mut(part.item)
            .expect("engine item")
            .temperature = Some(part.state.temperature);
    }
    let engine = powertrain
        .engine
        .and_then(|item| world.powertrain.engine(item));
    let output = engine.map_or_else(EngineOutput::default, |e| e.state.output());
    if vehicle.ignition.on() != was_ignition {
        car_sound(
            world.audio,
            world.physics,
            reference,
            key,
            vehicle.prototype,
            |a| a.switch,
        );
    }
    if chassis_active
        && let Some(engine) = engine
        && output.running
        && !was_running
    {
        let engine_pose = to_rapier(world.items.get(engine.item).expect("engine item").pose);
        let sequence = world.attempts.wrapping_mul(4);
        let factor = 0.85 + 0.3 * start_sample(DEFAULT_SEED, sequence);
        let vertical = 2.0 * start_sample(DEFAULT_SEED, sequence.wrapping_add(1)) - 1.0;
        let angle = std::f32::consts::TAU * start_sample(DEFAULT_SEED, sequence.wrapping_add(2));
        let radius = start_sample(DEFAULT_SEED, sequence.wrapping_add(3)).cbrt();
        let horizontal = (1.0 - vertical * vertical).sqrt();
        let offset = rapier3d::math::Vector::new(
            horizontal * angle.cos(),
            vertical,
            horizontal * angle.sin(),
        ) * radius;
        let impulse = engine_pose.rotation
            * rapier3d::math::Vector::Y
            * engine.record.starting_push_force
            * factor
            * FIXED_DT;
        world.physics.edit_body(chassis, |body| {
            body.apply_impulse_at_point(impulse, engine_pose.translation + offset, true)
        });
    }
    let pushed = world
        .interaction
        .push_steering()
        .filter(|(body, _)| world.physics.vehicle_of(*body) == Some(key))
        .map(|(_, steer)| steer);
    vehicle.steer = vehicle_steer(
        vehicle.steer,
        pushed,
        (driving || remote.is_some()).then_some(controls.steer),
    );
    let drive = DriveInput {
        throttle,
        brake,
        steer: vehicle.steer,
        // Source `handbrake` axis beside the lever state.
        handbrake: (vehicle.handbrake as u8 as f32).max(controls.handbrake),
    };
    // Source `carscript.Update` plays the handbrake sound when the handbrake leaves or reaches 0.
    if (vehicle.pedal_inputs.handbrake > 0.01) != (drive.handbrake > 0.01) {
        car_sound(
            world.audio,
            world.physics,
            reference,
            key,
            vehicle.prototype,
            |a| a.handbrake,
        );
    }
    vehicle.pedal_inputs = crate::simulation::control_pedals::PedalInputs {
        gas: throttle,
        brake,
        clutch: drivetrain.clutch,
        handbrake: drive.handbrake,
    };
    // Source `carscript.Update` calls `Balance()` for a seated driver or an in-use push grip.
    // A push leaves source `isteer` at 0 after `ResetInput`, so the balance steer is 0.
    // Source `isteer` is right minus left. Port steer is left minus right.
    if chassis_active
        && (driving || remote.is_some() || pushed.is_some())
        && let Some(balance) = &world_vehicle(reference, vehicle.prototype).balance
    {
        let steer = if driving || remote.is_some() {
            -controls.steer.clamp(-1., 1.)
        } else {
            0.
        };
        apply_balance(
            world.physics,
            chassis,
            balance,
            &mut vehicle.balance,
            speed.abs() * 3.6,
            steer,
        );
    }
    if chassis_active {
        let mut torques = if engine.is_some() {
            crate::simulation::drivetrain::transmitted_torques(output, gearbox, gear)
        } else {
            Default::default()
        };
        if let Some(pedals) = &record.pedals {
            if driving {
                vehicle.pedals.disabled = output.running;
            }
            let disabled = vehicle.pedals.disabled;
            // Source `UpdMulti` uses `bikepedal.multiGas` when the remote pedals more.
            if let Some(r) = remote
                && r.bike_gas > vehicle.pedals.gas
            {
                vehicle.pedals.multi_gas = r.bike_gas;
            }
            torques.pedal_nm = step_pedals(
                world,
                reference,
                key,
                vehicle,
                pedals,
                disabled,
                controls.throttle,
            );
        }
        world.physics.apply_vehicle_forces(key, drive, torques);
    }
    // The sender half of source `carscript.UpdMulti`. It sends the driver controls after AutoShift.
    if driving {
        vehicle.net.sent = Some(crate::net::car::CarFloats {
            steer: vehicle.steer,
            horn: (input.held & keys::HORN != 0) as u8 as f32,
            gas: drivetrain.throttle,
            brake: drivetrain.brake,
            clutch: drivetrain.clutch,
            bike_gas: vehicle.pedals.gas,
        });
    }
}

/// Source `bikepedalscript.Update()`. Returns the motor torque of each pedal wheel.
/// `running`: source `disabled`. `throttle`: source `inputscript.throttle`.
fn step_pedals(
    world: &mut CarWorld,
    reference: &ReferenceManifest,
    key: &ObjectKey,
    vehicle: &mut VehicleRuntime,
    pedals: &crate::reference::PedalsReference,
    running: bool,
    throttle: f32,
) -> Option<f32> {
    use crate::simulation::pedals;
    let record = world_vehicle(reference, vehicle.prototype);
    let seated = world.player.seat_vehicle() == Some(key)
        && world.player.seat_source() == Some(&record.seats[pedals.seat as usize].source);
    let mut rpms = [0.; pedals::MAX_WHEELS];
    for (rpm, wheel) in rpms.iter_mut().zip(&pedals.driven_wheels) {
        *rpm = world.physics.wheel_motion(key, *wheel as usize).0;
    }
    let mounted = pedals
        .driven_slots
        .iter()
        .filter(|slot| {
            record.wheels.iter().enumerate().any(|(i, wheel)| {
                wheel.slot == **slot && world.physics.wheel_assembly(key, i).has_rim()
            })
        })
        .count();
    let player = &reference.player;
    let step = pedals::step(
        pedals,
        player.pedaling_nm,
        player.pedaling_rpm,
        &mut vehicle.pedals,
        seated,
        throttle,
        &rpms[..pedals.driven_wheels.len()],
        mounted,
        running,
        FIXED_DT,
    );
    if step.angle != 0. {
        vehicle.pedal_angle = (vehicle.pedal_angle + step.angle) % 360.;
    }
    step.torque
}
