use std::collections::BTreeMap;

use automation_instance::{
    AutomationInstance, InstanceId, InstanceIdGenerator, InstanceResources, InstanceStatus,
    InstanceStore,
};
use automation_state::{InstanceResourceRefs, InteractionRuleSet};
use discord_model::{ChannelId, MessageId, OverwriteTarget, RoleId};
use resource_resolution::ResourceBindingMap;

use crate::adapter::{
    AdapterError, AdapterErrorKind, AutomationServices, CreateChannelSpec, CreateRoleSpec,
    DiscordMutationAdapter, InteractionResponder, PostPanelSpec,
};
use crate::event::{RuntimeContext, RuntimeEvent};
use crate::interpret::interpret;
use crate::plan::{
    ActionPlan, CreatedResource, PlannedAction, PlannedChannel, PlannedOverwriteTarget, PlannedRole,
};
use crate::template::{SanitizeContext, TemplateError, TemplateString};

#[derive(Default)]
struct RuntimeBindings {
    created_roles: BTreeMap<String, RoleId>,
    created_channels: BTreeMap<String, ChannelId>,
    created_messages: BTreeMap<String, MessageId>,
    created_instances: BTreeMap<String, InstanceId>,
}

pub async fn run<M, R, S, G>(
    context: &RuntimeContext,
    plan: &ActionPlan,
    services: &AutomationServices<'_, M, R, S, G>,
) -> Result<Vec<CreatedResource>, AdapterError>
where
    M: DiscordMutationAdapter,
    R: InteractionResponder,
    S: InstanceStore,
    G: InstanceIdGenerator,
{
    let mut created = Vec::new();
    let mut runtime = RuntimeBindings::default();
    for (action_index, step) in plan.steps.iter().enumerate() {
        match step {
            PlannedAction::GrantRole { role, target } => {
                let role_id = resolve_planned_role(role, &runtime)?;
                services
                    .mutation
                    .grant_role(context.guild_id, *target, role_id)
                    .await?;
            }
            PlannedAction::RespondEphemeral { content } => {
                let rendered = render(content, context, SanitizeContext::EphemeralMessageContent)?;
                services.responder.respond_ephemeral(rendered).await?;
            }
            PlannedAction::OpenModal(modal) => {
                services.responder.open_modal(modal).await?;
            }
            PlannedAction::CreateChannel { key, name } => {
                let rendered = render(name, context, SanitizeContext::ChannelName)?;
                let id = services
                    .mutation
                    .create_channel(
                        context.guild_id,
                        CreateChannelSpec {
                            name: rendered.clone(),
                        },
                    )
                    .await?;
                runtime.created_channels.insert(key.clone(), id);
                created.push(CreatedResource::Channel {
                    action_index,
                    key: key.clone(),
                    name: rendered,
                    id,
                });
            }
            PlannedAction::CreateRole { key, name } => {
                let rendered = render(name, context, SanitizeContext::RoleName)?;
                let id = services
                    .mutation
                    .create_role(
                        context.guild_id,
                        CreateRoleSpec {
                            name: rendered.clone(),
                        },
                    )
                    .await?;
                runtime.created_roles.insert(key.clone(), id);
                created.push(CreatedResource::Role {
                    action_index,
                    key: key.clone(),
                    name: rendered,
                    id,
                });
            }
            PlannedAction::UpsertOverwrite {
                channel,
                target,
                allow,
                deny,
            } => {
                let channel_id = resolve_planned_channel(channel, &runtime)?;
                let overwrite_target = match target {
                    PlannedOverwriteTarget::Everyone => {
                        OverwriteTarget::Role(RoleId(context.guild_id.0))
                    }
                    PlannedOverwriteTarget::Role(role) => {
                        OverwriteTarget::Role(resolve_planned_role(role, &runtime)?)
                    }
                };
                services
                    .mutation
                    .upsert_overwrite(
                        context.guild_id,
                        channel_id,
                        overwrite_target,
                        *allow,
                        *deny,
                    )
                    .await?;
            }
            PlannedAction::PostPanel {
                key,
                channel,
                content,
                buttons,
            } => {
                let channel_id = resolve_planned_channel(channel, &runtime)?;
                let rendered = render(content, context, SanitizeContext::EphemeralMessageContent)?;
                let id = services
                    .mutation
                    .post_panel(
                        context.guild_id,
                        channel_id,
                        PostPanelSpec {
                            content: rendered,
                            buttons: buttons.clone(),
                        },
                    )
                    .await?;
                runtime.created_messages.insert(key.clone(), id);
                created.push(CreatedResource::Message {
                    action_index,
                    key: key.clone(),
                    channel: channel_id,
                    id,
                });
            }
            PlannedAction::DeferEphemeral => {
                services.responder.defer_ephemeral().await?;
            }
            PlannedAction::EditResponse { content } => {
                let rendered = render(content, context, SanitizeContext::EphemeralMessageContent)?;
                services.responder.edit_response(rendered).await?;
            }
            PlannedAction::RegisterInstance {
                key,
                kind,
                resources,
            } => {
                let resolved = resolve_manifest(resources, &runtime)?;
                let id = services.instance_ids.generate().map_err(|error| {
                    AdapterError::new(
                        AdapterErrorKind::BadRequest,
                        format!("instance id error: {error:?}"),
                    )
                })?;
                let instance = AutomationInstance {
                    id: id.clone(),
                    guild_id: context.guild_id,
                    ruleset_key: context.ruleset_key.clone(),
                    kind: kind.clone(),
                    created_by: context.actor,
                    resources: resolved,
                    status: InstanceStatus::Active,
                };
                services
                    .instances
                    .register(instance)
                    .await
                    .map_err(|error| {
                        AdapterError::new(
                            AdapterErrorKind::BadRequest,
                            format!("instance register error: {error:?}"),
                        )
                    })?;
                runtime.created_instances.insert(key.clone(), id.clone());
                created.push(CreatedResource::Instance {
                    action_index,
                    key: key.clone(),
                    id,
                });
            }
        }
    }
    Ok(created)
}

fn resolve_planned_role(
    role: &PlannedRole,
    runtime: &RuntimeBindings,
) -> Result<RoleId, AdapterError> {
    match role {
        PlannedRole::Resolved(id) => Ok(*id),
        PlannedRole::Created(key) => runtime
            .created_roles
            .get(key)
            .copied()
            .ok_or_else(|| unresolved_created_role(key)),
    }
}

fn resolve_planned_channel(
    channel: &PlannedChannel,
    runtime: &RuntimeBindings,
) -> Result<ChannelId, AdapterError> {
    match channel {
        PlannedChannel::Resolved(id) => Ok(*id),
        PlannedChannel::Created(key) => runtime
            .created_channels
            .get(key)
            .copied()
            .ok_or_else(|| unresolved_created_channel(key)),
    }
}

fn resolve_manifest(
    refs: &InstanceResourceRefs,
    runtime: &RuntimeBindings,
) -> Result<InstanceResources, AdapterError> {
    let mut resources = InstanceResources::default();
    for (alias, created) in &refs.roles {
        let id = runtime
            .created_roles
            .get(&created.created)
            .copied()
            .ok_or_else(|| unresolved_manifest(&created.created))?;
        resources.roles.insert(alias.clone(), id);
    }
    for (alias, created) in &refs.channels {
        let id = runtime
            .created_channels
            .get(&created.created)
            .copied()
            .ok_or_else(|| unresolved_manifest(&created.created))?;
        resources.channels.insert(alias.clone(), id);
    }
    for (alias, created) in &refs.messages {
        let id = runtime
            .created_messages
            .get(&created.created)
            .copied()
            .ok_or_else(|| unresolved_manifest(&created.created))?;
        resources.messages.insert(alias.clone(), id);
    }
    Ok(resources)
}

fn unresolved_manifest(key: &str) -> AdapterError {
    AdapterError::new(
        AdapterErrorKind::BadRequest,
        format!("unresolved manifest ref: {key}"),
    )
}

fn render(
    source: &str,
    context: &RuntimeContext,
    sanitize: SanitizeContext,
) -> Result<String, AdapterError> {
    TemplateString::parse(source)
        .and_then(|template| template.render(&context.inputs, sanitize))
        .map_err(template_error)
}

fn template_error(error: TemplateError) -> AdapterError {
    AdapterError::new(
        AdapterErrorKind::BadRequest,
        format!("template error: {error:?}"),
    )
}

fn unresolved_created_role(key: &str) -> AdapterError {
    AdapterError::new(
        AdapterErrorKind::BadRequest,
        format!("unresolved created role: {key}"),
    )
}

fn unresolved_created_channel(key: &str) -> AdapterError {
    AdapterError::new(
        AdapterErrorKind::BadRequest,
        format!("unresolved created channel: {key}"),
    )
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HandleOutcome {
    Executed,
    NoOp,
}

pub async fn handle_event<M, R, S, G>(
    event: &RuntimeEvent,
    ruleset: &InteractionRuleSet,
    bindings: &ResourceBindingMap,
    services: &AutomationServices<'_, M, R, S, G>,
    failure_message: &str,
    ruleset_key: &str,
) -> Result<HandleOutcome, AdapterError>
where
    M: DiscordMutationAdapter,
    R: InteractionResponder,
    S: InstanceStore,
    G: InstanceIdGenerator,
{
    match interpret(event, ruleset, bindings) {
        Some(plan) => {
            let context = RuntimeContext::from_event(event, ruleset_key);
            let mut steps = plan.steps;
            let defer_acked = if matches!(steps.first(), Some(PlannedAction::DeferEphemeral)) {
                services.responder.defer_ephemeral().await?;
                steps.remove(0);
                true
            } else {
                false
            };
            match run(&context, &ActionPlan { steps }, services).await {
                Ok(_) => Ok(HandleOutcome::Executed),
                Err(error) => {
                    if defer_acked {
                        if let Ok(rendered) = render(
                            failure_message,
                            &context,
                            SanitizeContext::EphemeralMessageContent,
                        ) {
                            let _ = services.responder.edit_response(rendered).await;
                        }
                    }
                    Err(error)
                }
            }
        }
        None => Ok(HandleOutcome::NoOp),
    }
}
