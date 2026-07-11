use serde::{Deserialize, Serialize};

use automation_instance::InstanceId;
use discord_model::{ChannelId, GuildId, MessageId, OverwriteTarget, Permissions, RoleId, UserId};

use crate::plan::ModalPresentation;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AdapterErrorKind {
    Forbidden,
    NotFound,
    RateLimited,
    Network,
    Unsupported,
    BadRequest,
    Unknown,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AdapterError {
    pub kind: AdapterErrorKind,
    pub message: String,
}

impl AdapterError {
    pub fn new(kind: AdapterErrorKind, message: impl Into<String>) -> Self {
        Self {
            kind,
            message: message.into(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CreateChannelSpec {
    pub name: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CreateRoleSpec {
    pub name: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PostPanelSpec {
    pub content: String,
    pub buttons: Vec<PostPanelButtonSpec>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PostPanelButtonSpec {
    pub label: String,
    pub route: ResolvedButtonRoute,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ResolvedButtonRoute {
    Static {
        key: String,
    },
    InstanceAction {
        instance_id: InstanceId,
        action: String,
    },
}

#[allow(async_fn_in_trait)]
pub trait DiscordMutationAdapter {
    async fn grant_role(
        &self,
        guild: GuildId,
        member: UserId,
        role: RoleId,
    ) -> Result<(), AdapterError>;

    async fn create_channel(
        &self,
        _guild: GuildId,
        _spec: CreateChannelSpec,
    ) -> Result<ChannelId, AdapterError> {
        Err(AdapterError::new(
            AdapterErrorKind::Unsupported,
            "create_channel is not supported",
        ))
    }

    async fn create_role(
        &self,
        _guild: GuildId,
        _spec: CreateRoleSpec,
    ) -> Result<RoleId, AdapterError> {
        Err(AdapterError::new(
            AdapterErrorKind::Unsupported,
            "create_role is not supported",
        ))
    }

    async fn upsert_overwrite(
        &self,
        _guild: GuildId,
        _channel: ChannelId,
        _target: OverwriteTarget,
        _allow: Permissions,
        _deny: Permissions,
    ) -> Result<(), AdapterError> {
        Err(AdapterError::new(
            AdapterErrorKind::Unsupported,
            "upsert_overwrite is not supported",
        ))
    }

    async fn post_panel(
        &self,
        _guild: GuildId,
        _channel: ChannelId,
        _spec: PostPanelSpec,
    ) -> Result<MessageId, AdapterError> {
        Err(AdapterError::new(
            AdapterErrorKind::Unsupported,
            "post_panel is not supported",
        ))
    }
}

#[allow(async_fn_in_trait)]
pub trait InteractionResponder {
    async fn respond_ephemeral(&self, content: String) -> Result<(), AdapterError>;

    async fn open_modal(&self, _modal: &ModalPresentation) -> Result<(), AdapterError> {
        Err(AdapterError::new(
            AdapterErrorKind::Unsupported,
            "open_modal is not supported",
        ))
    }

    async fn defer_ephemeral(&self) -> Result<(), AdapterError> {
        Err(AdapterError::new(
            AdapterErrorKind::Unsupported,
            "defer_ephemeral is not supported",
        ))
    }

    async fn edit_response(&self, _content: String) -> Result<(), AdapterError> {
        Err(AdapterError::new(
            AdapterErrorKind::Unsupported,
            "edit_response is not supported",
        ))
    }
}

pub struct AutomationServices<'a, M, R, S, G>
where
    M: DiscordMutationAdapter,
    R: InteractionResponder,
    S: automation_instance::InstanceStore,
    G: automation_instance::InstanceIdGenerator,
{
    pub mutation: &'a M,
    pub responder: &'a R,
    pub instances: &'a S,
    pub instance_ids: &'a G,
}
