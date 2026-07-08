use bitflags::bitflags;
use serde::{Deserialize, Deserializer, Serialize, Serializer};

bitflags! {
    #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
    pub struct Permissions: u64 {
        const CREATE_INSTANT_INVITE = 1 << 0;
        const KICK_MEMBERS          = 1 << 1;
        const BAN_MEMBERS           = 1 << 2;
        const ADMINISTRATOR         = 1 << 3;
        const MANAGE_CHANNELS       = 1 << 4;
        const MANAGE_GUILD          = 1 << 5;
        const VIEW_CHANNEL          = 1 << 10;
        const SEND_MESSAGES         = 1 << 11;
        const MANAGE_MESSAGES       = 1 << 13;
        const MANAGE_ROLES          = 1 << 28;
    }
}

impl Serialize for Permissions {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&self.bits().to_string())
    }
}

impl<'de> Deserialize<'de> for Permissions {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        let bits = s.parse::<u64>().map_err(serde::de::Error::custom)?;
        Ok(Permissions::from_bits_retain(bits))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bit_values_match_discord() {
        assert_eq!(Permissions::ADMINISTRATOR.bits(), 1 << 3);
        assert_eq!(Permissions::MANAGE_CHANNELS.bits(), 1 << 4);
        assert_eq!(Permissions::VIEW_CHANNEL.bits(), 1 << 10);
        assert_eq!(Permissions::SEND_MESSAGES.bits(), 1 << 11);
        assert_eq!(Permissions::MANAGE_ROLES.bits(), 1 << 28);
    }

    #[test]
    fn serializes_as_string() {
        let p = Permissions::VIEW_CHANNEL | Permissions::SEND_MESSAGES;
        let json = serde_json::to_string(&p).unwrap();
        assert_eq!(json, "\"3072\"");
        let back: Permissions = serde_json::from_str(&json).unwrap();
        assert_eq!(back, p);
    }

    #[test]
    fn retains_unmodeled_bits() {
        let json = "\"1099511627776\"";
        let p: Permissions = serde_json::from_str(json).unwrap();
        assert_eq!(p.bits(), 1u64 << 40);
        assert_eq!(serde_json::to_string(&p).unwrap(), json);
    }
}
