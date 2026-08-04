use std::fmt::{Debug, Formatter};

use automation_instance::{
    AutomationInstance, InstanceId, InstanceKind, InstanceResources, InstanceRuleSetVersion,
    InstanceStoreError,
};
use discord_model::{ChannelId, GuildId, MessageId, RoleId, UserId};

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
    ManifestDrift,
    DeleteFailed {
        resource: InstanceResource,
        source: DeleterError,
    },
    Store(InstanceStoreError),
}

#[derive(Clone, PartialEq, Eq)]
pub struct ExactInstanceTeardownRequestV1 {
    guild_id: GuildId,
    instance_id: InstanceId,
    expected_resources: InstanceResources,
    expected_registration_identity: Option<ExactInstanceRegistrationIdentityV1>,
}

#[derive(Clone, PartialEq, Eq)]
pub struct ExactInstanceRegistrationIdentityV1 {
    ruleset_key: String,
    ruleset_version: InstanceRuleSetVersion,
    kind: InstanceKind,
    created_by: UserId,
}

impl ExactInstanceRegistrationIdentityV1 {
    pub fn new(
        ruleset_key: impl Into<String>,
        ruleset_version: InstanceRuleSetVersion,
        kind: InstanceKind,
        created_by: UserId,
    ) -> Self {
        Self {
            ruleset_key: ruleset_key.into(),
            ruleset_version,
            kind,
            created_by,
        }
    }

    pub fn from_exact_instance_v1(instance: &AutomationInstance) -> Self {
        Self::new(
            instance.ruleset_key.clone(),
            instance.ruleset_version,
            instance.kind.clone(),
            instance.created_by,
        )
    }

    pub fn ruleset_key(&self) -> &str {
        &self.ruleset_key
    }

    pub fn ruleset_version(&self) -> InstanceRuleSetVersion {
        self.ruleset_version
    }

    pub fn kind(&self) -> &InstanceKind {
        &self.kind
    }

    pub fn created_by(&self) -> UserId {
        self.created_by
    }

    fn matches_instance_v1(&self, instance: &AutomationInstance) -> bool {
        self.ruleset_key == instance.ruleset_key
            && self.ruleset_version == instance.ruleset_version
            && self.kind == instance.kind
            && self.created_by == instance.created_by
    }
}

impl Debug for ExactInstanceRegistrationIdentityV1 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("ExactInstanceRegistrationIdentityV1(<redacted>)")
    }
}

impl ExactInstanceTeardownRequestV1 {
    pub fn new(
        guild_id: GuildId,
        instance_id: InstanceId,
        expected_resources: InstanceResources,
    ) -> Self {
        Self {
            guild_id,
            instance_id,
            expected_resources,
            expected_registration_identity: None,
        }
    }

    pub fn new_exact_v1(
        guild_id: GuildId,
        instance_id: InstanceId,
        expected_resources: InstanceResources,
        expected_registration_identity: ExactInstanceRegistrationIdentityV1,
    ) -> Self {
        Self {
            guild_id,
            instance_id,
            expected_resources,
            expected_registration_identity: Some(expected_registration_identity),
        }
    }

    pub fn from_exact_instance_v1(instance: &AutomationInstance) -> Self {
        Self::new_exact_v1(
            instance.guild_id,
            instance.id.clone(),
            instance.resources.clone(),
            ExactInstanceRegistrationIdentityV1::from_exact_instance_v1(instance),
        )
    }

    pub fn guild_id(&self) -> GuildId {
        self.guild_id
    }

    pub fn instance_id(&self) -> &InstanceId {
        &self.instance_id
    }

    pub fn expected_resources(&self) -> &InstanceResources {
        &self.expected_resources
    }

    pub fn expected_registration_identity(&self) -> Option<&ExactInstanceRegistrationIdentityV1> {
        self.expected_registration_identity.as_ref()
    }

    pub(crate) fn matches_instance_v1(&self, instance: &AutomationInstance) -> bool {
        self.expected_resources == instance.resources
            && self
                .expected_registration_identity
                .as_ref()
                .is_none_or(|expected| expected.matches_instance_v1(instance))
    }
}

impl Debug for ExactInstanceTeardownRequestV1 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ExactInstanceTeardownRequestV1")
            .field("guild_id", &"<redacted>")
            .field("instance_id", &"<redacted>")
            .field("role_count", &self.expected_resources.roles.len())
            .field("channel_count", &self.expected_resources.channels.len())
            .field("message_count", &self.expected_resources.messages.len())
            .finish()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InstanceTeardownRecoveryObservationV1 {
    ProvenNotStarted,
    DurableRetryPending,
    ProvenSucceeded,
}
