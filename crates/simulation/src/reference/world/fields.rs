//! Immutable sorted source fields. Links live only in their source value.
//!
//! JSON form: empty is `null`. Bool, integer, number and text are plain JSON values.
//! A number always has a fraction or exponent (`1.0`), so it differs from an integer.
//! An array is a JSON array. A record is a JSON object with sorted keys.
//! A link is `{"$":[file,path_id]}`. Unity field names cannot hold `$`.
use super::{SourceId, WorldInitField, WorldValue};
use serde::de::{self, MapAccess, SeqAccess, Visitor};
use serde::ser::{SerializeMap, SerializeSeq};
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::fmt;

const LINK: &str = "$";

#[derive(Clone, Debug)]
pub struct WorldFields(Vec<WorldInitField>);
impl Serialize for WorldFields {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut map = serializer.serialize_map(Some(self.0.len()))?;
        for field in &self.0 {
            map.serialize_entry(&field.name, &field.value)?;
        }
        map.end()
    }
}
impl<'de> Deserialize<'de> for WorldFields {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct FieldsVisitor;
        impl<'de> Visitor<'de> for FieldsVisitor {
            type Value = WorldFields;
            fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
                f.write_str("source fields")
            }
            fn visit_map<A: MapAccess<'de>>(self, map: A) -> Result<WorldFields, A::Error> {
                fields(map, None)
            }
        }
        deserializer.deserialize_map(FieldsVisitor)
    }
}
/// Reads sorted unique fields. `first` is a key the caller already read.
fn fields<'de, A: MapAccess<'de>>(
    mut map: A,
    first: Option<String>,
) -> Result<WorldFields, A::Error> {
    let mut entries = Vec::new();
    if let Some(name) = first {
        entries.push(WorldInitField {
            value: map.next_value()?,
            name,
        });
    }
    while let Some(name) = map.next_key::<String>()? {
        if entries.last().is_some_and(|f| f.name >= name) {
            return Err(de::Error::custom("Source fields must be sorted and unique"));
        }
        entries.push(WorldInitField {
            value: map.next_value()?,
            name,
        });
    }
    Ok(WorldFields(entries.into_boxed_slice().into_vec()))
}

impl Serialize for WorldValue {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self {
            Self::Empty => serializer.serialize_unit(),
            Self::Bool(v) => serializer.serialize_bool(*v),
            Self::Integer(v) => serializer.serialize_i64(*v),
            Self::Number(v) => serializer.serialize_f64(*v),
            Self::Text(v) => serializer.serialize_str(v),
            Self::Link(s) => {
                let mut map = serializer.serialize_map(Some(1))?;
                map.serialize_entry(LINK, &(&s.file, s.path_id))?;
                map.end()
            }
            Self::Array(values) => {
                let mut seq = serializer.serialize_seq(Some(values.len()))?;
                for v in values {
                    seq.serialize_element(v)?;
                }
                seq.end()
            }
            Self::Record(fields) => fields.serialize(serializer),
        }
    }
}
impl<'de> Deserialize<'de> for WorldValue {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        deserializer.deserialize_any(ValueVisitor)
    }
}
struct ValueVisitor;
impl<'de> Visitor<'de> for ValueVisitor {
    type Value = WorldValue;
    fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.write_str("a source value")
    }
    fn visit_unit<E>(self) -> Result<WorldValue, E> {
        Ok(WorldValue::Empty)
    }
    fn visit_bool<E>(self, v: bool) -> Result<WorldValue, E> {
        Ok(WorldValue::Bool(v))
    }
    fn visit_i64<E>(self, v: i64) -> Result<WorldValue, E> {
        Ok(WorldValue::Integer(v))
    }
    fn visit_u64<E: de::Error>(self, v: u64) -> Result<WorldValue, E> {
        i64::try_from(v)
            .map(WorldValue::Integer)
            .map_err(|_| E::custom("Source integer exceeds i64"))
    }
    fn visit_f64<E>(self, v: f64) -> Result<WorldValue, E> {
        Ok(WorldValue::Number(v))
    }
    fn visit_str<E>(self, v: &str) -> Result<WorldValue, E> {
        Ok(WorldValue::Text(v.to_owned()))
    }
    fn visit_string<E>(self, v: String) -> Result<WorldValue, E> {
        Ok(WorldValue::Text(v))
    }
    fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<WorldValue, A::Error> {
        let mut values = Vec::new();
        while let Some(v) = seq.next_element()? {
            values.push(v);
        }
        Ok(WorldValue::Array(values.into_boxed_slice().into_vec()))
    }
    fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<WorldValue, A::Error> {
        let Some(name) = map.next_key::<String>()? else {
            return Ok(WorldValue::Record(WorldFields(Vec::new())));
        };
        if name != LINK {
            return fields(map, Some(name)).map(WorldValue::Record);
        }
        let (file, path_id): (String, i64) = map.next_value()?;
        if map.next_key::<String>()?.is_some() {
            return Err(de::Error::custom("Source link has extra keys"));
        }
        Ok(WorldValue::Link(SourceId { file, path_id }))
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plain_values_round_trip() {
        let json = r#"{"a":null,"b":true,"c":3,"d":3.0,"e":"x","f":[1,{"$":["level1",7]}],"g":{"x":-0.5,"y":{}}}"#;
        let fields: WorldFields = serde_json::from_str(json).unwrap();
        assert!(matches!(fields["a"], WorldValue::Empty));
        assert!(matches!(fields["c"], WorldValue::Integer(3)));
        assert!(matches!(fields["d"], WorldValue::Number(v) if v == 3.0));
        let WorldValue::Array(f) = &fields["f"] else {
            panic!()
        };
        assert!(matches!(&f[1], WorldValue::Link(s) if s.file == "level1" && s.path_id == 7));
        assert!(matches!(&fields["g"], WorldValue::Record(r) if r.len() == 2));
        assert_eq!(serde_json::to_string(&fields).unwrap(), json);
        assert!(serde_json::from_str::<WorldFields>(r#"{"b":1,"a":2}"#).is_err());
        assert!(serde_json::from_str::<WorldFields>(r#"{"a":{"$":["f",1],"b":2}}"#).is_err());
    }
}
