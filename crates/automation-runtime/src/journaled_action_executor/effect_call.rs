use std::collections::BTreeMap;
use std::fmt::Write;

use automation_core::event::RuntimeContext;
use automation_core::preflight::{
    ActionEntryIdV1, PreflightButtonRouteV1, PreflightInstanceRefV1,
    PreflightInstanceResourceRefsV1, PreparedPlanActionV1,
};
use automation_core::{
    AdapterErrorKind, CreateChannelSpec, CreateRoleSpec, CreatedResource, PostPanelButtonSpec,
    PostPanelSpec, ResolvedButtonRoute, ResponseDeliveryOutcome, RunResult, TeardownActionResult,
};
use automation_instance::{
    AutomationInstance, InstanceId, InstanceMessageRef, InstanceResources, InstanceStatus,
    InstanceStoreError,
};
use automation_instance_teardown::{TeardownError, TeardownOutcome};
use automation_runtime_interaction::{
    build_interaction_effect_correlation_v1, InteractionEffectActionIndexV1,
    InteractionEffectAttemptOutcomeV1, InteractionEffectChannelIdV1, InteractionEffectDefinitionV1,
    InteractionEffectDependencyResolutionV1, InteractionEffectGuildIdV1,
    InteractionEffectIndeterminateClassV1, InteractionEffectInstanceStateV1,
    InteractionEffectKnownFailureClassV1, InteractionEffectKnownFailureV1,
    InteractionEffectMaterializedPlanV1, InteractionEffectMessageIdV1,
    InteractionEffectObservedOutputV1, InteractionEffectOverwriteTargetV1,
    InteractionEffectPermissionStateV1, InteractionEffectPreimageV1, InteractionEffectRoleIdV1,
    InteractionEffectTargetV1, InteractionInstanceManifestDigestV1,
};
use discord_model::{ChannelId, GuildId, MessageId, OverwriteTarget, Permissions, RoleId, UserId};

use super::{
    ExactInteractionTeardownSetV1, InteractionEffectExecutionPlanEntryV1,
    JournaledActionExecutionServicesV1,
};
use crate::discord_effects::{DiscordEffectAttemptOutcomeV1, RecoverableDiscordMutationAdapterV1};

#[derive(Default)]
pub(super) struct JournaledActionExecutionStateV1 {
    outputs: BTreeMap<InteractionEffectActionIndexV1, InteractionEffectObservedOutputV1>,
    roles: BTreeMap<ActionEntryIdV1, RoleId>,
    channels: BTreeMap<ActionEntryIdV1, ChannelId>,
    messages: BTreeMap<ActionEntryIdV1, InstanceMessageRef>,
    instances: BTreeMap<ActionEntryIdV1, AutomationInstance>,
    pub(super) result: RunResult,
    pub(super) mutable_successes: usize,
}

pub(super) fn materialize_effect_v1(
    definition: &automation_runtime_interaction::InteractionEffectPlanDefinitionV1,
    state: &JournaledActionExecutionStateV1,
) -> Result<InteractionEffectMaterializedPlanV1, ()> {
    let resolutions = definition
        .dependencies()
        .iter()
        .map(|dependency| {
            let output = state
                .outputs
                .get(&dependency.action_index())
                .cloned()
                .ok_or(())?;
            InteractionEffectDependencyResolutionV1::new(dependency.clone(), output).map_err(|_| ())
        })
        .collect::<Result<Vec<_>, _>>()?;
    definition.materialize(resolutions).map_err(|_| ())
}

pub(super) enum PreparedEffectCallV1 {
    CreateRole {
        guild: GuildId,
        spec: CreateRoleSpec,
    },
    CreateChannel {
        guild: GuildId,
        spec: CreateChannelSpec,
    },
    GrantRole {
        guild: GuildId,
        member: UserId,
        role: RoleId,
        output: InteractionEffectObservedOutputV1,
    },
    UpsertOverwrite {
        guild: GuildId,
        channel: ChannelId,
        target: OverwriteTarget,
        allow: Permissions,
        deny: Permissions,
        output: InteractionEffectObservedOutputV1,
    },
    PostPanel {
        guild: GuildId,
        channel: ChannelId,
        spec: PostPanelSpec,
    },
    RegisterInstance {
        instance: AutomationInstance,
        resolved_manifest_digest: InteractionInstanceManifestDigestV1,
        output: InteractionEffectObservedOutputV1,
    },
    TeardownInstance {
        request: automation_instance_teardown::ExactInstanceTeardownRequestV1,
        output: InteractionEffectObservedOutputV1,
    },
    EditResponse {
        content: String,
        output: InteractionEffectObservedOutputV1,
    },
}

impl PreparedEffectCallV1 {
    pub(super) fn resolved_instance_manifest_digest_v1(
        &self,
    ) -> Option<&InteractionInstanceManifestDigestV1> {
        match self {
            Self::RegisterInstance {
                resolved_manifest_digest,
                ..
            } => Some(resolved_manifest_digest),
            _ => None,
        }
    }
}

pub(super) fn prepare_effect_call_v1(
    entry: &InteractionEffectExecutionPlanEntryV1,
    materialized: &InteractionEffectMaterializedPlanV1,
    state: &JournaledActionExecutionStateV1,
    exact_teardowns: &ExactInteractionTeardownSetV1,
    context: &RuntimeContext,
) -> Result<PreparedEffectCallV1, ()> {
    let target = materialized.resolved_input().target();
    match (entry.action(), target) {
        (
            PreparedPlanActionV1::CreateRole { name, .. },
            InteractionEffectTargetV1::CreateRole { guild_id },
        ) if guild_id.get() == context.guild_id.0 => Ok(PreparedEffectCallV1::CreateRole {
            guild: context.guild_id,
            spec: CreateRoleSpec { name: name.clone() },
        }),
        (
            PreparedPlanActionV1::CreateChannel { name, .. },
            InteractionEffectTargetV1::CreateChannel { guild_id },
        ) if guild_id.get() == context.guild_id.0 => Ok(PreparedEffectCallV1::CreateChannel {
            guild: context.guild_id,
            spec: CreateChannelSpec { name: name.clone() },
        }),
        (
            PreparedPlanActionV1::GrantRole { target: actor, .. },
            InteractionEffectTargetV1::GrantRole { target },
        ) if target.guild_id().get() == context.guild_id.0 && target.user_id().get() == actor.0 => {
            let output = InteractionEffectObservedOutputV1::RoleMembership {
                target: *target,
                present: true,
            };
            Ok(PreparedEffectCallV1::GrantRole {
                guild: context.guild_id,
                member: *actor,
                role: RoleId(target.role_id().get()),
                output,
            })
        }
        (
            PreparedPlanActionV1::UpsertOverwrite { allow, deny, .. },
            InteractionEffectTargetV1::UpsertOverwrite { target, desired },
        ) if target.guild_id().get() == context.guild_id.0
            && desired.allow() == allow.bits()
            && desired.deny() == deny.bits() =>
        {
            let overwrite_target = match target.target() {
                InteractionEffectOverwriteTargetV1::Role(role) => {
                    OverwriteTarget::Role(RoleId(role.get()))
                }
                InteractionEffectOverwriteTargetV1::Member(member) => {
                    OverwriteTarget::Member(UserId(member.get()))
                }
            };
            let output = InteractionEffectObservedOutputV1::PermissionOverwrite {
                target: *target,
                state: InteractionEffectPermissionStateV1::Present(*desired),
            };
            Ok(PreparedEffectCallV1::UpsertOverwrite {
                guild: context.guild_id,
                channel: ChannelId(target.channel_id().get()),
                target: overwrite_target,
                allow: *allow,
                deny: *deny,
                output,
            })
        }
        (
            PreparedPlanActionV1::PostPanel {
                channel: _,
                content,
                buttons,
                ..
            },
            InteractionEffectTargetV1::PostPanel {
                guild_id,
                channel_id,
                ..
            },
        ) if guild_id.get() == context.guild_id.0 => {
            let buttons = buttons
                .iter()
                .map(|button| PostPanelButtonSpec {
                    label: button.label.clone(),
                    route: match &button.route {
                        PreflightButtonRouteV1::Static { key } => {
                            ResolvedButtonRoute::Static { key: key.clone() }
                        }
                        PreflightButtonRouteV1::InstanceAction {
                            instance_id,
                            action,
                            ..
                        } => ResolvedButtonRoute::InstanceAction {
                            instance_id: instance_id.clone(),
                            action: action.clone(),
                        },
                    },
                })
                .collect();
            Ok(PreparedEffectCallV1::PostPanel {
                guild: context.guild_id,
                channel: ChannelId(channel_id.get()),
                spec: PostPanelSpec {
                    content: content.clone(),
                    buttons,
                },
            })
        }
        (
            PreparedPlanActionV1::RegisterInstance {
                id,
                kind,
                resources,
                ..
            },
            InteractionEffectTargetV1::RegisterInstance {
                target,
                kind: resolved_kind,
                manifest_digest,
            },
        ) if target.guild_id().get() == context.guild_id.0
            && kind == resolved_kind
            && planned_instance_id_v1(materialized) == Some(id) =>
        {
            let resources = resolve_instance_resources_v1(resources, state)?;
            let instance = AutomationInstance {
                id: id.clone(),
                guild_id: context.guild_id,
                ruleset_key: context.ruleset_key.clone(),
                ruleset_version: context.ruleset_version,
                kind: kind.clone(),
                created_by: context.actor,
                resources,
                status: InstanceStatus::Active,
            };
            let resolved_manifest_digest =
                crate::action_plan_wire_preflight::exact_instance_manifest_digest_v1(
                    context.guild_id,
                    &instance.resources,
                )
                .map_err(|_| ())?;
            let output = InteractionEffectObservedOutputV1::InstanceState {
                target: target.clone(),
                state: InteractionEffectInstanceStateV1::Present {
                    manifest_digest: manifest_digest.clone(),
                },
            };
            Ok(PreparedEffectCallV1::RegisterInstance {
                instance,
                resolved_manifest_digest,
                output,
            })
        }
        (
            PreparedPlanActionV1::TeardownInstance {
                instance: PreflightInstanceRefV1::Existing(instance_id),
                ..
            },
            InteractionEffectTargetV1::TeardownInstance { target },
        ) if target.guild_id().get() == context.guild_id.0
            && planned_instance_id_v1(materialized) == Some(instance_id) =>
        {
            let request = exact_teardowns
                .get(entry.action_entry())
                .filter(|request| {
                    request.guild_id() == context.guild_id
                        && request.instance_id() == instance_id
                        && exact_teardown_preimage_matches_v1(materialized, request)
                })
                .cloned()
                .ok_or(())?;
            let output = InteractionEffectObservedOutputV1::InstanceState {
                target: target.clone(),
                state: InteractionEffectInstanceStateV1::Absent,
            };
            Ok(PreparedEffectCallV1::TeardownInstance { request, output })
        }
        (
            PreparedPlanActionV1::EditResponse { content, .. },
            InteractionEffectTargetV1::EditResponse {
                receipt_identity,
                payload_digest,
            },
        ) => Ok(PreparedEffectCallV1::EditResponse {
            content: content.clone(),
            output: InteractionEffectObservedOutputV1::OriginalResponse {
                receipt_identity: *receipt_identity,
                payload_digest: payload_digest.clone(),
            },
        }),
        _ => Err(()),
    }
}

pub(super) struct EffectCallResultV1 {
    pub(super) outcome: InteractionEffectAttemptOutcomeV1,
    pub(super) teardown: Option<TeardownOutcome>,
    pub(super) registered_instance: Option<AutomationInstance>,
}

pub(super) async fn execute_effect_call_v1<J, M, R, S, T>(
    prepared: PreparedEffectCallV1,
    materialized: &InteractionEffectMaterializedPlanV1,
    services: &JournaledActionExecutionServicesV1<'_, J, M, R, S, T>,
) -> EffectCallResultV1
where
    M: RecoverableDiscordMutationAdapterV1,
    R: automation_core::InteractionResponder,
    S: automation_instance::InstanceRegistrarV1,
    T: automation_instance_teardown::DurableInstanceTeardownServiceV1,
{
    let definition = materialized.definition();
    let correlation = build_interaction_effect_correlation_v1(definition);
    let mut teardown = None;
    let mut registered_instance = None;
    let outcome = match prepared {
        PreparedEffectCallV1::CreateRole { guild, spec } => map_discord_output_v1(
            definition,
            services
                .mutation
                .create_role_effect_v1(guild, spec, &correlation)
                .await,
            |role| {
                Some(InteractionEffectObservedOutputV1::CreatedRole {
                    guild_id: InteractionEffectGuildIdV1::new(guild.0).ok()?,
                    role_id: InteractionEffectRoleIdV1::new(role.0).ok()?,
                })
            },
        ),
        PreparedEffectCallV1::CreateChannel { guild, spec } => map_discord_output_v1(
            definition,
            services
                .mutation
                .create_channel_effect_v1(guild, spec, &correlation)
                .await,
            |channel| {
                Some(InteractionEffectObservedOutputV1::CreatedChannel {
                    guild_id: InteractionEffectGuildIdV1::new(guild.0).ok()?,
                    channel_id: InteractionEffectChannelIdV1::new(channel.0).ok()?,
                })
            },
        ),
        PreparedEffectCallV1::GrantRole {
            guild,
            member,
            role,
            output,
        } => map_discord_unit_v1(
            definition,
            services
                .mutation
                .grant_role_effect_v1(guild, member, role, &correlation)
                .await,
            output,
        ),
        PreparedEffectCallV1::UpsertOverwrite {
            guild,
            channel,
            target,
            allow,
            deny,
            output,
        } => map_discord_unit_v1(
            definition,
            services
                .mutation
                .upsert_overwrite_effect_v1(guild, channel, target, allow, deny, &correlation)
                .await,
            output,
        ),
        PreparedEffectCallV1::PostPanel {
            guild,
            channel,
            spec,
        } => {
            let payload_digest = match materialized.resolved_input().target() {
                InteractionEffectTargetV1::PostPanel { payload_digest, .. } => {
                    payload_digest.clone()
                }
                _ => return protocol_call_result_v1(),
            };
            map_discord_output_v1(
                definition,
                services
                    .mutation
                    .post_panel_effect_v1(guild, channel, spec, &correlation)
                    .await,
                |message| {
                    Some(InteractionEffectObservedOutputV1::PostedMessage {
                        guild_id: InteractionEffectGuildIdV1::new(guild.0).ok()?,
                        channel_id: InteractionEffectChannelIdV1::new(channel.0).ok()?,
                        message_id: InteractionEffectMessageIdV1::new(message.0).ok()?,
                        payload_digest,
                    })
                },
            )
        }
        PreparedEffectCallV1::RegisterInstance {
            instance,
            resolved_manifest_digest: _,
            output,
        } => {
            let authoritative_instance = instance.clone();
            match services.instances.register_instance_v1(instance).await {
                Ok(()) => {
                    registered_instance = Some(authoritative_instance);
                    known_success_v1(definition, output)
                }
                Err(error) => instance_store_outcome_v1(error),
            }
        }
        PreparedEffectCallV1::TeardownInstance { request, output } => {
            match services.teardown.teardown_exact_v1(&request).await {
                Ok(
                    result @ (TeardownOutcome::Completed
                    | TeardownOutcome::ResumedAndCompleted
                    | TeardownOutcome::AlreadyDeleted),
                ) => {
                    teardown = Some(result);
                    known_success_v1(definition, output)
                }
                Ok(TeardownOutcome::InProgress) => {
                    InteractionEffectAttemptOutcomeV1::Indeterminate(
                        InteractionEffectIndeterminateClassV1::ProviderUnavailable,
                    )
                }
                Err(error) => teardown_outcome_v1(error),
            }
        }
        PreparedEffectCallV1::EditResponse { content, output } => {
            match services.responder.edit_response(content).await {
                Ok(()) => known_success_v1(definition, output),
                Err(error) => adapter_error_outcome_v1(error.kind),
            }
        }
    };
    EffectCallResultV1 {
        outcome,
        teardown,
        registered_instance,
    }
}

pub(super) fn record_success_v1(
    entry: &InteractionEffectExecutionPlanEntryV1,
    output: &InteractionEffectObservedOutputV1,
    teardown: Option<TeardownOutcome>,
    registered_instance: Option<AutomationInstance>,
    state: &mut JournaledActionExecutionStateV1,
) -> Result<(), ()> {
    let effect_index = entry.definition().action().action_index();
    if state.outputs.insert(effect_index, output.clone()).is_some() {
        return Err(());
    }
    match (entry.action(), output) {
        (
            PreparedPlanActionV1::CreateRole {
                entry,
                output: reference,
                key,
                name,
            },
            InteractionEffectObservedOutputV1::CreatedRole { role_id, .. },
        ) if *entry == reference.producer() => {
            let id = RoleId(role_id.get());
            if state.roles.insert(*entry, id).is_some() {
                return Err(());
            }
            state.result.created.push(CreatedResource::Role {
                action_index: usize::from(entry.ordinal()),
                key: key.clone(),
                name: name.clone(),
                id,
            });
            state.mutable_successes += 1;
        }
        (
            PreparedPlanActionV1::CreateChannel {
                entry,
                output: reference,
                key,
                name,
            },
            InteractionEffectObservedOutputV1::CreatedChannel { channel_id, .. },
        ) if *entry == reference.producer() => {
            let id = ChannelId(channel_id.get());
            if state.channels.insert(*entry, id).is_some() {
                return Err(());
            }
            state.result.created.push(CreatedResource::Channel {
                action_index: usize::from(entry.ordinal()),
                key: key.clone(),
                name: name.clone(),
                id,
            });
            state.mutable_successes += 1;
        }
        (
            PreparedPlanActionV1::PostPanel {
                entry,
                output: reference,
                key,
                ..
            },
            InteractionEffectObservedOutputV1::PostedMessage {
                channel_id,
                message_id,
                ..
            },
        ) if *entry == reference.producer() => {
            let channel = ChannelId(channel_id.get());
            let id = MessageId(message_id.get());
            if state
                .messages
                .insert(*entry, InstanceMessageRef { channel, id })
                .is_some()
            {
                return Err(());
            }
            state.result.created.push(CreatedResource::Message {
                action_index: usize::from(entry.ordinal()),
                key: key.clone(),
                channel,
                id,
            });
            state.mutable_successes += 1;
        }
        (
            PreparedPlanActionV1::RegisterInstance {
                entry,
                output: reference,
                key,
                id,
                kind: _,
                resources: _,
            },
            InteractionEffectObservedOutputV1::InstanceState {
                state: InteractionEffectInstanceStateV1::Present { .. },
                ..
            },
        ) if *entry == reference.producer() => {
            let instance = registered_instance.ok_or(())?;
            if instance.id != *id {
                return Err(());
            }
            if state.instances.insert(*entry, instance).is_some() {
                return Err(());
            }
            state.result.created.push(CreatedResource::Instance {
                action_index: usize::from(entry.ordinal()),
                key: key.clone(),
                id: id.clone(),
            });
            state.mutable_successes += 1;
        }
        (
            PreparedPlanActionV1::TeardownInstance { entry, instance },
            InteractionEffectObservedOutputV1::InstanceState {
                state: InteractionEffectInstanceStateV1::Absent,
                ..
            },
        ) => {
            let instance_id = match instance {
                PreflightInstanceRefV1::Existing(instance_id) => instance_id.clone(),
                PreflightInstanceRefV1::Registered(reference) => state
                    .instances
                    .get(&reference.producer())
                    .map(|instance| instance.id.clone())
                    .ok_or(())?,
            };
            state.result.teardowns.push(TeardownActionResult {
                action_index: usize::from(entry.ordinal()),
                instance_id,
                teardown: teardown.ok_or(())?,
                response: ResponseDeliveryOutcome::Sent,
            });
            state.mutable_successes += 1;
        }
        (
            PreparedPlanActionV1::GrantRole { .. },
            InteractionEffectObservedOutputV1::RoleMembership { .. },
        )
        | (
            PreparedPlanActionV1::UpsertOverwrite { .. },
            InteractionEffectObservedOutputV1::PermissionOverwrite { .. },
        ) => {
            state.mutable_successes += 1;
        }
        (
            PreparedPlanActionV1::EditResponse { .. },
            InteractionEffectObservedOutputV1::OriginalResponse { .. },
        ) => {}
        _ => return Err(()),
    }
    Ok(())
}

fn resolve_instance_resources_v1(
    references: &PreflightInstanceResourceRefsV1,
    state: &JournaledActionExecutionStateV1,
) -> Result<InstanceResources, ()> {
    let roles = references
        .roles
        .iter()
        .map(|(alias, reference)| {
            state
                .roles
                .get(&reference.producer())
                .copied()
                .map(|id| (alias.clone(), id))
                .ok_or(())
        })
        .collect::<Result<_, _>>()?;
    let channels = references
        .channels
        .iter()
        .map(|(alias, reference)| {
            state
                .channels
                .get(&reference.producer())
                .copied()
                .map(|id| (alias.clone(), id))
                .ok_or(())
        })
        .collect::<Result<_, _>>()?;
    let messages = references
        .messages
        .iter()
        .map(|(alias, reference)| {
            state
                .messages
                .get(&reference.producer())
                .cloned()
                .map(|message| (alias.clone(), message))
                .ok_or(())
        })
        .collect::<Result<_, _>>()?;
    Ok(InstanceResources {
        roles,
        channels,
        messages,
    })
}

fn planned_instance_id_v1(
    materialized: &InteractionEffectMaterializedPlanV1,
) -> Option<&InstanceId> {
    match materialized.planned_recovery_input().target() {
        automation_runtime_interaction::InteractionEffectPlannedTargetV1::RegisterInstance {
            target,
            ..
        }
        | automation_runtime_interaction::InteractionEffectPlannedTargetV1::TeardownInstance {
            target,
        } => Some(target.instance_id()),
        _ => None,
    }
}

fn exact_teardown_preimage_matches_v1(
    materialized: &InteractionEffectMaterializedPlanV1,
    request: &automation_instance_teardown::ExactInstanceTeardownRequestV1,
) -> bool {
    let InteractionEffectPreimageV1::InstanceRegistration {
        before: InteractionEffectInstanceStateV1::Present { manifest_digest },
        ..
    } = materialized.resolved_input().preimage()
    else {
        return false;
    };
    canonical_manifest_payload_digest_v1(request.expected_resources()) == *manifest_digest
}

fn canonical_manifest_payload_digest_v1(
    resources: &InstanceResources,
) -> automation_runtime_interaction::InteractionEffectPayloadDigestV1 {
    let mut canonical = String::new();
    canonical.push_str("{\"channels\":{");
    for (index, (alias, id)) in resources.channels.iter().enumerate() {
        if index > 0 {
            canonical.push(',');
        }
        write!(&mut canonical, "\"{alias}\":\"{}\"", id.0)
            .expect("writing canonical manifest cannot fail");
    }
    canonical.push_str("},\"messages\":{");
    for (index, (alias, message)) in resources.messages.iter().enumerate() {
        if index > 0 {
            canonical.push(',');
        }
        write!(
            &mut canonical,
            "\"{alias}\":{{\"channel\":\"{}\",\"id\":\"{}\"}}",
            message.channel.0, message.id.0
        )
        .expect("writing canonical manifest cannot fail");
    }
    canonical.push_str("},\"roles\":{");
    for (index, (alias, id)) in resources.roles.iter().enumerate() {
        if index > 0 {
            canonical.push(',');
        }
        write!(&mut canonical, "\"{alias}\":\"{}\"", id.0)
            .expect("writing canonical manifest cannot fail");
    }
    canonical.push_str("}}");
    automation_runtime_interaction::InteractionEffectPayloadDigestV1::from_canonical_bytes(
        canonical.as_bytes(),
    )
}

fn map_discord_output_v1<T>(
    definition: &InteractionEffectDefinitionV1,
    outcome: DiscordEffectAttemptOutcomeV1<T>,
    output: impl FnOnce(T) -> Option<InteractionEffectObservedOutputV1>,
) -> InteractionEffectAttemptOutcomeV1 {
    match outcome {
        DiscordEffectAttemptOutcomeV1::KnownSucceeded(value) => match output(value) {
            Some(output) => known_success_v1(definition, output),
            None => InteractionEffectAttemptOutcomeV1::Indeterminate(
                InteractionEffectIndeterminateClassV1::MalformedResponse,
            ),
        },
        DiscordEffectAttemptOutcomeV1::KnownFailed(failure) => {
            InteractionEffectAttemptOutcomeV1::KnownFailed(failure)
        }
        DiscordEffectAttemptOutcomeV1::Indeterminate(failure) => {
            InteractionEffectAttemptOutcomeV1::Indeterminate(failure)
        }
    }
}

fn map_discord_unit_v1(
    definition: &InteractionEffectDefinitionV1,
    outcome: DiscordEffectAttemptOutcomeV1<()>,
    output: InteractionEffectObservedOutputV1,
) -> InteractionEffectAttemptOutcomeV1 {
    map_discord_output_v1(definition, outcome, |()| Some(output))
}

fn known_success_v1(
    definition: &InteractionEffectDefinitionV1,
    output: InteractionEffectObservedOutputV1,
) -> InteractionEffectAttemptOutcomeV1 {
    InteractionEffectAttemptOutcomeV1::known_succeeded(definition, output).unwrap_or(
        InteractionEffectAttemptOutcomeV1::Indeterminate(
            InteractionEffectIndeterminateClassV1::MalformedResponse,
        ),
    )
}

fn instance_store_outcome_v1(error: InstanceStoreError) -> InteractionEffectAttemptOutcomeV1 {
    match error {
        InstanceStoreError::DuplicateInstance => {
            known_failure_v1(InteractionEffectKnownFailureClassV1::Conflict)
        }
        InstanceStoreError::NotFound => {
            known_failure_v1(InteractionEffectKnownFailureClassV1::NotFound)
        }
        InstanceStoreError::TimedOut => InteractionEffectAttemptOutcomeV1::Indeterminate(
            InteractionEffectIndeterminateClassV1::DeadlineElapsed,
        ),
        InstanceStoreError::Backend(_) => InteractionEffectAttemptOutcomeV1::Indeterminate(
            InteractionEffectIndeterminateClassV1::ProviderUnavailable,
        ),
    }
}

fn teardown_outcome_v1(error: TeardownError) -> InteractionEffectAttemptOutcomeV1 {
    match error {
        TeardownError::InstanceNotFound => {
            known_failure_v1(InteractionEffectKnownFailureClassV1::NotFound)
        }
        TeardownError::ManifestDrift => {
            known_failure_v1(InteractionEffectKnownFailureClassV1::Conflict)
        }
        TeardownError::Lookup(InstanceStoreError::NotFound) => {
            known_failure_v1(InteractionEffectKnownFailureClassV1::NotFound)
        }
        TeardownError::Lookup(InstanceStoreError::DuplicateInstance)
        | TeardownError::Store(InstanceStoreError::DuplicateInstance) => {
            known_failure_v1(InteractionEffectKnownFailureClassV1::Conflict)
        }
        TeardownError::Lookup(InstanceStoreError::TimedOut)
        | TeardownError::Store(InstanceStoreError::TimedOut) => {
            InteractionEffectAttemptOutcomeV1::Indeterminate(
                InteractionEffectIndeterminateClassV1::DeadlineElapsed,
            )
        }
        TeardownError::Lookup(InstanceStoreError::Backend(_))
        | TeardownError::Store(InstanceStoreError::Backend(_))
        | TeardownError::Store(InstanceStoreError::NotFound)
        | TeardownError::DeleteFailed { .. } => InteractionEffectAttemptOutcomeV1::Indeterminate(
            InteractionEffectIndeterminateClassV1::ProviderUnavailable,
        ),
    }
}

fn adapter_error_outcome_v1(kind: AdapterErrorKind) -> InteractionEffectAttemptOutcomeV1 {
    match kind {
        AdapterErrorKind::Forbidden => {
            known_failure_v1(InteractionEffectKnownFailureClassV1::Forbidden)
        }
        AdapterErrorKind::NotFound => {
            known_failure_v1(InteractionEffectKnownFailureClassV1::NotFound)
        }
        AdapterErrorKind::RateLimited => {
            known_failure_v1(InteractionEffectKnownFailureClassV1::RateLimitedBeforeDispatch)
        }
        AdapterErrorKind::Unsupported => {
            known_failure_v1(InteractionEffectKnownFailureClassV1::Rejected)
        }
        AdapterErrorKind::BadRequest | AdapterErrorKind::InvalidEventRoute => {
            known_failure_v1(InteractionEffectKnownFailureClassV1::InvalidRequest)
        }
        AdapterErrorKind::Network => InteractionEffectAttemptOutcomeV1::Indeterminate(
            InteractionEffectIndeterminateClassV1::ConnectionLost,
        ),
        AdapterErrorKind::Unknown => InteractionEffectAttemptOutcomeV1::Indeterminate(
            InteractionEffectIndeterminateClassV1::Unknown,
        ),
    }
}

fn known_failure_v1(
    class: InteractionEffectKnownFailureClassV1,
) -> InteractionEffectAttemptOutcomeV1 {
    InteractionEffectAttemptOutcomeV1::KnownFailed(
        InteractionEffectKnownFailureV1::new(class, None)
            .expect("a status-free effect failure is valid"),
    )
}

fn protocol_call_result_v1() -> EffectCallResultV1 {
    EffectCallResultV1 {
        outcome: InteractionEffectAttemptOutcomeV1::Indeterminate(
            InteractionEffectIndeterminateClassV1::MalformedResponse,
        ),
        teardown: None,
        registered_instance: None,
    }
}
