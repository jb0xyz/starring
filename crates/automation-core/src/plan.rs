use automation_state::ModalFieldSpec;
use discord_model::{ChannelId, RoleId, UserId};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ActionPlan {
    pub steps: Vec<PlannedAction>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ModalPresentation {
    pub key: String,
    pub title: String,
    pub fields: Vec<ModalFieldSpec>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PlannedAction {
    GrantRole { role: RoleId, target: UserId },
    RespondEphemeral { content: String },
    OpenModal(ModalPresentation),
    CreateChannel { name: String },
    CreateRole { name: String },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CreatedResource {
    Channel {
        action_index: usize,
        name: String,
        id: ChannelId,
    },
    Role {
        action_index: usize,
        name: String,
        id: RoleId,
    },
}
