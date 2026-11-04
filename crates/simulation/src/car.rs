//! Source `carscript`: the state of one car copy and its fixed step.
//! `GarageRuntime` holds the copies and calls `VehicleRuntime::step` once for each car.
mod reach;
mod step;
pub use reach::CarFrames;
pub use step::{CarInput, CarWorld};

use crate::reference::{ItemId, ReferenceManifest};
use crate::simulation::{
    FIXED_DT,
    ignition::IgnitionState,
    powertrain::{Powertrain, PowertrainParts},
};
use crate::vehicle::PhysicsWorld;
use crate::world::ObjectKey;
use rapier3d::prelude::RigidBodyHandle;

/// The vehicle record of car prototype `index`. The binder adds runtimes only for car prototypes.
pub fn world_vehicle(
    reference: &ReferenceManifest,
    index: usize,
) -> &crate::reference::WorldVehicleReference {
    reference.world.prototypes[index]
        .vehicle
        .as_ref()
        .expect("car prototype")
}

/// The source electronics cutoff. `electronicsscript` output above it powers ignition.
const ELECTRONICS_CUTOFF: f32 = 0.2;

/// Per-copy inputs of a world car runtime.
pub struct WorldCarParts {
    /// Stable seed of the copy, from its object key.
    pub seed: u64,
    /// Car root `tankscript` item.
    pub fuel_tank: Option<ItemId>,
    /// Enabled solid chassis colliders in the chassis frame.
    pub recovery_bounds: crate::reference::Bounds,
    /// Other bodies of the car instance.
    pub bodies: Vec<RigidBodyHandle>,
    /// Electronics nodes of the car.
    pub electronics: Vec<crate::electronics::NodeElectronics>,
    /// `carscript.electronics` index in `electronics`. None: the car has no electronics.
    pub car_electronics: Option<usize>,
    /// Links of each record light controller.
    pub lights: Vec<crate::simulation::car_lights::CarLightLinks>,
    /// The item of the `carscript.InteractSwitchLights` usable.
    pub light_switch: Option<ItemId>,
    /// The item of the `carscript.InteractHandbrake` usable.
    pub handbrake_switch: Option<ItemId>,
    /// Plate label texts in `WorldVehicleReference::plates` order.
    pub plates: Vec<String>,
    /// Source `carscript.blood`.
    pub blood: Option<crate::crash::Blood>,
    /// `wheelgraphicsscript.T` of each wheel, in wheel order.
    pub wheel_graphics: Vec<crate::world::wheel_graphics::WheelGraphic>,
}

/// Car-side state of a car copy. The world change records keep it while the car is retired
/// and in the save.
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VehicleState {
    pub ignition: bool,
    pub gear: usize,
    pub handbrake: bool,
    pub trip_m: f64,
    pub requested_headlights: u8,
    pub steer: f32,
    /// Source save `blood`: the car blood `ON`.
    pub blood: bool,
}

/// Source `carscript` state of one car copy. Engine state lives on the engine part.
pub struct VehicleRuntime {
    /// Index of the car prototype in `reference.world.prototypes`.
    pub prototype: usize,
    pub ignition: IgnitionState,
    pub handbrake: bool,
    /// Distance this vehicle has driven, in meters.
    pub trip_m: f64,
    /// Source carscript.steer over steerAngle. It keeps its value when nobody drives.
    pub steer: f32,
    pub drivetrain: crate::simulation::drivetrain::DrivetrainState,
    pub exhaust: crate::exhaust::ExhaustState,
    pub coolant_steam: crate::coolant_steam::CoolantSteamState,
    /// Source `carscript.Tank`: the root tank, else the tank part in `fuel_tank_slot`, else
    /// the first tank attached to that part.
    /// Each step finds it once at its start.
    pub fuel_tank: Option<ItemId>,
    /// Car root `tankscript` item.
    root_tank: Option<ItemId>,
    /// Source `carscript.tankslot`.
    pub fuel_tank_slot: Option<crate::reference::PartSlotId>,
    /// Source `carscript.startingTime`.
    pub starter_delay_seconds: f32,
    /// Electronics nodes of the car. EMPs jam them.
    pub electronics: Vec<crate::electronics::NodeElectronics>,
    /// `carscript.electronics` index in `electronics`.
    pub car_electronics: Option<usize>,
    /// Gauge state. The presentation advances it each frame.
    pub instruments: Option<crate::simulation::instruments::InstrumentState>,
    /// Light controllers of the car.
    pub lights: crate::simulation::car_lights::CarLights,
    /// Brake input of the last step. The brake lamps read it.
    brake: f32,
    pub wheel_effects: crate::wheel_effects::WheelEffects,
    pub lever: Option<crate::simulation::lever::LeverState>,
    pub(crate) recovery_bounds: crate::reference::Bounds,
    /// Bodies of the car hierarchy other than the chassis. Recovery moves them with it.
    pub(crate) bodies: Vec<RigidBodyHandle>,
    /// Absolute position at the start of the current step.
    pub(crate) previous: [f64; 3],
    /// Source `carscript.lspeed`: forward speed in km/h at the start of the current step.
    pub(crate) last_speed: f32,
    /// Source `HornHit` state. It is not saved.
    pub horn_hit: Option<crate::crash::HornHit>,
    /// Source `carscript.blood`.
    pub blood: Option<crate::crash::Blood>,
    /// Parts mounted on this car. Each step finds them once at its start.
    powertrain: Powertrain,
    /// `wheelgraphicsscript.T` of each wheel, in wheel order.
    pub wheel_graphics: Vec<crate::world::wheel_graphics::WheelGraphic>,
    /// Requested headlight state: off, low or high.
    pub requested_headlights: u8,
    /// The item of the `carscript.InteractSwitchLights` usable.
    pub light_switch: Option<ItemId>,
    /// The item of the `carscript.InteractHandbrake` usable.
    pub handbrake_switch: Option<ItemId>,
    /// Plate label texts, in `WorldVehicleReference::plates` order.
    pub plates: Vec<String>,
    /// `carscript.kmhmeters` slots: (row, slot) pairs.
    pub kmh_slots: Vec<(usize, crate::reference::PartSlotId)>,
    /// Source `carscript` balance state. It is not saved. Recovery resets it.
    pub balance: crate::simulation::balance::BalanceState,
    /// Source `bikepedalscript` state. It is not saved.
    pub pedals: crate::simulation::pedals::PedalState,
    /// Total pedal turn in degrees, modulo 360. The presentation poses the pedal nodes from it.
    pub pedal_angle: f32,
    /// Source `carscript` drive leg poses of the pedals.
    pub pedal_reach: crate::simulation::reach::PedalReach,
    /// Source `carscript` throttle, brake, clutch and handbrake of the last step.
    /// The control pedals and the handbrake lamp read them.
    pub pedal_inputs: crate::simulation::control_pedals::PedalInputs,
    /// Source `UpdPedal` state of the control pedals. It is not saved.
    pub control_pedals: crate::simulation::control_pedals::ControlPedals,
    /// Picks the `valtoSounds` clip of each shift.
    shift_random: crate::world::UnityRandom,
    /// Multiplayer state. It is not saved.
    pub net: crate::net::car::CarNet,
}

impl VehicleRuntime {
    /// The `carscript.electronics` output. It is 1 without electronics.
    pub fn electronics_output(&self) -> f32 {
        self.car_electronics
            .map_or(1., |i| self.electronics[i].state.output())
    }
    /// `carscript.Electronics()`. True with no electronics, or with output above the cutoff.
    pub fn electronics_enabled(&self) -> bool {
        self.car_electronics
            .is_none_or(|i| self.electronics[i].state.output() > ELECTRONICS_CUTOFF)
    }
    /// Runtime of a car copy of world prototype `prototype`.
    pub fn world(
        prototype: usize,
        record: &crate::reference::WorldVehicleReference,
        skid: &crate::reference::SkidReference,
        parts: WorldCarParts,
    ) -> Self {
        Self {
            prototype,
            handbrake: record.handbrake,
            trip_m: 0.0,
            steer: 0.,
            ignition: IgnitionState::default(),
            drivetrain: crate::simulation::drivetrain::DrivetrainState::new(record.into()),
            lever: record.lever.as_ref().map(|lever| {
                crate::simulation::lever::LeverState::rest(lever, record.driving.initial_gear)
            }),
            exhaust: crate::exhaust::ExhaustState::new(&record.exhaust, parts.seed),
            coolant_steam: crate::coolant_steam::CoolantSteamState::new(
                record.coolant_steam.as_ref(),
                parts.seed ^ 0x7374_6561_6d00,
            ),
            fuel_tank: parts.fuel_tank,
            root_tank: parts.fuel_tank,
            fuel_tank_slot: None,
            starter_delay_seconds: record.starter_delay_seconds,
            electronics: parts.electronics,
            car_electronics: parts.car_electronics,
            instruments: record
                .instruments
                .as_ref()
                .map(crate::simulation::instruments::InstrumentState::new),
            lights: crate::simulation::car_lights::CarLights::new(&record.lights, parts.lights),
            brake: 0.,
            wheel_effects: crate::wheel_effects::WheelEffects::new(
                &record.wheel_effects,
                skid,
                parts.seed,
            ),
            recovery_bounds: parts.recovery_bounds,
            bodies: parts.bodies,
            previous: [0.0; 3],
            last_speed: 0.,
            horn_hit: None,
            blood: parts.blood,
            powertrain: Powertrain::default(),
            wheel_graphics: parts.wheel_graphics,
            requested_headlights: 0,
            light_switch: parts.light_switch,
            handbrake_switch: parts.handbrake_switch,
            plates: parts.plates,
            kmh_slots: Vec::new(),
            balance: Default::default(),
            pedals: Default::default(),
            pedal_angle: 0.,
            pedal_reach: Default::default(),
            pedal_inputs: crate::simulation::control_pedals::PedalInputs {
                handbrake: record.handbrake as u8 as f32,
                ..Default::default()
            },
            control_pedals: Default::default(),
            shift_random: crate::world::UnityRandom::new((parts.seed ^ 0x7661_6c74_6f00) as i32),
            net: crate::net::car::CarNet::new(parts.seed),
        }
    }
    /// The vehicle record of the car prototype.
    pub fn record<'a>(
        &self,
        reference: &'a ReferenceManifest,
    ) -> &'a crate::reference::WorldVehicleReference {
        world_vehicle(reference, self.prototype)
    }
    /// A hash of the state that the car copy presents: gauges, lamp materials, node
    /// visibility and the poses of `wheels`. It changes when the presentation must sync.
    pub fn presentation_signature(&self, wheels: &[crate::vehicle::WheelState]) -> u64 {
        use std::hash::{Hash, Hasher};
        let mut h = std::hash::DefaultHasher::new();
        // Steer, suspension and roll move the wheel `T` nodes.
        for w in wheels {
            for v in [w.steering, w.suspension_length, w.angle] {
                v.to_bits().hash(&mut h);
            }
        }
        self.requested_headlights.hash(&mut h);
        if let Some(s) = &self.instruments {
            for a in s.needle_angles.iter().chain(&s.digit_angles) {
                a.to_bits().hash(&mut h);
            }
            (s.lamp_bits, s.digits_lit).hash(&mut h);
        }
        self.pedal_angle.to_bits().hash(&mut h);
        for v in self.control_pedals.values() {
            v.to_bits().hash(&mut h);
        }
        if let Some(l) = &self.lever {
            let (t, r) = (l.translation.to_array(), l.rotation.to_array());
            for v in t.iter().chain(&r) {
                v.to_bits().hash(&mut h);
            }
        }
        for m in self.lights.materials() {
            m.choice.map(|c| (c.light, c.state, c.on)).hash(&mut h);
        }
        for v in self.lights.visibility() {
            v.hash(&mut h);
        }
        self.blood.as_ref().map(|b| b.shown).hash(&mut h);
        h.finish()
    }
    /// Source `carscript.InteractSwitchLights`: off, low, high.
    pub fn cycle_headlights(&mut self) {
        self.requested_headlights = (self.requested_headlights + 1) % 3;
    }
    /// The material that renderer node `node` of a world car shows. None: the authored material.
    pub fn renderer_material<'a>(
        &self,
        record: &'a crate::reference::WorldVehicleReference,
        node: u32,
    ) -> Option<&'a crate::reference::MaterialLink> {
        if let Some(choice) = self
            .lights
            .materials()
            .iter()
            .find(|m| m.node == node)
            .and_then(|m| m.choice)
        {
            return choice.link(&record.lights);
        }
        let (r, state) = (record.instruments.as_ref()?, self.instruments.as_ref()?);
        let pick = |on: bool, a, b| if on { a } else { b };
        if let Some((i, lamp)) = r.lamps.iter().enumerate().find(|(_, l)| l.node.0 == node) {
            return Some(pick(state.lamp_bits & (1 << i) != 0, &lamp.on, &lamp.off));
        }
        if let Some(digit) = r.digits().iter().find(|d| d.node.0 == node) {
            return digit.material(state.digits_lit);
        }
        r.all_backlights().find_map(|b| match b {
            crate::reference::MaterialSwitchReference::Renderer { node: n, on, off }
                if n.0 == node =>
            {
                Some(pick(state.digits_lit, on, off))
            }
            _ => None,
        })
    }
    /// Visibility that a world car sets on node `node`: lamp Working/nWorking nodes and the
    /// light switch state nodes. None: the car does not set it.
    pub fn node_visible(
        &self,
        record: &crate::reference::WorldVehicleReference,
        node: u32,
    ) -> Option<bool> {
        if let Some(switch) = &record.light_switch
            && let Some(state) = switch.state_nodes.iter().position(|n| n.contains(&node))
        {
            return Some(state == self.requested_headlights as usize);
        }
        self.lights
            .visibility()
            .iter()
            .find(|(n, _)| *n == node)
            .map(|(_, visible)| *visible)
    }
    pub fn state(&self) -> VehicleState {
        VehicleState {
            ignition: self.ignition.ignition,
            gear: self.drivetrain.gear(),
            handbrake: self.handbrake,
            trip_m: self.trip_m,
            requested_headlights: self.requested_headlights,
            steer: self.steer,
            blood: self.blood.as_ref().is_some_and(|b| b.on),
        }
    }
    /// Restores a captured state. The gear must be in the record range.
    /// The gear lever rests on the restored gear.
    pub fn restore(
        &mut self,
        record: &crate::reference::WorldVehicleReference,
        state: &VehicleState,
    ) {
        self.ignition.ignition = state.ignition;
        self.drivetrain.restore_gear(state.gear);
        self.lever = record
            .lever
            .as_ref()
            .map(|lever| crate::simulation::lever::LeverState::rest(lever, state.gear));
        self.handbrake = state.handbrake;
        self.pedal_inputs.handbrake = state.handbrake as u8 as f32;
        self.trip_m = state.trip_m;
        self.requested_headlights = state.requested_headlights;
        self.steer = state.steer;
        // Source load calls `GotDirty` if the saved blood is on. Else the prototype state stays.
        if let (true, Some(blood)) = (state.blood, &mut self.blood) {
            blood.got_dirty();
        }
    }
}

/// A `carscript` one-shot sound of car `key` at its chassis. `pick` selects it.
pub(crate) fn car_sound(
    audio: &mut Option<crate::audio::AudioState>,
    physics: &PhysicsWorld,
    reference: &ReferenceManifest,
    key: &ObjectKey,
    prototype: usize,
    pick: impl Fn(&crate::reference::CarAudioReference) -> Option<crate::reference::CarShotReference>,
) {
    let (Some(audio), Some(shot)) = (
        audio.as_mut(),
        world_vehicle(reference, prototype)
            .audio
            .as_ref()
            .and_then(pick),
    ) else {
        return;
    };
    audio.car_shot(
        key,
        &shot,
        *physics.rapier().bodies[physics.chassis_handle(key)].position(),
    );
}

pub fn instrument_sample(
    items: &crate::interaction::items::ItemStore,
    physics: &PhysicsWorld,
    powertrain: &PowertrainParts,
    key: &ObjectKey,
    vehicle: &VehicleRuntime,
    record: &crate::reference::WorldVehicleReference,
    headlights: u8,
) -> crate::simulation::instruments::InstrumentSample {
    let limits = &record.gauge_limits;
    let amount = |id: ItemId| {
        items
            .get(id)
            .and_then(|i| i.fluid.as_ref())
            .map_or(0., |f| f.finite_total())
    };
    let engine = vehicle
        .powertrain
        .engine
        .and_then(|item| powertrain.engine(item));
    crate::simulation::instruments::InstrumentSample {
        speed_kmh: physics.snapshot(key).speed_mps * 3.6,
        fuel_liters: vehicle.fuel_tank.map_or(0., amount),
        oil_liters: engine.and_then(|e| e.oil_tank).map_or(0., amount),
        temperature: engine.map_or(0., |e| e.state.output().temperature),
        rpm: engine.map_or(0., |e| e.state.output().rpm),
        two_stroke: engine.is_some_and(|e| e.record.two_stroke),
        oil_penalty: engine.map_or(0., |e| e.state.output().oil_penalty),
        oil_led_two_stroke: record.oil_led_two_stroke,
        headlights,
        local_ignition: vehicle.ignition.ignition
            && vehicle.electronics_output() > limits.electronics_off_threshold,
        electronics_output: vehicle.electronics_output(),
        handbrake: vehicle.pedal_inputs.handbrake,
        engine: engine.is_some(),
        fuel_rate: engine.map_or(0., |e| e.state.output().consumed_fuel_liters / FIXED_DT),
        distance_m: vehicle.trip_m,
    }
}

/// Source `carscript.Balance()` for one fixed step. Returns the roll torque as an angular impulse.
/// `speed_kmh`: source `speed`. `steer`: source `isteer`, -1..1.
fn apply_balance(
    physics: &mut PhysicsWorld,
    chassis: RigidBodyHandle,
    reference: &crate::reference::BalanceReference,
    state: &mut crate::simulation::balance::BalanceState,
    speed_kmh: f32,
    steer: f32,
) {
    let rotation = *physics.rapier().bodies[chassis].rotation();
    let torque = crate::simulation::balance::step(
        reference,
        state,
        speed_kmh,
        steer,
        rotation * rapier3d::math::Vector::Y,
        rotation * -rapier3d::math::Vector::Z,
        FIXED_DT,
    );
    // Source `AddRelativeTorque`. The torque is in the chassis frame.
    physics.edit_body(chassis, |body| {
        body.apply_torque_impulse(rotation * torque * FIXED_DT, true)
    });
}

/// Source carscript steer. A push grip wins, then the driver. Else the car keeps its steer.
fn vehicle_steer(current: f32, pushed: Option<f32>, driven: Option<f32>) -> f32 {
    pushed.or(driven).unwrap_or(current)
}

impl VehicleRuntime {
    /// The engine part item mounted at the last step.
    pub fn engine(&self) -> Option<ItemId> {
        self.powertrain.engine
    }
    /// Finds the parts and the fuel tank mounted on car `key`. The step reads the result.
    pub fn refresh_parts(
        &mut self,
        key: &ObjectKey,
        powertrain: &PowertrainParts,
        items: &crate::interaction::items::ItemStore,
        parts: &crate::interaction::parts::PartAttachments,
    ) {
        use crate::interaction::items::ItemLocation;
        self.powertrain = powertrain.mounted_on(items, key);
        // The source uses the part `tankscript`, else the first tank of its `tosaveitem` (Bike03 tanks).
        let tank = |item: ItemId| items.get(item).is_some_and(|i| i.fluid.is_some());
        self.fuel_tank = self.root_tank.or_else(|| {
            let part = self.fuel_tank_slot.and_then(|slot| parts.slot_item(slot))?;
            if tank(part) {
                return Some(part);
            }
            items
                .iter()
                .find(|i| i.location == ItemLocation::Attached(part) && tank(i.id))
                .map(|i| i.id)
        });
    }
    /// Source `carscript.Update`. Eases the gear lever and control pedals by render time `dt`.
    pub fn advance_controls(&mut self, reference: &ReferenceManifest, dt: f32) {
        let record = world_vehicle(reference, self.prototype);
        if let (Some(lever), Some(r)) = (&mut self.lever, &record.lever) {
            lever.step(r, &reference.driving.lever, dt);
        }
        self.control_pedals
            .step(&record.control_pedals, self.pedal_inputs, dt);
    }
    /// Car inputs of the trailer lights hitched to this car.
    pub fn light_input(
        &self,
        reference: &ReferenceManifest,
    ) -> crate::simulation::car_lights::CarLightInput {
        crate::simulation::car_lights::CarLightInput {
            requested: self.requested_headlights,
            ignition: self.ignition.ignition,
            brake: self.brake,
            gear_ratio: world_vehicle(reference, self.prototype).gear_ratios
                [self.drivetrain.gear()],
        }
    }
}
