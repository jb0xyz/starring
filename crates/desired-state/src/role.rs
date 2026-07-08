use serde::{Deserialize, Serialize};

use discord_model::Permissions;

use crate::identity::Identity;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RoleIntent {
    #[serde(flatten)]
    pub identity: Identity,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub permissions: Option<Permissions>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::identity::{MatchStrategy, Ownership, ResourceKey, ResourceState};
    use discord_model::Permissions;

    #[test]
    fn role_intent_flatten_roundtrip() {
        let r = RoleIntent {
            identity: Identity {
                key: ResourceKey("verified_member".to_string()),
                match_by: MatchStrategy::ByName,
                ownership: Ownership::Managed,
                state: ResourceState::Present,
            },
            name: Some("인증됨".to_string()),
            permissions: Some(Permissions::empty()),
        };
        let json = serde_json::to_string(&r).unwrap();
        assert_eq!(serde_json::from_str::<RoleIntent>(&json).unwrap(), r);
    }

    #[test]
    fn role_intent_flatten_defaults() {
        let json = r#"{"key":"r1","name":"x"}"#;
        let r: RoleIntent = serde_json::from_str(json).unwrap();
        assert_eq!(r.identity.key, ResourceKey("r1".to_string()));
        assert_eq!(r.identity.ownership, Ownership::Managed);
        assert!(r.permissions.is_none());
    }
}
