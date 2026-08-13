use std::collections::{BTreeMap, BTreeSet};

use automation_core::{validate_structural, SanitizeContext, TemplateError, TemplateString};
use automation_state::{InteractionRule, InteractionRuleSet};
use serde::{Deserialize, Serialize};

use crate::model::{
    lower_action, lower_modal, lower_panel, lower_trigger, ActionButtonRouteV1, ActionV1,
    AutomationSpecV1, ChannelReferenceV1, ConditionExprV1, InstanceReferenceV1, OverwriteTargetV1,
    RoleReferenceV1, TriggerV1, AUTOMATION_SPEC_KIND_V1, AUTOMATION_SPEC_SCHEMA_VERSION_V1,
};

pub const MAX_AUTOMATION_DISPLAY_NAME_CHARS_V1: usize = 100;
pub const MAX_AUTOMATION_DESCRIPTION_BYTES_V1: usize = 2_000;
pub const MAX_AUTOMATION_SPEC_CANONICAL_BYTES_V1: usize = 40 * 1_024;
pub const MAX_AUTOMATION_PANELS_V1: usize = 16;
pub const MAX_MODAL_DEFINITIONS_V1: usize = 16;
pub const MAX_AUTOMATION_WORKFLOWS_V1: usize = 32;
pub const MAX_ACTIONS_PER_WORKFLOW_V1: usize = 64;
pub const MAX_TOTAL_ACTIONS_V1: usize = 256;
pub const MAX_CONDITION_DEPTH_V1: usize = 8;
pub const MAX_CONDITION_NODES_V1: usize = 64;
pub const MAX_PANEL_CONTENT_CHARS_V1: usize = 2_000;
pub const MAX_PANEL_BUTTONS_V1: usize = 5;
pub const MAX_BUTTON_LABEL_CHARS_V1: usize = 80;
pub const MAX_MODAL_TITLE_CHARS_V1: usize = 45;
pub const MAX_MODAL_FIELDS_V1: usize = 5;
pub const MAX_MODAL_FIELD_LABEL_CHARS_V1: usize = 45;
pub const MAX_MODAL_INPUT_UTF16_UNITS_V1: usize = 4_000;
pub const MAX_TEMPLATE_SOURCE_CHARS_V1: usize = 2_000;
pub const MAX_RESOURCE_NAME_TEMPLATE_CHARS_V1: usize = 100;
pub const MAX_INSTANCE_RESOURCE_ALIASES_V1: usize = 64;
pub const MAX_SIMULATION_INPUTS_V1: usize = 5;
pub const MAX_SIMULATION_INPUT_UTF16_UNITS_V1: usize = 4_000;
pub const MAX_IDENTIFIER_BYTES_V1: usize = 64;
pub const MAX_INSTANCE_ACTION_ID_BYTES_V1: usize = 56;
pub const MAX_RESOURCE_ALIAS_BYTES_V1: usize = 32;
pub const MAX_DISCORD_CUSTOM_ID_BYTES_V1: usize = 100;

const MAX_DISCORD_GUILD_ID_DECIMAL_BYTES: usize = 20;
const COMPONENT_CUSTOM_ID_FIXED_BYTES: usize =
    "starring:".len() + MAX_DISCORD_GUILD_ID_DECIMAL_BYTES + 1 + 1 + "button:".len();

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AutomationSpecDiagnosticV1 {
    pub code: String,
    pub path: String,
    pub message: String,
}

#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
#[error("automation spec validation failed")]
pub struct AutomationSpecValidationErrorV1 {
    diagnostics: Vec<AutomationSpecDiagnosticV1>,
}

impl AutomationSpecValidationErrorV1 {
    pub fn diagnostics(&self) -> &[AutomationSpecDiagnosticV1] {
        &self.diagnostics
    }
}

pub fn validate_automation_spec_v1(
    spec: &AutomationSpecV1,
) -> Result<(), AutomationSpecValidationErrorV1> {
    let mut diagnostics = Vec::new();
    if spec.schema_version != AUTOMATION_SPEC_SCHEMA_VERSION_V1 {
        push(
            &mut diagnostics,
            "unsupported_schema_version",
            "/schema_version",
            "AutomationSpec V1 requires schema_version 1",
        );
    }
    if spec.kind != AUTOMATION_SPEC_KIND_V1 {
        push(
            &mut diagnostics,
            "invalid_kind",
            "/kind",
            format!("AutomationSpec V1 requires kind {AUTOMATION_SPEC_KIND_V1}"),
        );
    }
    if serde_json::to_vec(spec)
        .is_ok_and(|bytes| bytes.len() > MAX_AUTOMATION_SPEC_CANONICAL_BYTES_V1)
    {
        push(
            &mut diagnostics,
            "automation_spec_too_large",
            "/",
            format!(
                "the automation spec must not exceed {MAX_AUTOMATION_SPEC_CANONICAL_BYTES_V1} encoded bytes"
            ),
        );
    }
    validate_identifier(&mut diagnostics, "/key", &spec.key);
    let display_name_chars = spec.display_name.chars().count();
    if spec.display_name.trim().is_empty()
        || display_name_chars > MAX_AUTOMATION_DISPLAY_NAME_CHARS_V1
    {
        push(
            &mut diagnostics,
            "invalid_display_name",
            "/display_name",
            format!(
                "display_name must contain 1..={MAX_AUTOMATION_DISPLAY_NAME_CHARS_V1} characters"
            ),
        );
    }
    if spec.description.len() > MAX_AUTOMATION_DESCRIPTION_BYTES_V1 {
        push(
            &mut diagnostics,
            "description_too_large",
            "/description",
            format!(
                "description must not exceed {MAX_AUTOMATION_DESCRIPTION_BYTES_V1} UTF-8 bytes"
            ),
        );
    }
    bounded_len(
        &mut diagnostics,
        "/panels",
        spec.panels.len(),
        MAX_AUTOMATION_PANELS_V1,
    );
    bounded_len(
        &mut diagnostics,
        "/modals",
        spec.modals.len(),
        MAX_MODAL_DEFINITIONS_V1,
    );
    if spec.workflows.is_empty() || spec.workflows.len() > MAX_AUTOMATION_WORKFLOWS_V1 {
        push(
            &mut diagnostics,
            "invalid_workflow_count",
            "/workflows",
            format!("workflows must contain 1..={MAX_AUTOMATION_WORKFLOWS_V1} entries"),
        );
    }

    validate_panels(spec, &mut diagnostics);
    let modal_fields = validate_modals(spec, &mut diagnostics);
    validate_workflows(spec, &modal_fields, &mut diagnostics);
    validate_rendered_route_handlers(spec, &mut diagnostics);
    validate_created_instance_route_resources(spec, &mut diagnostics);

    if diagnostics.is_empty() {
        let lowered = lower_shape(spec);
        if let Err(errors) = validate_structural(&lowered) {
            for (index, error) in errors.into_iter().enumerate() {
                push(
                    &mut diagnostics,
                    "invalid_interaction_graph",
                    format!("/compiled_ruleset/errors/{index}"),
                    format!("{error:?}"),
                );
            }
        }
    }

    diagnostics.sort_by(|left, right| {
        left.path
            .cmp(&right.path)
            .then_with(|| left.code.cmp(&right.code))
            .then_with(|| left.message.cmp(&right.message))
    });
    if diagnostics.is_empty() {
        Ok(())
    } else {
        Err(AutomationSpecValidationErrorV1 { diagnostics })
    }
}

fn validate_rendered_route_handlers(
    spec: &AutomationSpecV1,
    diagnostics: &mut Vec<AutomationSpecDiagnosticV1>,
) {
    let mut handlers = BTreeMap::<String, usize>::new();
    for workflow in &spec.workflows {
        let identity = match &workflow.trigger {
            TriggerV1::ButtonClick { trigger_id } => format!("button:{trigger_id}"),
            TriggerV1::ModalSubmit { modal_id } => format!("modal:{modal_id}"),
            TriggerV1::InstanceAction { action_id } => format!("instance:{action_id}"),
        };
        *handlers.entry(identity).or_default() += 1;
    }

    for (panel_index, panel) in spec.panels.iter().enumerate() {
        for (button_index, button) in panel.buttons.iter().enumerate() {
            require_route_handler(
                diagnostics,
                &handlers,
                &format!("/panels/{panel_index}/buttons/{button_index}/trigger_id"),
                &format!("button:{}", button.trigger_id),
            );
        }
    }

    for (workflow_index, workflow) in spec.workflows.iter().enumerate() {
        for (action_index, node) in workflow.actions.iter().enumerate() {
            let path = format!("/workflows/{workflow_index}/actions/{action_index}/action");
            match &node.action {
                ActionV1::OpenModal { modal_id } => require_route_handler(
                    diagnostics,
                    &handlers,
                    &format!("{path}/modal_id"),
                    &format!("modal:{modal_id}"),
                ),
                ActionV1::PostPanel { buttons, .. } => {
                    for (button_index, button) in buttons.iter().enumerate() {
                        let identity = match &button.route {
                            ActionButtonRouteV1::Static { trigger_id } => {
                                format!("button:{trigger_id}")
                            }
                            ActionButtonRouteV1::InstanceAction { action_id, .. } => {
                                format!("instance:{action_id}")
                            }
                        };
                        require_route_handler(
                            diagnostics,
                            &handlers,
                            &format!("{path}/buttons/{button_index}/route"),
                            &identity,
                        );
                    }
                }
                _ => {}
            }
        }
    }
}

fn require_route_handler(
    diagnostics: &mut Vec<AutomationSpecDiagnosticV1>,
    handlers: &BTreeMap<String, usize>,
    path: &str,
    identity: &str,
) {
    if handlers.get(identity) != Some(&1) {
        push(
            diagnostics,
            "rendered_route_without_handler",
            path,
            "every rendered button or opened modal must have exactly one matching workflow",
        );
    }
}

fn validate_created_instance_route_resources(
    spec: &AutomationSpecV1,
    diagnostics: &mut Vec<AutomationSpecDiagnosticV1>,
) {
    let mut handler_counts = BTreeMap::<String, usize>::new();
    let mut required_event_roles = BTreeMap::<String, BTreeSet<String>>::new();
    let mut forwarded_event_actions = BTreeMap::<String, BTreeSet<String>>::new();

    for workflow in &spec.workflows {
        let TriggerV1::InstanceAction { action_id } = &workflow.trigger else {
            continue;
        };
        *handler_counts.entry(action_id.clone()).or_default() += 1;
        let roles = required_event_roles.entry(action_id.clone()).or_default();
        let forwarded = forwarded_event_actions
            .entry(action_id.clone())
            .or_default();
        for node in &workflow.actions {
            collect_event_role_aliases(&node.action, roles);
            if let ActionV1::PostPanel { buttons, .. } = &node.action {
                for button in buttons {
                    if let ActionButtonRouteV1::InstanceAction {
                        instance: InstanceReferenceV1::Event,
                        action_id,
                    } = &button.route
                    {
                        forwarded.insert(action_id.clone());
                    }
                }
            }
        }
    }

    loop {
        let mut changed = false;
        for (action_id, downstream_actions) in &forwarded_event_actions {
            let inherited = downstream_actions
                .iter()
                .filter_map(|downstream| required_event_roles.get(downstream))
                .flat_map(|roles| roles.iter().cloned())
                .collect::<BTreeSet<_>>();
            let roles = required_event_roles.entry(action_id.clone()).or_default();
            let previous_len = roles.len();
            roles.extend(inherited);
            changed |= roles.len() != previous_len;
        }
        if !changed {
            break;
        }
    }

    for (workflow_index, workflow) in spec.workflows.iter().enumerate() {
        let mut manifests = BTreeMap::new();
        let mut manifest_counts = BTreeMap::<&str, usize>::new();
        for node in &workflow.actions {
            if let ActionV1::RegisterInstance {
                output, resources, ..
            } = &node.action
            {
                *manifest_counts.entry(output.as_str()).or_default() += 1;
                manifests.entry(output.as_str()).or_insert(resources);
            }
        }
        for (action_index, node) in workflow.actions.iter().enumerate() {
            let ActionV1::PostPanel { buttons, .. } = &node.action else {
                continue;
            };
            for (button_index, button) in buttons.iter().enumerate() {
                let ActionButtonRouteV1::InstanceAction {
                    instance: InstanceReferenceV1::Created { output },
                    action_id,
                } = &button.route
                else {
                    continue;
                };
                if manifest_counts.get(output.as_str()) != Some(&1)
                    || handler_counts.get(action_id) != Some(&1)
                {
                    continue;
                }
                let Some(resources) = manifests.get(output.as_str()) else {
                    continue;
                };
                let Some(required_roles) = required_event_roles.get(action_id) else {
                    continue;
                };
                for alias in required_roles {
                    if !resources.roles.contains_key(alias) {
                        push(
                            diagnostics,
                            "created_instance_missing_handler_resource",
                            format!(
                                "/workflows/{workflow_index}/actions/{action_index}/action/buttons/{button_index}/route/instance"
                            ),
                            format!(
                                "the created instance must declare role alias {alias} required by this handler or a forwarded instance action"
                            ),
                        );
                    }
                }
            }
        }
    }
}

fn collect_event_role_aliases(action: &ActionV1, aliases: &mut BTreeSet<String>) {
    let role = match action {
        ActionV1::GrantRole { role, .. } => Some(role),
        ActionV1::UpsertOverwrite {
            target: OverwriteTargetV1::Role { role },
            ..
        } => Some(role),
        _ => None,
    };
    if let Some(RoleReferenceV1::Instance {
        instance: InstanceReferenceV1::Event,
        alias,
    }) = role
    {
        aliases.insert(alias.clone());
    }
}

fn validate_panels(spec: &AutomationSpecV1, diagnostics: &mut Vec<AutomationSpecDiagnosticV1>) {
    let mut panel_ids = BTreeSet::new();
    let mut trigger_ids = BTreeSet::new();
    for (panel_index, panel) in spec.panels.iter().enumerate() {
        let path = format!("/panels/{panel_index}");
        validate_identifier(diagnostics, &format!("{path}/id"), &panel.id);
        validate_identifier(diagnostics, &format!("{path}/channel"), &panel.channel);
        if !panel_ids.insert(panel.id.as_str()) {
            push(
                diagnostics,
                "duplicate_panel_id",
                format!("{path}/id"),
                "panel IDs must be unique",
            );
        }
        if panel.content.is_empty() && panel.buttons.is_empty() {
            push(
                diagnostics,
                "empty_panel",
                &path,
                "a panel must contain content or at least one button",
            );
        }
        bounded_chars(
            diagnostics,
            &format!("{path}/content"),
            &panel.content,
            0,
            MAX_PANEL_CONTENT_CHARS_V1,
            "invalid_panel_content",
        );
        validate_declared_panel_content(diagnostics, &format!("{path}/content"), &panel.content);
        if panel.buttons.len() > MAX_PANEL_BUTTONS_V1 {
            push(
                diagnostics,
                "too_many_panel_buttons",
                format!("{path}/buttons"),
                format!("panels support at most {MAX_PANEL_BUTTONS_V1} buttons"),
            );
        }
        for (button_index, button) in panel.buttons.iter().enumerate() {
            let button_path = format!("{path}/buttons/{button_index}");
            bounded_chars(
                diagnostics,
                &format!("{button_path}/label"),
                &button.label,
                1,
                MAX_BUTTON_LABEL_CHARS_V1,
                "invalid_button_label",
            );
            validate_identifier(
                diagnostics,
                &format!("{button_path}/trigger_id"),
                &button.trigger_id,
            );
            validate_component_custom_id_budget(
                diagnostics,
                &format!("{button_path}/trigger_id"),
                &spec.key,
                &button.trigger_id,
            );
            if !trigger_ids.insert(button.trigger_id.as_str()) {
                push(
                    diagnostics,
                    "duplicate_button_trigger_id",
                    format!("{button_path}/trigger_id"),
                    "button trigger IDs must be unique across declared panels",
                );
            }
        }
    }
}

fn validate_modals<'a>(
    spec: &'a AutomationSpecV1,
    diagnostics: &mut Vec<AutomationSpecDiagnosticV1>,
) -> BTreeMap<&'a str, BTreeSet<&'a str>> {
    let mut modal_ids = BTreeSet::new();
    let mut modal_fields = BTreeMap::new();
    for (modal_index, modal) in spec.modals.iter().enumerate() {
        let path = format!("/modals/{modal_index}");
        validate_identifier(diagnostics, &format!("{path}/id"), &modal.id);
        validate_component_custom_id_budget(
            diagnostics,
            &format!("{path}/id"),
            &spec.key,
            &modal.id,
        );
        if !modal_ids.insert(modal.id.as_str()) {
            push(
                diagnostics,
                "duplicate_modal_id",
                format!("{path}/id"),
                "modal IDs must be unique",
            );
        }
        bounded_chars(
            diagnostics,
            &format!("{path}/title"),
            &modal.title,
            1,
            MAX_MODAL_TITLE_CHARS_V1,
            "invalid_modal_title",
        );
        if modal.fields.is_empty() || modal.fields.len() > MAX_MODAL_FIELDS_V1 {
            push(
                diagnostics,
                "invalid_modal_field_count",
                format!("{path}/fields"),
                format!("modals must contain 1..={MAX_MODAL_FIELDS_V1} fields"),
            );
        }
        let mut field_ids = BTreeSet::new();
        for (field_index, field) in modal.fields.iter().enumerate() {
            let field_path = format!("{path}/fields/{field_index}");
            validate_identifier(diagnostics, &format!("{field_path}/id"), &field.id);
            if !field_ids.insert(field.id.as_str()) {
                push(
                    diagnostics,
                    "duplicate_modal_field_id",
                    format!("{field_path}/id"),
                    "modal field IDs must be unique within the modal",
                );
            }
            bounded_chars(
                diagnostics,
                &format!("{field_path}/label"),
                &field.label,
                1,
                MAX_MODAL_FIELD_LABEL_CHARS_V1,
                "invalid_modal_field_label",
            );
            if field
                .min_length
                .is_some_and(|value| usize::from(value) > MAX_MODAL_INPUT_UTF16_UNITS_V1)
            {
                push(
                    diagnostics,
                    "invalid_modal_min_length",
                    format!("{field_path}/min_length"),
                    "modal min_length must not exceed 4000 UTF-16 code units",
                );
            }
            if field.max_length.is_some_and(|value| {
                value == 0 || usize::from(value) > MAX_MODAL_INPUT_UTF16_UNITS_V1
            }) {
                push(
                    diagnostics,
                    "invalid_modal_max_length",
                    format!("{field_path}/max_length"),
                    "modal max_length must be in 1..=4000 UTF-16 code units",
                );
            }
            if matches!((field.min_length, field.max_length), (Some(min), Some(max)) if min > max) {
                push(
                    diagnostics,
                    "invalid_modal_length_range",
                    &field_path,
                    "modal min_length must not exceed max_length",
                );
            }
        }
        modal_fields.insert(modal.id.as_str(), field_ids);
    }
    modal_fields
}

fn validate_workflows(
    spec: &AutomationSpecV1,
    modal_fields: &BTreeMap<&str, BTreeSet<&str>>,
    diagnostics: &mut Vec<AutomationSpecDiagnosticV1>,
) {
    let mut workflow_ids = BTreeSet::new();
    let mut action_ids = BTreeSet::new();
    let mut trigger_identities = BTreeSet::new();
    let mut total_actions = 0usize;
    for (workflow_index, workflow) in spec.workflows.iter().enumerate() {
        let path = format!("/workflows/{workflow_index}");
        validate_identifier(diagnostics, &format!("{path}/id"), &workflow.id);
        if !workflow_ids.insert(workflow.id.as_str()) {
            push(
                diagnostics,
                "duplicate_workflow_id",
                format!("{path}/id"),
                "workflow IDs must be unique",
            );
        }
        let trigger_identity = match &workflow.trigger {
            TriggerV1::ButtonClick { trigger_id } => {
                validate_identifier(
                    diagnostics,
                    &format!("{path}/trigger/trigger_id"),
                    trigger_id,
                );
                format!("button:{trigger_id}")
            }
            TriggerV1::ModalSubmit { modal_id } => {
                validate_identifier(diagnostics, &format!("{path}/trigger/modal_id"), modal_id);
                format!("modal:{modal_id}")
            }
            TriggerV1::InstanceAction { action_id } => {
                validate_instance_action_id(
                    diagnostics,
                    &format!("{path}/trigger/action_id"),
                    action_id,
                );
                format!("instance:{action_id}")
            }
        };
        if !trigger_identities.insert(trigger_identity) {
            push(
                diagnostics,
                "duplicate_trigger",
                format!("{path}/trigger"),
                "only one workflow may consume a trigger",
            );
        }
        if workflow.actions.is_empty() || workflow.actions.len() > MAX_ACTIONS_PER_WORKFLOW_V1 {
            push(
                diagnostics,
                "invalid_action_count",
                format!("{path}/actions"),
                format!("each workflow must contain 1..={MAX_ACTIONS_PER_WORKFLOW_V1} actions"),
            );
        }
        total_actions = total_actions.saturating_add(workflow.actions.len());
        let mut condition_nodes = 0;
        let registered_instance_outputs = workflow
            .actions
            .iter()
            .filter_map(|node| match &node.action {
                ActionV1::RegisterInstance { output, .. } => Some(output.as_str()),
                _ => None,
            })
            .collect::<BTreeSet<_>>();
        let declared_fields = match &workflow.trigger {
            TriggerV1::ModalSubmit { modal_id } => modal_fields.get(modal_id.as_str()),
            TriggerV1::ButtonClick { .. } | TriggerV1::InstanceAction { .. } => None,
        };
        validate_condition(
            &workflow.condition,
            &format!("{path}/condition"),
            1,
            &mut condition_nodes,
            declared_fields,
            matches!(workflow.trigger, TriggerV1::ModalSubmit { .. }),
            diagnostics,
        );
        let mut teardown_seen = false;
        for (action_index, node) in workflow.actions.iter().enumerate() {
            let node_path = format!("{path}/actions/{action_index}");
            if matches!(
                node.action,
                ActionV1::RespondEphemeral { .. } | ActionV1::OpenModal { .. }
            ) && action_index + 1 != workflow.actions.len()
            {
                push(
                    diagnostics,
                    "initial_response_not_final",
                    format!("{node_path}/action"),
                    "respond_ephemeral and open_modal must be the final workflow action",
                );
            }
            if teardown_seen && !matches!(node.action, ActionV1::EditResponse { .. }) {
                push(
                    diagnostics,
                    "action_after_teardown",
                    format!("{node_path}/action"),
                    "teardown_instance must be the final mutable action; only edit_response may follow",
                );
            }
            validate_identifier(diagnostics, &format!("{node_path}/id"), &node.id);
            if !action_ids.insert(node.id.as_str()) {
                push(
                    diagnostics,
                    "duplicate_action_node_id",
                    format!("{node_path}/id"),
                    "action node IDs must be unique across the automation",
                );
            }
            validate_action(
                spec,
                &node.action,
                &format!("{node_path}/action"),
                &workflow.trigger,
                &registered_instance_outputs,
                diagnostics,
            );
            teardown_seen |= matches!(node.action, ActionV1::TeardownInstance { .. });
        }
        let direct_response_count = workflow
            .actions
            .iter()
            .filter(|node| {
                matches!(
                    node.action,
                    ActionV1::RespondEphemeral { .. } | ActionV1::OpenModal { .. }
                )
            })
            .count();
        let initial_response_count = workflow
            .actions
            .iter()
            .filter(|node| {
                matches!(
                    node.action,
                    ActionV1::RespondEphemeral { .. }
                        | ActionV1::OpenModal { .. }
                        | ActionV1::DeferEphemeral
                )
            })
            .count();
        if initial_response_count != 1 {
            push(
                diagnostics,
                "invalid_initial_response_count",
                format!("{path}/actions"),
                "each workflow must acknowledge its interaction exactly once with respond_ephemeral, open_modal, or a leading defer_ephemeral",
            );
        }
        if direct_response_count > 1 {
            push(
                diagnostics,
                "multiple_initial_responses",
                format!("{path}/actions"),
                "a workflow may send at most one immediate interaction response",
            );
        }
    }
    if total_actions > MAX_TOTAL_ACTIONS_V1 {
        push(
            diagnostics,
            "automation_action_budget_exceeded",
            "/workflows",
            format!("the automation must not exceed {MAX_TOTAL_ACTIONS_V1} total actions"),
        );
    }
}

fn validate_action(
    spec: &AutomationSpecV1,
    action: &ActionV1,
    path: &str,
    workflow_trigger: &TriggerV1,
    registered_instance_outputs: &BTreeSet<&str>,
    diagnostics: &mut Vec<AutomationSpecDiagnosticV1>,
) {
    match action {
        ActionV1::GrantRole { role, .. } => validate_role_ref(diagnostics, path, role),
        ActionV1::RespondEphemeral { content } | ActionV1::EditResponse { content } => {
            bounded_chars(
                diagnostics,
                &format!("{path}/content"),
                content,
                1,
                MAX_TEMPLATE_SOURCE_CHARS_V1,
                "invalid_response_content",
            );
            validate_literal_template(
                diagnostics,
                &format!("{path}/content"),
                content,
                SanitizeContext::EphemeralMessageContent,
            );
        }
        ActionV1::OpenModal { modal_id } => {
            validate_identifier(diagnostics, &format!("{path}/modal_id"), modal_id);
        }
        ActionV1::CreateChannel { output, name } | ActionV1::CreateRole { output, name } => {
            validate_identifier(diagnostics, &format!("{path}/output"), output);
            bounded_chars(
                diagnostics,
                &format!("{path}/name"),
                name,
                1,
                MAX_RESOURCE_NAME_TEMPLATE_CHARS_V1,
                "invalid_resource_name_template",
            );
            let context = if matches!(action, ActionV1::CreateChannel { .. }) {
                SanitizeContext::ChannelName
            } else {
                SanitizeContext::RoleName
            };
            validate_literal_template(diagnostics, &format!("{path}/name"), name, context);
        }
        ActionV1::UpsertOverwrite {
            channel,
            target,
            allow,
            deny,
        } => {
            validate_channel_ref(diagnostics, path, channel);
            if let OverwriteTargetV1::Role { role } = target {
                validate_role_ref(diagnostics, path, role);
            }
            validate_permission_set(diagnostics, &format!("{path}/allow"), allow);
            validate_permission_set(diagnostics, &format!("{path}/deny"), deny);
            if allow.iter().any(|permission| deny.contains(permission)) {
                push(
                    diagnostics,
                    "overlapping_overwrite_permissions",
                    path,
                    "the same permission cannot be both allowed and denied",
                );
            }
            if allow.is_empty() && deny.is_empty() {
                push(
                    diagnostics,
                    "empty_overwrite",
                    path,
                    "an overwrite must allow or deny at least one permission",
                );
            }
        }
        ActionV1::PostPanel {
            output,
            channel,
            content,
            buttons,
        } => {
            validate_identifier(diagnostics, &format!("{path}/output"), output);
            validate_channel_ref(diagnostics, path, channel);
            if content.is_empty() {
                push(
                    diagnostics,
                    "empty_panel_content",
                    format!("{path}/content"),
                    "runtime panels require nonempty content",
                );
            }
            bounded_chars(
                diagnostics,
                &format!("{path}/content"),
                content,
                1,
                MAX_TEMPLATE_SOURCE_CHARS_V1,
                "invalid_panel_content",
            );
            validate_literal_template(
                diagnostics,
                &format!("{path}/content"),
                content,
                SanitizeContext::EphemeralMessageContent,
            );
            if buttons.is_empty() || buttons.len() > MAX_PANEL_BUTTONS_V1 {
                push(
                    diagnostics,
                    "invalid_runtime_panel_button_count",
                    format!("{path}/buttons"),
                    format!(
                        "runtime panels require 1..={MAX_PANEL_BUTTONS_V1} buttons in runtime V1"
                    ),
                );
            }
            let mut route_identities = BTreeSet::new();
            for (index, button) in buttons.iter().enumerate() {
                let button_path = format!("{path}/buttons/{index}");
                bounded_chars(
                    diagnostics,
                    &format!("{button_path}/label"),
                    &button.label,
                    1,
                    MAX_BUTTON_LABEL_CHARS_V1,
                    "invalid_button_label",
                );
                match &button.route {
                    ActionButtonRouteV1::Static { trigger_id } => {
                        validate_identifier(
                            diagnostics,
                            &format!("{button_path}/route/trigger_id"),
                            trigger_id,
                        );
                        validate_component_custom_id_budget(
                            diagnostics,
                            &format!("{button_path}/route/trigger_id"),
                            &spec.key,
                            trigger_id,
                        );
                        if !route_identities.insert(format!("static:{trigger_id}")) {
                            push(
                                diagnostics,
                                "duplicate_runtime_panel_route",
                                format!("{button_path}/route"),
                                "runtime panel button routes must produce unique custom IDs",
                            );
                        }
                    }
                    ActionButtonRouteV1::InstanceAction {
                        instance,
                        action_id,
                    } => {
                        validate_instance_ref(diagnostics, &button_path, instance);
                        validate_instance_action_id(
                            diagnostics,
                            &format!("{button_path}/route/action_id"),
                            action_id,
                        );
                        let route_identity = match instance {
                            InstanceReferenceV1::Event => {
                                if !matches!(workflow_trigger, TriggerV1::InstanceAction { .. }) {
                                    push(
                                        diagnostics,
                                        "event_instance_requires_instance_trigger",
                                        format!("{button_path}/route/instance"),
                                        "the event instance is available only in an instance_action workflow",
                                    );
                                }
                                format!("event:{action_id}")
                            }
                            InstanceReferenceV1::Created { output } => {
                                if !registered_instance_outputs.contains(output.as_str()) {
                                    push(
                                        diagnostics,
                                        "unknown_panel_instance_output",
                                        format!("{button_path}/route/instance/output"),
                                        "created instance routes must reference a register_instance output in the same workflow",
                                    );
                                }
                                format!("created:{output}:{action_id}")
                            }
                        };
                        if !route_identities.insert(route_identity) {
                            push(
                                diagnostics,
                                "duplicate_runtime_panel_route",
                                format!("{button_path}/route"),
                                "runtime panel button routes must produce unique custom IDs",
                            );
                        }
                    }
                }
            }
        }
        ActionV1::DeferEphemeral => {}
        ActionV1::RegisterInstance {
            output,
            instance_kind,
            resources,
        } => {
            validate_identifier(diagnostics, &format!("{path}/output"), output);
            validate_identifier(diagnostics, &format!("{path}/instance_kind"), instance_kind);
            let resource_count =
                resources.roles.len() + resources.channels.len() + resources.messages.len();
            if resource_count > MAX_INSTANCE_RESOURCE_ALIASES_V1 {
                push(
                    diagnostics,
                    "instance_resource_budget_exceeded",
                    format!("{path}/resources"),
                    format!(
                        "an instance may declare at most {MAX_INSTANCE_RESOURCE_ALIASES_V1} resource aliases"
                    ),
                );
            }
            for (kind, entries) in [
                ("roles", &resources.roles),
                ("channels", &resources.channels),
                ("messages", &resources.messages),
            ] {
                for (alias, reference) in entries {
                    validate_resource_alias(
                        diagnostics,
                        &format!("{path}/resources/{kind}/{alias}"),
                        alias,
                    );
                    validate_identifier(
                        diagnostics,
                        &format!("{path}/resources/{kind}/{alias}/output"),
                        &reference.output,
                    );
                }
            }
        }
        ActionV1::TeardownInstance { instance } => {
            validate_instance_ref(diagnostics, path, instance)
        }
    }
}

fn validate_condition(
    condition: &ConditionExprV1,
    path: &str,
    depth: usize,
    nodes: &mut usize,
    declared_fields: Option<&BTreeSet<&str>>,
    modal_trigger: bool,
    diagnostics: &mut Vec<AutomationSpecDiagnosticV1>,
) {
    *nodes = nodes.saturating_add(1);
    if depth > MAX_CONDITION_DEPTH_V1 {
        push(
            diagnostics,
            "condition_depth_exceeded",
            path,
            format!("condition depth must not exceed {MAX_CONDITION_DEPTH_V1}"),
        );
        return;
    }
    if *nodes > MAX_CONDITION_NODES_V1 {
        push(
            diagnostics,
            "condition_node_budget_exceeded",
            path,
            format!("a condition must not exceed {MAX_CONDITION_NODES_V1} nodes"),
        );
        return;
    }
    match condition {
        ConditionExprV1::Always => {}
        ConditionExprV1::InputNonEmpty { input_id }
        | ConditionExprV1::InputEquals { input_id, .. } => {
            validate_identifier(diagnostics, &format!("{path}/input_id"), input_id);
            if !modal_trigger {
                push(
                    diagnostics,
                    "input_condition_requires_modal_trigger",
                    path,
                    "input conditions are available only for modal_submit workflows",
                );
            } else if declared_fields.is_some_and(|fields| !fields.contains(input_id.as_str())) {
                push(
                    diagnostics,
                    "unknown_condition_input",
                    format!("{path}/input_id"),
                    "condition input_id must name a field declared by the trigger modal",
                );
            }
            if let ConditionExprV1::InputEquals { value, .. } = condition {
                if value.encode_utf16().count() > MAX_MODAL_INPUT_UTF16_UNITS_V1 {
                    push(
                        diagnostics,
                        "condition_literal_too_large",
                        format!("{path}/value"),
                        "condition literals must not exceed 4000 UTF-16 code units",
                    );
                }
            }
        }
        ConditionExprV1::All { conditions } | ConditionExprV1::Any { conditions } => {
            if conditions.is_empty() {
                push(
                    diagnostics,
                    "empty_condition_group",
                    format!("{path}/conditions"),
                    "all and any condition groups must not be empty",
                );
            }
            for (index, child) in conditions.iter().enumerate() {
                validate_condition(
                    child,
                    &format!("{path}/conditions/{index}"),
                    depth + 1,
                    nodes,
                    declared_fields,
                    modal_trigger,
                    diagnostics,
                );
            }
        }
        ConditionExprV1::Not { condition } => validate_condition(
            condition,
            &format!("{path}/condition"),
            depth + 1,
            nodes,
            declared_fields,
            modal_trigger,
            diagnostics,
        ),
    }
}

fn validate_role_ref(
    diagnostics: &mut Vec<AutomationSpecDiagnosticV1>,
    path: &str,
    reference: &RoleReferenceV1,
) {
    match reference {
        RoleReferenceV1::Existing { binding } => {
            validate_identifier(diagnostics, &format!("{path}/role/binding"), binding)
        }
        RoleReferenceV1::Created { output } => {
            validate_identifier(diagnostics, &format!("{path}/role/output"), output)
        }
        RoleReferenceV1::Instance { instance, alias } => {
            validate_instance_ref(diagnostics, &format!("{path}/role"), instance);
            validate_resource_alias(diagnostics, &format!("{path}/role/alias"), alias);
        }
    }
}

fn validate_channel_ref(
    diagnostics: &mut Vec<AutomationSpecDiagnosticV1>,
    path: &str,
    reference: &ChannelReferenceV1,
) {
    match reference {
        ChannelReferenceV1::Existing { binding } => {
            validate_identifier(diagnostics, &format!("{path}/channel/binding"), binding)
        }
        ChannelReferenceV1::Created { output } => {
            validate_identifier(diagnostics, &format!("{path}/channel/output"), output)
        }
    }
}

fn validate_instance_ref(
    diagnostics: &mut Vec<AutomationSpecDiagnosticV1>,
    path: &str,
    reference: &InstanceReferenceV1,
) {
    if let InstanceReferenceV1::Created { output } = reference {
        validate_identifier(diagnostics, &format!("{path}/instance/output"), output);
    }
}

fn validate_permission_set(
    diagnostics: &mut Vec<AutomationSpecDiagnosticV1>,
    path: &str,
    permissions: &[crate::model::DiscordPermissionV1],
) {
    let mut unique = BTreeSet::new();
    if permissions
        .iter()
        .copied()
        .any(|permission| !unique.insert(permission))
    {
        push(
            diagnostics,
            "duplicate_permission",
            path,
            "permission lists must not contain duplicates",
        );
    }
}

fn validate_literal_template(
    diagnostics: &mut Vec<AutomationSpecDiagnosticV1>,
    path: &str,
    source: &str,
    context: SanitizeContext,
) {
    let Ok(template) = TemplateString::parse(source) else {
        return;
    };
    let input_keys = template.input_keys();
    let empty_inputs = input_keys
        .iter()
        .map(|key| ((*key).to_string(), String::new()))
        .collect::<BTreeMap<_, _>>();
    let unrenderable = match template.render(&empty_inputs, context) {
        Err(TemplateError::TooLong { .. }) => true,
        Err(TemplateError::EmptyAfterSanitize) => input_keys.is_empty(),
        Err(
            TemplateError::BadSyntax(_)
            | TemplateError::UnsupportedVariable(_)
            | TemplateError::MissingInput(_),
        ) => true,
        Ok(_) => false,
    };
    if unrenderable {
        push(
            diagnostics,
            "literal_template_unrenderable",
            path,
            "the template's fixed output must remain within its runtime limit, and literal-only output must remain nonempty after sanitization",
        );
    }
}

fn validate_declared_panel_content(
    diagnostics: &mut Vec<AutomationSpecDiagnosticV1>,
    path: &str,
    content: &str,
) {
    if content.is_empty() {
        return;
    }
    let rendered = TemplateString::parse(content).and_then(|template| {
        template.render(&BTreeMap::new(), SanitizeContext::EphemeralMessageContent)
    });
    if !matches!(rendered, Ok(ref safe) if safe == content) {
        push(
            diagnostics,
            "unsafe_declared_panel_content",
            path,
            "declared panel content must already be nonempty, mention-neutral, control-free, and within the runtime message limit",
        );
    }
}

fn validate_component_custom_id_budget(
    diagnostics: &mut Vec<AutomationSpecDiagnosticV1>,
    path: &str,
    ruleset_key: &str,
    component_key: &str,
) {
    if COMPONENT_CUSTOM_ID_FIXED_BYTES + ruleset_key.len() + component_key.len()
        > MAX_DISCORD_CUSTOM_ID_BYTES_V1
    {
        push(
            diagnostics,
            "component_custom_id_too_large",
            path,
            "the automation key and component ID do not fit Discord's 100-byte custom ID limit",
        );
    }
}

fn validate_instance_action_id(
    diagnostics: &mut Vec<AutomationSpecDiagnosticV1>,
    path: &str,
    value: &str,
) {
    validate_identifier(diagnostics, path, value);
    if value.len() > MAX_INSTANCE_ACTION_ID_BYTES_V1 {
        push(
            diagnostics,
            "instance_action_id_too_large",
            path,
            "instance action IDs must fit the Discord custom ID budget",
        );
    }
}

pub(crate) fn lower_shape(spec: &AutomationSpecV1) -> InteractionRuleSet {
    InteractionRuleSet {
        version: 1,
        panels: spec.panels.iter().map(lower_panel).collect(),
        modals: spec.modals.iter().map(lower_modal).collect(),
        rules: spec
            .workflows
            .iter()
            .map(|workflow| InteractionRule {
                key: workflow.id.clone(),
                trigger: lower_trigger(&workflow.trigger),
                actions: workflow
                    .actions
                    .iter()
                    .map(|node| lower_action(&node.action))
                    .collect(),
            })
            .collect(),
    }
}

fn validate_identifier(diagnostics: &mut Vec<AutomationSpecDiagnosticV1>, path: &str, value: &str) {
    let valid = !value.is_empty()
        && value.len() <= MAX_IDENTIFIER_BYTES_V1
        && value
            .bytes()
            .next()
            .is_some_and(|byte| byte.is_ascii_lowercase())
        && value
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_');
    if !valid {
        push(
            diagnostics,
            "invalid_identifier",
            path,
            "identifiers must match [a-z][a-z0-9_]{0,63}",
        );
    }
}

fn validate_resource_alias(
    diagnostics: &mut Vec<AutomationSpecDiagnosticV1>,
    path: &str,
    value: &str,
) {
    let valid = !value.is_empty()
        && value.len() <= MAX_RESOURCE_ALIAS_BYTES_V1
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'));
    if !valid {
        push(
            diagnostics,
            "invalid_resource_alias",
            path,
            "instance resource aliases must contain 1..=32 ASCII alphanumeric, '_' or '-' bytes",
        );
    }
}

fn bounded_chars(
    diagnostics: &mut Vec<AutomationSpecDiagnosticV1>,
    path: &str,
    value: &str,
    minimum: usize,
    maximum: usize,
    code: &str,
) {
    let count = value.chars().count();
    if count < minimum || count > maximum {
        push(
            diagnostics,
            code,
            path,
            format!("value must contain {minimum}..={maximum} characters"),
        );
    }
}

fn bounded_len(
    diagnostics: &mut Vec<AutomationSpecDiagnosticV1>,
    path: &str,
    actual: usize,
    maximum: usize,
) {
    if actual > maximum {
        push(
            diagnostics,
            "collection_budget_exceeded",
            path,
            format!("collection must not exceed {maximum} entries"),
        );
    }
}

pub(crate) fn diagnostic(
    code: &str,
    path: impl Into<String>,
    message: impl Into<String>,
) -> AutomationSpecDiagnosticV1 {
    AutomationSpecDiagnosticV1 {
        code: code.to_string(),
        path: path.into(),
        message: message.into(),
    }
}

fn push(
    diagnostics: &mut Vec<AutomationSpecDiagnosticV1>,
    code: &str,
    path: impl Into<String>,
    message: impl Into<String>,
) {
    diagnostics.push(diagnostic(code, path, message));
}
