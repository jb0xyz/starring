use std::collections::{BTreeMap, BTreeSet};
use std::fmt::{Display, Formatter};

use automation_instance::{InstanceId, InstanceIdGenerationError, InstanceKind};
use automation_state::{ButtonSpec, ModalFieldSpec};
use discord_model::{Channel, ChannelId, GuildId, Member, Permissions, Role, RoleId, UserId};

use crate::event::RuntimeContext;
use crate::plan::ModalPresentation;
use crate::template::TemplateError;

pub const MAX_PREFLIGHT_ACTIONS_V1: usize = 256;
pub const MAX_PREFLIGHT_DIGEST_MATERIAL_BYTES_V1: usize = 1_048_576;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ActionEntryIdV1(u16);

impl ActionEntryIdV1 {
    pub fn ordinal(self) -> u16 {
        self.0
    }

    pub(crate) fn from_index(index: usize) -> Result<Self, ActionPlanPreflightErrorV1> {
        if index >= MAX_PREFLIGHT_ACTIONS_V1 {
            return Err(ActionPlanPreflightErrorV1::TooManyActions {
                count: index + 1,
                limit: MAX_PREFLIGHT_ACTIONS_V1,
            });
        }
        Ok(Self(index as u16))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct CreatedRoleOutputRefV1(ActionEntryIdV1);

impl CreatedRoleOutputRefV1 {
    pub fn producer(self) -> ActionEntryIdV1 {
        self.0
    }

    pub(crate) fn new(producer: ActionEntryIdV1) -> Self {
        Self(producer)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct CreatedChannelOutputRefV1(ActionEntryIdV1);

impl CreatedChannelOutputRefV1 {
    pub fn producer(self) -> ActionEntryIdV1 {
        self.0
    }

    pub(crate) fn new(producer: ActionEntryIdV1) -> Self {
        Self(producer)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct CreatedMessageOutputRefV1(ActionEntryIdV1);

impl CreatedMessageOutputRefV1 {
    pub fn producer(self) -> ActionEntryIdV1 {
        self.0
    }

    pub(crate) fn new(producer: ActionEntryIdV1) -> Self {
        Self(producer)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct CreatedInstanceOutputRefV1(ActionEntryIdV1);

impl CreatedInstanceOutputRefV1 {
    pub fn producer(self) -> ActionEntryIdV1 {
        self.0
    }

    pub(crate) fn new(producer: ActionEntryIdV1) -> Self {
        Self(producer)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ProducerOutputKindV1 {
    Role,
    Channel,
    Message,
    Instance,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum FreshObservationV1 {
    GuildRoles,
    GuildChannels,
    BotMember,
    ActorMember,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ActionInputDependencyV1 {
    Static,
    PriorEffect(ActionEntryIdV1),
    FreshObservation(FreshObservationV1),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ActionPlanSnapshotRequestV1 {
    guild_id: GuildId,
    actor: UserId,
    observations: BTreeSet<FreshObservationV1>,
    existing_roles: BTreeSet<RoleId>,
    existing_channels: BTreeSet<ChannelId>,
}

impl ActionPlanSnapshotRequestV1 {
    pub fn complete(guild_id: GuildId, actor: UserId) -> Self {
        Self {
            guild_id,
            actor,
            observations: BTreeSet::from([
                FreshObservationV1::GuildRoles,
                FreshObservationV1::GuildChannels,
                FreshObservationV1::BotMember,
                FreshObservationV1::ActorMember,
            ]),
            existing_roles: BTreeSet::new(),
            existing_channels: BTreeSet::new(),
        }
    }

    pub fn guild_id(&self) -> GuildId {
        self.guild_id
    }

    pub fn actor(&self) -> UserId {
        self.actor
    }

    pub fn observations(&self) -> &BTreeSet<FreshObservationV1> {
        &self.observations
    }

    pub fn existing_roles(&self) -> &BTreeSet<RoleId> {
        &self.existing_roles
    }

    pub fn existing_channels(&self) -> &BTreeSet<ChannelId> {
        &self.existing_channels
    }

    pub(crate) fn new(guild_id: GuildId, actor: UserId) -> Self {
        Self {
            guild_id,
            actor,
            observations: BTreeSet::new(),
            existing_roles: BTreeSet::new(),
            existing_channels: BTreeSet::new(),
        }
    }

    pub(crate) fn observe(&mut self, observation: FreshObservationV1) {
        self.observations.insert(observation);
    }

    pub(crate) fn require_role(&mut self, role_id: RoleId) {
        self.existing_roles.insert(role_id);
    }

    pub(crate) fn require_channel(&mut self, channel_id: ChannelId) {
        self.existing_channels.insert(channel_id);
    }
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ActionPlanSnapshotIdentityV1(String);

impl ActionPlanSnapshotIdentityV1 {
    pub fn new(value: String) -> Result<Self, ActionPlanPreflightErrorV1> {
        if value.is_empty()
            || value.len() > 128
            || !value
                .bytes()
                .all(|byte| byte.is_ascii_graphic() && !byte.is_ascii_whitespace())
        {
            return Err(ActionPlanPreflightErrorV1::InvalidSnapshotIdentity);
        }
        Ok(Self(value))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ActionPlanSnapshotV1 {
    pub guild_id: GuildId,
    pub identity: ActionPlanSnapshotIdentityV1,
    pub roles: Option<Vec<Role>>,
    pub channels: Option<Vec<Channel>>,
    pub bot_member: Option<Member>,
    pub actor_member: Option<Member>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PreflightRoleRefV1 {
    Existing(RoleId),
    Instance(RoleId),
    Produced(CreatedRoleOutputRefV1),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PreflightChannelRefV1 {
    Existing(ChannelId),
    Produced(CreatedChannelOutputRefV1),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PreflightOverwriteTargetV1 {
    Everyone,
    Role(PreflightRoleRefV1),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PreflightInstanceRefV1 {
    Existing(InstanceId),
    Registered(CreatedInstanceOutputRefV1),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PreflightButtonRouteV1 {
    Static {
        key: String,
    },
    InstanceAction {
        instance_id: InstanceId,
        producer: Option<CreatedInstanceOutputRefV1>,
        action: String,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PreflightButtonSpecV1 {
    pub label: String,
    pub route: PreflightButtonRouteV1,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PreflightInstanceResourceRefsV1 {
    pub roles: BTreeMap<String, CreatedRoleOutputRefV1>,
    pub channels: BTreeMap<String, CreatedChannelOutputRefV1>,
    pub messages: BTreeMap<String, CreatedMessageOutputRefV1>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PreparedPlanActionV1 {
    GrantRole {
        entry: ActionEntryIdV1,
        role: PreflightRoleRefV1,
        target: UserId,
    },
    RespondEphemeral {
        entry: ActionEntryIdV1,
        content: String,
    },
    OpenModal {
        entry: ActionEntryIdV1,
        modal: ModalPresentation,
    },
    CreateChannel {
        entry: ActionEntryIdV1,
        output: CreatedChannelOutputRefV1,
        key: String,
        name: String,
    },
    CreateRole {
        entry: ActionEntryIdV1,
        output: CreatedRoleOutputRefV1,
        key: String,
        name: String,
    },
    UpsertOverwrite {
        entry: ActionEntryIdV1,
        channel: PreflightChannelRefV1,
        target: PreflightOverwriteTargetV1,
        allow: Permissions,
        deny: Permissions,
    },
    PostPanel {
        entry: ActionEntryIdV1,
        output: CreatedMessageOutputRefV1,
        key: String,
        channel: PreflightChannelRefV1,
        content: String,
        buttons: Vec<PreflightButtonSpecV1>,
    },
    DeferEphemeral {
        entry: ActionEntryIdV1,
    },
    EditResponse {
        entry: ActionEntryIdV1,
        content: String,
    },
    RegisterInstance {
        entry: ActionEntryIdV1,
        output: CreatedInstanceOutputRefV1,
        key: String,
        id: InstanceId,
        kind: InstanceKind,
        resources: PreflightInstanceResourceRefsV1,
    },
    TeardownInstance {
        entry: ActionEntryIdV1,
        instance: PreflightInstanceRefV1,
    },
}

impl PreparedPlanActionV1 {
    pub fn entry(&self) -> ActionEntryIdV1 {
        match self {
            Self::GrantRole { entry, .. }
            | Self::RespondEphemeral { entry, .. }
            | Self::OpenModal { entry, .. }
            | Self::CreateChannel { entry, .. }
            | Self::CreateRole { entry, .. }
            | Self::UpsertOverwrite { entry, .. }
            | Self::PostPanel { entry, .. }
            | Self::DeferEphemeral { entry }
            | Self::EditResponse { entry, .. }
            | Self::RegisterInstance { entry, .. }
            | Self::TeardownInstance { entry, .. } => *entry,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PreparedActionPlanV1 {
    pub(crate) context: RuntimeContext,
    pub(crate) actions: Vec<PreparedPlanActionV1>,
    pub(crate) snapshot_request: ActionPlanSnapshotRequestV1,
    pub(crate) dependencies: BTreeMap<ActionEntryIdV1, BTreeSet<ActionInputDependencyV1>>,
    pub(crate) digest_material: Vec<u8>,
}

impl PreparedActionPlanV1 {
    pub fn context(&self) -> &RuntimeContext {
        &self.context
    }

    pub fn actions(&self) -> &[PreparedPlanActionV1] {
        &self.actions
    }

    pub fn snapshot_request(&self) -> &ActionPlanSnapshotRequestV1 {
        &self.snapshot_request
    }

    pub fn dependencies(&self) -> &BTreeMap<ActionEntryIdV1, BTreeSet<ActionInputDependencyV1>> {
        &self.dependencies
    }

    pub fn digest_material_v1(&self) -> &[u8] {
        &self.digest_material
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PreflightedActionPlanV1 {
    pub(crate) prepared: PreparedActionPlanV1,
    pub(crate) snapshot_identity: ActionPlanSnapshotIdentityV1,
}

impl PreflightedActionPlanV1 {
    pub fn context(&self) -> &RuntimeContext {
        self.prepared.context()
    }

    pub fn actions(&self) -> &[PreparedPlanActionV1] {
        self.prepared.actions()
    }

    pub fn snapshot_request(&self) -> &ActionPlanSnapshotRequestV1 {
        self.prepared.snapshot_request()
    }

    pub fn dependencies(&self) -> &BTreeMap<ActionEntryIdV1, BTreeSet<ActionInputDependencyV1>> {
        self.prepared.dependencies()
    }

    pub fn digest_material_v1(&self) -> &[u8] {
        self.prepared.digest_material_v1()
    }

    pub fn snapshot_identity(&self) -> &ActionPlanSnapshotIdentityV1 {
        &self.snapshot_identity
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ActionPlanPreflightErrorV1 {
    TooManyActions {
        count: usize,
        limit: usize,
    },
    InvalidContext,
    InvalidSnapshotIdentity,
    InvalidKey {
        entry: ActionEntryIdV1,
    },
    DuplicateProducerKey {
        key: String,
        first: ActionEntryIdV1,
        duplicate: ActionEntryIdV1,
    },
    UnknownProducer {
        entry: ActionEntryIdV1,
        key: String,
        expected: ProducerOutputKindV1,
    },
    ProducerTypeMismatch {
        entry: ActionEntryIdV1,
        key: String,
        expected: ProducerOutputKindV1,
        actual: ProducerOutputKindV1,
    },
    ProducerNotPrior {
        entry: ActionEntryIdV1,
        producer: ActionEntryIdV1,
    },
    InstanceContextMissing {
        entry: ActionEntryIdV1,
    },
    InstanceContextGuildMismatch {
        entry: ActionEntryIdV1,
    },
    InstanceResourceMissing {
        entry: ActionEntryIdV1,
        alias: String,
    },
    InstanceIdGeneration {
        entry: ActionEntryIdV1,
        error: InstanceIdGenerationError,
    },
    DuplicatePreallocatedInstanceId {
        entry: ActionEntryIdV1,
    },
    UnsupportedGrantTarget {
        entry: ActionEntryIdV1,
    },
    Template {
        entry: ActionEntryIdV1,
        error: TemplateError,
    },
    InvalidModal {
        entry: ActionEntryIdV1,
    },
    InvalidPanel {
        entry: ActionEntryIdV1,
    },
    ConflictingOverwrite {
        entry: ActionEntryIdV1,
    },
    DigestMaterialTooLarge {
        size: usize,
        limit: usize,
    },
    SnapshotGuildMismatch,
    MissingSnapshotEvidence(FreshObservationV1),
    UnexpectedSnapshotActor,
    InvalidSnapshotRoleSet,
    InvalidSnapshotChannelSet,
    InvalidSnapshotMember,
    RequiredRoleMissing(RoleId),
    RequiredChannelMissing(ChannelId),
    RoleLimitExceeded,
    ChannelLimitExceeded,
    OverwriteLimitExceeded {
        entry: ActionEntryIdV1,
    },
    BotPermissionMissing {
        entry: ActionEntryIdV1,
        permission: Permissions,
    },
    TargetRoleMissing {
        entry: ActionEntryIdV1,
    },
    TargetRoleUnassignable {
        entry: ActionEntryIdV1,
    },
    TargetRoleOutranksBot {
        entry: ActionEntryIdV1,
    },
    ChannelUnavailable {
        entry: ActionEntryIdV1,
    },
    ChannelCannotReceiveMessages {
        entry: ActionEntryIdV1,
    },
    SnapshotDrift,
    ExecutionInvariant {
        entry: ActionEntryIdV1,
    },
}

impl Display for ActionPlanPreflightErrorV1 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "action plan preflight failed: {self:?}")
    }
}

impl std::error::Error for ActionPlanPreflightErrorV1 {}

pub(crate) fn validate_modal_shape(
    entry: ActionEntryIdV1,
    modal: &ModalPresentation,
) -> Result<(), ActionPlanPreflightErrorV1> {
    if !bounded_text(&modal.key, 1, 100)
        || !bounded_text(&modal.title, 1, 45)
        || modal.fields.is_empty()
        || modal.fields.len() > 5
        || modal.fields.iter().any(invalid_modal_field)
    {
        return Err(ActionPlanPreflightErrorV1::InvalidModal { entry });
    }
    Ok(())
}

pub(crate) fn validate_buttons_shape(
    entry: ActionEntryIdV1,
    buttons: &[ButtonSpec],
) -> Result<(), ActionPlanPreflightErrorV1> {
    if buttons.len() > 5
        || buttons.iter().any(|button| {
            !bounded_text(&button.label, 1, 80)
                || match &button.route {
                    automation_state::ButtonRoute::Static { key } => !bounded_key(key),
                    automation_state::ButtonRoute::InstanceAction { action, .. } => {
                        !bounded_key(action)
                    }
                }
        })
    {
        return Err(ActionPlanPreflightErrorV1::InvalidPanel { entry });
    }
    Ok(())
}

pub(crate) fn bounded_key(value: &str) -> bool {
    bounded_text(value, 1, 100)
}

fn invalid_modal_field(field: &ModalFieldSpec) -> bool {
    !bounded_text(&field.key, 1, 100)
        || !bounded_text(&field.label, 1, 45)
        || field.max_length == Some(0)
        || field.max_length.is_some_and(|value| value > 4_000)
        || field.min_length.is_some_and(|value| value > 4_000)
        || matches!((field.min_length, field.max_length), (Some(min), Some(max)) if min > max)
}

fn bounded_text(value: &str, min: usize, max: usize) -> bool {
    let length = value.chars().count();
    length >= min && length <= max
}
