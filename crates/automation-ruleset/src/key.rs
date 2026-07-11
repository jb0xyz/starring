use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Deserializer, Serialize, Serializer};

const MAX_LEN: usize = 64;

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct RuleSetKey(String);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RuleSetKeyError {
    Empty,
    TooLong,
    InvalidChar,
}

impl RuleSetKey {
    pub fn parse(value: &str) -> Result<Self, RuleSetKeyError> {
        if value.is_empty() {
            return Err(RuleSetKeyError::Empty);
        }
        if value.len() > MAX_LEN {
            return Err(RuleSetKeyError::TooLong);
        }
        if !value
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
        {
            return Err(RuleSetKeyError::InvalidChar);
        }
        Ok(RuleSetKey(value.to_string()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for RuleSetKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl FromStr for RuleSetKey {
    type Err = RuleSetKeyError;
    fn from_str(value: &str) -> Result<Self, Self::Err> {
        RuleSetKey::parse(value)
    }
}

impl AsRef<str> for RuleSetKey {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

impl Serialize for RuleSetKey {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.0)
    }
}

impl<'de> Deserialize<'de> for RuleSetKey {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let value = String::deserialize(deserializer)?;
        RuleSetKey::parse(&value).map_err(|e| serde::de::Error::custom(format!("{e:?}")))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn valid_keys_parse() {
        assert_eq!(
            RuleSetKey::parse("studyroom_demo").unwrap().as_str(),
            "studyroom_demo"
        );
        assert!(RuleSetKey::parse(&"a".repeat(64)).is_ok());
    }

    #[test]
    fn invalid_keys_rejected() {
        assert_eq!(RuleSetKey::parse(""), Err(RuleSetKeyError::Empty));
        assert_eq!(
            RuleSetKey::parse(&"a".repeat(65)),
            Err(RuleSetKeyError::TooLong)
        );
        assert_eq!(
            RuleSetKey::parse("bad key"),
            Err(RuleSetKeyError::InvalidChar)
        );
    }

    #[test]
    fn deserialize_rejects_invalid() {
        assert!(serde_json::from_str::<RuleSetKey>(r#""ok_key""#).is_ok());
        assert!(serde_json::from_str::<RuleSetKey>(r#""bad key""#).is_err());
    }
}
