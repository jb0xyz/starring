use crate::tools::ToolDefinition;
use crate::turn::{
    interpret_intent_core_frontier, resolve_intent_decision_frontier, INTERPRET_INTENT_CORE,
    RESOLVE_INTENT_DECISION,
};

use super::state::IntentRecipeStageSnapshotV2;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum IntentFrontierV4 {
    InterpretCore,
    Resolve,
}

impl IntentFrontierV4 {
    pub(super) fn from_stage(stage: &IntentRecipeStageSnapshotV2) -> Self {
        match stage {
            IntentRecipeStageSnapshotV2::Empty
            | IntentRecipeStageSnapshotV2::PreviewReady { .. } => Self::InterpretCore,
            IntentRecipeStageSnapshotV2::AwaitingDecision { .. } => Self::Resolve,
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
