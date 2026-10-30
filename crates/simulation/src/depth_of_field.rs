//! Source `postprocesscontrol` focus of the `DOF` volume.
use crate::lighting::{CycleOutput, CyclePhase, inverse_lerp, lerp};
use crate::reference::postprocessing::FocusControl;
use crate::vehicle::{PhysicsWorld, VehiclePose, to_rapier};
use rapier3d::math::Vector;
use rapier3d::prelude::{ColliderHandle, QueryFilter, Ray};

/// The applied `dof.focusDistance` (`outcucc`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Focus {
    pub distance: f32,
}

impl Focus {
    pub fn new(r: &FocusControl) -> Self {
        Self {
            distance: r.focus_distance,
        }
    }
    /// `postprocesscontrol.Update`. `hit` is the distance of the camera ray, None without a hit.
    pub fn step(&mut self, r: &FocusControl, hit: Option<f32>, cycle: &CycleOutput, dt: f32) {
        let [min, max] = r.focus;
        let mut focus = hit.map_or(max, |d| lerp(min, max, inverse_lerp(r.ray[0], r.ray[1], d)));
        let limit = match cycle.phase {
            CyclePhase::Day => None,
            CyclePhase::Night => Some(r.night_focus),
            CyclePhase::Dusk => Some(lerp(max, r.night_focus, cycle.progress)),
            CyclePhase::Dawn => Some(lerp(r.night_focus, max, cycle.progress)),
        };
        // Mathf.Clamp tests the minimum first.
        if let Some(limit) = limit {
            if focus < min {
                focus = min;
            } else if focus > limit {
                focus = limit;
            }
        }
        self.distance = lerp(self.distance, focus, r.focus_lerp * dt);
    }
}

/// The `h.forward` ray. Triggers count (queriesHitTriggers). Unity rays ignore colliders containing the origin.
pub fn ray(physics: &PhysicsWorld, camera: VehiclePose, r: &FocusControl) -> Option<f32> {
    let camera = to_rapier(camera);
    let ray = Ray::new(camera.translation, camera.rotation * -Vector::Z);
    let targets = |_: ColliderHandle, collider: &rapier3d::prelude::Collider| {
        collider.is_enabled()
            && collider.collision_groups().memberships.bits() & r.layer_mask != 0
            && !collider
                .shape()
                .contains_point(collider.position(), ray.origin)
    };
    physics
        .queries(QueryFilter::default().predicate(&targets))
        .cast_ray(&ray, r.ray[1], true)
        .map(|(_, distance)| distance)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::reference::postprocessing::Lens;

    #[test]
    fn ray_sets_focus_and_night_clamps_it() {
        let r = FocusControl {
            lens: Lens {
                aperture: 1.,
                focal_length: 8.2,
            },
            focus_distance: 0.1,
            layer_mask: 1,
            focus: [0., 0.575],
            night_focus: 0.5,
            focus_lerp: 5.,
            ray: [0., 1.],
        };
        let mut cycle = CycleOutput::default();
        let mut f = Focus::new(&r);
        // A full lerp step reaches the target.
        f.step(&r, Some(0.5), &cycle, 0.2);
        assert!((f.distance - 0.2875).abs() < 1e-6);
        f.step(&r, None, &cycle, 0.2);
        assert!((f.distance - 0.575).abs() < 1e-6);
        cycle.phase = CyclePhase::Night;
        f.step(&r, None, &cycle, 0.2);
        assert!((f.distance - 0.5).abs() < 1e-6);
        cycle.phase = CyclePhase::Dusk;
        cycle.progress = 0.5;
        f.step(&r, None, &cycle, 0.1);
        assert!((f.distance - 0.51875).abs() < 1e-6);
    }
}
