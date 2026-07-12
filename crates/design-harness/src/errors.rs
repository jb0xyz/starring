use automation_core::{AdapterError, AdapterErrorKind, ValidationError};
use automation_state::{ActionSpec, ChannelRef, InstanceRef, InteractionRuleSet, RoleRef};
use serde::Serialize;

use crate::draft::{Draft, DraftSummary};

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
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
