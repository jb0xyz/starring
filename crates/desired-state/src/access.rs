use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use discord_model::Permissions;

use crate::identity::ResourceKey;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum Capability {
    View,
    Send,
    React,
    ManageMessages,
    Connect,
    Speak,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct AccessGrant {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub allow: Vec<Capability>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub deny: Vec<Capability>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct AccessIntent {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub everyone: Option<AccessGrant>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub roles: BTreeMap<ResourceKey, AccessGrant>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OverwriteOp {
    #[default]
    Add,
    Remove,
    Replace,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "target", content = "id", rename_all = "snake_case")]
pub enum OverwriteTargetIntent {
    Role(ResourceKey),
    Member(String),
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PermissionOverwriteIntent {
    pub target: OverwriteTargetIntent,
    #[serde(default)]
    pub op: OverwriteOp,
    #[serde(default = "Permissions::empty")]
    pub allow: Permissions,
    #[serde(default = "Permissions::empty")]
    pub deny: Permissions,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::identity::ResourceKey;
    use discord_model::Permissions;

    #[test]
    fn capability_serde() {
        assert_eq!(
            serde_json::to_string(&Capability::View).unwrap(),
            r#""view""#
        );
        assert_eq!(
            serde_json::to_string(&Capability::ManageMessages).unwrap(),
            r#""manage_messages""#
        );
    }

    #[test]
    fn access_intent_roundtrip() {
        let mut roles = std::collections::BTreeMap::new();
        roles.insert(
            ResourceKey("verified_member".to_string()),
            AccessGrant {
                allow: vec![Capability::View, Capability::Send],
                deny: vec![],
            },
        );
        let a = AccessIntent {
            everyone: Some(AccessGrant {
                allow: vec![],
                deny: vec![Capability::View],
            }),
            roles,
        };
        let json = serde_json::to_string(&a).unwrap();
        assert_eq!(serde_json::from_str::<AccessIntent>(&json).unwrap(), a);
    }

    #[test]
    fn overwrite_intent_roundtrip() {
        let o = PermissionOverwriteIntent {
            target: OverwriteTargetIntent::Role(ResourceKey("verified_member".to_string())),
            op: OverwriteOp::Add,
            allow: Permissions::VIEW_CHANNEL,
            deny: Permissions::empty(),
        };
        let json = serde_json::to_string(&o).unwrap();
        assert_eq!(
            serde_json::from_str::<PermissionOverwriteIntent>(&json).unwrap(),
            o
        );
        assert_eq!(OverwriteOp::default(), OverwriteOp::Add);
    }

    #[test]
    fn overwrite_target_serde() {
        let t = OverwriteTargetIntent::Member("42".to_string());
        let json = serde_json::to_string(&t).unwrap();
        assert_eq!(json, r#"{"target":"member","id":"42"}"#);
        assert_eq!(
            serde_json::from_str::<OverwriteTargetIntent>(&json).unwrap(),
            t
        );
    }
}
