use automation_instance::InstanceStoreError;
use discord_model::{ChannelId, GuildId, MessageId, RoleId};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum InstanceResource {
    Role {
        alias: String,
        id: RoleId,
    },
    Channel {
        alias: String,
        id: ChannelId,
    },
    Message {
        alias: String,
        channel: ChannelId,
        id: MessageId,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DeleteOutcome {
    Deleted,
    AlreadyGone,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DeleterErrorKind {
    Forbidden,
    RateLimited,
    Network,
    Unknown,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DeleterError {
    pub kind: DeleterErrorKind,
    pub message: String,
}

#[allow(async_fn_in_trait)]
pub trait InstanceDeleter {
    async fn delete_message(
        &self,
        guild: GuildId,
        channel: ChannelId,
        message: MessageId,
    ) -> Result<DeleteOutcome, DeleterError>;
    async fn delete_channel(
        &self,
        guild: GuildId,
        channel: ChannelId,
    ) -> Result<DeleteOutcome, DeleterError>;
    async fn delete_role(
        &self,
        guild: GuildId,
        role: RoleId,
    ) -> Result<DeleteOutcome, DeleterError>;
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TeardownOutcome {
    Completed,
    ResumedAndCompleted,
    AlreadyDeleted,
    InProgress,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TeardownError {
    Lookup(InstanceStoreError),
    InstanceNotFound,
    DeleteFailed {
        resource: InstanceResource,
        source: DeleterError,
    },
    Store(InstanceStoreError),
}
