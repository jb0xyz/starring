use std::collections::BTreeMap;

use automation_instance::{
    AutomationInstance, InstanceId, InstanceIdGenerator, InstanceMessageRef, InstanceRegistrarV1,
    InstanceResources, InstanceStatus,
};
use automation_instance_teardown::InstanceTeardownService;
use discord_model::{ChannelId, OverwriteTarget, RoleId};

use crate::adapter::{
    AdapterError, AdapterErrorKind, AutomationServices, CreateChannelSpec, CreateRoleSpec,
    DiscordMutationAdapter, InteractionResponder, PostPanelButtonSpec, PostPanelSpec,
    ResolvedButtonRoute,
};
use crate::plan::{CreatedResource, ResponseDeliveryOutcome, RunResult, TeardownActionResult};

use super::types::{
    ActionEntryIdV1, ActionPlanPreflightErrorV1, ActionPlanSnapshotIdentityV1,
    PreflightButtonRouteV1, PreflightChannelRefV1, PreflightInstanceRefV1,
    PreflightInstanceResourceRefsV1, PreflightOverwriteTargetV1, PreflightRoleRefV1,
    PreflightedActionPlanV1, PreparedPlanActionV1,
};

#[derive(Default)]
struct PreflightExecutionStateV1 {
    roles: BTreeMap<ActionEntryIdV1, RoleId>,
    channels: BTreeMap<ActionEntryIdV1, ChannelId>,
    messages: BTreeMap<ActionEntryIdV1, InstanceMessageRef>,
    instances: BTreeMap<ActionEntryIdV1, InstanceId>,
}

pub async fn execute_preflighted_action_plan_v1<M, R, S, G, T>(
    plan: PreflightedActionPlanV1,
    expected_snapshot_identity: &ActionPlanSnapshotIdentityV1,
    services: &AutomationServices<'_, M, R, S, G, T>,
) -> Result<RunResult, AdapterError>
where
    M: DiscordMutationAdapter,
    R: InteractionResponder,
    S: InstanceRegistrarV1,
    G: InstanceIdGenerator,
    T: InstanceTeardownService,
{
    if plan.snapshot_identity() != expected_snapshot_identity {
        return Err(preflight_error(ActionPlanPreflightErrorV1::SnapshotDrift));
    }
    let mut state = PreflightExecutionStateV1::default();
    let mut created = Vec::new();
    let mut teardowns: Vec<TeardownActionResult> = Vec::new();
    let guild_id = plan.context().guild_id;
    for action in plan.actions() {
        match action {
            PreparedPlanActionV1::GrantRole {
                entry,
                role,
                target,
            } => {
                let role = state.resolve_role(*entry, role)?;
                services
                    .mutation
                    .grant_role(guild_id, *target, role)
                    .await?;
            }
            PreparedPlanActionV1::RespondEphemeral { content, .. } => {
                services
                    .responder
                    .respond_ephemeral(content.clone())
                    .await?;
            }
            PreparedPlanActionV1::OpenModal { modal, .. } => {
                services.responder.open_modal(modal).await?;
            }
            PreparedPlanActionV1::CreateChannel {
                entry,
                output,
                key,
                name,
            } => {
                ensure_output(*entry, output.producer())?;
                let id = services
                    .mutation
                    .create_channel(guild_id, CreateChannelSpec { name: name.clone() })
                    .await?;
                state.channels.insert(output.producer(), id);
                created.push(CreatedResource::Channel {
                    action_index: usize::from(entry.ordinal()),
                    key: key.clone(),
                    name: name.clone(),
                    id,
                });
            }
            PreparedPlanActionV1::CreateRole {
                entry,
                output,
                key,
                name,
            } => {
                ensure_output(*entry, output.producer())?;
                let id = services
                    .mutation
                    .create_role(guild_id, CreateRoleSpec { name: name.clone() })
                    .await?;
                state.roles.insert(output.producer(), id);
                created.push(CreatedResource::Role {
                    action_index: usize::from(entry.ordinal()),
                    key: key.clone(),
                    name: name.clone(),
                    id,
                });
            }
            PreparedPlanActionV1::UpsertOverwrite {
                entry,
                channel,
                target,
                allow,
                deny,
            } => {
                let channel = state.resolve_channel(*entry, channel)?;
                let target = match target {
                    PreflightOverwriteTargetV1::Everyone => {
                        OverwriteTarget::Role(RoleId(guild_id.0))
                    }
                    PreflightOverwriteTargetV1::Role(role) => {
                        OverwriteTarget::Role(state.resolve_role(*entry, role)?)
                    }
                };
                services
                    .mutation
                    .upsert_overwrite(guild_id, channel, target, *allow, *deny)
                    .await?;
            }
            PreparedPlanActionV1::PostPanel {
                entry,
                output,
                key,
                channel,
                content,
                buttons,
            } => {
                ensure_output(*entry, output.producer())?;
                let channel = state.resolve_channel(*entry, channel)?;
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
                let id = services
                    .mutation
                    .post_panel(
                        guild_id,
                        channel,
                        PostPanelSpec {
                            content: content.clone(),
                            buttons,
                        },
                    )
                    .await?;
                state
                    .messages
                    .insert(output.producer(), InstanceMessageRef { channel, id });
                created.push(CreatedResource::Message {
                    action_index: usize::from(entry.ordinal()),
                    key: key.clone(),
                    channel,
                    id,
                });
            }
            PreparedPlanActionV1::DeferEphemeral { .. } => {
                services.responder.defer_ephemeral().await?;
            }
            PreparedPlanActionV1::EditResponse { content, .. } => {
                if let Err(error) = services.responder.edit_response(content.clone()).await {
                    if teardowns.is_empty() {
                        return Err(error);
                    }
                    for teardown in &mut teardowns {
                        teardown.response = ResponseDeliveryOutcome::Failed;
                    }
                }
            }
            PreparedPlanActionV1::RegisterInstance {
                entry,
                output,
                key,
                id,
                kind,
                resources,
            } => {
                ensure_output(*entry, output.producer())?;
                let resources = state.resolve_resources(*entry, resources)?;
                let instance = AutomationInstance {
                    id: id.clone(),
                    guild_id,
                    ruleset_key: plan.context().ruleset_key.clone(),
                    ruleset_version: plan.context().ruleset_version,
                    kind: kind.clone(),
                    created_by: plan.context().actor,
                    resources,
                    status: InstanceStatus::Active,
                };
                services
                    .instances
                    .register_instance_v1(instance)
                    .await
                    .map_err(|error| {
                        AdapterError::new(
                            AdapterErrorKind::BadRequest,
                            format!("instance register error: {error:?}"),
                        )
                    })?;
                state.instances.insert(output.producer(), id.clone());
                created.push(CreatedResource::Instance {
                    action_index: usize::from(entry.ordinal()),
                    key: key.clone(),
                    id: id.clone(),
                });
            }
            PreparedPlanActionV1::TeardownInstance { entry, instance } => {
                let instance_id = match instance {
                    PreflightInstanceRefV1::Existing(instance_id) => instance_id.clone(),
                    PreflightInstanceRefV1::Registered(reference) => state
                        .instances
                        .get(&reference.producer())
                        .cloned()
                        .ok_or_else(|| invariant(*entry))?,
                };
                let teardown = services
                    .teardown
                    .teardown(guild_id, instance_id.clone())
                    .await
                    .map_err(|error| {
                        AdapterError::new(
                            AdapterErrorKind::BadRequest,
                            format!("instance teardown error: {error:?}"),
                        )
                    })?;
                teardowns.push(TeardownActionResult {
                    action_index: usize::from(entry.ordinal()),
                    instance_id,
                    teardown,
                    response: ResponseDeliveryOutcome::Sent,
                });
            }
        }
    }
    Ok(RunResult { created, teardowns })
}

impl PreflightExecutionStateV1 {
    fn resolve_role(
        &self,
        entry: ActionEntryIdV1,
        reference: &PreflightRoleRefV1,
    ) -> Result<RoleId, AdapterError> {
        match reference {
            PreflightRoleRefV1::Existing(role_id) | PreflightRoleRefV1::Instance(role_id) => {
                Ok(*role_id)
            }
            PreflightRoleRefV1::Produced(reference) => self
                .roles
                .get(&reference.producer())
                .copied()
                .ok_or_else(|| invariant(entry)),
        }
    }

    fn resolve_channel(
        &self,
        entry: ActionEntryIdV1,
        reference: &PreflightChannelRefV1,
    ) -> Result<ChannelId, AdapterError> {
        match reference {
            PreflightChannelRefV1::Existing(channel_id) => Ok(*channel_id),
            PreflightChannelRefV1::Produced(reference) => self
                .channels
                .get(&reference.producer())
                .copied()
                .ok_or_else(|| invariant(entry)),
        }
    }

    fn resolve_resources(
        &self,
        entry: ActionEntryIdV1,
        references: &PreflightInstanceResourceRefsV1,
    ) -> Result<InstanceResources, AdapterError> {
        let mut resources = InstanceResources::default();
        for (alias, reference) in &references.roles {
            let id = self
                .roles
                .get(&reference.producer())
                .copied()
                .ok_or_else(|| invariant(entry))?;
            resources.roles.insert(alias.clone(), id);
        }
        for (alias, reference) in &references.channels {
            let id = self
                .channels
                .get(&reference.producer())
                .copied()
                .ok_or_else(|| invariant(entry))?;
            resources.channels.insert(alias.clone(), id);
        }
        for (alias, reference) in &references.messages {
            let message = self
                .messages
                .get(&reference.producer())
                .cloned()
                .ok_or_else(|| invariant(entry))?;
            resources.messages.insert(alias.clone(), message);
        }
        Ok(resources)
    }
}

fn ensure_output(entry: ActionEntryIdV1, producer: ActionEntryIdV1) -> Result<(), AdapterError> {
    if entry == producer {
        Ok(())
    } else {
        Err(invariant(entry))
    }
}

fn invariant(entry: ActionEntryIdV1) -> AdapterError {
    preflight_error(ActionPlanPreflightErrorV1::ExecutionInvariant { entry })
}

fn preflight_error(error: ActionPlanPreflightErrorV1) -> AdapterError {
    AdapterError::new(AdapterErrorKind::BadRequest, error.to_string())
}
