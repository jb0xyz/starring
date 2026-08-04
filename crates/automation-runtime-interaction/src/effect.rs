use std::num::{NonZeroU16, NonZeroU64};

use automation_instance::InstanceKind;
use discord_model::{ChannelId, GuildId, MessageId, RoleId, UserId};

use crate::effect_digest::{
    InteractionEffectActionDigestV1, InteractionEffectExpectedPostimageDigestV1,
    InteractionEffectIdentityDigestV1, InteractionEffectInputDigestV1,
    InteractionEffectObservationEvidenceDigestV1, InteractionEffectOpaqueIdentityDigestV1,
    InteractionEffectOutputDigestV1, InteractionEffectPayloadDigestV1,
    InteractionEffectPlannedIdentityDigestV1, InteractionEffectPreimageDigestV1,
};
use crate::{
    InteractionActionPlanDigestV1, InteractionPreflightCertificateDigestV1,
    InteractionReceiptIdentityV1,
};

pub const MAX_INTERACTION_EFFECT_ACTIONS_V1: u16 = 256;
pub const MAX_INTERACTION_EFFECT_DEPENDENCIES_V1: usize = 32;
pub const MAX_INTERACTION_EFFECT_ATTEMPTS_V1: u16 = 64;

#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum InteractionEffectIdentityErrorV1 {
    #[error("interaction effect action index is outside the supported range")]
    ActionIndex,
    #[error("interaction effect attempt is outside the supported range")]
    Attempt,
    #[error("interaction effect external identity must be non-zero")]
    ExternalIdentity,
    #[error("interaction effect permission allow and deny sets overlap")]
    PermissionOverlap,
    #[error("interaction effect instance state is invalid")]
    InstanceState,
    #[error("interaction effect target does not match its action kind")]
    TargetKind,
    #[error("interaction effect preimage does not match its target")]
    Preimage,
    #[error("interaction effect dependency count is outside the supported range")]
    DependencyCount,
    #[error("interaction effect dependencies must be unique and ordered")]
    DependencyOrder,
    #[error("interaction effect dependency must precede its consumer")]
    DependencyNotPrior,
    #[error("interaction effect dependency resolution count does not match its plan")]
    DependencyResolutionCount,
    #[error("interaction effect dependency resolution does not match its plan")]
    DependencyResolutionMismatch,
    #[error("interaction effect observed output does not match its definition")]
    ObservedOutput,
    #[error("interaction effect recovery correlation does not match its planned identity")]
    RecoveryCorrelation,
    #[error("interaction effect recovery binding does not match its resolved identity")]
    RecoveryIdentity,
    #[error("interaction effect HTTP status is invalid")]
    HttpStatus,
}

macro_rules! define_external_identity {
    ($name:ident, $external:ident) => {
        #[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
        pub struct $name(NonZeroU64);

        impl $name {
            pub fn new(value: u64) -> Result<Self, InteractionEffectIdentityErrorV1> {
                NonZeroU64::new(value)
                    .map(Self)
                    .ok_or(InteractionEffectIdentityErrorV1::ExternalIdentity)
            }

            pub fn get(self) -> u64 {
                self.0.get()
            }
        }

        impl TryFrom<$external> for $name {
            type Error = InteractionEffectIdentityErrorV1;

            fn try_from(value: $external) -> Result<Self, Self::Error> {
                Self::new(value.0)
            }
        }

        impl From<$name> for $external {
            fn from(value: $name) -> Self {
                Self(value.get())
            }
        }
    };
}

define_external_identity!(InteractionEffectGuildIdV1, GuildId);
define_external_identity!(InteractionEffectRoleIdV1, RoleId);
define_external_identity!(InteractionEffectChannelIdV1, ChannelId);
define_external_identity!(InteractionEffectUserIdV1, UserId);
define_external_identity!(InteractionEffectMessageIdV1, MessageId);

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct InteractionEffectActionIndexV1(u16);

impl InteractionEffectActionIndexV1 {
    pub fn new(value: u16) -> Result<Self, InteractionEffectIdentityErrorV1> {
        if value >= MAX_INTERACTION_EFFECT_ACTIONS_V1 {
            return Err(InteractionEffectIdentityErrorV1::ActionIndex);
        }
        Ok(Self(value))
    }

    pub fn get(self) -> u16 {
        self.0
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct InteractionEffectAttemptV1(NonZeroU16);

impl InteractionEffectAttemptV1 {
    pub fn new(value: u16) -> Result<Self, InteractionEffectIdentityErrorV1> {
        let value = NonZeroU16::new(value).ok_or(InteractionEffectIdentityErrorV1::Attempt)?;
        if value.get() > MAX_INTERACTION_EFFECT_ATTEMPTS_V1 {
            return Err(InteractionEffectIdentityErrorV1::Attempt);
        }
        Ok(Self(value))
    }

    pub fn get(self) -> u16 {
        self.0.get()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum InteractionEffectKindV1 {
    CreateRole,
    CreateChannel,
    GrantRole,
    UpsertOverwrite,
    PostPanel,
    RegisterInstance,
    TeardownInstance,
    EditResponse,
}

impl InteractionEffectKindV1 {
    pub fn code(self) -> &'static str {
        match self {
            Self::CreateRole => "create_role",
            Self::CreateChannel => "create_channel",
            Self::GrantRole => "grant_role",
            Self::UpsertOverwrite => "upsert_overwrite",
            Self::PostPanel => "post_panel",
            Self::RegisterInstance => "register_instance",
            Self::TeardownInstance => "teardown_instance",
            Self::EditResponse => "edit_response",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum InteractionEffectOutputClassV1 {
    CreatedRole,
    CreatedChannel,
    RoleMembership,
    PermissionOverwrite,
    PostedMessage,
    InstanceState,
    OriginalResponse,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum InteractionEffectCorrelationClassV1 {
    AuditLogReason,
    MessageNonce,
    InternalIdempotencyKey,
    InteractionReceipt,
    Unsupported,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum InteractionEffectCompensationClassV1 {
    DeleteCreatedRole,
    DeleteCreatedChannel,
    RestoreRoleMembership,
    RestorePermissionOverwrite,
    DeletePostedMessage,
    RestoreInstanceRegistration,
    NotCompensable,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum InteractionEffectRecoveryScopeV1 {
    MutableProvisioning,
    ResponseTail,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum InteractionEffectOverwriteTargetV1 {
    Role(InteractionEffectRoleIdV1),
    Member(InteractionEffectUserIdV1),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct InteractionEffectRoleMembershipTargetV1 {
    guild_id: InteractionEffectGuildIdV1,
    user_id: InteractionEffectUserIdV1,
    role_id: InteractionEffectRoleIdV1,
}

impl InteractionEffectRoleMembershipTargetV1 {
    pub fn new(
        guild_id: InteractionEffectGuildIdV1,
        user_id: InteractionEffectUserIdV1,
        role_id: InteractionEffectRoleIdV1,
    ) -> Self {
        Self {
            guild_id,
            user_id,
            role_id,
        }
    }

    pub fn guild_id(self) -> InteractionEffectGuildIdV1 {
        self.guild_id
    }

    pub fn user_id(self) -> InteractionEffectUserIdV1 {
        self.user_id
    }

    pub fn role_id(self) -> InteractionEffectRoleIdV1 {
        self.role_id
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct InteractionEffectPermissionTargetV1 {
    guild_id: InteractionEffectGuildIdV1,
    channel_id: InteractionEffectChannelIdV1,
    target: InteractionEffectOverwriteTargetV1,
}

impl InteractionEffectPermissionTargetV1 {
    pub fn new(
        guild_id: InteractionEffectGuildIdV1,
        channel_id: InteractionEffectChannelIdV1,
        target: InteractionEffectOverwriteTargetV1,
    ) -> Self {
        Self {
            guild_id,
            channel_id,
            target,
        }
    }

    pub fn guild_id(self) -> InteractionEffectGuildIdV1 {
        self.guild_id
    }

    pub fn channel_id(self) -> InteractionEffectChannelIdV1 {
        self.channel_id
    }

    pub fn target(self) -> InteractionEffectOverwriteTargetV1 {
        self.target
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct InteractionEffectPermissionValueV1 {
    allow: u64,
    deny: u64,
}

impl InteractionEffectPermissionValueV1 {
    pub fn new(allow: u64, deny: u64) -> Result<Self, InteractionEffectIdentityErrorV1> {
        if allow & deny != 0 {
            return Err(InteractionEffectIdentityErrorV1::PermissionOverlap);
        }
        Ok(Self { allow, deny })
    }

    pub fn allow(self) -> u64 {
        self.allow
    }

    pub fn deny(self) -> u64 {
        self.deny
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum InteractionEffectPermissionStateV1 {
    Absent,
    Present(InteractionEffectPermissionValueV1),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InteractionEffectInstanceTargetV1 {
    guild_id: InteractionEffectGuildIdV1,
    instance_identity_digest: InteractionEffectOpaqueIdentityDigestV1,
}

impl InteractionEffectInstanceTargetV1 {
    pub fn new(
        guild_id: InteractionEffectGuildIdV1,
        instance_identity_digest: InteractionEffectOpaqueIdentityDigestV1,
    ) -> Self {
        Self {
            guild_id,
            instance_identity_digest,
        }
    }

    pub fn guild_id(&self) -> InteractionEffectGuildIdV1 {
        self.guild_id
    }

    pub fn instance_identity_digest(&self) -> &InteractionEffectOpaqueIdentityDigestV1 {
        &self.instance_identity_digest
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum InteractionEffectInstanceStateV1 {
    Absent,
    Present {
        manifest_digest: InteractionEffectPayloadDigestV1,
    },
}

impl InteractionEffectInstanceStateV1 {
    pub fn is_present(&self) -> bool {
        matches!(self, Self::Present { .. })
    }

    pub fn manifest_digest(&self) -> Option<&InteractionEffectPayloadDigestV1> {
        match self {
            Self::Absent => None,
            Self::Present { manifest_digest } => Some(manifest_digest),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum InteractionEffectTargetV1 {
    CreateRole {
        guild_id: InteractionEffectGuildIdV1,
    },
    CreateChannel {
        guild_id: InteractionEffectGuildIdV1,
    },
    GrantRole {
        target: InteractionEffectRoleMembershipTargetV1,
    },
    UpsertOverwrite {
        target: InteractionEffectPermissionTargetV1,
        desired: InteractionEffectPermissionValueV1,
    },
    PostPanel {
        guild_id: InteractionEffectGuildIdV1,
        channel_id: InteractionEffectChannelIdV1,
        payload_digest: InteractionEffectPayloadDigestV1,
    },
    RegisterInstance {
        target: InteractionEffectInstanceTargetV1,
        kind: InstanceKind,
        manifest_digest: InteractionEffectPayloadDigestV1,
    },
    TeardownInstance {
        target: InteractionEffectInstanceTargetV1,
    },
    EditResponse {
        receipt_identity: InteractionReceiptIdentityV1,
        payload_digest: InteractionEffectPayloadDigestV1,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum InteractionEffectRecoveryTargetV1 {
    CreateRole {
        guild_id: InteractionEffectGuildIdV1,
    },
    CreateChannel {
        guild_id: InteractionEffectGuildIdV1,
    },
    GrantRole {
        target: InteractionEffectRoleMembershipTargetV1,
    },
    UpsertOverwrite {
        target: InteractionEffectPermissionTargetV1,
        desired: InteractionEffectPermissionValueV1,
    },
    PostPanel {
        guild_id: InteractionEffectGuildIdV1,
        channel_id: InteractionEffectChannelIdV1,
        payload_digest: InteractionEffectPayloadDigestV1,
    },
    RegisterInstance {
        target: InteractionEffectInstanceTargetV1,
        kind: InstanceKind,
        manifest_digest: InteractionEffectPayloadDigestV1,
    },
    TeardownInstance {
        target: InteractionEffectInstanceTargetV1,
    },
    EditResponse {
        receipt_identity: InteractionReceiptIdentityV1,
        payload_digest: InteractionEffectPayloadDigestV1,
    },
}

impl InteractionEffectRecoveryTargetV1 {
    pub fn kind(&self) -> InteractionEffectKindV1 {
        match self {
            Self::CreateRole { .. } => InteractionEffectKindV1::CreateRole,
            Self::CreateChannel { .. } => InteractionEffectKindV1::CreateChannel,
            Self::GrantRole { .. } => InteractionEffectKindV1::GrantRole,
            Self::UpsertOverwrite { .. } => InteractionEffectKindV1::UpsertOverwrite,
            Self::PostPanel { .. } => InteractionEffectKindV1::PostPanel,
            Self::RegisterInstance { .. } => InteractionEffectKindV1::RegisterInstance,
            Self::TeardownInstance { .. } => InteractionEffectKindV1::TeardownInstance,
            Self::EditResponse { .. } => InteractionEffectKindV1::EditResponse,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InteractionEffectRecoveryBindingV1 {
    target: InteractionEffectRecoveryTargetV1,
    preimage: InteractionEffectPreimageV1,
    planned_identity_digest: InteractionEffectPlannedIdentityDigestV1,
    resolved_identity_digest: InteractionEffectIdentityDigestV1,
    expected_postimage_digest: InteractionEffectExpectedPostimageDigestV1,
    correlation: InteractionEffectCorrelationV1,
    preimage_digest: InteractionEffectPreimageDigestV1,
}

impl InteractionEffectRecoveryBindingV1 {
    pub fn new(
        target: InteractionEffectRecoveryTargetV1,
        preimage: InteractionEffectPreimageV1,
        planned_identity_digest: InteractionEffectPlannedIdentityDigestV1,
        resolved_identity_digest: InteractionEffectIdentityDigestV1,
        expected_postimage_digest: InteractionEffectExpectedPostimageDigestV1,
        correlation: InteractionEffectCorrelationV1,
    ) -> Result<Self, InteractionEffectIdentityErrorV1> {
        validate_recovery_preimage_v1(&target, &preimage)?;
        let expected_correlation =
            crate::effect_digest::build_interaction_effect_recovery_correlation_v1(
                &planned_identity_digest,
                correlation_class_v1(target.kind()),
            );
        if correlation != expected_correlation {
            return Err(InteractionEffectIdentityErrorV1::RecoveryCorrelation);
        }
        let preimage_digest =
            crate::effect_digest::build_interaction_effect_preimage_digest_v1(&preimage);
        Ok(Self {
            target,
            preimage,
            planned_identity_digest,
            resolved_identity_digest,
            expected_postimage_digest,
            correlation,
            preimage_digest,
        })
    }

    pub fn kind(&self) -> InteractionEffectKindV1 {
        self.target.kind()
    }

    pub fn target(&self) -> &InteractionEffectRecoveryTargetV1 {
        &self.target
    }

    pub fn preimage(&self) -> &InteractionEffectPreimageV1 {
        &self.preimage
    }

    pub fn planned_identity_digest(&self) -> &InteractionEffectPlannedIdentityDigestV1 {
        &self.planned_identity_digest
    }

    pub fn resolved_identity_digest(&self) -> &InteractionEffectIdentityDigestV1 {
        &self.resolved_identity_digest
    }

    pub fn expected_postimage_digest(&self) -> &InteractionEffectExpectedPostimageDigestV1 {
        &self.expected_postimage_digest
    }

    pub fn correlation(&self) -> &InteractionEffectCorrelationV1 {
        &self.correlation
    }

    pub fn preimage_digest(&self) -> &InteractionEffectPreimageDigestV1 {
        &self.preimage_digest
    }

    pub fn output_class(&self) -> InteractionEffectOutputClassV1 {
        output_class_v1(self.kind())
    }

    pub fn compensation_class(&self) -> InteractionEffectCompensationClassV1 {
        compensation_class_v1(self.kind())
    }

    pub fn validate_observed_output(
        &self,
        output: &InteractionEffectObservedOutputV1,
    ) -> Result<(), InteractionEffectIdentityErrorV1> {
        if recovery_observed_output_matches_v1(&self.target, output) {
            Ok(())
        } else {
            Err(InteractionEffectIdentityErrorV1::ObservedOutput)
        }
    }

    pub fn verify_resolved_definition(
        &self,
        definition: &InteractionEffectDefinitionV1,
    ) -> Result<(), InteractionEffectIdentityErrorV1> {
        if self.planned_identity_digest != *definition.planned_identity_digest()
            || self.resolved_identity_digest
                != crate::effect_digest::build_interaction_effect_identity_digest_v1(definition)
            || !recovery_target_matches_resolved_v1(&self.target, definition.target())
            || self.preimage != *definition.preimage()
        {
            return Err(InteractionEffectIdentityErrorV1::RecoveryIdentity);
        }
        Ok(())
    }
}

impl InteractionEffectTargetV1 {
    pub fn kind(&self) -> InteractionEffectKindV1 {
        match self {
            Self::CreateRole { .. } => InteractionEffectKindV1::CreateRole,
            Self::CreateChannel { .. } => InteractionEffectKindV1::CreateChannel,
            Self::GrantRole { .. } => InteractionEffectKindV1::GrantRole,
            Self::UpsertOverwrite { .. } => InteractionEffectKindV1::UpsertOverwrite,
            Self::PostPanel { .. } => InteractionEffectKindV1::PostPanel,
            Self::RegisterInstance { .. } => InteractionEffectKindV1::RegisterInstance,
            Self::TeardownInstance { .. } => InteractionEffectKindV1::TeardownInstance,
            Self::EditResponse { .. } => InteractionEffectKindV1::EditResponse,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum InteractionEffectPreimageV1 {
    None,
    RoleMembership {
        target: InteractionEffectRoleMembershipTargetV1,
        present: bool,
    },
    PermissionOverwrite {
        target: InteractionEffectPermissionTargetV1,
        before: InteractionEffectPermissionStateV1,
    },
    InstanceRegistration {
        target: InteractionEffectInstanceTargetV1,
        before: InteractionEffectInstanceStateV1,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InteractionEffectPlannedDependencyV1 {
    action_index: InteractionEffectActionIndexV1,
    producer_identity_digest: InteractionEffectPlannedIdentityDigestV1,
    output_class: InteractionEffectOutputClassV1,
}

impl InteractionEffectPlannedDependencyV1 {
    pub fn new(
        action_index: InteractionEffectActionIndexV1,
        producer_identity_digest: InteractionEffectPlannedIdentityDigestV1,
        output_class: InteractionEffectOutputClassV1,
    ) -> Self {
        Self {
            action_index,
            producer_identity_digest,
            output_class,
        }
    }

    pub fn action_index(&self) -> InteractionEffectActionIndexV1 {
        self.action_index
    }

    pub fn producer_identity_digest(&self) -> &InteractionEffectPlannedIdentityDigestV1 {
        &self.producer_identity_digest
    }

    pub fn output_class(&self) -> InteractionEffectOutputClassV1 {
        self.output_class
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InteractionEffectDependencyResolutionV1 {
    planned: InteractionEffectPlannedDependencyV1,
    output_digest: InteractionEffectOutputDigestV1,
    output: InteractionEffectObservedOutputV1,
}

impl InteractionEffectDependencyResolutionV1 {
    pub fn new(
        planned: InteractionEffectPlannedDependencyV1,
        output: InteractionEffectObservedOutputV1,
    ) -> Result<Self, InteractionEffectIdentityErrorV1> {
        if planned.output_class() != output.class() {
            return Err(InteractionEffectIdentityErrorV1::DependencyResolutionMismatch);
        }
        let output_digest =
            crate::effect_digest::build_interaction_effect_output_digest_v1(&output);
        Ok(Self {
            planned,
            output_digest,
            output,
        })
    }

    pub fn planned(&self) -> &InteractionEffectPlannedDependencyV1 {
        &self.planned
    }

    pub fn output_digest(&self) -> &InteractionEffectOutputDigestV1 {
        &self.output_digest
    }

    pub fn output(&self) -> &InteractionEffectObservedOutputV1 {
        &self.output
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InteractionEffectDependencyV1 {
    planned: InteractionEffectPlannedDependencyV1,
    output_digest: InteractionEffectOutputDigestV1,
}

impl InteractionEffectDependencyV1 {
    pub fn new(
        action_index: InteractionEffectActionIndexV1,
        producer_identity_digest: InteractionEffectPlannedIdentityDigestV1,
        output_class: InteractionEffectOutputClassV1,
        output_digest: InteractionEffectOutputDigestV1,
    ) -> Self {
        Self {
            planned: InteractionEffectPlannedDependencyV1::new(
                action_index,
                producer_identity_digest,
                output_class,
            ),
            output_digest,
        }
    }

    pub fn from_resolution(resolution: InteractionEffectDependencyResolutionV1) -> Self {
        Self {
            planned: resolution.planned,
            output_digest: resolution.output_digest,
        }
    }

    pub fn action_index(&self) -> InteractionEffectActionIndexV1 {
        self.planned.action_index()
    }

    pub fn producer_identity_digest(&self) -> &InteractionEffectPlannedIdentityDigestV1 {
        self.planned.producer_identity_digest()
    }

    pub fn output_class(&self) -> InteractionEffectOutputClassV1 {
        self.planned.output_class()
    }

    pub fn output_digest(&self) -> &InteractionEffectOutputDigestV1 {
        &self.output_digest
    }

    pub fn planned(&self) -> &InteractionEffectPlannedDependencyV1 {
        &self.planned
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InteractionEffectActionIdentityV1 {
    receipt_identity: InteractionReceiptIdentityV1,
    action_plan_digest: InteractionActionPlanDigestV1,
    preflight_certificate_digest: InteractionPreflightCertificateDigestV1,
    action_index: InteractionEffectActionIndexV1,
    kind: InteractionEffectKindV1,
    action_digest: InteractionEffectActionDigestV1,
    input_digest: InteractionEffectInputDigestV1,
}

impl InteractionEffectActionIdentityV1 {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        receipt_identity: InteractionReceiptIdentityV1,
        action_plan_digest: InteractionActionPlanDigestV1,
        preflight_certificate_digest: InteractionPreflightCertificateDigestV1,
        action_index: InteractionEffectActionIndexV1,
        kind: InteractionEffectKindV1,
        action_digest: InteractionEffectActionDigestV1,
        input_digest: InteractionEffectInputDigestV1,
    ) -> Self {
        Self {
            receipt_identity,
            action_plan_digest,
            preflight_certificate_digest,
            action_index,
            kind,
            action_digest,
            input_digest,
        }
    }

    pub fn receipt_identity(&self) -> InteractionReceiptIdentityV1 {
        self.receipt_identity
    }

    pub fn action_plan_digest(&self) -> &InteractionActionPlanDigestV1 {
        &self.action_plan_digest
    }

    pub fn preflight_certificate_digest(&self) -> &InteractionPreflightCertificateDigestV1 {
        &self.preflight_certificate_digest
    }

    pub fn action_index(&self) -> InteractionEffectActionIndexV1 {
        self.action_index
    }

    pub fn kind(&self) -> InteractionEffectKindV1 {
        self.kind
    }

    pub fn action_digest(&self) -> &InteractionEffectActionDigestV1 {
        &self.action_digest
    }

    pub fn input_digest(&self) -> &InteractionEffectInputDigestV1 {
        &self.input_digest
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InteractionEffectDefinitionV1 {
    action: InteractionEffectActionIdentityV1,
    target: InteractionEffectTargetV1,
    preimage: InteractionEffectPreimageV1,
    dependencies: Vec<InteractionEffectDependencyV1>,
    planned_identity_digest: InteractionEffectPlannedIdentityDigestV1,
}

impl InteractionEffectDefinitionV1 {
    pub fn new(
        action: InteractionEffectActionIdentityV1,
        target: InteractionEffectTargetV1,
        preimage: InteractionEffectPreimageV1,
        dependencies: Vec<InteractionEffectDependencyV1>,
    ) -> Result<Self, InteractionEffectIdentityErrorV1> {
        if action.kind() != target.kind() {
            return Err(InteractionEffectIdentityErrorV1::TargetKind);
        }
        validate_preimage_v1(&target, &preimage)?;
        let planned = dependencies
            .iter()
            .map(InteractionEffectDependencyV1::planned)
            .cloned()
            .collect::<Vec<_>>();
        validate_planned_dependencies_v1(action.action_index(), &planned)?;
        let planned_identity_digest = legacy_planned_identity_digest_v1(&action);
        Ok(Self {
            action,
            target,
            preimage,
            dependencies,
            planned_identity_digest,
        })
    }

    pub(crate) fn from_materialized(
        action: InteractionEffectActionIdentityV1,
        target: InteractionEffectTargetV1,
        preimage: InteractionEffectPreimageV1,
        dependencies: Vec<InteractionEffectDependencyV1>,
        planned_identity_digest: InteractionEffectPlannedIdentityDigestV1,
    ) -> Result<Self, InteractionEffectIdentityErrorV1> {
        let mut definition = Self::new(action, target, preimage, dependencies)?;
        definition.planned_identity_digest = planned_identity_digest;
        Ok(definition)
    }

    pub fn action(&self) -> &InteractionEffectActionIdentityV1 {
        &self.action
    }

    pub fn target(&self) -> &InteractionEffectTargetV1 {
        &self.target
    }

    pub fn preimage(&self) -> &InteractionEffectPreimageV1 {
        &self.preimage
    }

    pub fn dependencies(&self) -> &[InteractionEffectDependencyV1] {
        &self.dependencies
    }

    pub fn planned_identity_digest(&self) -> &InteractionEffectPlannedIdentityDigestV1 {
        &self.planned_identity_digest
    }

    pub fn output_class(&self) -> InteractionEffectOutputClassV1 {
        output_class_v1(self.action.kind())
    }

    pub fn correlation_class(&self) -> InteractionEffectCorrelationClassV1 {
        correlation_class_v1(self.action.kind())
    }

    pub fn compensation_class(&self) -> InteractionEffectCompensationClassV1 {
        compensation_class_v1(self.action.kind())
    }

    pub fn recovery_scope(&self) -> InteractionEffectRecoveryScopeV1 {
        recovery_scope_v1(self.action.kind())
    }

    pub fn validate_observed_output(
        &self,
        output: &InteractionEffectObservedOutputV1,
    ) -> Result<(), InteractionEffectIdentityErrorV1> {
        if observed_output_matches_v1(self.target(), output) {
            Ok(())
        } else {
            Err(InteractionEffectIdentityErrorV1::ObservedOutput)
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InteractionEffectCorrelationV1 {
    class: InteractionEffectCorrelationClassV1,
    marker_digest: crate::effect_digest::InteractionEffectCorrelationDigestV1,
    message_nonce: Option<NonZeroU64>,
}

impl InteractionEffectCorrelationV1 {
    pub(crate) fn new(
        class: InteractionEffectCorrelationClassV1,
        marker_digest: crate::effect_digest::InteractionEffectCorrelationDigestV1,
        message_nonce: Option<NonZeroU64>,
    ) -> Self {
        Self {
            class,
            marker_digest,
            message_nonce,
        }
    }

    pub fn class(&self) -> InteractionEffectCorrelationClassV1 {
        self.class
    }

    pub fn marker_digest(&self) -> &crate::effect_digest::InteractionEffectCorrelationDigestV1 {
        &self.marker_digest
    }

    pub fn message_nonce(&self) -> Option<NonZeroU64> {
        self.message_nonce
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum InteractionEffectObservedOutputV1 {
    CreatedRole {
        guild_id: InteractionEffectGuildIdV1,
        role_id: InteractionEffectRoleIdV1,
    },
    CreatedChannel {
        guild_id: InteractionEffectGuildIdV1,
        channel_id: InteractionEffectChannelIdV1,
    },
    RoleMembership {
        target: InteractionEffectRoleMembershipTargetV1,
        present: bool,
    },
    PermissionOverwrite {
        target: InteractionEffectPermissionTargetV1,
        state: InteractionEffectPermissionStateV1,
    },
    PostedMessage {
        guild_id: InteractionEffectGuildIdV1,
        channel_id: InteractionEffectChannelIdV1,
        message_id: InteractionEffectMessageIdV1,
        payload_digest: InteractionEffectPayloadDigestV1,
    },
    InstanceState {
        target: InteractionEffectInstanceTargetV1,
        state: InteractionEffectInstanceStateV1,
    },
    OriginalResponse {
        receipt_identity: InteractionReceiptIdentityV1,
        payload_digest: InteractionEffectPayloadDigestV1,
    },
}

impl InteractionEffectObservedOutputV1 {
    pub fn class(&self) -> InteractionEffectOutputClassV1 {
        match self {
            Self::CreatedRole { .. } => InteractionEffectOutputClassV1::CreatedRole,
            Self::CreatedChannel { .. } => InteractionEffectOutputClassV1::CreatedChannel,
            Self::RoleMembership { .. } => InteractionEffectOutputClassV1::RoleMembership,
            Self::PermissionOverwrite { .. } => InteractionEffectOutputClassV1::PermissionOverwrite,
            Self::PostedMessage { .. } => InteractionEffectOutputClassV1::PostedMessage,
            Self::InstanceState { .. } => InteractionEffectOutputClassV1::InstanceState,
            Self::OriginalResponse { .. } => InteractionEffectOutputClassV1::OriginalResponse,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InteractionEffectKnownFailureClassV1 {
    Rejected,
    Forbidden,
    NotFound,
    RateLimitedBeforeDispatch,
    Conflict,
    InvalidRequest,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct InteractionEffectKnownFailureV1 {
    class: InteractionEffectKnownFailureClassV1,
    http_status: Option<u16>,
}

impl InteractionEffectKnownFailureV1 {
    pub fn new(
        class: InteractionEffectKnownFailureClassV1,
        http_status: Option<u16>,
    ) -> Result<Self, InteractionEffectIdentityErrorV1> {
        if http_status.is_some_and(|status| !(400..=599).contains(&status)) {
            return Err(InteractionEffectIdentityErrorV1::HttpStatus);
        }
        Ok(Self { class, http_status })
    }

    pub fn class(self) -> InteractionEffectKnownFailureClassV1 {
        self.class
    }

    pub fn http_status(self) -> Option<u16> {
        self.http_status
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InteractionEffectIndeterminateClassV1 {
    DeadlineElapsed,
    ConnectionLost,
    Cancelled,
    MalformedResponse,
    PersistenceCommit,
    ProviderUnavailable,
    Unknown,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum InteractionEffectAttemptOutcomeV1 {
    KnownSucceeded(InteractionEffectObservedOutputV1),
    KnownFailed(InteractionEffectKnownFailureV1),
    Indeterminate(InteractionEffectIndeterminateClassV1),
}

impl InteractionEffectAttemptOutcomeV1 {
    pub fn known_succeeded(
        definition: &InteractionEffectDefinitionV1,
        output: InteractionEffectObservedOutputV1,
    ) -> Result<Self, InteractionEffectIdentityErrorV1> {
        definition.validate_observed_output(&output)?;
        Ok(Self::KnownSucceeded(output))
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InteractionEffectObservationEvidenceV1 {
    digest: InteractionEffectObservationEvidenceDigestV1,
    correlation_class: InteractionEffectCorrelationClassV1,
    exact_correlation_matches: u16,
    conflicting_matches: u16,
    target_identity_matches: bool,
    actor_identity_matches: bool,
    postimage_matches: bool,
}

impl InteractionEffectObservationEvidenceV1 {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        digest: InteractionEffectObservationEvidenceDigestV1,
        correlation_class: InteractionEffectCorrelationClassV1,
        exact_correlation_matches: u16,
        conflicting_matches: u16,
        target_identity_matches: bool,
        actor_identity_matches: bool,
        postimage_matches: bool,
    ) -> Self {
        Self {
            digest,
            correlation_class,
            exact_correlation_matches,
            conflicting_matches,
            target_identity_matches,
            actor_identity_matches,
            postimage_matches,
        }
    }

    pub fn digest(&self) -> &InteractionEffectObservationEvidenceDigestV1 {
        &self.digest
    }

    pub fn correlation_class(&self) -> InteractionEffectCorrelationClassV1 {
        self.correlation_class
    }

    pub fn exact_correlation_matches(&self) -> u16 {
        self.exact_correlation_matches
    }

    pub fn conflicting_matches(&self) -> u16 {
        self.conflicting_matches
    }

    pub fn target_identity_matches(&self) -> bool {
        self.target_identity_matches
    }

    pub fn actor_identity_matches(&self) -> bool {
        self.actor_identity_matches
    }

    pub fn postimage_matches(&self) -> bool {
        self.postimage_matches
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum InteractionEffectObservationOutcomeV1 {
    ExactMatch {
        output: InteractionEffectObservedOutputV1,
        evidence: InteractionEffectObservationEvidenceV1,
    },
    ExactAbsence {
        evidence: InteractionEffectObservationEvidenceV1,
    },
    Pending {
        evidence: InteractionEffectObservationEvidenceV1,
    },
    Conflict {
        evidence: InteractionEffectObservationEvidenceV1,
    },
    Unsupported {
        evidence: InteractionEffectObservationEvidenceV1,
    },
}

impl InteractionEffectObservationOutcomeV1 {
    pub fn evidence(&self) -> &InteractionEffectObservationEvidenceV1 {
        match self {
            Self::ExactMatch { evidence, .. }
            | Self::ExactAbsence { evidence }
            | Self::Pending { evidence }
            | Self::Conflict { evidence }
            | Self::Unsupported { evidence } => evidence,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum InteractionEffectCompensationOutcomeV1 {
    Succeeded {
        restored_preimage_digest: InteractionEffectPreimageDigestV1,
    },
    KnownFailed(InteractionEffectKnownFailureV1),
    Indeterminate(InteractionEffectIndeterminateClassV1),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum InteractionEffectCompensationObservationOutcomeV1 {
    Restored {
        restored_preimage_digest: InteractionEffectPreimageDigestV1,
        evidence: InteractionEffectObservationEvidenceV1,
    },
    Pending {
        evidence: InteractionEffectObservationEvidenceV1,
    },
    Conflict {
        evidence: InteractionEffectObservationEvidenceV1,
    },
    Unsupported {
        evidence: InteractionEffectObservationEvidenceV1,
    },
}

impl InteractionEffectCompensationObservationOutcomeV1 {
    pub fn evidence(&self) -> &InteractionEffectObservationEvidenceV1 {
        match self {
            Self::Restored { evidence, .. }
            | Self::Pending { evidence }
            | Self::Conflict { evidence }
            | Self::Unsupported { evidence } => evidence,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum InteractionEffectStateV1 {
    Planned,
    Intended,
    KnownSucceeded,
    KnownFailed,
    Indeterminate,
    Observing,
    ObservationPending,
    ReconciledSucceeded,
    CompensationIntended,
    Compensated,
    CompensationIndeterminate,
    CompensationObserving,
    CompensationObservationPending,
    RecoveryRequired,
}

impl InteractionEffectStateV1 {
    pub fn is_terminal(self) -> bool {
        matches!(
            self,
            Self::KnownFailed | Self::Compensated | Self::RecoveryRequired
        )
    }

    pub fn has_known_success(self) -> bool {
        matches!(self, Self::KnownSucceeded | Self::ReconciledSucceeded)
    }

    pub fn requires_observation(self) -> bool {
        matches!(
            self,
            Self::Intended | Self::Indeterminate | Self::ObservationPending | Self::Observing
        )
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InteractionEffectRecoveryRequiredReasonV1 {
    ObservationConflict,
    ObservationUnsupported,
    ObservationBudgetExhausted,
    CompensationKnownFailed,
    CompensationConflict,
    CompensationUnsupported,
    CompensationBudgetExhausted,
    NonCompensableSuccess,
    JournalConflict,
    AuthorityLost,
    ProtocolViolation,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum InteractionEffectTransitionV1<'a> {
    RecordIntent,
    RecordAttemptResult(&'a InteractionEffectAttemptOutcomeV1),
    BeginObservation,
    RecordObservation(&'a InteractionEffectObservationOutcomeV1),
    RecordCompensationIntent,
    RecordCompensationResult(&'a InteractionEffectCompensationOutcomeV1),
    BeginCompensationObservation,
    RecordCompensationObservation(&'a InteractionEffectCompensationObservationOutcomeV1),
    RequireRecovery(InteractionEffectRecoveryRequiredReasonV1),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum InteractionEffectTransitionErrorV1 {
    #[error("terminal interaction effect cannot transition")]
    Terminal,
    #[error("interaction effect transition is invalid")]
    InvalidTransition,
    #[error("interaction effect output does not match the bound definition")]
    OutputMismatch,
    #[error("interaction effect is not compensable")]
    NotCompensable,
}

pub fn validate_interaction_effect_transition_v1(
    definition: &InteractionEffectDefinitionV1,
    current: InteractionEffectStateV1,
    transition: InteractionEffectTransitionV1<'_>,
) -> Result<InteractionEffectStateV1, InteractionEffectTransitionErrorV1> {
    if current.is_terminal() {
        return Err(InteractionEffectTransitionErrorV1::Terminal);
    }
    let next = match (current, transition) {
        (InteractionEffectStateV1::Planned, InteractionEffectTransitionV1::RecordIntent) => {
            InteractionEffectStateV1::Intended
        }
        (
            InteractionEffectStateV1::Intended,
            InteractionEffectTransitionV1::RecordAttemptResult(outcome),
        ) => match outcome {
            InteractionEffectAttemptOutcomeV1::KnownSucceeded(output) => {
                definition
                    .validate_observed_output(output)
                    .map_err(|_| InteractionEffectTransitionErrorV1::OutputMismatch)?;
                InteractionEffectStateV1::KnownSucceeded
            }
            InteractionEffectAttemptOutcomeV1::KnownFailed(_) => {
                InteractionEffectStateV1::KnownFailed
            }
            InteractionEffectAttemptOutcomeV1::Indeterminate(_) => {
                InteractionEffectStateV1::Indeterminate
            }
        },
        (
            InteractionEffectStateV1::Intended
            | InteractionEffectStateV1::Indeterminate
            | InteractionEffectStateV1::ObservationPending
            | InteractionEffectStateV1::Observing,
            InteractionEffectTransitionV1::BeginObservation,
        ) => InteractionEffectStateV1::Observing,
        (
            InteractionEffectStateV1::Observing,
            InteractionEffectTransitionV1::RecordObservation(outcome),
        ) => match outcome {
            InteractionEffectObservationOutcomeV1::ExactMatch { output, .. } => {
                definition
                    .validate_observed_output(output)
                    .map_err(|_| InteractionEffectTransitionErrorV1::OutputMismatch)?;
                InteractionEffectStateV1::ReconciledSucceeded
            }
            InteractionEffectObservationOutcomeV1::ExactAbsence { .. } => {
                InteractionEffectStateV1::KnownFailed
            }
            InteractionEffectObservationOutcomeV1::Pending { .. } => {
                InteractionEffectStateV1::ObservationPending
            }
            InteractionEffectObservationOutcomeV1::Conflict { .. }
            | InteractionEffectObservationOutcomeV1::Unsupported { .. } => {
                InteractionEffectStateV1::RecoveryRequired
            }
        },
        (
            InteractionEffectStateV1::KnownSucceeded
            | InteractionEffectStateV1::ReconciledSucceeded,
            InteractionEffectTransitionV1::RecordCompensationIntent,
        ) => {
            if definition.compensation_class()
                == InteractionEffectCompensationClassV1::NotCompensable
            {
                return Err(InteractionEffectTransitionErrorV1::NotCompensable);
            }
            InteractionEffectStateV1::CompensationIntended
        }
        (
            InteractionEffectStateV1::CompensationIntended,
            InteractionEffectTransitionV1::RecordCompensationResult(outcome),
        ) => match outcome {
            InteractionEffectCompensationOutcomeV1::Succeeded { .. } => {
                InteractionEffectStateV1::Compensated
            }
            InteractionEffectCompensationOutcomeV1::KnownFailed(_) => {
                InteractionEffectStateV1::RecoveryRequired
            }
            InteractionEffectCompensationOutcomeV1::Indeterminate(_) => {
                InteractionEffectStateV1::CompensationIndeterminate
            }
        },
        (
            InteractionEffectStateV1::CompensationIntended
            | InteractionEffectStateV1::CompensationIndeterminate
            | InteractionEffectStateV1::CompensationObserving
            | InteractionEffectStateV1::CompensationObservationPending,
            InteractionEffectTransitionV1::BeginCompensationObservation,
        ) => InteractionEffectStateV1::CompensationObserving,
        (
            InteractionEffectStateV1::CompensationObserving,
            InteractionEffectTransitionV1::RecordCompensationObservation(outcome),
        ) => match outcome {
            InteractionEffectCompensationObservationOutcomeV1::Restored { .. } => {
                InteractionEffectStateV1::Compensated
            }
            InteractionEffectCompensationObservationOutcomeV1::Pending { .. } => {
                InteractionEffectStateV1::CompensationObservationPending
            }
            InteractionEffectCompensationObservationOutcomeV1::Conflict { .. }
            | InteractionEffectCompensationObservationOutcomeV1::Unsupported { .. } => {
                InteractionEffectStateV1::RecoveryRequired
            }
        },
        (_, InteractionEffectTransitionV1::RequireRecovery(_)) => {
            InteractionEffectStateV1::RecoveryRequired
        }
        _ => return Err(InteractionEffectTransitionErrorV1::InvalidTransition),
    };
    Ok(next)
}

fn output_class_v1(kind: InteractionEffectKindV1) -> InteractionEffectOutputClassV1 {
    match kind {
        InteractionEffectKindV1::CreateRole => InteractionEffectOutputClassV1::CreatedRole,
        InteractionEffectKindV1::CreateChannel => InteractionEffectOutputClassV1::CreatedChannel,
        InteractionEffectKindV1::GrantRole => InteractionEffectOutputClassV1::RoleMembership,
        InteractionEffectKindV1::UpsertOverwrite => {
            InteractionEffectOutputClassV1::PermissionOverwrite
        }
        InteractionEffectKindV1::PostPanel => InteractionEffectOutputClassV1::PostedMessage,
        InteractionEffectKindV1::RegisterInstance | InteractionEffectKindV1::TeardownInstance => {
            InteractionEffectOutputClassV1::InstanceState
        }
        InteractionEffectKindV1::EditResponse => InteractionEffectOutputClassV1::OriginalResponse,
    }
}

fn correlation_class_v1(kind: InteractionEffectKindV1) -> InteractionEffectCorrelationClassV1 {
    match kind {
        InteractionEffectKindV1::CreateRole
        | InteractionEffectKindV1::CreateChannel
        | InteractionEffectKindV1::GrantRole
        | InteractionEffectKindV1::UpsertOverwrite => {
            InteractionEffectCorrelationClassV1::AuditLogReason
        }
        InteractionEffectKindV1::PostPanel => InteractionEffectCorrelationClassV1::Unsupported,
        InteractionEffectKindV1::RegisterInstance | InteractionEffectKindV1::TeardownInstance => {
            InteractionEffectCorrelationClassV1::InternalIdempotencyKey
        }
        InteractionEffectKindV1::EditResponse => {
            InteractionEffectCorrelationClassV1::InteractionReceipt
        }
    }
}

fn legacy_planned_identity_digest_v1(
    action: &InteractionEffectActionIdentityV1,
) -> InteractionEffectPlannedIdentityDigestV1 {
    let mut canonical = Vec::with_capacity(320);
    canonical.extend_from_slice(b"starring.runtime.interaction.effect.legacy_plan.v1\0");
    canonical.extend_from_slice(
        &action
            .receipt_identity()
            .application_id()
            .get()
            .to_be_bytes(),
    );
    canonical.extend_from_slice(
        &action
            .receipt_identity()
            .interaction_id()
            .get()
            .to_be_bytes(),
    );
    canonical.extend_from_slice(action.action_plan_digest().as_str().as_bytes());
    canonical.extend_from_slice(action.preflight_certificate_digest().as_str().as_bytes());
    canonical.extend_from_slice(&action.action_index().get().to_be_bytes());
    canonical.extend_from_slice(action.kind().code().as_bytes());
    canonical.extend_from_slice(action.action_digest().as_str().as_bytes());
    canonical.extend_from_slice(action.input_digest().as_str().as_bytes());
    InteractionEffectPlannedIdentityDigestV1::from_canonical_bytes(&canonical)
}

fn compensation_class_v1(kind: InteractionEffectKindV1) -> InteractionEffectCompensationClassV1 {
    match kind {
        InteractionEffectKindV1::CreateRole => {
            InteractionEffectCompensationClassV1::DeleteCreatedRole
        }
        InteractionEffectKindV1::CreateChannel => {
            InteractionEffectCompensationClassV1::DeleteCreatedChannel
        }
        InteractionEffectKindV1::GrantRole => {
            InteractionEffectCompensationClassV1::RestoreRoleMembership
        }
        InteractionEffectKindV1::UpsertOverwrite => {
            InteractionEffectCompensationClassV1::RestorePermissionOverwrite
        }
        InteractionEffectKindV1::PostPanel => {
            InteractionEffectCompensationClassV1::DeletePostedMessage
        }
        InteractionEffectKindV1::RegisterInstance => {
            InteractionEffectCompensationClassV1::RestoreInstanceRegistration
        }
        InteractionEffectKindV1::TeardownInstance | InteractionEffectKindV1::EditResponse => {
            InteractionEffectCompensationClassV1::NotCompensable
        }
    }
}

fn recovery_scope_v1(kind: InteractionEffectKindV1) -> InteractionEffectRecoveryScopeV1 {
    match kind {
        InteractionEffectKindV1::EditResponse => InteractionEffectRecoveryScopeV1::ResponseTail,
        InteractionEffectKindV1::CreateRole
        | InteractionEffectKindV1::CreateChannel
        | InteractionEffectKindV1::GrantRole
        | InteractionEffectKindV1::UpsertOverwrite
        | InteractionEffectKindV1::PostPanel
        | InteractionEffectKindV1::RegisterInstance
        | InteractionEffectKindV1::TeardownInstance => {
            InteractionEffectRecoveryScopeV1::MutableProvisioning
        }
    }
}

fn validate_preimage_v1(
    target: &InteractionEffectTargetV1,
    preimage: &InteractionEffectPreimageV1,
) -> Result<(), InteractionEffectIdentityErrorV1> {
    let matches = match (target, preimage) {
        (
            InteractionEffectTargetV1::CreateRole { .. }
            | InteractionEffectTargetV1::CreateChannel { .. }
            | InteractionEffectTargetV1::PostPanel { .. }
            | InteractionEffectTargetV1::EditResponse { .. },
            InteractionEffectPreimageV1::None,
        ) => true,
        (
            InteractionEffectTargetV1::GrantRole { target },
            InteractionEffectPreimageV1::RoleMembership {
                target: preimage_target,
                ..
            },
        ) => target == preimage_target,
        (
            InteractionEffectTargetV1::UpsertOverwrite { target, .. },
            InteractionEffectPreimageV1::PermissionOverwrite {
                target: preimage_target,
                ..
            },
        ) => target == preimage_target,
        (
            InteractionEffectTargetV1::RegisterInstance { target, .. }
            | InteractionEffectTargetV1::TeardownInstance { target },
            InteractionEffectPreimageV1::InstanceRegistration {
                target: preimage_target,
                ..
            },
        ) => target == preimage_target,
        _ => false,
    };
    if matches {
        Ok(())
    } else {
        Err(InteractionEffectIdentityErrorV1::Preimage)
    }
}

fn validate_recovery_preimage_v1(
    target: &InteractionEffectRecoveryTargetV1,
    preimage: &InteractionEffectPreimageV1,
) -> Result<(), InteractionEffectIdentityErrorV1> {
    let matches = match (target, preimage) {
        (
            InteractionEffectRecoveryTargetV1::CreateRole { .. }
            | InteractionEffectRecoveryTargetV1::CreateChannel { .. }
            | InteractionEffectRecoveryTargetV1::PostPanel { .. }
            | InteractionEffectRecoveryTargetV1::EditResponse { .. },
            InteractionEffectPreimageV1::None,
        ) => true,
        (
            InteractionEffectRecoveryTargetV1::GrantRole { target },
            InteractionEffectPreimageV1::RoleMembership {
                target: preimage_target,
                ..
            },
        ) => target == preimage_target,
        (
            InteractionEffectRecoveryTargetV1::UpsertOverwrite { target, .. },
            InteractionEffectPreimageV1::PermissionOverwrite {
                target: preimage_target,
                ..
            },
        ) => target == preimage_target,
        (
            InteractionEffectRecoveryTargetV1::RegisterInstance { target, .. }
            | InteractionEffectRecoveryTargetV1::TeardownInstance { target },
            InteractionEffectPreimageV1::InstanceRegistration {
                target: preimage_target,
                ..
            },
        ) => target == preimage_target,
        _ => false,
    };
    if matches {
        Ok(())
    } else {
        Err(InteractionEffectIdentityErrorV1::Preimage)
    }
}

fn recovery_target_matches_resolved_v1(
    recovery: &InteractionEffectRecoveryTargetV1,
    resolved: &InteractionEffectTargetV1,
) -> bool {
    match (recovery, resolved) {
        (
            InteractionEffectRecoveryTargetV1::CreateRole { guild_id },
            InteractionEffectTargetV1::CreateRole {
                guild_id: resolved_guild,
            },
        )
        | (
            InteractionEffectRecoveryTargetV1::CreateChannel { guild_id },
            InteractionEffectTargetV1::CreateChannel {
                guild_id: resolved_guild,
            },
        ) => guild_id == resolved_guild,
        (
            InteractionEffectRecoveryTargetV1::GrantRole { target },
            InteractionEffectTargetV1::GrantRole {
                target: resolved_target,
            },
        ) => target == resolved_target,
        (
            InteractionEffectRecoveryTargetV1::UpsertOverwrite { target, desired },
            InteractionEffectTargetV1::UpsertOverwrite {
                target: resolved_target,
                desired: resolved_desired,
            },
        ) => target == resolved_target && desired == resolved_desired,
        (
            InteractionEffectRecoveryTargetV1::PostPanel {
                guild_id,
                channel_id,
                payload_digest,
            },
            InteractionEffectTargetV1::PostPanel {
                guild_id: resolved_guild,
                channel_id: resolved_channel,
                payload_digest: resolved_payload,
            },
        ) => {
            guild_id == resolved_guild
                && channel_id == resolved_channel
                && payload_digest == resolved_payload
        }
        (
            InteractionEffectRecoveryTargetV1::RegisterInstance {
                target,
                kind,
                manifest_digest,
            },
            InteractionEffectTargetV1::RegisterInstance {
                target: resolved_target,
                kind: resolved_kind,
                manifest_digest: resolved_manifest,
            },
        ) => {
            target == resolved_target
                && kind == resolved_kind
                && manifest_digest == resolved_manifest
        }
        (
            InteractionEffectRecoveryTargetV1::TeardownInstance { target },
            InteractionEffectTargetV1::TeardownInstance {
                target: resolved_target,
            },
        ) => target == resolved_target,
        (
            InteractionEffectRecoveryTargetV1::EditResponse {
                receipt_identity,
                payload_digest,
            },
            InteractionEffectTargetV1::EditResponse {
                receipt_identity: resolved_receipt,
                payload_digest: resolved_payload,
            },
        ) => receipt_identity == resolved_receipt && payload_digest == resolved_payload,
        _ => false,
    }
}

fn validate_planned_dependencies_v1(
    consumer: InteractionEffectActionIndexV1,
    dependencies: &[InteractionEffectPlannedDependencyV1],
) -> Result<(), InteractionEffectIdentityErrorV1> {
    if dependencies.len() > MAX_INTERACTION_EFFECT_DEPENDENCIES_V1 {
        return Err(InteractionEffectIdentityErrorV1::DependencyCount);
    }
    let mut previous = None;
    for dependency in dependencies {
        if dependency.action_index() >= consumer {
            return Err(InteractionEffectIdentityErrorV1::DependencyNotPrior);
        }
        if previous.is_some_and(|index| index >= dependency.action_index()) {
            return Err(InteractionEffectIdentityErrorV1::DependencyOrder);
        }
        previous = Some(dependency.action_index());
    }
    Ok(())
}

fn observed_output_matches_v1(
    target: &InteractionEffectTargetV1,
    output: &InteractionEffectObservedOutputV1,
) -> bool {
    match (target, output) {
        (
            InteractionEffectTargetV1::CreateRole { guild_id },
            InteractionEffectObservedOutputV1::CreatedRole {
                guild_id: observed_guild,
                ..
            },
        ) => guild_id == observed_guild,
        (
            InteractionEffectTargetV1::CreateChannel { guild_id },
            InteractionEffectObservedOutputV1::CreatedChannel {
                guild_id: observed_guild,
                ..
            },
        ) => guild_id == observed_guild,
        (
            InteractionEffectTargetV1::GrantRole { target },
            InteractionEffectObservedOutputV1::RoleMembership {
                target: observed_target,
                present,
            },
        ) => target == observed_target && *present,
        (
            InteractionEffectTargetV1::UpsertOverwrite { target, desired },
            InteractionEffectObservedOutputV1::PermissionOverwrite {
                target: observed_target,
                state: InteractionEffectPermissionStateV1::Present(observed),
            },
        ) => target == observed_target && desired == observed,
        (
            InteractionEffectTargetV1::PostPanel {
                guild_id,
                channel_id,
                payload_digest,
            },
            InteractionEffectObservedOutputV1::PostedMessage {
                guild_id: observed_guild,
                channel_id: observed_channel,
                payload_digest: observed_payload,
                ..
            },
        ) => {
            guild_id == observed_guild
                && channel_id == observed_channel
                && payload_digest == observed_payload
        }
        (
            InteractionEffectTargetV1::RegisterInstance {
                target,
                manifest_digest,
                ..
            },
            InteractionEffectObservedOutputV1::InstanceState {
                target: observed_target,
                state:
                    InteractionEffectInstanceStateV1::Present {
                        manifest_digest: observed_manifest,
                    },
            },
        ) => target == observed_target && manifest_digest == observed_manifest,
        (
            InteractionEffectTargetV1::TeardownInstance { target },
            InteractionEffectObservedOutputV1::InstanceState {
                target: observed_target,
                state: InteractionEffectInstanceStateV1::Absent,
            },
        ) => target == observed_target,
        (
            InteractionEffectTargetV1::EditResponse {
                receipt_identity,
                payload_digest,
            },
            InteractionEffectObservedOutputV1::OriginalResponse {
                receipt_identity: observed_receipt,
                payload_digest: observed_payload,
            },
        ) => receipt_identity == observed_receipt && payload_digest == observed_payload,
        _ => false,
    }
}

fn recovery_observed_output_matches_v1(
    target: &InteractionEffectRecoveryTargetV1,
    output: &InteractionEffectObservedOutputV1,
) -> bool {
    match (target, output) {
        (
            InteractionEffectRecoveryTargetV1::CreateRole { guild_id },
            InteractionEffectObservedOutputV1::CreatedRole {
                guild_id: observed_guild,
                ..
            },
        ) => guild_id == observed_guild,
        (
            InteractionEffectRecoveryTargetV1::CreateChannel { guild_id },
            InteractionEffectObservedOutputV1::CreatedChannel {
                guild_id: observed_guild,
                ..
            },
        ) => guild_id == observed_guild,
        (
            InteractionEffectRecoveryTargetV1::GrantRole { target },
            InteractionEffectObservedOutputV1::RoleMembership {
                target: observed_target,
                present,
            },
        ) => target == observed_target && *present,
        (
            InteractionEffectRecoveryTargetV1::UpsertOverwrite { target, desired },
            InteractionEffectObservedOutputV1::PermissionOverwrite {
                target: observed_target,
                state: InteractionEffectPermissionStateV1::Present(observed),
            },
        ) => target == observed_target && desired == observed,
        (
            InteractionEffectRecoveryTargetV1::RegisterInstance {
                target,
                manifest_digest,
                ..
            },
            InteractionEffectObservedOutputV1::InstanceState {
                target: observed_target,
                state:
                    InteractionEffectInstanceStateV1::Present {
                        manifest_digest: observed_manifest,
                    },
            },
        ) => target == observed_target && manifest_digest == observed_manifest,
        (
            InteractionEffectRecoveryTargetV1::TeardownInstance { target },
            InteractionEffectObservedOutputV1::InstanceState {
                target: observed_target,
                state: InteractionEffectInstanceStateV1::Absent,
            },
        ) => target == observed_target,
        (
            InteractionEffectRecoveryTargetV1::PostPanel {
                guild_id,
                channel_id,
                payload_digest,
            },
            InteractionEffectObservedOutputV1::PostedMessage {
                guild_id: observed_guild,
                channel_id: observed_channel,
                payload_digest: observed_payload,
                ..
            },
        ) => {
            guild_id == observed_guild
                && channel_id == observed_channel
                && payload_digest == observed_payload
        }
        (
            InteractionEffectRecoveryTargetV1::EditResponse {
                receipt_identity,
                payload_digest,
            },
            InteractionEffectObservedOutputV1::OriginalResponse {
                receipt_identity: observed_receipt,
                payload_digest: observed_payload,
            },
        ) => receipt_identity == observed_receipt && payload_digest == observed_payload,
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::effect_digest::{
        InteractionEffectActionDigestV1, InteractionEffectInputDigestV1,
        InteractionEffectPayloadDigestV1,
    };
    use crate::{
        DiscordApplicationIdV1, DiscordInteractionIdV1, InteractionActionPlanDigestV1,
        InteractionPreflightCertificateDigestV1,
    };

    fn digest(value: char) -> String {
        value.to_string().repeat(64)
    }

    fn receipt() -> InteractionReceiptIdentityV1 {
        InteractionReceiptIdentityV1::new(
            DiscordApplicationIdV1::new(1).unwrap(),
            DiscordInteractionIdV1::new(2).unwrap(),
        )
    }

    fn action(index: u16, kind: InteractionEffectKindV1) -> InteractionEffectActionIdentityV1 {
        InteractionEffectActionIdentityV1::new(
            receipt(),
            InteractionActionPlanDigestV1::parse(digest('a')).unwrap(),
            InteractionPreflightCertificateDigestV1::parse(digest('b')).unwrap(),
            InteractionEffectActionIndexV1::new(index).unwrap(),
            kind,
            InteractionEffectActionDigestV1::parse(digest('c')).unwrap(),
            InteractionEffectInputDigestV1::parse(digest('d')).unwrap(),
        )
    }

    fn role_definition() -> InteractionEffectDefinitionV1 {
        InteractionEffectDefinitionV1::new(
            action(0, InteractionEffectKindV1::CreateRole),
            InteractionEffectTargetV1::CreateRole {
                guild_id: InteractionEffectGuildIdV1::new(10).unwrap(),
            },
            InteractionEffectPreimageV1::None,
            Vec::new(),
        )
        .unwrap()
    }

    fn evidence() -> InteractionEffectObservationEvidenceV1 {
        InteractionEffectObservationEvidenceV1::new(
            InteractionEffectObservationEvidenceDigestV1::from_canonical_bytes(b"evidence"),
            InteractionEffectCorrelationClassV1::AuditLogReason,
            1,
            0,
            true,
            true,
            true,
        )
    }

    #[test]
    fn scalar_identities_and_bounds_reject_invalid_values() {
        assert_eq!(
            InteractionEffectRoleIdV1::new(0),
            Err(InteractionEffectIdentityErrorV1::ExternalIdentity)
        );
        assert_eq!(
            InteractionEffectActionIndexV1::new(MAX_INTERACTION_EFFECT_ACTIONS_V1),
            Err(InteractionEffectIdentityErrorV1::ActionIndex)
        );
        assert_eq!(
            InteractionEffectAttemptV1::new(0),
            Err(InteractionEffectIdentityErrorV1::Attempt)
        );
        assert_eq!(
            InteractionEffectAttemptV1::new(MAX_INTERACTION_EFFECT_ATTEMPTS_V1 + 1),
            Err(InteractionEffectIdentityErrorV1::Attempt)
        );
        assert_eq!(
            InteractionEffectPermissionValueV1::new(0b11, 0b10),
            Err(InteractionEffectIdentityErrorV1::PermissionOverlap)
        );
    }

    #[test]
    fn definition_derives_exact_contract_and_rejects_wrong_preimage() {
        let definition = role_definition();
        assert_eq!(
            definition.output_class(),
            InteractionEffectOutputClassV1::CreatedRole
        );
        assert_eq!(
            definition.correlation_class(),
            InteractionEffectCorrelationClassV1::AuditLogReason
        );
        assert_eq!(
            definition.compensation_class(),
            InteractionEffectCompensationClassV1::DeleteCreatedRole
        );
        let invalid = InteractionEffectDefinitionV1::new(
            action(0, InteractionEffectKindV1::CreateRole),
            InteractionEffectTargetV1::CreateRole {
                guild_id: InteractionEffectGuildIdV1::new(10).unwrap(),
            },
            InteractionEffectPreimageV1::RoleMembership {
                target: InteractionEffectRoleMembershipTargetV1::new(
                    InteractionEffectGuildIdV1::new(10).unwrap(),
                    InteractionEffectUserIdV1::new(11).unwrap(),
                    InteractionEffectRoleIdV1::new(12).unwrap(),
                ),
                present: false,
            },
            Vec::new(),
        );
        assert_eq!(invalid, Err(InteractionEffectIdentityErrorV1::Preimage));
    }

    #[test]
    fn dependencies_are_prior_unique_and_strictly_ordered() {
        let dependency = |index| {
            InteractionEffectDependencyV1::new(
                InteractionEffectActionIndexV1::new(index).unwrap(),
                InteractionEffectPlannedIdentityDigestV1::parse(digest('e')).unwrap(),
                InteractionEffectOutputClassV1::CreatedRole,
                InteractionEffectOutputDigestV1::parse(digest('f')).unwrap(),
            )
        };
        let target = InteractionEffectTargetV1::CreateChannel {
            guild_id: InteractionEffectGuildIdV1::new(10).unwrap(),
        };
        assert!(InteractionEffectDefinitionV1::new(
            action(2, InteractionEffectKindV1::CreateChannel),
            target.clone(),
            InteractionEffectPreimageV1::None,
            vec![dependency(0), dependency(1)],
        )
        .is_ok());
        assert_eq!(
            InteractionEffectDefinitionV1::new(
                action(2, InteractionEffectKindV1::CreateChannel),
                target.clone(),
                InteractionEffectPreimageV1::None,
                vec![dependency(1), dependency(0)],
            ),
            Err(InteractionEffectIdentityErrorV1::DependencyOrder)
        );
        assert_eq!(
            InteractionEffectDefinitionV1::new(
                action(2, InteractionEffectKindV1::CreateChannel),
                target,
                InteractionEffectPreimageV1::None,
                vec![dependency(2)],
            ),
            Err(InteractionEffectIdentityErrorV1::DependencyNotPrior)
        );
    }

    #[test]
    fn observed_output_must_match_typed_target() {
        let definition = role_definition();
        let valid = InteractionEffectObservedOutputV1::CreatedRole {
            guild_id: InteractionEffectGuildIdV1::new(10).unwrap(),
            role_id: InteractionEffectRoleIdV1::new(20).unwrap(),
        };
        let wrong_guild = InteractionEffectObservedOutputV1::CreatedRole {
            guild_id: InteractionEffectGuildIdV1::new(11).unwrap(),
            role_id: InteractionEffectRoleIdV1::new(20).unwrap(),
        };
        assert!(definition.validate_observed_output(&valid).is_ok());
        assert_eq!(
            definition.validate_observed_output(&wrong_guild),
            Err(InteractionEffectIdentityErrorV1::ObservedOutput)
        );
    }

    #[test]
    fn recovery_binding_verifies_the_persisted_resolved_identity() {
        let definition = role_definition();
        let planned_identity = definition.planned_identity_digest().clone();
        let correlation = crate::effect_digest::build_interaction_effect_recovery_correlation_v1(
            &planned_identity,
            InteractionEffectCorrelationClassV1::AuditLogReason,
        );
        let binding = InteractionEffectRecoveryBindingV1::new(
            InteractionEffectRecoveryTargetV1::CreateRole {
                guild_id: InteractionEffectGuildIdV1::new(10).unwrap(),
            },
            InteractionEffectPreimageV1::None,
            planned_identity.clone(),
            crate::effect_digest::build_interaction_effect_identity_digest_v1(&definition),
            crate::effect_digest::InteractionEffectExpectedPostimageDigestV1::parse(digest('e'))
                .unwrap(),
            correlation.clone(),
        )
        .unwrap();
        assert_eq!(binding.verify_resolved_definition(&definition), Ok(()));

        let tampered_identity = InteractionEffectRecoveryBindingV1::new(
            InteractionEffectRecoveryTargetV1::CreateRole {
                guild_id: InteractionEffectGuildIdV1::new(10).unwrap(),
            },
            InteractionEffectPreimageV1::None,
            planned_identity,
            crate::effect_digest::InteractionEffectIdentityDigestV1::parse(digest('f')).unwrap(),
            crate::effect_digest::InteractionEffectExpectedPostimageDigestV1::parse(digest('e'))
                .unwrap(),
            correlation,
        )
        .unwrap();
        assert_eq!(
            tampered_identity.verify_resolved_definition(&definition),
            Err(InteractionEffectIdentityErrorV1::RecoveryIdentity)
        );

        let tampered_target = InteractionEffectDefinitionV1::new(
            action(0, InteractionEffectKindV1::CreateRole),
            InteractionEffectTargetV1::CreateRole {
                guild_id: InteractionEffectGuildIdV1::new(11).unwrap(),
            },
            InteractionEffectPreimageV1::None,
            Vec::new(),
        )
        .unwrap();
        assert_eq!(
            binding.verify_resolved_definition(&tampered_target),
            Err(InteractionEffectIdentityErrorV1::RecoveryIdentity)
        );
    }

    #[test]
    fn successful_effect_can_only_compensate_after_durable_intent() {
        let definition = role_definition();
        let output = InteractionEffectObservedOutputV1::CreatedRole {
            guild_id: InteractionEffectGuildIdV1::new(10).unwrap(),
            role_id: InteractionEffectRoleIdV1::new(20).unwrap(),
        };
        let outcome =
            InteractionEffectAttemptOutcomeV1::known_succeeded(&definition, output).unwrap();
        let state = validate_interaction_effect_transition_v1(
            &definition,
            InteractionEffectStateV1::Planned,
            InteractionEffectTransitionV1::RecordIntent,
        )
        .unwrap();
        let state = validate_interaction_effect_transition_v1(
            &definition,
            state,
            InteractionEffectTransitionV1::RecordAttemptResult(&outcome),
        )
        .unwrap();
        assert_eq!(state, InteractionEffectStateV1::KnownSucceeded);
        let state = validate_interaction_effect_transition_v1(
            &definition,
            state,
            InteractionEffectTransitionV1::RecordCompensationIntent,
        )
        .unwrap();
        assert_eq!(state, InteractionEffectStateV1::CompensationIntended);
        let restored = InteractionEffectCompensationOutcomeV1::Succeeded {
            restored_preimage_digest: InteractionEffectPreimageDigestV1::from_canonical_bytes(
                b"none",
            ),
        };
        assert_eq!(
            validate_interaction_effect_transition_v1(
                &definition,
                state,
                InteractionEffectTransitionV1::RecordCompensationResult(&restored),
            )
            .unwrap(),
            InteractionEffectStateV1::Compensated
        );
    }

    #[test]
    fn indeterminate_effect_is_observed_without_replaying_intent() {
        let definition = role_definition();
        let indeterminate = InteractionEffectAttemptOutcomeV1::Indeterminate(
            InteractionEffectIndeterminateClassV1::ConnectionLost,
        );
        let mut state = validate_interaction_effect_transition_v1(
            &definition,
            InteractionEffectStateV1::Planned,
            InteractionEffectTransitionV1::RecordIntent,
        )
        .unwrap();
        state = validate_interaction_effect_transition_v1(
            &definition,
            state,
            InteractionEffectTransitionV1::RecordAttemptResult(&indeterminate),
        )
        .unwrap();
        assert_eq!(state, InteractionEffectStateV1::Indeterminate);
        assert_eq!(
            validate_interaction_effect_transition_v1(
                &definition,
                state,
                InteractionEffectTransitionV1::RecordIntent,
            ),
            Err(InteractionEffectTransitionErrorV1::InvalidTransition)
        );
        state = validate_interaction_effect_transition_v1(
            &definition,
            state,
            InteractionEffectTransitionV1::BeginObservation,
        )
        .unwrap();
        let pending = InteractionEffectObservationOutcomeV1::Pending {
            evidence: evidence(),
        };
        assert_eq!(
            validate_interaction_effect_transition_v1(
                &definition,
                state,
                InteractionEffectTransitionV1::RecordObservation(&pending),
            )
            .unwrap(),
            InteractionEffectStateV1::ObservationPending
        );
    }

    #[test]
    fn noncompensable_success_fails_closed() {
        let definition = InteractionEffectDefinitionV1::new(
            action(0, InteractionEffectKindV1::EditResponse),
            InteractionEffectTargetV1::EditResponse {
                receipt_identity: receipt(),
                payload_digest: InteractionEffectPayloadDigestV1::parse(digest('e')).unwrap(),
            },
            InteractionEffectPreimageV1::None,
            Vec::new(),
        )
        .unwrap();
        assert_eq!(
            validate_interaction_effect_transition_v1(
                &definition,
                InteractionEffectStateV1::KnownSucceeded,
                InteractionEffectTransitionV1::RecordCompensationIntent,
            ),
            Err(InteractionEffectTransitionErrorV1::NotCompensable)
        );
    }
}
