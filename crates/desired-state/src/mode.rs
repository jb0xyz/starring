use serde::{Deserialize, Serialize};

use crate::identity::ResourceKey;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DesiredStateMode {
    #[default]
    Patch,
    ScopedAuthoritative,
    FullAuthoritative,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "value", rename_all = "snake_case")]
pub enum ResourceScope {
    All,
    Keys(Vec<ResourceKey>),
    NamePrefix(String),
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Scope {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub roles: Option<ResourceScope>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub channels: Option<ResourceScope>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mode_default_and_serde() {
        assert_eq!(DesiredStateMode::default(), DesiredStateMode::Patch);
        assert_eq!(
            serde_json::to_string(&DesiredStateMode::ScopedAuthoritative).unwrap(),
            r#""scoped_authoritative""#
        );
    }

    #[test]
    fn resource_scope_serde() {
        let s = ResourceScope::Keys(vec![ResourceKey("a".to_string())]);
        let json = serde_json::to_string(&s).unwrap();
        assert_eq!(json, r#"{"kind":"keys","value":["a"]}"#);
        assert_eq!(serde_json::from_str::<ResourceScope>(&json).unwrap(), s);
    }

    #[test]
    fn scope_roundtrip() {
        let scope = Scope {
            roles: Some(ResourceScope::All),
            channels: None,
        };
        let json = serde_json::to_string(&scope).unwrap();
        assert_eq!(serde_json::from_str::<Scope>(&json).unwrap(), scope);
    }
}
