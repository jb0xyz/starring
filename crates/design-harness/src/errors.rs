use std::collections::BTreeSet;

use automation_core::{AdapterError, AdapterErrorKind, ValidationError};
use automation_state::{ActionSpec, ChannelRef, InstanceRef, InteractionRuleSet, RoleRef};
use serde::{Deserialize, Serialize};
use serde_json::{error::Category, Value};

use crate::draft::{Draft, DraftSummary};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct StructuredError {
    pub code: String,
    pub location: String,
    pub message: String,
    pub hint: String,
}

impl StructuredError {
    pub fn new(
        code: impl Into<String>,
        location: impl Into<String>,
        message: impl Into<String>,
        hint: impl Into<String>,
    ) -> Self {
        Self {
            code: code.into(),
            location: location.into(),
            message: message.into(),
            hint: hint.into(),
        }
    }
}

pub(crate) fn translate_tool_arguments_error(
    tool_name: &str,
    error: &serde_json::Error,
    parameters: &Value,
) -> StructuredError {
    let detail = error.to_string();
    let base_location = format!("tool.{tool_name}.arguments");
    let valid_kinds = kind_values(parameters);
    let (code, location, message) = if let Some(field) = quoted_value(&detail, "missing field `") {
        (
            "MISSING_REQUIRED_FIELD",
            format!("{base_location}.{field}"),
            format!("missing required field {field}"),
        )
    } else if let Some(value) = quoted_value(&detail, "unknown variant `") {
        if is_kind_error(&detail, &valid_kinds) {
            (
                "INVALID_KIND",
                base_location,
                format!("kind {value} is not valid"),
            )
        } else {
            (
                "INVALID_TOOL_ARGUMENTS",
                base_location,
                format!("value {value} is not allowed"),
            )
        }
    } else if let Some(field) = quoted_value(&detail, "unknown field `") {
        (
            "UNKNOWN_FIELD",
            format!("{base_location}.{field}"),
            format!("field {field} is not recognized"),
        )
    } else if detail.contains("invalid type:") {
        (
            "INVALID_FIELD_TYPE",
            base_location,
            "a field value has a type that does not match the schema".to_string(),
        )
    } else if matches!(
        error.classify(),
        Category::Io | Category::Syntax | Category::Eof
    ) {
        (
            "INVALID_TOOL_ARGUMENTS",
            base_location,
            "tool arguments are not valid JSON".to_string(),
        )
    } else {
        (
            "INVALID_TOOL_ARGUMENTS",
            base_location,
            "tool arguments do not match the expected schema".to_string(),
        )
    };
    let mut hint = format!(
        "{tool_name} expects {}",
        format_schema(parameters, parameters, 0)
    );
    if code == "INVALID_KIND" && !valid_kinds.is_empty() {
        hint.push_str("; kind must be one of: ");
        hint.push_str(&valid_kinds.into_iter().collect::<Vec<_>>().join(", "));
    }
    StructuredError::new(code, location, message, hint)
}

fn quoted_value(message: &str, prefix: &str) -> Option<String> {
    let start = message.find(prefix)? + prefix.len();
    let remaining = &message[start..];
    let end = remaining.find('`')?;
    Some(remaining[..end].to_string())
}

fn is_kind_error(message: &str, valid_kinds: &BTreeSet<String>) -> bool {
    valid_kinds
        .iter()
        .any(|kind| message.contains(&format!("`{kind}`")))
}

fn format_schema(schema: &Value, root: &Value, depth: usize) -> String {
    if depth > 12 {
        return "value".to_string();
    }
    if let Some(reference) = schema.get("$ref").and_then(Value::as_str) {
        return resolve_reference(root, reference)
            .map(|resolved| format_schema(resolved, root, depth + 1))
            .unwrap_or_else(|| "value".to_string());
    }
    if let Some(value) = schema.get("const") {
        return scalar_name(value);
    }
    if let Some(values) = schema.get("enum").and_then(Value::as_array) {
        return values.iter().map(scalar_name).collect::<Vec<_>>().join("|");
    }
    if let Some(variants) = schema
        .get("oneOf")
        .or_else(|| schema.get("anyOf"))
        .and_then(Value::as_array)
    {
        return variants
            .iter()
            .map(|variant| format_schema(variant, root, depth + 1))
            .collect::<Vec<_>>()
            .join(" | ");
    }
    match schema.get("type").and_then(Value::as_str) {
        Some("object") => format_object(schema, root, depth + 1),
        Some("array") => format_array(schema, root, depth + 1),
        Some("string") => "string".to_string(),
        Some("boolean") => "boolean".to_string(),
        Some("integer") => "integer".to_string(),
        Some("number") => "number".to_string(),
        Some("null") => "null".to_string(),
        _ if schema.get("properties").is_some() => format_object(schema, root, depth + 1),
        _ => "value".to_string(),
    }
}

fn format_object(schema: &Value, root: &Value, depth: usize) -> String {
    let Some(properties) = schema.get("properties").and_then(Value::as_object) else {
        return "{}".to_string();
    };
    let required = schema
        .get("required")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .collect::<BTreeSet<_>>();
    let fields = properties
        .iter()
        .map(|(name, property)| {
            format_property(
                name,
                property,
                required.contains(name.as_str()),
                root,
                depth + 1,
            )
        })
        .collect::<Vec<_>>()
        .join(", ");
    format!("{{ {fields} }}")
}

fn format_property(
    name: &str,
    schema: &Value,
    required: bool,
    root: &Value,
    depth: usize,
) -> String {
    let marker = if required { "(required)" } else { "" };
    if schema_type(schema, root) == Some("array") {
        let resolved = resolve_schema(schema, root);
        let item = resolved
            .get("items")
            .map(|items| format_schema(items, root, depth + 1))
            .unwrap_or_else(|| "value".to_string());
        format!("{name}: array{marker} of {item}")
    } else {
        format!("{name}: {}{marker}", format_schema(schema, root, depth + 1))
    }
}

fn format_array(schema: &Value, root: &Value, depth: usize) -> String {
    let item = schema
        .get("items")
        .map(|items| format_schema(items, root, depth + 1))
        .unwrap_or_else(|| "value".to_string());
    format!("array of {item}")
}

fn schema_type<'a>(schema: &'a Value, root: &'a Value) -> Option<&'a str> {
    resolve_schema(schema, root)
        .get("type")
        .and_then(Value::as_str)
}

fn resolve_schema<'a>(schema: &'a Value, root: &'a Value) -> &'a Value {
    schema
        .get("$ref")
        .and_then(Value::as_str)
        .and_then(|reference| resolve_reference(root, reference))
        .unwrap_or(schema)
}

fn resolve_reference<'a>(root: &'a Value, reference: &str) -> Option<&'a Value> {
    let name = reference.strip_prefix("#/$defs/")?;
    root.get("$defs")?.get(name)
}

fn scalar_name(value: &Value) -> String {
    match value {
        Value::String(value) => value.clone(),
        _ => value.to_string(),
    }
}

fn kind_values(schema: &Value) -> BTreeSet<String> {
    let mut values = BTreeSet::new();
    collect_kind_values(schema, &mut values);
    values
}

fn collect_kind_values(schema: &Value, values: &mut BTreeSet<String>) {
    match schema {
        Value::Array(items) => {
            for item in items {
                collect_kind_values(item, values);
            }
        }
        Value::Object(fields) => {
            if let Some(kind) = fields
                .get("properties")
                .and_then(Value::as_object)
                .and_then(|properties| properties.get("kind"))
                .and_then(|kind| kind.get("const"))
                .and_then(Value::as_str)
            {
                values.insert(kind.to_string());
            }
            for value in fields.values() {
                collect_kind_values(value, values);
            }
        }
        _ => {}
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct ToolSuccess {
    pub ok: bool,
    pub revision: u64,
    pub change: String,
    pub draft: DraftSummary,
    pub validation: String,
    pub simulation: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct ToolFailure {
    pub ok: bool,
    pub code: String,
    pub location: String,
    pub message: String,
    pub hint: String,
    pub revision: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(untagged)]
pub enum ToolResult {
    Success(ToolSuccess),
    Failure(ToolFailure),
}

impl ToolResult {
    pub fn success(draft: &Draft, change: impl Into<String>) -> Self {
        Self::Success(ToolSuccess {
            ok: true,
            revision: draft.draft_revision,
            change: change.into(),
            draft: draft.summary(),
            validation: draft.validation_status(),
            simulation: draft.simulation_status(),
        })
    }

    pub fn failure_from(draft: &Draft, error: StructuredError) -> Self {
        Self::Failure(ToolFailure {
            ok: false,
            code: error.code,
            location: error.location,
            message: error.message,
            hint: error.hint,
            revision: draft.draft_revision,
        })
    }

    pub fn is_ok(&self) -> bool {
        matches!(self, Self::Success(_))
    }

    pub fn success_value(&self) -> Option<&ToolSuccess> {
        match self {
            Self::Success(value) => Some(value),
            Self::Failure(_) => None,
        }
    }

    pub fn failure(&self) -> Option<&ToolFailure> {
        match self {
            Self::Success(_) => None,
            Self::Failure(value) => Some(value),
        }
    }

    pub fn as_json(&self) -> String {
        serde_json::to_string(self).unwrap_or_else(|_| {
            r#"{"ok":false,"code":"RESULT_SERIALIZATION_FAILED","location":"tool","message":"Tool result serialization failed","hint":"Stop the burst and report the harness error","revision":0}"#.to_string()
        })
    }
}

pub fn translate_run_error(error: &AdapterError) -> StructuredError {
    let (code, message, hint) = match error.kind {
        AdapterErrorKind::Forbidden => (
            "SIMULATION_FORBIDDEN",
            "The golden trace was rejected by the mutation adapter",
            "Check the requested permission and role operations",
        ),
        AdapterErrorKind::NotFound => (
            "SIMULATION_RESOURCE_NOT_FOUND",
            "The golden trace referenced a missing resource",
            "Check created and existing resource references",
        ),
        AdapterErrorKind::RateLimited => (
            "SIMULATION_RATE_LIMITED",
            "The golden trace mutation was rate limited",
            "Retry the simulation after the adapter is available",
        ),
        AdapterErrorKind::Network => (
            "SIMULATION_NETWORK_ERROR",
            "The golden trace adapter was unavailable",
            "Retry the simulation after the adapter is available",
        ),
        AdapterErrorKind::Unsupported => (
            "SIMULATION_UNSUPPORTED_ACTION",
            "The golden trace contains an unsupported action",
            "Use only actions supported by the design harness",
        ),
        AdapterErrorKind::BadRequest => (
            "SIMULATION_EXECUTION_FAILED",
            "The golden trace could not execute the draft",
            "Check action ordering, created references, and the instance manifest",
        ),
        AdapterErrorKind::InvalidEventRoute => (
            "SIMULATION_INVALID_EVENT_ROUTE",
            "The golden trace used the wrong event execution path",
            "Use a button or modal rule for draft simulation",
        ),
        AdapterErrorKind::Unknown => (
            "SIMULATION_UNKNOWN_ERROR",
            "The golden trace failed for an unknown adapter reason",
            "Review the latest Draft summary and retry after correcting the rule",
        ),
    };
    StructuredError::new(code, "simulation", message, hint)
}

pub fn translate_validation_error(
    ruleset: &InteractionRuleSet,
    error: &ValidationError,
) -> StructuredError {
    match error {
        ValidationError::InstanceResourceMissingFromManifest { key, kind } => StructuredError::new(
            "INSTANCE_RESOURCE_MISSING",
            action_location(ruleset, "", key),
            format!(
                "Created {} {key} is missing from the instance manifest",
                created_kind_name(*kind)
            ),
            format!("Add {key} exactly once to set_register_instance"),
        ),
        ValidationError::InstanceResourceProducedAfterRegister { key, kind } => {
            StructuredError::new(
                "REGISTER_BEFORE_RESOURCE_CREATED",
                action_location(ruleset, "", key),
                format!(
                    "Created {} {key} appears after RegisterInstance",
                    created_kind_name(*kind)
                ),
                "Move set_register_instance after all resource and panel actions",
            )
        }
        ValidationError::UnknownCreatedRoleRef { rule, key }
        | ValidationError::UnknownCreatedChannelRef { rule, key }
        | ValidationError::UnknownCreatedMessageRef { rule, key }
        | ValidationError::UnknownCreatedInstanceRef { rule, key } => StructuredError::new(
            "UNRESOLVED_CREATED_REFERENCE",
            action_location(ruleset, rule, key),
            format!("Created resource {key} does not exist yet"),
            format!("Add the matching create action before this reference to {key}"),
        ),
        ValidationError::CreatedRoleRefTypeMismatch { rule, key }
        | ValidationError::CreatedChannelRefTypeMismatch { rule, key }
        | ValidationError::CreatedMessageRefTypeMismatch { rule, key }
        | ValidationError::CreatedInstanceRefTypeMismatch { rule, key } => StructuredError::new(
            "CREATED_REFERENCE_TYPE_MISMATCH",
            action_location(ruleset, rule, key),
            format!("Created resource {key} has the wrong type"),
            "Use a created reference whose resource kind matches this action",
        ),
        ValidationError::DuplicatePanelKey(key) => duplicate_error("panel", key),
        ValidationError::DuplicateButtonKey(key) => duplicate_error("button", key),
        ValidationError::DuplicateRuleKey(key) => duplicate_error("rule", key),
        ValidationError::DuplicateModalKey(key) => duplicate_error("modal", key),
        ValidationError::DuplicateActionKey { rule, key } => StructuredError::new(
            "DUPLICATE_ACTION_KEY",
            action_location(ruleset, rule, key),
            format!("Action key {key} is repeated"),
            "Use a unique key for every created resource in the rule",
        ),
        ValidationError::DuplicateModalFieldKey { modal, field } => StructuredError::new(
            "DUPLICATE_MODAL_FIELD",
            format!("modal.{modal}.field.{field}"),
            format!("Modal field {field} is repeated"),
            "Use a unique key for every modal field",
        ),
        ValidationError::InvalidModalFieldMinLength {
            modal,
            field,
            min_length,
        } => StructuredError::new(
            "INVALID_MODAL_FIELD_MIN_LENGTH",
            format!("modal.{modal}.field.{field}.min_length"),
            format!("Modal field {field} has minimum length {min_length}"),
            "Use a minimum length from 0 through 4000",
        ),
        ValidationError::InvalidModalFieldMaxLength {
            modal,
            field,
            max_length,
        } => StructuredError::new(
            "INVALID_MODAL_FIELD_MAX_LENGTH",
            format!("modal.{modal}.field.{field}.max_length"),
            format!("Modal field {field} has maximum length {max_length}"),
            "Use a maximum length from 1 through 4000",
        ),
        ValidationError::InvalidModalFieldLengthRange {
            modal,
            field,
            min_length,
            max_length,
        } => StructuredError::new(
            "INVALID_MODAL_FIELD_LENGTH_RANGE",
            format!("modal.{modal}.field.{field}"),
            format!(
                "Modal field {field} minimum length {min_length} exceeds maximum length {max_length}"
            ),
            "Use a minimum length less than or equal to the maximum length",
        ),
        ValidationError::UnknownButtonRef { rule, component } => StructuredError::new(
            "UNKNOWN_BUTTON_REFERENCE",
            format!("rule.{rule}.trigger"),
            format!("Button {component} is not declared"),
            format!("Add button {component} before beginning this rule"),
        ),
        ValidationError::UnknownModalRef { rule, modal } => StructuredError::new(
            "UNKNOWN_MODAL_REFERENCE",
            format!("rule.{rule}"),
            format!("Modal {modal} is not declared"),
            format!("Call add_modal for {modal} before referencing it"),
        ),
        ValidationError::UnknownRoleRef { rule, role } => StructuredError::new(
            "UNRESOLVED_BINDING",
            format!("rule.{rule}"),
            format!("Role binding {} does not exist", role.0),
            "Use a known fixed binding or a created role reference",
        ),
        ValidationError::UnknownChannelRef { rule, channel } => StructuredError::new(
            "UNRESOLVED_BINDING",
            format!("rule.{rule}"),
            format!("Channel binding {} does not exist", channel.0),
            "Use study_hub or a created channel reference",
        ),
        ValidationError::ConflictingTrigger { component } => StructuredError::new(
            "CONFLICTING_TRIGGER",
            format!("button.{component}"),
            format!("Button {component} triggers more than one rule"),
            "Keep one rule for each static button trigger",
        ),
        ValidationError::ConflictingModalTrigger { modal } => StructuredError::new(
            "CONFLICTING_TRIGGER",
            format!("modal.{modal}"),
            format!("Modal {modal} triggers more than one rule"),
            "Keep one rule for each modal submit trigger",
        ),
        ValidationError::EmptyResponseContent { rule } => rule_error(
            "EMPTY_RESPONSE_CONTENT",
            rule,
            "Response content is empty",
            "Provide non-empty response content",
        ),
        ValidationError::BadTemplate { rule } => rule_error(
            "BAD_TEMPLATE",
            rule,
            "A response or name template is invalid",
            "Use only ${input.field_key} placeholders",
        ),
        ValidationError::InputTemplateInButtonRule { rule, input } => StructuredError::new(
            "INPUT_TEMPLATE_OUTSIDE_MODAL",
            format!("rule.{rule}"),
            format!("Input {input} is unavailable for this trigger"),
            "Use input templates only in a modal submit rule",
        ),
        ValidationError::UnknownTemplateInput { rule, modal, input } => StructuredError::new(
            "UNKNOWN_TEMPLATE_INPUT",
            format!("rule.{rule}"),
            format!("Input {input} is not a field of modal {modal}"),
            format!("Add field {input} to modal {modal} or correct the placeholder"),
        ),
        ValidationError::InstanceResourceDeclaredMultipleTimes { key } => StructuredError::new(
            "INSTANCE_RESOURCE_DECLARED_MULTIPLE_TIMES",
            "register_instance",
            format!("Created resource {key} appears more than once in the manifest"),
            format!("Keep exactly one manifest entry for {key}"),
        ),
        ValidationError::MultipleRegisterInstance { rule } => rule_error(
            "MULTIPLE_REGISTER_INSTANCE",
            rule,
            "The rule contains more than one RegisterInstance action",
            "Use set_register_instance once per instance-creation rule",
        ),
        ValidationError::EmptyInstanceResources { rule } => rule_error(
            "EMPTY_INSTANCE_RESOURCES",
            rule,
            "The instance manifest is empty",
            "List every created role, channel, and panel message",
        ),
        ValidationError::InvalidResourceAlias { rule, alias } => StructuredError::new(
            "INVALID_RESOURCE_ALIAS",
            format!("rule.{rule}.register.{alias}"),
            format!("Resource alias {alias} is invalid"),
            "Use 1 to 32 ASCII letters, digits, underscores, or hyphens",
        ),
        ValidationError::OverlappingOverwrite { rule } => rule_error(
            "OVERLAPPING_OVERWRITE",
            rule,
            "An overwrite allows and denies the same permission",
            "Remove each overlapping permission from allow or deny",
        ),
        ValidationError::EmptyOverwrite { rule } => rule_error(
            "EMPTY_OVERWRITE",
            rule,
            "An overwrite has no allow or deny permissions",
            "Add at least one allow or deny permission",
        ),
        ValidationError::EmptyButtonLabel { rule, button } => StructuredError::new(
            "EMPTY_BUTTON_LABEL",
            format!("rule.{rule}.button.{button}"),
            "A posted panel button label is empty",
            "Provide a visible button label",
        ),
        ValidationError::TooManyPanelButtons { rule, count } => StructuredError::new(
            "TOO_MANY_PANEL_BUTTONS",
            format!("rule.{rule}"),
            format!("A posted panel has {count} buttons"),
            "Use no more than five buttons per panel",
        ),
        ValidationError::DeferNotFirst { rule } => rule_error(
            "DEFER_NOT_FIRST",
            rule,
            "DeferEphemeral is not the first action",
            "Add defer_ephemeral before every other action",
        ),
        ValidationError::ConflictingInitialResponse { rule } => rule_error(
            "CONFLICTING_INITIAL_RESPONSE",
            rule,
            "The rule mixes deferred and immediate responses",
            "Use either OpenModal or a DeferEphemeral/EditResponse lifecycle",
        ),
        ValidationError::EditResponseWithoutDefer { rule } => rule_error(
            "EDIT_WITHOUT_DEFER",
            rule,
            "EditResponse has no preceding DeferEphemeral",
            "Add defer_ephemeral as the first action",
        ),
        ValidationError::DeferredMissingEditResponse { rule } => rule_error(
            "DEFER_MISSING_EDIT",
            rule,
            "A deferred rule has no terminal EditResponse",
            "Add edit_response as the final action",
        ),
        ValidationError::MultipleEditResponse { rule } => rule_error(
            "MULTIPLE_EDIT_RESPONSE",
            rule,
            "The rule has more than one EditResponse",
            "Keep one terminal edit_response action",
        ),
        ValidationError::EditResponseNotLast { rule } => rule_error(
            "EDIT_RESPONSE_NOT_LAST",
            rule,
            "EditResponse is not the final action",
            "Move edit_response to the end of the rule",
        ),
        ValidationError::InstanceRoleOutsideInstanceRule { rule } => rule_error(
            "INSTANCE_ROLE_OUTSIDE_INSTANCE_RULE",
            rule,
            "An instance role is used outside an instance-action rule",
            "Use a created or existing role in this rule",
        ),
        ValidationError::InstanceRoleMustUseEvent { rule } => rule_error(
            "INSTANCE_ROLE_MUST_USE_EVENT",
            rule,
            "An instance role does not reference the current event instance",
            "Use the event instance for instance-action role aliases",
        ),
        ValidationError::InstanceActionRuleMustDefer { rule } => rule_error(
            "INSTANCE_ACTION_MUST_DEFER",
            rule,
            "An instance-action rule does not defer first",
            "Wrap the rule in DeferEphemeral and terminal EditResponse actions",
        ),
        ValidationError::TeardownInstanceOutsideInstanceRule { rule } => rule_error(
            "TEARDOWN_OUTSIDE_INSTANCE_RULE",
            rule,
            "TeardownInstance is used outside an instance-action rule",
            "Use TeardownInstance only with an instance_action trigger",
        ),
        ValidationError::TeardownRegisterConflict { rule } => rule_error(
            "TEARDOWN_REGISTER_CONFLICT",
            rule,
            "The rule both registers and tears down an instance",
            "Separate instance creation and teardown into different rules",
        ),
    }
}

fn duplicate_error(kind: &str, key: &str) -> StructuredError {
    StructuredError::new(
        "DUPLICATE_KEY",
        format!("{kind}.{key}"),
        format!("{kind} key {key} is repeated"),
        format!("Use a unique key for every {kind}"),
    )
}

fn rule_error(code: &str, rule: &str, message: &str, hint: &str) -> StructuredError {
    StructuredError::new(code, format!("rule.{rule}"), message, hint)
}

fn created_kind_name(kind: automation_core::validate::CreatedKind) -> &'static str {
    match kind {
        automation_core::validate::CreatedKind::Role => "role",
        automation_core::validate::CreatedKind::Channel => "channel",
        automation_core::validate::CreatedKind::Message => "message",
        automation_core::validate::CreatedKind::Instance => "instance",
    }
}

fn action_location(ruleset: &InteractionRuleSet, rule_key: &str, key: &str) -> String {
    let rule = if rule_key.is_empty() {
        ruleset.rules.iter().find(|rule| {
            rule.actions
                .iter()
                .any(|action| action_references_key(action, key))
        })
    } else {
        ruleset.rules.iter().find(|rule| rule.key == rule_key)
    };
    let Some(rule) = rule else {
        return "draft.rules".to_string();
    };
    let index = rule
        .actions
        .iter()
        .position(|action| action_references_key(action, key))
        .unwrap_or(0);
    format!("rule.{}.actions[{index}]", rule.key)
}

fn action_references_key(action: &ActionSpec, key: &str) -> bool {
    match action {
        ActionSpec::CreateRole {
            key: action_key, ..
        }
        | ActionSpec::CreateChannel {
            key: action_key, ..
        }
        | ActionSpec::PostPanel {
            key: action_key, ..
        }
        | ActionSpec::RegisterInstance {
            key: action_key, ..
        } => action_key == key,
        ActionSpec::GrantRole { role, .. } => role_ref_key(role) == Some(key),
        ActionSpec::UpsertOverwrite {
            channel, target, ..
        } => {
            channel_ref_key(channel) == Some(key)
                || matches!(target, automation_state::OverwriteTargetSpec::Role(role) if role_ref_key(role) == Some(key))
        }
        ActionSpec::TeardownInstance { instance } => instance_ref_key(instance) == Some(key),
        _ => false,
    }
}

fn role_ref_key(reference: &RoleRef) -> Option<&str> {
    match reference {
        RoleRef::Created(reference) => Some(reference.created.as_str()),
        RoleRef::Instance { instance, .. } => instance_ref_key(instance),
        RoleRef::Existing(_) => None,
    }
}

fn channel_ref_key(reference: &ChannelRef) -> Option<&str> {
    match reference {
        ChannelRef::Created(reference) => Some(reference.created.as_str()),
        ChannelRef::Existing(_) => None,
    }
}

fn instance_ref_key(reference: &InstanceRef) -> Option<&str> {
    match reference {
        InstanceRef::Created(reference) => Some(reference.created.as_str()),
        InstanceRef::Event => None,
    }
}

#[cfg(test)]
mod tests {
    use automation_state::InteractionRuleSet;

    use super::*;

    fn ruleset() -> InteractionRuleSet {
        InteractionRuleSet {
            version: 1,
            panels: vec![],
            modals: vec![],
            rules: vec![],
        }
    }

    #[test]
    fn modal_length_contract_errors_have_stable_codes_and_locations() {
        let cases = [
            (
                ValidationError::InvalidModalFieldMinLength {
                    modal: "room".to_string(),
                    field: "name".to_string(),
                    min_length: 4001,
                },
                "INVALID_MODAL_FIELD_MIN_LENGTH",
                "modal.room.field.name.min_length",
            ),
            (
                ValidationError::InvalidModalFieldMaxLength {
                    modal: "room".to_string(),
                    field: "name".to_string(),
                    max_length: 0,
                },
                "INVALID_MODAL_FIELD_MAX_LENGTH",
                "modal.room.field.name.max_length",
            ),
            (
                ValidationError::InvalidModalFieldLengthRange {
                    modal: "room".to_string(),
                    field: "name".to_string(),
                    min_length: 5,
                    max_length: 4,
                },
                "INVALID_MODAL_FIELD_LENGTH_RANGE",
                "modal.room.field.name",
            ),
        ];

        for (error, code, location) in cases {
            let translated = translate_validation_error(&ruleset(), &error);
            assert_eq!(translated.code, code);
            assert_eq!(translated.location, location);
        }
    }
}
