use std::fmt;
use std::num::NonZeroU32;

use serde::{Deserialize, Deserializer, Serialize, Serializer};

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct InstanceRuleSetVersion(NonZeroU32);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InstanceRuleSetVersionError {
    Zero,
}

impl InstanceRuleSetVersion {
    pub fn new(value: u32) -> Result<Self, InstanceRuleSetVersionError> {
        NonZeroU32::new(value)
            .map(InstanceRuleSetVersion)
            .ok_or(InstanceRuleSetVersionError::Zero)
    }

    pub fn get(self) -> u32 {
        self.0.get()
    }
}

impl fmt::Display for InstanceRuleSetVersion {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0.get())
    }
}

impl Serialize for InstanceRuleSetVersion {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_u32(self.0.get())
    }
}

impl<'de> Deserialize<'de> for InstanceRuleSetVersion {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let value = u32::deserialize(deserializer)?;
        InstanceRuleSetVersion::new(value)
            .map_err(|error| serde::de::Error::custom(format!("{error:?}")))
    }
}
