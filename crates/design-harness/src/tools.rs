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
use crate::errors::{StructuredError, ToolResult};
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
struct AddButtonInput {
    panel_key: String,
    label: String,
    route: ButtonRouteInput,
}

#[derive(Clone, Deserialize, JsonSchema)]
#[serde(untagged)]
enum ButtonRouteInput {
    Static { r#static: String },
    InstanceAction { instance_action: String },
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
    trigger: TriggerInput,
}

#[derive(Deserialize, JsonSchema)]
#[serde(untagged)]
enum TriggerInput {
    ButtonClick { button_click: String },
    ModalSubmit { modal_submit: String },
    InstanceAction { instance_action: String },
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
#[serde(untagged)]
enum ReferenceInput {
    Created { created: String },
    Existing { existing: String },
}

#[derive(Deserialize, JsonSchema)]
#[serde(untagged)]
enum OverwriteTargetInput {
    Everyone(String),
    Role { role: ReferenceInput },
}

#[derive(Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
enum PermissionActionInput {
    UpsertOverwrite {
        rule_key: String,
        channel: ReferenceInput,
        target: OverwriteTargetInput,
        allow: Vec<String>,
        deny: Vec<String>,
    },
    GrantRole {
        rule_key: String,
        role: ReferenceInput,
        target: ActorTargetInput,
    },
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
    DeferEphemeral { rule_key: String },
    EditResponse { rule_key: String, content: String },
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
        definition::<BeginRuleInput>("begin_rule", "Begin a rule with one trigger"),
        definition::<ResourceActionInput>(
            "add_resource_action",
            "Append a role or channel creation action",
        ),
        definition::<PermissionActionInput>(
            "add_permission_action",
            "Append an overwrite or role grant action",
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
        if let Err(error) = parse::<EmptyInput>(arguments) {
            return ToolResult::failure_from(
                draft,
                StructuredError {
                    location: name.to_string(),
                    ..error
                },
            );
        }
        return validate_draft(draft);
    }
    if name == "simulate_draft" {
        if let Err(error) = parse::<EmptyInput>(arguments) {
            return ToolResult::failure_from(
                draft,
                StructuredError {
                    location: name.to_string(),
                    ..error
                },
            );
        }
        return simulate_draft(draft).await;
    }

    let result = match name {
        "add_panel" => parse(arguments).and_then(|input| add_panel(draft, input)),
        "add_button" => parse(arguments).and_then(|input| add_button(draft, input)),
        "add_modal" => parse(arguments).and_then(|input| add_modal(draft, input)),
        "begin_rule" => parse(arguments).and_then(|input| begin_rule(draft, input)),
        "add_resource_action" => {
            parse(arguments).and_then(|input| add_resource_action(draft, input))
        }
        "add_permission_action" => {
            parse(arguments).and_then(|input| add_permission_action(draft, input))
        }
        "add_interaction_action" => {
            parse(arguments).and_then(|input| add_interaction_action(draft, input))
        }
        "add_post_panel_action" => {
            parse(arguments).and_then(|input| add_post_panel_action(draft, input))
        }
        "set_register_instance" => {
            parse(arguments).and_then(|input| set_register_instance(draft, input))
        }
        _ => {
            return ToolResult::failure_from(
                draft,
                StructuredError::new(
                    "UNKNOWN_TOOL",
                    "tool",
                    "The requested design tool does not exist",
                    "Use one of the eleven registered design tools",
                ),
            );
        }
    };

    match result {
        Ok(change) => {
            draft.mark_mutated();
            ToolResult::success(draft, change)
        }
        Err(error) => {
            let error = if error.code == "INVALID_TOOL_ARGUMENTS" {
                StructuredError {
                    location: name.to_string(),
                    ..error
                }
            } else {
                error
            };
            ToolResult::failure_from(draft, error)
        }
    }
}

fn parse<T: for<'de> Deserialize<'de>>(arguments: &str) -> Result<T, StructuredError> {
    serde_json::from_str(arguments).map_err(|error| {
        StructuredError::new(
            "INVALID_TOOL_ARGUMENTS",
            "tool",
            "The tool arguments do not match the required shape",
            error.to_string(),
        )
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

fn add_modal(draft: &mut Draft, input: AddModalInput) -> Result<String, StructuredError> {
    let key = input.key;
    draft.ruleset.modals.push(ModalSpec {
        key: key.clone(),
        title: input.title,
        fields: input.fields.into_iter().map(modal_field).collect(),
    });
    Ok(format!("Added modal {key}"))
}

fn begin_rule(draft: &mut Draft, input: BeginRuleInput) -> Result<String, StructuredError> {
    let key = input.key;
    draft.ruleset.rules.push(InteractionRule {
        key: key.clone(),
        trigger: trigger(input.trigger),
        actions: Vec::new(),
    });
    Ok(format!("Began rule {key}"))
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

fn add_permission_action(
    draft: &mut Draft,
    input: PermissionActionInput,
) -> Result<String, StructuredError> {
    let (rule_key, action, change) = match input {
        PermissionActionInput::UpsertOverwrite {
            rule_key,
            channel,
            target,
            allow,
            deny,
        } => {
            let action = overwrite_action(channel, target, &allow, &deny)?;
            let change = format!("Added UpsertOverwrite to rule {rule_key}");
            (rule_key, action, change)
        }
        PermissionActionInput::GrantRole {
            rule_key,
            role,
            target,
        } => {
            let target = match target {
                ActorTargetInput::Actor => ActionTarget::Actor,
            };
            let action = ActionSpec::GrantRole {
                role: role_reference(role)?,
                target,
            };
            let change = format!("Added GrantRole to rule {rule_key}");
            (rule_key, action, change)
        }
    };
    append_action(draft, &rule_key, action)?;
    Ok(change)
}

fn add_interaction_action(
    draft: &mut Draft,
    input: InteractionActionInput,
) -> Result<String, StructuredError> {
    let (rule_key, action, label) = match input {
        InteractionActionInput::OpenModal { rule_key, modal } => {
            (rule_key, ActionSpec::OpenModal { modal }, "OpenModal")
        }
        InteractionActionInput::DeferEphemeral { rule_key } => {
            (rule_key, ActionSpec::DeferEphemeral, "DeferEphemeral")
        }
        InteractionActionInput::EditResponse { rule_key, content } => (
            rule_key,
            ActionSpec::EditResponse { content },
            "EditResponse",
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
        TriggerInput::ButtonClick { button_click } => TriggerSpec::ButtonClick {
            component: button_click,
        },
        TriggerInput::ModalSubmit { modal_submit } => TriggerSpec::ModalSubmit {
            modal: modal_submit,
        },
        TriggerInput::InstanceAction { instance_action } => TriggerSpec::InstanceAction {
            action: instance_action,
        },
    }
}

fn declared_button_route(input: ButtonRouteInput) -> ButtonRoute {
    match input {
        ButtonRouteInput::Static { r#static } => ButtonRoute::Static { key: r#static },
        ButtonRouteInput::InstanceAction { instance_action } => ButtonRoute::InstanceAction {
            instance: InstanceRef::Event,
            action: instance_action,
        },
    }
}

fn pending_button_route(input: ButtonRouteInput) -> ButtonRoute {
    match input {
        ButtonRouteInput::Static { r#static } => ButtonRoute::Static { key: r#static },
        ButtonRouteInput::InstanceAction { instance_action } => ButtonRoute::InstanceAction {
            instance: InstanceRef::Created(CreatedRef {
                created: PENDING_INSTANCE_REFERENCE.to_string(),
            }),
            action: instance_action,
        },
    }
}

fn role_reference(input: ReferenceInput) -> Result<RoleRef, StructuredError> {
    match input {
        ReferenceInput::Created { created } => Ok(RoleRef::Created(CreatedRef { created })),
        ReferenceInput::Existing { existing } => serde_json::from_value(Value::String(existing))
            .map(RoleRef::Existing)
            .map_err(reference_conversion_error),
    }
}

fn channel_reference(input: ReferenceInput) -> Result<ChannelRef, StructuredError> {
    match input {
        ReferenceInput::Created { created } => Ok(ChannelRef::Created(CreatedRef { created })),
        ReferenceInput::Existing { existing } => serde_json::from_value(Value::String(existing))
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

fn overwrite_action(
    channel: ReferenceInput,
    target: OverwriteTargetInput,
    allow: &[String],
    deny: &[String],
) -> Result<ActionSpec, StructuredError> {
    let channel = reference_json(channel);
    let target = match target {
        OverwriteTargetInput::Everyone(value) if value == "everyone" => json!("everyone"),
        OverwriteTargetInput::Everyone(_) => {
            return Err(StructuredError::new(
                "INVALID_OVERWRITE_TARGET",
                "tool.target",
                "The string overwrite target must be everyone",
                "Use everyone or {\"role\":{\"created\":\"alias\"}}",
            ));
        }
        OverwriteTargetInput::Role { role } => json!({"role": reference_json(role)}),
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
            "tool.add_permission_action",
            "The overwrite action could not be normalized",
            error.to_string(),
        )
    })
}

fn reference_json(input: ReferenceInput) -> Value {
    match input {
        ReferenceInput::Created { created } => json!({"created": created}),
        ReferenceInput::Existing { existing } => json!(existing),
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
