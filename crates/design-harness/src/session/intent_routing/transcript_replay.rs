use crate::errors::StructuredError;
use crate::intent::ExistingChannelKey;
use crate::llm::Message;
use crate::turn::parse_interpret_intent_core_for_human;

use super::super::SessionSnapshotError;
use super::adjudicate::{
    adjudicate_intent_core_v4, IntentCoreAdjudicationV4, PrivateStudyRoomSelectionV4,
};
use super::decision::{IntentRouteDecisionKindV2, IntentRouteDecisionV2};
use super::grounding::deterministically_selected_option;
use super::request_evidence::IntentRequestEvidenceChainV1;
use super::state::{intent_error, snapshot_error, IntentFallbackKind};
use super::transcript_restore::{
    parse_intent_state_anchor, restored_human_text, validate_intent_state_anchor,
};

pub(super) struct ReplayedRoutedCoreV4 {
    fallback_kind: IntentFallbackKind,
    response: String,
}

impl ReplayedRoutedCoreV4 {
    pub(super) fn from_semantics(replayed: ReplayedRoutedSemanticsV4) -> Self {
        Self {
            fallback_kind: replayed.fallback_kind,
            response: replayed.response,
        }
    }

    pub(super) fn is_discussion(&self) -> bool {
        self.fallback_kind == IntentFallbackKind::Discussion
    }

    pub(super) fn response(&self) -> &str {
        &self.response
    }
}

pub(super) struct ReplayedRoutedSemanticsV4 {
    pub(super) fallback_kind: IntentFallbackKind,
    pub(super) decision: IntentRouteDecisionV2,
    pub(super) response: String,
}

pub(super) struct ReplayedPrivateSemanticsV4 {
    pub(super) selection: Box<PrivateStudyRoomSelectionV4>,
    pub(super) human: String,
    pub(super) available_channel_keys: Vec<String>,
    pub(super) expected_revision: u64,
    pub(super) initial_head: String,
}

pub(super) enum ReplayedCoreSemanticsV4 {
    Private(ReplayedPrivateSemanticsV4),
    Routed(ReplayedRoutedSemanticsV4),
}

pub(super) enum CoreReplayErrorV4 {
    Snapshot(SessionSnapshotError),
    Semantic {
        error: StructuredError,
        revision: u64,
    },
}

pub(super) fn replay_core_semantics(
    messages: &[Message],
    human_message_index: u64,
    arguments: &str,
) -> Result<ReplayedCoreSemanticsV4, CoreReplayErrorV4> {
    let human_index = usize::try_from(human_message_index).map_err(|_| {
        CoreReplayErrorV4::Snapshot(snapshot_error(
            "routed Core human transcript index overflowed",
        ))
    })?;
    let human = restored_human_text(messages.get(human_index).ok_or_else(|| {
        CoreReplayErrorV4::Snapshot(snapshot_error(
            "routed Core human transcript message is missing",
        ))
    })?)
    .map_err(CoreReplayErrorV4::Snapshot)?;
    let state = parse_intent_state_anchor(messages.get(human_index.saturating_add(1)).ok_or_else(
        || CoreReplayErrorV4::Snapshot(snapshot_error("routed Core state anchor is missing")),
    )?)
    .map_err(CoreReplayErrorV4::Snapshot)?;
    validate_intent_state_anchor(&state).map_err(CoreReplayErrorV4::Snapshot)?;
    let failure_revision = state.expected_revision;
    let mut core = parse_interpret_intent_core_for_human(arguments, &human).map_err(|error| {
        CoreReplayErrorV4::Semantic {
            error,
            revision: failure_revision,
        }
    })?;
    if core.expected_revision() != state.expected_revision {
        return Err(CoreReplayErrorV4::Semantic {
            error: intent_error(
                "STALE_INTENT_WORKSPACE_REVISION",
                "intent.expected_revision",
                format!(
                    "Intent revision {} does not match the current revision {}",
                    core.expected_revision(),
                    state.expected_revision
                ),
                format!("Retry with expected_revision {}", state.expected_revision),
            ),
            revision: failure_revision,
        });
    }
    let grounded_channel = deterministically_selected_option(&human, &state.available_channel_keys)
        .map(ExistingChannelKey);
    core.apply_human_grounding(&human, grounded_channel.as_ref())
        .map_err(|error| CoreReplayErrorV4::Semantic {
            error,
            revision: failure_revision,
        })?;
    let request_evidence = IntentRequestEvidenceChainV1::from_initial_human(
        messages,
        human_message_index,
        core.expected_revision(),
    )
    .map_err(|error| CoreReplayErrorV4::Semantic {
        error,
        revision: failure_revision,
    })?;
    let initial_head =
        request_evidence
            .initial_head()
            .map_err(|error| CoreReplayErrorV4::Semantic {
                error,
                revision: failure_revision,
            })?;
    let (fallback_kind, decision, response) = match adjudicate_intent_core_v4(core, &initial_head)
        .map_err(|error| {
        CoreReplayErrorV4::Semantic {
            error,
            revision: failure_revision,
        }
    })? {
        IntentCoreAdjudicationV4::PrivateStudyRoom(selection) => {
            return Ok(ReplayedCoreSemanticsV4::Private(
                ReplayedPrivateSemanticsV4 {
                    selection,
                    human,
                    available_channel_keys: state.available_channel_keys,
                    expected_revision: state.expected_revision,
                    initial_head,
                },
            ));
        }
        IntentCoreAdjudicationV4::TypedPlanner(permit) => {
            let (_, _, decision, response) = permit.into_parts();
            (IntentFallbackKind::TypedPlanner, decision, response)
        }
        IntentCoreAdjudicationV4::Terminal(permit) => {
            let (decision, response) = permit.into_parts();
            let fallback_kind = match decision.kind() {
                IntentRouteDecisionKindV2::CapabilityGap => IntentFallbackKind::CapabilityGap,
                IntentRouteDecisionKindV2::Reject => IntentFallbackKind::Reject,
                IntentRouteDecisionKindV2::Discussion => IntentFallbackKind::Discussion,
                IntentRouteDecisionKindV2::PrivateStudyRoom
                | IntentRouteDecisionKindV2::TypedPlanner => {
                    return Err(CoreReplayErrorV4::Snapshot(snapshot_error(
                        "routed Core replay reached an inconsistent terminal decision",
                    )));
                }
            };
            (fallback_kind, decision, response)
        }
    };
    Ok(ReplayedCoreSemanticsV4::Routed(ReplayedRoutedSemanticsV4 {
        fallback_kind,
        decision,
        response,
    }))
}

pub(super) fn core_replay_snapshot_error(error: CoreReplayErrorV4) -> SessionSnapshotError {
    match error {
        CoreReplayErrorV4::Snapshot(error) => error,
        CoreReplayErrorV4::Semantic { error, .. } => restored_semantics_error(error),
    }
}

pub(super) fn restored_semantics_error(error: StructuredError) -> SessionSnapshotError {
    snapshot_error(format!(
        "persisted model semantics failed deterministic replay {}: {}",
        error.code, error.message
    ))
}
