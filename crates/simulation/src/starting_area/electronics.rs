//! Source `empscript` and `electronicsscript` updates: EMPs jam the electronics in range.
use super::GarageRuntime;

impl GarageRuntime {
    /// Advances each EMP, then decays and jams every electronics state.
    pub(super) fn advance_electronics(&mut self, dt: f32) {
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
        self.stream.expose_bus_doors(emps, &self.area.items, dt);
        self.stream.expose_radios(emps, &self.area.items, dt);
        self.stream.expose_detectors(emps, &self.area.items, dt);
        self.stream.expose_dials(emps, &self.area.items, dt);
    }
}
