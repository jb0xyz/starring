use serde::{Deserialize, Serialize};

use crate::ids::{ChannelId, GuildId, RoleId, UserId};
use crate::permissions::Permissions;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ChannelType {
    Text,
    Voice,
    Category,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", content = "id", rename_all = "snake_case")]
pub enum OverwriteTarget {
    Role(RoleId),
    Member(UserId),
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Guild {
    pub id: GuildId,
    pub name: String,
    pub owner_id: UserId,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Role {
    pub id: RoleId,
    pub name: String,
    pub permissions: Permissions,
    pub position: i32,
    pub managed: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PermissionOverwrite {
    pub target: OverwriteTarget,
    pub allow: Permissions,
    pub deny: Permissions,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Channel {
    pub id: ChannelId,
    pub name: String,
    pub channel_type: ChannelType,
    pub parent_id: Option<ChannelId>,
    pub position: i32,
    pub overwrites: Vec<PermissionOverwrite>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Member {
    pub user_id: UserId,
    pub roles: Vec<RoleId>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn overwrite_target_serde() {
        let t = OverwriteTarget::Role(RoleId(7));
        let json = serde_json::to_string(&t).unwrap();
        assert_eq!(json, r#"{"type":"role","id":"7"}"#);
        assert_eq!(serde_json::from_str::<OverwriteTarget>(&json).unwrap(), t);
    }

    #[test]
    fn channel_type_serde() {
        assert_eq!(
            serde_json::to_string(&ChannelType::Category).unwrap(),
            r#""category""#
        );
    }

    #[test]
    fn role_roundtrip() {
        let role = Role {
            id: RoleId(1),
            name: "인증됨".to_string(),
            permissions: Permissions::VIEW_CHANNEL | Permissions::SEND_MESSAGES,
            position: 3,
            managed: false,
        };
        let json = serde_json::to_string(&role).unwrap();
        assert_eq!(serde_json::from_str::<Role>(&json).unwrap(), role);
    }

    #[test]
    fn channel_with_overwrites_roundtrip() {
        let channel = Channel {
            id: ChannelId(10),
            name: "일반".to_string(),
            channel_type: ChannelType::Text,
            parent_id: Some(ChannelId(9)),
            position: 0,
            overwrites: vec![PermissionOverwrite {
                target: OverwriteTarget::Role(RoleId(1)),
                allow: Permissions::VIEW_CHANNEL,
                deny: Permissions::empty(),
            }],
        };
        let json = serde_json::to_string(&channel).unwrap();
        assert_eq!(serde_json::from_str::<Channel>(&json).unwrap(), channel);
    }

    #[test]
    fn guild_and_member_roundtrip() {
        let guild = Guild {
            id: GuildId(1),
            name: "srv".into(),
            owner_id: UserId(99),
        };
        assert_eq!(
            serde_json::from_str::<Guild>(&serde_json::to_string(&guild).unwrap()).unwrap(),
            guild
        );
        let member = Member {
            user_id: UserId(5),
            roles: vec![RoleId(1), RoleId(2)],
        };
        assert_eq!(
            serde_json::from_str::<Member>(&serde_json::to_string(&member).unwrap()).unwrap(),
            member
        );
    }
}
