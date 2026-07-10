use serde::{Deserialize, Serialize};

use discord_model::{GuildId, RoleId, UserId};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AdapterErrorKind {
    Forbidden,
    NotFound,
    RateLimited,
    Network,
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

#[allow(async_fn_in_trait)]
pub trait DiscordMutationAdapter {
    async fn grant_role(
        &self,
        guild: GuildId,
        member: UserId,
        role: RoleId,
    ) -> Result<(), AdapterError>;
}

#[allow(async_fn_in_trait)]
pub trait InteractionResponder {
    async fn respond_ephemeral(&self, content: String) -> Result<(), AdapterError>;
}
