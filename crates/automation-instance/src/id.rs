use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Deserializer, Serialize, Serializer};

const MAX_LEN: usize = 32;

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct InstanceId(String);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InstanceIdError {
    Empty,
    TooLong,
    InvalidChar,
}

impl InstanceId {
    pub fn parse(value: &str) -> Result<Self, InstanceIdError> {
        if value.is_empty() {
            return Err(InstanceIdError::Empty);
        }
        if value.len() > MAX_LEN {
            return Err(InstanceIdError::TooLong);
        }
        if !value.chars().all(|character| {
            character.is_ascii_alphanumeric() || character == '_' || character == '-'
        }) {
            return Err(InstanceIdError::InvalidChar);
        }
        Ok(InstanceId(value.to_string()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for InstanceId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl FromStr for InstanceId {
    type Err = InstanceIdError;
    fn from_str(value: &str) -> Result<Self, Self::Err> {
        InstanceId::parse(value)
    }
}

impl TryFrom<String> for InstanceId {
    type Error = InstanceIdError;
    fn try_from(value: String) -> Result<Self, Self::Error> {
        InstanceId::parse(&value)
    }
}

impl AsRef<str> for InstanceId {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

impl Serialize for InstanceId {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&self.0)
    }
}

impl<'de> Deserialize<'de> for InstanceId {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        InstanceId::parse(&value).map_err(|error| serde::de::Error::custom(format!("{error:?}")))
    }
}
