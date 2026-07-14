use automation_instance::{AutomationInstance, InstanceId, InstanceStatus};
use discord_model::{ChannelId, OverwriteTarget, Permissions, RoleId, UserId};

use crate::adapter::{AdapterError, PostPanelSpec};
use crate::event::RuntimeContext;
use crate::plan::{ModalPresentation, PlannedAction, PlannedOverwriteTarget};
use crate::template::{SanitizeContext, TemplateError, TemplateString};

use super::state::{unresolved_planned_instance, ExecutionState};

pub(super) enum PreparedAction {
    GrantRole {
        role: RoleId,
        target: UserId,
    },
    RespondEphemeral {
        content: String,
    },
    OpenModal(ModalPresentation),
    CreateChannel {
        action_index: usize,
        key: String,
        name: String,
    },
    CreateRole {
        action_index: usize,
        key: String,
        name: String,
    },
    UpsertOverwrite {
        channel: ChannelId,
        target: OverwriteTarget,
        allow: Permissions,
        deny: Permissions,
    },
    PostPanel {
        action_index: usize,
        key: String,
        channel: ChannelId,
        spec: PostPanelSpec,
    },
    DeferEphemeral,
    EditResponse {
        content: String,
    },
    RegisterInstance {
        action_index: usize,
        key: String,
        id: InstanceId,
        instance: AutomationInstance,
    },
    TeardownInstance {
        action_index: usize,
        instance_id: InstanceId,
    },
}

pub(super) fn prepare_action(
    action_index: usize,
    step: &PlannedAction,
    context: &RuntimeContext,
    state: &ExecutionState,
) -> Result<PreparedAction, AdapterError> {
    match step {
        PlannedAction::GrantRole { role, target } => Ok(PreparedAction::GrantRole {
            role: state.resolve_role(role, context)?,
            target: *target,
        }),
        PlannedAction::RespondEphemeral { content } => Ok(PreparedAction::RespondEphemeral {
            content: render(content, context, SanitizeContext::EphemeralMessageContent)?,
        }),
        PlannedAction::OpenModal(modal) => Ok(PreparedAction::OpenModal(modal.clone())),
        PlannedAction::CreateChannel { key, name } => Ok(PreparedAction::CreateChannel {
            action_index,
            key: key.clone(),
            name: render(name, context, SanitizeContext::ChannelName)?,
        }),
        PlannedAction::CreateRole { key, name } => Ok(PreparedAction::CreateRole {
            action_index,
            key: key.clone(),
            name: render(name, context, SanitizeContext::RoleName)?,
        }),
        PlannedAction::UpsertOverwrite {
            channel,
            target,
            allow,
            deny,
        } => {
            let channel = state.resolve_channel(channel)?;
            let target = match target {
                PlannedOverwriteTarget::Everyone => {
                    OverwriteTarget::Role(RoleId(context.guild_id.0))
                }
                PlannedOverwriteTarget::Role(role) => {
                    OverwriteTarget::Role(state.resolve_role(role, context)?)
                }
            };
            Ok(PreparedAction::UpsertOverwrite {
                channel,
                target,
                allow: *allow,
                deny: *deny,
            })
        }
        PlannedAction::PostPanel {
            key,
            channel,
            content,
            buttons,
        } => Ok(PreparedAction::PostPanel {
            action_index,
            key: key.clone(),
            channel: state.resolve_channel(channel)?,
            spec: PostPanelSpec {
                content: render(content, context, SanitizeContext::EphemeralMessageContent)?,
                buttons: state.resolve_panel_buttons(buttons, context)?,
            },
        }),
        PlannedAction::DeferEphemeral => Ok(PreparedAction::DeferEphemeral),
        PlannedAction::EditResponse { content } => Ok(PreparedAction::EditResponse {
            content: render(content, context, SanitizeContext::EphemeralMessageContent)?,
        }),
        PlannedAction::RegisterInstance {
            key,
            kind,
            resources,
        } => {
            let id = state
                .planned_instances
                .get(key)
                .cloned()
                .ok_or_else(|| unresolved_planned_instance(key))?;
            let instance = AutomationInstance {
                id: id.clone(),
                guild_id: context.guild_id,
                ruleset_key: context.ruleset_key.clone(),
                ruleset_version: context.ruleset_version,
                kind: kind.clone(),
                created_by: context.actor,
                resources: state.resolve_manifest(resources)?,
                status: InstanceStatus::Active,
            };
            Ok(PreparedAction::RegisterInstance {
                action_index,
                key: key.clone(),
                id,
                instance,
            })
        }
        PlannedAction::TeardownInstance { instance } => Ok(PreparedAction::TeardownInstance {
            action_index,
            instance_id: state.resolve_instance_ref(instance, context)?,
        }),
    }
}

pub(super) fn prepare_failure_message(
    source: &str,
    context: &RuntimeContext,
) -> Result<String, AdapterError> {
    render(source, context, SanitizeContext::EphemeralMessageContent)
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
        crate::adapter::AdapterErrorKind::BadRequest,
        format!("template error: {error:?}"),
    )
}
