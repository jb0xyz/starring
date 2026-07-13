use automation_state::{
    ActionSpec, ActionTarget, ButtonRoute, ButtonSpec, ChannelRef, CreatedRef, InstanceRef,
    InteractionRule, RoleRef,
};
use serde_json::{json, Value};

use crate::draft::Draft;
use crate::errors::StructuredError;

use super::{
    action_not_found, find_rule_mut, reference_conversion_error, ActionKindInput, ActionPatchInput,
    ActionSelectorInput, ActorTargetInput, AddGrantRoleActionInput, AddPostPanelActionInput,
    AddUpsertOverwriteActionInput, ButtonRouteInput, InteractionActionInput,
    OverwriteTargetKindInput, ReferenceInput, RemoveActionInput, ResourceActionInput,
    UpdateActionInput, PENDING_INSTANCE_REFERENCE,
};

pub(super) fn add_resource_action(
    draft: &mut Draft,
    input: ResourceActionInput,
) -> Result<String, StructuredError> {
    let (rule_key, action, change) = match input {
        ResourceActionInput::CreateRole {
            rule_key,
            key,
            name,
        } => {
            let change = format!("Added CreateRole {key} to rule {rule_key}");
            (rule_key, ActionSpec::CreateRole { key, name }, change)
        }
        ResourceActionInput::CreateChannel {
            rule_key,
            key,
            name,
        } => {
            let change = format!("Added CreateChannel {key} to rule {rule_key}");
            (rule_key, ActionSpec::CreateChannel { key, name }, change)
        }
    };
    append_action(draft, &rule_key, action)?;
    Ok(change)
}

pub(super) fn add_grant_role_action(
    draft: &mut Draft,
    input: AddGrantRoleActionInput,
) -> Result<String, StructuredError> {
    let target = match input.target {
        ActorTargetInput::Actor => ActionTarget::Actor,
    };
    let action = ActionSpec::GrantRole {
        role: role_reference(input.role)?,
        target,
    };
    append_action(draft, &input.rule_key, action)?;
    Ok(format!("Added GrantRole to rule {}", input.rule_key))
}

pub(super) fn add_upsert_overwrite_action(
    draft: &mut Draft,
    input: AddUpsertOverwriteActionInput,
) -> Result<String, StructuredError> {
    let action = overwrite_action(
        input.channel,
        input.target_kind,
        input.role,
        &input.allow,
        &input.deny,
    )?;
    append_action(draft, &input.rule_key, action)?;
    Ok(format!("Added UpsertOverwrite to rule {}", input.rule_key))
}

pub(super) fn add_interaction_action(
    draft: &mut Draft,
    input: InteractionActionInput,
) -> Result<String, StructuredError> {
    let (rule_key, action, label) = match input {
        InteractionActionInput::OpenModal { rule_key, modal } => {
            (rule_key, ActionSpec::OpenModal { modal }, "OpenModal")
        }
        InteractionActionInput::RespondEphemeral { rule_key, content } => (
            rule_key,
            ActionSpec::RespondEphemeral { content },
            "RespondEphemeral",
        ),
        InteractionActionInput::DeferEphemeral { rule_key } => {
            (rule_key, ActionSpec::DeferEphemeral, "DeferEphemeral")
        }
        InteractionActionInput::EditResponse { rule_key, content } => (
            rule_key,
            ActionSpec::EditResponse { content },
            "EditResponse",
        ),
        InteractionActionInput::TeardownInstance { rule_key } => (
            rule_key,
            ActionSpec::TeardownInstance {
                instance: InstanceRef::Event,
            },
            "TeardownInstance",
        ),
    };
    append_action(draft, &rule_key, action)?;
    Ok(format!("Added {label} to rule {rule_key}"))
}

pub(super) fn add_post_panel_action(
    draft: &mut Draft,
    input: AddPostPanelActionInput,
) -> Result<String, StructuredError> {
    let action = ActionSpec::PostPanel {
        key: input.key.clone(),
        channel: channel_reference(input.channel)?,
        content: input.content,
        buttons: input
            .buttons
            .into_iter()
            .map(|button| ButtonSpec {
                label: button.label,
                route: pending_button_route(button.route),
            })
            .collect(),
    };
    append_action(draft, &input.rule_key, action)?;
    Ok(format!(
        "Added PostPanel {} to rule {}",
        input.key, input.rule_key
    ))
}

pub(super) fn update_action(
    draft: &mut Draft,
    input: UpdateActionInput,
) -> Result<String, StructuredError> {
    let rule = find_rule_mut(draft, &input.rule_key)?;
    let index = action_index(rule, &input.selector, &input.rule_key)?;
    let registration = rule.actions.iter().find_map(|action| match action {
        ActionSpec::RegisterInstance { key, .. } => Some(key.clone()),
        _ => None,
    });
    apply_action_patch(
        &mut rule.actions[index],
        input.patch,
        registration.as_deref(),
        &input.rule_key,
    )?;
    Ok(format!("Updated action in rule {}", input.rule_key))
}

pub(super) fn remove_action(
    draft: &mut Draft,
    input: RemoveActionInput,
) -> Result<String, StructuredError> {
    let rule = find_rule_mut(draft, &input.rule_key)?;
    let index = action_index(rule, &input.selector, &input.rule_key)?;
    rule.actions.remove(index);
    Ok(format!("Removed action from rule {}", input.rule_key))
}

fn apply_action_patch(
    action: &mut ActionSpec,
    patch: ActionPatchInput,
    registration: Option<&str>,
    rule_key: &str,
) -> Result<(), StructuredError> {
    match (action, patch) {
        (ActionSpec::CreateRole { name, .. }, ActionPatchInput::CreateRole { name: value })
        | (
            ActionSpec::CreateChannel { name, .. },
            ActionPatchInput::CreateChannel { name: value },
        ) => {
            *name = value;
        }
        (
            ActionSpec::GrantRole { role, target },
            ActionPatchInput::GrantRole {
                role: value,
                target: actor,
            },
        ) => {
            *role = role_reference(value)?;
            *target = match actor {
                ActorTargetInput::Actor => ActionTarget::Actor,
            };
        }
        (
            ActionSpec::RespondEphemeral { content },
            ActionPatchInput::RespondEphemeral { content: value },
        )
        | (
            ActionSpec::EditResponse { content },
            ActionPatchInput::EditResponse { content: value },
        ) => {
            *content = value;
        }
        (ActionSpec::OpenModal { modal }, ActionPatchInput::OpenModal { modal: value }) => {
            *modal = value;
        }
        (
            slot @ ActionSpec::UpsertOverwrite { .. },
            ActionPatchInput::UpsertOverwrite {
                channel,
                target_kind,
                role,
                allow,
                deny,
            },
        ) => {
            *slot = overwrite_action(channel, target_kind, role, &allow, &deny)?;
        }
        (
            ActionSpec::PostPanel {
                channel,
                content,
                buttons,
                ..
            },
            ActionPatchInput::PostPanel {
                channel: value,
                content: next_content,
                buttons: next_buttons,
            },
        ) => {
            *channel = channel_reference(value)?;
            *content = next_content;
            *buttons = next_buttons
                .into_iter()
                .map(|button| ButtonSpec {
                    label: button.label,
                    route: registered_button_route(button.route, registration),
                })
                .collect();
        }
        (_, patch) => {
            return Err(StructuredError::new(
                "ACTION_PATCH_KIND_MISMATCH",
                format!("rule.{rule_key}.actions"),
                format!(
                    "The selected action cannot use a {} patch",
                    action_patch_kind(&patch)
                ),
                "Select an action of the same kind or use the matching patch kind",
            ));
        }
    }
    Ok(())
}

fn append_action(
    draft: &mut Draft,
    rule_key: &str,
    action: ActionSpec,
) -> Result<(), StructuredError> {
    let rule = find_rule_mut(draft, rule_key)?;
    if matches!(action, ActionSpec::DeferEphemeral) {
        rule.actions.insert(0, action);
        return Ok(());
    }
    if matches!(action, ActionSpec::EditResponse { .. }) {
        rule.actions.push(action);
        return Ok(());
    }
    if let Some(index) = rule
        .actions
        .iter()
        .position(|candidate| matches!(candidate, ActionSpec::RegisterInstance { .. }))
    {
        rule.actions.insert(index, action);
        return Ok(());
    }
    insert_before_edit(rule, action);
    Ok(())
}

pub(super) fn insert_before_edit(rule: &mut InteractionRule, action: ActionSpec) {
    let index = rule
        .actions
        .iter()
        .position(|candidate| matches!(candidate, ActionSpec::EditResponse { .. }))
        .unwrap_or(rule.actions.len());
    rule.actions.insert(index, action);
}

pub(super) fn registered_button_route(
    input: ButtonRouteInput,
    registration: Option<&str>,
) -> ButtonRoute {
    match input {
        ButtonRouteInput::Static { key } => ButtonRoute::Static { key },
        ButtonRouteInput::InstanceAction { action } => ButtonRoute::InstanceAction {
            instance: InstanceRef::Created(CreatedRef {
                created: registration
                    .unwrap_or(PENDING_INSTANCE_REFERENCE)
                    .to_string(),
            }),
            action,
        },
    }
}

fn pending_button_route(input: ButtonRouteInput) -> ButtonRoute {
    match input {
        ButtonRouteInput::Static { key } => ButtonRoute::Static { key },
        ButtonRouteInput::InstanceAction { action } => ButtonRoute::InstanceAction {
            instance: InstanceRef::Created(CreatedRef {
                created: PENDING_INSTANCE_REFERENCE.to_string(),
            }),
            action,
        },
    }
}

fn role_reference(input: ReferenceInput) -> Result<RoleRef, StructuredError> {
    match input {
        ReferenceInput::Created { name } => Ok(RoleRef::Created(CreatedRef { created: name })),
        ReferenceInput::Existing { name } => serde_json::from_value(Value::String(name))
            .map(RoleRef::Existing)
            .map_err(reference_conversion_error),
    }
}

fn channel_reference(input: ReferenceInput) -> Result<ChannelRef, StructuredError> {
    match input {
        ReferenceInput::Created { name } => Ok(ChannelRef::Created(CreatedRef { created: name })),
        ReferenceInput::Existing { name } => serde_json::from_value(Value::String(name))
            .map(ChannelRef::Existing)
            .map_err(reference_conversion_error),
    }
}

fn action_index(
    rule: &InteractionRule,
    selector: &ActionSelectorInput,
    rule_key: &str,
) -> Result<usize, StructuredError> {
    match selector {
        ActionSelectorInput::ByKey { key } => {
            let matches: Vec<usize> = rule
                .actions
                .iter()
                .enumerate()
                .filter_map(|(index, action)| {
                    (action_key(action) == Some(key.as_str())).then_some(index)
                })
                .collect();
            match matches.as_slice() {
                [index] => Ok(*index),
                [] => Err(action_not_found(rule_key)),
                _ => Err(StructuredError::new(
                    "AMBIGUOUS_ACTION_SELECTOR",
                    format!("rule.{rule_key}.actions"),
                    format!("More than one keyed action is named {key}"),
                    "Validate duplicate action keys before editing by key",
                )),
            }
        }
        ActionSelectorInput::ByKind { action, occurrence } => rule
            .actions
            .iter()
            .enumerate()
            .filter(|(_, candidate)| action_kind(candidate) == *action)
            .nth(*occurrence)
            .map(|(index, _)| index)
            .ok_or_else(|| action_not_found(rule_key)),
    }
}

fn action_key(action: &ActionSpec) -> Option<&str> {
    match action {
        ActionSpec::CreateRole { key, .. }
        | ActionSpec::CreateChannel { key, .. }
        | ActionSpec::PostPanel { key, .. }
        | ActionSpec::RegisterInstance { key, .. } => Some(key.as_str()),
        _ => None,
    }
}

fn action_kind(action: &ActionSpec) -> ActionKindInput {
    match action {
        ActionSpec::GrantRole { .. } => ActionKindInput::GrantRole,
        ActionSpec::RespondEphemeral { .. } => ActionKindInput::RespondEphemeral,
        ActionSpec::OpenModal { .. } => ActionKindInput::OpenModal,
        ActionSpec::CreateChannel { .. } => ActionKindInput::CreateChannel,
        ActionSpec::CreateRole { .. } => ActionKindInput::CreateRole,
        ActionSpec::UpsertOverwrite { .. } => ActionKindInput::UpsertOverwrite,
        ActionSpec::PostPanel { .. } => ActionKindInput::PostPanel,
        ActionSpec::DeferEphemeral => ActionKindInput::DeferEphemeral,
        ActionSpec::EditResponse { .. } => ActionKindInput::EditResponse,
        ActionSpec::RegisterInstance { .. } => ActionKindInput::RegisterInstance,
        ActionSpec::TeardownInstance { .. } => ActionKindInput::TeardownInstance,
    }
}

fn action_patch_kind(patch: &ActionPatchInput) -> &'static str {
    match patch {
        ActionPatchInput::CreateRole { .. } => "create_role",
        ActionPatchInput::CreateChannel { .. } => "create_channel",
        ActionPatchInput::GrantRole { .. } => "grant_role",
        ActionPatchInput::RespondEphemeral { .. } => "respond_ephemeral",
        ActionPatchInput::OpenModal { .. } => "open_modal",
        ActionPatchInput::UpsertOverwrite { .. } => "upsert_overwrite",
        ActionPatchInput::PostPanel { .. } => "post_panel",
        ActionPatchInput::EditResponse { .. } => "edit_response",
    }
}

fn overwrite_action(
    channel: ReferenceInput,
    target_kind: OverwriteTargetKindInput,
    role: Option<ReferenceInput>,
    allow: &[String],
    deny: &[String],
) -> Result<ActionSpec, StructuredError> {
    let channel = reference_json(channel);
    let target = match (target_kind, role) {
        (OverwriteTargetKindInput::Everyone, None) => json!("everyone"),
        (OverwriteTargetKindInput::Everyone, Some(_)) => {
            return Err(StructuredError::new(
                "UNEXPECTED_OVERWRITE_ROLE",
                "tool.add_upsert_overwrite_action.arguments.role",
                "role must be omitted when target_kind is everyone",
                "Remove role or change target_kind to role",
            ));
        }
        (OverwriteTargetKindInput::Role, Some(role)) => {
            json!({"role": reference_json(role)})
        }
        (OverwriteTargetKindInput::Role, None) => {
            return Err(StructuredError::new(
                "MISSING_OVERWRITE_ROLE",
                "tool.add_upsert_overwrite_action.arguments.role",
                "role is required when target_kind is role",
                "Set role to {\"kind\":\"created\",\"name\":\"alias\"} or an existing reference",
            ));
        }
    };
    let value = json!({
        "type": "upsert_overwrite",
        "channel": channel,
        "target": target,
        "allow": permission_bits(allow)?.to_string(),
        "deny": permission_bits(deny)?.to_string()
    });
    serde_json::from_value(value).map_err(|error| {
        StructuredError::new(
            "ACTION_NORMALIZATION_FAILED",
            "tool.add_upsert_overwrite_action",
            "The overwrite action could not be normalized",
            error.to_string(),
        )
    })
}

fn reference_json(input: ReferenceInput) -> Value {
    match input {
        ReferenceInput::Created { name } => json!({"created": name}),
        ReferenceInput::Existing { name } => json!(name),
    }
}

fn permission_bits(names: &[String]) -> Result<u64, StructuredError> {
    let mut bits = 0;
    for name in names {
        bits |= match name.as_str() {
            "create_instant_invite" => 1 << 0,
            "kick_members" => 1 << 1,
            "ban_members" => 1 << 2,
            "administrator" => 1 << 3,
            "manage_channels" => 1 << 4,
            "manage_guild" => 1 << 5,
            "add_reactions" => 1 << 6,
            "view_channel" => 1 << 10,
            "send_messages" => 1 << 11,
            "manage_messages" => 1 << 13,
            "embed_links" => 1 << 14,
            "attach_files" => 1 << 15,
            "read_message_history" => 1 << 16,
            "mention_everyone" => 1 << 17,
            "connect" => 1 << 20,
            "speak" => 1 << 21,
            "manage_roles" => 1 << 28,
            "moderate_members" => 1 << 40,
            _ => {
                return Err(StructuredError::new(
                    "UNKNOWN_PERMISSION",
                    "tool.permissions",
                    format!("Permission {name} is not modeled"),
                    "Use a permission name exposed by discord-model",
                ));
            }
        };
    }
    Ok(bits)
}
