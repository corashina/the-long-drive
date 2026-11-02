//! Gauge parts: a `meterscript` on a part that `carscript.kmhmeters` updates through a slot.
//! A gauge outside a listed slot keeps its last values.
use super::dials::{digit_angle, lerp, lerp_angle};
use super::instruments::{InstrumentSample, lamp_bits, meter_lit, needle_angle};
use crate::reference::{GaugeLimits, GaugePartReference, KmhMeterReference};
use serde::{Deserialize, Serialize};

/// Source `save_meterscript`: needle input values and counter values.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GaugeSnapshot {
    pub values: Vec<f32>,
    pub counters: Vec<f32>,
}

/// Runtime state of one gauge part. Buffers keep their size after `new`.
#[derive(Clone, Debug)]
pub struct GaugeState {
    /// Source `mutatok[i].value`: the input of each needle.
    pub values: Vec<f32>,
    /// The value that each needle shows. `mutatoscript` moves it to the input.
    shown: Vec<f32>,
    pub needle_angles: Vec<f32>,
    /// Source `szamlap.value` of each counter, in km.
    pub counters: Vec<f32>,
    digit_values: Vec<f32>,
    pub digit_angles: Vec<f32>,
    pub lamp_bits: u32,
    /// Backlights and digits show their on material.
    pub lit: bool,
    /// The presented state changed since the last `advance`.
    changed: bool,
}

impl GaugeState {
    pub fn new(r: &GaugePartReference) -> Self {
        let digits = r.counters.iter().flat_map(|c| &c.digits);
        Self {
            values: vec![0.; r.needles.len()],
            shown: vec![0.; r.needles.len()],
            needle_angles: r.needles.iter().map(|n| needle_angle(n, 0.)).collect(),
            counters: r.counters.iter().map(|c| c.initial_value).collect(),
            digit_values: digits.clone().map(|d| d.initial_value).collect(),
            digit_angles: digits.map(|d| d.initial_angle).collect(),
            lamp_bits: 0,
            lit: false,
            changed: true,
        }
    }

    /// Source `METERUPD(meter, kind, force, force, modifier)`. With `force`, every needle
    /// reads the speed.
    pub fn meter_update(
        &mut self,
        r: &GaugePartReference,
        kind: &str,
        force: bool,
        modifier: f32,
        s: &InstrumentSample,
        dt: f32,
    ) {
        for (value, n) in self.values.iter_mut().zip(&r.needles) {
            let signal = n.signal.as_str();
            if !(signal == kind || force) {
                continue;
            }
            *value = if signal == "speed" || force {
                s.speed_kmh.abs() * modifier
            } else {
                match signal {
                    "fuel" => {
                        let fuel = if s.local_ignition { s.fuel_liters } else { 0. };
                        lerp(*value, modifier * fuel, dt * 5.)
                    }
                    "temp" => {
                        let temp = if s.local_ignition { s.temperature } else { 0. };
                        lerp(*value, modifier * temp, dt * 5.)
                    }
                    "rpm" if s.local_ignition => s.rpm * modifier,
                    "rpm" => 0.,
                    _ => *value,
                }
            };
        }
    }

    /// Source `METERUPD2`: lamps, the distance counters and the meter lights.
    pub fn meter_update2(
        &mut self,
        r: &GaugePartReference,
        limits: &GaugeLimits,
        s: &InstrumentSample,
        distance_km: f32,
    ) {
        let bits = lamp_bits(&r.lamps, limits, s);
        let lit = meter_lit(s);
        self.changed |= bits != self.lamp_bits || lit != self.lit;
        self.lamp_bits = bits;
        self.lit = lit;
        for counter in &mut self.counters {
            *counter += distance_km;
        }
    }

    /// Both updates of one `kmhslottype` row for a mounted gauge.
    pub fn kmh_update(
        &mut self,
        r: &GaugePartReference,
        k: &KmhMeterReference,
        limits: &GaugeLimits,
        s: &InstrumentSample,
        distance_km: f32,
        dt: f32,
    ) {
        for kind in &k.types {
            self.meter_update(r, kind, k.force, k.modifier, s, dt);
        }
        self.meter_update2(r, limits, s, distance_km);
    }

    /// `mutatoscript` and `forgoszamlap` frame updates. They run on a removed gauge too.
    /// Returns true when the presented state changed.
    pub fn advance(&mut self, r: &GaugePartReference, dt: f32) -> bool {
        for (i, n) in r.needles.iter().enumerate() {
            self.shown[i] = if n.no_value_lerp {
                self.values[i] + n.offset
            } else if n.lerp_angle {
                lerp_angle(self.shown[i], self.values[i], dt * n.value_smoothing)
            } else {
                lerp(self.shown[i], self.values[i], dt * n.value_smoothing)
            };
            let angle = needle_angle(n, self.shown[i]);
            self.changed |= angle != self.needle_angles[i];
            self.needle_angles[i] = angle;
        }
        let mut start = 0;
        for (c, &km) in r.counters.iter().zip(&self.counters) {
            let digits = start..start + c.digits.len();
            let value = if c.use_multiplier {
                km * c.multiplier
            } else {
                km
            };
            let base = c.base as f32;
            for (d, v) in c.digits.iter().zip(&mut self.digit_values[digits.clone()]) {
                if !d.direct {
                    let place = base.powf(d.place as f32 - c.decimals + 1.);
                    *v = if place == 0. {
                        0.
                    } else {
                        value % place / place * base
                    };
                }
            }
            for (i, d) in c.digits.iter().enumerate() {
                let target = digit_angle(&self.digit_values[digits.clone()], i, d.continuous, base);
                let angle = &mut self.digit_angles[start + i];
                let next = lerp_angle(*angle, target, dt * 20.);
                self.changed |= next != *angle;
                *angle = next;
            }
            start = digits.end;
        }
        std::mem::take(&mut self.changed)
    }

    pub fn capture_save(&self) -> GaugeSnapshot {
        GaugeSnapshot {
            values: self.values.clone(),
            counters: self.counters.clone(),
        }
    }

    /// Takes saved values. The needles jump to them.
    pub fn restore_save(&mut self, r: &GaugePartReference, saved: &GaugeSnapshot) -> bool {
        if saved.values.len() != self.values.len()
            || saved.counters.len() != self.counters.len()
            || !saved
                .values
                .iter()
                .chain(&saved.counters)
                .all(|v| v.is_finite())
        {
            return false;
        }
        self.values.copy_from_slice(&saved.values);
        self.shown.copy_from_slice(&saved.values);
        self.counters.copy_from_slice(&saved.counters);
        for (angle, (n, &v)) in self
            .needle_angles
            .iter_mut()
            .zip(r.needles.iter().zip(&self.values))
        {
            *angle = needle_angle(n, v);
        }
        self.changed = true;
        true
    }

    /// The material that renderer node `node` of the gauge shows. None: not a gauge renderer.
    pub fn renderer_material<'a>(
        &self,
        r: &'a GaugePartReference,
        node: u32,
    ) -> Option<&'a crate::reference::MaterialLink> {
        let pick = |on: bool, a, b| if on { a } else { b };
        if let Some((i, lamp)) = r.lamps.iter().enumerate().find(|(_, l)| l.node.0 == node) {
            return Some(pick(self.lamp_bits & (1 << i) != 0, &lamp.on, &lamp.off));
        }
        if let Some(d) = r
            .counters
            .iter()
            .flat_map(|c| &c.digits)
            .find(|d| d.node.0 == node)
        {
            return d.material(self.lit);
        }
        std::iter::once(&r.backlight)
            .chain(r.needles.iter().map(|n| &n.backlight))
            .find_map(|b| match b {
                crate::reference::MaterialSwitchReference::Renderer { node: n, on, off }
                    if n.0 == node =>
                {
                    Some(pick(self.lit, on, off))
                }
                _ => None,
            })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn needle(signal: &str) -> serde_json::Value {
        json!({"source": {"file": "a", "path_id": 2}, "node": 1, "backlight": {"kind": "absent"},
            "signal": signal, "min_value": 0.0, "max_value": 200.0, "min_angle": 0.0, "max_angle": 270.0,
            "axis": 2, "rest": [0.0, 0.0, 0.0], "offset": 0.0, "inv": false, "value_smoothing": 8.0,
            "signal_smoothing": 0.0, "no_value_lerp": false, "lerp_angle": false})
    }

    /// Bus01 gauge in a kmh slot: with `force`, the rpm needle reads the speed too. Out of the
    /// slot, the gauge keeps its values.
    #[test]
    fn forced_slot_gauge_reads_speed_and_keeps_values_when_removed() {
        let r: GaugePartReference = serde_json::from_value(json!({
            "source": {"file": "a", "path_id": 1}, "node": 0, "backlight": {"kind": "absent"},
            "needles": [needle("speed"), needle("rpm")], "lamps": [],
            "counters": [{"source": {"file": "a", "path_id": 3}, "initial_value": 10.0, "base": 10,
                "decimals": 1.0, "use_multiplier": false, "multiplier": 1.0, "digits": []}]}))
        .unwrap();
        let k: KmhMeterReference = serde_json::from_value(json!(
            {"force": true, "modifier": 1.0, "types": ["speed"], "slots": []}))
        .unwrap();
        let limits: GaugeLimits =
            serde_json::from_value(crate::reference::gauge_limits_fixture()).unwrap();
        let sample = InstrumentSample {
            speed_kmh: 50.,
            rpm: 3000.,
            local_ignition: true,
            headlights: 1,
            ..Default::default()
        };
        let mut state = GaugeState::new(&r);
        for _ in 0..200 {
            state.kmh_update(&r, &k, &limits, &sample, 0.01, 0.02);
            state.advance(&r, 0.02);
        }
        assert_eq!(state.values, [50., 50.]);
        assert!(
            (state.counters[0] - 12.).abs() < 1e-3,
            "{:?}",
            state.counters
        );
        assert!(state.lit);
        let angles = state.needle_angles.clone();
        // Removed: no kmh update. The needles hold their last values.
        for _ in 0..100 {
            state.advance(&r, 0.02);
        }
        assert_eq!(state.values, [50., 50.]);
        assert_eq!(state.needle_angles, angles);
        // Without force, the rpm needle reads the rpm.
        let k = KmhMeterReference {
            force: false,
            types: vec!["rpm".into()],
            ..k
        };
        state.kmh_update(&r, &k, &limits, &sample, 0., 0.02);
        assert_eq!(state.values, [50., 3000.]);
    }
}
