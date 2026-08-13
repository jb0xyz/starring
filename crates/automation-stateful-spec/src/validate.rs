use std::collections::{BTreeMap, BTreeSet};

use automation_spec::{
    validate_automation_spec_v1, ActionButtonRouteV1, ActionNodeV1, ActionV1, ConditionExprV1,
    InstanceReferenceV1, OverwriteTargetV1, RoleReferenceV1, TriggerV1,
};
use serde::{Deserialize, Serialize};

use crate::model::{
    StatePrimitiveTypeV1, StateScopeV1, StateValueTypeV1, StateValueV1, StateVariableV1,
    StatefulBranchV1, StatefulConditionExprV1, StatefulSpecV1, StatefulValueExprV1,
    MAX_SAFE_INTEGER_V1, STATEFUL_SPEC_KIND_V1, STATEFUL_SPEC_SCHEMA_VERSION_V1,
};
use crate::view::{automation_spec_validation_view_v1, BranchViewV1};

pub const MAX_STATEFUL_SPEC_CANONICAL_BYTES_V1: usize = 64 * 1_024;
pub const MAX_STATE_VARIABLES_V1: usize = 64;
pub const MAX_STATEFUL_WORKFLOWS_V1: usize = 32;
pub const MAX_NODES_PER_BRANCH_V1: usize = 64;
pub const MAX_STATE_ACTIONS_PER_BRANCH_V1: usize = 32;
pub const MAX_TOTAL_STATEFUL_NODES_V1: usize = 512;
pub const MAX_CONDITION_DEPTH_V1: usize = 8;
pub const MAX_CONDITION_NODES_V1: usize = 64;
pub const MAX_VALUE_EXPR_DEPTH_V1: usize = 8;
pub const MAX_VALUE_EXPR_NODES_V1: usize = 64;
pub const MAX_STATE_TEXT_BYTES_V1: usize = 4_000;
pub const MAX_STATE_TEXT_UTF16_UNITS_V1: usize = 4_000;
const MAX_IDENTIFIER_BYTES_V1: usize = 64;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StatefulSpecDiagnosticV1 {
    pub code: String,
    pub path: String,
    pub message: String,
}

#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
#[error("stateful spec validation failed")]
pub struct StatefulSpecValidationErrorV1 {
    diagnostics: Vec<StatefulSpecDiagnosticV1>,
}

impl StatefulSpecValidationErrorV1 {
    pub fn diagnostics(&self) -> &[StatefulSpecDiagnosticV1] {
        &self.diagnostics
    }
}

pub fn validate_stateful_spec_v1(
    spec: &StatefulSpecV1,
) -> Result<(), StatefulSpecValidationErrorV1> {
    let mut diagnostics = Vec::new();
    validate_header_and_bounds(spec, &mut diagnostics);
    let variables = validate_state_declarations(spec, &mut diagnostics);
    validate_graph(spec, &variables, &mut diagnostics);
    validate_cross_branch_instance_resources(spec, &mut diagnostics);
    validate_runtime_shape_views(spec, &mut diagnostics);
    diagnostics.sort_by(|left, right| {
        left.path
            .cmp(&right.path)
            .then(left.code.cmp(&right.code))
            .then(left.message.cmp(&right.message))
    });
    diagnostics.dedup();
    if diagnostics.is_empty() {
        Ok(())
    } else {
        Err(StatefulSpecValidationErrorV1 { diagnostics })
    }
}

fn validate_header_and_bounds(
    spec: &StatefulSpecV1,
    diagnostics: &mut Vec<StatefulSpecDiagnosticV1>,
) {
    if spec.schema_version != STATEFUL_SPEC_SCHEMA_VERSION_V1 {
        push(
            diagnostics,
            "unsupported_schema_version",
            "/schema_version",
            "StatefulSpec V1 requires schema_version 1",
        );
    }
    if spec.kind != STATEFUL_SPEC_KIND_V1 {
        push(
            diagnostics,
            "invalid_kind",
            "/kind",
            format!("StatefulSpec V1 requires kind {STATEFUL_SPEC_KIND_V1}"),
        );
    }
    if serde_json::to_vec(spec)
        .is_ok_and(|bytes| bytes.len() > MAX_STATEFUL_SPEC_CANONICAL_BYTES_V1)
    {
        push(
            diagnostics,
            "stateful_spec_too_large",
            "/",
            format!(
                "the stateful spec must not exceed {MAX_STATEFUL_SPEC_CANONICAL_BYTES_V1} encoded bytes"
            ),
        );
    }
    if spec.state_variables.len() > MAX_STATE_VARIABLES_V1 {
        push(
            diagnostics,
            "state_variable_count_exceeded",
            "/state_variables",
            format!("at most {MAX_STATE_VARIABLES_V1} state variables are supported"),
        );
    }
    let workflow_count = spec
        .stateless_workflows
        .len()
        .saturating_add(spec.stateful_workflows.len());
    if workflow_count == 0 || workflow_count > MAX_STATEFUL_WORKFLOWS_V1 {
        push(
            diagnostics,
            "invalid_workflow_count",
            "/",
            format!(
                "stateless_workflows and stateful_workflows together must contain 1..={MAX_STATEFUL_WORKFLOWS_V1} entries"
            ),
        );
    }
}

fn validate_state_declarations<'a>(
    spec: &'a StatefulSpecV1,
    diagnostics: &mut Vec<StatefulSpecDiagnosticV1>,
) -> BTreeMap<&'a str, &'a StateVariableV1> {
    let mut variables = BTreeMap::new();
    for (index, variable) in spec.state_variables.iter().enumerate() {
        let path = format!("/state_variables/{index}");
        validate_identifier(diagnostics, &format!("{path}/id"), &variable.id);
        if variables.insert(variable.id.as_str(), variable).is_some() {
            push(
                diagnostics,
                "duplicate_state_variable_id",
                format!("{path}/id"),
                "state variable IDs must be unique",
            );
        }
        match &variable.value_type {
            StateValueTypeV1::Bool => {}
            StateValueTypeV1::Integer { min, max }
                if min > max || *min < -MAX_SAFE_INTEGER_V1 || *max > MAX_SAFE_INTEGER_V1 =>
            {
                push(
                    diagnostics,
                    "invalid_integer_bounds",
                    format!("{path}/value_type"),
                    "integer state requires JS-safe min/max with min less than or equal to max",
                )
            }
            StateValueTypeV1::Integer { .. } => {}
            StateValueTypeV1::Text { max_utf8_bytes }
                if *max_utf8_bytes == 0
                    || usize::from(*max_utf8_bytes) > MAX_STATE_TEXT_BYTES_V1 =>
            {
                push(
                    diagnostics,
                    "invalid_text_bound",
                    format!("{path}/value_type/max_utf8_bytes"),
                    "text state max_utf8_bytes must be in 1..=4000",
                )
            }
            StateValueTypeV1::Text { .. } => {}
        }
        validate_state_value(
            &variable.initial_value,
            &format!("{path}/initial_value"),
            diagnostics,
        );
        if variable.value_type.primitive_type() != variable.initial_value.primitive_type() {
            push(
                diagnostics,
                "initial_value_type_mismatch",
                format!("{path}/initial_value"),
                "initial_value primitive type must match value_type",
            );
        } else if !variable.value_type.accepts(&variable.initial_value) {
            push(
                diagnostics,
                "initial_value_out_of_bounds",
                format!("{path}/initial_value"),
                "initial_value must satisfy its declared integer or text bounds",
            );
        }
    }
    variables
}

fn validate_graph(
    spec: &StatefulSpecV1,
    variables: &BTreeMap<&str, &StateVariableV1>,
    diagnostics: &mut Vec<StatefulSpecDiagnosticV1>,
) {
    let modal_fields = spec
        .modals
        .iter()
        .map(|modal| {
            (
                modal.id.as_str(),
                modal
                    .fields
                    .iter()
                    .map(|field| field.id.as_str())
                    .collect::<BTreeSet<_>>(),
            )
        })
        .collect::<BTreeMap<_, _>>();
    let mut workflow_ids = BTreeSet::new();
    let mut trigger_ids = BTreeSet::new();
    let mut node_ids = BTreeSet::new();

    for (index, workflow) in spec.stateless_workflows.iter().enumerate() {
        let path = format!("/stateless_workflows/{index}");
        register_workflow_identity(
            &workflow.id,
            &workflow.trigger,
            &path,
            &mut workflow_ids,
            &mut trigger_ids,
            diagnostics,
        );
        if !matches!(workflow.condition, ConditionExprV1::Always) {
            push(
                diagnostics,
                "stateless_condition_must_be_always",
                format!("{path}/condition"),
                "StatefulSpec V1 stateless workflows must use the always condition; conditional branching belongs in stateful_workflows",
            );
        }
        for (action_index, node) in workflow.actions.iter().enumerate() {
            register_node_id(
                &node.id,
                &format!("{path}/actions/{action_index}/id"),
                &mut node_ids,
                diagnostics,
            );
        }
    }

    let mut total_nodes = 0usize;
    for (index, workflow) in spec.stateful_workflows.iter().enumerate() {
        let path = format!("/stateful_workflows/{index}");
        register_workflow_identity(
            &workflow.id,
            &workflow.trigger,
            &path,
            &mut workflow_ids,
            &mut trigger_ids,
            diagnostics,
        );
        let declared_inputs = match &workflow.trigger {
            TriggerV1::ModalSubmit { modal_id } => modal_fields.get(modal_id.as_str()),
            TriggerV1::ButtonClick { .. } | TriggerV1::InstanceAction { .. } => None,
        };
        let input_context = InputContext {
            is_modal: matches!(workflow.trigger, TriggerV1::ModalSubmit { .. }),
            declared_inputs,
        };
        let mut condition_nodes = 0;
        validate_condition(
            &workflow.condition,
            &format!("{path}/condition"),
            1,
            &mut condition_nodes,
            &workflow.trigger,
            input_context,
            variables,
            diagnostics,
        );
        for (branch_name, branch) in [
            ("on_true", &workflow.on_true),
            ("on_false", &workflow.on_false),
        ] {
            total_nodes = total_nodes
                .saturating_add(branch.state_actions.len())
                .saturating_add(branch.effects.len())
                .saturating_add(1);
            validate_branch(
                branch,
                &format!("{path}/{branch_name}"),
                &workflow.trigger,
                input_context,
                variables,
                &mut node_ids,
                diagnostics,
            );
        }
    }
    if total_nodes > MAX_TOTAL_STATEFUL_NODES_V1 {
        push(
            diagnostics,
            "stateful_node_budget_exceeded",
            "/stateful_workflows",
            format!("at most {MAX_TOTAL_STATEFUL_NODES_V1} stateful nodes are supported"),
        );
    }
}

fn validate_cross_branch_instance_resources(
    spec: &StatefulSpecV1,
    diagnostics: &mut Vec<StatefulSpecDiagnosticV1>,
) {
    let mut handler_counts = BTreeMap::<String, usize>::new();
    let mut required_event_roles = BTreeMap::<String, BTreeSet<String>>::new();
    let mut forwarded_event_actions = BTreeMap::<String, BTreeSet<String>>::new();

    for workflow in &spec.stateless_workflows {
        let TriggerV1::InstanceAction { action_id } = &workflow.trigger else {
            continue;
        };
        register_handler_requirements(
            action_id,
            &workflow.actions,
            &mut handler_counts,
            &mut required_event_roles,
            &mut forwarded_event_actions,
        );
    }
    for workflow in &spec.stateful_workflows {
        let TriggerV1::InstanceAction { action_id } = &workflow.trigger else {
            continue;
        };
        *handler_counts.entry(action_id.clone()).or_default() += 1;
        for branch in [&workflow.on_true, &workflow.on_false] {
            collect_handler_requirements(
                action_id,
                &branch.effects,
                &mut required_event_roles,
                &mut forwarded_event_actions,
            );
        }
    }

    // A forwarding handler inherits the union of every downstream handler requirement. The fixed
    // point is intentionally computed after true/false requirements have already been unioned.
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

    for (workflow_index, workflow) in spec.stateless_workflows.iter().enumerate() {
        validate_creator_branch(
            &workflow.actions,
            &format!("/stateless_workflows/{workflow_index}/actions"),
            &handler_counts,
            &required_event_roles,
            diagnostics,
        );
    }
    for (workflow_index, workflow) in spec.stateful_workflows.iter().enumerate() {
        for (branch_name, branch) in [
            ("on_true", &workflow.on_true),
            ("on_false", &workflow.on_false),
        ] {
            validate_creator_branch(
                &branch.effects,
                &format!("/stateful_workflows/{workflow_index}/{branch_name}/effects"),
                &handler_counts,
                &required_event_roles,
                diagnostics,
            );
        }
    }
}

fn register_handler_requirements(
    action_id: &str,
    actions: &[ActionNodeV1],
    handler_counts: &mut BTreeMap<String, usize>,
    required_event_roles: &mut BTreeMap<String, BTreeSet<String>>,
    forwarded_event_actions: &mut BTreeMap<String, BTreeSet<String>>,
) {
    *handler_counts.entry(action_id.to_string()).or_default() += 1;
    collect_handler_requirements(
        action_id,
        actions,
        required_event_roles,
        forwarded_event_actions,
    );
}

fn collect_handler_requirements(
    action_id: &str,
    actions: &[ActionNodeV1],
    required_event_roles: &mut BTreeMap<String, BTreeSet<String>>,
    forwarded_event_actions: &mut BTreeMap<String, BTreeSet<String>>,
) {
    let roles = required_event_roles
        .entry(action_id.to_string())
        .or_default();
    let forwarded = forwarded_event_actions
        .entry(action_id.to_string())
        .or_default();
    for node in actions {
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

fn validate_creator_branch(
    actions: &[ActionNodeV1],
    path: &str,
    handler_counts: &BTreeMap<String, usize>,
    required_event_roles: &BTreeMap<String, BTreeSet<String>>,
    diagnostics: &mut Vec<StatefulSpecDiagnosticV1>,
) {
    let mut manifests = BTreeMap::new();
    let mut manifest_counts = BTreeMap::<&str, usize>::new();
    for node in actions {
        if let ActionV1::RegisterInstance {
            output, resources, ..
        } = &node.action
        {
            *manifest_counts.entry(output.as_str()).or_default() += 1;
            manifests.entry(output.as_str()).or_insert(resources);
        }
    }
    for (action_index, node) in actions.iter().enumerate() {
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
                        "created_instance_missing_cross_branch_handler_resource",
                        format!(
                            "{path}/{action_index}/action/buttons/{button_index}/route/instance"
                        ),
                        format!(
                            "the created instance must declare role alias {alias} required by either handler branch or a transitively forwarded instance action"
                        ),
                    );
                }
            }
        }
    }
}

fn register_workflow_identity<'a>(
    id: &'a str,
    trigger: &TriggerV1,
    path: &str,
    workflow_ids: &mut BTreeSet<&'a str>,
    trigger_ids: &mut BTreeSet<String>,
    diagnostics: &mut Vec<StatefulSpecDiagnosticV1>,
) {
    validate_identifier(diagnostics, &format!("{path}/id"), id);
    if !workflow_ids.insert(id) {
        push(
            diagnostics,
            "duplicate_workflow_id",
            format!("{path}/id"),
            "workflow IDs must be unique across stateless and stateful workflows",
        );
    }
    if !trigger_ids.insert(trigger_identity(trigger)) {
        push(
            diagnostics,
            "duplicate_trigger",
            format!("{path}/trigger"),
            "only one workflow may consume a trigger",
        );
    }
}

#[derive(Clone, Copy)]
struct InputContext<'a> {
    is_modal: bool,
    declared_inputs: Option<&'a BTreeSet<&'a str>>,
}

#[allow(clippy::too_many_arguments)]
fn validate_branch<'a>(
    branch: &'a StatefulBranchV1,
    path: &str,
    trigger: &TriggerV1,
    input_context: InputContext<'_>,
    variables: &BTreeMap<&str, &StateVariableV1>,
    node_ids: &mut BTreeSet<&'a str>,
    diagnostics: &mut Vec<StatefulSpecDiagnosticV1>,
) {
    let branch_nodes = branch
        .state_actions
        .len()
        .saturating_add(branch.effects.len())
        .saturating_add(1);
    if branch_nodes > MAX_NODES_PER_BRANCH_V1 {
        push(
            diagnostics,
            "branch_node_count_exceeded",
            path,
            format!("a branch must not exceed {MAX_NODES_PER_BRANCH_V1} authored nodes"),
        );
    }
    if branch.state_actions.len() > MAX_STATE_ACTIONS_PER_BRANCH_V1 {
        push(
            diagnostics,
            "branch_state_action_count_exceeded",
            format!("{path}/state_actions"),
            format!("a branch must not exceed {MAX_STATE_ACTIONS_PER_BRANCH_V1} state assignments"),
        );
    }
    let mut written_variables = BTreeSet::new();
    for (index, action) in branch.state_actions.iter().enumerate() {
        let action_path = format!("{path}/state_actions/{index}");
        register_node_id(
            &action.id,
            &format!("{action_path}/id"),
            node_ids,
            diagnostics,
        );
        validate_identifier(
            diagnostics,
            &format!("{action_path}/variable_id"),
            &action.variable_id,
        );
        if !written_variables.insert(action.variable_id.as_str()) {
            push(
                diagnostics,
                "duplicate_branch_state_write",
                format!("{action_path}/variable_id"),
                "parallel branch assignment may target each variable at most once",
            );
        }
        let variable = validate_state_reference(
            &action.variable_id,
            &format!("{action_path}/variable_id"),
            trigger,
            variables,
            diagnostics,
        );
        let mut expression_nodes = 0;
        let expression_type = infer_value_expr(
            &action.value,
            &format!("{action_path}/value"),
            1,
            &mut expression_nodes,
            trigger,
            input_context,
            variables,
            diagnostics,
        );
        if let (Some(variable), Some(expression_type)) = (variable, expression_type) {
            if variable.value_type.primitive_type() != expression_type {
                push(
                    diagnostics,
                    "state_set_type_mismatch",
                    format!("{action_path}/value"),
                    "state assignment expression type must match its target variable",
                );
            }
            if let StatefulValueExprV1::Literal { value } = &action.value {
                if !variable.value_type.accepts(value) {
                    push(
                        diagnostics,
                        "state_set_literal_out_of_bounds",
                        format!("{action_path}/value"),
                        "literal assignment must satisfy the target variable bounds",
                    );
                }
            }
        }
    }
    for (index, effect) in branch.effects.iter().enumerate() {
        let effect_path = format!("{path}/effects/{index}");
        register_node_id(
            &effect.id,
            &format!("{effect_path}/id"),
            node_ids,
            diagnostics,
        );
        if matches!(
            effect.action,
            ActionV1::RespondEphemeral { .. }
                | ActionV1::OpenModal { .. }
                | ActionV1::DeferEphemeral
                | ActionV1::EditResponse { .. }
        ) {
            push(
                diagnostics,
                "response_action_forbidden_in_effects",
                format!("{effect_path}/action"),
                "effects cannot contain response actions; stateful branches use an implicit defer and dedicated final response",
            );
        }
    }
    register_node_id(
        &branch.response.id,
        &format!("{path}/response/id"),
        node_ids,
        diagnostics,
    );
}

#[allow(clippy::too_many_arguments)]
fn validate_condition(
    condition: &StatefulConditionExprV1,
    path: &str,
    depth: usize,
    nodes: &mut usize,
    trigger: &TriggerV1,
    input_context: InputContext<'_>,
    variables: &BTreeMap<&str, &StateVariableV1>,
    diagnostics: &mut Vec<StatefulSpecDiagnosticV1>,
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
        StatefulConditionExprV1::Always => {}
        StatefulConditionExprV1::InputNonEmpty { input_id }
        | StatefulConditionExprV1::InputEquals { input_id, .. } => {
            validate_input_reference(input_id, path, input_context, diagnostics);
            if let StatefulConditionExprV1::InputEquals { value, .. } = condition {
                validate_text(value, &format!("{path}/value"), diagnostics);
            }
        }
        StatefulConditionExprV1::StateEquals { variable_id, value } => {
            let variable = validate_state_reference(
                variable_id,
                &format!("{path}/variable_id"),
                trigger,
                variables,
                diagnostics,
            );
            let mut expression_nodes = 0;
            let value_type = infer_value_expr(
                value,
                &format!("{path}/value"),
                1,
                &mut expression_nodes,
                trigger,
                input_context,
                variables,
                diagnostics,
            );
            if let (Some(variable), Some(value_type)) = (variable, value_type) {
                if variable.value_type.primitive_type() != value_type {
                    push(
                        diagnostics,
                        "state_equality_type_mismatch",
                        format!("{path}/value"),
                        "state equality operands must have the same primitive type",
                    );
                }
            }
        }
        StatefulConditionExprV1::IntegerCompare { left, right, .. } => {
            for (side, expression) in [("left", left), ("right", right)] {
                let mut expression_nodes = 0;
                if infer_value_expr(
                    expression,
                    &format!("{path}/{side}"),
                    1,
                    &mut expression_nodes,
                    trigger,
                    input_context,
                    variables,
                    diagnostics,
                )
                .is_some_and(|value_type| value_type != StatePrimitiveTypeV1::Integer)
                {
                    push(
                        diagnostics,
                        "integer_comparison_type_mismatch",
                        format!("{path}/{side}"),
                        "integer comparisons require integer operands",
                    );
                }
            }
        }
        StatefulConditionExprV1::All { conditions }
        | StatefulConditionExprV1::Any { conditions } => {
            if conditions.is_empty() {
                push(
                    diagnostics,
                    "empty_condition_group",
                    format!("{path}/conditions"),
                    "all and any groups must not be empty",
                );
            }
            for (index, child) in conditions.iter().enumerate() {
                validate_condition(
                    child,
                    &format!("{path}/conditions/{index}"),
                    depth + 1,
                    nodes,
                    trigger,
                    input_context,
                    variables,
                    diagnostics,
                );
            }
        }
        StatefulConditionExprV1::Not { condition } => validate_condition(
            condition,
            &format!("{path}/condition"),
            depth + 1,
            nodes,
            trigger,
            input_context,
            variables,
            diagnostics,
        ),
    }
}

#[allow(clippy::too_many_arguments)]
fn infer_value_expr(
    expression: &StatefulValueExprV1,
    path: &str,
    depth: usize,
    nodes: &mut usize,
    trigger: &TriggerV1,
    input_context: InputContext<'_>,
    variables: &BTreeMap<&str, &StateVariableV1>,
    diagnostics: &mut Vec<StatefulSpecDiagnosticV1>,
) -> Option<StatePrimitiveTypeV1> {
    *nodes = nodes.saturating_add(1);
    if depth > MAX_VALUE_EXPR_DEPTH_V1 {
        push(
            diagnostics,
            "value_expression_depth_exceeded",
            path,
            format!("value expression depth must not exceed {MAX_VALUE_EXPR_DEPTH_V1}"),
        );
        return None;
    }
    if *nodes > MAX_VALUE_EXPR_NODES_V1 {
        push(
            diagnostics,
            "value_expression_node_budget_exceeded",
            path,
            format!("a value expression must not exceed {MAX_VALUE_EXPR_NODES_V1} nodes"),
        );
        return None;
    }
    match expression {
        StatefulValueExprV1::Literal { value } => {
            validate_state_value(value, &format!("{path}/value"), diagnostics);
            Some(value.primitive_type())
        }
        StatefulValueExprV1::InputText { input_id } => {
            validate_input_reference(input_id, path, input_context, diagnostics);
            Some(StatePrimitiveTypeV1::Text)
        }
        StatefulValueExprV1::State { variable_id } => validate_state_reference(
            variable_id,
            &format!("{path}/variable_id"),
            trigger,
            variables,
            diagnostics,
        )
        .map(|variable| variable.value_type.primitive_type()),
        StatefulValueExprV1::CheckedAdd { left, right }
        | StatefulValueExprV1::CheckedSub { left, right } => {
            for (side, child) in [("left", left.as_ref()), ("right", right.as_ref())] {
                if infer_value_expr(
                    child,
                    &format!("{path}/{side}"),
                    depth + 1,
                    nodes,
                    trigger,
                    input_context,
                    variables,
                    diagnostics,
                )
                .is_some_and(|value_type| value_type != StatePrimitiveTypeV1::Integer)
                {
                    push(
                        diagnostics,
                        "checked_arithmetic_type_mismatch",
                        format!("{path}/{side}"),
                        "checked arithmetic requires integer operands",
                    );
                }
            }
            if let (Some(left), Some(right)) =
                (constant_integer_value(left), constant_integer_value(right))
            {
                let result = if matches!(expression, StatefulValueExprV1::CheckedAdd { .. }) {
                    left.checked_add(right)
                } else {
                    left.checked_sub(right)
                };
                if result.is_none_or(|value| {
                    !(-MAX_SAFE_INTEGER_V1..=MAX_SAFE_INTEGER_V1).contains(&value)
                }) {
                    push(
                        diagnostics,
                        "constant_checked_arithmetic_overflow",
                        path,
                        "constant checked arithmetic must stay within the JavaScript-safe integer range",
                    );
                }
            }
            Some(StatePrimitiveTypeV1::Integer)
        }
    }
}

fn constant_integer_value(expression: &StatefulValueExprV1) -> Option<i64> {
    match expression {
        StatefulValueExprV1::Literal {
            value: StateValueV1::Integer { value },
        } => Some(*value),
        StatefulValueExprV1::CheckedAdd { left, right } => {
            constant_integer_value(left)?.checked_add(constant_integer_value(right)?)
        }
        StatefulValueExprV1::CheckedSub { left, right } => {
            constant_integer_value(left)?.checked_sub(constant_integer_value(right)?)
        }
        StatefulValueExprV1::Literal { .. }
        | StatefulValueExprV1::InputText { .. }
        | StatefulValueExprV1::State { .. } => None,
    }
}

fn validate_state_reference<'a>(
    variable_id: &str,
    path: &str,
    trigger: &TriggerV1,
    variables: &BTreeMap<&'a str, &'a StateVariableV1>,
    diagnostics: &mut Vec<StatefulSpecDiagnosticV1>,
) -> Option<&'a StateVariableV1> {
    validate_identifier(diagnostics, path, variable_id);
    let Some(variable) = variables.get(variable_id).copied() else {
        push(
            diagnostics,
            "unknown_state_variable",
            path,
            "state references must name a declared variable",
        );
        return None;
    };
    if matches!(
        variable.scope,
        StateScopeV1::Instance | StateScopeV1::ActorInstance
    ) && !matches!(trigger, TriggerV1::InstanceAction { .. })
    {
        push(
            diagnostics,
            "instance_scope_unavailable",
            path,
            "instance and actor_instance state is available only to instance_action workflows",
        );
    }
    Some(variable)
}

fn validate_input_reference(
    input_id: &str,
    path: &str,
    context: InputContext<'_>,
    diagnostics: &mut Vec<StatefulSpecDiagnosticV1>,
) {
    validate_identifier(diagnostics, &format!("{path}/input_id"), input_id);
    if !context.is_modal {
        push(
            diagnostics,
            "input_reference_requires_modal_trigger",
            path,
            "input references are available only to modal_submit workflows",
        );
    } else if context
        .declared_inputs
        .is_some_and(|fields| !fields.contains(input_id))
    {
        push(
            diagnostics,
            "unknown_input_reference",
            format!("{path}/input_id"),
            "input_id must name a field declared by the trigger modal",
        );
    }
}

fn validate_state_value(
    value: &StateValueV1,
    path: &str,
    diagnostics: &mut Vec<StatefulSpecDiagnosticV1>,
) {
    match value {
        StateValueV1::Integer { value }
            if !(-MAX_SAFE_INTEGER_V1..=MAX_SAFE_INTEGER_V1).contains(value) =>
        {
            push(
                diagnostics,
                "integer_value_not_js_safe",
                format!("{path}/value"),
                "integer values must be within the JavaScript-safe integer range",
            );
        }
        StateValueV1::Text { value } => validate_text(value, &format!("{path}/value"), diagnostics),
        StateValueV1::Bool { .. } | StateValueV1::Integer { .. } => {}
    }
}

fn validate_text(value: &str, path: &str, diagnostics: &mut Vec<StatefulSpecDiagnosticV1>) {
    if value.contains('\0') {
        push(
            diagnostics,
            "state_text_contains_nul",
            path,
            "state text must not contain U+0000",
        );
    }
    if value.len() > MAX_STATE_TEXT_BYTES_V1
        || value.encode_utf16().count() > MAX_STATE_TEXT_UTF16_UNITS_V1
    {
        push(
            diagnostics,
            "state_text_too_large",
            path,
            "state text must not exceed 4000 UTF-8 bytes or UTF-16 code units",
        );
    }
}

fn validate_runtime_shape_views(
    spec: &StatefulSpecV1,
    diagnostics: &mut Vec<StatefulSpecDiagnosticV1>,
) {
    for (branch_name, selected) in [("true", BranchViewV1::True), ("false", BranchViewV1::False)] {
        let view = automation_spec_validation_view_v1(spec, selected);
        if let Err(error) = validate_automation_spec_v1(&view) {
            for diagnostic in error.diagnostics() {
                push(
                    diagnostics,
                    format!("runtime_shape_{}", diagnostic.code),
                    format!("/{branch_name}_branch_runtime_shape{}", diagnostic.path),
                    diagnostic.message.clone(),
                );
            }
        }
    }
}

fn register_node_id<'a>(
    id: &'a str,
    path: &str,
    node_ids: &mut BTreeSet<&'a str>,
    diagnostics: &mut Vec<StatefulSpecDiagnosticV1>,
) {
    validate_identifier(diagnostics, path, id);
    if !node_ids.insert(id) {
        push(
            diagnostics,
            "duplicate_node_id",
            path,
            "node IDs must be unique across stateless and stateful workflows",
        );
    }
}

fn trigger_identity(trigger: &TriggerV1) -> String {
    match trigger {
        TriggerV1::ButtonClick { trigger_id } => format!("button:{trigger_id}"),
        TriggerV1::ModalSubmit { modal_id } => format!("modal:{modal_id}"),
        TriggerV1::InstanceAction { action_id } => format!("instance:{action_id}"),
    }
}

fn validate_identifier(diagnostics: &mut Vec<StatefulSpecDiagnosticV1>, path: &str, value: &str) {
    if value.is_empty()
        || value.len() > MAX_IDENTIFIER_BYTES_V1
        || !value
            .bytes()
            .next()
            .is_some_and(|byte| byte.is_ascii_lowercase())
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_')
    {
        push(
            diagnostics,
            "invalid_identifier",
            path,
            "identifiers must match [a-z][a-z0-9_]{0,63}",
        );
    }
}

pub(crate) fn push(
    diagnostics: &mut Vec<StatefulSpecDiagnosticV1>,
    code: impl Into<String>,
    path: impl Into<String>,
    message: impl Into<String>,
) {
    diagnostics.push(StatefulSpecDiagnosticV1 {
        code: code.into(),
        path: path.into(),
        message: message.into(),
    });
}
