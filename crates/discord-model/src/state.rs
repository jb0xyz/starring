use serde::{Deserialize, Serialize};

use crate::entities::{Channel, Guild, Member, Role};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct GuildState {
    pub guild: Guild,
    pub roles: Vec<Role>,
    pub channels: Vec<Channel>,
    #[serde(default)]
    pub members: Vec<Member>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Guild, GuildId, UserId};

    #[test]
    fn guild_state_roundtrip() {
        let state = GuildState {
            guild: Guild {
                id: GuildId(1),
                name: "srv".into(),
                owner_id: UserId(99),
            },
            roles: vec![],
            channels: vec![],
            members: vec![],
        };
        let json = serde_json::to_string(&state).unwrap();
        assert_eq!(serde_json::from_str::<GuildState>(&json).unwrap(), state);
    }

    #[test]
    fn members_default_when_absent() {
        let json = r#"{"guild":{"id":"1","name":"srv","owner_id":"99"},"roles":[],"channels":[]}"#;
        let state: GuildState = serde_json::from_str(json).unwrap();
        assert!(state.members.is_empty());
    }
}
