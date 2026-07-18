use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Deserializer, Serialize, Serializer};

const MAX_LEN: usize = 64;

#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum ActivationIdError {
    #[error("id is empty")]
    Empty,
    #[error("id is too long")]
    TooLong,
    #[error("id contains an invalid character")]
    InvalidChar,
}

macro_rules! define_id {
    ($name:ident) => {
        #[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
        pub struct $name(String);

        impl $name {
            pub fn parse(value: &str) -> Result<Self, ActivationIdError> {
                if value.is_empty() {
                    return Err(ActivationIdError::Empty);
                }
                if value.len() > MAX_LEN {
                    return Err(ActivationIdError::TooLong);
                }
                if !value
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'-')
                {
                    return Err(ActivationIdError::InvalidChar);
                }
                Ok(Self(value.to_string()))
            }

            pub fn as_str(&self) -> &str {
                &self.0
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                formatter.write_str(&self.0)
            }
        }

        impl FromStr for $name {
            type Err = ActivationIdError;

            fn from_str(value: &str) -> Result<Self, Self::Err> {
                Self::parse(value)
            }
        }

        impl TryFrom<String> for $name {
            type Error = ActivationIdError;

            fn try_from(value: String) -> Result<Self, Self::Error> {
                Self::parse(&value)
            }
        }

        impl AsRef<str> for $name {
            fn as_ref(&self) -> &str {
                &self.0
            }
        }

        impl Serialize for $name {
            fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
            where
                S: Serializer,
            {
                serializer.serialize_str(&self.0)
            }
        }

        impl<'de> Deserialize<'de> for $name {
            fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
            where
                D: Deserializer<'de>,
            {
                let value = String::deserialize(deserializer)?;
                Self::parse(&value).map_err(|error| serde::de::Error::custom(error.to_string()))
            }
        }
    };
}

define_id!(ActivationRequestId);
define_id!(ApplyAttemptId);

#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
#[error("activation hash identity must be lowercase SHA-256 hexadecimal")]
pub struct ActivationHashError;

macro_rules! define_hash {
    ($name:ident) => {
        #[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
        pub struct $name(String);

        impl $name {
            pub fn parse(value: &str) -> Result<Self, ActivationHashError> {
                if value.len() == 64
                    && value
                        .bytes()
                        .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
                {
                    Ok(Self(value.to_string()))
                } else {
                    Err(ActivationHashError)
                }
            }

            pub fn as_str(&self) -> &str {
                &self.0
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                formatter.write_str(&self.0)
            }
        }

        impl Serialize for $name {
            fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
            where
                S: Serializer,
            {
                serializer.serialize_str(&self.0)
            }
        }

        impl<'de> Deserialize<'de> for $name {
            fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
            where
                D: Deserializer<'de>,
            {
                let value = String::deserialize(deserializer)?;
                Self::parse(&value).map_err(serde::de::Error::custom)
            }
        }
    };
}

define_hash!(ActivationDigest);
define_hash!(ActivationPromotionId);
