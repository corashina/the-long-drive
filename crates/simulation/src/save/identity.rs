use super::SaveError;
use crate::reference::*;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::hash::Hash;

#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SourceKey {
    pub file: String,
    pub path_id: String,
}
impl From<&SourceId> for SourceKey {
    fn from(source: &SourceId) -> Self {
        Self {
            file: source.file.clone(),
            path_id: source.path_id.to_string(),
        }
    }
}
impl SourceKey {
    pub fn source_id(&self) -> Result<SourceId, SaveError> {
        if self.file.is_empty() {
            return Err(SaveError::InvalidLinks("Empty source file".into()));
        }
        Ok(SourceId {
            file: self.file.clone(),
            path_id: signed(&self.path_id)?,
        })
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum InstanceId {
    Initial {
        source: SourceKey,
    },
    Generated {
        #[serde(with = "lossless")]
        id: crate::world::GeneratedId,
    },
}
impl InstanceId {
    pub fn initial(source: &SourceId) -> Self {
        Self::Initial {
            source: source.into(),
        }
    }
}

/// Reference types have separate maps because source objects can have several roles.
pub struct IdentityMap {
    pub items: HashMap<SourceKey, ItemId>,
    pub generated: HashMap<crate::world::GeneratedId, ItemId>,
    pub bodies: HashMap<SourceKey, BodyId>,
    pub nodes: HashMap<SourceKey, NodeId>,
    pub anchors: HashMap<SourceKey, AnchorId>,
    pub part_slots: HashMap<SourceKey, PartSlotId>,
    pub mechanisms: HashMap<SourceKey, MechanismId>,
    pub openings: HashMap<SourceKey, OpeningId>,
    pub lights: HashMap<SourceKey, LightId>,
    pub controllers: HashMap<SourceKey, LightControlId>,
}
impl IdentityMap {
    pub fn new(reference: &ReferenceManifest) -> Result<Self, SaveError> {
        Ok(Self {
            generated: HashMap::new(),
            items: index(reference.items.iter().map(|r| (&r.source, r.id)))?,
            bodies: index(reference.bodies.iter().map(|r| (&r.source, r.id)))?,
            nodes: index(reference.nodes.iter().map(|r| (&r.source, r.id)))?,
            anchors: index(reference.anchors.iter().map(|r| (&r.source, r.id)))?,
            part_slots: index(reference.part_slots.iter().map(|r| (&r.source, r.id)))?,
            mechanisms: index(reference.mechanisms.iter().map(|r| (&r.source, r.id)))?,
            openings: index(reference.openings.iter().map(|r| (&r.source, r.id)))?,
            lights: index(reference.lighting.lights.iter().map(|r| (&r.source, r.id)))?,
            controllers: index(
                reference
                    .lighting
                    .controllers
                    .iter()
                    .map(|r| (&r.source, r.id)),
            )?,
        })
    }
    pub fn bind_generated(&mut self, items: &crate::interaction::items::ItemStore) {
        self.generated.clear();
        self.generated.extend(
            items
                .generated_bindings()
                .map(|(key, item)| (key.clone(), item.id)),
        );
    }
    pub fn item_instance(&self, id: ItemId) -> Result<InstanceId, SaveError> {
        if let Some((key, _)) = self.generated.iter().find(|(_, value)| **value == id) {
            return Ok(InstanceId::Generated { id: key.clone() });
        }
        Ok(InstanceId::Initial {
            source: source_key(&self.items, id)?,
        })
    }
    pub fn item(&self, instance: &InstanceId) -> Result<ItemId, SaveError> {
        match instance {
            InstanceId::Initial { source } => self
                .items
                .get(source)
                .copied()
                .ok_or_else(|| SaveError::InvalidLinks(format!("Unknown item {source:?}"))),
            InstanceId::Generated { id } => {
                self.generated.get(id).copied().ok_or_else(|| {
                    SaveError::InvalidLinks("Generated item binding is pending".into())
                })
            }
        }
    }
}
pub fn source_key<T: Copy + Eq>(
    map: &HashMap<SourceKey, T>,
    id: T,
) -> Result<SourceKey, SaveError> {
    map.iter()
        .find(|(_, value)| **value == id)
        .map(|(key, _)| key.clone())
        .ok_or_else(|| SaveError::InvalidLinks("Missing runtime source identity".into()))
}
fn index<'a, T: Copy + Eq + Hash>(
    records: impl Iterator<Item = (&'a SourceId, T)>,
) -> Result<HashMap<SourceKey, T>, SaveError> {
    let mut result = HashMap::new();
    let mut ids = std::collections::HashSet::new();
    for (source, id) in records {
        let key = SourceKey::from(source);
        key.source_id()?;
        if result.insert(key, id).is_some() || !ids.insert(id) {
            return Err(SaveError::InvalidLinks(
                "Duplicate source identity or runtime ID".into(),
            ));
        }
    }
    Ok(result)
}
pub(super) fn signed(value: &str) -> Result<i64, SaveError> {
    value
        .parse::<i64>()
        .ok()
        .filter(|number| number.to_string() == value)
        .ok_or_else(|| SaveError::InvalidLinks("Invalid signed decimal identity".into()))
}
pub fn unsigned(value: &str) -> Result<u64, SaveError> {
    value
        .parse::<u64>()
        .ok()
        .filter(|number| number.to_string() == value)
        .ok_or_else(|| SaveError::InvalidLinks("Invalid unsigned decimal identity".into()))
}

/// Browser storage parses and stringifies JSON. Source i64 identities use decimal strings.
pub mod lossless {
    use serde::{Deserialize, Deserializer, Serialize, Serializer, de::DeserializeOwned};
    use serde_json::Value;
    fn convert(value: &mut Value, encode: bool) -> Result<(), String> {
        match value {
            Value::Array(values) => {
                for value in values {
                    convert(value, encode)?;
                }
            }
            Value::Object(fields) => {
                for (name, value) in fields {
                    if name == "path_id" {
                        scalar(value, encode)?;
                    } else if name == "unit" {
                        let values = value.as_array_mut().ok_or("Invalid generated unit")?;
                        for value in values {
                            scalar(value, encode)?;
                        }
                    } else {
                        convert(value, encode)?;
                    }
                }
            }
            _ => (),
        }
        Ok(())
    }
    fn scalar(value: &mut Value, encode: bool) -> Result<(), String> {
        if encode {
            *value = Value::String(value.as_i64().ok_or("Invalid source integer")?.to_string());
        } else {
            let text = value
                .as_str()
                .ok_or("Source identity must be a decimal string")?;
            let number = text.parse::<i64>().map_err(|_| "Invalid source integer")?;
            if number.to_string() != text {
                return Err("Noncanonical source integer".into());
            }
            *value = Value::Number(number.into());
        }
        Ok(())
    }
    pub fn serialize<T: Serialize, S: Serializer>(
        value: &T,
        serializer: S,
    ) -> Result<S::Ok, S::Error> {
        let mut value = serde_json::to_value(value).map_err(serde::ser::Error::custom)?;
        convert(&mut value, true).map_err(serde::ser::Error::custom)?;
        value.serialize(serializer)
    }
    pub fn deserialize<'de, T: DeserializeOwned, D: Deserializer<'de>>(
        deserializer: D,
    ) -> Result<T, D::Error> {
        let mut value = Value::deserialize(deserializer)?;
        convert(&mut value, false).map_err(serde::de::Error::custom)?;
        serde_json::from_value(value).map_err(serde::de::Error::custom)
    }
}
