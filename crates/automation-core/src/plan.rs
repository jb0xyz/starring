use automation_state::{ButtonSpec, ModalFieldSpec};
use discord_model::{ChannelId, MessageId, Permissions, RoleId, UserId};

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
pub enum PlannedRole {
    Resolved(RoleId),
    Created(String),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PlannedChannel {
    Resolved(ChannelId),
    Created(String),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PlannedOverwriteTarget {
    Everyone,
    Role(PlannedRole),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PlannedAction {
    GrantRole {
        role: PlannedRole,
        target: UserId,
    },
    RespondEphemeral {
        content: String,
    },
    OpenModal(ModalPresentation),
    CreateChannel {
        key: String,
        name: String,
    },
    CreateRole {
        key: String,
        name: String,
    },
    UpsertOverwrite {
        channel: PlannedChannel,
        target: PlannedOverwriteTarget,
        allow: Permissions,
        deny: Permissions,
    },
    PostPanel {
        channel: PlannedChannel,
        content: String,
        buttons: Vec<ButtonSpec>,
    },
    DeferEphemeral,
    EditResponse {
        content: String,
    },
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
    Message {
        action_index: usize,
        channel: ChannelId,
        id: MessageId,
    },
}
