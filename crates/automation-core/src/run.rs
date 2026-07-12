use std::collections::{btree_map::Entry, BTreeMap};

use automation_instance::{
    AutomationInstance, InstanceId, InstanceIdGenerator, InstanceMessageRef, InstanceResources,
    InstanceStatus, InstanceStore,
};
use automation_instance_teardown::InstanceTeardownService;
use automation_state::{
    ButtonRoute, ButtonSpec, InstanceRef, InstanceResourceRefs, InteractionRuleSet,
};
use discord_model::{ChannelId, OverwriteTarget, RoleId};
use resource_resolution::ResourceBindingMap;

use crate::adapter::{
    AdapterError, AdapterErrorKind, AutomationServices, CreateChannelSpec, CreateRoleSpec,
    DiscordMutationAdapter, InteractionResponder, PostPanelButtonSpec, PostPanelSpec,
    ResolvedButtonRoute,
};
use crate::event::{EventKind, RunningRuleSetIdentity, RuntimeContext, RuntimeEvent};
use crate::interpret::interpret;
use crate::plan::{
    ActionPlan, CreatedResource, PlannedAction, PlannedChannel, PlannedOverwriteTarget,
    PlannedRole, ResponseDeliveryOutcome, RunResult, TeardownActionResult,
};
use crate::template::{SanitizeContext, TemplateError, TemplateString};

#[derive(Default)]
struct RuntimeBindings {
    created_roles: BTreeMap<String, RoleId>,
    created_channels: BTreeMap<String, ChannelId>,
    created_messages: BTreeMap<String, InstanceMessageRef>,
    planned_instances: BTreeMap<String, InstanceId>,
    created_instances: BTreeMap<String, InstanceId>,
}

pub async fn run<M, R, S, G, T>(
    context: &RuntimeContext,
    plan: &ActionPlan,
    services: &AutomationServices<'_, M, R, S, G, T>,
) -> Result<RunResult, AdapterError>
where
    M: DiscordMutationAdapter,
    R: InteractionResponder,
    S: InstanceStore,
    G: InstanceIdGenerator,
    T: InstanceTeardownService,
{
    let mut created = Vec::new();
    let mut teardowns: Vec<TeardownActionResult> = Vec::new();
    let mut runtime = RuntimeBindings {
        planned_instances: prepare_instance_identities(plan, services.instance_ids)?,
        ..RuntimeBindings::default()
    };
    for (action_index, step) in plan.steps.iter().enumerate() {
        match step {
            PlannedAction::GrantRole { role, target } => {
                let role_id = resolve_planned_role(role, &runtime, context)?;
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
                        OverwriteTarget::Role(resolve_planned_role(role, &runtime, context)?)
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
                            buttons: resolve_panel_buttons(buttons, &runtime, context)?,
                        },
                    )
                    .await?;
                runtime.created_messages.insert(
                    key.clone(),
                    InstanceMessageRef {
                        channel: channel_id,
                        id,
                    },
                );
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
                if let Err(error) = services.responder.edit_response(rendered).await {
                    if teardowns.is_empty() {
                        return Err(error);
                    }
                    for teardown in &mut teardowns {
                        teardown.response = ResponseDeliveryOutcome::Failed;
                    }
                }
            }
            PlannedAction::RegisterInstance {
                key,
                kind,
                resources,
            } => {
                let id = runtime
                    .planned_instances
                    .get(key)
                    .cloned()
                    .ok_or_else(|| unresolved_planned_instance(key))?;
                let resolved = resolve_manifest(resources, &runtime)?;
                let instance = AutomationInstance {
                    id: id.clone(),
                    guild_id: context.guild_id,
                    ruleset_key: context.ruleset_key.clone(),
                    ruleset_version: context.ruleset_version,
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
            PlannedAction::TeardownInstance { instance } => {
                let instance_id = resolve_instance_ref(instance, &runtime, context)?;
                let outcome = services
                    .teardown
                    .teardown(context.guild_id, instance_id.clone())
                    .await
                    .map_err(teardown_error)?;
                teardowns.push(TeardownActionResult {
                    action_index,
                    instance_id,
                    teardown: outcome,
                    response: ResponseDeliveryOutcome::Sent,
                });
            }
        }
    }
    Ok(RunResult { created, teardowns })
}

fn prepare_instance_identities<G>(
    plan: &ActionPlan,
    instance_ids: &G,
) -> Result<BTreeMap<String, InstanceId>, AdapterError>
where
    G: InstanceIdGenerator,
{
    let mut planned = BTreeMap::new();
    for step in &plan.steps {
        if let PlannedAction::RegisterInstance { key, .. } = step {
            if let Entry::Vacant(entry) = planned.entry(key.clone()) {
                let id = instance_ids.generate().map_err(instance_id_error)?;
                entry.insert(id);
            }
        }
    }
    Ok(planned)
}

fn resolve_planned_role(
    role: &PlannedRole,
    runtime: &RuntimeBindings,
    context: &RuntimeContext,
) -> Result<RoleId, AdapterError> {
    match role {
        PlannedRole::Resolved(id) => Ok(*id),
        PlannedRole::Created(key) => runtime
            .created_roles
            .get(key)
            .copied()
            .ok_or_else(|| unresolved_created_role(key)),
        PlannedRole::Instance { alias } => {
            let resolved = context
                .instance
                .as_ref()
                .ok_or_else(|| instance_context_missing(alias))?;
            resolved
                .instance
                .resources
                .roles
                .get(alias)
                .copied()
                .ok_or_else(|| instance_resource_not_found(&resolved.instance.id, alias))
        }
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

fn resolve_panel_buttons(
    buttons: &[ButtonSpec],
    runtime: &RuntimeBindings,
    context: &RuntimeContext,
) -> Result<Vec<PostPanelButtonSpec>, AdapterError> {
    buttons
        .iter()
        .map(|button| {
            let route = match &button.route {
                ButtonRoute::Static { key } => ResolvedButtonRoute::Static { key: key.clone() },
                ButtonRoute::InstanceAction { instance, action } => {
                    ResolvedButtonRoute::InstanceAction {
                        instance_id: resolve_button_instance(instance, runtime, context)?,
                        action: action.clone(),
                    }
                }
            };
            Ok(PostPanelButtonSpec {
                label: button.label.clone(),
                route,
            })
        })
        .collect()
}

fn resolve_button_instance(
    instance: &InstanceRef,
    runtime: &RuntimeBindings,
    context: &RuntimeContext,
) -> Result<InstanceId, AdapterError> {
    match instance {
        InstanceRef::Created(created) => runtime
            .planned_instances
            .get(&created.created)
            .cloned()
            .ok_or_else(|| unresolved_planned_instance(&created.created)),
        InstanceRef::Event => context
            .instance
            .as_ref()
            .map(|resolved| resolved.instance.id.clone())
            .ok_or_else(event_instance_missing),
    }
}

fn resolve_instance_ref(
    instance: &InstanceRef,
    runtime: &RuntimeBindings,
    context: &RuntimeContext,
) -> Result<InstanceId, AdapterError> {
    match instance {
        InstanceRef::Event => context
            .instance
            .as_ref()
            .map(|resolved| resolved.instance.id.clone())
            .ok_or_else(event_instance_missing),
        InstanceRef::Created(created) => runtime
            .created_instances
            .get(&created.created)
            .or_else(|| runtime.planned_instances.get(&created.created))
            .cloned()
            .ok_or_else(|| unresolved_planned_instance(&created.created)),
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
        let message = runtime
            .created_messages
            .get(&created.created)
            .cloned()
            .ok_or_else(|| unresolved_manifest(&created.created))?;
        resources.messages.insert(alias.clone(), message);
    }
    Ok(resources)
}

fn unresolved_manifest(key: &str) -> AdapterError {
    AdapterError::new(
        AdapterErrorKind::BadRequest,
        format!("unresolved manifest ref: {key}"),
    )
}

fn instance_id_error(error: automation_instance::InstanceIdGenerationError) -> AdapterError {
    AdapterError::new(
        AdapterErrorKind::BadRequest,
        format!("instance id error: {error:?}"),
    )
}

fn unresolved_planned_instance(key: &str) -> AdapterError {
    AdapterError::new(
        AdapterErrorKind::BadRequest,
        format!("unresolved planned instance: {key}"),
    )
}

fn teardown_error(error: automation_instance_teardown::TeardownError) -> AdapterError {
    AdapterError::new(
        AdapterErrorKind::BadRequest,
        format!("instance teardown error: {error:?}"),
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

fn event_instance_missing() -> AdapterError {
    AdapterError::new(
        AdapterErrorKind::BadRequest,
        "InstanceContextMissing: button route",
    )
}

fn instance_context_missing(alias: &str) -> AdapterError {
    AdapterError::new(
        AdapterErrorKind::BadRequest,
        format!("InstanceContextMissing: role alias={alias}"),
    )
}

fn instance_resource_not_found(instance_id: &InstanceId, alias: &str) -> AdapterError {
    AdapterError::new(
        AdapterErrorKind::BadRequest,
        format!("InstanceResourceNotFound: instance={instance_id}, resource=role, alias={alias}"),
    )
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HandleOutcome {
    Executed,
    NoOp,
}

pub async fn handle_event<M, R, S, G, T>(
    event: &RuntimeEvent,
    ruleset: &InteractionRuleSet,
    bindings: &ResourceBindingMap,
    services: &AutomationServices<'_, M, R, S, G, T>,
    failure_message: &str,
    identity: &RunningRuleSetIdentity,
) -> Result<HandleOutcome, AdapterError>
where
    M: DiscordMutationAdapter,
    R: InteractionResponder,
    S: InstanceStore,
    G: InstanceIdGenerator,
    T: InstanceTeardownService,
{
    if let EventKind::InstanceAction { .. } = &event.kind {
        return Err(AdapterError::new(
            AdapterErrorKind::InvalidEventRoute,
            "InstanceAction must be dispatched via automation-ruleset-dispatch",
        ));
    }
    let context = RuntimeContext::from_event(event, identity);
    match interpret(event, ruleset, bindings) {
        Some(plan) => {
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
