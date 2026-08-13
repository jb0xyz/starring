use std::collections::BTreeMap;

use automation_core::{normalize_modal_submit_inputs, EventKind, ModalInputError, RuntimeEvent};
use discord_model::{GuildId, UserId};
use serde::{Deserialize, Serialize};

use crate::model::{AutomationSpecV1, ConditionExprV1, TriggerV1};
use crate::validate::{
    diagnostic, lower_shape, validate_automation_spec_v1, AutomationSpecDiagnosticV1,
    AutomationSpecValidationErrorV1, MAX_SIMULATION_INPUTS_V1, MAX_SIMULATION_INPUT_UTF16_UNITS_V1,
};

pub const MAX_SIMULATION_INPUT_BYTES_V1: usize = 4_000;
pub const MAX_SIMULATION_PAYLOAD_BYTES_V1: usize = 20_000;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AutomationSimulationEventV1 {
    pub trigger: TriggerV1,
    #[serde(default)]
    pub inputs: BTreeMap<String, String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AutomationSimulationOutcomeV1 {
    NoTriggerMatch,
    ConditionNotSatisfied,
    ActionsPlanned,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AutomationSimulationTraceV1 {
    pub outcome: AutomationSimulationOutcomeV1,
    pub workflow_id: Option<String>,
    pub condition_result: Option<bool>,
    pub normalized_inputs: BTreeMap<String, String>,
    pub action_node_ids: Vec<String>,
}

#[derive(Debug, thiserror::Error)]
pub enum AutomationSimulationErrorV1 {
    #[error("automation spec is invalid")]
    InvalidSpec(#[from] AutomationSpecValidationErrorV1),
    #[error("automation simulation event is invalid")]
    InvalidEvent {
        diagnostics: Vec<AutomationSpecDiagnosticV1>,
    },
}

impl AutomationSimulationErrorV1 {
    pub fn diagnostics(&self) -> &[AutomationSpecDiagnosticV1] {
        match self {
            Self::InvalidSpec(error) => error.diagnostics(),
            Self::InvalidEvent { diagnostics } => diagnostics,
        }
    }
}

pub fn simulate_automation_spec_v1(
    spec: &AutomationSpecV1,
    event: &AutomationSimulationEventV1,
) -> Result<AutomationSimulationTraceV1, AutomationSimulationErrorV1> {
    validate_automation_spec_v1(spec)?;
    validate_simulation_event(event)?;
    let Some(workflow) = spec
        .workflows
        .iter()
        .find(|workflow| workflow.trigger == event.trigger)
    else {
        return Ok(AutomationSimulationTraceV1 {
            outcome: AutomationSimulationOutcomeV1::NoTriggerMatch,
            workflow_id: None,
            condition_result: None,
            normalized_inputs: BTreeMap::new(),
            action_node_ids: Vec::new(),
        });
    };
    let normalized_inputs = normalize_event_inputs(spec, event)?;
    let condition_result = evaluate_condition(&workflow.condition, &normalized_inputs);
    Ok(AutomationSimulationTraceV1 {
        outcome: if condition_result {
            AutomationSimulationOutcomeV1::ActionsPlanned
        } else {
            AutomationSimulationOutcomeV1::ConditionNotSatisfied
        },
        workflow_id: Some(workflow.id.clone()),
        condition_result: Some(condition_result),
        normalized_inputs,
        action_node_ids: if condition_result {
            workflow
                .actions
                .iter()
                .map(|node| node.id.clone())
                .collect()
        } else {
            Vec::new()
        },
    })
}

fn validate_simulation_event(
    event: &AutomationSimulationEventV1,
) -> Result<(), AutomationSimulationErrorV1> {
    let mut diagnostics = Vec::new();
    let trigger_id = match &event.trigger {
        TriggerV1::ButtonClick { trigger_id } => trigger_id,
        TriggerV1::ModalSubmit { modal_id } => modal_id,
        TriggerV1::InstanceAction { action_id } => action_id,
    };
    if !valid_identifier(trigger_id) {
        diagnostics.push(diagnostic(
            "invalid_simulation_trigger",
            "/event/trigger",
            "simulation trigger IDs must match [a-z][a-z0-9_]{0,63}",
        ));
    }
    if !matches!(event.trigger, TriggerV1::ModalSubmit { .. }) && !event.inputs.is_empty() {
        diagnostics.push(diagnostic(
            "inputs_require_modal_trigger",
            "/event/inputs",
            "only modal_submit simulation events may carry inputs",
        ));
    }
    if event.inputs.len() > MAX_SIMULATION_INPUTS_V1 {
        diagnostics.push(diagnostic(
            "simulation_input_count_exceeded",
            "/event/inputs",
            format!("simulation events support at most {MAX_SIMULATION_INPUTS_V1} inputs"),
        ));
    }
    let mut payload_bytes = trigger_id.len();
    for (key, value) in &event.inputs {
        payload_bytes = payload_bytes
            .saturating_add(key.len())
            .saturating_add(value.len());
        if !valid_identifier(key) {
            diagnostics.push(diagnostic(
                "invalid_simulation_input_id",
                format!("/event/inputs/{key}"),
                "simulation input IDs must match [a-z][a-z0-9_]{0,63}",
            ));
        }
        if value.len() > MAX_SIMULATION_INPUT_BYTES_V1
            || value.encode_utf16().count() > MAX_SIMULATION_INPUT_UTF16_UNITS_V1
        {
            diagnostics.push(diagnostic(
                "simulation_input_too_large",
                format!("/event/inputs/{key}"),
                "simulation inputs must not exceed 4000 UTF-8 bytes or UTF-16 code units",
            ));
        }
    }
    if payload_bytes > MAX_SIMULATION_PAYLOAD_BYTES_V1 {
        diagnostics.push(diagnostic(
            "simulation_payload_too_large",
            "/event",
            "simulation trigger and input payload must not exceed 20000 UTF-8 bytes",
        ));
    }
    if diagnostics.is_empty() {
        Ok(())
    } else {
        diagnostics
            .sort_by(|left, right| left.path.cmp(&right.path).then(left.code.cmp(&right.code)));
        Err(AutomationSimulationErrorV1::InvalidEvent { diagnostics })
    }
}

fn normalize_event_inputs(
    spec: &AutomationSpecV1,
    event: &AutomationSimulationEventV1,
) -> Result<BTreeMap<String, String>, AutomationSimulationErrorV1> {
    let TriggerV1::ModalSubmit { modal_id } = &event.trigger else {
        return Ok(BTreeMap::new());
    };
    let runtime_event = RuntimeEvent {
        guild_id: GuildId(1),
        actor: UserId(1),
        kind: EventKind::ModalSubmit {
            modal: modal_id.clone(),
            inputs: event.inputs.clone(),
        },
    };
    match normalize_modal_submit_inputs(&runtime_event, &lower_shape(spec)) {
        Ok(Some(inputs)) => Ok(inputs),
        Ok(None) => Ok(BTreeMap::new()),
        Err(error) => Err(AutomationSimulationErrorV1::InvalidEvent {
            diagnostics: vec![modal_input_diagnostic(error)],
        }),
    }
}

fn modal_input_diagnostic(error: ModalInputError) -> AutomationSpecDiagnosticV1 {
    match error {
        ModalInputError::ModalDefinitionMissing { modal } => diagnostic(
            "simulation_modal_missing",
            "/event/trigger/modal_id",
            format!("modal definition is missing: {modal}"),
        ),
        ModalInputError::RequiredMissing { field, .. } => diagnostic(
            "simulation_required_input_missing",
            format!("/event/inputs/{field}"),
            "required modal input is missing or empty after normalization",
        ),
        ModalInputError::Unexpected { field, .. } => diagnostic(
            "simulation_input_unexpected",
            format!("/event/inputs/{field}"),
            "input is not declared by the trigger modal",
        ),
        ModalInputError::TooShort { field, .. } => diagnostic(
            "simulation_input_too_short",
            format!("/event/inputs/{field}"),
            "input is shorter than the modal minimum after normalization",
        ),
        ModalInputError::TooLong { field, .. } => diagnostic(
            "simulation_input_too_long",
            format!("/event/inputs/{field}"),
            "input is longer than the modal maximum after normalization",
        ),
    }
}

fn evaluate_condition(condition: &ConditionExprV1, inputs: &BTreeMap<String, String>) -> bool {
    match condition {
        ConditionExprV1::Always => true,
        ConditionExprV1::InputNonEmpty { input_id } => {
            inputs.get(input_id).is_some_and(|value| !value.is_empty())
        }
        ConditionExprV1::InputEquals { input_id, value } => inputs.get(input_id) == Some(value),
        ConditionExprV1::All { conditions } => conditions
            .iter()
            .all(|condition| evaluate_condition(condition, inputs)),
        ConditionExprV1::Any { conditions } => conditions
            .iter()
            .any(|condition| evaluate_condition(condition, inputs)),
        ConditionExprV1::Not { condition } => !evaluate_condition(condition, inputs),
    }
}

fn valid_identifier(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 64
        && value
            .bytes()
            .next()
            .is_some_and(|byte| byte.is_ascii_lowercase())
        && value
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_')
}
