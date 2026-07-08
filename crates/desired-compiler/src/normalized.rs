use serde::{Deserialize, Serialize};

use desired_state::{DesiredStateMode, Identity, ResourceKey, Scope};
use discord_model::{ChannelType, Permissions};

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct NormalizedDesiredState {
    pub mode: DesiredStateMode,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scope: Option<Scope>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub roles: Vec<NormalizedRole>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub channels: Vec<NormalizedChannel>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub verification_panels: Vec<NormalizedVerificationPanel>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct NormalizedRole {
    pub identity: Identity,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub permissions: Option<Permissions>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct NormalizedChannel {
    pub identity: Identity,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub channel_type: Option<ChannelType>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent: Option<ResourceKey>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub overwrites: Vec<NormalizedOverwrite>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct NormalizedOverwrite {
    pub target: NormalizedTarget,
    pub allow: Permissions,
    pub deny: Permissions,
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(tag = "target", content = "id", rename_all = "snake_case")]
pub enum NormalizedTarget {
    Everyone,
    Role(ResourceKey),
    Member(String),
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct NormalizedVerificationPanel {
    pub identity: Identity,
    pub channel: ResourceKey,
    pub grants_role: ResourceKey,
}

#[cfg(test)]
mod tests {
    use super::*;
    use desired_state::ResourceKey;
    use discord_model::Permissions;

    #[test]
    fn normalized_target_serde() {
        let t = NormalizedTarget::Everyone;
        assert_eq!(
            serde_json::to_string(&t).unwrap(),
            r#"{"target":"everyone"}"#
        );
        let r = NormalizedTarget::Role(ResourceKey("verified".to_string()));
        assert_eq!(
            serde_json::to_string(&r).unwrap(),
            r#"{"target":"role","id":"verified"}"#
        );
        assert_eq!(
            serde_json::from_str::<NormalizedTarget>(&serde_json::to_string(&r).unwrap()).unwrap(),
            r
        );
    }

    #[test]
    fn normalized_state_roundtrip() {
        let s = NormalizedDesiredState::default();
        let json = serde_json::to_string(&s).unwrap();
        assert_eq!(
            serde_json::from_str::<NormalizedDesiredState>(&json).unwrap(),
            s
        );
        let ow = NormalizedOverwrite {
            target: NormalizedTarget::Everyone,
            allow: Permissions::empty(),
            deny: Permissions::VIEW_CHANNEL,
        };
        let json = serde_json::to_string(&ow).unwrap();
        assert_eq!(
            serde_json::from_str::<NormalizedOverwrite>(&json).unwrap(),
            ow
        );
    }
}
