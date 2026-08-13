//! Shared pure stateful expression/branch evaluator.
//!
//! This core has no simulation transport or canonical fixture size limit. Callers must supply an
//! already validated spec, already normalized inputs, and one exact in-bounds value per declared
//! state variable. Both preview simulation and the runtime proof boundary use this same function.

use std::collections::BTreeMap;

use automation_spec::{
    TriggerV1, MAX_SIMULATION_INPUTS_V1, MAX_SIMULATION_INPUT_BYTES_V1,
    MAX_SIMULATION_PAYLOAD_BYTES_V1,
};
use zeroize::Zeroize;

use crate::model::{
    IntegerComparisonV1, StateValueV1, StatefulBranchV1, StatefulConditionExprV1, StatefulSpecV1,
    StatefulValueExprV1, MAX_SAFE_INTEGER_V1,
};
use crate::validate::{
    validate_stateful_spec_v1, MAX_STATE_TEXT_BYTES_V1, MAX_STATE_TEXT_UTF16_UNITS_V1,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StatefulCoreBranchSelectionV1 {
    True,
    False,
}

#[derive(Clone, PartialEq, Eq)]
pub struct StatefulCoreTransitionV1 {
    node_id: String,
    variable_id: String,
    before: StateValueV1,
    after: StateValueV1,
}

impl StatefulCoreTransitionV1 {
    pub fn node_id(&self) -> &str {
        &self.node_id
    }

    pub fn variable_id(&self) -> &str {
        &self.variable_id
    }

    pub fn before(&self) -> &StateValueV1 {
        &self.before
    }

    pub fn after(&self) -> &StateValueV1 {
        &self.after
    }
}

impl Drop for StatefulCoreTransitionV1 {
    fn drop(&mut self) {
        zeroize_value(&mut self.before);
        zeroize_value(&mut self.after);
    }
}

/// No `Debug` or serde: normalized inputs and state may contain private text.
#[derive(Clone, PartialEq, Eq)]
pub struct StatefulCoreEvaluationV1 {
    workflow_id: String,
    condition_result: bool,
    branch: StatefulCoreBranchSelectionV1,
    state_after: BTreeMap<String, StateValueV1>,
    transitions: Vec<StatefulCoreTransitionV1>,
    external_node_ids: Vec<String>,
}

impl StatefulCoreEvaluationV1 {
    pub fn workflow_id(&self) -> &str {
        &self.workflow_id
    }

    pub fn condition_result(&self) -> bool {
        self.condition_result
    }

    pub fn branch(&self) -> StatefulCoreBranchSelectionV1 {
        self.branch
    }

    pub fn state_after(&self) -> &BTreeMap<String, StateValueV1> {
        &self.state_after
    }

    pub fn transitions(&self) -> &[StatefulCoreTransitionV1] {
        &self.transitions
    }

    pub fn external_node_ids(&self) -> &[String] {
        &self.external_node_ids
    }
}

impl Drop for StatefulCoreEvaluationV1 {
    fn drop(&mut self) {
        for value in self.state_after.values_mut() {
            zeroize_value(value);
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum StatefulCoreEvaluationErrorV1 {
    #[error("stateful spec is invalid")]
    InvalidSpec,
    #[error("stateful trigger does not select exactly one stateful workflow")]
    WorkflowMismatch,
    #[error("normalized event inputs are invalid")]
    InvalidInputs,
    #[error("resolved state is incomplete or out of bounds")]
    InvalidState,
    #[error("stateful expression evaluation failed: {code}")]
    Evaluation {
        code: &'static str,
        node_id: Option<String>,
    },
}

pub fn evaluate_validated_stateful_workflow_v1(
    spec: &StatefulSpecV1,
    trigger: &TriggerV1,
    normalized_inputs: &BTreeMap<String, String>,
    state_before: &BTreeMap<String, StateValueV1>,
) -> Result<StatefulCoreEvaluationV1, StatefulCoreEvaluationErrorV1> {
    validate_stateful_spec_v1(spec).map_err(|_| StatefulCoreEvaluationErrorV1::InvalidSpec)?;
    validate_inputs(normalized_inputs)?;
    validate_state(spec, state_before)?;
    let mut workflows = spec
        .stateful_workflows
        .iter()
        .filter(|workflow| workflow.trigger == *trigger);
    let workflow = workflows
        .next()
        .ok_or(StatefulCoreEvaluationErrorV1::WorkflowMismatch)?;
    if workflows.next().is_some() {
        return Err(StatefulCoreEvaluationErrorV1::WorkflowMismatch);
    }
    let condition_result =
        evaluate_condition(&workflow.condition, normalized_inputs, state_before)?;
    let (selection, branch) = if condition_result {
        (StatefulCoreBranchSelectionV1::True, &workflow.on_true)
    } else {
        (StatefulCoreBranchSelectionV1::False, &workflow.on_false)
    };
    let declarations = spec
        .state_variables
        .iter()
        .map(|variable| (variable.id.as_str(), variable))
        .collect::<BTreeMap<_, _>>();
    let execution =
        execute_parallel_branch(branch, normalized_inputs, state_before, &declarations)?;
    Ok(StatefulCoreEvaluationV1 {
        workflow_id: workflow.id.clone(),
        condition_result,
        branch: selection,
        state_after: execution.state_after,
        transitions: execution.transitions,
        external_node_ids: execution.external_node_ids,
    })
}

fn validate_inputs(inputs: &BTreeMap<String, String>) -> Result<(), StatefulCoreEvaluationErrorV1> {
    if inputs.len() > MAX_SIMULATION_INPUTS_V1 {
        return Err(StatefulCoreEvaluationErrorV1::InvalidInputs);
    }
    let payload_bytes = inputs.iter().try_fold(0usize, |total, (key, value)| {
        if key.is_empty()
            || key.len() > 64
            || value.len() > MAX_SIMULATION_INPUT_BYTES_V1
            || value.encode_utf16().count() > MAX_SIMULATION_INPUT_BYTES_V1
        {
            return Err(StatefulCoreEvaluationErrorV1::InvalidInputs);
        }
        if value.contains('\0') {
            return Err(evaluation("input_text_contains_nul", None));
        }
        Ok(total.saturating_add(key.len()).saturating_add(value.len()))
    })?;
    if payload_bytes > MAX_SIMULATION_PAYLOAD_BYTES_V1 {
        return Err(StatefulCoreEvaluationErrorV1::InvalidInputs);
    }
    Ok(())
}

fn validate_state(
    spec: &StatefulSpecV1,
    state: &BTreeMap<String, StateValueV1>,
) -> Result<(), StatefulCoreEvaluationErrorV1> {
    if state.len() != spec.state_variables.len() {
        return Err(StatefulCoreEvaluationErrorV1::InvalidState);
    }
    for variable in &spec.state_variables {
        let value = state
            .get(&variable.id)
            .ok_or(StatefulCoreEvaluationErrorV1::InvalidState)?;
        if !variable.value_type.accepts(value) {
            return Err(StatefulCoreEvaluationErrorV1::InvalidState);
        }
        if let StateValueV1::Text { value } = value {
            if value.contains('\0')
                || value.len() > MAX_STATE_TEXT_BYTES_V1
                || value.encode_utf16().count() > MAX_STATE_TEXT_UTF16_UNITS_V1
            {
                return Err(StatefulCoreEvaluationErrorV1::InvalidState);
            }
        }
    }
    Ok(())
}

fn evaluate_condition(
    condition: &StatefulConditionExprV1,
    inputs: &BTreeMap<String, String>,
    state: &BTreeMap<String, StateValueV1>,
) -> Result<bool, StatefulCoreEvaluationErrorV1> {
    match condition {
        StatefulConditionExprV1::Always => Ok(true),
        StatefulConditionExprV1::InputNonEmpty { input_id } => {
            Ok(inputs.get(input_id).is_some_and(|value| !value.is_empty()))
        }
        StatefulConditionExprV1::InputEquals { input_id, value } => {
            Ok(inputs.get(input_id) == Some(value))
        }
        StatefulConditionExprV1::StateEquals { variable_id, value } => {
            Ok(state.get(variable_id) == Some(&evaluate_value(value, inputs, state, None)?))
        }
        StatefulConditionExprV1::IntegerCompare {
            left,
            operator,
            right,
        } => {
            let StateValueV1::Integer { value: left } = evaluate_value(left, inputs, state, None)?
            else {
                return Err(evaluation("invalid_integer_comparison_operand", None));
            };
            let StateValueV1::Integer { value: right } =
                evaluate_value(right, inputs, state, None)?
            else {
                return Err(evaluation("invalid_integer_comparison_operand", None));
            };
            Ok(match operator {
                IntegerComparisonV1::Equal => left == right,
                IntegerComparisonV1::NotEqual => left != right,
                IntegerComparisonV1::LessThan => left < right,
                IntegerComparisonV1::LessThanOrEqual => left <= right,
                IntegerComparisonV1::GreaterThan => left > right,
                IntegerComparisonV1::GreaterThanOrEqual => left >= right,
            })
        }
        StatefulConditionExprV1::All { conditions } => {
            for condition in conditions {
                if !evaluate_condition(condition, inputs, state)? {
                    return Ok(false);
                }
            }
            Ok(true)
        }
        StatefulConditionExprV1::Any { conditions } => {
            for condition in conditions {
                if evaluate_condition(condition, inputs, state)? {
                    return Ok(true);
                }
            }
            Ok(false)
        }
        StatefulConditionExprV1::Not { condition } => {
            Ok(!evaluate_condition(condition, inputs, state)?)
        }
    }
}

struct BranchExecutionV1 {
    state_after: BTreeMap<String, StateValueV1>,
    transitions: Vec<StatefulCoreTransitionV1>,
    external_node_ids: Vec<String>,
}

fn execute_parallel_branch(
    branch: &StatefulBranchV1,
    inputs: &BTreeMap<String, String>,
    state_before: &BTreeMap<String, StateValueV1>,
    declarations: &BTreeMap<&str, &crate::model::StateVariableV1>,
) -> Result<BranchExecutionV1, StatefulCoreEvaluationErrorV1> {
    let mut pending = Vec::with_capacity(branch.state_actions.len());
    for action in &branch.state_actions {
        let before = state_before
            .get(&action.variable_id)
            .cloned()
            .ok_or(StatefulCoreEvaluationErrorV1::InvalidState)?;
        // Every RHS intentionally reads the same immutable pre-state.
        let after = evaluate_value(&action.value, inputs, state_before, Some(&action.id))?;
        let declaration = declarations
            .get(action.variable_id.as_str())
            .ok_or(StatefulCoreEvaluationErrorV1::InvalidState)?;
        if !declaration.value_type.accepts(&after) {
            return Err(evaluation("state_value_out_of_bounds", Some(&action.id)));
        }
        pending.push(StatefulCoreTransitionV1 {
            node_id: action.id.clone(),
            variable_id: action.variable_id.clone(),
            before,
            after,
        });
    }
    let mut state_after = state_before.clone();
    for transition in &pending {
        state_after.insert(transition.variable_id.clone(), transition.after.clone());
    }
    let mut external_node_ids = branch
        .effects
        .iter()
        .map(|effect| effect.id.clone())
        .collect::<Vec<_>>();
    external_node_ids.push(branch.response.id.clone());
    Ok(BranchExecutionV1 {
        state_after,
        transitions: pending,
        external_node_ids,
    })
}

fn evaluate_value(
    expression: &StatefulValueExprV1,
    inputs: &BTreeMap<String, String>,
    state: &BTreeMap<String, StateValueV1>,
    node_id: Option<&str>,
) -> Result<StateValueV1, StatefulCoreEvaluationErrorV1> {
    match expression {
        StatefulValueExprV1::Literal { value } => Ok(value.clone()),
        StatefulValueExprV1::InputText { input_id } => {
            let value = inputs.get(input_id).cloned().unwrap_or_default();
            if value.contains('\0') {
                return Err(evaluation("input_text_contains_nul", node_id));
            }
            Ok(StateValueV1::Text { value })
        }
        StatefulValueExprV1::State { variable_id } => state
            .get(variable_id)
            .cloned()
            .ok_or(StatefulCoreEvaluationErrorV1::InvalidState),
        StatefulValueExprV1::CheckedAdd { left, right }
        | StatefulValueExprV1::CheckedSub { left, right } => {
            let StateValueV1::Integer { value: left_value } =
                evaluate_value(left, inputs, state, node_id)?
            else {
                return Err(evaluation("checked_arithmetic_type_mismatch", node_id));
            };
            let StateValueV1::Integer { value: right_value } =
                evaluate_value(right, inputs, state, node_id)?
            else {
                return Err(evaluation("checked_arithmetic_type_mismatch", node_id));
            };
            let value = if matches!(expression, StatefulValueExprV1::CheckedAdd { .. }) {
                left_value.checked_add(right_value)
            } else {
                left_value.checked_sub(right_value)
            }
            .filter(|value| (-MAX_SAFE_INTEGER_V1..=MAX_SAFE_INTEGER_V1).contains(value))
            .ok_or_else(|| evaluation("integer_overflow", node_id))?;
            Ok(StateValueV1::Integer { value })
        }
    }
}

fn evaluation(code: &'static str, node_id: Option<&str>) -> StatefulCoreEvaluationErrorV1 {
    StatefulCoreEvaluationErrorV1::Evaluation {
        code,
        node_id: node_id.map(str::to_string),
    }
}

fn zeroize_value(value: &mut StateValueV1) {
    if let StateValueV1::Text { value } = value {
        value.zeroize();
    }
}
