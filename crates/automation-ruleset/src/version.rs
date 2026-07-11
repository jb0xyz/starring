use std::fmt;
use std::num::NonZeroU32;

use serde::{Deserialize, Deserializer, Serialize, Serializer};

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct RuleSetVersionId(NonZeroU32);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RuleSetVersionError {
    Zero,
    Overflow,
}

impl RuleSetVersionId {
    pub const FIRST: RuleSetVersionId = RuleSetVersionId(NonZeroU32::MIN);

    pub fn new(value: u32) -> Result<Self, RuleSetVersionError> {
        NonZeroU32::new(value)
            .map(RuleSetVersionId)
            .ok_or(RuleSetVersionError::Zero)
    }

    pub fn get(self) -> u32 {
        self.0.get()
    }

    pub fn next(self) -> Result<Self, RuleSetVersionError> {
        self.0
            .get()
            .checked_add(1)
            .and_then(NonZeroU32::new)
            .map(RuleSetVersionId)
            .ok_or(RuleSetVersionError::Overflow)
    }
}

impl fmt::Display for RuleSetVersionId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0.get())
    }
}

impl Serialize for RuleSetVersionId {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_u32(self.0.get())
    }
}

impl<'de> Deserialize<'de> for RuleSetVersionId {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let value = u32::deserialize(deserializer)?;
        RuleSetVersionId::new(value).map_err(|e| serde::de::Error::custom(format!("{e:?}")))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct RuleSetSchemaVersion(NonZeroU32);

impl RuleSetSchemaVersion {
    pub fn new(value: u32) -> Result<Self, RuleSetVersionError> {
        NonZeroU32::new(value)
            .map(RuleSetSchemaVersion)
            .ok_or(RuleSetVersionError::Zero)
    }

    pub fn get(self) -> u32 {
        self.0.get()
    }
}

impl Serialize for RuleSetSchemaVersion {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_u32(self.0.get())
    }
}

impl<'de> Deserialize<'de> for RuleSetSchemaVersion {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let value = u32::deserialize(deserializer)?;
        RuleSetSchemaVersion::new(value).map_err(|e| serde::de::Error::custom(format!("{e:?}")))
    }
}

pub const CURRENT_RULESET_SCHEMA_VERSION: RuleSetSchemaVersion =
    RuleSetSchemaVersion(NonZeroU32::MIN);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn version_zero_rejected() {
        assert_eq!(RuleSetVersionId::new(0), Err(RuleSetVersionError::Zero));
        assert_eq!(RuleSetSchemaVersion::new(0), Err(RuleSetVersionError::Zero));
        assert!(serde_json::from_str::<RuleSetVersionId>("0").is_err());
        assert!(serde_json::from_str::<RuleSetSchemaVersion>("0").is_err());
    }

    #[test]
    fn first_and_next() {
        assert_eq!(RuleSetVersionId::FIRST.get(), 1);
        assert_eq!(RuleSetVersionId::FIRST.next().unwrap().get(), 2);
    }

    #[test]
    fn next_overflow_is_error() {
        let max = RuleSetVersionId::new(u32::MAX).unwrap();
        assert_eq!(max.next(), Err(RuleSetVersionError::Overflow));
    }

    #[test]
    fn current_schema_is_one() {
        assert_eq!(CURRENT_RULESET_SCHEMA_VERSION.get(), 1);
    }
}
