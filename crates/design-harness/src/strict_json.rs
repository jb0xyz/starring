use std::collections::BTreeSet;
use std::fmt;

use serde::de::{Error, MapAccess, SeqAccess, Visitor};
use serde::{Deserialize, Deserializer};
use serde_json::{Map, Number, Value};

#[derive(Debug)]
pub(crate) enum StrictJsonError {
    Malformed(serde_json::Error),
    DuplicateObjectKey { path: String },
}

pub(crate) fn parse_json_with_unique_object_keys(source: &str) -> Result<Value, StrictJsonError> {
    let mut deserializer = serde_json::Deserializer::from_str(source);
    let parsed =
        StrictJsonValue::deserialize(&mut deserializer).map_err(StrictJsonError::Malformed)?;
    deserializer.end().map_err(StrictJsonError::Malformed)?;
    if let Some(path) = parsed.duplicate_path {
        return Err(StrictJsonError::DuplicateObjectKey { path });
    }
    Ok(parsed.value)
}

struct StrictJsonValue {
    value: Value,
    duplicate_path: Option<String>,
}

impl<'de> Deserialize<'de> for StrictJsonValue {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        deserializer.deserialize_any(StrictJsonVisitor)
    }
}

struct StrictJsonVisitor;

impl<'de> Visitor<'de> for StrictJsonVisitor {
    type Value = StrictJsonValue;

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("a JSON value")
    }

    fn visit_bool<E>(self, value: bool) -> Result<Self::Value, E> {
        Ok(strict_value(Value::Bool(value)))
    }

    fn visit_i64<E>(self, value: i64) -> Result<Self::Value, E> {
        Ok(strict_value(Value::Number(Number::from(value))))
    }

    fn visit_u64<E>(self, value: u64) -> Result<Self::Value, E> {
        Ok(strict_value(Value::Number(Number::from(value))))
    }

    fn visit_f64<E>(self, value: f64) -> Result<Self::Value, E>
    where
        E: Error,
    {
        let value = Number::from_f64(value).ok_or_else(|| E::custom("invalid JSON number"))?;
        Ok(strict_value(Value::Number(value)))
    }

    fn visit_str<E>(self, value: &str) -> Result<Self::Value, E> {
        Ok(strict_value(Value::String(value.to_string())))
    }

    fn visit_string<E>(self, value: String) -> Result<Self::Value, E> {
        Ok(strict_value(Value::String(value)))
    }

    fn visit_none<E>(self) -> Result<Self::Value, E> {
        Ok(strict_value(Value::Null))
    }

    fn visit_unit<E>(self) -> Result<Self::Value, E> {
        Ok(strict_value(Value::Null))
    }

    fn visit_some<D>(self, deserializer: D) -> Result<Self::Value, D::Error>
    where
        D: Deserializer<'de>,
    {
        StrictJsonValue::deserialize(deserializer)
    }

    fn visit_seq<A>(self, mut sequence: A) -> Result<Self::Value, A::Error>
    where
        A: SeqAccess<'de>,
    {
        let mut values = Vec::new();
        let mut duplicate_path = None;
        while let Some(value) = sequence.next_element::<StrictJsonValue>()? {
            if duplicate_path.is_none() {
                duplicate_path = value
                    .duplicate_path
                    .map(|path| format!("[{}].{path}", values.len()));
            }
            values.push(value.value);
        }
        Ok(StrictJsonValue {
            value: Value::Array(values),
            duplicate_path,
        })
    }

    fn visit_map<A>(self, mut object: A) -> Result<Self::Value, A::Error>
    where
        A: MapAccess<'de>,
    {
        let mut keys = BTreeSet::new();
        let mut values = Map::new();
        let mut duplicate_path = None;
        while let Some(key) = object.next_key::<String>()? {
            let duplicate_key = !keys.insert(key.clone());
            let value = object.next_value::<StrictJsonValue>()?;
            if duplicate_path.is_none() {
                duplicate_path = if duplicate_key {
                    Some(key.clone())
                } else {
                    value
                        .duplicate_path
                        .as_ref()
                        .map(|path| format!("{key}.{path}"))
                };
            }
            values.insert(key, value.value);
        }
        Ok(StrictJsonValue {
            value: Value::Object(values),
            duplicate_path,
        })
    }
}

fn strict_value(value: Value) -> StrictJsonValue {
    StrictJsonValue {
        value,
        duplicate_path: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unique_parser_accepts_every_json_root_shape() {
        for source in ["null", "true", "1", "\"text\"", "[]", "{}"] {
            parse_json_with_unique_object_keys(source).unwrap();
        }
    }

    #[test]
    fn unique_parser_reports_root_nested_and_array_duplicate_paths() {
        for (source, expected) in [
            (r#"{"a":1,"a":2}"#, "a"),
            (r#"{"a":{"b":1,"b":2}}"#, "a.b"),
            (r#"[{"a":1,"a":2}]"#, "[0].a"),
        ] {
            match parse_json_with_unique_object_keys(source) {
                Err(StrictJsonError::DuplicateObjectKey { path }) => {
                    assert_eq!(path, expected);
                }
                _ => panic!("expected duplicate object key"),
            }
        }
    }

    #[test]
    fn unique_parser_classifies_malformed_and_trailing_input() {
        for source in ["{", "{} trailing"] {
            assert!(matches!(
                parse_json_with_unique_object_keys(source),
                Err(StrictJsonError::Malformed(_))
            ));
        }
    }
}
