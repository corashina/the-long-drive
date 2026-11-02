//! Source spricniscript trigger and ammoscript count.
use crate::reference::{AcquisitionAmmo, SprayBottleReference};

/// Mathf.Lerp clamps its factor.
fn lerp(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t.clamp(0., 1.)
}

/// spricniscript currentAngle, lastAngle, triggered, and pressing.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct SprayTrigger {
    pub angle: f32,
    last: f32,
    triggered: bool,
    pressing: bool,
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct TriggerEvents {
    /// Fire runs. It still checks the ammo.
    pub fire: bool,
    /// The trigger re-arms and plays the reset clip.
    pub reset: bool,
}
impl SprayTrigger {
    /// Pressing: the trigger eases toward its maximum angle.
    pub fn press(&mut self, r: &SprayBottleReference, dt: f32) {
        self.pressing = true;
        self.angle = lerp(self.angle, r.trigger_max_angle, r.angle_speed * dt);
    }
    /// Update: release easing, the fire edge, and the re-arm.
    pub fn update(&mut self, r: &SprayBottleReference, dt: f32) -> TriggerEvents {
        if !self.pressing {
            self.angle = lerp(self.angle, 0., r.angle_speed * dt);
        }
        let mut events = TriggerEvents::default();
        if self.angle >= r.trigger_set_angle && self.last < self.angle && !self.triggered {
            self.triggered = true;
            events.fire = true;
        }
        if self.triggered && self.angle <= r.trigger_reset_angle {
            self.triggered = false;
            events.reset = true;
        }
        self.last = self.angle;
        self.pressing = false;
        events
    }
    /// The trigger node local Euler X angle in degrees.
    pub fn euler_x(&self, r: &SprayBottleReference) -> f32 {
        if r.invert { -self.angle } else { self.angle }
    }
}

/// ammoscript: an int or float count of one ammo type.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Ammo {
    pub count: f32,
    pub maximum: f32,
    pub float: bool,
    pub kind: u32,
}
impl Ammo {
    pub fn new(r: &AcquisitionAmmo) -> Self {
        Self {
            count: if r.float_or_int {
                r.float_count
            } else {
                r.count as f32
            },
            maximum: r.maximum,
            float: r.float_or_int,
            kind: r.r#type,
        }
    }
    /// The count after UpdAmmoFromMulti clamping, or None when it is not valid.
    pub fn with_count(self, count: f32) -> Option<Self> {
        (count.is_finite()
            && (0. ..=self.maximum).contains(&count)
            && (self.float || count.fract() == 0.))
            .then_some(Self { count, ..self })
    }
    /// Decrease: the int count drops by the truncated value.
    pub fn decrease(&mut self, value: f32) -> f32 {
        let value = value.clamp(0., self.count.max(0.));
        self.count -= if self.float { value } else { value.trunc() };
        value
    }
    /// CanReload without the weapon and magazine paths.
    pub fn can_reload(&self, from: &Ammo) -> bool {
        self.kind == from.kind && self.count < self.maximum && from.count > 0.
    }
    /// Reload: take up to `value` from a source of the same type.
    pub fn reload(&mut self, from: &mut Ammo, value: f32) {
        if from.kind != self.kind || from.count <= 0. || self.count >= self.maximum {
            return;
        }
        let value = value
            .clamp(0., self.maximum - self.count)
            .clamp(0., from.count);
        self.count += if self.float { value } else { value.trunc() };
        from.decrease(value);
    }
}
