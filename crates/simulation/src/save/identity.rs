use super::SaveError;
use crate::reference::*;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::hash::Hash;
use std::sync::Arc;

#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SourceKey {
    pub file: String,
    /// A number inside `lossless` records, which decode every `path_id` to a number.
    #[serde(deserialize_with = "decimal_text")]
    pub path_id: String,
}
fn decimal_text<'de, D: serde::Deserializer<'de>>(deserializer: D) -> Result<String, D::Error> {
    match serde_json::Value::deserialize(deserializer)? {
        serde_json::Value::String(text) => Ok(text),
        serde_json::Value::Number(number) if number.is_i64() => Ok(number.to_string()),
        _ => Err(serde::de::Error::custom("Invalid source key path")),
    }
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

/// Source keys of one reference type, with O(1) lookups both ways. One copy of each key.
pub struct Keys<T> {
    ids: HashMap<Arc<SourceKey>, T>,
    keys: HashMap<T, Arc<SourceKey>>,
}
impl<T: Copy + Eq + Hash> Keys<T> {
    fn new<'a>(records: impl Iterator<Item = (&'a SourceId, T)>) -> Result<Self, SaveError> {
        let mut result = Self {
            ids: HashMap::new(),
            keys: HashMap::new(),
        };
        for (source, id) in records {
            let key = Arc::new(SourceKey::from(source));
            key.source_id()?;
            if result.ids.insert(key.clone(), id).is_some() || result.keys.insert(id, key).is_some()
            {
                return Err(SaveError::InvalidLinks(
                    "Duplicate source identity or runtime ID".into(),
                ));
            }
        }
        Ok(result)
    }
    pub fn get(&self, key: &SourceKey) -> Option<&T> {
        self.ids.get(key)
    }
    pub fn contains(&self, id: T) -> bool {
        self.keys.contains_key(&id)
    }
    /// The source key of a runtime ID.
    pub fn source(&self, id: T) -> Result<SourceKey, SaveError> {
        self.keys
            .get(&id)
            .map(|key| SourceKey::clone(key))
            .ok_or_else(|| SaveError::InvalidLinks("Missing runtime source identity".into()))
    }
    pub fn iter(&self) -> impl Iterator<Item = (&SourceKey, &T)> + '_ {
        self.ids.iter().map(|(key, id)| (&**key, id))
    }
    pub fn ids(&self) -> impl Iterator<Item = T> + '_ {
        self.keys.keys().copied()
    }
    pub(crate) fn len(&self) -> usize {
        self.keys.len()
    }
}
impl<T> std::ops::Index<&SourceKey> for Keys<T> {
    type Output = T;
    fn index(&self, key: &SourceKey) -> &T {
        &self.ids[key]
    }
}

/// Source identities of the reference. The runtime builds it once per session.
/// Reference types have separate maps because source objects can have several roles.
pub struct IdentityMap {
    pub items: Keys<ItemId>,
    pub bodies: Keys<BodyId>,
    pub nodes: Keys<NodeId>,
    pub anchors: Keys<AnchorId>,
    pub part_slots: Keys<PartSlotId>,
    pub mechanisms: Keys<MechanismId>,
    pub openings: Keys<OpeningId>,
    pub lights: Keys<LightId>,
    pub controllers: Keys<LightControlId>,
}
impl IdentityMap {
    pub fn new(reference: &ReferenceManifest) -> Result<Self, SaveError> {
        Ok(Self {
            items: Keys::new(reference.items.iter().map(|r| (&r.source, r.id)))?,
            bodies: Keys::new(reference.bodies.iter().map(|r| (&r.source, r.id)))?,
            nodes: Keys::new(reference.nodes.iter().map(|r| (&r.source, r.id)))?,
            anchors: Keys::new(reference.anchors.iter().map(|r| (&r.source, r.id)))?,
            part_slots: Keys::new(reference.part_slots.iter().map(|r| (&r.source, r.id)))?,
            mechanisms: Keys::new(reference.mechanisms.iter().map(|r| (&r.source, r.id)))?,
            openings: Keys::new(reference.openings.iter().map(|r| (&r.source, r.id)))?,
            lights: Keys::new(reference.lighting.lights.iter().map(|r| (&r.source, r.id)))?,
            controllers: Keys::new(
                reference
                    .lighting
                    .controllers
                    .iter()
                    .map(|r| (&r.source, r.id)),
            )?,
        })
    }
    /// The save identity of an item. `items` holds the generated item bindings.
    pub fn item_instance(
        &self,
        items: &crate::interaction::items::ItemStore,
        id: ItemId,
    ) -> Result<InstanceId, SaveError> {
        if let Some(key) = items.generated_key(id) {
            return Ok(InstanceId::Generated { id: key.clone() });
        }
        Ok(InstanceId::Initial {
            source: self.items.source(id)?,
        })
    }
    /// An imported item. A generated item is an error: use `bound_item`.
    pub fn item(&self, instance: &InstanceId) -> Result<ItemId, SaveError> {
        match instance {
            InstanceId::Initial { source } => self
                .items
                .get(source)
                .copied()
                .ok_or_else(|| SaveError::InvalidLinks(format!("Unknown item {source:?}"))),
            InstanceId::Generated { .. } => Err(SaveError::InvalidLinks(
                "Generated item binding is pending".into(),
            )),
        }
    }
    /// An imported item, or a generated item that the world stream has bound.
    pub fn bound_item(
        &self,
        items: &crate::interaction::items::ItemStore,
        instance: &InstanceId,
    ) -> Result<ItemId, SaveError> {
        match instance {
            InstanceId::Generated { id } => items
                .generated_item_id(id)
                .filter(|id| items.get(*id).is_some())
                .ok_or_else(|| SaveError::InvalidLinks("Generated item binding is pending".into())),
            InstanceId::Initial { .. } => self.item(instance),
        }
    }
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
            // A `SourceKey` path is already decimal text.
            let number = match value {
                Value::String(text) => text.parse::<i64>().map_err(|_| "Invalid source integer")?,
                _ => value.as_i64().ok_or("Invalid source integer")?,
            };
            *value = Value::String(number.to_string());
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
