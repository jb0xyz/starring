use std::fmt::{Debug, Formatter};
use std::sync::Arc;

use automation_instance::{
    InstanceId, InstanceKind, InstanceRuleSetVersion, InstanceStatus, InstanceStoreError,
    InstanceTeardownStoreV1,
};
use automation_instance_teardown::{
    DeleterErrorKind, DurableInstanceTeardownServiceV1, ExactInstanceTeardownRequestV1, Teardown,
    TeardownError, TeardownOutcome,
};
use automation_ruleset::{RuleSetKey, RuleSetVersionId};
use automation_runtime_interaction::{
    InteractionEffectCompensationObservationOutcomeV1, InteractionEffectCompensationOutcomeV1,
    InteractionEffectCorrelationClassV1, InteractionEffectIndeterminateClassV1,
    InteractionEffectInstanceStateV1, InteractionEffectKnownFailureClassV1,
    InteractionEffectKnownFailureV1, InteractionEffectObservationEvidenceDigestV1,
    InteractionEffectObservationEvidenceV1, InteractionEffectObservationOutcomeV1,
    InteractionEffectObservedOutputV1, InteractionEffectOverwriteTargetV1,
    InteractionEffectPermissionStateV1, InteractionEffectPlannedInstanceTargetV1,
    InteractionEffectPreimageV1, InteractionEffectRecoveryBindingV1,
    InteractionEffectRecoveryTargetV1, InteractionInstanceManifestDigestV1, InteractionTokenV1,
};
use discord_model::{ChannelId, GuildId, MessageId, OverwriteTarget, RoleId, UserId};
use twilight_http::Client;

use crate::discord_effects::{
    DiscordEffectAttemptOutcomeV1, DiscordEffectCompensationObservationOutcomeV1,
    DiscordEffectCompensationObserverV1, DiscordEffectObservationEvidenceV1,
    DiscordEffectObservationOutcomeV1, DiscordEffectReadFailureV1,
    RecoverableDiscordMutationAdapterV1,
};
use crate::discord_original_response::{
    DiscordOriginalResponseObservationOutcomeV1, DiscordOriginalResponseObservationRequestV1,
    DiscordOriginalResponseObserverV1,
};
use crate::instance_deleter::OwnedTwilightInstanceDeleter;
use crate::mutation::TwilightMutationAdapter;

const DISCORD_EVIDENCE_DOMAIN_V1: &[u8] =
    b"starring.runtime.interaction.discord_recovery_evidence.v1\0";
const INTERNAL_EVIDENCE_DOMAIN_V1: &[u8] =
    b"starring.runtime.interaction.internal_recovery_evidence.v1\0";

#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum RuntimeInteractionEffectRecoveryDefinitionErrorV1 {
    #[error("runtime interaction effect recovery instance identity is missing")]
    MissingInstanceIdentity,
    #[error("runtime interaction effect recovery instance identity is unexpected")]
    UnexpectedInstanceIdentity,
    #[error("runtime interaction effect recovery instance identity does not match")]
    InstanceIdentityMismatch,
    #[error("runtime interaction effect recovery registration identity is missing")]
    MissingRegistrationIdentity,
    #[error("runtime interaction effect recovery registration identity is unexpected")]
    UnexpectedRegistrationIdentity,
    #[error("runtime interaction effect recovery registration identity does not match")]
    RegistrationIdentityMismatch,
}

#[derive(Clone, PartialEq, Eq)]
pub struct RuntimeInteractionEffectInstanceRegistrationIdentityV1 {
    ruleset_key: RuleSetKey,
    ruleset_version: InstanceRuleSetVersion,
    kind: InstanceKind,
    created_by: UserId,
    resolved_instance_manifest_digest: InteractionInstanceManifestDigestV1,
}

impl RuntimeInteractionEffectInstanceRegistrationIdentityV1 {
    pub fn new(
        ruleset_key: RuleSetKey,
        ruleset_version: InstanceRuleSetVersion,
        kind: InstanceKind,
        created_by: UserId,
        resolved_instance_manifest_digest: InteractionInstanceManifestDigestV1,
    ) -> Self {
        Self {
            ruleset_key,
            ruleset_version,
            kind,
            created_by,
            resolved_instance_manifest_digest,
        }
    }

    pub fn from_ruleset_version_v1(
        ruleset_key: RuleSetKey,
        ruleset_version: RuleSetVersionId,
        kind: InstanceKind,
        created_by: UserId,
        resolved_instance_manifest_digest: InteractionInstanceManifestDigestV1,
    ) -> Self {
        Self::new(
            ruleset_key,
            InstanceRuleSetVersion::new(ruleset_version.get())
                .expect("ruleset version identity is non-zero"),
            kind,
            created_by,
            resolved_instance_manifest_digest,
        )
    }

    pub fn ruleset_key(&self) -> &RuleSetKey {
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

    pub fn resolved_instance_manifest_digest(&self) -> &InteractionInstanceManifestDigestV1 {
        &self.resolved_instance_manifest_digest
    }
}

impl Debug for RuntimeInteractionEffectInstanceRegistrationIdentityV1 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimeInteractionEffectInstanceRegistrationIdentityV1(<redacted>)")
    }
}

#[derive(Clone, PartialEq, Eq)]
pub struct RuntimeInteractionEffectRecoveryDefinitionV1 {
    binding: InteractionEffectRecoveryBindingV1,
    instance_id: Option<InstanceId>,
    registration_identity: Option<RuntimeInteractionEffectInstanceRegistrationIdentityV1>,
    ruleset_key: RuleSetKey,
}

impl RuntimeInteractionEffectRecoveryDefinitionV1 {
    pub fn new(
        binding: InteractionEffectRecoveryBindingV1,
        instance_id: Option<InstanceId>,
        registration_identity: Option<RuntimeInteractionEffectInstanceRegistrationIdentityV1>,
        ruleset_key: RuleSetKey,
    ) -> Result<Self, RuntimeInteractionEffectRecoveryDefinitionErrorV1> {
        let target = match binding.target() {
            InteractionEffectRecoveryTargetV1::RegisterInstance { target, .. }
            | InteractionEffectRecoveryTargetV1::TeardownInstance { target } => Some(target),
            _ => None,
        };
        match (target, instance_id.as_ref()) {
            (Some(_), None) => {
                return Err(
                    RuntimeInteractionEffectRecoveryDefinitionErrorV1::MissingInstanceIdentity,
                )
            }
            (None, Some(_)) => {
                return Err(
                    RuntimeInteractionEffectRecoveryDefinitionErrorV1::UnexpectedInstanceIdentity,
                )
            }
            (Some(target), Some(instance_id)) => {
                let planned = InteractionEffectPlannedInstanceTargetV1::new(
                    target.guild_id(),
                    instance_id.clone(),
                );
                if planned.instance_identity_digest() != target.instance_identity_digest() {
                    return Err(
                        RuntimeInteractionEffectRecoveryDefinitionErrorV1::InstanceIdentityMismatch,
                    );
                }
            }
            (None, None) => {}
        }
        match (binding.target(), registration_identity.as_ref()) {
            (InteractionEffectRecoveryTargetV1::RegisterInstance { kind, .. }, Some(identity))
                if identity.ruleset_key() == &ruleset_key && identity.kind() == kind => {}
            (InteractionEffectRecoveryTargetV1::RegisterInstance { .. }, None) => {
                return Err(
                    RuntimeInteractionEffectRecoveryDefinitionErrorV1::MissingRegistrationIdentity,
                )
            }
            (InteractionEffectRecoveryTargetV1::RegisterInstance { .. }, Some(_)) => {
                return Err(
                    RuntimeInteractionEffectRecoveryDefinitionErrorV1::RegistrationIdentityMismatch,
                )
            }
            (_, Some(_)) => return Err(
                RuntimeInteractionEffectRecoveryDefinitionErrorV1::UnexpectedRegistrationIdentity,
            ),
            (_, None) => {}
        }
        Ok(Self {
            binding,
            instance_id,
            registration_identity,
            ruleset_key,
        })
    }

    pub fn binding(&self) -> &InteractionEffectRecoveryBindingV1 {
        &self.binding
    }

    pub fn instance_id(&self) -> Option<&InstanceId> {
        self.instance_id.as_ref()
    }

    pub fn registration_identity(
        &self,
    ) -> Option<&RuntimeInteractionEffectInstanceRegistrationIdentityV1> {
        self.registration_identity.as_ref()
    }

    pub fn instance_manifest_digest(&self) -> Option<&InteractionInstanceManifestDigestV1> {
        self.registration_identity
            .as_ref()
            .map(RuntimeInteractionEffectInstanceRegistrationIdentityV1::resolved_instance_manifest_digest)
    }

    pub fn ruleset_key(&self) -> &RuleSetKey {
        &self.ruleset_key
    }
}

impl Debug for RuntimeInteractionEffectRecoveryDefinitionV1 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("RuntimeInteractionEffectRecoveryDefinitionV1")
            .field("kind", &self.binding.kind())
            .field("payload", &"<redacted>")
            .finish()
    }
}

pub struct RuntimeInteractionEffectRecoveryObservationRequestV1 {
    definition: RuntimeInteractionEffectRecoveryDefinitionV1,
    interaction_token: Option<InteractionTokenV1>,
}

impl RuntimeInteractionEffectRecoveryObservationRequestV1 {
    pub fn new(
        definition: RuntimeInteractionEffectRecoveryDefinitionV1,
        interaction_token: Option<InteractionTokenV1>,
    ) -> Result<Self, RuntimeInteractionEffectRecoveryObservationRequestErrorV1> {
        let response = matches!(
            definition.binding().target(),
            InteractionEffectRecoveryTargetV1::EditResponse { .. }
        );
        if response != interaction_token.is_some() {
            return Err(RuntimeInteractionEffectRecoveryObservationRequestErrorV1::TokenShape);
        }
        Ok(Self {
            definition,
            interaction_token,
        })
    }

    pub fn definition(&self) -> &RuntimeInteractionEffectRecoveryDefinitionV1 {
        &self.definition
    }

    fn into_parts(
        self,
    ) -> (
        RuntimeInteractionEffectRecoveryDefinitionV1,
        Option<InteractionTokenV1>,
    ) {
        (self.definition, self.interaction_token)
    }
}

impl Debug for RuntimeInteractionEffectRecoveryObservationRequestV1 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimeInteractionEffectRecoveryObservationRequestV1(<redacted>)")
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum RuntimeInteractionEffectRecoveryObservationRequestErrorV1 {
    #[error("runtime interaction effect recovery token shape is invalid")]
    TokenShape,
}

#[derive(Clone, PartialEq, Eq)]
pub struct RuntimeInteractionEffectRecoveryCompensationRequestV1 {
    definition: RuntimeInteractionEffectRecoveryDefinitionV1,
    successful_output: InteractionEffectObservedOutputV1,
}

impl RuntimeInteractionEffectRecoveryCompensationRequestV1 {
    pub fn new(
        definition: RuntimeInteractionEffectRecoveryDefinitionV1,
        successful_output: InteractionEffectObservedOutputV1,
    ) -> Result<Self, RuntimeInteractionEffectRecoveryCompensationRequestErrorV1> {
        definition
            .binding()
            .validate_observed_output(&successful_output)
            .map_err(|_| RuntimeInteractionEffectRecoveryCompensationRequestErrorV1::Output)?;
        Ok(Self {
            definition,
            successful_output,
        })
    }

    pub fn definition(&self) -> &RuntimeInteractionEffectRecoveryDefinitionV1 {
        &self.definition
    }

    pub fn successful_output(&self) -> &InteractionEffectObservedOutputV1 {
        &self.successful_output
    }
}

impl Debug for RuntimeInteractionEffectRecoveryCompensationRequestV1 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimeInteractionEffectRecoveryCompensationRequestV1(<redacted>)")
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum RuntimeInteractionEffectRecoveryCompensationRequestErrorV1 {
    #[error("runtime interaction effect recovery success output does not match")]
    Output,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RuntimeInteractionEffectRecoveryRequiredV1 {
    DiscordReadRejected,
    ResponseTokenUnavailable,
    ObservationProtocol,
    CompensationConflict,
    CompensationUnsupported,
    NonCompensable,
    InternalConflict,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RuntimeInteractionEffectRecoveryRouteBlockV1 {
    DiscordForbidden,
    InternalAuthority,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RuntimeInteractionEffectInternalReadFailureV1 {
    TimedOut,
    Unavailable,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RuntimeInteractionEffectRecoveryDeferredV1 {
    Discord(DiscordEffectReadFailureV1),
    Internal(RuntimeInteractionEffectInternalReadFailureV1),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RuntimeInteractionEffectRecoveryObservationDispositionV1 {
    Reconcile(InteractionEffectObservationOutcomeV1),
    Deferred(RuntimeInteractionEffectRecoveryDeferredV1),
    RecoveryRequired(RuntimeInteractionEffectRecoveryRequiredV1),
    RouteBlocked(RuntimeInteractionEffectRecoveryRouteBlockV1),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RuntimeInteractionEffectRecoveryCompensationDispositionV1 {
    Finish(InteractionEffectCompensationOutcomeV1),
    Deferred(RuntimeInteractionEffectRecoveryDeferredV1),
    RecoveryRequired(RuntimeInteractionEffectRecoveryRequiredV1),
    RouteBlocked(RuntimeInteractionEffectRecoveryRouteBlockV1),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RuntimeInteractionEffectRecoveryCompensationObservationDispositionV1 {
    Reconcile(InteractionEffectCompensationObservationOutcomeV1),
    Deferred(RuntimeInteractionEffectRecoveryDeferredV1),
    RecoveryRequired(RuntimeInteractionEffectRecoveryRequiredV1),
    RouteBlocked(RuntimeInteractionEffectRecoveryRouteBlockV1),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RuntimeInteractionEffectDiscordObservedOutputV1 {
    CreatedRole(RoleId),
    CreatedChannel(ChannelId),
    RoleMembership(bool),
    PermissionOverwrite(InteractionEffectPermissionStateV1),
    PostedPanel(MessageId),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RuntimeInteractionEffectDiscordObservationRequestV1 {
    CreateRole {
        guild: GuildId,
        bot_user: UserId,
        expected_postimage:
            automation_runtime_interaction::InteractionEffectExpectedPostimageDigestV1,
        correlation: automation_runtime_interaction::InteractionEffectCorrelationV1,
    },
    CreateChannel {
        guild: GuildId,
        bot_user: UserId,
        expected_postimage:
            automation_runtime_interaction::InteractionEffectExpectedPostimageDigestV1,
        correlation: automation_runtime_interaction::InteractionEffectCorrelationV1,
    },
    GrantRole {
        guild: GuildId,
        bot_user: UserId,
        member: UserId,
        role: RoleId,
        expected_postimage:
            automation_runtime_interaction::InteractionEffectExpectedPostimageDigestV1,
        correlation: automation_runtime_interaction::InteractionEffectCorrelationV1,
    },
    UpsertOverwrite {
        guild: GuildId,
        bot_user: UserId,
        channel: ChannelId,
        target: OverwriteTarget,
        expected_postimage:
            automation_runtime_interaction::InteractionEffectExpectedPostimageDigestV1,
        correlation: automation_runtime_interaction::InteractionEffectCorrelationV1,
    },
    PostPanel {
        expected_postimage:
            automation_runtime_interaction::InteractionEffectExpectedPostimageDigestV1,
        correlation: automation_runtime_interaction::InteractionEffectCorrelationV1,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RuntimeInteractionEffectDiscordCompensationRequestV1 {
    CreateRole {
        guild: GuildId,
        bot_user: UserId,
        role: RoleId,
        expected_postimage:
            automation_runtime_interaction::InteractionEffectExpectedPostimageDigestV1,
        correlation: automation_runtime_interaction::InteractionEffectCorrelationV1,
    },
    CreateChannel {
        guild: GuildId,
        bot_user: UserId,
        channel: ChannelId,
        expected_postimage:
            automation_runtime_interaction::InteractionEffectExpectedPostimageDigestV1,
        correlation: automation_runtime_interaction::InteractionEffectCorrelationV1,
    },
    GrantRole {
        guild: GuildId,
        bot_user: UserId,
        member: UserId,
        role: RoleId,
        before_present: bool,
        expected_postimage:
            automation_runtime_interaction::InteractionEffectExpectedPostimageDigestV1,
        correlation: automation_runtime_interaction::InteractionEffectCorrelationV1,
    },
    UpsertOverwrite {
        guild: GuildId,
        bot_user: UserId,
        channel: ChannelId,
        target: OverwriteTarget,
        before: InteractionEffectPermissionStateV1,
        expected_postimage:
            automation_runtime_interaction::InteractionEffectExpectedPostimageDigestV1,
        correlation: automation_runtime_interaction::InteractionEffectCorrelationV1,
    },
    PostPanel {
        guild: GuildId,
        channel: ChannelId,
        message: MessageId,
        bot_user: UserId,
        expected_postimage:
            automation_runtime_interaction::InteractionEffectExpectedPostimageDigestV1,
        correlation: automation_runtime_interaction::InteractionEffectCorrelationV1,
    },
}

#[allow(async_fn_in_trait)]
pub trait RuntimeInteractionEffectDiscordRecoveryPortV1 {
    async fn observe_discord_effect_v1(
        &self,
        request: RuntimeInteractionEffectDiscordObservationRequestV1,
    ) -> DiscordEffectObservationOutcomeV1<RuntimeInteractionEffectDiscordObservedOutputV1>;

    async fn compensate_discord_effect_v1(
        &self,
        request: RuntimeInteractionEffectDiscordCompensationRequestV1,
    ) -> DiscordEffectAttemptOutcomeV1<()>;

    async fn observe_discord_compensation_v1(
        &self,
        request: RuntimeInteractionEffectDiscordCompensationRequestV1,
    ) -> DiscordEffectCompensationObservationOutcomeV1;
}

impl<T> RuntimeInteractionEffectDiscordRecoveryPortV1 for T
where
    T: RecoverableDiscordMutationAdapterV1 + DiscordEffectCompensationObserverV1,
{
    async fn observe_discord_effect_v1(
        &self,
        request: RuntimeInteractionEffectDiscordObservationRequestV1,
    ) -> DiscordEffectObservationOutcomeV1<RuntimeInteractionEffectDiscordObservedOutputV1> {
        match request {
            RuntimeInteractionEffectDiscordObservationRequestV1::CreateRole {
                guild,
                bot_user,
                expected_postimage,
                correlation,
            } => self
                .observe_created_role_effect_v1(guild, bot_user, &expected_postimage, &correlation)
                .await
                .map_output_v1(RuntimeInteractionEffectDiscordObservedOutputV1::CreatedRole),
            RuntimeInteractionEffectDiscordObservationRequestV1::CreateChannel {
                guild,
                bot_user,
                expected_postimage,
                correlation,
            } => self
                .observe_created_channel_effect_v1(
                    guild,
                    bot_user,
                    &expected_postimage,
                    &correlation,
                )
                .await
                .map_output_v1(RuntimeInteractionEffectDiscordObservedOutputV1::CreatedChannel),
            RuntimeInteractionEffectDiscordObservationRequestV1::GrantRole {
                guild,
                bot_user,
                member,
                role,
                expected_postimage,
                correlation,
            } => self
                .observe_role_membership_effect_v1(
                    guild,
                    bot_user,
                    member,
                    role,
                    &expected_postimage,
                    &correlation,
                )
                .await
                .map_output_v1(RuntimeInteractionEffectDiscordObservedOutputV1::RoleMembership),
            RuntimeInteractionEffectDiscordObservationRequestV1::UpsertOverwrite {
                guild,
                bot_user,
                channel,
                target,
                expected_postimage,
                correlation,
            } => {
                self.observe_overwrite_effect_v1(
                    guild,
                    bot_user,
                    channel,
                    target,
                    &expected_postimage,
                    &correlation,
                )
                .await
                .map_output_v1(RuntimeInteractionEffectDiscordObservedOutputV1::PermissionOverwrite)
            }
            RuntimeInteractionEffectDiscordObservationRequestV1::PostPanel {
                expected_postimage,
                correlation,
            } => self
                .observe_post_panel_effect_v1(&expected_postimage, &correlation)
                .await
                .map_output_v1(RuntimeInteractionEffectDiscordObservedOutputV1::PostedPanel),
        }
    }

    async fn compensate_discord_effect_v1(
        &self,
        request: RuntimeInteractionEffectDiscordCompensationRequestV1,
    ) -> DiscordEffectAttemptOutcomeV1<()> {
        match request {
            RuntimeInteractionEffectDiscordCompensationRequestV1::CreateRole {
                guild,
                bot_user,
                role,
                expected_postimage,
                correlation,
            } => {
                self.compensate_created_role_effect_v1(
                    guild,
                    bot_user,
                    role,
                    &expected_postimage,
                    &correlation,
                )
                .await
            }
            RuntimeInteractionEffectDiscordCompensationRequestV1::CreateChannel {
                guild,
                bot_user,
                channel,
                expected_postimage,
                correlation,
            } => {
                self.compensate_created_channel_effect_v1(
                    guild,
                    bot_user,
                    channel,
                    &expected_postimage,
                    &correlation,
                )
                .await
            }
            RuntimeInteractionEffectDiscordCompensationRequestV1::GrantRole {
                guild,
                bot_user,
                member,
                role,
                before_present,
                expected_postimage,
                correlation,
            } => {
                self.compensate_role_membership_effect_v1(
                    guild,
                    bot_user,
                    member,
                    role,
                    before_present,
                    &expected_postimage,
                    &correlation,
                )
                .await
            }
            RuntimeInteractionEffectDiscordCompensationRequestV1::UpsertOverwrite {
                guild,
                bot_user,
                channel,
                target,
                before,
                expected_postimage,
                correlation,
            } => {
                self.compensate_overwrite_effect_v1(
                    guild,
                    bot_user,
                    channel,
                    target,
                    &expected_postimage,
                    before,
                    &correlation,
                )
                .await
            }
            RuntimeInteractionEffectDiscordCompensationRequestV1::PostPanel {
                guild,
                channel,
                message,
                bot_user,
                expected_postimage,
                correlation,
            } => {
                self.compensate_posted_panel_effect_v1(
                    guild,
                    channel,
                    message,
                    bot_user,
                    &expected_postimage,
                    &correlation,
                )
                .await
            }
        }
    }

    async fn observe_discord_compensation_v1(
        &self,
        request: RuntimeInteractionEffectDiscordCompensationRequestV1,
    ) -> DiscordEffectCompensationObservationOutcomeV1 {
        match request {
            RuntimeInteractionEffectDiscordCompensationRequestV1::CreateRole {
                guild,
                bot_user,
                role,
                expected_postimage,
                correlation,
            } => {
                self.observe_compensated_created_role_effect_v1(
                    guild,
                    bot_user,
                    role,
                    &expected_postimage,
                    &correlation,
                )
                .await
            }
            RuntimeInteractionEffectDiscordCompensationRequestV1::CreateChannel {
                guild,
                bot_user,
                channel,
                expected_postimage,
                correlation,
            } => {
                self.observe_compensated_created_channel_effect_v1(
                    guild,
                    bot_user,
                    channel,
                    &expected_postimage,
                    &correlation,
                )
                .await
            }
            RuntimeInteractionEffectDiscordCompensationRequestV1::GrantRole {
                guild,
                bot_user,
                member,
                role,
                before_present,
                expected_postimage,
                correlation,
            } => {
                self.observe_compensated_role_membership_effect_v1(
                    guild,
                    bot_user,
                    member,
                    role,
                    before_present,
                    &expected_postimage,
                    &correlation,
                )
                .await
            }
            RuntimeInteractionEffectDiscordCompensationRequestV1::UpsertOverwrite {
                guild,
                bot_user,
                channel,
                target,
                before,
                expected_postimage,
                correlation,
            } => {
                self.observe_compensated_overwrite_effect_v1(
                    guild,
                    bot_user,
                    channel,
                    target,
                    before,
                    &expected_postimage,
                    &correlation,
                )
                .await
            }
            RuntimeInteractionEffectDiscordCompensationRequestV1::PostPanel {
                guild,
                channel,
                message,
                bot_user,
                expected_postimage,
                correlation,
            } => {
                self.observe_compensated_posted_panel_effect_v1(
                    guild,
                    channel,
                    message,
                    bot_user,
                    &expected_postimage,
                    &correlation,
                )
                .await
            }
        }
    }
}

trait MapDiscordObservationOutputV1<T> {
    fn map_output_v1<U>(self, map: impl FnOnce(T) -> U) -> DiscordEffectObservationOutcomeV1<U>;
}

impl<T> MapDiscordObservationOutputV1<T> for DiscordEffectObservationOutcomeV1<T> {
    fn map_output_v1<U>(self, map: impl FnOnce(T) -> U) -> DiscordEffectObservationOutcomeV1<U> {
        match self {
            DiscordEffectObservationOutcomeV1::ExactMatch { output, evidence } => {
                DiscordEffectObservationOutcomeV1::ExactMatch {
                    output: map(output),
                    evidence,
                }
            }
            DiscordEffectObservationOutcomeV1::Pending { evidence } => {
                DiscordEffectObservationOutcomeV1::Pending { evidence }
            }
            DiscordEffectObservationOutcomeV1::Conflict { evidence } => {
                DiscordEffectObservationOutcomeV1::Conflict { evidence }
            }
            DiscordEffectObservationOutcomeV1::Unsupported { evidence } => {
                DiscordEffectObservationOutcomeV1::Unsupported { evidence }
            }
            DiscordEffectObservationOutcomeV1::Unavailable(failure) => {
                DiscordEffectObservationOutcomeV1::Unavailable(failure)
            }
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RuntimeInteractionEffectInstanceObservationRequestV1 {
    target: automation_runtime_interaction::InteractionEffectInstanceTargetV1,
    instance_id: InstanceId,
    expected_postimage: InteractionEffectInstanceStateV1,
    expected_preimage: InteractionEffectInstanceStateV1,
    registration_identity: Option<RuntimeInteractionEffectInstanceRegistrationIdentityV1>,
}

impl RuntimeInteractionEffectInstanceObservationRequestV1 {
    pub fn target(&self) -> &automation_runtime_interaction::InteractionEffectInstanceTargetV1 {
        &self.target
    }

    pub fn instance_id(&self) -> &InstanceId {
        &self.instance_id
    }

    pub fn expected_postimage(&self) -> &InteractionEffectInstanceStateV1 {
        &self.expected_postimage
    }

    pub fn expected_preimage(&self) -> &InteractionEffectInstanceStateV1 {
        &self.expected_preimage
    }

    pub fn resolved_instance_manifest_digest(
        &self,
    ) -> Option<&InteractionInstanceManifestDigestV1> {
        self.registration_identity.as_ref().map(
            RuntimeInteractionEffectInstanceRegistrationIdentityV1::resolved_instance_manifest_digest,
        )
    }

    pub fn registration_identity(
        &self,
    ) -> Option<&RuntimeInteractionEffectInstanceRegistrationIdentityV1> {
        self.registration_identity.as_ref()
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RuntimeInteractionEffectInstanceRestoreRequestV1 {
    target: automation_runtime_interaction::InteractionEffectInstanceTargetV1,
    instance_id: InstanceId,
    expected_current: InteractionEffectInstanceStateV1,
    restore: InteractionEffectInstanceStateV1,
    expected_postimage: automation_runtime_interaction::InteractionEffectExpectedPostimageDigestV1,
    restored_preimage_digest: automation_runtime_interaction::InteractionEffectPreimageDigestV1,
    registration_identity: RuntimeInteractionEffectInstanceRegistrationIdentityV1,
}

impl RuntimeInteractionEffectInstanceRestoreRequestV1 {
    pub fn target(&self) -> &automation_runtime_interaction::InteractionEffectInstanceTargetV1 {
        &self.target
    }

    pub fn instance_id(&self) -> &InstanceId {
        &self.instance_id
    }

    pub fn expected_current(&self) -> &InteractionEffectInstanceStateV1 {
        &self.expected_current
    }

    pub fn restore(&self) -> &InteractionEffectInstanceStateV1 {
        &self.restore
    }

    pub fn expected_postimage(
        &self,
    ) -> &automation_runtime_interaction::InteractionEffectExpectedPostimageDigestV1 {
        &self.expected_postimage
    }

    pub fn restored_preimage_digest(
        &self,
    ) -> &automation_runtime_interaction::InteractionEffectPreimageDigestV1 {
        &self.restored_preimage_digest
    }

    pub fn resolved_instance_manifest_digest(&self) -> &InteractionInstanceManifestDigestV1 {
        self.registration_identity
            .resolved_instance_manifest_digest()
    }

    pub fn registration_identity(&self) -> &RuntimeInteractionEffectInstanceRegistrationIdentityV1 {
        &self.registration_identity
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RuntimeInteractionEffectInstanceObservationOutcomeV1 {
    Exact(InteractionEffectInstanceStateV1),
    Pending,
    Conflict,
    Unavailable(RuntimeInteractionEffectInternalReadFailureV1),
    RouteBlocked,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RuntimeInteractionEffectInstanceRestoreOutcomeV1 {
    Restored,
    KnownFailed(InteractionEffectKnownFailureV1),
    Indeterminate(InteractionEffectIndeterminateClassV1),
    Conflict,
    RouteBlocked,
}

#[allow(async_fn_in_trait)]
pub trait RuntimeInteractionEffectInternalRecoveryPortV1 {
    async fn observe_instance_state_v1(
        &self,
        request: RuntimeInteractionEffectInstanceObservationRequestV1,
    ) -> RuntimeInteractionEffectInstanceObservationOutcomeV1;

    async fn restore_instance_state_v1(
        &self,
        request: RuntimeInteractionEffectInstanceRestoreRequestV1,
    ) -> RuntimeInteractionEffectInstanceRestoreOutcomeV1;
}

pub struct OwnedSharedGatewayInternalEffectRecoveryV1<I> {
    instances: I,
    teardown: Arc<Teardown<I, OwnedTwilightInstanceDeleter>>,
}

impl<I> OwnedSharedGatewayInternalEffectRecoveryV1<I> {
    pub(crate) fn new(
        instances: I,
        teardown: Arc<Teardown<I, OwnedTwilightInstanceDeleter>>,
    ) -> Self {
        Self {
            instances,
            teardown,
        }
    }
}

impl<I> Debug for OwnedSharedGatewayInternalEffectRecoveryV1<I> {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("OwnedSharedGatewayInternalEffectRecoveryV1(<redacted>)")
    }
}

impl<I> RuntimeInteractionEffectInternalRecoveryPortV1
    for OwnedSharedGatewayInternalEffectRecoveryV1<I>
where
    I: InstanceTeardownStoreV1 + Send + Sync,
{
    async fn observe_instance_state_v1(
        &self,
        request: RuntimeInteractionEffectInstanceObservationRequestV1,
    ) -> RuntimeInteractionEffectInstanceObservationOutcomeV1 {
        let instance = match self
            .instances
            .get_for_teardown_v1(
                GuildId(request.target().guild_id().get()),
                request.instance_id(),
            )
            .await
        {
            Ok(instance) => instance,
            Err(error) => return instance_lookup_failure_v1(error),
        };
        let Some(instance) = instance else {
            return RuntimeInteractionEffectInstanceObservationOutcomeV1::Exact(
                InteractionEffectInstanceStateV1::Absent,
            );
        };
        if instance.guild_id.0 != request.target().guild_id().get()
            || &instance.id != request.instance_id()
        {
            return RuntimeInteractionEffectInstanceObservationOutcomeV1::Conflict;
        }
        let digest = match crate::action_plan_wire_preflight::exact_instance_manifest_digest_v1(
            instance.guild_id,
            &instance.resources,
        ) {
            Ok(digest) => digest,
            Err(_) => return RuntimeInteractionEffectInstanceObservationOutcomeV1::Conflict,
        };
        if !instance_manifest_allowed_v1(&request, &digest) {
            return RuntimeInteractionEffectInstanceObservationOutcomeV1::Conflict;
        }
        if !instance_registration_allowed_v1(&instance, &request, &digest) {
            return RuntimeInteractionEffectInstanceObservationOutcomeV1::Conflict;
        }
        match instance.status {
            InstanceStatus::Active | InstanceStatus::Disabled => {
                if matches!(
                    request.expected_postimage(),
                    InteractionEffectInstanceStateV1::Present { .. }
                ) {
                    RuntimeInteractionEffectInstanceObservationOutcomeV1::Exact(
                        request.expected_postimage().clone(),
                    )
                } else if matches!(
                    request.expected_preimage(),
                    InteractionEffectInstanceStateV1::Present { .. }
                ) {
                    RuntimeInteractionEffectInstanceObservationOutcomeV1::Exact(
                        request.expected_preimage().clone(),
                    )
                } else {
                    RuntimeInteractionEffectInstanceObservationOutcomeV1::Conflict
                }
            }
            InstanceStatus::Deleting => {
                RuntimeInteractionEffectInstanceObservationOutcomeV1::Pending
            }
            InstanceStatus::Deleted => RuntimeInteractionEffectInstanceObservationOutcomeV1::Exact(
                InteractionEffectInstanceStateV1::Absent,
            ),
        }
    }

    async fn restore_instance_state_v1(
        &self,
        request: RuntimeInteractionEffectInstanceRestoreRequestV1,
    ) -> RuntimeInteractionEffectInstanceRestoreOutcomeV1 {
        if request.restore() != &InteractionEffectInstanceStateV1::Absent {
            return RuntimeInteractionEffectInstanceRestoreOutcomeV1::Conflict;
        }
        let guild_id = GuildId(request.target().guild_id().get());
        let instance = match self
            .instances
            .get_for_teardown_v1(guild_id, request.instance_id())
            .await
        {
            Ok(instance) => instance,
            Err(error) => return instance_restore_store_failure_v1(error),
        };
        let Some(instance) = instance else {
            return RuntimeInteractionEffectInstanceRestoreOutcomeV1::Restored;
        };
        if instance.guild_id != guild_id || &instance.id != request.instance_id() {
            return RuntimeInteractionEffectInstanceRestoreOutcomeV1::Conflict;
        }
        let digest = match crate::action_plan_wire_preflight::exact_instance_manifest_digest_v1(
            instance.guild_id,
            &instance.resources,
        ) {
            Ok(digest) => digest,
            Err(_) => return RuntimeInteractionEffectInstanceRestoreOutcomeV1::Conflict,
        };
        if digest != *request.resolved_instance_manifest_digest()
            || !matches!(
                request.expected_current(),
                InteractionEffectInstanceStateV1::Present { .. }
            )
            || !instance_registration_matches_v1(
                &instance,
                request.registration_identity(),
                &digest,
            )
        {
            return RuntimeInteractionEffectInstanceRestoreOutcomeV1::Conflict;
        }
        if instance.status == InstanceStatus::Deleted {
            return RuntimeInteractionEffectInstanceRestoreOutcomeV1::Restored;
        }
        let exact = ExactInstanceTeardownRequestV1::from_exact_instance_v1(&instance);
        match self.teardown.teardown_exact_v1(&exact).await {
            Ok(
                TeardownOutcome::Completed
                | TeardownOutcome::ResumedAndCompleted
                | TeardownOutcome::AlreadyDeleted,
            ) => RuntimeInteractionEffectInstanceRestoreOutcomeV1::Restored,
            Ok(TeardownOutcome::InProgress) => {
                RuntimeInteractionEffectInstanceRestoreOutcomeV1::Indeterminate(
                    InteractionEffectIndeterminateClassV1::ProviderUnavailable,
                )
            }
            Err(error) => instance_restore_failure_v1(error),
        }
    }
}

#[derive(Clone)]
pub struct OwnedSharedGatewayDiscordEffectsV1 {
    http: Arc<Client>,
    bot_user: UserId,
}

impl OwnedSharedGatewayDiscordEffectsV1 {
    pub(crate) fn new(http: Arc<Client>, bot_user: UserId) -> Self {
        Self { http, bot_user }
    }

    pub fn bot_user_v1(&self) -> UserId {
        self.bot_user
    }

    pub fn adapter_v1(&self, ruleset_key: &RuleSetKey) -> TwilightMutationAdapter<'_> {
        TwilightMutationAdapter::new(&self.http, ruleset_key.as_str().to_owned())
    }
}

impl Debug for OwnedSharedGatewayDiscordEffectsV1 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("OwnedSharedGatewayDiscordEffectsV1(<redacted>)")
    }
}

pub struct RuntimeInteractionEffectRecoveryExecutorV1<'a, D, R, I> {
    discord: &'a D,
    original_response: &'a R,
    internal: &'a I,
    bot_user: UserId,
}

impl<'a, D, R, I> RuntimeInteractionEffectRecoveryExecutorV1<'a, D, R, I> {
    pub fn new(
        discord: &'a D,
        original_response: &'a R,
        internal: &'a I,
        bot_user: UserId,
    ) -> Self {
        Self {
            discord,
            original_response,
            internal,
            bot_user,
        }
    }
}

impl<D, R, I> Debug for RuntimeInteractionEffectRecoveryExecutorV1<'_, D, R, I> {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimeInteractionEffectRecoveryExecutorV1(<redacted>)")
    }
}

impl<D, R, I> RuntimeInteractionEffectRecoveryExecutorV1<'_, D, R, I>
where
    D: RuntimeInteractionEffectDiscordRecoveryPortV1,
    R: DiscordOriginalResponseObserverV1,
    I: RuntimeInteractionEffectInternalRecoveryPortV1,
{
    pub async fn observe_v1(
        &self,
        request: RuntimeInteractionEffectRecoveryObservationRequestV1,
    ) -> RuntimeInteractionEffectRecoveryObservationDispositionV1 {
        let (definition, token) = request.into_parts();
        match definition.binding().target() {
            InteractionEffectRecoveryTargetV1::EditResponse { .. } => {
                self.observe_original_response_v1(definition, token).await
            }
            InteractionEffectRecoveryTargetV1::RegisterInstance { .. }
            | InteractionEffectRecoveryTargetV1::TeardownInstance { .. } => {
                self.observe_internal_v1(&definition).await
            }
            _ => self.observe_discord_v1(&definition).await,
        }
    }

    pub async fn compensate_v1(
        &self,
        request: RuntimeInteractionEffectRecoveryCompensationRequestV1,
    ) -> RuntimeInteractionEffectRecoveryCompensationDispositionV1 {
        match request.definition.binding().target() {
            InteractionEffectRecoveryTargetV1::RegisterInstance { .. } => {
                self.compensate_internal_v1(&request).await
            }
            InteractionEffectRecoveryTargetV1::TeardownInstance { .. }
            | InteractionEffectRecoveryTargetV1::EditResponse { .. } => {
                RuntimeInteractionEffectRecoveryCompensationDispositionV1::RecoveryRequired(
                    RuntimeInteractionEffectRecoveryRequiredV1::NonCompensable,
                )
            }
            _ => self.compensate_discord_v1(&request).await,
        }
    }

    pub async fn observe_compensation_v1(
        &self,
        request: RuntimeInteractionEffectRecoveryCompensationRequestV1,
    ) -> RuntimeInteractionEffectRecoveryCompensationObservationDispositionV1 {
        match request.definition.binding().target() {
            InteractionEffectRecoveryTargetV1::RegisterInstance { .. } => {
                self.observe_internal_compensation_v1(&request).await
            }
            InteractionEffectRecoveryTargetV1::TeardownInstance { .. }
            | InteractionEffectRecoveryTargetV1::EditResponse { .. } => {
                RuntimeInteractionEffectRecoveryCompensationObservationDispositionV1::RecoveryRequired(
                    RuntimeInteractionEffectRecoveryRequiredV1::NonCompensable,
                )
            }
            _ => self.observe_discord_compensation_v1(&request).await,
        }
    }

    async fn observe_discord_v1(
        &self,
        definition: &RuntimeInteractionEffectRecoveryDefinitionV1,
    ) -> RuntimeInteractionEffectRecoveryObservationDispositionV1 {
        let request = match discord_observation_request_v1(definition.binding(), self.bot_user) {
            Some(request) => request,
            None => {
                return RuntimeInteractionEffectRecoveryObservationDispositionV1::RecoveryRequired(
                    RuntimeInteractionEffectRecoveryRequiredV1::ObservationProtocol,
                )
            }
        };
        let outcome = self.discord.observe_discord_effect_v1(request).await;
        map_discord_observation_v1(definition.binding(), outcome)
    }

    async fn observe_original_response_v1(
        &self,
        definition: RuntimeInteractionEffectRecoveryDefinitionV1,
        token: Option<InteractionTokenV1>,
    ) -> RuntimeInteractionEffectRecoveryObservationDispositionV1 {
        let InteractionEffectRecoveryTargetV1::EditResponse {
            receipt_identity,
            payload_digest,
        } = definition.binding().target()
        else {
            return RuntimeInteractionEffectRecoveryObservationDispositionV1::RecoveryRequired(
                RuntimeInteractionEffectRecoveryRequiredV1::ObservationProtocol,
            );
        };
        let Some(token) = token else {
            return RuntimeInteractionEffectRecoveryObservationDispositionV1::RecoveryRequired(
                RuntimeInteractionEffectRecoveryRequiredV1::ResponseTokenUnavailable,
            );
        };
        let outcome = self
            .original_response
            .observe_original_response_v1(DiscordOriginalResponseObservationRequestV1::new(
                *receipt_identity,
                token,
                definition.binding().expected_postimage_digest().clone(),
                payload_digest.clone(),
            ))
            .await;
        map_original_response_observation_v1(definition.binding(), outcome)
    }

    async fn observe_internal_v1(
        &self,
        definition: &RuntimeInteractionEffectRecoveryDefinitionV1,
    ) -> RuntimeInteractionEffectRecoveryObservationDispositionV1 {
        let Some(request) = internal_observation_request_v1(definition) else {
            return RuntimeInteractionEffectRecoveryObservationDispositionV1::RecoveryRequired(
                RuntimeInteractionEffectRecoveryRequiredV1::ObservationProtocol,
            );
        };
        let observed = self.internal.observe_instance_state_v1(request).await;
        map_internal_observation_v1(definition, observed)
    }

    async fn compensate_discord_v1(
        &self,
        request: &RuntimeInteractionEffectRecoveryCompensationRequestV1,
    ) -> RuntimeInteractionEffectRecoveryCompensationDispositionV1 {
        let Some(operation) = discord_compensation_request_v1(request, self.bot_user) else {
            return RuntimeInteractionEffectRecoveryCompensationDispositionV1::RecoveryRequired(
                RuntimeInteractionEffectRecoveryRequiredV1::ObservationProtocol,
            );
        };
        map_compensation_attempt_v1(
            request.definition.binding(),
            self.discord.compensate_discord_effect_v1(operation).await,
        )
    }

    async fn compensate_internal_v1(
        &self,
        request: &RuntimeInteractionEffectRecoveryCompensationRequestV1,
    ) -> RuntimeInteractionEffectRecoveryCompensationDispositionV1 {
        let Some(operation) = internal_restore_request_v1(request) else {
            return RuntimeInteractionEffectRecoveryCompensationDispositionV1::RecoveryRequired(
                RuntimeInteractionEffectRecoveryRequiredV1::ObservationProtocol,
            );
        };
        match self.internal.restore_instance_state_v1(operation).await {
            RuntimeInteractionEffectInstanceRestoreOutcomeV1::Restored => {
                RuntimeInteractionEffectRecoveryCompensationDispositionV1::Finish(
                    InteractionEffectCompensationOutcomeV1::Succeeded {
                        restored_preimage_digest: request
                            .definition
                            .binding()
                            .preimage_digest()
                            .clone(),
                    },
                )
            }
            RuntimeInteractionEffectInstanceRestoreOutcomeV1::KnownFailed(failure) => {
                map_compensation_known_failure_v1(failure)
            }
            RuntimeInteractionEffectInstanceRestoreOutcomeV1::Indeterminate(class) => {
                RuntimeInteractionEffectRecoveryCompensationDispositionV1::Finish(
                    InteractionEffectCompensationOutcomeV1::Indeterminate(class),
                )
            }
            RuntimeInteractionEffectInstanceRestoreOutcomeV1::Conflict => {
                RuntimeInteractionEffectRecoveryCompensationDispositionV1::RecoveryRequired(
                    RuntimeInteractionEffectRecoveryRequiredV1::InternalConflict,
                )
            }
            RuntimeInteractionEffectInstanceRestoreOutcomeV1::RouteBlocked => {
                RuntimeInteractionEffectRecoveryCompensationDispositionV1::RouteBlocked(
                    RuntimeInteractionEffectRecoveryRouteBlockV1::InternalAuthority,
                )
            }
        }
    }

    async fn observe_discord_compensation_v1(
        &self,
        request: &RuntimeInteractionEffectRecoveryCompensationRequestV1,
    ) -> RuntimeInteractionEffectRecoveryCompensationObservationDispositionV1 {
        let Some(operation) = discord_compensation_request_v1(request, self.bot_user) else {
            return RuntimeInteractionEffectRecoveryCompensationObservationDispositionV1::RecoveryRequired(
                RuntimeInteractionEffectRecoveryRequiredV1::ObservationProtocol,
            );
        };
        map_discord_compensation_observation_v1(
            request.definition.binding(),
            self.discord
                .observe_discord_compensation_v1(operation)
                .await,
        )
    }

    async fn observe_internal_compensation_v1(
        &self,
        request: &RuntimeInteractionEffectRecoveryCompensationRequestV1,
    ) -> RuntimeInteractionEffectRecoveryCompensationObservationDispositionV1 {
        let definition = &request.definition;
        let Some(observation) = internal_observation_request_v1(definition) else {
            return RuntimeInteractionEffectRecoveryCompensationObservationDispositionV1::RecoveryRequired(
                RuntimeInteractionEffectRecoveryRequiredV1::ObservationProtocol,
            );
        };
        let observed = self.internal.observe_instance_state_v1(observation).await;
        map_internal_compensation_observation_v1(definition, observed)
    }
}

fn discord_observation_request_v1(
    binding: &InteractionEffectRecoveryBindingV1,
    bot_user: UserId,
) -> Option<RuntimeInteractionEffectDiscordObservationRequestV1> {
    let expected_postimage = binding.expected_postimage_digest().clone();
    let correlation = binding.correlation().clone();
    Some(match binding.target() {
        InteractionEffectRecoveryTargetV1::CreateRole { guild_id } => {
            RuntimeInteractionEffectDiscordObservationRequestV1::CreateRole {
                guild: GuildId(guild_id.get()),
                bot_user,
                expected_postimage,
                correlation,
            }
        }
        InteractionEffectRecoveryTargetV1::CreateChannel { guild_id } => {
            RuntimeInteractionEffectDiscordObservationRequestV1::CreateChannel {
                guild: GuildId(guild_id.get()),
                bot_user,
                expected_postimage,
                correlation,
            }
        }
        InteractionEffectRecoveryTargetV1::GrantRole { target } => {
            RuntimeInteractionEffectDiscordObservationRequestV1::GrantRole {
                guild: GuildId(target.guild_id().get()),
                bot_user,
                member: UserId(target.user_id().get()),
                role: RoleId(target.role_id().get()),
                expected_postimage,
                correlation,
            }
        }
        InteractionEffectRecoveryTargetV1::UpsertOverwrite { target, .. } => {
            RuntimeInteractionEffectDiscordObservationRequestV1::UpsertOverwrite {
                guild: GuildId(target.guild_id().get()),
                bot_user,
                channel: ChannelId(target.channel_id().get()),
                target: discord_overwrite_target_v1(target.target()),
                expected_postimage,
                correlation,
            }
        }
        InteractionEffectRecoveryTargetV1::PostPanel { .. } => {
            RuntimeInteractionEffectDiscordObservationRequestV1::PostPanel {
                expected_postimage,
                correlation,
            }
        }
        _ => return None,
    })
}

fn discord_compensation_request_v1(
    request: &RuntimeInteractionEffectRecoveryCompensationRequestV1,
    bot_user: UserId,
) -> Option<RuntimeInteractionEffectDiscordCompensationRequestV1> {
    let binding = request.definition.binding();
    let expected_postimage = binding.expected_postimage_digest().clone();
    let correlation = binding.correlation().clone();
    Some(
        match (
            binding.target(),
            request.successful_output(),
            binding.preimage(),
        ) {
            (
                InteractionEffectRecoveryTargetV1::CreateRole { guild_id },
                InteractionEffectObservedOutputV1::CreatedRole { role_id, .. },
                InteractionEffectPreimageV1::None,
            ) => RuntimeInteractionEffectDiscordCompensationRequestV1::CreateRole {
                guild: GuildId(guild_id.get()),
                bot_user,
                role: RoleId(role_id.get()),
                expected_postimage,
                correlation,
            },
            (
                InteractionEffectRecoveryTargetV1::CreateChannel { guild_id },
                InteractionEffectObservedOutputV1::CreatedChannel { channel_id, .. },
                InteractionEffectPreimageV1::None,
            ) => RuntimeInteractionEffectDiscordCompensationRequestV1::CreateChannel {
                guild: GuildId(guild_id.get()),
                bot_user,
                channel: ChannelId(channel_id.get()),
                expected_postimage,
                correlation,
            },
            (
                InteractionEffectRecoveryTargetV1::GrantRole { target },
                InteractionEffectObservedOutputV1::RoleMembership { .. },
                InteractionEffectPreimageV1::RoleMembership { present, .. },
            ) => RuntimeInteractionEffectDiscordCompensationRequestV1::GrantRole {
                guild: GuildId(target.guild_id().get()),
                bot_user,
                member: UserId(target.user_id().get()),
                role: RoleId(target.role_id().get()),
                before_present: *present,
                expected_postimage,
                correlation,
            },
            (
                InteractionEffectRecoveryTargetV1::UpsertOverwrite { target, .. },
                InteractionEffectObservedOutputV1::PermissionOverwrite { .. },
                InteractionEffectPreimageV1::PermissionOverwrite { before, .. },
            ) => RuntimeInteractionEffectDiscordCompensationRequestV1::UpsertOverwrite {
                guild: GuildId(target.guild_id().get()),
                bot_user,
                channel: ChannelId(target.channel_id().get()),
                target: discord_overwrite_target_v1(target.target()),
                before: *before,
                expected_postimage,
                correlation,
            },
            (
                InteractionEffectRecoveryTargetV1::PostPanel {
                    guild_id,
                    channel_id,
                    ..
                },
                InteractionEffectObservedOutputV1::PostedMessage { message_id, .. },
                InteractionEffectPreimageV1::None,
            ) => RuntimeInteractionEffectDiscordCompensationRequestV1::PostPanel {
                guild: GuildId(guild_id.get()),
                channel: ChannelId(channel_id.get()),
                message: MessageId(message_id.get()),
                bot_user,
                expected_postimage,
                correlation,
            },
            _ => return None,
        },
    )
}

fn internal_observation_request_v1(
    definition: &RuntimeInteractionEffectRecoveryDefinitionV1,
) -> Option<RuntimeInteractionEffectInstanceObservationRequestV1> {
    let target = match definition.binding().target() {
        InteractionEffectRecoveryTargetV1::RegisterInstance { target, .. }
        | InteractionEffectRecoveryTargetV1::TeardownInstance { target } => target.clone(),
        _ => return None,
    };
    Some(RuntimeInteractionEffectInstanceObservationRequestV1 {
        target,
        instance_id: definition.instance_id()?.clone(),
        expected_postimage: expected_instance_postimage_v1(definition.binding())?,
        expected_preimage: instance_preimage_v1(definition.binding())?,
        registration_identity: definition.registration_identity().cloned(),
    })
}

fn internal_restore_request_v1(
    request: &RuntimeInteractionEffectRecoveryCompensationRequestV1,
) -> Option<RuntimeInteractionEffectInstanceRestoreRequestV1> {
    let binding = request.definition.binding();
    let (
        InteractionEffectRecoveryTargetV1::RegisterInstance {
            target,
            manifest_digest,
            ..
        },
        InteractionEffectPreimageV1::InstanceRegistration {
            before,
            target: before_target,
        },
    ) = (binding.target(), binding.preimage())
    else {
        return None;
    };
    if target != before_target {
        return None;
    }
    Some(RuntimeInteractionEffectInstanceRestoreRequestV1 {
        target: target.clone(),
        instance_id: request.definition.instance_id()?.clone(),
        expected_current: InteractionEffectInstanceStateV1::Present {
            manifest_digest: manifest_digest.clone(),
        },
        restore: before.clone(),
        expected_postimage: binding.expected_postimage_digest().clone(),
        restored_preimage_digest: binding.preimage_digest().clone(),
        registration_identity: request.definition.registration_identity()?.clone(),
    })
}

fn map_discord_observation_v1(
    binding: &InteractionEffectRecoveryBindingV1,
    outcome: DiscordEffectObservationOutcomeV1<RuntimeInteractionEffectDiscordObservedOutputV1>,
) -> RuntimeInteractionEffectRecoveryObservationDispositionV1 {
    match outcome {
        DiscordEffectObservationOutcomeV1::ExactMatch { output, evidence } => {
            if matches!(
                binding.target(),
                InteractionEffectRecoveryTargetV1::PostPanel { .. }
            ) || !exact_evidence_matches_v1(binding, evidence)
            {
                return RuntimeInteractionEffectRecoveryObservationDispositionV1::RecoveryRequired(
                    RuntimeInteractionEffectRecoveryRequiredV1::ObservationProtocol,
                );
            }
            let Some(output) = interaction_output_v1(binding, output) else {
                return RuntimeInteractionEffectRecoveryObservationDispositionV1::RecoveryRequired(
                    RuntimeInteractionEffectRecoveryRequiredV1::ObservationProtocol,
                );
            };
            RuntimeInteractionEffectRecoveryObservationDispositionV1::Reconcile(
                InteractionEffectObservationOutcomeV1::ExactMatch {
                    output,
                    evidence: interaction_evidence_v1(evidence),
                },
            )
        }
        DiscordEffectObservationOutcomeV1::Pending { evidence } => {
            RuntimeInteractionEffectRecoveryObservationDispositionV1::Reconcile(
                InteractionEffectObservationOutcomeV1::Pending {
                    evidence: interaction_evidence_v1(evidence),
                },
            )
        }
        DiscordEffectObservationOutcomeV1::Conflict { evidence } => {
            RuntimeInteractionEffectRecoveryObservationDispositionV1::Reconcile(
                InteractionEffectObservationOutcomeV1::Conflict {
                    evidence: interaction_evidence_v1(evidence),
                },
            )
        }
        DiscordEffectObservationOutcomeV1::Unsupported { evidence } => {
            RuntimeInteractionEffectRecoveryObservationDispositionV1::Reconcile(
                InteractionEffectObservationOutcomeV1::Unsupported {
                    evidence: interaction_evidence_v1(evidence),
                },
            )
        }
        DiscordEffectObservationOutcomeV1::Unavailable(failure) => {
            map_discord_read_failure_v1(failure, false)
        }
    }
}

fn map_original_response_observation_v1(
    binding: &InteractionEffectRecoveryBindingV1,
    outcome: DiscordOriginalResponseObservationOutcomeV1,
) -> RuntimeInteractionEffectRecoveryObservationDispositionV1 {
    match outcome {
        DiscordOriginalResponseObservationOutcomeV1::ExactMatch { output, evidence } => {
            if binding.validate_observed_output(&output).is_err()
                || !exact_evidence_matches_v1(binding, evidence)
            {
                return protocol_observation_v1();
            }
            RuntimeInteractionEffectRecoveryObservationDispositionV1::Reconcile(
                InteractionEffectObservationOutcomeV1::ExactMatch {
                    output,
                    evidence: interaction_evidence_v1(evidence),
                },
            )
        }
        DiscordOriginalResponseObservationOutcomeV1::ExactAbsence { evidence } => {
            RuntimeInteractionEffectRecoveryObservationDispositionV1::Reconcile(
                InteractionEffectObservationOutcomeV1::ExactAbsence {
                    evidence: interaction_evidence_v1(evidence),
                },
            )
        }
        DiscordOriginalResponseObservationOutcomeV1::Conflict { evidence } => {
            RuntimeInteractionEffectRecoveryObservationDispositionV1::Reconcile(
                InteractionEffectObservationOutcomeV1::Conflict {
                    evidence: interaction_evidence_v1(evidence),
                },
            )
        }
        DiscordOriginalResponseObservationOutcomeV1::Unavailable(failure) => {
            map_discord_read_failure_v1(failure, true)
        }
    }
}

fn map_discord_read_failure_v1(
    failure: DiscordEffectReadFailureV1,
    response: bool,
) -> RuntimeInteractionEffectRecoveryObservationDispositionV1 {
    match failure {
        DiscordEffectReadFailureV1::KnownFailed(known)
            if known.class() == InteractionEffectKnownFailureClassV1::Forbidden && response =>
        {
            RuntimeInteractionEffectRecoveryObservationDispositionV1::RecoveryRequired(
                RuntimeInteractionEffectRecoveryRequiredV1::ResponseTokenUnavailable,
            )
        }
        DiscordEffectReadFailureV1::KnownFailed(known)
            if known.class() == InteractionEffectKnownFailureClassV1::Forbidden =>
        {
            RuntimeInteractionEffectRecoveryObservationDispositionV1::RouteBlocked(
                RuntimeInteractionEffectRecoveryRouteBlockV1::DiscordForbidden,
            )
        }
        DiscordEffectReadFailureV1::KnownFailed(known)
            if known.class() == InteractionEffectKnownFailureClassV1::RateLimitedBeforeDispatch =>
        {
            RuntimeInteractionEffectRecoveryObservationDispositionV1::Deferred(
                RuntimeInteractionEffectRecoveryDeferredV1::Discord(failure),
            )
        }
        DiscordEffectReadFailureV1::KnownFailed(_) => {
            RuntimeInteractionEffectRecoveryObservationDispositionV1::RecoveryRequired(
                if response {
                    RuntimeInteractionEffectRecoveryRequiredV1::ResponseTokenUnavailable
                } else {
                    RuntimeInteractionEffectRecoveryRequiredV1::DiscordReadRejected
                },
            )
        }
        DiscordEffectReadFailureV1::Indeterminate(_) => {
            RuntimeInteractionEffectRecoveryObservationDispositionV1::Deferred(
                RuntimeInteractionEffectRecoveryDeferredV1::Discord(failure),
            )
        }
    }
}

fn map_internal_observation_v1(
    definition: &RuntimeInteractionEffectRecoveryDefinitionV1,
    observed: RuntimeInteractionEffectInstanceObservationOutcomeV1,
) -> RuntimeInteractionEffectRecoveryObservationDispositionV1 {
    match observed {
        RuntimeInteractionEffectInstanceObservationOutcomeV1::Exact(state) => {
            let expected = expected_instance_postimage_v1(definition.binding());
            let preimage = instance_preimage_v1(definition.binding());
            let evidence = internal_evidence_v1(
                if expected.as_ref() == Some(&state) {
                    1
                } else {
                    0
                },
                if expected.as_ref() == Some(&state) || preimage.as_ref() == Some(&state) {
                    0
                } else {
                    1
                },
                true,
                true,
                expected.as_ref() == Some(&state),
            );
            if expected.as_ref() == Some(&state) {
                let Some(target) = instance_target_v1(definition.binding()) else {
                    return protocol_observation_v1();
                };
                RuntimeInteractionEffectRecoveryObservationDispositionV1::Reconcile(
                    InteractionEffectObservationOutcomeV1::ExactMatch {
                        output: InteractionEffectObservedOutputV1::InstanceState { target, state },
                        evidence,
                    },
                )
            } else if preimage.as_ref() == Some(&state) {
                RuntimeInteractionEffectRecoveryObservationDispositionV1::Reconcile(
                    InteractionEffectObservationOutcomeV1::ExactAbsence { evidence },
                )
            } else {
                RuntimeInteractionEffectRecoveryObservationDispositionV1::Reconcile(
                    InteractionEffectObservationOutcomeV1::Conflict { evidence },
                )
            }
        }
        RuntimeInteractionEffectInstanceObservationOutcomeV1::Pending => {
            RuntimeInteractionEffectRecoveryObservationDispositionV1::Reconcile(
                InteractionEffectObservationOutcomeV1::Pending {
                    evidence: internal_evidence_v1(0, 0, true, true, false),
                },
            )
        }
        RuntimeInteractionEffectInstanceObservationOutcomeV1::Conflict => {
            RuntimeInteractionEffectRecoveryObservationDispositionV1::Reconcile(
                InteractionEffectObservationOutcomeV1::Conflict {
                    evidence: internal_evidence_v1(0, 1, true, true, false),
                },
            )
        }
        RuntimeInteractionEffectInstanceObservationOutcomeV1::Unavailable(failure) => {
            RuntimeInteractionEffectRecoveryObservationDispositionV1::Deferred(
                RuntimeInteractionEffectRecoveryDeferredV1::Internal(failure),
            )
        }
        RuntimeInteractionEffectInstanceObservationOutcomeV1::RouteBlocked => {
            RuntimeInteractionEffectRecoveryObservationDispositionV1::RouteBlocked(
                RuntimeInteractionEffectRecoveryRouteBlockV1::InternalAuthority,
            )
        }
    }
}

fn map_compensation_attempt_v1(
    binding: &InteractionEffectRecoveryBindingV1,
    outcome: DiscordEffectAttemptOutcomeV1<()>,
) -> RuntimeInteractionEffectRecoveryCompensationDispositionV1 {
    match outcome {
        DiscordEffectAttemptOutcomeV1::KnownSucceeded(()) => {
            RuntimeInteractionEffectRecoveryCompensationDispositionV1::Finish(
                InteractionEffectCompensationOutcomeV1::Succeeded {
                    restored_preimage_digest: binding.preimage_digest().clone(),
                },
            )
        }
        DiscordEffectAttemptOutcomeV1::KnownFailed(failure) => {
            map_compensation_known_failure_v1(failure)
        }
        DiscordEffectAttemptOutcomeV1::Indeterminate(class) => {
            RuntimeInteractionEffectRecoveryCompensationDispositionV1::Finish(
                InteractionEffectCompensationOutcomeV1::Indeterminate(class),
            )
        }
    }
}

fn map_compensation_known_failure_v1(
    failure: InteractionEffectKnownFailureV1,
) -> RuntimeInteractionEffectRecoveryCompensationDispositionV1 {
    match failure.class() {
        InteractionEffectKnownFailureClassV1::Forbidden => {
            RuntimeInteractionEffectRecoveryCompensationDispositionV1::RouteBlocked(
                RuntimeInteractionEffectRecoveryRouteBlockV1::DiscordForbidden,
            )
        }
        InteractionEffectKnownFailureClassV1::RateLimitedBeforeDispatch => {
            RuntimeInteractionEffectRecoveryCompensationDispositionV1::Deferred(
                RuntimeInteractionEffectRecoveryDeferredV1::Discord(
                    DiscordEffectReadFailureV1::KnownFailed(failure),
                ),
            )
        }
        _ => RuntimeInteractionEffectRecoveryCompensationDispositionV1::Finish(
            InteractionEffectCompensationOutcomeV1::KnownFailed(failure),
        ),
    }
}

fn map_discord_compensation_observation_v1(
    binding: &InteractionEffectRecoveryBindingV1,
    outcome: DiscordEffectCompensationObservationOutcomeV1,
) -> RuntimeInteractionEffectRecoveryCompensationObservationDispositionV1 {
    match outcome {
        DiscordEffectCompensationObservationOutcomeV1::Restored { evidence } => {
            RuntimeInteractionEffectRecoveryCompensationObservationDispositionV1::Reconcile(
                InteractionEffectCompensationObservationOutcomeV1::Restored {
                    restored_preimage_digest: binding.preimage_digest().clone(),
                    evidence: interaction_evidence_v1(evidence),
                },
            )
        }
        DiscordEffectCompensationObservationOutcomeV1::Pending { evidence } => {
            RuntimeInteractionEffectRecoveryCompensationObservationDispositionV1::Reconcile(
                InteractionEffectCompensationObservationOutcomeV1::Pending {
                    evidence: interaction_evidence_v1(evidence),
                },
            )
        }
        DiscordEffectCompensationObservationOutcomeV1::Conflict { evidence } => {
            RuntimeInteractionEffectRecoveryCompensationObservationDispositionV1::Reconcile(
                InteractionEffectCompensationObservationOutcomeV1::Conflict {
                    evidence: interaction_evidence_v1(evidence),
                },
            )
        }
        DiscordEffectCompensationObservationOutcomeV1::Unsupported { evidence } => {
            RuntimeInteractionEffectRecoveryCompensationObservationDispositionV1::Reconcile(
                InteractionEffectCompensationObservationOutcomeV1::Unsupported {
                    evidence: interaction_evidence_v1(evidence),
                },
            )
        }
        DiscordEffectCompensationObservationOutcomeV1::Unavailable(failure) => {
            map_compensation_read_failure_v1(failure)
        }
    }
}

fn map_compensation_read_failure_v1(
    failure: DiscordEffectReadFailureV1,
) -> RuntimeInteractionEffectRecoveryCompensationObservationDispositionV1 {
    match failure {
        DiscordEffectReadFailureV1::KnownFailed(known)
            if known.class() == InteractionEffectKnownFailureClassV1::Forbidden =>
        {
            RuntimeInteractionEffectRecoveryCompensationObservationDispositionV1::RouteBlocked(
                RuntimeInteractionEffectRecoveryRouteBlockV1::DiscordForbidden,
            )
        }
        DiscordEffectReadFailureV1::KnownFailed(known)
            if known.class() == InteractionEffectKnownFailureClassV1::RateLimitedBeforeDispatch =>
        {
            RuntimeInteractionEffectRecoveryCompensationObservationDispositionV1::Deferred(
                RuntimeInteractionEffectRecoveryDeferredV1::Discord(failure),
            )
        }
        DiscordEffectReadFailureV1::KnownFailed(_) => {
            RuntimeInteractionEffectRecoveryCompensationObservationDispositionV1::RecoveryRequired(
                RuntimeInteractionEffectRecoveryRequiredV1::DiscordReadRejected,
            )
        }
        DiscordEffectReadFailureV1::Indeterminate(_) => {
            RuntimeInteractionEffectRecoveryCompensationObservationDispositionV1::Deferred(
                RuntimeInteractionEffectRecoveryDeferredV1::Discord(failure),
            )
        }
    }
}

fn map_internal_compensation_observation_v1(
    definition: &RuntimeInteractionEffectRecoveryDefinitionV1,
    observed: RuntimeInteractionEffectInstanceObservationOutcomeV1,
) -> RuntimeInteractionEffectRecoveryCompensationObservationDispositionV1 {
    let Some(preimage) = instance_preimage_v1(definition.binding()) else {
        return protocol_compensation_observation_v1();
    };
    let Some(expected_current) = expected_instance_postimage_v1(definition.binding()) else {
        return protocol_compensation_observation_v1();
    };
    match observed {
        RuntimeInteractionEffectInstanceObservationOutcomeV1::Exact(state) if state == preimage => {
            RuntimeInteractionEffectRecoveryCompensationObservationDispositionV1::Reconcile(
                InteractionEffectCompensationObservationOutcomeV1::Restored {
                    restored_preimage_digest: definition.binding().preimage_digest().clone(),
                    evidence: internal_evidence_v1(1, 0, true, true, true),
                },
            )
        }
        RuntimeInteractionEffectInstanceObservationOutcomeV1::Exact(state)
            if state == expected_current =>
        {
            RuntimeInteractionEffectRecoveryCompensationObservationDispositionV1::Reconcile(
                InteractionEffectCompensationObservationOutcomeV1::Pending {
                    evidence: internal_evidence_v1(0, 0, true, true, false),
                },
            )
        }
        RuntimeInteractionEffectInstanceObservationOutcomeV1::Exact(_) => {
            RuntimeInteractionEffectRecoveryCompensationObservationDispositionV1::Reconcile(
                InteractionEffectCompensationObservationOutcomeV1::Conflict {
                    evidence: internal_evidence_v1(0, 1, true, true, false),
                },
            )
        }
        RuntimeInteractionEffectInstanceObservationOutcomeV1::Pending => {
            RuntimeInteractionEffectRecoveryCompensationObservationDispositionV1::Reconcile(
                InteractionEffectCompensationObservationOutcomeV1::Pending {
                    evidence: internal_evidence_v1(0, 0, true, true, false),
                },
            )
        }
        RuntimeInteractionEffectInstanceObservationOutcomeV1::Conflict => {
            RuntimeInteractionEffectRecoveryCompensationObservationDispositionV1::Reconcile(
                InteractionEffectCompensationObservationOutcomeV1::Conflict {
                    evidence: internal_evidence_v1(0, 1, true, true, false),
                },
            )
        }
        RuntimeInteractionEffectInstanceObservationOutcomeV1::Unavailable(failure) => {
            RuntimeInteractionEffectRecoveryCompensationObservationDispositionV1::Deferred(
                RuntimeInteractionEffectRecoveryDeferredV1::Internal(failure),
            )
        }
        RuntimeInteractionEffectInstanceObservationOutcomeV1::RouteBlocked => {
            RuntimeInteractionEffectRecoveryCompensationObservationDispositionV1::RouteBlocked(
                RuntimeInteractionEffectRecoveryRouteBlockV1::InternalAuthority,
            )
        }
    }
}

fn interaction_output_v1(
    binding: &InteractionEffectRecoveryBindingV1,
    observed: RuntimeInteractionEffectDiscordObservedOutputV1,
) -> Option<InteractionEffectObservedOutputV1> {
    let output = match (binding.target(), observed) {
        (
            InteractionEffectRecoveryTargetV1::CreateRole { guild_id },
            RuntimeInteractionEffectDiscordObservedOutputV1::CreatedRole(role),
        ) => InteractionEffectObservedOutputV1::CreatedRole {
            guild_id: *guild_id,
            role_id: automation_runtime_interaction::InteractionEffectRoleIdV1::new(role.0).ok()?,
        },
        (
            InteractionEffectRecoveryTargetV1::CreateChannel { guild_id },
            RuntimeInteractionEffectDiscordObservedOutputV1::CreatedChannel(channel),
        ) => InteractionEffectObservedOutputV1::CreatedChannel {
            guild_id: *guild_id,
            channel_id: automation_runtime_interaction::InteractionEffectChannelIdV1::new(
                channel.0,
            )
            .ok()?,
        },
        (
            InteractionEffectRecoveryTargetV1::GrantRole { target },
            RuntimeInteractionEffectDiscordObservedOutputV1::RoleMembership(present),
        ) => InteractionEffectObservedOutputV1::RoleMembership {
            target: *target,
            present,
        },
        (
            InteractionEffectRecoveryTargetV1::UpsertOverwrite { target, .. },
            RuntimeInteractionEffectDiscordObservedOutputV1::PermissionOverwrite(state),
        ) => InteractionEffectObservedOutputV1::PermissionOverwrite {
            target: *target,
            state,
        },
        (
            InteractionEffectRecoveryTargetV1::PostPanel {
                guild_id,
                channel_id,
                payload_digest,
            },
            RuntimeInteractionEffectDiscordObservedOutputV1::PostedPanel(message),
        ) => InteractionEffectObservedOutputV1::PostedMessage {
            guild_id: *guild_id,
            channel_id: *channel_id,
            message_id: automation_runtime_interaction::InteractionEffectMessageIdV1::new(
                message.0,
            )
            .ok()?,
            payload_digest: payload_digest.clone(),
        },
        _ => return None,
    };
    binding.validate_observed_output(&output).ok()?;
    Some(output)
}

fn expected_instance_postimage_v1(
    binding: &InteractionEffectRecoveryBindingV1,
) -> Option<InteractionEffectInstanceStateV1> {
    match binding.target() {
        InteractionEffectRecoveryTargetV1::RegisterInstance {
            manifest_digest, ..
        } => Some(InteractionEffectInstanceStateV1::Present {
            manifest_digest: manifest_digest.clone(),
        }),
        InteractionEffectRecoveryTargetV1::TeardownInstance { .. } => {
            Some(InteractionEffectInstanceStateV1::Absent)
        }
        _ => None,
    }
}

fn instance_preimage_v1(
    binding: &InteractionEffectRecoveryBindingV1,
) -> Option<InteractionEffectInstanceStateV1> {
    match binding.preimage() {
        InteractionEffectPreimageV1::InstanceRegistration { before, .. } => Some(before.clone()),
        _ => None,
    }
}

fn instance_target_v1(
    binding: &InteractionEffectRecoveryBindingV1,
) -> Option<automation_runtime_interaction::InteractionEffectInstanceTargetV1> {
    match binding.target() {
        InteractionEffectRecoveryTargetV1::RegisterInstance { target, .. }
        | InteractionEffectRecoveryTargetV1::TeardownInstance { target } => Some(target.clone()),
        _ => None,
    }
}

fn instance_lookup_failure_v1(
    error: InstanceStoreError,
) -> RuntimeInteractionEffectInstanceObservationOutcomeV1 {
    match error {
        InstanceStoreError::TimedOut => {
            RuntimeInteractionEffectInstanceObservationOutcomeV1::Unavailable(
                RuntimeInteractionEffectInternalReadFailureV1::TimedOut,
            )
        }
        InstanceStoreError::Backend(_) => {
            RuntimeInteractionEffectInstanceObservationOutcomeV1::Unavailable(
                RuntimeInteractionEffectInternalReadFailureV1::Unavailable,
            )
        }
        InstanceStoreError::DuplicateInstance | InstanceStoreError::NotFound => {
            RuntimeInteractionEffectInstanceObservationOutcomeV1::Conflict
        }
    }
}

fn instance_manifest_allowed_v1(
    request: &RuntimeInteractionEffectInstanceObservationRequestV1,
    actual: &InteractionInstanceManifestDigestV1,
) -> bool {
    if let Some(expected) = request.resolved_instance_manifest_digest() {
        return expected == actual
            && matches!(
                request.expected_postimage(),
                InteractionEffectInstanceStateV1::Present { .. }
            );
    }
    matches!(
        request.expected_preimage(),
        InteractionEffectInstanceStateV1::Present { manifest_digest }
            if manifest_digest.as_str() == actual.as_str()
    )
}

fn instance_registration_matches_v1(
    instance: &automation_instance::AutomationInstance,
    expected: &RuntimeInteractionEffectInstanceRegistrationIdentityV1,
    actual_manifest_digest: &InteractionInstanceManifestDigestV1,
) -> bool {
    instance.ruleset_key == expected.ruleset_key().as_str()
        && instance.ruleset_version == expected.ruleset_version()
        && &instance.kind == expected.kind()
        && instance.created_by == expected.created_by()
        && actual_manifest_digest == expected.resolved_instance_manifest_digest()
}

fn instance_registration_allowed_v1(
    instance: &automation_instance::AutomationInstance,
    request: &RuntimeInteractionEffectInstanceObservationRequestV1,
    actual_manifest_digest: &InteractionInstanceManifestDigestV1,
) -> bool {
    request.registration_identity().is_some_and(|expected| {
        instance_registration_matches_v1(instance, expected, actual_manifest_digest)
    })
}

fn instance_restore_store_failure_v1(
    error: InstanceStoreError,
) -> RuntimeInteractionEffectInstanceRestoreOutcomeV1 {
    match error {
        InstanceStoreError::TimedOut => {
            RuntimeInteractionEffectInstanceRestoreOutcomeV1::Indeterminate(
                InteractionEffectIndeterminateClassV1::DeadlineElapsed,
            )
        }
        InstanceStoreError::Backend(_) => {
            RuntimeInteractionEffectInstanceRestoreOutcomeV1::Indeterminate(
                InteractionEffectIndeterminateClassV1::ProviderUnavailable,
            )
        }
        InstanceStoreError::DuplicateInstance | InstanceStoreError::NotFound => {
            RuntimeInteractionEffectInstanceRestoreOutcomeV1::Conflict
        }
    }
}

fn instance_restore_failure_v1(
    error: TeardownError,
) -> RuntimeInteractionEffectInstanceRestoreOutcomeV1 {
    match error {
        TeardownError::Lookup(error) | TeardownError::Store(error) => {
            instance_restore_store_failure_v1(error)
        }
        TeardownError::InstanceNotFound => {
            RuntimeInteractionEffectInstanceRestoreOutcomeV1::Restored
        }
        TeardownError::ManifestDrift => RuntimeInteractionEffectInstanceRestoreOutcomeV1::Conflict,
        TeardownError::DeleteFailed { source, .. } => match source.kind {
            DeleterErrorKind::Forbidden => {
                RuntimeInteractionEffectInstanceRestoreOutcomeV1::RouteBlocked
            }
            DeleterErrorKind::RateLimited => {
                RuntimeInteractionEffectInstanceRestoreOutcomeV1::Indeterminate(
                    InteractionEffectIndeterminateClassV1::ProviderUnavailable,
                )
            }
            DeleterErrorKind::Network => {
                RuntimeInteractionEffectInstanceRestoreOutcomeV1::Indeterminate(
                    InteractionEffectIndeterminateClassV1::ConnectionLost,
                )
            }
            DeleterErrorKind::Unknown => {
                RuntimeInteractionEffectInstanceRestoreOutcomeV1::Indeterminate(
                    InteractionEffectIndeterminateClassV1::Unknown,
                )
            }
        },
    }
}

fn interaction_evidence_v1(
    evidence: DiscordEffectObservationEvidenceV1,
) -> InteractionEffectObservationEvidenceV1 {
    let mut canonical = Vec::with_capacity(DISCORD_EVIDENCE_DOMAIN_V1.len() + 12);
    canonical.extend_from_slice(DISCORD_EVIDENCE_DOMAIN_V1);
    canonical.push(correlation_code_v1(evidence.correlation_class()));
    canonical.extend_from_slice(&evidence.exact_correlation_matches().to_be_bytes());
    canonical.extend_from_slice(&evidence.conflicting_matches().to_be_bytes());
    canonical.push(u8::from(evidence.target_identity_matches()));
    canonical.push(u8::from(evidence.actor_identity_matches()));
    canonical.push(u8::from(evidence.postimage_matches()));
    InteractionEffectObservationEvidenceV1::new(
        InteractionEffectObservationEvidenceDigestV1::from_canonical_bytes(&canonical),
        evidence.correlation_class(),
        evidence.exact_correlation_matches(),
        evidence.conflicting_matches(),
        evidence.target_identity_matches(),
        evidence.actor_identity_matches(),
        evidence.postimage_matches(),
    )
}

fn exact_evidence_matches_v1(
    binding: &InteractionEffectRecoveryBindingV1,
    evidence: DiscordEffectObservationEvidenceV1,
) -> bool {
    evidence.correlation_class() == binding.correlation().class()
        && evidence.exact_correlation_matches() == 1
        && evidence.conflicting_matches() == 0
        && evidence.target_identity_matches()
        && evidence.actor_identity_matches()
        && evidence.postimage_matches()
}

fn internal_evidence_v1(
    exact: u16,
    conflicting: u16,
    target: bool,
    actor: bool,
    postimage: bool,
) -> InteractionEffectObservationEvidenceV1 {
    let mut canonical = Vec::with_capacity(INTERNAL_EVIDENCE_DOMAIN_V1.len() + 12);
    canonical.extend_from_slice(INTERNAL_EVIDENCE_DOMAIN_V1);
    canonical.extend_from_slice(&exact.to_be_bytes());
    canonical.extend_from_slice(&conflicting.to_be_bytes());
    canonical.push(u8::from(target));
    canonical.push(u8::from(actor));
    canonical.push(u8::from(postimage));
    InteractionEffectObservationEvidenceV1::new(
        InteractionEffectObservationEvidenceDigestV1::from_canonical_bytes(&canonical),
        InteractionEffectCorrelationClassV1::InternalIdempotencyKey,
        exact,
        conflicting,
        target,
        actor,
        postimage,
    )
}

fn correlation_code_v1(class: InteractionEffectCorrelationClassV1) -> u8 {
    match class {
        InteractionEffectCorrelationClassV1::AuditLogReason => 1,
        InteractionEffectCorrelationClassV1::MessageNonce => 2,
        InteractionEffectCorrelationClassV1::InternalIdempotencyKey => 3,
        InteractionEffectCorrelationClassV1::InteractionReceipt => 4,
        InteractionEffectCorrelationClassV1::Unsupported => 5,
    }
}

fn discord_overwrite_target_v1(target: InteractionEffectOverwriteTargetV1) -> OverwriteTarget {
    match target {
        InteractionEffectOverwriteTargetV1::Role(role) => OverwriteTarget::Role(RoleId(role.get())),
        InteractionEffectOverwriteTargetV1::Member(member) => {
            OverwriteTarget::Member(UserId(member.get()))
        }
    }
}

fn protocol_observation_v1() -> RuntimeInteractionEffectRecoveryObservationDispositionV1 {
    RuntimeInteractionEffectRecoveryObservationDispositionV1::RecoveryRequired(
        RuntimeInteractionEffectRecoveryRequiredV1::ObservationProtocol,
    )
}

fn protocol_compensation_observation_v1(
) -> RuntimeInteractionEffectRecoveryCompensationObservationDispositionV1 {
    RuntimeInteractionEffectRecoveryCompensationObservationDispositionV1::RecoveryRequired(
        RuntimeInteractionEffectRecoveryRequiredV1::ObservationProtocol,
    )
}

#[cfg(test)]
mod tests;
