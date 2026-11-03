//! Source `flashlightscript.UpdMulti` (`SFlashlight`) and `busdoorscript` (`SBus`): lamp switches
//! and bus doors. The port reads them each `SYNC_TIME` and sends what changed since it last sent or
//! received it.
use super::items::Out;
use super::message::{BusMessage, LampMessage, Message};
use super::state::SYNC_TIME;
use crate::reference::{LightControllerKind, ReferenceManifest};
use crate::starting_area::GarageRuntime;
use std::collections::HashMap;

#[derive(Default)]
pub struct LampSync {
    timer: f32,
    /// World lamps by seed: on and broken bits.
    lamps: HashMap<u64, (bool, u64)>,
    /// Garage switches by control id.
    switches: HashMap<u32, bool>,
    /// Buses by seed: opened and signed bits.
    buses: HashMap<u64, (u32, u32)>,
}

impl LampSync {
    pub fn clear(&mut self) {
        *self = Self::default();
    }

    /// Sends the changes each `SYNC_TIME`. A lamp or bus that binds takes its first state as the
    /// baseline.
    pub(super) fn send(
        &mut self,
        reference: &ReferenceManifest,
        runtime: &GarageRuntime,
        dt: f32,
        out: &mut Out,
    ) {
        self.timer += dt;
        if self.timer < SYNC_TIME {
            return;
        }
        self.timer = 0.;
        for (key, on, broken) in runtime.stream.lamp_controls() {
            if let Some(known) = self.lamps.insert(key, (on, broken))
                && known != (on, broken)
            {
                out.push(&LampMessage {
                    key,
                    broken,
                    control: 0,
                    on: u32::from(on),
                });
            }
        }
        let controllers = &reference.lighting.controllers;
        for (c, state) in controllers.iter().zip(&runtime.lighting.controllers) {
            if c.kind == LightControllerKind::Switch
                && let Some(known) = self.switches.insert(c.id.0, state.on)
                && known != state.on
            {
                out.push(&LampMessage {
                    key: 0,
                    broken: 0,
                    control: c.id.0,
                    on: u32::from(state.on),
                });
            }
        }
        for (key, doors) in runtime.stream.bus_door_states() {
            let mut now = (0, 0);
            for (i, d) in doors.iter().enumerate().take(32) {
                now.0 |= u32::from(d.opened) << i;
                now.1 |= u32::from(d.signed) << i;
            }
            if let Some(known) = self.buses.insert(key, now)
                && known != now
            {
                out.push(&BusMessage {
                    key,
                    opened: now.0,
                    signed: now.1,
                });
            }
        }
    }

    /// Source `UpdFlashlight` and `UpdBus` of another peer.
    pub fn receive(
        &mut self,
        reference: &ReferenceManifest,
        runtime: &mut GarageRuntime,
        packet: &[u8],
    ) {
        if let Ok((_, m)) = LampMessage::decode(packet) {
            self.got_lamp(reference, runtime, &m);
        } else if let Ok((_, m)) = BusMessage::decode(packet) {
            self.got_bus(runtime, &m);
        }
    }

    /// Source `UpdFlashlight`.
    fn got_lamp(
        &mut self,
        reference: &ReferenceManifest,
        runtime: &mut GarageRuntime,
        m: &LampMessage,
    ) {
        let on = m.on != 0;
        if m.key != 0 {
            let stream = &mut runtime.stream;
            match stream.set_generated_lamp(m.key, on, m.broken, &mut runtime.area.items) {
                Ok(true) => {
                    if let Some(now) = stream.lamp_controls().find(|(k, ..)| *k == m.key) {
                        self.lamps.insert(m.key, (now.1, now.2));
                    }
                }
                Ok(false) => {}
                Err(error) => stream.fail(error),
            }
            return;
        }
        let controllers = &reference.lighting.controllers;
        if let Some(i) = controllers
            .iter()
            .position(|c| c.id.0 == m.control && c.kind == LightControllerKind::Switch)
            && runtime.lighting.controllers[i].on != on
        {
            runtime
                .lighting
                .use_control(&reference.lighting, controllers[i].id);
            self.switches.insert(m.control, on);
        }
    }

    /// Source `UpdBus`.
    fn got_bus(&mut self, runtime: &mut GarageRuntime, m: &BusMessage) {
        if runtime.stream.set_bus_doors(m.key, m.opened, m.signed) {
            self.buses.insert(m.key, (m.opened, m.signed));
        }
    }
}
