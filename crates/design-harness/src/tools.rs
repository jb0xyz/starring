use automation_state::InteractionRule;
use schemars::{schema_for, JsonSchema};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::draft::Draft;
use crate::errors::{
    translate_tool_arguments_error, translate_validation_error, StructuredError, ToolResult,
};
use crate::gates::{simulate_draft, validate_draft};

mod actions;
mod instance;
mod surface;

use actions::{
    add_grant_role_action, add_interaction_action, add_post_panel_action, add_resource_action,
    add_upsert_overwrite_action, remove_action, update_action,
};
use instance::set_register_instance;
use surface::{
    add_button, add_modal, add_panel, begin_rule, remove_button, remove_modal, remove_panel,
    remove_rule, update_button, update_modal, update_panel, update_rule,
};

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

#[derive(Clone, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
enum RoleReferenceInput {
    Created { name: String },
    Existing { name: String },
    InstanceEvent { alias: String },
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
    role: RoleReferenceInput,
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
        role: RoleReferenceInput,
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

fn reference_conversion_error(error: serde_json::Error) -> StructuredError {
    StructuredError::new(
        "INVALID_REFERENCE",
        "tool.reference",
        "The resource reference could not be normalized",
        error.to_string(),
    )
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
