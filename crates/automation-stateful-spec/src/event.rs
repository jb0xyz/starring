use std::collections::BTreeMap;

use automation_spec::{
    simulate_automation_spec_v1, AutomationSimulationErrorV1, AutomationSimulationEventV1,
    AutomationSimulationOutcomeV1, TriggerV1,
};

use crate::model::StatefulSpecV1;
use crate::validate::{
    validate_stateful_spec_v1, StatefulSpecDiagnosticV1, StatefulSpecValidationErrorV1,
};
use crate::view::{automation_spec_validation_view_v1, BranchViewV1};

#[derive(Debug, thiserror::Error)]
pub enum StatefulEventNormalizationErrorV1 {
    #[error("stateful spec is invalid")]
    InvalidSpec(#[from] StatefulSpecValidationErrorV1),
    #[error("stateful event input is invalid")]
    InvalidEvent {
        diagnostics: Vec<StatefulSpecDiagnosticV1>,
    },
    #[error("validated stateful spec produced an invalid private shape-validation view")]
    InvalidShapeView,
}

impl StatefulEventNormalizationErrorV1 {
    pub fn diagnostics(&self) -> &[StatefulSpecDiagnosticV1] {
        match self {
            Self::InvalidSpec(error) => error.diagnostics(),
            Self::InvalidEvent { diagnostics } => diagnostics,
            Self::InvalidShapeView => &[],
        }
    }
}

/// Normalizes raw modal inputs using the exact shared AutomationSpec/live modal policy. The
/// trigger must already have been derived from a verified interaction; this helper performs no
/// routing or request authentication. Non-modal triggers require an empty input map.
pub fn normalize_stateful_event_inputs_v1(
    spec: &StatefulSpecV1,
    trigger: &TriggerV1,
    raw_inputs: &BTreeMap<String, String>,
) -> Result<BTreeMap<String, String>, StatefulEventNormalizationErrorV1> {
    validate_stateful_spec_v1(spec)?;
    let view = automation_spec_validation_view_v1(spec, BranchViewV1::True);
    let event = AutomationSimulationEventV1 {
        trigger: trigger.clone(),
        inputs: raw_inputs.clone(),
    };
    match simulate_automation_spec_v1(&view, &event) {
        Ok(trace) if trace.outcome != AutomationSimulationOutcomeV1::NoTriggerMatch => {
            if let Some((input_id, _)) = trace
                .normalized_inputs
                .iter()
                .find(|(_, value)| value.contains('\0'))
            {
                return Err(StatefulEventNormalizationErrorV1::InvalidEvent {
                    diagnostics: vec![StatefulSpecDiagnosticV1 {
                        code: "event_input_contains_nul".to_string(),
                        path: format!("/event/inputs/{input_id}"),
                        message: "normalized event inputs must not contain U+0000".to_string(),
                    }],
                });
            }
            Ok(trace.normalized_inputs)
        }
        Ok(_) => Err(StatefulEventNormalizationErrorV1::InvalidEvent {
            diagnostics: vec![StatefulSpecDiagnosticV1 {
                code: "event_trigger_not_declared".to_string(),
                path: "/event/trigger".to_string(),
                message: "the derived trigger must match exactly one validated workflow"
                    .to_string(),
            }],
        }),
        Err(AutomationSimulationErrorV1::InvalidEvent { diagnostics }) => {
            Err(StatefulEventNormalizationErrorV1::InvalidEvent {
                diagnostics: diagnostics
                    .into_iter()
                    .map(|diagnostic| StatefulSpecDiagnosticV1 {
                        code: format!("event_{}", diagnostic.code),
                        path: diagnostic.path,
                        message: diagnostic.message,
                    })
                    .collect(),
            })
        }
        Err(AutomationSimulationErrorV1::InvalidSpec(_)) => {
            Err(StatefulEventNormalizationErrorV1::InvalidShapeView)
        }
    }
}
