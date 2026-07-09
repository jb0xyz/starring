use serde::{Deserialize, Serialize};

use discord_model::{GuildState, RoleId};

use crate::permissions::{can_send, can_view};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SubjectSpec {
    pub name: String,
    pub roles: Vec<RoleId>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AccessCell {
    pub subject: String,
    pub channel: String,
    pub can_view: bool,
    pub can_send: bool,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct AccessMatrix {
    pub cells: Vec<AccessCell>,
}

pub fn access_matrix(guild: &GuildState, subjects: &[SubjectSpec]) -> AccessMatrix {
    let mut cells = Vec::new();
    for subject in subjects {
        for channel in &guild.channels {
            cells.push(AccessCell {
                subject: subject.name.clone(),
                channel: channel.name.clone(),
                can_view: can_view(guild, &subject.roles, channel),
                can_send: can_send(guild, &subject.roles, channel),
            });
        }
    }
    AccessMatrix { cells }
}

#[cfg(test)]
mod tests {
    use super::*;
    use discord_model::{
        Channel, ChannelId, ChannelType, Guild, GuildId, GuildState, Permissions, Role, RoleId,
        UserId,
    };

    #[test]
    fn matrix_covers_subjects_and_channels() {
        let g = GuildState {
            guild: Guild {
                id: GuildId(1),
                name: "g".to_string(),
                owner_id: UserId(1),
            },
            roles: vec![Role {
                id: RoleId(1),
                name: "everyone".to_string(),
                permissions: Permissions::VIEW_CHANNEL,
                position: 0,
                managed: false,
            }],
            channels: vec![Channel {
                id: ChannelId(10),
                name: "general".to_string(),
                channel_type: ChannelType::Text,
                parent_id: None,
                position: 0,
                overwrites: vec![],
            }],
            members: vec![],
        };
        let subjects = vec![SubjectSpec {
            name: "new".to_string(),
            roles: vec![],
        }];
        let m = access_matrix(&g, &subjects);
        assert_eq!(m.cells.len(), 1);
        assert_eq!(m.cells[0].subject, "new");
        assert_eq!(m.cells[0].channel, "general");
        assert!(m.cells[0].can_view);
    }
}
