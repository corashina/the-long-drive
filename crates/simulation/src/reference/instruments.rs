use super::*;
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MaterialLink {
    pub source: SourceId,
    pub asset: AssetId,
    pub gltf_material: u32,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum MaterialSwitchReference {
    Absent,
    Renderer {
        node: NodeId,
        on: MaterialLink,
        off: MaterialLink,
    },
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NeedleReference {
    pub source: SourceId,
    pub node: NodeId,
    pub backlight: MaterialSwitchReference,
    pub signal: String,
    pub min_value: f32,
    pub max_value: f32,
    pub min_angle: f32,
    pub max_angle: f32,
    pub axis: u8,
    pub rest: [f32; 3],
    pub offset: f32,
    pub inv: bool,
    pub value_smoothing: f32,
    pub signal_smoothing: f32,
    pub no_value_lerp: bool,
    pub lerp_angle: bool,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LampReference {
    pub source: SourceId,
    pub node: NodeId,
    pub signal: String,
    pub on: MaterialLink,
    pub off: MaterialLink,
    pub min_value: f32,
    pub max_value: f32,
    pub stay_on: bool,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DigitReference {
    pub node: NodeId,
    pub place: i32,
    pub axis: u8,
    pub inv: bool,
    pub direct: bool,
    pub continuous: bool,
    pub initial_value: f32,
    pub initial_angle: f32,
    /// Source `forgoszamlap.szam.MOn` and `MOff`. None: the source keeps the current material.
    pub on: Option<MaterialLink>,
    pub off: Option<MaterialLink>,
}

impl DigitReference {
    /// The material the digit shows. A missing material keeps the other one.
    /// None: the digit never changes its renderer material.
    pub fn material(&self, lit: bool) -> Option<&MaterialLink> {
        let (a, b) = if lit {
            (&self.on, &self.off)
        } else {
            (&self.off, &self.on)
        };
        a.as_ref().or(b.as_ref())
    }

    pub fn materials(&self) -> impl Iterator<Item = &MaterialLink> {
        self.on.iter().chain(&self.off)
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OdometerReference {
    pub source: SourceId,
    pub initial_value: f32,
    pub base: u32,
    pub decimals: f32,
    pub use_multiplier: bool,
    pub multiplier: f32,
    pub digits: Vec<DigitReference>,
}
/// Built-in gauges of one car (`carscript.meters`), with any needle and lamp count.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InstrumentReference {
    /// One `meterscript` backlight per meter.
    pub backlights: Vec<MaterialSwitchReference>,
    pub needles: Vec<NeedleReference>,
    pub lamps: Vec<LampReference>,
    /// None: no meter has a digit counter.
    pub odometer: Option<OdometerReference>,
}
impl InstrumentReference {
    /// Odometer digits. Empty without an odometer.
    pub fn digits(&self) -> &[DigitReference] {
        self.odometer.as_ref().map_or(&[], |o| &o.digits)
    }
    /// The meter backlights and the needle backlights.
    pub fn all_backlights(&self) -> impl Iterator<Item = &MaterialSwitchReference> {
        self.backlights
            .iter()
            .chain(self.needles.iter().map(|n| &n.backlight))
    }
}
/// `carscript` lamp thresholds and the electronics output that every gauge of the car reads.
#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GaugeLimits {
    pub electronics_output: f32,
    pub electronics_off_threshold: f32,
    pub low_fuel: f32,
    pub low_oil: f32,
    pub oil_test_rpm: f32,
    pub battery_test_rpm: f32,
}
/// A `meterscript` on a gauge part. `carscript.kmhmeters` updates it while the part is in a
/// listed slot. Node ids are indices in the prototype that holds the part.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GaugePartReference {
    pub source: SourceId,
    /// The meter node. The part item is the nearest item at or above it.
    pub node: u32,
    pub backlight: MaterialSwitchReference,
    pub needles: Vec<NeedleReference>,
    pub lamps: Vec<LampReference>,
    /// The `kilometerszamlalo` and `napiszamlalo` counters. Each adds the driven distance.
    pub counters: Vec<OdometerReference>,
}
impl GaugePartReference {
    /// Every material that the gauge can show on renderer node `node`.
    pub fn renderer_materials(&self, node: u32) -> impl Iterator<Item = &MaterialLink> {
        let lamps = self
            .lamps
            .iter()
            .filter(move |l| l.node.0 == node)
            .flat_map(|l| [&l.on, &l.off]);
        let digits = self
            .counters
            .iter()
            .flat_map(|c| &c.digits)
            .filter(move |d| d.node.0 == node)
            .flat_map(DigitReference::materials);
        let backlights = std::iter::once(&self.backlight)
            .chain(self.needles.iter().map(|n| &n.backlight))
            .filter_map(move |b| match b {
                MaterialSwitchReference::Renderer { node: n, on, off } if n.0 == node => {
                    Some([on, off])
                }
                _ => None,
            })
            .flatten();
        lamps.chain(digits).chain(backlights)
    }
}
