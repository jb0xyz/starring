use crate::tools::ToolDefinition;
use crate::turn::{
    interpret_intent_core_frontier, resolve_intent_decision_frontier, INTERPRET_INTENT_CORE,
    RESOLVE_INTENT_DECISION,
};

use super::state::IntentRecipeStageSnapshotV1;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum IntentFrontierV3 {
    InterpretCore,
    Resolve,
}

impl IntentFrontierV3 {
    pub(super) fn from_stage(stage: &IntentRecipeStageSnapshotV1) -> Self {
        match stage {
            IntentRecipeStageSnapshotV1::Empty
            | IntentRecipeStageSnapshotV1::PreviewReady { .. } => Self::InterpretCore,
            IntentRecipeStageSnapshotV1::AwaitingDecision { .. } => Self::Resolve,
        }
    }

    pub(super) fn name(self) -> &'static str {
        match self {
            Self::InterpretCore => INTERPRET_INTENT_CORE,
            Self::Resolve => RESOLVE_INTENT_DECISION,
        }
    }

    pub(super) fn tools(self) -> Vec<ToolDefinition> {
        match self {
            Self::InterpretCore => interpret_intent_core_frontier().into(),
            Self::Resolve => resolve_intent_decision_frontier().into(),
        }
    }
}
