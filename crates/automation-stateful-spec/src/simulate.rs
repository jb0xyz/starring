use std::collections::BTreeMap;

use automation_spec::{
    simulate_automation_spec_v1, AutomationSimulationErrorV1, AutomationSimulationEventV1,
    AutomationSimulationOutcomeV1, TriggerV1, MAX_SIMULATION_INPUTS_V1,
    MAX_SIMULATION_INPUT_BYTES_V1, MAX_SIMULATION_PAYLOAD_BYTES_V1,
};
use serde::{Deserialize, Serialize};

use crate::canonical::{
    stateful_simulation_trace_digest_v1, stateful_spec_digest_v1,
    StatefulSimulationTraceDigestErrorV1, StatefulSimulationTraceDigestV1,
    StatefulSpecDigestErrorV1, StatefulSpecDigestV1,
};
use crate::evaluate::{evaluate_validated_stateful_workflow_v1, StatefulCoreBranchSelectionV1};
use crate::model::{StateValueV1, StatefulSpecV1};
use crate::validate::{
    push, validate_stateful_spec_v1, StatefulSpecDiagnosticV1, StatefulSpecValidationErrorV1,
    MAX_STATE_TEXT_BYTES_V1, MAX_STATE_TEXT_UTF16_UNITS_V1,
};
use crate::view::{automation_spec_validation_view_v1, BranchViewV1};

pub const STATEFUL_SIMULATION_TRACE_SCHEMA_VERSION_V1: u16 = 1;
pub const STATEFUL_SIMULATION_TRACE_KIND_V1: &str = "starring.stateful-simulation-trace.v1";
pub const MAX_STATEFUL_SIMULATION_CELLS_V1: usize = 64;
pub const MAX_STATEFUL_SIMULATION_FIXTURE_CANONICAL_BYTES_V1: usize = 64 * 1_024;
pub const MAX_STATEFUL_SIMULATION_TOTAL_CANONICAL_BYTES_V1: usize = 128 * 1_024;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StatefulSimulationEventV1 {
    pub trigger: TriggerV1,
    #[serde(default)]
    pub inputs: BTreeMap<String, String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StatefulSimulationInputV1 {
    pub event: StatefulSimulationEventV1,
    #[serde(default)]
    pub state: Vec<StateSimulationCellV1>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StateSimulationCellV1 {
    pub variable_id: String,
    pub value: StateValueV1,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StatefulSimulationOutcomeV1 {
    NoTriggerMatch,
    StatelessConditionNotSatisfied,
    StatelessActionsPlanned,
    StatefulBranchPlanned,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StatefulSimulationWorkflowKindV1 {
    Stateless,
    Stateful,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StatefulBranchSelectionV1 {
    True,
    False,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StateTransitionV1 {
    pub node_id: String,
    pub variable_id: String,
    pub before: StateValueV1,
    pub after: StateValueV1,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StatefulSimulationTraceV1 {
    pub schema_version: u16,
    pub kind: String,
    pub spec_digest: StatefulSpecDigestV1,
    pub event_trigger: TriggerV1,
    pub outcome: StatefulSimulationOutcomeV1,
    pub workflow_id: Option<String>,
    pub workflow_kind: Option<StatefulSimulationWorkflowKindV1>,
    pub condition_result: Option<bool>,
    pub branch: Option<StatefulBranchSelectionV1>,
    pub normalized_inputs: BTreeMap<String, String>,
    pub state_before: BTreeMap<String, StateValueV1>,
    pub state_after: BTreeMap<String, StateValueV1>,
    pub state_transitions: Vec<StateTransitionV1>,
    pub external_node_ids: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StatefulSimulationResultV1 {
    pub spec_digest: StatefulSpecDigestV1,
    pub trace: StatefulSimulationTraceV1,
    pub trace_digest: StatefulSimulationTraceDigestV1,
}

#[derive(Debug, thiserror::Error)]
pub enum StatefulSimulationErrorV1 {
    #[error("stateful spec is invalid")]
    InvalidSpec(#[from] StatefulSpecValidationErrorV1),
    #[error("stateful simulation input is invalid")]
    InvalidInput {
        diagnostics: Vec<StatefulSpecDiagnosticV1>,
    },
    #[error("stateful simulation evaluation failed: {code}")]
    Evaluation {
        code: &'static str,
        node_id: Option<String>,
    },
    #[error("stateful spec identity could not be computed")]
    SpecIdentity(#[from] StatefulSpecDigestErrorV1),
    #[error("stateful simulation trace identity could not be computed")]
    TraceIdentity(#[from] StatefulSimulationTraceDigestErrorV1),
    #[error("validated stateful spec produced an invalid private shape-validation view")]
    InvalidShapeView,
}

impl StatefulSimulationErrorV1 {
    pub fn diagnostics(&self) -> &[StatefulSpecDiagnosticV1] {
        match self {
            Self::InvalidSpec(error) => error.diagnostics(),
            Self::InvalidInput { diagnostics } => diagnostics,
            Self::Evaluation { .. }
            | Self::SpecIdentity(_)
            | Self::TraceIdentity(_)
            | Self::InvalidShapeView => &[],
        }
    }
}

pub fn simulate_stateful_spec_v1(
    spec: &StatefulSpecV1,
    input: &StatefulSimulationInputV1,
) -> Result<StatefulSimulationResultV1, StatefulSimulationErrorV1> {
    validate_stateful_spec_v1(spec)?;
    validate_simulation_size(spec, input)?;
    let state_before = resolve_simulation_state(spec, &input.state)?;
    let spec_digest = stateful_spec_digest_v1(spec)?;
    let shape_event = AutomationSimulationEventV1 {
        trigger: input.event.trigger.clone(),
        inputs: input.event.inputs.clone(),
    };
    let shape_view = automation_spec_validation_view_v1(spec, BranchViewV1::True);
    let normalized = match simulate_automation_spec_v1(&shape_view, &shape_event) {
        Ok(trace) => trace,
        Err(AutomationSimulationErrorV1::InvalidEvent { diagnostics }) => {
            return Err(StatefulSimulationErrorV1::InvalidInput {
                diagnostics: diagnostics
                    .into_iter()
                    .map(|diagnostic| StatefulSpecDiagnosticV1 {
                        code: format!("event_{}", diagnostic.code),
                        path: diagnostic.path,
                        message: diagnostic.message,
                    })
                    .collect(),
            });
        }
        Err(AutomationSimulationErrorV1::InvalidSpec(_)) => {
            return Err(StatefulSimulationErrorV1::InvalidShapeView);
        }
    };

    if normalized.outcome == AutomationSimulationOutcomeV1::NoTriggerMatch {
        return finish(
            spec_digest,
            base_trace(
                spec_digest,
                &input.event.trigger,
                StatefulSimulationOutcomeV1::NoTriggerMatch,
                normalized.normalized_inputs,
                state_before,
            ),
        );
    }

    if let Some(workflow) = spec
        .stateless_workflows
        .iter()
        .find(|workflow| workflow.trigger == input.event.trigger)
    {
        let actions_planned = normalized.outcome == AutomationSimulationOutcomeV1::ActionsPlanned;
        let mut trace = base_trace(
            spec_digest,
            &input.event.trigger,
            if actions_planned {
                StatefulSimulationOutcomeV1::StatelessActionsPlanned
            } else {
                StatefulSimulationOutcomeV1::StatelessConditionNotSatisfied
            },
            normalized.normalized_inputs,
            state_before,
        );
        trace.workflow_id = Some(workflow.id.clone());
        trace.workflow_kind = Some(StatefulSimulationWorkflowKindV1::Stateless);
        trace.condition_result = normalized.condition_result;
        trace.external_node_ids = normalized.action_node_ids;
        return finish(spec_digest, trace);
    }

    let execution = evaluate_validated_stateful_workflow_v1(
        spec,
        &input.event.trigger,
        &normalized.normalized_inputs,
        &state_before,
    )
    .map_err(|error| match error {
        crate::StatefulCoreEvaluationErrorV1::Evaluation { code, node_id } => {
            StatefulSimulationErrorV1::Evaluation { code, node_id }
        }
        _ => evaluation("shared_stateful_core_failed", None),
    })?;
    let selection = match execution.branch() {
        StatefulCoreBranchSelectionV1::True => StatefulBranchSelectionV1::True,
        StatefulCoreBranchSelectionV1::False => StatefulBranchSelectionV1::False,
    };
    let trace = StatefulSimulationTraceV1 {
        schema_version: STATEFUL_SIMULATION_TRACE_SCHEMA_VERSION_V1,
        kind: STATEFUL_SIMULATION_TRACE_KIND_V1.to_string(),
        spec_digest,
        event_trigger: input.event.trigger.clone(),
        outcome: StatefulSimulationOutcomeV1::StatefulBranchPlanned,
        workflow_id: Some(execution.workflow_id().to_string()),
        workflow_kind: Some(StatefulSimulationWorkflowKindV1::Stateful),
        condition_result: Some(execution.condition_result()),
        branch: Some(selection),
        normalized_inputs: normalized.normalized_inputs,
        state_before,
        state_after: execution.state_after().clone(),
        state_transitions: execution
            .transitions()
            .iter()
            .map(|transition| StateTransitionV1 {
                node_id: transition.node_id().to_string(),
                variable_id: transition.variable_id().to_string(),
                before: transition.before().clone(),
                after: transition.after().clone(),
            })
            .collect(),
        external_node_ids: execution.external_node_ids().to_vec(),
    };
    finish(spec_digest, trace)
}

fn validate_simulation_size(
    spec: &StatefulSpecV1,
    input: &StatefulSimulationInputV1,
) -> Result<(), StatefulSimulationErrorV1> {
    let mut diagnostics = cheap_simulation_shape_diagnostics(input);
    if !diagnostics.is_empty() {
        diagnostics
            .sort_by(|left, right| left.path.cmp(&right.path).then(left.code.cmp(&right.code)));
        return Err(StatefulSimulationErrorV1::InvalidInput { diagnostics });
    }
    let fixture_bytes =
        serde_json::to_vec(input).map_err(|_| StatefulSimulationErrorV1::InvalidInput {
            diagnostics: vec![StatefulSpecDiagnosticV1 {
                code: "simulation_fixture_encoding_failed".to_string(),
                path: "/".to_string(),
                message: "the simulation fixture could not be encoded".to_string(),
            }],
        })?;
    let spec_bytes =
        serde_json::to_vec(spec).map_err(|_| StatefulSimulationErrorV1::InvalidInput {
            diagnostics: vec![StatefulSpecDiagnosticV1 {
                code: "simulation_spec_encoding_failed".to_string(),
                path: "/".to_string(),
                message: "the simulation spec could not be encoded".to_string(),
            }],
        })?;
    let mut diagnostics = Vec::new();
    if fixture_bytes.len() > MAX_STATEFUL_SIMULATION_FIXTURE_CANONICAL_BYTES_V1 {
        push(
            &mut diagnostics,
            "simulation_fixture_too_large",
            "/",
            format!(
                "the simulation fixture must not exceed {MAX_STATEFUL_SIMULATION_FIXTURE_CANONICAL_BYTES_V1} encoded bytes"
            ),
        );
    }
    if spec_bytes.len().saturating_add(fixture_bytes.len())
        > MAX_STATEFUL_SIMULATION_TOTAL_CANONICAL_BYTES_V1
    {
        push(
            &mut diagnostics,
            "simulation_total_size_exceeded",
            "/",
            format!(
                "the spec and simulation fixture together must not exceed {MAX_STATEFUL_SIMULATION_TOTAL_CANONICAL_BYTES_V1} encoded bytes"
            ),
        );
    }
    if diagnostics.is_empty() {
        Ok(())
    } else {
        Err(StatefulSimulationErrorV1::InvalidInput { diagnostics })
    }
}

fn cheap_simulation_shape_diagnostics(
    input: &StatefulSimulationInputV1,
) -> Vec<StatefulSpecDiagnosticV1> {
    let mut diagnostics = Vec::new();
    if input.state.len() > MAX_STATEFUL_SIMULATION_CELLS_V1 {
        push(
            &mut diagnostics,
            "simulation_state_cell_count_exceeded",
            "/state",
            format!(
                "simulation fixtures support at most {MAX_STATEFUL_SIMULATION_CELLS_V1} state cells"
            ),
        );
        return diagnostics;
    }
    if input.event.inputs.len() > MAX_SIMULATION_INPUTS_V1 {
        push(
            &mut diagnostics,
            "simulation_input_count_exceeded",
            "/event/inputs",
            format!("simulation events support at most {MAX_SIMULATION_INPUTS_V1} inputs"),
        );
        return diagnostics;
    }
    let trigger_bytes = match &input.event.trigger {
        TriggerV1::ButtonClick { trigger_id } => trigger_id.len(),
        TriggerV1::ModalSubmit { modal_id } => modal_id.len(),
        TriggerV1::InstanceAction { action_id } => action_id.len(),
    };
    let mut payload_bytes = trigger_bytes;
    for (input_id, value) in &input.event.inputs {
        payload_bytes = payload_bytes
            .saturating_add(input_id.len())
            .saturating_add(value.len());
        if input_id.len() > 64
            || value.len() > MAX_SIMULATION_INPUT_BYTES_V1
            || value.encode_utf16().count() > MAX_SIMULATION_INPUT_BYTES_V1
        {
            push(
                &mut diagnostics,
                "simulation_input_prebound_exceeded",
                format!("/event/inputs/{input_id}"),
                "simulation input keys and values must satisfy the bounded event surface",
            );
        }
    }
    if payload_bytes > MAX_SIMULATION_PAYLOAD_BYTES_V1 {
        push(
            &mut diagnostics,
            "simulation_payload_prebound_exceeded",
            "/event",
            "simulation input payload must not exceed 20000 UTF-8 bytes",
        );
    }
    for (index, cell) in input.state.iter().enumerate() {
        let value_oversized = match &cell.value {
            StateValueV1::Text { value } => {
                value.len() > MAX_STATE_TEXT_BYTES_V1
                    || value.encode_utf16().count() > MAX_STATE_TEXT_UTF16_UNITS_V1
            }
            StateValueV1::Bool { .. } | StateValueV1::Integer { .. } => false,
        };
        if cell.variable_id.len() > 64 || value_oversized {
            push(
                &mut diagnostics,
                "simulation_state_cell_prebound_exceeded",
                format!("/state/{index}"),
                "simulation state cell identifiers and values must satisfy the bounded state surface",
            );
        }
    }
    diagnostics
}

fn base_trace(
    spec_digest: StatefulSpecDigestV1,
    event_trigger: &TriggerV1,
    outcome: StatefulSimulationOutcomeV1,
    normalized_inputs: BTreeMap<String, String>,
    state: BTreeMap<String, StateValueV1>,
) -> StatefulSimulationTraceV1 {
    StatefulSimulationTraceV1 {
        schema_version: STATEFUL_SIMULATION_TRACE_SCHEMA_VERSION_V1,
        kind: STATEFUL_SIMULATION_TRACE_KIND_V1.to_string(),
        spec_digest,
        event_trigger: event_trigger.clone(),
        outcome,
        workflow_id: None,
        workflow_kind: None,
        condition_result: None,
        branch: None,
        normalized_inputs,
        state_before: state.clone(),
        state_after: state,
        state_transitions: Vec::new(),
        external_node_ids: Vec::new(),
    }
}

fn finish(
    spec_digest: StatefulSpecDigestV1,
    trace: StatefulSimulationTraceV1,
) -> Result<StatefulSimulationResultV1, StatefulSimulationErrorV1> {
    let trace_digest = stateful_simulation_trace_digest_v1(&trace)?;
    Ok(StatefulSimulationResultV1 {
        spec_digest,
        trace,
        trace_digest,
    })
}

fn resolve_simulation_state(
    spec: &StatefulSpecV1,
    state: &[StateSimulationCellV1],
) -> Result<BTreeMap<String, StateValueV1>, StatefulSimulationErrorV1> {
    let declarations = spec
        .state_variables
        .iter()
        .map(|variable| (variable.id.as_str(), variable))
        .collect::<BTreeMap<_, _>>();
    let mut diagnostics = Vec::new();
    let mut supplied = BTreeMap::new();
    for (index, cell) in state.iter().enumerate() {
        let path = format!("/state/{index}");
        let Some(variable) = declarations.get(cell.variable_id.as_str()).copied() else {
            push(
                &mut diagnostics,
                "simulation_state_unknown",
                format!("{path}/variable_id"),
                "simulation state cells must name declared variables",
            );
            continue;
        };
        if supplied
            .insert(cell.variable_id.as_str(), cell.value.clone())
            .is_some()
        {
            push(
                &mut diagnostics,
                "simulation_state_duplicate",
                format!("{path}/variable_id"),
                "simulation state variable IDs must be unique",
            );
        }
        if variable.value_type.primitive_type() != cell.value.primitive_type() {
            push(
                &mut diagnostics,
                "simulation_state_type_mismatch",
                format!("{path}/value"),
                "simulation state primitive types must match their declarations",
            );
        } else if !variable.value_type.accepts(&cell.value) {
            push(
                &mut diagnostics,
                "simulation_state_out_of_bounds",
                format!("{path}/value"),
                "simulation state values must satisfy their declarations",
            );
        }
        if let StateValueV1::Text { value } = &cell.value {
            if value.contains('\0') {
                push(
                    &mut diagnostics,
                    "simulation_state_text_contains_nul",
                    format!("{path}/value/value"),
                    "state text must not contain U+0000",
                );
            }
            if value.len() > MAX_STATE_TEXT_BYTES_V1
                || value.encode_utf16().count() > MAX_STATE_TEXT_UTF16_UNITS_V1
            {
                push(
                    &mut diagnostics,
                    "simulation_state_text_too_large",
                    format!("{path}/value/value"),
                    "simulation state text must not exceed 4000 UTF-8 bytes or UTF-16 code units",
                );
            }
        }
    }
    diagnostics.sort_by(|left, right| left.path.cmp(&right.path).then(left.code.cmp(&right.code)));
    if !diagnostics.is_empty() {
        return Err(StatefulSimulationErrorV1::InvalidInput { diagnostics });
    }
    Ok(spec
        .state_variables
        .iter()
        .map(|variable| {
            let value = supplied
                .get(variable.id.as_str())
                .cloned()
                .unwrap_or_else(|| variable.initial_value.clone());
            (variable.id.clone(), value)
        })
        .collect())
}

fn evaluation(code: &'static str, node_id: Option<&str>) -> StatefulSimulationErrorV1 {
    StatefulSimulationErrorV1::Evaluation {
        code,
        node_id: node_id.map(str::to_string),
    }
}
