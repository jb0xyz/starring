use std::collections::BTreeMap;

use discord_model::RoleId;
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OnboardingMode {
    Open,
    VerificationRequired,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Visibility {
    pub everyone: bool,
    #[serde(default)]
    pub roles: BTreeMap<RoleId, bool>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use discord_model::RoleId;

    #[test]
    fn onboarding_mode_serde() {
        assert_eq!(
            serde_json::to_string(&OnboardingMode::VerificationRequired).unwrap(),
            r#""verification_required""#
        );
    }

    #[test]
    fn visibility_roundtrip() {
        let mut roles = std::collections::BTreeMap::new();
        roles.insert(RoleId(1), true);
        let v = Visibility {
            everyone: false,
            roles,
        };
        let json = serde_json::to_string(&v).unwrap();
        assert_eq!(json, r#"{"everyone":false,"roles":{"1":true}}"#);
        assert_eq!(serde_json::from_str::<Visibility>(&json).unwrap(), v);
    }
}
