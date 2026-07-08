use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Deserializer, Serialize, Serializer};

macro_rules! define_id {
    ($name:ident) => {
        #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
        pub struct $name(pub u64);

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                write!(f, "{}", self.0)
            }
        }

        impl FromStr for $name {
            type Err = std::num::ParseIntError;

            fn from_str(s: &str) -> Result<Self, Self::Err> {
                Ok($name(s.parse()?))
            }
        }

        impl Serialize for $name {
            fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
            where
                S: Serializer,
            {
                serializer.serialize_str(&self.0.to_string())
            }
        }

        impl<'de> Deserialize<'de> for $name {
            fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
            where
                D: Deserializer<'de>,
            {
                let s = String::deserialize(deserializer)?;
                s.parse().map_err(serde::de::Error::custom)
            }
        }
    };
}

define_id!(GuildId);
define_id!(RoleId);
define_id!(ChannelId);
define_id!(UserId);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn serializes_as_json_string() {
        let id = GuildId(123456789012345678);
        let json = serde_json::to_string(&id).unwrap();
        assert_eq!(json, "\"123456789012345678\"");
        let back: GuildId = serde_json::from_str(&json).unwrap();
        assert_eq!(back, id);
    }

    #[test]
    fn display_and_from_str_roundtrip() {
        let id: RoleId = "42".parse().unwrap();
        assert_eq!(id, RoleId(42));
        assert_eq!(id.to_string(), "42");
    }

    #[test]
    fn distinct_id_types_do_not_mix() {
        let a = ChannelId(1);
        let b = UserId(1);
        assert_eq!(a.0, b.0);
    }
}
