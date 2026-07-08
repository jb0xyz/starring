use serde::{Deserialize, Serialize};

use discord_model::ChannelType;

use crate::access::{AccessIntent, PermissionOverwriteIntent};
use crate::identity::{Identity, ResourceKey};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChannelIntent {
    #[serde(flatten)]
    pub identity: Identity,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub channel_type: Option<ChannelType>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent: Option<ResourceKey>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub access: Option<AccessIntent>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub raw_overwrites: Option<Vec<PermissionOverwriteIntent>>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::access::{AccessGrant, AccessIntent, Capability};
    use crate::identity::{Identity, MatchStrategy, Ownership, ResourceKey, ResourceState};
    use discord_model::ChannelType;

    #[test]
    fn channel_intent_roundtrip() {
        let mut roles = std::collections::BTreeMap::new();
        roles.insert(
            ResourceKey("verified_member".to_string()),
            AccessGrant {
                allow: vec![Capability::View, Capability::Send],
                deny: vec![],
            },
        );
        let c = ChannelIntent {
            identity: Identity {
                key: ResourceKey("general_channel".to_string()),
                match_by: MatchStrategy::ByName,
                ownership: Ownership::Managed,
                state: ResourceState::Present,
            },
            name: Some("일반".to_string()),
            channel_type: Some(ChannelType::Text),
            parent: None,
            access: Some(AccessIntent {
                everyone: None,
                roles,
            }),
            raw_overwrites: None,
        };
        let json = serde_json::to_string(&c).unwrap();
        assert_eq!(serde_json::from_str::<ChannelIntent>(&json).unwrap(), c);
    }
}
