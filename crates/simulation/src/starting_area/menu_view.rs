//! The start menu view. Port choice: the source menu shows a figure. This port shows the road
//! ahead of the start, from near the road surface, with the afternoon sun over the road.
//! Before the game starts, the frame camera and the day cycle output come from this view.
use crate::lighting::{CycleOutput, CycleState, sun_pitch};
use crate::reference::{CycleReference, ReferenceManifest};
use crate::world::{WorldOrigin, WorldStream};
use glam::{Quat, Vec3};

/// Metres along the road (world -Z) from the player spawn to the camera.
const AHEAD: f64 = 40.;
/// Camera height over the road.
const EYE: f32 = 0.6;
/// Metres along the road to the road point that the camera looks at. This keeps the near road centered.
const LOOK: f64 = 120.;
/// Lowest sun elevation in degrees. The sun stays in the upper half of the view.
const SUN_ELEVATION: f32 = 15.;
/// Cycle samples of the afternoon search.
const SAMPLES: usize = 512;

pub(super) struct MenuView {
    absolute: [f64; 3],
    rotation: [f32; 4],
    /// Source cycle seconds of the day-phase afternoon sun at `SUN_ELEVATION`.
    seconds: f64,
    /// Source sun yaw in degrees that puts the sun over the view direction.
    sun_yaw: f32,
}

impl MenuView {
    pub(super) fn new(reference: &ReferenceManifest, stream: &WorldStream) -> Self {
        let spawn_z = f64::from(reference.area.player_spawn.translation[2]);
        // The generated road, not the ideal road curve: the road lags the curve and has its own height.
        let mut road = stream
            .road_surface_points()
            .skip_while(|point| point[2] > spawn_z - AHEAD);
        let mut absolute = road.next().expect("the road reaches the menu view");
        let target = road
            .find(|point| point[2] <= absolute[2] - LOOK)
            .expect("the road reaches the menu view target");
        absolute[1] += f64::from(EYE);
        // A level view: the horizon is in the middle.
        let forward = Vec3::new(
            (target[0] - absolute[0]) as f32,
            0.,
            (target[2] - absolute[2]) as f32,
        )
        .normalize();
        let rotation = Quat::from_rotation_y(f32::atan2(-forward.x, -forward.z));
        let cycle = &reference.lighting.cycle;
        let seconds = afternoon_seconds(cycle);
        let pitch = sun_pitch(cycle, seconds).to_radians();
        Self {
            absolute,
            rotation: rotation.to_array(),
            seconds,
            sun_yaw: sun_yaw(pitch, forward).to_degrees(),
        }
    }
    /// The camera pose relative to `origin`.
    pub(super) fn pose(&self, origin: &WorldOrigin) -> crate::vehicle::VehiclePose {
        crate::vehicle::VehiclePose {
            position: origin.local(self.absolute),
            rotation: self.rotation,
        }
    }
    pub(super) fn cycle(&self, reference: &CycleReference, state: &CycleState) -> CycleOutput {
        state.output_at(reference, self.seconds, self.sun_yaw)
    }
}

/// The last day-phase time whose sun is at `SUN_ELEVATION` or higher. The day phase has the
/// blue day sky; the dusk phase after it has the yellow sunset sky.
fn afternoon_seconds(reference: &CycleReference) -> f64 {
    let dawn = f64::from(reference.dawn_seconds);
    let dusk = f64::from(reference.day_seconds - reference.dusk_seconds);
    (0..=SAMPLES)
        .map(|i| dusk - (dusk - dawn) * i as f64 / SAMPLES as f64)
        .find(|&t| elevation(sun_pitch(reference, t)) >= SUN_ELEVATION)
        .unwrap_or(dusk)
}

/// Sun elevation in degrees of a source pitch in degrees. A pitch over 90 degrees is the afternoon side.
fn elevation(pitch: f32) -> f32 {
    pitch.to_radians().sin().asin().to_degrees()
}

/// Source yaw in radians that puts the sun over `forward`. The direction toward the sun is
/// `Ry(-yaw) * Rx(-pitch) * Z` = (-sin yaw cos pitch, sin pitch, cos yaw cos pitch).
fn sun_yaw(pitch: f32, forward: Vec3) -> f32 {
    if pitch.cos() < 0. {
        f32::atan2(forward.x, -forward.z)
    } else {
        f32::atan2(-forward.x, forward.z)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The sky draws the sun at `Quat::from_euler(YXZ, -yaw, -pitch, 0) * Z`.
    fn toward_sun(pitch: f32, yaw: f32) -> Vec3 {
        Quat::from_euler(glam::EulerRot::YXZ, -yaw, -pitch, 0.) * Vec3::Z
    }

    #[test]
    fn sun_sits_over_the_view_direction() {
        let forward = Vec3::new(0.3, 0., -1.).normalize();
        for degrees in [8_f32, 172.] {
            let pitch = degrees.to_radians();
            let sun = toward_sun(pitch, sun_yaw(pitch, forward));
            let flat = Vec3::new(sun.x, 0., sun.z).normalize();
            assert!(flat.dot(forward) > 0.9999, "{degrees}: {sun}");
            assert!((sun.y - pitch.sin()).abs() < 1e-5);
        }
    }
}
