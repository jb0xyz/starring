use serde::{Deserialize, Serialize};

use discord_model::{ChannelId, ChannelType, GuildId, OverwriteTarget, Permissions, RoleId};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RoleSpec {
    pub name: Option<String>,
    pub permissions: Option<Permissions>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChannelSpec {
    pub name: Option<String>,
    pub channel_type: Option<ChannelType>,
    pub parent_id: Option<ChannelId>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AdapterErrorKind {
    RateLimited,
    Timeout,
    Network,
    ServerError,
    Forbidden,
    MissingPermissions,
    RoleHierarchy,
    NotFound,
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

    pub fn is_retryable(&self) -> bool {
        matches!(
            self.kind,
            AdapterErrorKind::RateLimited
                | AdapterErrorKind::Timeout
                | AdapterErrorKind::Network
                | AdapterErrorKind::ServerError
        )
    }
}

#[allow(async_fn_in_trait)]
pub trait DiscordAdapter {
    async fn create_role(&self, guild: GuildId, spec: RoleSpec) -> Result<RoleId, AdapterError>;
    async fn update_role(
        &self,
        guild: GuildId,
        id: RoleId,
        spec: RoleSpec,
    ) -> Result<(), AdapterError>;
    async fn delete_role(&self, guild: GuildId, id: RoleId) -> Result<(), AdapterError>;
    async fn create_channel(
        &self,
        guild: GuildId,
        spec: ChannelSpec,
    ) -> Result<ChannelId, AdapterError>;
    async fn update_channel(
        &self,
        guild: GuildId,
        id: ChannelId,
        spec: ChannelSpec,
    ) -> Result<(), AdapterError>;
    async fn delete_channel(&self, guild: GuildId, id: ChannelId) -> Result<(), AdapterError>;
    async fn upsert_overwrite(
        &self,
        guild: GuildId,
        channel: ChannelId,
        target: OverwriteTarget,
        allow: Permissions,
        deny: Permissions,
    ) -> Result<(), AdapterError>;
}
