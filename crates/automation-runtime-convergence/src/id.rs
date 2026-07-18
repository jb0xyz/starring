use std::fmt::{Display, Formatter};
use std::num::NonZeroU64;

use serde::{Deserialize, Deserializer, Serialize, Serializer};

#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum OpaqueRuntimeIdError {
    #[error("runtime identifier must not be empty")]
    Empty,
    #[error("runtime identifier exceeds 128 bytes")]
    TooLong,
    #[error("runtime identifier contains unsupported characters")]
    InvalidCharacter,
}

macro_rules! define_opaque_id {
    ($name:ident) => {
        #[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
        #[serde(transparent)]
        pub struct $name(String);

        impl $name {
            pub fn parse(value: impl Into<String>) -> Result<Self, OpaqueRuntimeIdError> {
                let value = value.into();
                if value.is_empty() {
                    return Err(OpaqueRuntimeIdError::Empty);
                }
                if value.len() > 128 {
                    return Err(OpaqueRuntimeIdError::TooLong);
                }
                if !value.bytes().all(|byte| {
                    byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b':' | b'.')
                }) {
                    return Err(OpaqueRuntimeIdError::InvalidCharacter);
                }
                Ok(Self(value))
            }

            pub fn as_str(&self) -> &str {
                &self.0
            }
        }

        impl Display for $name {
            fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
                formatter.write_str(&self.0)
            }
        }

        impl<'de> Deserialize<'de> for $name {
            fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
            where
                D: Deserializer<'de>,
            {
                let value = String::deserialize(deserializer)?;
                Self::parse(value).map_err(serde::de::Error::custom)
            }
        }
    };
}

define_opaque_id!(DeploymentId);
define_opaque_id!(TenantId);
define_opaque_id!(InstallationId);
define_opaque_id!(ControllerId);
define_opaque_id!(ProcessInstanceId);
define_opaque_id!(ActivationRequestId);
define_opaque_id!(PanelCertificateId);
define_opaque_id!(RuntimeFailureId);

#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum PromotionIdError {
    #[error("promotion identifier must contain exactly 64 characters")]
    Length,
    #[error("promotion identifier must be lowercase hexadecimal")]
    LowerHex,
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(transparent)]
pub struct PromotionId(String);

impl PromotionId {
    pub fn parse(value: impl Into<String>) -> Result<Self, PromotionIdError> {
        let value = value.into();
        if value.len() != 64 {
            return Err(PromotionIdError::Length);
        }
        if !value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        {
            return Err(PromotionIdError::LowerHex);
        }
        Ok(Self(value))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl Display for PromotionId {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl<'de> Deserialize<'de> for PromotionId {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        Self::parse(value).map_err(serde::de::Error::custom)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum RuntimeRevisionError {
    #[error("runtime revision must be non-zero")]
    Zero,
    #[error("runtime revision overflow")]
    Overflow,
}

macro_rules! define_revision {
    ($name:ident) => {
        #[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
        pub struct $name(NonZeroU64);

        impl $name {
            pub const FIRST: Self = Self(NonZeroU64::MIN);

            pub fn new(value: u64) -> Result<Self, RuntimeRevisionError> {
                NonZeroU64::new(value)
                    .map(Self)
                    .ok_or(RuntimeRevisionError::Zero)
            }

            pub fn get(self) -> u64 {
                self.0.get()
            }

            pub fn next(self) -> Result<Self, RuntimeRevisionError> {
                self.get()
                    .checked_add(1)
                    .and_then(NonZeroU64::new)
                    .map(Self)
                    .ok_or(RuntimeRevisionError::Overflow)
            }
        }

        impl Display for $name {
            fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
                write!(formatter, "{}", self.get())
            }
        }

        impl Serialize for $name {
            fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
            where
                S: Serializer,
            {
                serializer.serialize_u64(self.get())
            }
        }

        impl<'de> Deserialize<'de> for $name {
            fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
            where
                D: Deserializer<'de>,
            {
                let value = u64::deserialize(deserializer)?;
                Self::new(value).map_err(serde::de::Error::custom)
            }
        }
    };
}

define_revision!(DeploymentRevision);
define_revision!(RuntimeGeneration);
define_revision!(BindingRevision);
define_revision!(FencingToken);
