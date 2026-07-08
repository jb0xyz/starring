use serde::{Deserialize, Serialize};

use crate::identity::{Identity, ResourceKey};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct VerificationIntent {
    #[serde(flatten)]
    pub identity: Identity,
    pub channel: ResourceKey,
    pub grants_role: ResourceKey,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModerationIntent {}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct LoggingIntent {}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "feature", content = "spec", rename_all = "snake_case")]
pub enum FeatureIntent {
    Verification(VerificationIntent),
    Moderation(ModerationIntent),
    Logging(LoggingIntent),
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::identity::{Identity, MatchStrategy, Ownership, ResourceKey, ResourceState};

    #[test]
    fn verification_feature_roundtrip() {
        let f = FeatureIntent::Verification(VerificationIntent {
            identity: Identity {
                key: ResourceKey("verify_panel".to_string()),
                match_by: MatchStrategy::ByName,
                ownership: Ownership::Managed,
                state: ResourceState::Present,
            },
            channel: ResourceKey("verification_channel".to_string()),
            grants_role: ResourceKey("verified_member".to_string()),
        });
        let json = serde_json::to_string(&f).unwrap();
        assert_eq!(serde_json::from_str::<FeatureIntent>(&json).unwrap(), f);
    }

    #[test]
    fn skeleton_feature_roundtrip() {
        let f = FeatureIntent::Moderation(ModerationIntent::default());
        let json = serde_json::to_string(&f).unwrap();
        assert_eq!(serde_json::from_str::<FeatureIntent>(&json).unwrap(), f);
    }
}
