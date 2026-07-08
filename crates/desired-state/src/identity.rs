use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct ResourceKey(pub String);

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "by", content = "value", rename_all = "snake_case")]
#[non_exhaustive]
pub enum MatchStrategy {
    #[default]
    ByName,
    ByExplicitId(String),
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Ownership {
    #[default]
    Managed,
    Adopted,
    Referenced,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ResourceState {
    #[default]
    Present,
    Absent,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Identity {
    pub key: ResourceKey,
    #[serde(rename = "match", default)]
    pub match_by: MatchStrategy,
    #[serde(default)]
    pub ownership: Ownership,
    #[serde(default)]
    pub state: ResourceState,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resource_key_serializes_as_string() {
        let k = ResourceKey("verified_member".to_string());
        assert_eq!(serde_json::to_string(&k).unwrap(), r#""verified_member""#);
    }

    #[test]
    fn enum_defaults() {
        assert_eq!(MatchStrategy::default(), MatchStrategy::ByName);
        assert_eq!(Ownership::default(), Ownership::Managed);
        assert_eq!(ResourceState::default(), ResourceState::Present);
    }

    #[test]
    fn identity_defaults_when_absent() {
        let json = r#"{"key":"r1"}"#;
        let id: Identity = serde_json::from_str(json).unwrap();
        assert_eq!(id.key, ResourceKey("r1".to_string()));
        assert_eq!(id.match_by, MatchStrategy::ByName);
        assert_eq!(id.ownership, Ownership::Managed);
        assert_eq!(id.state, ResourceState::Present);
    }

    #[test]
    fn match_explicit_id_serde() {
        let m = MatchStrategy::ByExplicitId("123".to_string());
        let json = serde_json::to_string(&m).unwrap();
        assert_eq!(json, r#"{"by":"by_explicit_id","value":"123"}"#);
        assert_eq!(serde_json::from_str::<MatchStrategy>(&json).unwrap(), m);
    }
}
