use serde::{Deserialize, Serialize};

use discord_model::{ChannelId, GuildId, RoleId, UserId};

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
}
