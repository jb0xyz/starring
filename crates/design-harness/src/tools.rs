use std::collections::BTreeMap;

use automation_state::{
    ActionSpec, ActionTarget, ButtonRoute, ButtonSpec, ChannelRef, CreatedRef, InstanceKind,
    InstanceRef, InstanceResourceRefs, InteractionRule, ModalFieldSpec, ModalFieldStyle, ModalSpec,
    PanelSpec, RoleRef, TriggerSpec,
};
use schemars::{schema_for, JsonSchema};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::draft::Draft;
use crate::errors::{
    translate_tool_arguments_error, translate_validation_error, StructuredError, ToolResult,
};
use crate::gates::{simulate_draft, validate_draft};

const PENDING_INSTANCE_REFERENCE: &str = "__pending_instance__";

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct ToolDefinition {
    pub name: String,
    pub description: String,
    pub parameters: Value,
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct AddPanelInput {
    key: String,
    channel: String,
    content: String,
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct UpdatePanelInput {
    key: String,
    channel: Option<String>,
    content: Option<String>,
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct RemovePanelInput {
    key: String,
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct AddButtonInput {
    panel_key: String,
    label: String,
    route: ButtonRouteInput,
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct UpdateButtonInput {
    panel_key: String,
    selector: ButtonRouteInput,
    label: Option<String>,
    route: Option<ButtonRouteInput>,
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct RemoveButtonInput {
    panel_key: String,
    selector: ButtonRouteInput,
}

#[derive(Clone, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum ButtonRouteInput {
    Static { key: String },
    InstanceAction { action: String },
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct AddModalInput {
    key: String,
    title: String,
    fields: Vec<ModalFieldInput>,
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct UpdateModalInput {
    key: String,
    title: Option<String>,
    fields: Option<Vec<ModalFieldInput>>,
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct RemoveModalInput {
    key: String,
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct ModalFieldInput {
    key: String,
    label: String,
    style: ModalFieldStyleInput,
    required: bool,
}

#[derive(Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
enum ModalFieldStyleInput {
    Short,
    Paragraph,
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct BeginRuleInput {
    key: String,
    trigger_kind: TriggerKindInput,
    trigger_ref: String,
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct UpdateRuleInput {
    key: String,
    trigger: TriggerInput,
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct RemoveRuleInput {
    key: String,
}

#[derive(Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum TriggerInput {
    ButtonClick { component: String },
    ModalSubmit { modal: String },
    InstanceAction { action: String },
}

#[derive(Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
enum TriggerKindInput {
    ButtonClick,
    ModalSubmit,
    InstanceAction,
}

#[derive(Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
enum ResourceActionInput {
    CreateRole {
        rule_key: String,
        key: String,
        name: String,
    },
    CreateChannel {
        rule_key: String,
        key: String,
        name: String,
    },
}

#[derive(Clone, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum ReferenceInput {
    Created { name: String },
    Existing { name: String },
}

#[derive(Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
enum OverwriteTargetKindInput {
    Everyone,
    Role,
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct AddGrantRoleActionInput {
    rule_key: String,
    role: ReferenceInput,
    target: ActorTargetInput,
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct AddUpsertOverwriteActionInput {
    rule_key: String,
    channel: ReferenceInput,
    target_kind: OverwriteTargetKindInput,
    role: Option<ReferenceInput>,
    allow: Vec<String>,
    deny: Vec<String>,
}

#[derive(Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
enum ActorTargetInput {
    Actor,
}

#[derive(Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
enum InteractionActionInput {
    OpenModal { rule_key: String, modal: String },
    RespondEphemeral { rule_key: String, content: String },
    DeferEphemeral { rule_key: String },
    EditResponse { rule_key: String, content: String },
    TeardownInstance { rule_key: String },
}

#[derive(Clone, Copy, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum ActionKindInput {
    GrantRole,
    RespondEphemeral,
    OpenModal,
    CreateChannel,
    CreateRole,
    UpsertOverwrite,
    PostPanel,
    DeferEphemeral,
    EditResponse,
    RegisterInstance,
    TeardownInstance,
}

#[derive(Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
enum ActionSelectorInput {
    ByKey {
        key: String,
    },
    ByKind {
        action: ActionKindInput,
        #[serde(default)]
        occurrence: usize,
    },
}

#[derive(Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
enum ActionPatchInput {
    CreateRole {
        name: String,
    },
    CreateChannel {
        name: String,
    },
    GrantRole {
        role: ReferenceInput,
        target: ActorTargetInput,
    },
    RespondEphemeral {
        content: String,
    },
    OpenModal {
        modal: String,
    },
    UpsertOverwrite {
        channel: ReferenceInput,
        target_kind: OverwriteTargetKindInput,
        role: Option<ReferenceInput>,
        allow: Vec<String>,
        deny: Vec<String>,
    },
    PostPanel {
        channel: ReferenceInput,
        content: String,
        buttons: Vec<PostPanelButtonInput>,
    },
    EditResponse {
        content: String,
    },
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct UpdateActionInput {
    rule_key: String,
    selector: ActionSelectorInput,
    patch: ActionPatchInput,
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct RemoveActionInput {
    rule_key: String,
    selector: ActionSelectorInput,
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct AddPostPanelActionInput {
    rule_key: String,
    key: String,
    channel: ReferenceInput,
    content: String,
    buttons: Vec<PostPanelButtonInput>,
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct PostPanelButtonInput {
    label: String,
    route: ButtonRouteInput,
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct SetRegisterInstanceInput {
    rule_key: String,
    instance_key: String,
    kind: String,
    roles: Vec<ManifestEntryInput>,
    channels: Vec<ManifestEntryInput>,
    messages: Vec<ManifestEntryInput>,
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct ManifestEntryInput {
    alias: String,
    created: String,
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct EmptyInput {}

pub fn tool_definitions() -> Vec<ToolDefinition> {
    vec![
        definition::<AddPanelInput>("add_panel", "Add a declared panel"),
        definition::<AddButtonInput>("add_button", "Add a button to a declared panel"),
        definition::<AddModalInput>("add_modal", "Add a modal and its text fields"),
        definition::<BeginRuleInput>(
            "begin_rule",
            "Begin a rule; trigger_ref is the button component, modal key, or instance action selected by trigger_kind",
        ),
        definition::<ResourceActionInput>(
            "add_resource_action",
            "Append a role or channel creation action",
        ),
        definition::<AddGrantRoleActionInput>(
            "add_grant_role_action",
            "Append a role grant action",
        ),
        definition::<AddUpsertOverwriteActionInput>(
            "add_upsert_overwrite_action",
            "Append a permission overwrite action",
        ),
        definition::<InteractionActionInput>(
            "add_interaction_action",
            "Append a modal, defer, or edit response action",
        ),
        definition::<AddPostPanelActionInput>(
            "add_post_panel_action",
            "Append a panel posting action",
        ),
        definition::<SetRegisterInstanceInput>(
            "set_register_instance",
            "Finalize a rule with its complete instance footprint",
        ),
        definition::<UpdatePanelInput>(
            "update_panel",
            "Update a panel channel or content while keeping its stable key",
        ),
        definition::<RemovePanelInput>("remove_panel", "Remove an unreferenced panel by key"),
        definition::<UpdateButtonInput>(
            "update_button",
            "Update a declared button selected by its current route",
        ),
        definition::<RemoveButtonInput>(
            "remove_button",
            "Remove an unreferenced declared button selected by its current route",
        ),
        definition::<UpdateModalInput>(
            "update_modal",
            "Update a modal title or replace its fields while keeping its stable key",
        ),
        definition::<RemoveModalInput>("remove_modal", "Remove an unreferenced modal by key"),
        definition::<UpdateRuleInput>(
            "update_rule",
            "Update a rule trigger while keeping its stable key and actions",
        ),
        definition::<RemoveRuleInput>("remove_rule", "Remove a rule by its stable key"),
        definition::<UpdateActionInput>(
            "update_action",
            "Update an action without changing its identity or order; prefer by_key, while by_kind occurrence is zero-based and can shift after edits",
        ),
        definition::<RemoveActionInput>(
            "remove_action",
            "Remove an action; prefer by_key, while by_kind occurrence is zero-based and can shift after edits",
        ),
        definition::<EmptyInput>("validate_draft", "Validate the current Draft revision"),
        definition::<EmptyInput>("simulate_draft", "Run the current validated Draft revision"),
    ]
}

fn definition<T: JsonSchema>(name: &str, description: &str) -> ToolDefinition {
    ToolDefinition {
        name: name.to_string(),
        description: description.to_string(),
        parameters: serde_json::to_value(schema_for!(T)).unwrap_or_else(|_| json!({})),
    }
}

pub async fn dispatch_tool(draft: &mut Draft, name: &str, arguments: &str) -> ToolResult {
    if name == "validate_draft" {
        if let Err(error) = parse::<EmptyInput>(name, arguments) {
            return ToolResult::failure_from(draft, error);
        }
        return validate_draft(draft);
    }
    if name == "simulate_draft" {
        if let Err(error) = parse::<EmptyInput>(name, arguments) {
            return ToolResult::failure_from(draft, error);
        }
        return simulate_draft(draft).await;
    }

    let result = match name {
        "add_panel" => parse(name, arguments).and_then(|input| add_panel(draft, input)),
        "add_button" => parse(name, arguments).and_then(|input| add_button(draft, input)),
        "add_modal" => parse(name, arguments).and_then(|input| add_modal(draft, input)),
        "begin_rule" => parse(name, arguments).and_then(|input| begin_rule(draft, input)),
        "add_resource_action" => {
            parse(name, arguments).and_then(|input| add_resource_action(draft, input))
        }
        "add_grant_role_action" => {
            parse(name, arguments).and_then(|input| add_grant_role_action(draft, input))
        }
        "add_upsert_overwrite_action" => {
            parse(name, arguments).and_then(|input| add_upsert_overwrite_action(draft, input))
        }
        "add_interaction_action" => {
            parse(name, arguments).and_then(|input| add_interaction_action(draft, input))
        }
        "add_post_panel_action" => {
            parse(name, arguments).and_then(|input| add_post_panel_action(draft, input))
        }
        "set_register_instance" => {
            parse(name, arguments).and_then(|input| set_register_instance(draft, input))
        }
        "update_panel" => parse(name, arguments)
            .and_then(|input| checked_edit(draft, |candidate| update_panel(candidate, input))),
        "remove_panel" => parse(name, arguments)
            .and_then(|input| checked_edit(draft, |candidate| remove_panel(candidate, input))),
        "update_button" => parse(name, arguments)
            .and_then(|input| checked_edit(draft, |candidate| update_button(candidate, input))),
        "remove_button" => parse(name, arguments)
            .and_then(|input| checked_edit(draft, |candidate| remove_button(candidate, input))),
        "update_modal" => parse(name, arguments)
            .and_then(|input| checked_edit(draft, |candidate| update_modal(candidate, input))),
        "remove_modal" => parse(name, arguments)
            .and_then(|input| checked_edit(draft, |candidate| remove_modal(candidate, input))),
        "update_rule" => parse(name, arguments)
            .and_then(|input| checked_edit(draft, |candidate| update_rule(candidate, input))),
        "remove_rule" => parse(name, arguments)
            .and_then(|input| checked_edit(draft, |candidate| remove_rule(candidate, input))),
        "update_action" => parse(name, arguments)
            .and_then(|input| checked_edit(draft, |candidate| update_action(candidate, input))),
        "remove_action" => parse(name, arguments)
            .and_then(|input| checked_edit(draft, |candidate| remove_action(candidate, input))),
        _ => {
            return ToolResult::failure_from(
                draft,
                StructuredError::new(
                    "UNKNOWN_TOOL",
                    "tool",
                    "The requested design tool does not exist",
                    "Use one of the registered design tools",
                ),
            );
        }
    };

    match result {
        Ok(change) => {
            draft.mark_mutated();
            ToolResult::success(draft, change)
        }
        Err(error) => ToolResult::failure_from(draft, error),
    }
}

fn parse<T: for<'de> Deserialize<'de>>(name: &str, arguments: &str) -> Result<T, StructuredError> {
    serde_json::from_str(arguments).map_err(|error| {
        let parameters = tool_definitions()
            .into_iter()
            .find(|definition| definition.name == name)
            .map(|definition| definition.parameters)
            .unwrap_or_else(|| json!({}));
        translate_tool_arguments_error(name, &error, &parameters)
    })
}

fn add_panel(draft: &mut Draft, input: AddPanelInput) -> Result<String, StructuredError> {
    let channel =
        serde_json::from_value(Value::String(input.channel)).map_err(reference_conversion_error)?;
    let key = input.key;
    draft.ruleset.panels.push(PanelSpec {
        key: key.clone(),
        channel,
        content: input.content,
        buttons: Vec::new(),
    });
    Ok(format!("Added panel {key}"))
}

fn checked_edit<F>(draft: &mut Draft, edit: F) -> Result<String, StructuredError>
where
    F: FnOnce(&mut Draft) -> Result<String, StructuredError>,
{
    let mut candidate = draft.clone();
    let change = edit(&mut candidate)?;
    if let Some(error) = draft.newly_dangling_after(&candidate).first() {
        return Err(translate_validation_error(&candidate.ruleset, error));
    }
    let unresolved = draft.newly_unresolved_after(&candidate);
    if !unresolved.is_empty() {
        return Err(StructuredError::new(
            "DANGLING_REFERENCE",
            "draft.references",
            format!(
                "The edit would leave unresolved references: {}",
                unresolved.join(", ")
            ),
            "Update or remove dependent rules and actions before retrying this edit",
        ));
    }
    draft.ruleset = candidate.ruleset;
    Ok(change)
}

fn update_panel(draft: &mut Draft, input: UpdatePanelInput) -> Result<String, StructuredError> {
    if input.channel.is_none() && input.content.is_none() {
        return Err(empty_update("update_panel", "channel or content"));
    }
    let panel = draft
        .ruleset
        .panels
        .iter_mut()
        .find(|panel| panel.key == input.key)
        .ok_or_else(|| missing_panel(&input.key))?;
    if let Some(channel) = input.channel {
        panel.channel =
            serde_json::from_value(Value::String(channel)).map_err(reference_conversion_error)?;
    }
    if let Some(content) = input.content {
        panel.content = content;
    }
    Ok(format!("Updated panel {}", input.key))
}

fn remove_panel(draft: &mut Draft, input: RemovePanelInput) -> Result<String, StructuredError> {
    let index = draft
        .ruleset
        .panels
        .iter()
        .position(|panel| panel.key == input.key)
        .ok_or_else(|| missing_panel(&input.key))?;
    draft.ruleset.panels.remove(index);
    Ok(format!("Removed panel {}", input.key))
}

fn add_button(draft: &mut Draft, input: AddButtonInput) -> Result<String, StructuredError> {
    let panel = draft
        .ruleset
        .panels
        .iter_mut()
        .find(|panel| panel.key == input.panel_key)
        .ok_or_else(|| missing_panel(&input.panel_key))?;
    panel.buttons.push(ButtonSpec {
        label: input.label,
        route: declared_button_route(input.route),
    });
    Ok(format!("Added button to panel {}", input.panel_key))
}

fn update_button(draft: &mut Draft, input: UpdateButtonInput) -> Result<String, StructuredError> {
    if input.label.is_none() && input.route.is_none() {
        return Err(empty_update("update_button", "label or route"));
    }
    let panel = draft
        .ruleset
        .panels
        .iter_mut()
        .find(|panel| panel.key == input.panel_key)
        .ok_or_else(|| missing_panel(&input.panel_key))?;
    let index = button_index(panel, &input.selector, &input.panel_key)?;
    let button = &mut panel.buttons[index];
    if let Some(label) = input.label {
        button.label = label;
    }
    if let Some(route) = input.route {
        button.route = declared_button_route(route);
    }
    Ok(format!("Updated button in panel {}", input.panel_key))
}

fn remove_button(draft: &mut Draft, input: RemoveButtonInput) -> Result<String, StructuredError> {
    let panel = draft
        .ruleset
        .panels
        .iter_mut()
        .find(|panel| panel.key == input.panel_key)
        .ok_or_else(|| missing_panel(&input.panel_key))?;
    let index = button_index(panel, &input.selector, &input.panel_key)?;
    panel.buttons.remove(index);
    Ok(format!("Removed button from panel {}", input.panel_key))
}

fn add_modal(draft: &mut Draft, input: AddModalInput) -> Result<String, StructuredError> {
    let key = input.key;
    draft.ruleset.modals.push(ModalSpec {
        key: key.clone(),
        title: input.title,
        fields: input.fields.into_iter().map(modal_field).collect(),
    });
    Ok(format!("Added modal {key}"))
}

fn update_modal(draft: &mut Draft, input: UpdateModalInput) -> Result<String, StructuredError> {
    if input.title.is_none() && input.fields.is_none() {
        return Err(empty_update("update_modal", "title or fields"));
    }
    let modal = draft
        .ruleset
        .modals
        .iter_mut()
        .find(|modal| modal.key == input.key)
        .ok_or_else(|| missing_modal(&input.key))?;
    if let Some(title) = input.title {
        modal.title = title;
    }
    if let Some(fields) = input.fields {
        modal.fields = fields.into_iter().map(modal_field).collect();
    }
    Ok(format!("Updated modal {}", input.key))
}

fn remove_modal(draft: &mut Draft, input: RemoveModalInput) -> Result<String, StructuredError> {
    let index = draft
        .ruleset
        .modals
        .iter()
        .position(|modal| modal.key == input.key)
        .ok_or_else(|| missing_modal(&input.key))?;
    draft.ruleset.modals.remove(index);
    Ok(format!("Removed modal {}", input.key))
}

fn begin_rule(draft: &mut Draft, input: BeginRuleInput) -> Result<String, StructuredError> {
    let key = input.key;
    draft.ruleset.rules.push(InteractionRule {
        key: key.clone(),
        trigger: begin_trigger(input.trigger_kind, input.trigger_ref),
        actions: Vec::new(),
    });
    Ok(format!("Began rule {key}"))
}

fn update_rule(draft: &mut Draft, input: UpdateRuleInput) -> Result<String, StructuredError> {
    let rule = find_rule_mut(draft, &input.key)?;
    rule.trigger = trigger(input.trigger);
    Ok(format!("Updated rule {}", input.key))
}

fn remove_rule(draft: &mut Draft, input: RemoveRuleInput) -> Result<String, StructuredError> {
    let index = draft
        .ruleset
        .rules
        .iter()
        .position(|rule| rule.key == input.key)
        .ok_or_else(|| missing_rule(&input.key))?;
    draft.ruleset.rules.remove(index);
    Ok(format!("Removed rule {}", input.key))
}

fn add_resource_action(
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

fn add_grant_role_action(
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

fn add_upsert_overwrite_action(
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

fn add_interaction_action(
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

fn add_post_panel_action(
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

fn update_action(draft: &mut Draft, input: UpdateActionInput) -> Result<String, StructuredError> {
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

fn remove_action(draft: &mut Draft, input: RemoveActionInput) -> Result<String, StructuredError> {
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

fn set_register_instance(
    draft: &mut Draft,
    input: SetRegisterInstanceInput,
) -> Result<String, StructuredError> {
    let rule = find_rule_mut(draft, &input.rule_key)?;
    let existing_registration =
        rule.actions
            .iter()
            .enumerate()
            .find_map(|(index, action)| match action {
                ActionSpec::RegisterInstance { key, .. } => Some((index, key.clone())),
                _ => None,
            });

    let ownable = ownable_resources(rule);
    if ownable.is_empty() {
        return Err(StructuredError::new(
            "EMPTY_INSTANCE_RESOURCES",
            format!("rule.{}.actions", input.rule_key),
            "The instance-creation rule has no ownable resources",
            "Create a role, channel, or posted panel before set_register_instance",
        ));
    }
    let roles = manifest_map(&input.rule_key, "role", input.roles)?;
    let channels = manifest_map(&input.rule_key, "channel", input.channels)?;
    let messages = manifest_map(&input.rule_key, "message", input.messages)?;
    ensure_complete_manifest(&input.rule_key, &ownable, &roles, &channels, &messages)?;

    for action in &mut rule.actions {
        if let ActionSpec::PostPanel { buttons, .. } = action {
            for button in buttons {
                if let ButtonRoute::InstanceAction { instance, .. } = &mut button.route {
                    if matches!(
                        instance,
                        InstanceRef::Created(reference)
                            if reference.created == PENDING_INSTANCE_REFERENCE
                                || existing_registration
                                    .as_ref()
                                    .is_some_and(|(_, key)| reference.created == *key)
                    ) {
                        *instance = InstanceRef::Created(CreatedRef {
                            created: input.instance_key.clone(),
                        });
                    }
                }
            }
        }
    }

    let action = ActionSpec::RegisterInstance {
        key: input.instance_key.clone(),
        kind: InstanceKind(input.kind),
        resources: InstanceResourceRefs {
            roles,
            channels,
            messages,
        },
    };
    if let Some((index, _)) = existing_registration {
        rule.actions.remove(index);
    }
    insert_before_edit(rule, action);
    Ok(format!(
        "Finalized instance {} for rule {}",
        input.instance_key, input.rule_key
    ))
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

fn insert_before_edit(rule: &mut InteractionRule, action: ActionSpec) {
    let index = rule
        .actions
        .iter()
        .position(|candidate| matches!(candidate, ActionSpec::EditResponse { .. }))
        .unwrap_or(rule.actions.len());
    rule.actions.insert(index, action);
}

fn find_rule_mut<'a>(
    draft: &'a mut Draft,
    rule_key: &str,
) -> Result<&'a mut InteractionRule, StructuredError> {
    draft
        .ruleset
        .rules
        .iter_mut()
        .find(|rule| rule.key == rule_key)
        .ok_or_else(|| {
            StructuredError::new(
                "RULE_NOT_FOUND",
                format!("rule.{rule_key}"),
                "The target rule does not exist",
                "Call begin_rule before adding actions",
            )
        })
}

fn modal_field(input: ModalFieldInput) -> ModalFieldSpec {
    ModalFieldSpec {
        key: input.key,
        label: input.label,
        style: match input.style {
            ModalFieldStyleInput::Short => ModalFieldStyle::Short,
            ModalFieldStyleInput::Paragraph => ModalFieldStyle::Paragraph,
        },
        required: input.required,
    }
}

fn trigger(input: TriggerInput) -> TriggerSpec {
    match input {
        TriggerInput::ButtonClick { component } => TriggerSpec::ButtonClick { component },
        TriggerInput::ModalSubmit { modal } => TriggerSpec::ModalSubmit { modal },
        TriggerInput::InstanceAction { action } => TriggerSpec::InstanceAction { action },
    }
}

fn begin_trigger(kind: TriggerKindInput, trigger_ref: String) -> TriggerSpec {
    match kind {
        TriggerKindInput::ButtonClick => TriggerSpec::ButtonClick {
            component: trigger_ref,
        },
        TriggerKindInput::ModalSubmit => TriggerSpec::ModalSubmit { modal: trigger_ref },
        TriggerKindInput::InstanceAction => TriggerSpec::InstanceAction {
            action: trigger_ref,
        },
    }
}

fn declared_button_route(input: ButtonRouteInput) -> ButtonRoute {
    match input {
        ButtonRouteInput::Static { key } => ButtonRoute::Static { key },
        ButtonRouteInput::InstanceAction { action } => ButtonRoute::InstanceAction {
            instance: InstanceRef::Event,
            action,
        },
    }
}

fn registered_button_route(input: ButtonRouteInput, registration: Option<&str>) -> ButtonRoute {
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

fn reference_conversion_error(error: serde_json::Error) -> StructuredError {
    StructuredError::new(
        "INVALID_REFERENCE",
        "tool.reference",
        "The resource reference could not be normalized",
        error.to_string(),
    )
}

fn button_index(
    panel: &PanelSpec,
    selector: &ButtonRouteInput,
    panel_key: &str,
) -> Result<usize, StructuredError> {
    let matches: Vec<usize> = panel
        .buttons
        .iter()
        .enumerate()
        .filter_map(|(index, button)| {
            button_route_matches(&button.route, selector).then_some(index)
        })
        .collect();
    match matches.as_slice() {
        [index] => Ok(*index),
        [] => Err(StructuredError::new(
            "BUTTON_NOT_FOUND",
            format!("panel.{panel_key}.buttons"),
            "No button has the selected route",
            "Use the button's current static key or instance action as selector",
        )),
        _ => Err(StructuredError::new(
            "AMBIGUOUS_BUTTON_SELECTOR",
            format!("panel.{panel_key}.buttons"),
            "More than one button has the selected route",
            "Make button routes unique before editing by selector",
        )),
    }
}

fn button_route_matches(route: &ButtonRoute, selector: &ButtonRouteInput) -> bool {
    match (route, selector) {
        (ButtonRoute::Static { key }, ButtonRouteInput::Static { key: selected }) => {
            key == selected
        }
        (
            ButtonRoute::InstanceAction { action, .. },
            ButtonRouteInput::InstanceAction { action: selected },
        ) => action == selected,
        _ => false,
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

fn ownable_resources(rule: &InteractionRule) -> BTreeMap<String, &'static str> {
    rule.actions
        .iter()
        .filter_map(|action| match action {
            ActionSpec::CreateRole { key, .. } => Some((key.clone(), "role")),
            ActionSpec::CreateChannel { key, .. } => Some((key.clone(), "channel")),
            ActionSpec::PostPanel { key, .. } => Some((key.clone(), "message")),
            _ => None,
        })
        .collect()
}

fn manifest_map(
    rule_key: &str,
    kind: &str,
    entries: Vec<ManifestEntryInput>,
) -> Result<BTreeMap<String, CreatedRef>, StructuredError> {
    let mut result = BTreeMap::new();
    for entry in entries {
        if result
            .insert(
                entry.alias.clone(),
                CreatedRef {
                    created: entry.created,
                },
            )
            .is_some()
        {
            return Err(StructuredError::new(
                "DUPLICATE_INSTANCE_RESOURCE_ALIAS",
                format!("rule.{rule_key}.register.{kind}.{}", entry.alias),
                "The instance manifest repeats an alias",
                "Use each manifest alias once",
            ));
        }
    }
    Ok(result)
}

fn ensure_complete_manifest(
    rule_key: &str,
    ownable: &BTreeMap<String, &'static str>,
    roles: &BTreeMap<String, CreatedRef>,
    channels: &BTreeMap<String, CreatedRef>,
    messages: &BTreeMap<String, CreatedRef>,
) -> Result<(), StructuredError> {
    let mut counts = BTreeMap::new();
    let mut declared = BTreeMap::new();
    for reference in roles.values() {
        *counts.entry(reference.created.clone()).or_insert(0usize) += 1;
        declared.insert(reference.created.clone(), "role");
    }
    for reference in channels.values() {
        *counts.entry(reference.created.clone()).or_insert(0usize) += 1;
        declared.insert(reference.created.clone(), "channel");
    }
    for reference in messages.values() {
        *counts.entry(reference.created.clone()).or_insert(0usize) += 1;
        declared.insert(reference.created.clone(), "message");
    }

    for (key, kind) in ownable {
        match declared.get(key) {
            None => {
                return Err(StructuredError::new(
                    "INSTANCE_RESOURCE_MISSING",
                    format!("rule.{rule_key}.actions"),
                    format!("Created {kind} {key} is missing from the instance manifest"),
                    format!("Add {key} once to the {kind} manifest"),
                ));
            }
            Some(declared_kind) if declared_kind != kind => {
                return Err(StructuredError::new(
                    "INSTANCE_RESOURCE_TYPE_MISMATCH",
                    format!("rule.{rule_key}.register"),
                    format!("Created resource {key} is declared as {declared_kind}"),
                    format!("Move {key} to the {kind} manifest"),
                ));
            }
            Some(_) => {}
        }
    }
    for (key, count) in counts {
        if count > 1 {
            return Err(StructuredError::new(
                "INSTANCE_RESOURCE_DECLARED_MULTIPLE_TIMES",
                format!("rule.{rule_key}.register"),
                format!("Created resource {key} is declared more than once"),
                format!("Keep exactly one manifest entry for {key}"),
            ));
        }
        if !ownable.contains_key(&key) {
            return Err(StructuredError::new(
                "UNRESOLVED_CREATED_REFERENCE",
                format!("rule.{rule_key}.register"),
                format!("Manifest resource {key} has not been created"),
                format!("Add the matching create action before registering {key}"),
            ));
        }
    }
    Ok(())
}

fn missing_panel(panel_key: &str) -> StructuredError {
    StructuredError::new(
        "PANEL_NOT_FOUND",
        format!("panel.{panel_key}"),
        "The target panel does not exist",
        "Call add_panel before add_button",
    )
}

fn missing_modal(modal_key: &str) -> StructuredError {
    StructuredError::new(
        "MODAL_NOT_FOUND",
        format!("modal.{modal_key}"),
        "The target modal does not exist",
        "Call add_modal before updating or removing it",
    )
}

fn missing_rule(rule_key: &str) -> StructuredError {
    StructuredError::new(
        "RULE_NOT_FOUND",
        format!("rule.{rule_key}"),
        "The target rule does not exist",
        "Call begin_rule before updating or removing it",
    )
}

fn action_not_found(rule_key: &str) -> StructuredError {
    StructuredError::new(
        "ACTION_NOT_FOUND",
        format!("rule.{rule_key}.actions"),
        "No action matches the selector",
        "Use by_key for keyed actions or a zero-based by_kind occurrence",
    )
}

fn empty_update(tool: &str, fields: &str) -> StructuredError {
    StructuredError::new(
        "EMPTY_UPDATE",
        format!("tool.{tool}.arguments"),
        "The update does not contain any changed fields",
        format!("Set at least one of {fields}"),
    )
}
