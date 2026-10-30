//! Authored mass, locked cargo mass, centre of mass overrides and the single carried-body override.
use super::PhysicsWorld;
use rapier3d::math::Vector;
use rapier3d::prelude::{MassProperties, RigidBodyHandle};

/// Preallocated non-vehicle cargo bodies.
pub(super) const CARGO_BODIES: usize = 64;

pub(super) struct BodyMassOverride {
    pub body: RigidBodyHandle,
    pub authored_mass_kg: f32,
    pub physical_mass_kg: f32,
}

impl PhysicsWorld {
    /// Source `Rigidbody.centerOfMass`: moves the local centre of mass and keeps mass and
    /// inertia. Later mass changes keep the override.
    pub fn set_center_of_mass(
        &mut self,
        body: RigidBodyHandle,
        local_com: Vector,
    ) -> Result<(), crate::reference::ReferenceError> {
        match self.center_of_mass.iter_mut().find(|(h, _)| *h == body) {
            Some(entry) => entry.1 = local_com,
            None => self.center_of_mass.push((body, local_com)),
        }
        if self.apply_center_of_mass(body) {
            Ok(())
        } else {
            self.clear_center_of_mass(body);
            Err(crate::reference::ReferenceError(
                "Center of mass needs additional mass".into(),
            ))
        }
    }

    /// Removes the override. The body takes its collider centre of mass again.
    pub fn clear_center_of_mass(&mut self, body: RigidBodyHandle) {
        let Some(index) = self.center_of_mass.iter().position(|(h, _)| *h == body) else {
            return;
        };
        self.center_of_mass.swap_remove(index);
        let mass = self.rapier().bodies[body].mass();
        let geometry = self.collider_mass_properties(body).mass();
        self.edit_body(body, |b| {
            b.set_additional_mass((mass - geometry).max(0.0), true)
        });
        self.recompute_body_mass(body);
    }

    fn collider_mass_properties(&self, body: RigidBodyHandle) -> MassProperties {
        let rapier = self.rapier();
        rapier.bodies[body]
            .colliders()
            .iter()
            .map(|h| &rapier.colliders[*h])
            .filter(|c| c.is_enabled())
            .map(|c| {
                c.mass_properties()
                    .transform_by(c.position_wrt_parent().unwrap())
            })
            .sum()
    }

    /// Moves the centre of mass of a body with an override. The additional mass takes the
    /// offset. Call after each mass change. Returns false when the body has no additional mass.
    pub(super) fn apply_center_of_mass(&mut self, body: RigidBodyHandle) -> bool {
        let Some(&(_, local_com)) = self.center_of_mass.iter().find(|(h, _)| *h == body) else {
            return true;
        };
        self.recompute_body_mass(body);
        let total = self.rapier().bodies[body].mass_properties().local_mprops;
        let geometry = self.collider_mass_properties(body);
        if total.mass() - geometry.mass() <= 0.0 {
            return false;
        }
        let target = MassProperties::with_principal_inertia_frame(
            local_com,
            total.mass(),
            total.principal_inertia(),
            total.principal_inertia_local_frame,
        );
        self.edit_body(body, |b| {
            b.set_additional_mass_properties(target - geometry, true)
        });
        self.recompute_body_mass(body);
        true
    }

    /// Returns authored mass with cargo while carry uses temporary physical mass.
    pub fn authored_body_mass(&self, body: RigidBodyHandle) -> f32 {
        self.body_mass_override
            .as_ref()
            .filter(|state| state.body == body)
            .map_or_else(
                || self.rapier().bodies[body].mass(),
                |state| state.authored_mass_kg,
            )
    }

    fn body_cargo(&self, body: RigidBodyHandle) -> f32 {
        self.body_cargo
            .iter()
            .find(|(handle, _)| *handle == body)
            .map_or(0.0, |(_, mass)| *mass)
    }

    /// Updates zero-density portable or assembly mass before any impulse.
    /// `mass_kg` excludes locked cargo. The body keeps its cargo.
    pub fn set_body_mass(&mut self, body: RigidBodyHandle, mass_kg: f32) {
        self.apply_body_mass(body, mass_kg + self.body_cargo(body));
    }

    /// Sets the authored mass with cargo. A carried body keeps its physical mass.
    fn apply_body_mass(&mut self, body: RigidBodyHandle, mass_kg: f32) {
        let physical_mass_kg = if let Some(state) = self
            .body_mass_override
            .as_mut()
            .filter(|state| state.body == body)
        {
            state.authored_mass_kg = mass_kg;
            state.physical_mass_kg
        } else {
            mass_kg
        };
        self.edit_body(body, |body| {
            body.set_additional_mass(physical_mass_kg, true)
        });
        self.recompute_body_mass(body);
        self.apply_center_of_mass(body);
    }

    /// Applies or clears the single carried-body mass override.
    pub fn set_body_mass_override(&mut self, body: RigidBodyHandle, mass_kg: Option<f32>) {
        if let Some(physical_mass_kg) = mass_kg {
            self.recompute_body_mass(body);
            let authored_mass_kg = self.authored_body_mass(body);
            assert!(
                self.body_mass_override
                    .as_ref()
                    .is_none_or(|state| state.body == body)
            );
            self.body_mass_override = Some(BodyMassOverride {
                body,
                authored_mass_kg,
                physical_mass_kg,
            });
            self.apply_body_mass(body, authored_mass_kg);
        } else if self
            .body_mass_override
            .as_ref()
            .is_some_and(|state| state.body == body)
        {
            let state = self.body_mass_override.take().unwrap();
            self.apply_body_mass(body, state.authored_mass_kg);
        }
    }

    /// Adds locked item mass to a carrier body. Vehicle cargo stays through `set_vehicle_mass`.
    /// Other bodies keep it through `set_body_mass`.
    pub fn add_cargo_mass(&mut self, body: RigidBodyHandle, mass_kg: f32) {
        self.recompute_body_mass(body);
        if let Some(vehicle) = self.vehicles.values_mut().find(|v| v.chassis == body) {
            let base = self.physics.bodies[body].mass() - vehicle.cargo_mass;
            vehicle.cargo_mass = (vehicle.cargo_mass + mass_kg).max(0.0);
            let mass = base + vehicle.cargo_mass;
            self.apply_vehicle_mass(body, mass);
            return;
        }
        let cargo = self.body_cargo(body);
        let base = self.authored_body_mass(body) - cargo;
        let cargo = (cargo + mass_kg).max(0.0);
        match self
            .body_cargo
            .iter()
            .position(|(handle, _)| *handle == body)
        {
            Some(index) if cargo <= 1e-6 => {
                self.body_cargo.swap_remove(index);
            }
            Some(index) => self.body_cargo[index].1 = cargo,
            None if cargo > 1e-6 => self.body_cargo.push((body, cargo)),
            None => {}
        }
        self.apply_body_mass(body, (base + cargo).max(0.001));
    }

    /// Forgets the cargo of a removed body.
    pub(super) fn forget_body_cargo(&mut self, body: RigidBodyHandle) {
        self.body_cargo.retain(|(handle, _)| *handle != body);
        self.center_of_mass.retain(|(handle, _)| *handle != body);
    }
}
