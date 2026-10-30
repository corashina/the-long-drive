//! Start car choice and pick. Source `mainmenuscript.RandomStartCar` and `startscript`.
use super::UnityRandom;
use crate::reference::{ReferenceError, ReferenceManifest, WorldReference};
use serde::{Deserialize, Serialize};

/// Source `RandomStartCar` chances, in draw order, by source `TerrainGenerationSettings` field.
/// A field without an exported start placement is skipped with its chance.
/// Today `StartCar06p` has none: V03 adds it.
const CHANCES: [(&str, i32); 15] = [
    ("StartCar01", 100),
    ("StartCar02", 75),
    ("StartCar02k", 65),
    ("StartCar03", 65),
    ("StartCar03k", 55),
    ("StartCar04", 55),
    ("StartCar05", 55),
    ("StartCar06", 55),
    ("StartCar06p", 55),
    ("StartCar08", 55),
    ("StartCar08c", 55),
    ("StartCar09", 30),
    ("StartCar09P", 30),
    ("StartCar10", 55),
    ("StartCar10k", 55),
];
/// Source `startscript`: the car09 type starts at one of two fields, 50/50.
const CAR09: [&str; 2] = ["StartCar09", "StartCar09S"];

/// A source start car field (`TerrainGenerationSettings.StartCar*`).
/// The save keeps the field name.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StartCar(&'static str);

impl StartCar {
    /// The Lada.
    pub const LADA: Self = Self("StartCar02");
    pub fn field(self) -> &'static str {
        self.0
    }
    pub fn from_field(field: &str) -> Option<Self> {
        CHANCES
            .iter()
            .map(|(f, _)| *f)
            .chain([CAR09[1]])
            .find(|f| *f == field)
            .map(Self)
    }
    /// Source `itemdatabase.carTypeToCarID[carType]` of the start placement car.
    /// The spawn context keeps it as the current car.
    pub fn current_car(self, reference: &ReferenceManifest) -> Result<i32, ReferenceError> {
        let world = &reference.world;
        let prototype = world
            .startup
            .iter()
            .find(|s| s.start_car && s.group == self.0)
            .and_then(|s| s.prototype.as_ref());
        world
            .prototypes
            .iter()
            .find(|p| Some(&p.source) == prototype)
            .and_then(|p| p.vehicle.as_ref())
            .and_then(|v| usize::try_from(v.car_type).ok())
            .and_then(|t| reference.acquisition.rules.car_type_to_item_index.get(t))
            .map(|id| *id as i32)
            .ok_or_else(|| ReferenceError(format!("{}: no start car placement", self.0)))
    }
}
impl Serialize for StartCar {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(self.0)
    }
}
impl<'de> Deserialize<'de> for StartCar {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let field = String::deserialize(deserializer)?;
        Self::from_field(&field)
            .ok_or_else(|| serde::de::Error::custom(format!("Unknown start car {field}")))
    }
}

/// The game mode start car: Random or one field.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum StartCarChoice {
    Random,
    Field(StartCar),
}
impl Default for StartCarChoice {
    fn default() -> Self {
        Self::Field(StartCar::LADA)
    }
}
impl StartCarChoice {
    /// Browser value: -1 Random, else an index into the start placements of `world`.
    /// Other values read as Random.
    pub fn from_index(world: &WorldReference, index: i32) -> Self {
        usize::try_from(index)
            .ok()
            .and_then(|i| start_fields(world).nth(i))
            .and_then(StartCar::from_field)
            .map_or(Self::Random, Self::Field)
    }
    /// Random draws from `random`, as `mainmenuscript.RandomStartCar` and then `startscript`.
    pub fn pick(self, world: &WorldReference, random: &mut UnityRandom) -> StartCar {
        let Self::Field(car) = self else {
            return pick_random(&start_fields(world).collect::<Vec<_>>(), random);
        };
        car
    }
}

/// Start car fields of the start placements, in export order.
pub fn start_fields(world: &WorldReference) -> impl Iterator<Item = &str> {
    world
        .startup
        .iter()
        .filter(|s| s.start_car)
        .map(|s| s.group.as_str())
}

fn pick_random(fields: &[&str], random: &mut UnityRandom) -> StartCar {
    let exported = |field: &str| fields.contains(&field);
    let available = |field: &str| {
        if field == CAR09[0] {
            CAR09.iter().all(|f| exported(f))
        } else {
            exported(field)
        }
    };
    let sum: i32 = CHANCES
        .iter()
        .filter(|(f, _)| available(f))
        .map(|(_, c)| c)
        .sum();
    // Source: `Random.Range(0f, sum)`, then the first cumulative chance above the draw.
    let draw = random.range(0., sum as f32);
    let mut cumulative = 0;
    let mut picked = StartCar::LADA.0;
    for (field, chance) in CHANCES.iter().filter(|(f, _)| available(f)) {
        cumulative += chance;
        if draw < cumulative as f32 {
            picked = field;
            break;
        }
    }
    if picked == CAR09[0] && random.range(0., 1.) >= 0.5 {
        picked = CAR09[1];
    }
    StartCar(picked)
}

/// The menu random. It is separate from the world seed, so the pick does not shift world draws.
/// Unity seeds its menu random from the clock; the port takes browser or clock entropy.
pub fn menu_random() -> UnityRandom {
    #[cfg(target_arch = "wasm32")]
    let seed = (js_sys::Math::random() * f64::from(u32::MAX)) as u32;
    #[cfg(not(target_arch = "wasm32"))]
    let seed = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.subsec_nanos());
    UnityRandom::new(seed as i32)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn picks(fields: &[&str], count: usize) -> std::collections::HashMap<&'static str, usize> {
        let mut random = UnityRandom::new(7);
        let mut counts = std::collections::HashMap::new();
        for _ in 0..count {
            let car = pick_random(fields, &mut random);
            *counts.entry(car.field()).or_insert(0) += 1;
        }
        counts
    }

    #[test]
    fn random_pick_follows_the_source_chances() {
        let fields = [
            "StartCar01",
            "StartCar02",
            "StartCar02k",
            "StartCar04",
            "StartCar05",
            "StartCar06",
            "StartCar09",
            "StartCar09S",
        ];
        let count = 100_000;
        let counts = picks(&fields, count);
        // Not exported fields are skipped with their chance.
        let sum = f64::from(100 + 75 + 65 + 55 * 3 + 30);
        for (field, chance) in [
            ("StartCar01", 100),
            ("StartCar02", 75),
            ("StartCar02k", 65),
            ("StartCar04", 55),
            ("StartCar05", 55),
            ("StartCar06", 55),
        ] {
            let share = counts[field] as f64 / count as f64;
            assert!(
                (share - f64::from(chance) / sum).abs() < 0.01,
                "{field} {share}"
            );
        }
        let car09 = (counts["StartCar09"] + counts["StartCar09S"]) as f64;
        assert!((car09 / count as f64 - 30. / sum).abs() < 0.01);
        assert!((counts["StartCar09"] as f64 / car09 - 0.5).abs() < 0.05);
    }

    #[test]
    fn start_car_saves_by_field() {
        let json = serde_json::to_string(&StartCar::LADA).unwrap();
        assert_eq!(json, "\"StartCar02\"");
        assert_eq!(
            serde_json::from_str::<StartCar>("\"StartCar09S\"").unwrap(),
            StartCar("StartCar09S")
        );
        assert!(serde_json::from_str::<StartCar>("\"StartCar99\"").is_err());
    }
}
