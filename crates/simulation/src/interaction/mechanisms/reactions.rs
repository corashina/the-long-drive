//! Source RotSound transitions retain each axis and endpoint identity.
use super::*;

#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct EndpointEvent {
    pub axis: usize,
    pub event: usize,
    pub intensity: f32,
}

pub(super) fn local_force(reference: &EndpointReactionReference) -> Vector {
    let EndpointReactionReference::Rotation {
        axes,
        inverted,
        force,
        ..
    } = reference
    else {
        return Vector::ZERO;
    };
    // Unity-to-runtime reflection changes forward Z once. Preserve the axis sum.
    Vector::new(
        axes[0] as u8 as f32,
        axes[1] as u8 as f32,
        -(axes[2] as u8 as f32),
    ) * (*force * if *inverted { -1. } else { 1. })
}

pub(super) fn transitions(
    previous: [f32; 2],
    current: [f32; 2],
    limits: [[f32; 2]; 2],
    input: [f32; 2],
    dt: f32,
    configured: [bool; 4],
) -> [Option<EndpointEvent>; 4] {
    let mut pending = [None; 4];
    if !dt.is_finite() || dt <= 0. || !input.iter().all(|v| v.is_finite()) {
        return pending;
    }
    let mut count = 0;
    // Source visits entering events before leaving events, for both axes.
    for leaving in [false, true] {
        for axis in 0..2 {
            for endpoint in 0..2 {
                let event = endpoint + if leaving { 2 } else { 0 };
                let was = previous[axis] == limits[axis][endpoint];
                let now = current[axis] == limits[axis][endpoint];
                if configured[event] && was != now && was == leaving {
                    pending[count] = Some(EndpointEvent {
                        axis,
                        event,
                        intensity: (input[axis].abs() / dt.clamp(0.001, 1000.) * 0.01)
                            .clamp(0., 1.),
                    });
                    count += 1;
                }
            }
        }
    }
    pending
}

impl MechanismState {
    pub(super) fn clear_reactions(&mut self) {
        for control in &mut self.controls {
            control.pending_reactions = [None; 4];
        }
    }

    pub fn apply_reactions(&mut self, physics: &mut PhysicsWorld) {
        for control in &mut self.controls {
            let pending = std::mem::replace(&mut control.pending_reactions, [None; 4]);
            if pending.iter().all(Option::is_none) {
                continue;
            }
            let frame = &self.frames[control.reaction_frame.unwrap()];
            if frame.inactive {
                continue;
            }
            let Some(root) = frame.reaction_root else {
                continue;
            };
            // Frame rotation changes directions. Node scale never changes force.
            let direction = frame.world.rotation * control.reaction_force;
            let point = frame.world.translation;
            for event in pending.into_iter().flatten() {
                let sign = if event.event < 2 { 1. } else { -1. };
                let impulse = direction * (event.intensity * sign * crate::simulation::FIXED_DT);
                physics.try_edit_body(root, |body| {
                    body.apply_impulse_at_point(impulse, point, true)
                });
            }
        }
    }
}
