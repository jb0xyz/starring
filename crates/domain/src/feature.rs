use discord_model::{ChannelId, RoleId};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct VerificationPanel {
    pub channel_id: ChannelId,
    pub grants_role: RoleId,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModerationRule {}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct LoggingRule {}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Feature {
    Verification(VerificationPanel),
    Moderation(ModerationRule),
    Logging(LoggingRule),
}

#[cfg(test)]
mod tests {
    use super::*;
    use discord_model::{ChannelId, RoleId};

    #[test]
    fn feature_verification_serde() {
        let f = Feature::Verification(VerificationPanel {
            channel_id: ChannelId(100),
            grants_role: RoleId(200),
        });
        let json = serde_json::to_string(&f).unwrap();
        assert_eq!(
            json,
            r#"{"kind":"verification","channel_id":"100","grants_role":"200"}"#
        );
        assert_eq!(serde_json::from_str::<Feature>(&json).unwrap(), f);
    }

    #[test]
    fn skeleton_features_serde() {
        let f = Feature::Moderation(ModerationRule::default());
        let json = serde_json::to_string(&f).unwrap();
        assert_eq!(json, r#"{"kind":"moderation"}"#);
        assert_eq!(serde_json::from_str::<Feature>(&json).unwrap(), f);
    }
}
