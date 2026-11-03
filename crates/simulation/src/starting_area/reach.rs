//! Car control reach of the seated player.
use super::GarageRuntime;
use crate::car::CarFrames;
use crate::reference::ReferenceManifest;
use crate::simulation::reach::ReachMove;
use crate::world::ObjectKey;

impl GarageRuntime {
    /// Sends the move of a car control when the player sits in car `key`.
    pub(super) fn reach_seated_car(
        &mut self,
        reference: &ReferenceManifest,
        key: &ObjectKey,
        control: impl FnOnce(&crate::reference::VehicleReach) -> Option<ReachMove>,
    ) {
        if self.area.player.seat_vehicle() != Some(key) {
            return;
        }
        let Some(vehicle) = self.vehicles.get(key) else {
            return;
        };
        let frames = CarFrames::new(
            reference,
            key,
            vehicle,
            &self.powertrain,
            &self.area.items,
            &self.physics,
        );
        if let Some(m) = control(&vehicle.record(reference).reach) {
            frames.send(&mut self.area.body, m);
        }
    }
}
