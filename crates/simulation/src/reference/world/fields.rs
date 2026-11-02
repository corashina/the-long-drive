//! Immutable sorted source fields. Links live only in their source value.
use super::{WorldInitField, WorldValue, compact_vec};
use serde::{Deserialize, Deserializer, Serialize};
#[derive(Clone, Debug, Serialize)]
#[serde(transparent)]
pub struct WorldFields(Vec<WorldInitField>);
impl<'de> Deserialize<'de> for WorldFields {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let entries: Vec<WorldInitField> = compact_vec(deserializer)?;
        if entries.windows(2).any(|w| w[0].name >= w[1].name) {
            return Err(serde::de::Error::custom(
                "Source fields must be sorted and unique",
            ));
        }
        Ok(Self(entries))
    }
}
impl WorldFields {
    pub fn get(&self, name: &str) -> Option<&WorldValue> {
        self.0
            .binary_search_by(|f| f.name.as_str().cmp(name))
            .ok()
            .map(|i| &self.0[i].value)
    }
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
    pub fn len(&self) -> usize {
        self.0.len()
    }
    pub fn values(&self) -> impl Iterator<Item = &WorldValue> {
        self.0.iter().map(|f| &f.value)
    }
    pub fn iter(&self) -> impl Iterator<Item = (&String, &WorldValue)> {
        self.0.iter().map(|f| (&f.name, &f.value))
    }
    pub(super) fn entries(&self) -> &Vec<WorldInitField> {
        &self.0
    }
}
impl std::ops::Index<&str> for WorldFields {
    type Output = WorldValue;
    fn index(&self, name: &str) -> &WorldValue {
        self.get(name).expect("checked source field")
    }
}
impl super::WorldSourceRecord {
    pub fn components(&self) -> impl Iterator<Item = &super::SourceId> {
        let values = match self.fields.get("m_Component") {
            Some(WorldValue::Array(values)) => values.as_slice(),
            _ => &[],
        };
        values.iter().filter_map(|value| match value {
            WorldValue::Record(fields) => match fields.get("component") {
                Some(WorldValue::Link(source)) => Some(source),
                _ => None,
            },
            _ => None,
        })
    }
}
