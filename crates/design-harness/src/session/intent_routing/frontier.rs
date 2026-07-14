use crate::tools::ToolDefinition;
use crate::turn::{
    interpret_intent_turn_frontier, resolve_intent_decision_frontier, INTERPRET_INTENT_TURN,
    RESOLVE_INTENT_DECISION,
};

use super::state::IntentRecipeStageSnapshotV1;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum IntentFrontierV2 {
    Interpret,
    Resolve,
}

impl IntentFrontierV2 {
    pub(super) fn from_stage(stage: &IntentRecipeStageSnapshotV1) -> Self {
        match stage {
            IntentRecipeStageSnapshotV1::Empty
            | IntentRecipeStageSnapshotV1::PreviewReady { .. } => Self::Interpret,
            IntentRecipeStageSnapshotV1::AwaitingDecision { .. } => Self::Resolve,
        }
    }

    pub(super) fn name(self) -> &'static str {
        match self {
            Self::Interpret => INTERPRET_INTENT_TURN,
            Self::Resolve => RESOLVE_INTENT_DECISION,
        }
    }

    pub(super) fn tools(self) -> Vec<ToolDefinition> {
        match self {
            Self::Interpret => interpret_intent_turn_frontier().into(),
            Self::Resolve => resolve_intent_decision_frontier().into(),
        }
    }
}
