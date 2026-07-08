use serde::{Deserialize, Serialize};

use crate::channel::ChannelIntent;
use crate::feature::FeatureIntent;
use crate::mode::{DesiredStateMode, Scope};
use crate::role::RoleIntent;

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct DesiredState {
    #[serde(default)]
    pub mode: DesiredStateMode,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scope: Option<Scope>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub roles: Vec<RoleIntent>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub channels: Vec<ChannelIntent>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub features: Vec<FeatureIntent>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::identity::{Identity, ResourceKey};
    use crate::role::RoleIntent;

    #[test]
    fn empty_defaults_to_patch() {
        let json = r#"{}"#;
        let s: DesiredState = serde_json::from_str(json).unwrap();
        assert_eq!(s.mode, DesiredStateMode::Patch);
        assert!(s.roles.is_empty());
    }

    #[test]
    fn desired_state_roundtrip() {
        let s = DesiredState {
            mode: DesiredStateMode::Patch,
            scope: None,
            roles: vec![RoleIntent {
                identity: Identity {
                    key: ResourceKey("r1".to_string()),
                    ..Default::default()
                },
                name: Some("x".to_string()),
                permissions: None,
            }],
            channels: vec![],
            features: vec![],
        };
        let json = serde_json::to_string(&s).unwrap();
        assert_eq!(serde_json::from_str::<DesiredState>(&json).unwrap(), s);
    }
}
