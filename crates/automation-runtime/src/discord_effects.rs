use automation_core::{CreateChannelSpec, CreateRoleSpec, DiscordMutationAdapter, PostPanelSpec};
use automation_runtime_interaction::{
    InteractionEffectCorrelationClassV1, InteractionEffectCorrelationV1,
    InteractionEffectExpectedPostimageDigestV1, InteractionEffectIndeterminateClassV1,
    InteractionEffectKnownFailureClassV1, InteractionEffectKnownFailureV1,
    InteractionEffectPermissionStateV1, InteractionEffectPermissionValueV1,
};
use discord_model::{ChannelId, GuildId, MessageId, OverwriteTarget, Permissions, RoleId, UserId};
use twilight_http::{error::ErrorType, request::AuditLogReason, response::DeserializeBodyError};
use twilight_model::{
    channel::{permission_overwrite::PermissionOverwriteType, ChannelType},
    guild::audit_log::{AuditLogEntry, AuditLogEventType},
    id::Id,
};

use crate::discord_effect_postimage::{
    created_channel_postimage_digest_v1, created_role_postimage_digest_v1,
    overwrite_postimage_digest_v1, panel_postimage_digest_v1, role_membership_postimage_digest_v1,
};
use crate::mutation::{to_button_component, TwilightMutationAdapter};

const AUDIT_REASON_PREFIX_V1: &str = "starring-effect-v1:";
const COMPENSATION_REASON_PREFIX_V1: &str = "starring-effect-compensation-v1:";
const AUDIT_OBSERVATION_LIMIT_V1: u16 = 100;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum DiscordEffectCorrelationErrorV1 {
    UnsupportedClass,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum DiscordEffectOverwriteDecodeErrorV1 {
    OverlappingPermissions,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DiscordEffectAttemptOutcomeV1<T> {
    KnownSucceeded(T),
    KnownFailed(InteractionEffectKnownFailureV1),
    Indeterminate(InteractionEffectIndeterminateClassV1),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DiscordEffectReadFailureV1 {
    KnownFailed(InteractionEffectKnownFailureV1),
    Indeterminate(InteractionEffectIndeterminateClassV1),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DiscordEffectObservationEvidenceV1 {
    correlation_class: InteractionEffectCorrelationClassV1,
    exact_correlation_matches: u16,
    conflicting_matches: u16,
    target_identity_matches: bool,
    actor_identity_matches: bool,
    postimage_matches: bool,
}

impl DiscordEffectObservationEvidenceV1 {
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn new(
        correlation_class: InteractionEffectCorrelationClassV1,
        exact_correlation_matches: u16,
        conflicting_matches: u16,
        target_identity_matches: bool,
        actor_identity_matches: bool,
        postimage_matches: bool,
    ) -> Self {
        Self {
            correlation_class,
            exact_correlation_matches,
            conflicting_matches,
            target_identity_matches,
            actor_identity_matches,
            postimage_matches,
        }
    }

    pub fn correlation_class(self) -> InteractionEffectCorrelationClassV1 {
        self.correlation_class
    }

    pub fn exact_correlation_matches(self) -> u16 {
        self.exact_correlation_matches
    }

    pub fn conflicting_matches(self) -> u16 {
        self.conflicting_matches
    }

    pub fn target_identity_matches(self) -> bool {
        self.target_identity_matches
    }

    pub fn actor_identity_matches(self) -> bool {
        self.actor_identity_matches
    }

    pub fn postimage_matches(self) -> bool {
        self.postimage_matches
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DiscordEffectObservationOutcomeV1<T> {
    ExactMatch {
        output: T,
        evidence: DiscordEffectObservationEvidenceV1,
    },
    Pending {
        evidence: DiscordEffectObservationEvidenceV1,
    },
    Conflict {
        evidence: DiscordEffectObservationEvidenceV1,
    },
    Unsupported {
        evidence: DiscordEffectObservationEvidenceV1,
    },
    Unavailable(DiscordEffectReadFailureV1),
}

impl<T> DiscordEffectObservationOutcomeV1<T> {
    pub fn evidence(&self) -> Option<DiscordEffectObservationEvidenceV1> {
        match self {
            Self::ExactMatch { evidence, .. }
            | Self::Pending { evidence }
            | Self::Conflict { evidence }
            | Self::Unsupported { evidence } => Some(*evidence),
            Self::Unavailable(_) => None,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DiscordEffectCompensationObservationOutcomeV1 {
    Restored {
        evidence: DiscordEffectObservationEvidenceV1,
    },
    Pending {
        evidence: DiscordEffectObservationEvidenceV1,
    },
    Conflict {
        evidence: DiscordEffectObservationEvidenceV1,
    },
    Unsupported {
        evidence: DiscordEffectObservationEvidenceV1,
    },
    Unavailable(DiscordEffectReadFailureV1),
}

impl DiscordEffectCompensationObservationOutcomeV1 {
    pub fn evidence(self) -> Option<DiscordEffectObservationEvidenceV1> {
        match self {
            Self::Restored { evidence }
            | Self::Pending { evidence }
            | Self::Conflict { evidence }
            | Self::Unsupported { evidence } => Some(evidence),
            Self::Unavailable(_) => None,
        }
    }
}

#[allow(async_fn_in_trait)]
pub trait DiscordEffectCompensationObserverV1 {
    async fn observe_compensated_created_role_effect_v1(
        &self,
        guild: GuildId,
        bot_user: UserId,
        role: RoleId,
        expected_postimage: &InteractionEffectExpectedPostimageDigestV1,
        correlation: &InteractionEffectCorrelationV1,
    ) -> DiscordEffectCompensationObservationOutcomeV1;

    async fn observe_compensated_created_channel_effect_v1(
        &self,
        guild: GuildId,
        bot_user: UserId,
        channel: ChannelId,
        expected_postimage: &InteractionEffectExpectedPostimageDigestV1,
        correlation: &InteractionEffectCorrelationV1,
    ) -> DiscordEffectCompensationObservationOutcomeV1;

    #[allow(clippy::too_many_arguments)]
    async fn observe_compensated_role_membership_effect_v1(
        &self,
        guild: GuildId,
        bot_user: UserId,
        member: UserId,
        role: RoleId,
        before_present: bool,
        expected_postimage: &InteractionEffectExpectedPostimageDigestV1,
        correlation: &InteractionEffectCorrelationV1,
    ) -> DiscordEffectCompensationObservationOutcomeV1;

    #[allow(clippy::too_many_arguments)]
    async fn observe_compensated_overwrite_effect_v1(
        &self,
        guild: GuildId,
        bot_user: UserId,
        channel: ChannelId,
        target: OverwriteTarget,
        before: InteractionEffectPermissionStateV1,
        expected_postimage: &InteractionEffectExpectedPostimageDigestV1,
        correlation: &InteractionEffectCorrelationV1,
    ) -> DiscordEffectCompensationObservationOutcomeV1;

    #[allow(clippy::too_many_arguments)]
    async fn observe_compensated_posted_panel_effect_v1(
        &self,
        guild: GuildId,
        channel: ChannelId,
        message: MessageId,
        bot_user: UserId,
        expected_postimage: &InteractionEffectExpectedPostimageDigestV1,
        correlation: &InteractionEffectCorrelationV1,
    ) -> DiscordEffectCompensationObservationOutcomeV1;
}

#[allow(async_fn_in_trait)]
pub trait RecoverableDiscordMutationAdapterV1: DiscordMutationAdapter {
    async fn create_role_effect_v1(
        &self,
        guild: GuildId,
        spec: CreateRoleSpec,
        correlation: &InteractionEffectCorrelationV1,
    ) -> DiscordEffectAttemptOutcomeV1<RoleId>;

    async fn create_channel_effect_v1(
        &self,
        guild: GuildId,
        spec: CreateChannelSpec,
        correlation: &InteractionEffectCorrelationV1,
    ) -> DiscordEffectAttemptOutcomeV1<ChannelId>;

    async fn grant_role_effect_v1(
        &self,
        guild: GuildId,
        member: UserId,
        role: RoleId,
        correlation: &InteractionEffectCorrelationV1,
    ) -> DiscordEffectAttemptOutcomeV1<()>;

    #[allow(clippy::too_many_arguments)]
    async fn upsert_overwrite_effect_v1(
        &self,
        guild: GuildId,
        channel: ChannelId,
        target: OverwriteTarget,
        allow: Permissions,
        deny: Permissions,
        correlation: &InteractionEffectCorrelationV1,
    ) -> DiscordEffectAttemptOutcomeV1<()>;

    async fn post_panel_effect_v1(
        &self,
        guild: GuildId,
        channel: ChannelId,
        spec: PostPanelSpec,
        correlation: &InteractionEffectCorrelationV1,
    ) -> DiscordEffectAttemptOutcomeV1<MessageId>;

    async fn observe_created_role_effect_v1(
        &self,
        guild: GuildId,
        bot_user: UserId,
        expected_postimage: &InteractionEffectExpectedPostimageDigestV1,
        correlation: &InteractionEffectCorrelationV1,
    ) -> DiscordEffectObservationOutcomeV1<RoleId>;

    async fn observe_created_channel_effect_v1(
        &self,
        guild: GuildId,
        bot_user: UserId,
        expected_postimage: &InteractionEffectExpectedPostimageDigestV1,
        correlation: &InteractionEffectCorrelationV1,
    ) -> DiscordEffectObservationOutcomeV1<ChannelId>;

    async fn observe_role_membership_effect_v1(
        &self,
        guild: GuildId,
        bot_user: UserId,
        member: UserId,
        role: RoleId,
        expected_postimage: &InteractionEffectExpectedPostimageDigestV1,
        correlation: &InteractionEffectCorrelationV1,
    ) -> DiscordEffectObservationOutcomeV1<bool>;

    #[allow(clippy::too_many_arguments)]
    async fn observe_overwrite_effect_v1(
        &self,
        guild: GuildId,
        bot_user: UserId,
        channel: ChannelId,
        target: OverwriteTarget,
        expected_postimage: &InteractionEffectExpectedPostimageDigestV1,
        correlation: &InteractionEffectCorrelationV1,
    ) -> DiscordEffectObservationOutcomeV1<InteractionEffectPermissionStateV1>;

    async fn observe_post_panel_effect_v1(
        &self,
        expected_postimage: &InteractionEffectExpectedPostimageDigestV1,
        correlation: &InteractionEffectCorrelationV1,
    ) -> DiscordEffectObservationOutcomeV1<MessageId>;

    async fn compensate_created_role_effect_v1(
        &self,
        guild: GuildId,
        bot_user: UserId,
        role: RoleId,
        expected_postimage: &InteractionEffectExpectedPostimageDigestV1,
        correlation: &InteractionEffectCorrelationV1,
    ) -> DiscordEffectAttemptOutcomeV1<()>;

    async fn compensate_created_channel_effect_v1(
        &self,
        guild: GuildId,
        bot_user: UserId,
        channel: ChannelId,
        expected_postimage: &InteractionEffectExpectedPostimageDigestV1,
        correlation: &InteractionEffectCorrelationV1,
    ) -> DiscordEffectAttemptOutcomeV1<()>;

    #[allow(clippy::too_many_arguments)]
    async fn compensate_role_membership_effect_v1(
        &self,
        guild: GuildId,
        bot_user: UserId,
        member: UserId,
        role: RoleId,
        before_present: bool,
        expected_postimage: &InteractionEffectExpectedPostimageDigestV1,
        correlation: &InteractionEffectCorrelationV1,
    ) -> DiscordEffectAttemptOutcomeV1<()>;

    #[allow(clippy::too_many_arguments)]
    async fn compensate_overwrite_effect_v1(
        &self,
        guild: GuildId,
        bot_user: UserId,
        channel: ChannelId,
        target: OverwriteTarget,
        expected_postimage: &InteractionEffectExpectedPostimageDigestV1,
        before: InteractionEffectPermissionStateV1,
        correlation: &InteractionEffectCorrelationV1,
    ) -> DiscordEffectAttemptOutcomeV1<()>;

    #[allow(clippy::too_many_arguments)]
    async fn compensate_posted_panel_effect_v1(
        &self,
        guild: GuildId,
        channel: ChannelId,
        message: MessageId,
        bot_user: UserId,
        expected_postimage: &InteractionEffectExpectedPostimageDigestV1,
        correlation: &InteractionEffectCorrelationV1,
    ) -> DiscordEffectAttemptOutcomeV1<()>;
}

impl RecoverableDiscordMutationAdapterV1 for TwilightMutationAdapter<'_> {
    async fn create_role_effect_v1(
        &self,
        guild: GuildId,
        spec: CreateRoleSpec,
        correlation: &InteractionEffectCorrelationV1,
    ) -> DiscordEffectAttemptOutcomeV1<RoleId> {
        let reason = match audit_reason_v1(correlation) {
            Ok(reason) => reason,
            Err(_) => return invalid_request_v1(),
        };
        let response = self
            .http
            .create_role(Id::new(guild.0))
            .name(&spec.name)
            .permissions(twilight_model::guild::Permissions::empty())
            .reason(&reason)
            .await;
        match response {
            Ok(response) => match response.model().await {
                Ok(role) => DiscordEffectAttemptOutcomeV1::KnownSucceeded(RoleId(role.id.get())),
                Err(error) => body_failure_v1(&error),
            },
            Err(error) => request_failure_v1(&error),
        }
    }

    async fn create_channel_effect_v1(
        &self,
        guild: GuildId,
        spec: CreateChannelSpec,
        correlation: &InteractionEffectCorrelationV1,
    ) -> DiscordEffectAttemptOutcomeV1<ChannelId> {
        let reason = match audit_reason_v1(correlation) {
            Ok(reason) => reason,
            Err(_) => return invalid_request_v1(),
        };
        let response = self
            .http
            .create_guild_channel(Id::new(guild.0), &spec.name)
            .reason(&reason)
            .await;
        match response {
            Ok(response) => match response.model().await {
                Ok(channel) => {
                    DiscordEffectAttemptOutcomeV1::KnownSucceeded(ChannelId(channel.id.get()))
                }
                Err(error) => body_failure_v1(&error),
            },
            Err(error) => request_failure_v1(&error),
        }
    }

    async fn grant_role_effect_v1(
        &self,
        guild: GuildId,
        member: UserId,
        role: RoleId,
        correlation: &InteractionEffectCorrelationV1,
    ) -> DiscordEffectAttemptOutcomeV1<()> {
        let reason = match audit_reason_v1(correlation) {
            Ok(reason) => reason,
            Err(_) => return invalid_request_v1(),
        };
        unit_request_outcome_v1(
            self.http
                .add_guild_member_role(Id::new(guild.0), Id::new(member.0), Id::new(role.0))
                .reason(&reason)
                .await,
        )
    }

    async fn upsert_overwrite_effect_v1(
        &self,
        _guild: GuildId,
        channel: ChannelId,
        target: OverwriteTarget,
        allow: Permissions,
        deny: Permissions,
        correlation: &InteractionEffectCorrelationV1,
    ) -> DiscordEffectAttemptOutcomeV1<()> {
        let reason = match audit_reason_v1(correlation) {
            Ok(reason) => reason,
            Err(_) => return invalid_request_v1(),
        };
        let overwrite = crate::mutation::to_permission_overwrite(target, allow, deny);
        unit_request_outcome_v1(
            self.http
                .update_channel_permission(Id::new(channel.0), &overwrite)
                .reason(&reason)
                .await,
        )
    }

    async fn post_panel_effect_v1(
        &self,
        guild: GuildId,
        channel: ChannelId,
        spec: PostPanelSpec,
        correlation: &InteractionEffectCorrelationV1,
    ) -> DiscordEffectAttemptOutcomeV1<MessageId> {
        if correlation.class() != InteractionEffectCorrelationClassV1::Unsupported {
            return invalid_request_v1();
        }
        let components = match panel_components_v1(guild, &self.ruleset_key, &spec) {
            Ok(components) => components,
            Err(()) => return invalid_request_v1(),
        };
        let response = self
            .http
            .create_message(Id::new(channel.0))
            .content(&spec.content)
            .components(&components)
            .await;
        match response {
            Ok(response) => match response.model().await {
                Ok(message) => {
                    DiscordEffectAttemptOutcomeV1::KnownSucceeded(MessageId(message.id.get()))
                }
                Err(error) => body_failure_v1(&error),
            },
            Err(error) => request_failure_v1(&error),
        }
    }

    async fn observe_created_role_effect_v1(
        &self,
        guild: GuildId,
        bot_user: UserId,
        expected_postimage: &InteractionEffectExpectedPostimageDigestV1,
        correlation: &InteractionEffectCorrelationV1,
    ) -> DiscordEffectObservationOutcomeV1<RoleId> {
        let targets = match self
            .exact_audit_targets_v1(guild, bot_user, AuditLogEventType::RoleCreate, correlation)
            .await
        {
            Ok(targets) => targets,
            Err(outcome) => return read_failure_as_observation_v1(outcome),
        };
        let target = match unique_audit_target_v1(&targets, None) {
            Ok(Some(target)) => target,
            Ok(None) => return pending_audit_v1(),
            Err(evidence) => return DiscordEffectObservationOutcomeV1::Conflict { evidence },
        };
        let role = match self.http.role(Id::new(guild.0), Id::new(target)).await {
            Ok(response) => match response.model().await {
                Ok(role) => role,
                Err(error) => return unavailable_body_v1(&error),
            },
            Err(error) if is_not_found_v1(&error) => {
                return conflict_audit_v1(targets.len(), true, true, false);
            }
            Err(error) => return unavailable_request_v1(&error),
        };
        if role.id.get() != target
            || created_role_postimage_digest_v1(&role.name, role.permissions.bits(), role.managed)
                != *expected_postimage
        {
            return conflict_audit_v1(targets.len(), true, true, false);
        }
        exact_audit_v1(RoleId(target))
    }

    async fn observe_created_channel_effect_v1(
        &self,
        guild: GuildId,
        bot_user: UserId,
        expected_postimage: &InteractionEffectExpectedPostimageDigestV1,
        correlation: &InteractionEffectCorrelationV1,
    ) -> DiscordEffectObservationOutcomeV1<ChannelId> {
        let targets = match self
            .exact_audit_targets_v1(
                guild,
                bot_user,
                AuditLogEventType::ChannelCreate,
                correlation,
            )
            .await
        {
            Ok(targets) => targets,
            Err(outcome) => return read_failure_as_observation_v1(outcome),
        };
        let target = match unique_audit_target_v1(&targets, None) {
            Ok(Some(target)) => target,
            Ok(None) => return pending_audit_v1(),
            Err(evidence) => return DiscordEffectObservationOutcomeV1::Conflict { evidence },
        };
        let channel = match self.http.channel(Id::new(target)).await {
            Ok(response) => match response.model().await {
                Ok(channel) => channel,
                Err(error) => return unavailable_body_v1(&error),
            },
            Err(error) if is_not_found_v1(&error) => {
                return conflict_audit_v1(targets.len(), true, true, false);
            }
            Err(error) => return unavailable_request_v1(&error),
        };
        if channel.id.get() != target
            || channel.guild_id.map(Id::get) != Some(guild.0)
            || channel.kind != ChannelType::GuildText
            || channel.name.as_deref().is_none_or(|name| {
                created_channel_postimage_digest_v1(name, "text") != *expected_postimage
            })
        {
            return conflict_audit_v1(targets.len(), true, true, false);
        }
        exact_audit_v1(ChannelId(target))
    }

    async fn observe_role_membership_effect_v1(
        &self,
        guild: GuildId,
        bot_user: UserId,
        member: UserId,
        role: RoleId,
        expected_postimage: &InteractionEffectExpectedPostimageDigestV1,
        correlation: &InteractionEffectCorrelationV1,
    ) -> DiscordEffectObservationOutcomeV1<bool> {
        let targets = match self
            .exact_audit_targets_v1(
                guild,
                bot_user,
                AuditLogEventType::MemberRoleUpdate,
                correlation,
            )
            .await
        {
            Ok(targets) => targets,
            Err(outcome) => return read_failure_as_observation_v1(outcome),
        };
        match unique_audit_target_v1(&targets, Some(member.0)) {
            Ok(Some(_)) => {}
            Ok(None) => return pending_audit_v1(),
            Err(evidence) => return DiscordEffectObservationOutcomeV1::Conflict { evidence },
        }
        let observed_member = match self
            .http
            .guild_member(Id::new(guild.0), Id::new(member.0))
            .await
        {
            Ok(response) => match response.model().await {
                Ok(member) => member,
                Err(error) => return unavailable_body_v1(&error),
            },
            Err(error) if is_not_found_v1(&error) => {
                return conflict_audit_v1(targets.len(), true, true, false);
            }
            Err(error) => return unavailable_request_v1(&error),
        };
        let present = observed_member.roles.iter().any(|id| id.get() == role.0);
        if role_membership_postimage_digest_v1(present) != *expected_postimage {
            return conflict_audit_v1(targets.len(), true, true, false);
        }
        exact_audit_v1(present)
    }

    async fn observe_overwrite_effect_v1(
        &self,
        guild: GuildId,
        bot_user: UserId,
        channel: ChannelId,
        target: OverwriteTarget,
        expected_postimage: &InteractionEffectExpectedPostimageDigestV1,
        correlation: &InteractionEffectCorrelationV1,
    ) -> DiscordEffectObservationOutcomeV1<InteractionEffectPermissionStateV1> {
        let targets = match self
            .exact_overwrite_audit_targets_v1(guild, bot_user, correlation)
            .await
        {
            Ok(targets) => targets,
            Err(outcome) => return read_failure_as_observation_v1(outcome),
        };
        match unique_audit_target_v1(&targets, Some(channel.0)) {
            Ok(Some(_)) => {}
            Ok(None) => return pending_audit_v1(),
            Err(evidence) => return DiscordEffectObservationOutcomeV1::Conflict { evidence },
        }
        let channel_model = match self.http.channel(Id::new(channel.0)).await {
            Ok(response) => match response.model().await {
                Ok(channel) => channel,
                Err(error) => return unavailable_body_v1(&error),
            },
            Err(error) if is_not_found_v1(&error) => {
                return conflict_audit_v1(targets.len(), true, true, false);
            }
            Err(error) => return unavailable_request_v1(&error),
        };
        let observed = match overwrite_state_v1(&channel_model, target) {
            Ok(observed) => observed,
            Err(_) => return malformed_overwrite_observation_v1(targets.len()),
        };
        if permission_state_postimage_digest_v1(observed) != Some(expected_postimage.clone()) {
            return conflict_audit_v1(targets.len(), true, true, false);
        }
        exact_audit_v1(observed)
    }

    async fn observe_post_panel_effect_v1(
        &self,
        _expected_postimage: &InteractionEffectExpectedPostimageDigestV1,
        correlation: &InteractionEffectCorrelationV1,
    ) -> DiscordEffectObservationOutcomeV1<MessageId> {
        let class_matches = correlation.class() == InteractionEffectCorrelationClassV1::Unsupported;
        DiscordEffectObservationOutcomeV1::Unsupported {
            evidence: DiscordEffectObservationEvidenceV1 {
                correlation_class: InteractionEffectCorrelationClassV1::Unsupported,
                exact_correlation_matches: 0,
                conflicting_matches: u16::from(!class_matches),
                target_identity_matches: false,
                actor_identity_matches: false,
                postimage_matches: false,
            },
        }
    }

    async fn compensate_created_role_effect_v1(
        &self,
        guild: GuildId,
        bot_user: UserId,
        role: RoleId,
        expected_postimage: &InteractionEffectExpectedPostimageDigestV1,
        correlation: &InteractionEffectCorrelationV1,
    ) -> DiscordEffectAttemptOutcomeV1<()> {
        match self.http.role(Id::new(guild.0), Id::new(role.0)).await {
            Ok(response) => match response.model().await {
                Ok(observed)
                    if observed.id.get() == role.0
                        && created_role_postimage_digest_v1(
                            &observed.name,
                            observed.permissions.bits(),
                            observed.managed,
                        ) == *expected_postimage => {}
                Ok(_) => return conflict_v1(),
                Err(error) => return body_failure_v1(&error),
            },
            Err(error) if is_not_found_v1(&error) => {
                return DiscordEffectAttemptOutcomeV1::KnownSucceeded(());
            }
            Err(error) => return request_failure_v1(&error),
        }
        match self
            .observe_created_role_effect_v1(guild, bot_user, expected_postimage, correlation)
            .await
        {
            DiscordEffectObservationOutcomeV1::ExactMatch { output, .. } if output == role => {}
            DiscordEffectObservationOutcomeV1::Unavailable(error) => {
                return read_failure_as_attempt_v1(error);
            }
            _ => return conflict_v1(),
        }
        let reason = match compensation_reason_v1(correlation) {
            Ok(reason) => reason,
            Err(_) => return invalid_request_v1(),
        };
        compensation_delete_outcome_v1(
            self.http
                .delete_role(Id::new(guild.0), Id::new(role.0))
                .reason(&reason)
                .await,
        )
    }

    async fn compensate_created_channel_effect_v1(
        &self,
        guild: GuildId,
        bot_user: UserId,
        channel: ChannelId,
        expected_postimage: &InteractionEffectExpectedPostimageDigestV1,
        correlation: &InteractionEffectCorrelationV1,
    ) -> DiscordEffectAttemptOutcomeV1<()> {
        match self.http.channel(Id::new(channel.0)).await {
            Ok(response) => match response.model().await {
                Ok(observed)
                    if observed.id.get() == channel.0
                        && observed.guild_id.map(Id::get) == Some(guild.0)
                        && observed.kind == ChannelType::GuildText
                        && observed.name.as_deref().is_some_and(|name| {
                            created_channel_postimage_digest_v1(name, "text") == *expected_postimage
                        }) => {}
                Ok(_) => return conflict_v1(),
                Err(error) => return body_failure_v1(&error),
            },
            Err(error) if is_not_found_v1(&error) => {
                return DiscordEffectAttemptOutcomeV1::KnownSucceeded(());
            }
            Err(error) => return request_failure_v1(&error),
        }
        match self
            .observe_created_channel_effect_v1(guild, bot_user, expected_postimage, correlation)
            .await
        {
            DiscordEffectObservationOutcomeV1::ExactMatch { output, .. } if output == channel => {}
            DiscordEffectObservationOutcomeV1::Unavailable(error) => {
                return read_failure_as_attempt_v1(error);
            }
            _ => return conflict_v1(),
        }
        let reason = match compensation_reason_v1(correlation) {
            Ok(reason) => reason,
            Err(_) => return invalid_request_v1(),
        };
        compensation_delete_outcome_v1(
            self.http
                .delete_channel(Id::new(channel.0))
                .reason(&reason)
                .await,
        )
    }

    async fn compensate_role_membership_effect_v1(
        &self,
        guild: GuildId,
        bot_user: UserId,
        member: UserId,
        role: RoleId,
        before_present: bool,
        expected_postimage: &InteractionEffectExpectedPostimageDigestV1,
        correlation: &InteractionEffectCorrelationV1,
    ) -> DiscordEffectAttemptOutcomeV1<()> {
        let current = match self.current_role_membership_v1(guild, member, role).await {
            Ok(current) => current,
            Err(error) => return read_failure_as_attempt_v1(error),
        };
        if current == before_present {
            return DiscordEffectAttemptOutcomeV1::KnownSucceeded(());
        }
        if role_membership_postimage_digest_v1(current) != *expected_postimage {
            return conflict_v1();
        }
        match self
            .observe_role_membership_effect_v1(
                guild,
                bot_user,
                member,
                role,
                expected_postimage,
                correlation,
            )
            .await
        {
            DiscordEffectObservationOutcomeV1::ExactMatch { output: true, .. } => {}
            DiscordEffectObservationOutcomeV1::Unavailable(error) => {
                return read_failure_as_attempt_v1(error);
            }
            _ => return conflict_v1(),
        }
        let reason = match compensation_reason_v1(correlation) {
            Ok(reason) => reason,
            Err(_) => return invalid_request_v1(),
        };
        unit_request_outcome_v1(
            self.http
                .remove_guild_member_role(Id::new(guild.0), Id::new(member.0), Id::new(role.0))
                .reason(&reason)
                .await,
        )
    }

    async fn compensate_overwrite_effect_v1(
        &self,
        guild: GuildId,
        bot_user: UserId,
        channel: ChannelId,
        target: OverwriteTarget,
        expected_postimage: &InteractionEffectExpectedPostimageDigestV1,
        before: InteractionEffectPermissionStateV1,
        correlation: &InteractionEffectCorrelationV1,
    ) -> DiscordEffectAttemptOutcomeV1<()> {
        let current = match self.current_overwrite_v1(channel, target).await {
            Ok(current) => current,
            Err(error) => return read_failure_as_attempt_v1(error),
        };
        if current == before {
            return DiscordEffectAttemptOutcomeV1::KnownSucceeded(());
        }
        if permission_state_postimage_digest_v1(current) != Some(expected_postimage.clone()) {
            return conflict_v1();
        }
        match self
            .observe_overwrite_effect_v1(
                guild,
                bot_user,
                channel,
                target,
                expected_postimage,
                correlation,
            )
            .await
        {
            DiscordEffectObservationOutcomeV1::ExactMatch { .. } => {}
            DiscordEffectObservationOutcomeV1::Unavailable(error) => {
                return read_failure_as_attempt_v1(error);
            }
            _ => return conflict_v1(),
        }
        let reason = match compensation_reason_v1(correlation) {
            Ok(reason) => reason,
            Err(_) => return invalid_request_v1(),
        };
        match before {
            InteractionEffectPermissionStateV1::Absent => {
                compensation_delete_outcome_v1(match target {
                    OverwriteTarget::Role(role) => {
                        self.http
                            .delete_channel_permission(Id::new(channel.0))
                            .role(Id::new(role.0))
                            .reason(&reason)
                            .await
                    }
                    OverwriteTarget::Member(member) => {
                        self.http
                            .delete_channel_permission(Id::new(channel.0))
                            .member(Id::new(member.0))
                            .reason(&reason)
                            .await
                    }
                })
            }
            InteractionEffectPermissionStateV1::Present(value) => {
                let overwrite = crate::mutation::to_permission_overwrite(
                    target,
                    Permissions::from_bits_retain(value.allow()),
                    Permissions::from_bits_retain(value.deny()),
                );
                unit_request_outcome_v1(
                    self.http
                        .update_channel_permission(Id::new(channel.0), &overwrite)
                        .reason(&reason)
                        .await,
                )
            }
        }
    }

    async fn compensate_posted_panel_effect_v1(
        &self,
        guild: GuildId,
        channel: ChannelId,
        message: MessageId,
        bot_user: UserId,
        expected_postimage: &InteractionEffectExpectedPostimageDigestV1,
        correlation: &InteractionEffectCorrelationV1,
    ) -> DiscordEffectAttemptOutcomeV1<()> {
        if correlation.class() != InteractionEffectCorrelationClassV1::Unsupported {
            return invalid_request_v1();
        }
        let observed = match self
            .http
            .message(Id::new(channel.0), Id::new(message.0))
            .await
        {
            Ok(response) => match response.model().await {
                Ok(message) => message,
                Err(error) => return body_failure_v1(&error),
            },
            Err(error) if is_not_found_v1(&error) => {
                return DiscordEffectAttemptOutcomeV1::KnownSucceeded(());
            }
            Err(error) => return request_failure_v1(&error),
        };
        if observed.id.get() != message.0
            || observed.channel_id.get() != channel.0
            || observed.guild_id.map(Id::get) != Some(guild.0)
            || observed.author.id.get() != bot_user.0
            || observed_panel_postimage_digest_v1(&observed) != Ok(expected_postimage.clone())
        {
            return conflict_v1();
        }
        compensation_delete_outcome_v1(
            self.http
                .delete_message(Id::new(channel.0), Id::new(message.0))
                .await,
        )
    }
}

impl DiscordEffectCompensationObserverV1 for TwilightMutationAdapter<'_> {
    async fn observe_compensated_created_role_effect_v1(
        &self,
        guild: GuildId,
        _bot_user: UserId,
        role: RoleId,
        expected_postimage: &InteractionEffectExpectedPostimageDigestV1,
        correlation: &InteractionEffectCorrelationV1,
    ) -> DiscordEffectCompensationObservationOutcomeV1 {
        if correlation.class() != InteractionEffectCorrelationClassV1::AuditLogReason {
            return unsupported_compensation_observation_v1(correlation.class());
        }
        let observed = match self.http.role(Id::new(guild.0), Id::new(role.0)).await {
            Ok(response) => match response.model().await {
                Ok(observed) => observed,
                Err(error) => return unavailable_compensation_body_v1(&error),
            },
            Err(error) if is_not_found_v1(&error) => {
                return restored_compensation_observation_v1(correlation.class(), true, false);
            }
            Err(error) => return unavailable_compensation_request_v1(&error),
        };
        let target_matches = observed.id.get() == role.0;
        let applied_postimage_matches = target_matches
            && created_role_postimage_digest_v1(
                &observed.name,
                observed.permissions.bits(),
                observed.managed,
            ) == *expected_postimage;
        classify_compensation_observation_v1(
            correlation.class(),
            target_matches,
            false,
            false,
            applied_postimage_matches,
        )
    }

    async fn observe_compensated_created_channel_effect_v1(
        &self,
        guild: GuildId,
        _bot_user: UserId,
        channel: ChannelId,
        expected_postimage: &InteractionEffectExpectedPostimageDigestV1,
        correlation: &InteractionEffectCorrelationV1,
    ) -> DiscordEffectCompensationObservationOutcomeV1 {
        if correlation.class() != InteractionEffectCorrelationClassV1::AuditLogReason {
            return unsupported_compensation_observation_v1(correlation.class());
        }
        let observed = match self.http.channel(Id::new(channel.0)).await {
            Ok(response) => match response.model().await {
                Ok(observed) => observed,
                Err(error) => return unavailable_compensation_body_v1(&error),
            },
            Err(error) if is_not_found_v1(&error) => {
                return restored_compensation_observation_v1(correlation.class(), true, false);
            }
            Err(error) => return unavailable_compensation_request_v1(&error),
        };
        let target_matches = observed.id.get() == channel.0
            && observed.guild_id.map(Id::get) == Some(guild.0)
            && observed.kind == ChannelType::GuildText;
        let applied_postimage_matches = target_matches
            && observed.name.as_deref().is_some_and(|name| {
                created_channel_postimage_digest_v1(name, "text") == *expected_postimage
            });
        classify_compensation_observation_v1(
            correlation.class(),
            target_matches,
            false,
            false,
            applied_postimage_matches,
        )
    }

    async fn observe_compensated_role_membership_effect_v1(
        &self,
        guild: GuildId,
        _bot_user: UserId,
        member: UserId,
        role: RoleId,
        before_present: bool,
        expected_postimage: &InteractionEffectExpectedPostimageDigestV1,
        correlation: &InteractionEffectCorrelationV1,
    ) -> DiscordEffectCompensationObservationOutcomeV1 {
        if correlation.class() != InteractionEffectCorrelationClassV1::AuditLogReason {
            return unsupported_compensation_observation_v1(correlation.class());
        }
        let current = match self.current_role_membership_v1(guild, member, role).await {
            Ok(current) => current,
            Err(DiscordEffectReadFailureV1::KnownFailed(failure))
                if failure.class() == InteractionEffectKnownFailureClassV1::NotFound =>
            {
                return conflict_compensation_observation_v1(
                    correlation.class(),
                    false,
                    false,
                    false,
                );
            }
            Err(error) => return unavailable_compensation_read_v1(error),
        };
        classify_compensation_observation_v1(
            correlation.class(),
            true,
            false,
            current == before_present,
            role_membership_postimage_digest_v1(current) == *expected_postimage,
        )
    }

    async fn observe_compensated_overwrite_effect_v1(
        &self,
        guild: GuildId,
        _bot_user: UserId,
        channel: ChannelId,
        target: OverwriteTarget,
        before: InteractionEffectPermissionStateV1,
        expected_postimage: &InteractionEffectExpectedPostimageDigestV1,
        correlation: &InteractionEffectCorrelationV1,
    ) -> DiscordEffectCompensationObservationOutcomeV1 {
        if correlation.class() != InteractionEffectCorrelationClassV1::AuditLogReason {
            return unsupported_compensation_observation_v1(correlation.class());
        }
        let observed = match self.http.channel(Id::new(channel.0)).await {
            Ok(response) => match response.model().await {
                Ok(observed) => observed,
                Err(error) => return unavailable_compensation_body_v1(&error),
            },
            Err(error) if is_not_found_v1(&error) => {
                return conflict_compensation_observation_v1(
                    correlation.class(),
                    false,
                    false,
                    false,
                );
            }
            Err(error) => return unavailable_compensation_request_v1(&error),
        };
        let target_matches = observed.id.get() == channel.0
            && observed.guild_id.map(Id::get) == Some(guild.0)
            && observed.kind == ChannelType::GuildText;
        if !target_matches {
            return conflict_compensation_observation_v1(correlation.class(), false, false, false);
        }
        let current = match overwrite_state_v1(&observed, target) {
            Ok(current) => current,
            Err(_) => {
                return malformed_overwrite_compensation_observation_v1(correlation.class());
            }
        };
        classify_compensation_observation_v1(
            correlation.class(),
            target_matches,
            false,
            current == before,
            permission_state_postimage_digest_v1(current)
                .is_some_and(|digest| digest == *expected_postimage),
        )
    }

    async fn observe_compensated_posted_panel_effect_v1(
        &self,
        guild: GuildId,
        channel: ChannelId,
        message: MessageId,
        bot_user: UserId,
        expected_postimage: &InteractionEffectExpectedPostimageDigestV1,
        correlation: &InteractionEffectCorrelationV1,
    ) -> DiscordEffectCompensationObservationOutcomeV1 {
        if correlation.class() != InteractionEffectCorrelationClassV1::Unsupported {
            return unsupported_compensation_observation_v1(correlation.class());
        }
        let observed = match self
            .http
            .message(Id::new(channel.0), Id::new(message.0))
            .await
        {
            Ok(response) => match response.model().await {
                Ok(observed) => observed,
                Err(error) => return unavailable_compensation_body_v1(&error),
            },
            Err(error) if is_not_found_v1(&error) => {
                return restored_compensation_observation_v1(correlation.class(), true, false);
            }
            Err(error) => return unavailable_compensation_request_v1(&error),
        };
        let target_matches = observed.id.get() == message.0
            && observed.channel_id.get() == channel.0
            && observed.guild_id.map(Id::get) == Some(guild.0);
        let actor_matches = observed.author.id.get() == bot_user.0;
        let applied_postimage_matches = target_matches
            && actor_matches
            && observed_panel_postimage_digest_v1(&observed) == Ok(expected_postimage.clone());
        classify_compensation_observation_v1(
            correlation.class(),
            target_matches,
            actor_matches,
            false,
            applied_postimage_matches,
        )
    }
}

impl TwilightMutationAdapter<'_> {
    async fn exact_audit_targets_v1(
        &self,
        guild: GuildId,
        bot_user: UserId,
        action: AuditLogEventType,
        correlation: &InteractionEffectCorrelationV1,
    ) -> Result<Vec<Option<u64>>, DiscordEffectReadFailureV1> {
        let reason = match audit_reason_v1(correlation) {
            Ok(reason) => reason,
            Err(_) => {
                return Err(DiscordEffectReadFailureV1::KnownFailed(
                    InteractionEffectKnownFailureV1::new(
                        InteractionEffectKnownFailureClassV1::InvalidRequest,
                        Some(400),
                    )
                    .expect("400 is a valid error status"),
                ));
            }
        };
        let audit_log = match self
            .http
            .audit_log(Id::new(guild.0))
            .action_type(action)
            .user_id(Id::new(bot_user.0))
            .limit(AUDIT_OBSERVATION_LIMIT_V1)
            .await
        {
            Ok(response) => match response.model().await {
                Ok(audit_log) => audit_log,
                Err(error) => return Err(read_body_failure_v1(&error)),
            },
            Err(error) => return Err(read_request_failure_v1(&error)),
        };
        Ok(exact_audit_targets_from_entries_v1(
            &audit_log.entries,
            action,
            bot_user,
            &reason,
        ))
    }

    async fn exact_overwrite_audit_targets_v1(
        &self,
        guild: GuildId,
        bot_user: UserId,
        correlation: &InteractionEffectCorrelationV1,
    ) -> Result<Vec<Option<u64>>, DiscordEffectReadFailureV1> {
        let mut targets = Vec::new();
        for action in [
            AuditLogEventType::ChannelOverwriteCreate,
            AuditLogEventType::ChannelOverwriteUpdate,
        ] {
            targets.extend(
                self.exact_audit_targets_v1(guild, bot_user, action, correlation)
                    .await?,
            );
        }
        Ok(targets)
    }

    async fn current_role_membership_v1(
        &self,
        guild: GuildId,
        member: UserId,
        role: RoleId,
    ) -> Result<bool, DiscordEffectReadFailureV1> {
        let member = match self
            .http
            .guild_member(Id::new(guild.0), Id::new(member.0))
            .await
        {
            Ok(response) => match response.model().await {
                Ok(member) => member,
                Err(error) => return Err(read_body_failure_v1(&error)),
            },
            Err(error) => return Err(read_request_failure_v1(&error)),
        };
        Ok(member.roles.iter().any(|id| id.get() == role.0))
    }

    async fn current_overwrite_v1(
        &self,
        channel: ChannelId,
        target: OverwriteTarget,
    ) -> Result<InteractionEffectPermissionStateV1, DiscordEffectReadFailureV1> {
        let channel = match self.http.channel(Id::new(channel.0)).await {
            Ok(response) => match response.model().await {
                Ok(channel) => channel,
                Err(error) => return Err(read_body_failure_v1(&error)),
            },
            Err(error) => return Err(read_request_failure_v1(&error)),
        };
        overwrite_state_v1(&channel, target).map_err(|_| malformed_overwrite_read_failure_v1())
    }
}

fn exact_audit_targets_from_entries_v1(
    entries: &[AuditLogEntry],
    action: AuditLogEventType,
    bot_user: UserId,
    reason: &str,
) -> Vec<Option<u64>> {
    entries
        .iter()
        .filter(|entry| {
            entry.action_type == action
                && entry.user_id.map(Id::get) == Some(bot_user.0)
                && entry.reason.as_deref() == Some(reason)
        })
        .map(|entry| entry.target_id.map(Id::get))
        .collect()
}

fn unique_audit_target_v1(
    targets: &[Option<u64>],
    expected: Option<u64>,
) -> Result<Option<u64>, DiscordEffectObservationEvidenceV1> {
    if targets.is_empty() {
        return Ok(None);
    }
    let target = targets.first().copied().flatten();
    let matches =
        targets.len() == 1 && target.is_some() && expected.is_none_or(|id| target == Some(id));
    if matches {
        Ok(target)
    } else {
        Err(DiscordEffectObservationEvidenceV1 {
            correlation_class: InteractionEffectCorrelationClassV1::AuditLogReason,
            exact_correlation_matches: 0,
            conflicting_matches: saturating_u16_v1(targets.len()),
            target_identity_matches: false,
            actor_identity_matches: true,
            postimage_matches: false,
        })
    }
}

fn audit_reason_v1(
    correlation: &InteractionEffectCorrelationV1,
) -> Result<String, DiscordEffectCorrelationErrorV1> {
    if correlation.class() != InteractionEffectCorrelationClassV1::AuditLogReason {
        return Err(DiscordEffectCorrelationErrorV1::UnsupportedClass);
    }
    Ok(format!(
        "{AUDIT_REASON_PREFIX_V1}{}",
        correlation.marker_digest().as_str()
    ))
}

fn compensation_reason_v1(
    correlation: &InteractionEffectCorrelationV1,
) -> Result<String, DiscordEffectCorrelationErrorV1> {
    if correlation.class() != InteractionEffectCorrelationClassV1::AuditLogReason {
        return Err(DiscordEffectCorrelationErrorV1::UnsupportedClass);
    }
    Ok(format!(
        "{COMPENSATION_REASON_PREFIX_V1}{}",
        correlation.marker_digest().as_str()
    ))
}

fn request_failure_v1<T>(error: &twilight_http::Error) -> DiscordEffectAttemptOutcomeV1<T> {
    match error.kind() {
        ErrorType::Response { status, .. } => classify_http_status_v1(status.get()),
        ErrorType::BuildingRequest
        | ErrorType::CreatingHeader { .. }
        | ErrorType::Json
        | ErrorType::Validation => invalid_request_v1(),
        ErrorType::Unauthorized => {
            known_failure_v1(InteractionEffectKnownFailureClassV1::Forbidden, Some(401))
        }
        ErrorType::RequestTimedOut => DiscordEffectAttemptOutcomeV1::Indeterminate(
            InteractionEffectIndeterminateClassV1::DeadlineElapsed,
        ),
        ErrorType::RequestError => DiscordEffectAttemptOutcomeV1::Indeterminate(
            InteractionEffectIndeterminateClassV1::ConnectionLost,
        ),
        ErrorType::RequestCanceled => DiscordEffectAttemptOutcomeV1::Indeterminate(
            InteractionEffectIndeterminateClassV1::Cancelled,
        ),
        ErrorType::Parsing { .. } => DiscordEffectAttemptOutcomeV1::Indeterminate(
            InteractionEffectIndeterminateClassV1::MalformedResponse,
        ),
        _ => DiscordEffectAttemptOutcomeV1::Indeterminate(
            InteractionEffectIndeterminateClassV1::Unknown,
        ),
    }
}

fn body_failure_v1<T>(_error: &DeserializeBodyError) -> DiscordEffectAttemptOutcomeV1<T> {
    DiscordEffectAttemptOutcomeV1::Indeterminate(
        InteractionEffectIndeterminateClassV1::MalformedResponse,
    )
}

fn classify_http_status_v1<T>(status: u16) -> DiscordEffectAttemptOutcomeV1<T> {
    if !(400..=599).contains(&status) {
        return DiscordEffectAttemptOutcomeV1::Indeterminate(
            InteractionEffectIndeterminateClassV1::Unknown,
        );
    }
    match status {
        400 => known_failure_v1(
            InteractionEffectKnownFailureClassV1::InvalidRequest,
            Some(status),
        ),
        401 | 403 => known_failure_v1(
            InteractionEffectKnownFailureClassV1::Forbidden,
            Some(status),
        ),
        404 => known_failure_v1(InteractionEffectKnownFailureClassV1::NotFound, Some(status)),
        409 => known_failure_v1(InteractionEffectKnownFailureClassV1::Conflict, Some(status)),
        429 => known_failure_v1(
            InteractionEffectKnownFailureClassV1::RateLimitedBeforeDispatch,
            Some(status),
        ),
        500..=599 => DiscordEffectAttemptOutcomeV1::Indeterminate(
            InteractionEffectIndeterminateClassV1::ProviderUnavailable,
        ),
        _ => known_failure_v1(InteractionEffectKnownFailureClassV1::Rejected, Some(status)),
    }
}

fn known_failure_v1<T>(
    class: InteractionEffectKnownFailureClassV1,
    status: Option<u16>,
) -> DiscordEffectAttemptOutcomeV1<T> {
    DiscordEffectAttemptOutcomeV1::KnownFailed(
        InteractionEffectKnownFailureV1::new(class, status)
            .expect("the classified Discord status is a valid error status"),
    )
}

fn invalid_request_v1<T>() -> DiscordEffectAttemptOutcomeV1<T> {
    known_failure_v1(
        InteractionEffectKnownFailureClassV1::InvalidRequest,
        Some(400),
    )
}

fn conflict_v1<T>() -> DiscordEffectAttemptOutcomeV1<T> {
    known_failure_v1(InteractionEffectKnownFailureClassV1::Conflict, None)
}

fn unit_request_outcome_v1<T>(
    result: Result<T, twilight_http::Error>,
) -> DiscordEffectAttemptOutcomeV1<()> {
    match result {
        Ok(_) => DiscordEffectAttemptOutcomeV1::KnownSucceeded(()),
        Err(error) => request_failure_v1(&error),
    }
}

fn compensation_delete_outcome_v1<T>(
    result: Result<T, twilight_http::Error>,
) -> DiscordEffectAttemptOutcomeV1<()> {
    match result {
        Ok(_) => DiscordEffectAttemptOutcomeV1::KnownSucceeded(()),
        Err(error) if is_not_found_v1(&error) => DiscordEffectAttemptOutcomeV1::KnownSucceeded(()),
        Err(error) => request_failure_v1(&error),
    }
}

fn is_not_found_v1(error: &twilight_http::Error) -> bool {
    matches!(
        error.kind(),
        ErrorType::Response { status, .. } if status.get() == 404
    )
}

fn read_request_failure_v1(error: &twilight_http::Error) -> DiscordEffectReadFailureV1 {
    match request_failure_v1::<()>(error) {
        DiscordEffectAttemptOutcomeV1::KnownFailed(error) => {
            DiscordEffectReadFailureV1::KnownFailed(error)
        }
        DiscordEffectAttemptOutcomeV1::Indeterminate(error) => {
            DiscordEffectReadFailureV1::Indeterminate(error)
        }
        DiscordEffectAttemptOutcomeV1::KnownSucceeded(()) => unreachable!(),
    }
}

fn read_body_failure_v1(_error: &DeserializeBodyError) -> DiscordEffectReadFailureV1 {
    DiscordEffectReadFailureV1::Indeterminate(
        InteractionEffectIndeterminateClassV1::MalformedResponse,
    )
}

fn read_failure_as_attempt_v1<T>(
    failure: DiscordEffectReadFailureV1,
) -> DiscordEffectAttemptOutcomeV1<T> {
    match failure {
        DiscordEffectReadFailureV1::KnownFailed(failure) => {
            DiscordEffectAttemptOutcomeV1::KnownFailed(failure)
        }
        DiscordEffectReadFailureV1::Indeterminate(failure) => {
            DiscordEffectAttemptOutcomeV1::Indeterminate(failure)
        }
    }
}

fn unavailable_request_v1<T>(error: &twilight_http::Error) -> DiscordEffectObservationOutcomeV1<T> {
    DiscordEffectObservationOutcomeV1::Unavailable(read_request_failure_v1(error))
}

fn unavailable_body_v1<T>(error: &DeserializeBodyError) -> DiscordEffectObservationOutcomeV1<T> {
    DiscordEffectObservationOutcomeV1::Unavailable(read_body_failure_v1(error))
}

fn read_failure_as_observation_v1<T>(
    failure: DiscordEffectReadFailureV1,
) -> DiscordEffectObservationOutcomeV1<T> {
    match failure {
        DiscordEffectReadFailureV1::KnownFailed(failure) => {
            DiscordEffectObservationOutcomeV1::Unavailable(DiscordEffectReadFailureV1::KnownFailed(
                failure,
            ))
        }
        DiscordEffectReadFailureV1::Indeterminate(failure) => {
            DiscordEffectObservationOutcomeV1::Unavailable(
                DiscordEffectReadFailureV1::Indeterminate(failure),
            )
        }
    }
}

fn classify_compensation_observation_v1(
    correlation_class: InteractionEffectCorrelationClassV1,
    target_identity_matches: bool,
    actor_identity_matches: bool,
    preimage_matches: bool,
    applied_postimage_matches: bool,
) -> DiscordEffectCompensationObservationOutcomeV1 {
    if target_identity_matches && preimage_matches {
        return restored_compensation_observation_v1(
            correlation_class,
            target_identity_matches,
            actor_identity_matches,
        );
    }
    if target_identity_matches && applied_postimage_matches {
        return pending_compensation_observation_v1(
            correlation_class,
            target_identity_matches,
            actor_identity_matches,
        );
    }
    conflict_compensation_observation_v1(
        correlation_class,
        target_identity_matches,
        actor_identity_matches,
        preimage_matches,
    )
}

fn compensation_observation_evidence_v1(
    correlation_class: InteractionEffectCorrelationClassV1,
    target_identity_matches: bool,
    actor_identity_matches: bool,
    preimage_matches: bool,
) -> DiscordEffectObservationEvidenceV1 {
    DiscordEffectObservationEvidenceV1 {
        correlation_class,
        exact_correlation_matches: 0,
        conflicting_matches: 0,
        target_identity_matches,
        actor_identity_matches,
        postimage_matches: preimage_matches,
    }
}

fn restored_compensation_observation_v1(
    correlation_class: InteractionEffectCorrelationClassV1,
    target_identity_matches: bool,
    actor_identity_matches: bool,
) -> DiscordEffectCompensationObservationOutcomeV1 {
    DiscordEffectCompensationObservationOutcomeV1::Restored {
        evidence: compensation_observation_evidence_v1(
            correlation_class,
            target_identity_matches,
            actor_identity_matches,
            true,
        ),
    }
}

fn pending_compensation_observation_v1(
    correlation_class: InteractionEffectCorrelationClassV1,
    target_identity_matches: bool,
    actor_identity_matches: bool,
) -> DiscordEffectCompensationObservationOutcomeV1 {
    DiscordEffectCompensationObservationOutcomeV1::Pending {
        evidence: compensation_observation_evidence_v1(
            correlation_class,
            target_identity_matches,
            actor_identity_matches,
            false,
        ),
    }
}

fn conflict_compensation_observation_v1(
    correlation_class: InteractionEffectCorrelationClassV1,
    target_identity_matches: bool,
    actor_identity_matches: bool,
    preimage_matches: bool,
) -> DiscordEffectCompensationObservationOutcomeV1 {
    DiscordEffectCompensationObservationOutcomeV1::Conflict {
        evidence: compensation_observation_evidence_v1(
            correlation_class,
            target_identity_matches,
            actor_identity_matches,
            preimage_matches,
        ),
    }
}

fn unsupported_compensation_observation_v1(
    correlation_class: InteractionEffectCorrelationClassV1,
) -> DiscordEffectCompensationObservationOutcomeV1 {
    DiscordEffectCompensationObservationOutcomeV1::Unsupported {
        evidence: compensation_observation_evidence_v1(correlation_class, false, false, false),
    }
}

fn unavailable_compensation_request_v1(
    error: &twilight_http::Error,
) -> DiscordEffectCompensationObservationOutcomeV1 {
    unavailable_compensation_read_v1(read_request_failure_v1(error))
}

fn unavailable_compensation_body_v1(
    error: &DeserializeBodyError,
) -> DiscordEffectCompensationObservationOutcomeV1 {
    unavailable_compensation_read_v1(read_body_failure_v1(error))
}

fn unavailable_compensation_read_v1(
    failure: DiscordEffectReadFailureV1,
) -> DiscordEffectCompensationObservationOutcomeV1 {
    DiscordEffectCompensationObservationOutcomeV1::Unavailable(failure)
}

fn pending_audit_v1<T>() -> DiscordEffectObservationOutcomeV1<T> {
    DiscordEffectObservationOutcomeV1::Pending {
        evidence: DiscordEffectObservationEvidenceV1 {
            correlation_class: InteractionEffectCorrelationClassV1::AuditLogReason,
            exact_correlation_matches: 0,
            conflicting_matches: 0,
            target_identity_matches: false,
            actor_identity_matches: false,
            postimage_matches: false,
        },
    }
}

fn exact_audit_v1<T>(output: T) -> DiscordEffectObservationOutcomeV1<T> {
    DiscordEffectObservationOutcomeV1::ExactMatch {
        output,
        evidence: DiscordEffectObservationEvidenceV1 {
            correlation_class: InteractionEffectCorrelationClassV1::AuditLogReason,
            exact_correlation_matches: 1,
            conflicting_matches: 0,
            target_identity_matches: true,
            actor_identity_matches: true,
            postimage_matches: true,
        },
    }
}

fn conflict_audit_v1<T>(
    correlations: usize,
    target_identity_matches: bool,
    actor_identity_matches: bool,
    postimage_matches: bool,
) -> DiscordEffectObservationOutcomeV1<T> {
    DiscordEffectObservationOutcomeV1::Conflict {
        evidence: DiscordEffectObservationEvidenceV1 {
            correlation_class: InteractionEffectCorrelationClassV1::AuditLogReason,
            exact_correlation_matches: 0,
            conflicting_matches: saturating_u16_v1(correlations.max(1)),
            target_identity_matches,
            actor_identity_matches,
            postimage_matches,
        },
    }
}

fn permission_state_postimage_digest_v1(
    state: InteractionEffectPermissionStateV1,
) -> Option<InteractionEffectExpectedPostimageDigestV1> {
    match state {
        InteractionEffectPermissionStateV1::Absent => None,
        InteractionEffectPermissionStateV1::Present(value) => Some(overwrite_postimage_digest_v1(
            Permissions::from_bits_retain(value.allow()),
            Permissions::from_bits_retain(value.deny()),
        )),
    }
}

fn overwrite_state_v1(
    channel: &twilight_model::channel::Channel,
    target: OverwriteTarget,
) -> Result<InteractionEffectPermissionStateV1, DiscordEffectOverwriteDecodeErrorV1> {
    let expected_id = target_id_v1(target);
    let expected_kind = match target {
        OverwriteTarget::Role(_) => PermissionOverwriteType::Role,
        OverwriteTarget::Member(_) => PermissionOverwriteType::Member,
    };
    let Some(overwrite) = channel
        .permission_overwrites
        .as_deref()
        .unwrap_or_default()
        .iter()
        .find(|overwrite| overwrite.id.get() == expected_id && overwrite.kind == expected_kind)
    else {
        return Ok(InteractionEffectPermissionStateV1::Absent);
    };
    decode_overwrite_value_v1(overwrite.allow.bits(), overwrite.deny.bits())
        .map(InteractionEffectPermissionStateV1::Present)
}

fn decode_overwrite_value_v1(
    allow: u64,
    deny: u64,
) -> Result<InteractionEffectPermissionValueV1, DiscordEffectOverwriteDecodeErrorV1> {
    InteractionEffectPermissionValueV1::new(allow, deny)
        .map_err(|_| DiscordEffectOverwriteDecodeErrorV1::OverlappingPermissions)
}

fn malformed_overwrite_observation_v1<T>(
    correlations: usize,
) -> DiscordEffectObservationOutcomeV1<T> {
    conflict_audit_v1(correlations, true, true, false)
}

fn malformed_overwrite_compensation_observation_v1(
    correlation_class: InteractionEffectCorrelationClassV1,
) -> DiscordEffectCompensationObservationOutcomeV1 {
    conflict_compensation_observation_v1(correlation_class, true, false, false)
}

fn malformed_overwrite_read_failure_v1() -> DiscordEffectReadFailureV1 {
    match InteractionEffectKnownFailureV1::new(InteractionEffectKnownFailureClassV1::Conflict, None)
    {
        Ok(failure) => DiscordEffectReadFailureV1::KnownFailed(failure),
        Err(_) => DiscordEffectReadFailureV1::Indeterminate(
            InteractionEffectIndeterminateClassV1::Unknown,
        ),
    }
}

fn target_id_v1(target: OverwriteTarget) -> u64 {
    match target {
        OverwriteTarget::Role(role) => role.0,
        OverwriteTarget::Member(member) => member.0,
    }
}

fn panel_components_v1(
    guild: GuildId,
    ruleset_key: &str,
    spec: &PostPanelSpec,
) -> Result<Vec<twilight_model::channel::message::component::Component>, ()> {
    let buttons = spec
        .buttons
        .iter()
        .map(|button| to_button_component(guild, ruleset_key, button).map_err(|_| ()))
        .collect::<Result<Vec<_>, _>>()?;
    Ok(vec![
        twilight_model::channel::message::component::Component::ActionRow(
            twilight_model::channel::message::component::ActionRow {
                id: None,
                components: buttons,
            },
        ),
    ])
}

fn observed_panel_postimage_digest_v1(
    message: &twilight_model::channel::Message,
) -> Result<InteractionEffectExpectedPostimageDigestV1, ()> {
    panel_components_postimage_digest_v1(&message.content, &message.components)
}

fn panel_components_postimage_digest_v1(
    content: &str,
    components: &[twilight_model::channel::message::component::Component],
) -> Result<InteractionEffectExpectedPostimageDigestV1, ()> {
    use twilight_model::channel::message::component::{ButtonStyle, Component};
    let [Component::ActionRow(row)] = components else {
        return Err(());
    };
    let mut buttons = Vec::with_capacity(row.components.len());
    for component in &row.components {
        let Component::Button(button) = component else {
            return Err(());
        };
        if button.disabled
            || button.emoji.is_some()
            || button.style != ButtonStyle::Primary
            || button.url.is_some()
            || button.sku_id.is_some()
        {
            return Err(());
        }
        let label = button.label.clone().ok_or(())?;
        let custom_id = button.custom_id.clone().ok_or(())?;
        buttons.push((label, custom_id));
    }
    Ok(panel_postimage_digest_v1(content, &buttons))
}

fn saturating_u16_v1(value: usize) -> u16 {
    u16::try_from(value).unwrap_or(u16::MAX)
}

#[cfg(test)]
mod tests {
    use automation_runtime_interaction::{
        InteractionEffectCorrelationClassV1, InteractionEffectKnownFailureClassV1,
        InteractionEffectPermissionStateV1, InteractionEffectPermissionValueV1,
    };
    use twilight_model::{
        channel::message::component::{ActionRow, Button, ButtonStyle, Component},
        guild::audit_log::{AuditLogEntry, AuditLogEventType},
        id::Id,
    };

    use super::{
        classify_compensation_observation_v1, classify_http_status_v1, decode_overwrite_value_v1,
        exact_audit_targets_from_entries_v1, malformed_overwrite_compensation_observation_v1,
        malformed_overwrite_observation_v1, malformed_overwrite_read_failure_v1,
        panel_components_postimage_digest_v1, permission_state_postimage_digest_v1,
        unique_audit_target_v1, unsupported_compensation_observation_v1,
        DiscordEffectAttemptOutcomeV1, DiscordEffectCompensationObservationOutcomeV1,
        DiscordEffectObservationOutcomeV1, DiscordEffectReadFailureV1,
    };
    use crate::discord_effect_postimage::overwrite_postimage_digest_v1;
    use discord_model::{OverwriteTarget, Permissions, RoleId, UserId};

    fn audit_entry_v1(
        action: AuditLogEventType,
        actor: u64,
        target: Option<u64>,
        reason: &str,
    ) -> AuditLogEntry {
        AuditLogEntry {
            action_type: action,
            changes: Vec::new(),
            guild_id: None,
            id: Id::new(9),
            options: None,
            reason: Some(reason.to_owned()),
            target_id: target.map(Id::new),
            user_id: Some(Id::new(actor)),
        }
    }

    #[test]
    fn exact_audit_matching_binds_action_actor_reason_and_target() {
        let entries = vec![
            audit_entry_v1(AuditLogEventType::RoleCreate, 7, Some(11), "exact"),
            audit_entry_v1(AuditLogEventType::RoleCreate, 8, Some(12), "exact"),
            audit_entry_v1(AuditLogEventType::ChannelCreate, 7, Some(13), "exact"),
            audit_entry_v1(AuditLogEventType::RoleCreate, 7, Some(14), "other"),
        ];
        let targets = exact_audit_targets_from_entries_v1(
            &entries,
            AuditLogEventType::RoleCreate,
            UserId(7),
            "exact",
        );
        assert_eq!(targets, vec![Some(11)]);
        assert_eq!(unique_audit_target_v1(&targets, Some(11)), Ok(Some(11)));
    }

    #[test]
    fn duplicate_or_missing_audit_targets_fail_closed() {
        let duplicate = vec![Some(11), Some(12)];
        let evidence = unique_audit_target_v1(&duplicate, None).unwrap_err();
        assert_eq!(evidence.exact_correlation_matches(), 0);
        assert_eq!(evidence.conflicting_matches(), 2);
        assert!(!evidence.target_identity_matches());
        assert!(evidence.actor_identity_matches());
        assert!(!evidence.postimage_matches());
        assert_eq!(
            evidence.correlation_class(),
            InteractionEffectCorrelationClassV1::AuditLogReason
        );
        assert!(unique_audit_target_v1(&[None], None).is_err());
    }

    #[test]
    fn all_http_client_failures_are_definitive_and_server_failures_are_indeterminate() {
        let expected = [
            (400, InteractionEffectKnownFailureClassV1::InvalidRequest),
            (403, InteractionEffectKnownFailureClassV1::Forbidden),
            (404, InteractionEffectKnownFailureClassV1::NotFound),
            (
                429,
                InteractionEffectKnownFailureClassV1::RateLimitedBeforeDispatch,
            ),
        ];
        for (status, class) in expected {
            let DiscordEffectAttemptOutcomeV1::KnownFailed(failure) =
                classify_http_status_v1::<()>(status)
            else {
                panic!("expected a known failure")
            };
            assert_eq!(failure.class(), class);
            assert_eq!(failure.http_status(), Some(status));
        }
        assert!(matches!(
            classify_http_status_v1::<()>(500),
            DiscordEffectAttemptOutcomeV1::Indeterminate(_)
        ));
    }

    #[test]
    fn panel_postimage_ignores_only_server_assigned_component_ids() {
        let expected = vec![Component::ActionRow(ActionRow {
            id: None,
            components: vec![Component::Button(Button {
                id: None,
                custom_id: Some("exact-route".to_owned()),
                disabled: false,
                emoji: None,
                label: Some("Join".to_owned()),
                style: ButtonStyle::Primary,
                url: None,
                sku_id: None,
            })],
        })];
        let mut observed = expected.clone();
        let Component::ActionRow(row) = &mut observed[0] else {
            panic!("expected an action row")
        };
        row.id = Some(7);
        match &mut row.components[0] {
            Component::Button(button) => button.id = Some(8),
            _ => panic!("expected a button"),
        }
        let digest = panel_components_postimage_digest_v1("panel", &expected).unwrap();
        assert_eq!(
            panel_components_postimage_digest_v1("panel", &observed),
            Ok(digest.clone())
        );
        let Component::ActionRow(row) = &mut observed[0] else {
            panic!("expected an action row")
        };
        match &mut row.components[0] {
            Component::Button(button) => {
                button.custom_id = Some("different-route".to_owned());
            }
            _ => panic!("expected a button"),
        }
        assert_ne!(
            panel_components_postimage_digest_v1("panel", &observed),
            Ok(digest)
        );
        assert!(panel_components_postimage_digest_v1(
            "panel",
            &[Component::Button(Button {
                id: None,
                custom_id: Some("exact-route".to_owned()),
                disabled: false,
                emoji: None,
                label: Some("Join".to_owned()),
                style: ButtonStyle::Primary,
                url: None,
                sku_id: None,
            })],
        )
        .is_err());
    }

    #[test]
    fn compensation_observation_uses_exact_state_without_inventing_audit_counts() {
        let restored = classify_compensation_observation_v1(
            InteractionEffectCorrelationClassV1::AuditLogReason,
            true,
            false,
            true,
            false,
        );
        let DiscordEffectCompensationObservationOutcomeV1::Restored { evidence } = restored else {
            panic!("expected exact restoration")
        };
        assert_eq!(evidence.exact_correlation_matches(), 0);
        assert_eq!(evidence.conflicting_matches(), 0);
        assert!(evidence.target_identity_matches());
        assert!(!evidence.actor_identity_matches());
        assert!(evidence.postimage_matches());

        let pending = classify_compensation_observation_v1(
            InteractionEffectCorrelationClassV1::AuditLogReason,
            true,
            false,
            false,
            true,
        );
        let DiscordEffectCompensationObservationOutcomeV1::Pending { evidence } = pending else {
            panic!("expected pending restoration")
        };
        assert_eq!(evidence.exact_correlation_matches(), 0);
        assert_eq!(evidence.conflicting_matches(), 0);
        assert!(evidence.target_identity_matches());
        assert!(!evidence.postimage_matches());

        let conflict = classify_compensation_observation_v1(
            InteractionEffectCorrelationClassV1::AuditLogReason,
            true,
            false,
            false,
            false,
        );
        let DiscordEffectCompensationObservationOutcomeV1::Conflict { evidence } = conflict else {
            panic!("expected conflicting restoration")
        };
        assert_eq!(evidence.exact_correlation_matches(), 0);
        assert_eq!(evidence.conflicting_matches(), 0);
        assert!(evidence.target_identity_matches());
        assert!(!evidence.postimage_matches());
    }

    #[test]
    fn compensation_observation_prefers_an_already_restored_preimage() {
        assert!(matches!(
            classify_compensation_observation_v1(
                InteractionEffectCorrelationClassV1::AuditLogReason,
                true,
                false,
                true,
                true,
            ),
            DiscordEffectCompensationObservationOutcomeV1::Restored { .. }
        ));
        let unsupported = unsupported_compensation_observation_v1(
            InteractionEffectCorrelationClassV1::InteractionReceipt,
        );
        let DiscordEffectCompensationObservationOutcomeV1::Unsupported { evidence } = unsupported
        else {
            panic!("expected unsupported correlation")
        };
        assert_eq!(
            evidence.correlation_class(),
            InteractionEffectCorrelationClassV1::InteractionReceipt
        );
        assert_eq!(evidence.exact_correlation_matches(), 0);
        assert_eq!(evidence.conflicting_matches(), 0);
    }

    #[test]
    fn malformed_overwrite_permissions_fail_closed_without_panicking() {
        assert!(decode_overwrite_value_v1(1, 1).is_err());

        let DiscordEffectObservationOutcomeV1::Conflict { evidence } =
            malformed_overwrite_observation_v1::<()>(1)
        else {
            panic!("expected conflicting observation")
        };
        assert_eq!(evidence.exact_correlation_matches(), 0);
        assert_eq!(evidence.conflicting_matches(), 1);
        assert!(evidence.target_identity_matches());
        assert!(evidence.actor_identity_matches());
        assert!(!evidence.postimage_matches());

        let DiscordEffectCompensationObservationOutcomeV1::Conflict { evidence } =
            malformed_overwrite_compensation_observation_v1(
                InteractionEffectCorrelationClassV1::AuditLogReason,
            )
        else {
            panic!("expected conflicting compensation observation")
        };
        assert_eq!(evidence.exact_correlation_matches(), 0);
        assert_eq!(evidence.conflicting_matches(), 0);
        assert!(evidence.target_identity_matches());
        assert!(!evidence.postimage_matches());

        let DiscordEffectReadFailureV1::KnownFailed(failure) =
            malformed_overwrite_read_failure_v1()
        else {
            panic!("expected typed read failure")
        };
        assert_eq!(
            failure.class(),
            InteractionEffectKnownFailureClassV1::Conflict
        );
        assert_eq!(failure.http_status(), None);
    }

    #[test]
    fn overwrite_compensation_retains_unmodeled_preimage_bits() {
        let unmodeled_allow = 1u64 << 63;
        let unmodeled_deny = 1u64 << 62;
        let value =
            InteractionEffectPermissionValueV1::new(unmodeled_allow, unmodeled_deny).unwrap();
        let state = InteractionEffectPermissionStateV1::Present(value);
        let allow = Permissions::from_bits_retain(unmodeled_allow);
        let deny = Permissions::from_bits_retain(unmodeled_deny);

        assert_eq!(
            permission_state_postimage_digest_v1(state),
            Some(overwrite_postimage_digest_v1(allow, deny))
        );
        let overwrite =
            crate::mutation::to_permission_overwrite(OverwriteTarget::Role(RoleId(7)), allow, deny);
        assert_eq!(overwrite.allow.unwrap().bits(), unmodeled_allow);
        assert_eq!(overwrite.deny.unwrap().bits(), unmodeled_deny);
    }
}
