//! Source `empscript` and `electronicsscript` updates: EMPs jam the electronics in range.
use super::GarageRuntime;
use crate::reference::ReferenceManifest;

/// Layer of the road `kozlampa` electronics collider. The exporter checks it.
const ROAD_LAMP_LAYER: u32 = 0;

impl GarageRuntime {
    /// Advances each EMP, then decays and jams every electronics state.
    pub(super) fn advance_electronics(&mut self, reference: &ReferenceManifest, dt: f32) {
        for emp in &mut self.emps {
            emp.advance(dt);
        }
        let emps = &self.emps;
        // Car electronics sit on the car hierarchy: the chassis position stands for each node.
        for (key, vehicle) in &mut self.vehicles {
            if vehicle.electronics.is_empty() {
                continue;
            }
            let chassis = self.physics.chassis_handle(key);
            let at = self.physics.rapier().bodies[chassis].translation();
            for e in &mut vehicle.electronics {
                e.state.expose(emps, at, e.layer, dt);
            }
        }
        // Road lamps share one state. The player position stands for the visible lamps.
        let player =
            rapier3d::math::Vector::from_array(self.area.player.focus_position(&self.physics));
        self.lighting
            .road_electronics
            .expose(emps, player, ROAD_LAMP_LAYER, dt);
        let layer = reference.ambient.player_electronics.layer;
        let e = &mut self.player_electronics;
        e.update(dt);
        for emp in emps {
            if emp.jam(player, layer, e, dt) && emp.affect_ambient_sound {
                self.ambient.jam(e.output(), dt);
            }
        }
        self.stream.expose_bus_doors(emps, &self.area.items, dt);
        self.stream.expose_radios(emps, &self.area.items, dt);
        self.stream.expose_detectors(emps, &self.area.items, dt);
        self.stream.expose_dials(emps, &self.area.items, dt);
        if let Err(error) = self.stream.expose_electronics(emps, &self.area.items, dt) {
            log::error!("Electronics refresh failed: {}", error.0);
        }
    }
}
