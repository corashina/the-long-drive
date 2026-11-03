//! Source `carscript.UpdPedal`. The pedal and handbrake lever nodes follow the car inputs.
use crate::reference::{ControlPedalReference, PedalInput, Transform};
use crate::simulation::instruments::source_rotation;

/// Source `carscript.pedalLerp`.
const LERP_RATE: f32 = 20.;

/// Car inputs that move the pedals. Each is in 0..=1.
#[derive(Clone, Copy, Debug, Default)]
pub struct PedalInputs {
    pub gas: f32,
    pub brake: f32,
    pub clutch: f32,
    pub handbrake: f32,
}

impl PedalInputs {
    fn get(&self, input: PedalInput) -> f32 {
        match input {
            PedalInput::Gas => self.gas,
            PedalInput::Brake => self.brake,
            PedalInput::Clutch => self.clutch,
            PedalInput::Handbrake => self.handbrake,
        }
    }
}

/// The input that poses each pedal, in record order. A `lerp` pedal eases toward its input.
/// The source eases the pose. Easing the input gives the same pose for a push pedal and
/// nearly the same for a turned one.
#[derive(Clone, Debug, Default)]
pub struct ControlPedals {
    values: Vec<f32>,
}

impl ControlPedals {
    /// Advances by render time `dt`.
    pub fn step(&mut self, pedals: &[ControlPedalReference], inputs: PedalInputs, dt: f32) {
        self.values.resize(pedals.len(), 0.);
        let t = (dt * LERP_RATE).clamp(0., 1.);
        for (p, value) in pedals.iter().zip(&mut self.values) {
            let input = inputs.get(p.input);
            *value = if p.lerp {
                *value + (input - *value) * t
            } else {
                input
            };
        }
    }

    /// The eased input of each pedal, in record order.
    pub fn values(&self) -> &[f32] {
        &self.values
    }

    /// The local pose of each pedal node. `rest` is the node rest pose.
    pub fn poses<'a>(
        &'a self,
        pedals: &'a [ControlPedalReference],
        rest: impl Fn(u32) -> Transform + 'a,
    ) -> impl Iterator<Item = (u32, Transform)> + 'a {
        pedals
            .iter()
            .zip(&self.values)
            .map(move |(p, &value)| (p.node, pose(p, value, rest(p.node))))
    }
}

/// Source `UpdPedal`. The rotation, or the position when `push`, is `value * amount` on one axis.
pub fn pose(p: &ControlPedalReference, value: f32, rest: Transform) -> Transform {
    let mut target = [0.; 3];
    target[p.axis as usize] = value * p.amount;
    if p.push {
        // Source z converts to -z.
        Transform {
            translation: [target[0], target[1], -target[2]],
            ..rest
        }
    } else {
        Transform {
            rotation: source_rotation(target),
            ..rest
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn handbrake_lever_eases_to_its_full_angle() {
        let lever = ControlPedalReference {
            input: PedalInput::Handbrake,
            node: 3,
            axis: 0,
            amount: -35.,
            lerp: true,
            push: false,
        };
        let rest = Transform {
            translation: [1., 2., 3.],
            rotation: [0., 0., 0., 1.],
            scale: [1.; 3],
        };
        let mut pedals = ControlPedals::default();
        let on = PedalInputs {
            handbrake: 1.,
            ..Default::default()
        };
        pedals.step(&[lever], on, 0.01);
        let (_, first) = pedals.poses(&[lever], |_| rest).next().unwrap();
        for _ in 0..100 {
            pedals.step(&[lever], on, 0.01);
        }
        let (node, last) = pedals.poses(&[lever], |_| rest).next().unwrap();
        let full = source_rotation([-35., 0., 0.]);
        assert_eq!((node, last.translation), (3, rest.translation));
        assert!((0..4).all(|i| (last.rotation[i] - full[i]).abs() < 1e-4));
        assert!((first.rotation[0] - full[0]).abs() > 0.1);
    }
}
