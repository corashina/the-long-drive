//! Source WheelCollider rules for one car wheel.
//!
//! wheelgraphicsscript.WheelSize picks the support profile. carscript.SetWheel
//! gives motor and brake torque. The wheel spins on its own inertia. Tire force
//! comes from the source slip curves multiplied by stiffness and suspension load.

use rapier3d::math::Vector;
use rapier3d::prelude::ColliderHandle;

use super::body::VehicleWheel;
use crate::reference::{WheelColliderReference, WheelFrictionCurve};

/// Wheel spin sub-steps in one physics step. Keeps stiff tire response stable.
const SPIN_SUBSTEPS: usize = 20;
/// Lowest ground speed used as the slip reference, in m/s.
const MIN_SLIP_SPEED: f32 = 1.0;
/// wheelgraphicsscript uses this mass when the wheel data has none.
const DEFAULT_WHEEL_MASS_KG: f32 = 20.0;

/// The parts mounted on one wheel slot.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum WheelAssembly {
    /// No rim. The car stands on the source hub.
    Hub,
    /// A rim without a tire.
    Rim { radius_m: f32, mass_kg: f32 },
    /// A rim with a tire. Quality is 1 for a new tire and 0 for a worn tire.
    Tire {
        radius_m: f32,
        mass_kg: f32,
        quality: f32,
    },
}

impl WheelAssembly {
    pub fn has_rim(self) -> bool {
        self != Self::Hub
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct WheelProfile {
    pub radius: f32,
    pub inertia: f32,
    pub forward_stiffness: f32,
    pub sideways_stiffness: f32,
}

/// Source mainscript stiffness values for tires and bare rims.
#[derive(Clone, Copy, Debug)]
pub(super) struct WheelSlip {
    /// Forward new, forward worn, sideways new, sideways worn.
    pub tire: [f32; 4],
    /// Forward, sideways.
    pub rim: [f32; 2],
}

/// Source wheelgraphicsscript.WheelSize, wheelscript.Refresh and gumiscript.Refresh.
pub(super) fn resolve_profile(
    slot: &VehicleWheel,
    slip: &WheelSlip,
    assembly: WheelAssembly,
) -> WheelProfile {
    let (radius, mass, forward, sideways) = match assembly {
        WheelAssembly::Hub => {
            return profile(
                slot.hub.radius_m,
                slot.hub.mass_kg,
                slot.hub.forward_stiffness,
                slot.hub.sideways_stiffness,
            );
        }
        WheelAssembly::Rim { radius_m, mass_kg } => (radius_m, mass_kg, slip.rim[0], slip.rim[1]),
        WheelAssembly::Tire {
            radius_m,
            mass_kg,
            quality,
        } => {
            // Source state 0 is a new tire. Quality is 1 - state / 4.
            let wear = 1.0 - quality.clamp(0.0, 1.0);
            let [forward_min, forward_max, sideways_min, sideways_max] = slip.tire;
            (
                radius_m,
                mass_kg,
                forward_min + (forward_max - forward_min) * wear,
                sideways_min + (sideways_max - sideways_min) * wear,
            )
        }
    };
    let mass = if mass == 0.0 {
        DEFAULT_WHEEL_MASS_KG
    } else {
        mass
    };
    let sideways = if slot.sideways_stiffness_modifier != 0.0 {
        sideways * slot.sideways_stiffness_modifier
    } else {
        sideways
    };
    profile(radius, mass, forward, sideways)
}

/// A bare source WheelCollider: its own radius, mass and curve stiffness.
pub fn collider_profile(collider: &WheelColliderReference) -> WheelProfile {
    profile(
        collider.radius,
        collider.wheel_mass_kg,
        collider.forward_friction.stiffness,
        collider.sideways_friction.stiffness,
    )
}

fn profile(radius: f32, mass: f32, forward: f32, sideways: f32) -> WheelProfile {
    WheelProfile {
        radius,
        inertia: (0.5 * mass * radius * radius).max(1e-4),
        forward_stiffness: forward,
        sideways_stiffness: sideways,
    }
}

#[derive(Clone, Copy, Debug)]
pub struct WheelContact {
    pub collider: ColliderHandle,
    pub point: Vector,
    pub normal: Vector,
}

#[derive(Clone, Copy, Debug)]
pub struct WheelState {
    pub profile: WheelProfile,
    /// Chassis-space wheel center at full compression.
    pub connection: Vector,
    pub steering: f32,
    pub angle: f32,
    pub previous_angle: f32,
    /// Rolling speed in rad/s. Positive rolls the car forward.
    pub omega: f32,
    /// Distance from the connection down to the wheel center.
    pub suspension_length: f32,
    pub contact: Option<WheelContact>,
}

impl WheelState {
    pub(super) fn new(connection: Vector, profile: WheelProfile, rest_length: f32) -> Self {
        Self {
            profile,
            connection,
            steering: 0.0,
            angle: 0.0,
            previous_angle: 0.0,
            omega: 0.0,
            suspension_length: rest_length,
            contact: None,
        }
    }

    pub fn rpm(&self) -> f32 {
        self.omega * 60.0 / std::f32::consts::TAU
    }
}

/// World values for one wheel solve.
#[derive(Clone, Copy, Debug)]
pub(super) struct WheelFrame {
    /// Chassis up, the suspension axis.
    pub up: Vector,
    /// Steered wheel axle.
    pub axle: Vector,
    /// Chassis velocity minus ground velocity at the contact point.
    pub velocity: Vector,
    /// Chassis mass carried by this wheel.
    pub sprung_mass: f32,
    pub gravity: f32,
}

/// Torque on one wheel. Motor torque has a sign. Brake torque is positive.
#[derive(Clone, Copy, Debug, Default)]
pub struct WheelTorques {
    pub motor_nm: f32,
    pub brake_nm: f32,
}

/// Source WheelFrictionCurve: rise to the extremum, fall to the asymptote, then hold.
pub(super) fn friction(curve: &WheelFrictionCurve, slip: f32) -> f32 {
    let s = slip.abs();
    let value = if s <= curve.extremum_slip {
        let t = s / curve.extremum_slip;
        curve.extremum_value * t * (2.0 - t)
    } else if s < curve.asymptote_slip {
        let t = (s - curve.extremum_slip) / (curve.asymptote_slip - curve.extremum_slip);
        curve.extremum_value
            + (curve.asymptote_value - curve.extremum_value) * t * t * (3.0 - 2.0 * t)
    } else {
        curve.asymptote_value
    };
    value.copysign(slip)
}

/// Tire and suspension result for one wheel contact.
#[derive(Clone, Copy, Debug)]
pub(super) struct WheelForce {
    pub up: Vector,
    pub load: f32,
    pub forward: Vector,
    pub side: Vector,
    /// Wheel surface speed along `forward`.
    pub surface_speed: f32,
    /// Tire force from the slip curves, in N.
    pub fx: f32,
    pub fy: f32,
    pub forward_peak: f32,
    pub sideways_peak: f32,
    /// Static grip: a braked wheel at rest, and sideways grip at low speed.
    pub hold_x: bool,
    pub hold_y: bool,
}

impl WheelForce {
    /// Accumulated tire impulse limits along `forward` and `side`.
    /// Static grip may reach the peak force either way. Sliding grip may
    /// reach the slip-curve force.
    pub fn limits(&self, dt: f32) -> [(f32, f32); 2] {
        let range = |force: f32, peak: f32, hold: bool| {
            if hold {
                (-peak * dt, peak * dt)
            } else {
                ((force * dt).min(0.0), (force * dt).max(0.0))
            }
        };
        [
            range(self.fx, self.forward_peak, self.hold_x),
            range(self.fy, self.sideways_peak, self.hold_y),
        ]
    }

    /// Keeps an accumulated impulse pair inside the friction ellipse.
    pub fn ellipse(&self, impulse: [f32; 2], dt: f32) -> [f32; 2] {
        let usage = ratio(impulse[0], self.forward_peak * dt).powi(2)
            + ratio(impulse[1], self.sideways_peak * dt).powi(2);
        if usage > 1.0 {
            let scale = usage.sqrt().recip();
            [impulse[0] * scale, impulse[1] * scale]
        } else {
            impulse
        }
    }
}

/// Integrates wheel spin and returns the contact force terms.
/// The state must hold the new contact and suspension length.
pub(super) fn solve(
    state: &mut WheelState,
    collider: &WheelColliderReference,
    frame: WheelFrame,
    torques: WheelTorques,
    dt: f32,
) -> Option<WheelForce> {
    let profile = state.profile;
    let h = dt / SPIN_SUBSTEPS as f32;
    let brake_step = torques.brake_nm.max(0.0) / profile.inertia * h;
    let Some(contact) = state.contact else {
        for _ in 0..SPIN_SUBSTEPS {
            spin(
                state,
                torques.motor_nm,
                0.0,
                collider.rotational_damping,
                brake_step,
                h,
            );
        }
        return None;
    };
    // Source rest position is targetPosition of the travel below the connection.
    let rest = collider.suspension_distance * collider.target_position;
    let compression_rate = -frame.velocity.dot(frame.up);
    let load = (frame.sprung_mass * frame.gravity
        + collider.spring * (rest - state.suspension_length)
        + collider.damper * compression_rate)
        .max(0.0);
    let normal = contact.normal;
    let mut forward = normal.cross(frame.axle);
    if forward.length_squared() < 1e-8 {
        forward = frame.up.cross(frame.axle);
    }
    let forward = forward.normalize();
    let side = forward.cross(normal).normalize();
    let vx = frame.velocity.dot(forward);
    let vy = frame.velocity.dot(side);
    let reference_speed = vx.abs().max(MIN_SLIP_SPEED);
    let forward_peak = collider.forward_friction.extremum_value * profile.forward_stiffness * load;
    let sideways_peak =
        collider.sideways_friction.extremum_value * profile.sideways_stiffness * load;
    let lateral = -friction(&collider.sideways_friction, (vy / reference_speed).atan())
        * profile.sideways_stiffness
        * load;
    let mut fx_sum = 0.0;
    let mut fy_sum = 0.0;
    for _ in 0..SPIN_SUBSTEPS {
        let u = state.omega * profile.radius;
        let slip = (u - vx) / reference_speed.max(u.abs());
        let mut fx = friction(&collider.forward_friction, slip) * profile.forward_stiffness * load;
        let mut fy = lateral;
        // Combined slip: keep the force pair inside the friction ellipse.
        let usage = ratio(fx, forward_peak).powi(2) + ratio(fy, sideways_peak).powi(2);
        if usage > 1.0 {
            let scale = usage.sqrt().recip();
            fx *= scale;
            fy *= scale;
        }
        spin(
            state,
            torques.motor_nm,
            fx * profile.radius,
            collider.rotational_damping,
            brake_step,
            h,
        );
        fx_sum += fx;
        fy_sum += fy;
    }
    let slow = vx.abs() < MIN_SLIP_SPEED;
    Some(WheelForce {
        up: frame.up,
        load,
        forward,
        side,
        surface_speed: state.omega * profile.radius,
        fx: fx_sum / SPIN_SUBSTEPS as f32,
        fy: fy_sum / SPIN_SUBSTEPS as f32,
        forward_peak,
        sideways_peak,
        hold_x: slow && state.omega == 0.0 && torques.brake_nm > 0.0,
        hold_y: slow,
    })
}

fn ratio(force: f32, peak: f32) -> f32 {
    if peak > 0.0 { force / peak } else { 0.0 }
}

/// One spin sub-step. Brake torque holds zero speed and never reverses the wheel.
fn spin(state: &mut WheelState, motor: f32, reaction: f32, damping: f32, brake_step: f32, h: f32) {
    let torque = motor - reaction - damping * state.omega;
    let omega = state.omega + torque / state.profile.inertia * h;
    state.omega = if omega.abs() <= brake_step {
        0.0
    } else {
        omega - brake_step.copysign(omega)
    };
    state.angle += state.omega * h;
}

#[cfg(test)]
mod tests;
