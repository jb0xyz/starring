use serde::{Deserialize, Serialize};

use crate::errors::StructuredError;
use crate::intent::identity::{canonical_json_digest, is_lowercase_sha256_hex, IdentityErrorSpec};
use crate::intent::{
    apply_existing_channel_decision, prepare_intent_workspace, ExistingChannelKey,
    IntentRequestedOutcome, IntentResolutionContext, IntentWorkspaceV2, PreparedIntentWorkspaceV2,
};
use crate::llm::{Message, MessageRole};

use super::grounding::deterministically_selected_option;
use super::state::INTENT_HUMAN_PREFIX;

const HUMAN_TURN_DIGEST_DOMAIN_V1: &[u8] = b"starring.intent.human_turn.v1\0";
const INITIAL_EVIDENCE_DIGEST_DOMAIN_V1: &[u8] = b"starring.intent.request_evidence.initial.v1\0";
const RESOLUTION_EVIDENCE_DIGEST_DOMAIN_V1: &[u8] =
    b"starring.intent.request_evidence.resolution.v1\0";
const TERMINAL_FINALIZATION_EVIDENCE_DIGEST_DOMAIN_V1: &[u8] =
    b"starring.intent.request_evidence.terminal_finalization.v1\0";
const ACTIVE_OPTIONS_DIGEST_DOMAIN_V1: &[u8] = b"starring.intent.request_evidence.options.v1\0";

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub(super) enum AcceptedIntentResolutionV1 {
    ExistingChannel(String),
}

impl AcceptedIntentResolutionV1 {
    fn option_value(&self) -> &str {
        match self {
            Self::ExistingChannel(value) => value,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub(super) enum IntentRequestEvidenceEntryV1 {
    InitialHuman {
        transcript_message_index: u64,
        expected_revision: u64,
        human_turn_digest: String,
    },
    AcceptedResolution {
        previous_chain_head: String,
        turn_index: u64,
        transcript_message_index: u64,
        expected_revision: u64,
        decision_id: String,
        decision_path: String,
        active_options_digest: String,
        human_turn_digest: String,
        accepted_typed_value: AcceptedIntentResolutionV1,
    },
    TerminalOutcomeFinalization {
        previous_chain_head: String,
        transcript_message_index: u64,
        expected_draft_revision: u64,
        prior_workspace_revision: u64,
        next_workspace_revision: u64,
        prior_requested_outcome: IntentRequestedOutcome,
        next_requested_outcome: IntentRequestedOutcome,
        human_turn_digest: String,
        standalone_request_evidence_digest: String,
        standalone_adjudication_digest: String,
        standalone_recipe_evidence_digest: String,
    },
}

impl IntentRequestEvidenceEntryV1 {
    pub(super) fn transcript_message_index(&self) -> u64 {
        match self {
            Self::InitialHuman {
                transcript_message_index,
                ..
            }
            | Self::AcceptedResolution {
                transcript_message_index,
                ..
            }
            | Self::TerminalOutcomeFinalization {
                transcript_message_index,
                ..
            } => *transcript_message_index,
        }
    }

    fn resolution_expected_revision(&self) -> Option<u64> {
        match self {
            Self::InitialHuman {
                expected_revision, ..
            }
            | Self::AcceptedResolution {
                expected_revision, ..
            } => Some(*expected_revision),
            Self::TerminalOutcomeFinalization { .. } => None,
        }
    }

    pub(super) fn human_turn_digest(&self) -> &str {
        match self {
            Self::InitialHuman {
                human_turn_digest, ..
            }
            | Self::AcceptedResolution {
                human_turn_digest, ..
            }
            | Self::TerminalOutcomeFinalization {
                human_turn_digest, ..
            } => human_turn_digest,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct IntentRequestEvidenceChainV1 {
    entries: Vec<IntentRequestEvidenceEntryV1>,
    head: String,
}

pub(super) struct AcceptedResolutionEvidenceInputV1<'a> {
    pub(super) turn_index: u64,
    pub(super) transcript_message_index: u64,
    pub(super) expected_revision: u64,
    pub(super) decision_id: &'a str,
    pub(super) decision_path: &'a str,
    pub(super) active_options: &'a [String],
    pub(super) accepted_typed_value: AcceptedIntentResolutionV1,
}

pub(super) struct TerminalOutcomeFinalizationEvidenceInputV1<'a> {
    pub(super) transcript_message_index: u64,
    pub(super) expected_draft_revision: u64,
    pub(super) prior_workspace_revision: u64,
    pub(super) next_workspace_revision: u64,
    pub(super) standalone_request_evidence_digest: &'a str,
    pub(super) standalone_adjudication_digest: &'a str,
    pub(super) standalone_recipe_evidence_digest: &'a str,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct TerminalOutcomeFinalizationEvidenceRefV1<'a> {
    previous_chain_head: &'a str,
    transcript_message_index: u64,
    expected_draft_revision: u64,
    prior_workspace_revision: u64,
    next_workspace_revision: u64,
    human_turn_digest: &'a str,
    standalone_request_evidence_digest: &'a str,
    standalone_adjudication_digest: &'a str,
    standalone_recipe_evidence_digest: &'a str,
}

impl<'a> TerminalOutcomeFinalizationEvidenceRefV1<'a> {
    pub(super) fn previous_chain_head(self) -> &'a str {
        self.previous_chain_head
    }

    pub(super) fn transcript_message_index(self) -> u64 {
        self.transcript_message_index
    }

    pub(super) fn expected_draft_revision(self) -> u64 {
        self.expected_draft_revision
    }

    pub(super) fn prior_workspace_revision(self) -> u64 {
        self.prior_workspace_revision
    }

    pub(super) fn next_workspace_revision(self) -> u64 {
        self.next_workspace_revision
    }

    pub(super) fn human_turn_digest(self) -> &'a str {
        self.human_turn_digest
    }

    pub(super) fn standalone_request_evidence_digest(self) -> &'a str {
        self.standalone_request_evidence_digest
    }

    pub(super) fn standalone_adjudication_digest(self) -> &'a str {
        self.standalone_adjudication_digest
    }

    pub(super) fn standalone_recipe_evidence_digest(self) -> &'a str {
        self.standalone_recipe_evidence_digest
    }
}

#[derive(Serialize)]
#[serde(deny_unknown_fields)]
struct HumanTurnDigestProjectionV1<'a> {
    text: &'a str,
}

#[derive(Serialize)]
#[serde(deny_unknown_fields)]
struct ActiveOptionsDigestProjectionV1<'a> {
    options: &'a [String],
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct IntentHumanEnvelopeV1 {
    text: String,
}

impl IntentRequestEvidenceChainV1 {
    pub(super) fn from_initial_human(
        transcript: &[Message],
        transcript_message_index: u64,
        expected_revision: u64,
    ) -> Result<Self, StructuredError> {
        Self::from_initial_human_with_prefix(
            transcript,
            transcript_message_index,
            expected_revision,
            INTENT_HUMAN_PREFIX,
        )
    }

    pub(super) fn from_initial_human_with_prefix(
        transcript: &[Message],
        transcript_message_index: u64,
        expected_revision: u64,
        prefix: &str,
    ) -> Result<Self, StructuredError> {
        let human_turn = transcript_human_turn(transcript, transcript_message_index, prefix)?;
        let entry = IntentRequestEvidenceEntryV1::InitialHuman {
            transcript_message_index,
            expected_revision,
            human_turn_digest: human_turn_digest(&human_turn)?,
        };
        let head = entry_digest(&entry)?;
        Ok(Self {
            entries: vec![entry],
            head,
        })
    }

    pub(super) fn append_accepted_resolution(
        &mut self,
        transcript: &[Message],
        input: AcceptedResolutionEvidenceInputV1<'_>,
    ) -> Result<(), StructuredError> {
        self.append_accepted_resolution_with_prefix(transcript, input, INTENT_HUMAN_PREFIX)
    }

    pub(super) fn append_accepted_resolution_with_prefix(
        &mut self,
        transcript: &[Message],
        input: AcceptedResolutionEvidenceInputV1<'_>,
        prefix: &str,
    ) -> Result<(), StructuredError> {
        self.validate_against_transcript_with_prefix(transcript, prefix)?;
        validate_resolution_input(&input)?;
        let human_turn = transcript_human_turn(transcript, input.transcript_message_index, prefix)?;
        let entry = IntentRequestEvidenceEntryV1::AcceptedResolution {
            previous_chain_head: self.head.clone(),
            turn_index: input.turn_index,
            transcript_message_index: input.transcript_message_index,
            expected_revision: input.expected_revision,
            decision_id: input.decision_id.to_string(),
            decision_path: input.decision_path.to_string(),
            active_options_digest: active_options_digest(input.active_options)?,
            human_turn_digest: human_turn_digest(&human_turn)?,
            accepted_typed_value: input.accepted_typed_value,
        };
        validate_append_shape(&self.entries, &entry)?;
        let head = entry_digest(&entry)?;
        self.entries.push(entry);
        self.head = head;
        Ok(())
    }

    pub(super) fn append_terminal_outcome_finalization(
        &mut self,
        transcript: &[Message],
        input: TerminalOutcomeFinalizationEvidenceInputV1<'_>,
    ) -> Result<(), StructuredError> {
        self.append_terminal_outcome_finalization_with_prefix(
            transcript,
            input,
            INTENT_HUMAN_PREFIX,
        )
    }

    pub(super) fn append_terminal_outcome_finalization_with_prefix(
        &mut self,
        transcript: &[Message],
        input: TerminalOutcomeFinalizationEvidenceInputV1<'_>,
        prefix: &str,
    ) -> Result<(), StructuredError> {
        self.validate_against_transcript_with_prefix(transcript, prefix)?;
        validate_terminal_finalization_input(&input)?;
        let human_turn = transcript_human_turn(transcript, input.transcript_message_index, prefix)?;
        let human_turn_digest = human_turn_digest(&human_turn)?;
        let standalone_request_evidence_digest = standalone_initial_evidence_digest(
            input.transcript_message_index,
            input.expected_draft_revision,
            &human_turn_digest,
        )?;
        if input.standalone_request_evidence_digest != standalone_request_evidence_digest {
            return Err(evidence_error(
                "INTENT_REQUEST_EVIDENCE_FINALIZATION_STANDALONE_REQUEST_MISMATCH",
                "intent.request_evidence.entries.standalone_request_evidence_digest",
                "Terminal finalization standalone request evidence does not match the current human turn and Draft revision",
                "Use the exact standalone request evidence head from the finalization turn",
            ));
        }
        let entry = IntentRequestEvidenceEntryV1::TerminalOutcomeFinalization {
            previous_chain_head: self.head.clone(),
            transcript_message_index: input.transcript_message_index,
            expected_draft_revision: input.expected_draft_revision,
            prior_workspace_revision: input.prior_workspace_revision,
            next_workspace_revision: input.next_workspace_revision,
            prior_requested_outcome: IntentRequestedOutcome::WorkingDraft,
            next_requested_outcome: IntentRequestedOutcome::ValidatedPreview,
            human_turn_digest,
            standalone_request_evidence_digest,
            standalone_adjudication_digest: input.standalone_adjudication_digest.to_string(),
            standalone_recipe_evidence_digest: input.standalone_recipe_evidence_digest.to_string(),
        };
        validate_append_shape(&self.entries, &entry)?;
        let head = entry_digest(&entry)?;
        self.entries.push(entry);
        self.head = head;
        Ok(())
    }

    pub(super) fn validate(&self) -> Result<(), StructuredError> {
        let Some(first) = self.entries.first() else {
            return Err(evidence_error(
                "INTENT_REQUEST_EVIDENCE_EMPTY",
                "intent.request_evidence.entries",
                "Request evidence must contain an initial human entry",
                "Recreate the intent session from its original human request",
            ));
        };
        validate_entry_shape(first, true)?;
        let mut computed_head = entry_digest(first)?;
        for (index, entry) in self.entries.iter().enumerate().skip(1) {
            validate_entry_shape(entry, false)?;
            let previous_chain_head = match entry {
                IntentRequestEvidenceEntryV1::AcceptedResolution {
                    previous_chain_head,
                    ..
                }
                | IntentRequestEvidenceEntryV1::TerminalOutcomeFinalization {
                    previous_chain_head,
                    ..
                } => previous_chain_head,
                IntentRequestEvidenceEntryV1::InitialHuman { .. } => {
                    return Err(evidence_error(
                        "INTENT_REQUEST_EVIDENCE_ORDER_INVALID",
                        "intent.request_evidence.entries",
                        "Only the first request evidence entry may be initial_human",
                        "Preserve the original ordered evidence chain",
                    ));
                }
            };
            if previous_chain_head != &computed_head {
                return Err(evidence_error(
                    "INTENT_REQUEST_EVIDENCE_LINK_INVALID",
                    "intent.request_evidence.entries.previous_chain_head",
                    "Request evidence chain link does not match the preceding entry",
                    "Restore an untampered request evidence chain",
                ));
            }
            validate_append_shape(&self.entries[..index], entry)?;
            computed_head = entry_digest(entry)?;
        }
        if self.head != computed_head {
            return Err(evidence_error(
                "INTENT_REQUEST_EVIDENCE_HEAD_INVALID",
                "intent.request_evidence.head",
                "Request evidence head does not match the ordered entries",
                "Restore an untampered request evidence chain",
            ));
        }
        Ok(())
    }

    pub(super) fn validate_against_transcript(
        &self,
        transcript: &[Message],
    ) -> Result<(), StructuredError> {
        self.validate_against_transcript_with_prefix(transcript, INTENT_HUMAN_PREFIX)
    }

    pub(super) fn validate_against_transcript_with_prefix(
        &self,
        transcript: &[Message],
        prefix: &str,
    ) -> Result<(), StructuredError> {
        self.validate()?;
        for entry in &self.entries {
            let human_turn =
                transcript_human_turn(transcript, entry.transcript_message_index(), prefix)?;
            let actual = human_turn_digest(&human_turn)?;
            if entry.human_turn_digest() != actual {
                return Err(evidence_error(
                    "INTENT_REQUEST_EVIDENCE_TRANSCRIPT_MISMATCH",
                    "intent.request_evidence.entries.human_turn_digest",
                    "Request evidence does not match its referenced human transcript message",
                    "Restore the original append-only intent transcript",
                ));
            }
            if let IntentRequestEvidenceEntryV1::TerminalOutcomeFinalization {
                transcript_message_index,
                expected_draft_revision,
                standalone_request_evidence_digest,
                ..
            } = entry
            {
                let expected = standalone_initial_evidence_digest(
                    *transcript_message_index,
                    *expected_draft_revision,
                    &actual,
                )?;
                if standalone_request_evidence_digest != &expected {
                    return Err(evidence_error(
                        "INTENT_REQUEST_EVIDENCE_FINALIZATION_STANDALONE_REQUEST_MISMATCH",
                        "intent.request_evidence.entries.standalone_request_evidence_digest",
                        "Terminal finalization standalone request evidence does not match the current human turn and Draft revision",
                        "Restore the exact standalone request evidence head from the finalization turn",
                    ));
                }
            }
        }
        Ok(())
    }

    pub(super) fn entries(&self) -> &[IntentRequestEvidenceEntryV1] {
        &self.entries
    }

    pub(super) fn head(&self) -> &str {
        &self.head
    }

    pub(super) fn initial_head(&self) -> Result<String, StructuredError> {
        self.entries
            .first()
            .map(entry_digest)
            .transpose()?
            .ok_or_else(|| {
                evidence_error(
                    "INTENT_REQUEST_EVIDENCE_EMPTY",
                    "intent.request_evidence.entries",
                    "Request evidence must contain an initial human entry",
                    "Recreate the intent session from its original human request",
                )
            })
    }

    pub(super) fn initial_human_turn_digest(&self) -> Result<&str, StructuredError> {
        match self.entries.first() {
            Some(IntentRequestEvidenceEntryV1::InitialHuman {
                human_turn_digest, ..
            }) => Ok(human_turn_digest),
            _ => Err(evidence_error(
                "INTENT_REQUEST_EVIDENCE_ORDER_INVALID",
                "intent.request_evidence.entries",
                "Request evidence does not begin with initial_human",
                "Preserve the original ordered evidence chain",
            )),
        }
    }

    pub(super) fn initial_expected_revision(&self) -> Result<u64, StructuredError> {
        match self.entries.first() {
            Some(IntentRequestEvidenceEntryV1::InitialHuman {
                expected_revision, ..
            }) => Ok(*expected_revision),
            _ => Err(evidence_error(
                "INTENT_REQUEST_EVIDENCE_ORDER_INVALID",
                "intent.request_evidence.entries",
                "Request evidence does not begin with initial_human",
                "Preserve the original ordered evidence chain",
            )),
        }
    }

    pub(super) fn terminal_outcome_finalization(
        &self,
    ) -> Option<TerminalOutcomeFinalizationEvidenceRefV1<'_>> {
        self.entries.iter().find_map(|entry| match entry {
            IntentRequestEvidenceEntryV1::TerminalOutcomeFinalization {
                previous_chain_head,
                transcript_message_index,
                expected_draft_revision,
                prior_workspace_revision,
                next_workspace_revision,
                human_turn_digest,
                standalone_request_evidence_digest,
                standalone_adjudication_digest,
                standalone_recipe_evidence_digest,
                ..
            } => Some(TerminalOutcomeFinalizationEvidenceRefV1 {
                previous_chain_head,
                transcript_message_index: *transcript_message_index,
                expected_draft_revision: *expected_draft_revision,
                prior_workspace_revision: *prior_workspace_revision,
                next_workspace_revision: *next_workspace_revision,
                human_turn_digest,
                standalone_request_evidence_digest,
                standalone_adjudication_digest,
                standalone_recipe_evidence_digest,
            }),
            IntentRequestEvidenceEntryV1::InitialHuman { .. }
            | IntentRequestEvidenceEntryV1::AcceptedResolution { .. } => None,
        })
    }

    pub(super) fn terminal_outcome_finalization_count(&self) -> usize {
        self.entries
            .iter()
            .filter(|entry| {
                matches!(
                    entry,
                    IntentRequestEvidenceEntryV1::TerminalOutcomeFinalization { .. }
                )
            })
            .count()
    }

    pub(super) fn validate_resolutions_against_workspace(
        &self,
        transcript: &[Message],
        workspace: &IntentWorkspaceV2,
        context: &IntentResolutionContext,
    ) -> Result<(), StructuredError> {
        self.validate_against_transcript(transcript)?;
        let mut reproduced = self.initial_workspace(workspace)?;
        validate_initial_channel_grounding(self, transcript, &reproduced, context)?;
        let accepted = self
            .entries
            .iter()
            .filter(|entry| {
                matches!(
                    entry,
                    IntentRequestEvidenceEntryV1::AcceptedResolution { .. }
                )
            })
            .collect::<Vec<_>>();
        for (offset, entry) in accepted.iter().enumerate() {
            let IntentRequestEvidenceEntryV1::AcceptedResolution {
                turn_index,
                transcript_message_index,
                expected_revision,
                decision_id,
                decision_path,
                active_options_digest: persisted_options_digest,
                accepted_typed_value,
                ..
            } = entry
            else {
                return Err(evidence_error(
                    "INTENT_REQUEST_EVIDENCE_ORDER_INVALID",
                    "intent.request_evidence.entries",
                    "Accepted resolution projection contains a non-resolution entry",
                    "Restore the exact ordered evidence chain",
                ));
            };
            let ordinal = u64::try_from(offset)
                .ok()
                .and_then(|value| value.checked_add(1))
                .ok_or_else(|| {
                    evidence_error(
                        "INTENT_REQUEST_EVIDENCE_OVERFLOW",
                        "intent.request_evidence.entries.turn_index",
                        "Accepted resolution turn index exceeds the supported range",
                        "Start a new intent session",
                    )
                })?;
            if *turn_index != ordinal || *expected_revision != reproduced.revision {
                return Err(evidence_error(
                    "INTENT_REQUEST_EVIDENCE_RESOLUTION_SEQUENCE_MISMATCH",
                    "intent.request_evidence.entries.turn_index",
                    "Accepted resolution turn or revision does not match the rederived decision sequence",
                    "Restore the exact decision order and workspace revision",
                ));
            }
            let PreparedIntentWorkspaceV2::NeedsInput {
                workspace: normalized,
                decisions,
            } = prepare_intent_workspace(reproduced.clone(), context)?
            else {
                return Err(evidence_error(
                    "INTENT_REQUEST_EVIDENCE_DECISION_NOT_REPRODUCED",
                    "intent.request_evidence.entries.decision_id",
                    "Accepted resolution history references a decision that is not pending",
                    "Restore the typed workspace before the accepted decision",
                ));
            };
            let [decision] = decisions.as_slice() else {
                return Err(evidence_error(
                    "INTENT_REQUEST_EVIDENCE_DECISION_NOT_REPRODUCED",
                    "intent.request_evidence.entries.decision_id",
                    "Accepted resolution history does not reproduce exactly one active decision",
                    "Restore the closed-recipe decision frontier",
                ));
            };
            if normalized != reproduced
                || decision.id != *decision_id
                || decision.path != *decision_path
                || active_options_digest(&decision.options)? != *persisted_options_digest
            {
                return Err(evidence_error(
                    "INTENT_REQUEST_EVIDENCE_DECISION_MISMATCH",
                    "intent.request_evidence.entries.decision_id",
                    "Accepted resolution evidence does not match the deterministically rederived decision",
                    "Restore the exact decision ID, path, and active options",
                ));
            }
            let human_turn =
                transcript_human_turn(transcript, *transcript_message_index, INTENT_HUMAN_PREFIX)?;
            let selected = accepted_typed_value.option_value();
            if deterministically_selected_option(&human_turn, &decision.options).as_deref()
                != Some(selected)
            {
                return Err(evidence_error(
                    "INTENT_REQUEST_EVIDENCE_HUMAN_SELECTION_MISMATCH",
                    "intent.request_evidence.entries.accepted_typed_value",
                    "Accepted typed value is not the one unambiguous active option named in the human reply",
                    "Restore the original human reply and deterministically accepted option",
                ));
            }
            reproduced = match apply_existing_channel_decision(
                &reproduced,
                *expected_revision,
                ExistingChannelKey(selected.to_string()),
                context,
            )? {
                PreparedIntentWorkspaceV2::NeedsInput { workspace, .. }
                | PreparedIntentWorkspaceV2::Resolved { workspace, .. } => workspace,
            };
        }
        if let Some(finalization) = self.terminal_outcome_finalization() {
            apply_terminal_outcome_finalization_to_workspace(&mut reproduced, finalization)?;
        }
        if reproduced != *workspace {
            return Err(evidence_error(
                "INTENT_REQUEST_EVIDENCE_WORKSPACE_MISMATCH",
                "intent.request_evidence.entries.accepted_typed_value",
                "Applying accepted resolutions does not reproduce the final typed workspace",
                "Restore the exact typed workspace and accepted values",
            ));
        }
        Ok(())
    }

    pub(super) fn initial_workspace(
        &self,
        workspace: &IntentWorkspaceV2,
    ) -> Result<IntentWorkspaceV2, StructuredError> {
        let accepted = self
            .entries
            .iter()
            .filter(|entry| {
                matches!(
                    entry,
                    IntentRequestEvidenceEntryV1::AcceptedResolution { .. }
                )
            })
            .collect::<Vec<_>>();
        if accepted.len() > 1 {
            return Err(evidence_error(
                "INTENT_REQUEST_EVIDENCE_RESOLUTION_COUNT_INVALID",
                "intent.request_evidence.entries",
                "The active recipe can contain at most one accepted existing-channel resolution",
                "Restore the exact closed-recipe decision history",
            ));
        }
        let accepted_count = u64::try_from(accepted.len()).map_err(|_| {
            evidence_error(
                "INTENT_REQUEST_EVIDENCE_OVERFLOW",
                "intent.request_evidence.entries",
                "Accepted resolution count exceeds the supported range",
                "Start a new intent session",
            )
        })?;
        let mut base = workspace.clone();
        if let Some(finalization) = self.terminal_outcome_finalization() {
            reverse_terminal_outcome_finalization_from_workspace(&mut base, finalization)?;
        }
        if base.revision != accepted_count.saturating_add(1) {
            return Err(evidence_error(
                "INTENT_REQUEST_EVIDENCE_WORKSPACE_REVISION_MISMATCH",
                "intent.request_evidence.entries.expected_revision",
                "Accepted resolution history does not reproduce the workspace revision",
                "Restore the exact typed workspace and accepted decision history",
            ));
        }
        for entry in accepted.iter().rev() {
            let IntentRequestEvidenceEntryV1::AcceptedResolution {
                accepted_typed_value,
                ..
            } = entry
            else {
                return Err(evidence_error(
                    "INTENT_REQUEST_EVIDENCE_ORDER_INVALID",
                    "intent.request_evidence.entries",
                    "Accepted resolution projection contains a non-resolution entry",
                    "Restore the exact ordered evidence chain",
                ));
            };
            reverse_existing_channel_resolution(&mut base, accepted_typed_value)?;
        }
        if base.revision != 1 {
            return Err(evidence_error(
                "INTENT_REQUEST_EVIDENCE_WORKSPACE_REVISION_MISMATCH",
                "intent.workspace.revision",
                "Reversing accepted resolutions did not recover the initial workspace revision",
                "Restore the exact typed workspace and accepted decision history",
            ));
        }
        Ok(base)
    }

    #[cfg(test)]
    pub(super) fn accepted_resolution_count(&self) -> usize {
        self.entries
            .iter()
            .filter(|entry| {
                matches!(
                    entry,
                    IntentRequestEvidenceEntryV1::AcceptedResolution { .. }
                )
            })
            .count()
    }
}

fn reverse_terminal_outcome_finalization_from_workspace(
    workspace: &mut IntentWorkspaceV2,
    finalization: TerminalOutcomeFinalizationEvidenceRefV1<'_>,
) -> Result<(), StructuredError> {
    if workspace.revision != finalization.next_workspace_revision()
        || workspace.requested_outcome != IntentRequestedOutcome::ValidatedPreview
    {
        return Err(evidence_error(
            "INTENT_REQUEST_EVIDENCE_FINALIZATION_WORKSPACE_MISMATCH",
            "intent.workspace.requested_outcome",
            "Terminal finalization evidence does not match the final workspace revision and outcome",
            "Restore the exact validated-preview workspace produced by finalization",
        ));
    }
    workspace.revision = finalization.prior_workspace_revision();
    workspace.requested_outcome = IntentRequestedOutcome::WorkingDraft;
    Ok(())
}

fn apply_terminal_outcome_finalization_to_workspace(
    workspace: &mut IntentWorkspaceV2,
    finalization: TerminalOutcomeFinalizationEvidenceRefV1<'_>,
) -> Result<(), StructuredError> {
    if workspace.revision != finalization.prior_workspace_revision()
        || workspace.requested_outcome != IntentRequestedOutcome::WorkingDraft
    {
        return Err(evidence_error(
            "INTENT_REQUEST_EVIDENCE_FINALIZATION_WORKSPACE_MISMATCH",
            "intent.workspace.requested_outcome",
            "Terminal finalization evidence does not match the prior workspace revision and outcome",
            "Restore the exact working-draft workspace before finalization",
        ));
    }
    workspace.revision = finalization.next_workspace_revision();
    workspace.requested_outcome = IntentRequestedOutcome::ValidatedPreview;
    Ok(())
}

fn validate_initial_channel_grounding(
    evidence: &IntentRequestEvidenceChainV1,
    transcript: &[Message],
    workspace: &IntentWorkspaceV2,
    context: &IntentResolutionContext,
) -> Result<(), StructuredError> {
    let Some(IntentRequestEvidenceEntryV1::InitialHuman {
        transcript_message_index,
        ..
    }) = evidence.entries.first()
    else {
        return Err(evidence_error(
            "INTENT_REQUEST_EVIDENCE_ORDER_INVALID",
            "intent.request_evidence.entries",
            "Request evidence does not begin with initial_human",
            "Preserve the original ordered evidence chain",
        ));
    };
    let human_turn =
        transcript_human_turn(transcript, *transcript_message_index, INTENT_HUMAN_PREFIX)?;
    let options = context
        .channel_bindings
        .iter()
        .map(|key| key.as_str().to_string())
        .collect::<Vec<_>>();
    let selected_channel = initial_model_extracted_channel(workspace)?;
    let human_selection = deterministically_selected_option(&human_turn, &options);
    if human_selection == selected_channel {
        Ok(())
    } else {
        Err(evidence_error(
            "INTENT_REQUEST_EVIDENCE_INITIAL_SELECTION_MISMATCH",
            "intent.request_evidence.entries.human_turn_digest",
            "Initial deterministic human channel selection does not match the typed workspace channel",
            "Restore the exact bidirectional human selection and grounded workspace value",
        ))
    }
}

fn initial_model_extracted_channel(
    workspace: &IntentWorkspaceV2,
) -> Result<Option<String>, StructuredError> {
    let projection = serde_json::to_value(workspace).map_err(|error| {
        evidence_error(
            "INTENT_REQUEST_EVIDENCE_WORKSPACE_SERIALIZATION_FAILED",
            "intent.workspace",
            error.to_string(),
            "Restore a serializable typed workspace",
        )
    })?;
    let Some(hub_channel) = projection.pointer("/features/0/configuration/parameters/hub_channel")
    else {
        return Err(evidence_error(
            "INTENT_REQUEST_EVIDENCE_WORKSPACE_MISMATCH",
            "intent.workspace.features.0.configuration.parameters.hub_channel",
            "Initial private-study-room workspace has no channel field",
            "Restore the closed private-study-room workspace",
        ));
    };
    if hub_channel.is_null() {
        return Ok(None);
    }
    let source = hub_channel
        .get("source")
        .and_then(serde_json::Value::as_str);
    let value = hub_channel.get("value").and_then(serde_json::Value::as_str);
    match (source, value) {
        (Some("model_extracted"), Some(value)) if !value.is_empty() => Ok(Some(value.to_string())),
        _ => Err(evidence_error(
            "INTENT_REQUEST_EVIDENCE_WORKSPACE_MISMATCH",
            "intent.workspace.features.0.configuration.parameters.hub_channel",
            "Initial channel must be absent or carry nonempty model-extracted provenance",
            "Restore the deterministic initial workspace provenance",
        )),
    }
}

fn reverse_existing_channel_resolution(
    workspace: &mut IntentWorkspaceV2,
    accepted: &AcceptedIntentResolutionV1,
) -> Result<(), StructuredError> {
    let mut projection = serde_json::to_value(&*workspace).map_err(|error| {
        evidence_error(
            "INTENT_REQUEST_EVIDENCE_WORKSPACE_SERIALIZATION_FAILED",
            "intent.workspace",
            error.to_string(),
            "Restore a serializable typed workspace",
        )
    })?;
    let Some(hub_channel) =
        projection.pointer_mut("/features/0/configuration/parameters/hub_channel")
    else {
        return Err(evidence_error(
            "INTENT_REQUEST_EVIDENCE_WORKSPACE_MISMATCH",
            "intent.workspace.features.0.configuration.parameters.hub_channel",
            "Accepted existing-channel evidence has no final typed workspace value",
            "Restore the user-confirmed channel value",
        ));
    };
    if hub_channel
        .get("source")
        .and_then(serde_json::Value::as_str)
        != Some("user_confirmed")
        || hub_channel.get("value").and_then(serde_json::Value::as_str)
            != Some(accepted.option_value())
    {
        return Err(evidence_error(
            "INTENT_REQUEST_EVIDENCE_WORKSPACE_MISMATCH",
            "intent.workspace.features.0.configuration.parameters.hub_channel",
            "Final typed workspace value does not match the accepted user-confirmed channel",
            "Restore the exact accepted channel and its user-confirmed provenance",
        ));
    }
    *hub_channel = serde_json::Value::Null;
    let previous_revision = workspace.revision.checked_sub(1).ok_or_else(|| {
        evidence_error(
            "INTENT_REQUEST_EVIDENCE_WORKSPACE_REVISION_MISMATCH",
            "intent.workspace.revision",
            "Workspace revision cannot be reversed for accepted resolution evidence",
            "Restore a valid positive workspace revision",
        )
    })?;
    projection["revision"] = serde_json::Value::from(previous_revision);
    *workspace = serde_json::from_value(projection).map_err(|error| {
        evidence_error(
            "INTENT_REQUEST_EVIDENCE_WORKSPACE_SERIALIZATION_FAILED",
            "intent.workspace",
            error.to_string(),
            "Restore the closed private-study-room workspace",
        )
    })?;
    Ok(())
}

pub(super) fn human_turn_digest(human_turn: &str) -> Result<String, StructuredError> {
    let normalized = human_turn.replace("\r\n", "\n");
    digest_serializable(
        HUMAN_TURN_DIGEST_DOMAIN_V1,
        &HumanTurnDigestProjectionV1 { text: &normalized },
        "intent.request_evidence.human_turn",
    )
}

pub(super) fn active_options_digest(active_options: &[String]) -> Result<String, StructuredError> {
    let canonical = canonical_active_options(active_options)?;
    digest_serializable(
        ACTIVE_OPTIONS_DIGEST_DOMAIN_V1,
        &ActiveOptionsDigestProjectionV1 {
            options: &canonical,
        },
        "intent.request_evidence.active_options",
    )
}

fn standalone_initial_evidence_digest(
    transcript_message_index: u64,
    expected_revision: u64,
    human_turn_digest: &str,
) -> Result<String, StructuredError> {
    entry_digest(&IntentRequestEvidenceEntryV1::InitialHuman {
        transcript_message_index,
        expected_revision,
        human_turn_digest: human_turn_digest.to_string(),
    })
}

fn canonical_active_options(active_options: &[String]) -> Result<Vec<String>, StructuredError> {
    if active_options.is_empty() {
        return Err(evidence_error(
            "INTENT_REQUEST_EVIDENCE_OPTIONS_EMPTY",
            "intent.request_evidence.active_options",
            "An accepted resolution must reference at least one active option",
            "Use the active decision options from the awaiting stage",
        ));
    }
    let mut canonical = active_options.to_vec();
    if canonical.iter().any(|value| value.is_empty()) {
        return Err(evidence_error(
            "INTENT_REQUEST_EVIDENCE_OPTION_INVALID",
            "intent.request_evidence.active_options",
            "Active decision options must be nonempty typed values",
            "Use the exact active decision options from the awaiting stage",
        ));
    }
    canonical.sort();
    if canonical.windows(2).any(|values| values[0] == values[1]) {
        return Err(evidence_error(
            "INTENT_REQUEST_EVIDENCE_OPTION_DUPLICATE",
            "intent.request_evidence.active_options",
            "Active decision options must not contain duplicates",
            "Use each canonical active decision option once",
        ));
    }
    Ok(canonical)
}

fn validate_resolution_input(
    input: &AcceptedResolutionEvidenceInputV1<'_>,
) -> Result<(), StructuredError> {
    if input.turn_index == 0 {
        return Err(evidence_error(
            "INTENT_REQUEST_EVIDENCE_TURN_INVALID",
            "intent.request_evidence.turn_index",
            "Accepted resolution turn index must be positive",
            "Use the append-only human turn sequence number",
        ));
    }
    if input.decision_id.trim().is_empty() || input.decision_path.trim().is_empty() {
        return Err(evidence_error(
            "INTENT_REQUEST_EVIDENCE_DECISION_INVALID",
            "intent.request_evidence.decision",
            "Accepted resolution must bind a deterministic decision ID and path",
            "Use the active missing decision without rewriting it",
        ));
    }
    let canonical = canonical_active_options(input.active_options)?;
    let accepted = input.accepted_typed_value.option_value();
    if accepted.is_empty()
        || canonical
            .binary_search_by(|option| option.as_str().cmp(accepted))
            .is_err()
    {
        return Err(evidence_error(
            "INTENT_REQUEST_EVIDENCE_ACCEPTED_VALUE_INVALID",
            "intent.request_evidence.accepted_typed_value",
            "Accepted typed value is not one of the active decision options",
            "Accept exactly one active option after deterministic validation",
        ));
    }
    Ok(())
}

fn validate_terminal_finalization_input(
    input: &TerminalOutcomeFinalizationEvidenceInputV1<'_>,
) -> Result<(), StructuredError> {
    validate_workspace_revision_transition(
        input.prior_workspace_revision,
        input.next_workspace_revision,
    )?;
    for (digest, field) in [
        (
            input.standalone_request_evidence_digest,
            "standalone_request_evidence_digest",
        ),
        (
            input.standalone_adjudication_digest,
            "standalone_adjudication_digest",
        ),
        (
            input.standalone_recipe_evidence_digest,
            "standalone_recipe_evidence_digest",
        ),
    ] {
        validate_digest_shape(digest, field)?;
    }
    Ok(())
}

fn validate_workspace_revision_transition(
    prior_workspace_revision: u64,
    next_workspace_revision: u64,
) -> Result<(), StructuredError> {
    let expected_next = prior_workspace_revision.checked_add(1).ok_or_else(|| {
        evidence_error(
            "INTENT_REQUEST_EVIDENCE_WORKSPACE_REVISION_OVERFLOW",
            "intent.request_evidence.entries.next_workspace_revision",
            "Terminal finalization workspace revision exceeds the supported range",
            "Start a new intent session before finalizing the working draft",
        )
    })?;
    if next_workspace_revision != expected_next {
        return Err(evidence_error(
            "INTENT_REQUEST_EVIDENCE_FINALIZATION_REVISION_INVALID",
            "intent.request_evidence.entries.next_workspace_revision",
            "Terminal finalization must advance the workspace revision exactly once",
            "Bind the exact working-draft and validated-preview workspace revisions",
        ));
    }
    Ok(())
}

fn validate_entry_shape(
    entry: &IntentRequestEvidenceEntryV1,
    must_be_initial: bool,
) -> Result<(), StructuredError> {
    match entry {
        IntentRequestEvidenceEntryV1::InitialHuman {
            human_turn_digest, ..
        } => {
            if !must_be_initial {
                return Err(evidence_error(
                    "INTENT_REQUEST_EVIDENCE_ORDER_INVALID",
                    "intent.request_evidence.entries",
                    "Only the first request evidence entry may be initial_human",
                    "Preserve the original ordered evidence chain",
                ));
            }
            validate_digest_shape(human_turn_digest, "human_turn_digest")
        }
        IntentRequestEvidenceEntryV1::AcceptedResolution {
            previous_chain_head,
            turn_index,
            decision_id,
            decision_path,
            active_options_digest,
            human_turn_digest,
            accepted_typed_value,
            ..
        } => {
            if must_be_initial {
                return Err(evidence_error(
                    "INTENT_REQUEST_EVIDENCE_ORDER_INVALID",
                    "intent.request_evidence.entries",
                    "Request evidence must begin with initial_human",
                    "Preserve the original ordered evidence chain",
                ));
            }
            validate_digest_shape(previous_chain_head, "previous_chain_head")?;
            validate_digest_shape(active_options_digest, "active_options_digest")?;
            validate_digest_shape(human_turn_digest, "human_turn_digest")?;
            if *turn_index == 0 || decision_id.trim().is_empty() || decision_path.trim().is_empty()
            {
                return Err(evidence_error(
                    "INTENT_REQUEST_EVIDENCE_ENTRY_INVALID",
                    "intent.request_evidence.entries",
                    "Accepted resolution evidence has an invalid turn or decision identity",
                    "Restore the exact accepted resolution evidence",
                ));
            }
            if accepted_typed_value.option_value().is_empty() {
                return Err(evidence_error(
                    "INTENT_REQUEST_EVIDENCE_ACCEPTED_VALUE_INVALID",
                    "intent.request_evidence.entries.accepted_typed_value",
                    "Accepted typed value must be nonempty",
                    "Restore the exact accepted typed decision value",
                ));
            }
            Ok(())
        }
        IntentRequestEvidenceEntryV1::TerminalOutcomeFinalization {
            previous_chain_head,
            prior_workspace_revision,
            next_workspace_revision,
            prior_requested_outcome,
            next_requested_outcome,
            human_turn_digest,
            standalone_request_evidence_digest,
            standalone_adjudication_digest,
            standalone_recipe_evidence_digest,
            ..
        } => {
            if must_be_initial {
                return Err(evidence_error(
                    "INTENT_REQUEST_EVIDENCE_ORDER_INVALID",
                    "intent.request_evidence.entries",
                    "Request evidence must begin with initial_human",
                    "Preserve the original ordered evidence chain",
                ));
            }
            validate_digest_shape(previous_chain_head, "previous_chain_head")?;
            validate_digest_shape(human_turn_digest, "human_turn_digest")?;
            validate_digest_shape(
                standalone_request_evidence_digest,
                "standalone_request_evidence_digest",
            )?;
            validate_digest_shape(
                standalone_adjudication_digest,
                "standalone_adjudication_digest",
            )?;
            validate_digest_shape(
                standalone_recipe_evidence_digest,
                "standalone_recipe_evidence_digest",
            )?;
            validate_workspace_revision_transition(
                *prior_workspace_revision,
                *next_workspace_revision,
            )?;
            if *prior_requested_outcome != IntentRequestedOutcome::WorkingDraft
                || *next_requested_outcome != IntentRequestedOutcome::ValidatedPreview
            {
                return Err(evidence_error(
                    "INTENT_REQUEST_EVIDENCE_FINALIZATION_OUTCOME_INVALID",
                    "intent.request_evidence.entries.next_requested_outcome",
                    "Terminal finalization must be the fixed working-draft to validated-preview transition",
                    "Restore the exact terminal outcome transition",
                ));
            }
            Ok(())
        }
    }
}

fn validate_append_shape(
    previous_entries: &[IntentRequestEvidenceEntryV1],
    entry: &IntentRequestEvidenceEntryV1,
) -> Result<(), StructuredError> {
    let Some(previous) = previous_entries.last() else {
        return Err(evidence_error(
            "INTENT_REQUEST_EVIDENCE_EMPTY",
            "intent.request_evidence.entries",
            "Appended evidence cannot precede initial human evidence",
            "Create initial human evidence first",
        ));
    };
    if matches!(
        previous,
        IntentRequestEvidenceEntryV1::TerminalOutcomeFinalization { .. }
    ) {
        return Err(evidence_error(
            "INTENT_REQUEST_EVIDENCE_FINALIZATION_NOT_TERMINAL",
            "intent.request_evidence.entries",
            "Terminal outcome finalization must be the final request evidence entry",
            "Remove all evidence appended after terminal finalization",
        ));
    }
    let transcript_message_index = entry.transcript_message_index();
    if transcript_message_index <= previous.transcript_message_index() {
        return Err(evidence_error(
            "INTENT_REQUEST_EVIDENCE_TRANSCRIPT_ORDER_INVALID",
            "intent.request_evidence.entries.transcript_message_index",
            "Request evidence transcript indices must increase",
            "Preserve append-only transcript ordering",
        ));
    }
    match entry {
        IntentRequestEvidenceEntryV1::AcceptedResolution {
            turn_index,
            expected_revision,
            decision_id,
            decision_path,
            ..
        } => {
            let Some(previous_expected_revision) = previous.resolution_expected_revision() else {
                return Err(evidence_error(
                    "INTENT_REQUEST_EVIDENCE_FINALIZATION_NOT_TERMINAL",
                    "intent.request_evidence.entries",
                    "Accepted resolution cannot follow terminal outcome finalization",
                    "Preserve terminal finalization as the final evidence entry",
                ));
            };
            if *expected_revision < previous_expected_revision {
                return Err(evidence_error(
                    "INTENT_REQUEST_EVIDENCE_REVISION_INVALID",
                    "intent.request_evidence.entries.expected_revision",
                    "Accepted resolution revision cannot precede prior evidence",
                    "Use the active decision expected revision",
                ));
            }
            let previous_turn = previous_entries.iter().rev().find_map(|value| match value {
                IntentRequestEvidenceEntryV1::AcceptedResolution { turn_index, .. } => {
                    Some(*turn_index)
                }
                IntentRequestEvidenceEntryV1::InitialHuman { .. }
                | IntentRequestEvidenceEntryV1::TerminalOutcomeFinalization { .. } => None,
            });
            if previous_turn.is_some_and(|value| *turn_index <= value) {
                return Err(evidence_error(
                    "INTENT_REQUEST_EVIDENCE_TURN_ORDER_INVALID",
                    "intent.request_evidence.entries.turn_index",
                    "Accepted resolution turn indices must increase",
                    "Preserve append-only human turn ordering",
                ));
            }
            if previous_entries.iter().any(|value| {
                matches!(
                    value,
                    IntentRequestEvidenceEntryV1::AcceptedResolution {
                        decision_id: existing_id,
                        decision_path: existing_path,
                        ..
                    } if existing_id == decision_id || existing_path == decision_path
                )
            }) {
                return Err(evidence_error(
                    "INTENT_REQUEST_EVIDENCE_DECISION_DUPLICATE",
                    "intent.request_evidence.entries.decision_id",
                    "A deterministic decision may be accepted only once",
                    "Preserve only the first accepted value for each decision",
                ));
            }
            Ok(())
        }
        IntentRequestEvidenceEntryV1::TerminalOutcomeFinalization {
            prior_workspace_revision,
            ..
        } => {
            if previous_entries.iter().any(|value| {
                matches!(
                    value,
                    IntentRequestEvidenceEntryV1::TerminalOutcomeFinalization { .. }
                )
            }) {
                return Err(evidence_error(
                    "INTENT_REQUEST_EVIDENCE_FINALIZATION_DUPLICATE",
                    "intent.request_evidence.entries",
                    "Terminal outcome finalization may appear only once",
                    "Preserve only the first terminal finalization entry",
                ));
            }
            let accepted_count = previous_entries
                .iter()
                .filter(|value| {
                    matches!(
                        value,
                        IntentRequestEvidenceEntryV1::AcceptedResolution { .. }
                    )
                })
                .count();
            let expected_prior_revision = u64::try_from(accepted_count)
                .ok()
                .and_then(|value| value.checked_add(1))
                .ok_or_else(|| {
                    evidence_error(
                        "INTENT_REQUEST_EVIDENCE_OVERFLOW",
                        "intent.request_evidence.entries.prior_workspace_revision",
                        "Request evidence workspace revision exceeds the supported range",
                        "Start a new intent session",
                    )
                })?;
            if *prior_workspace_revision != expected_prior_revision {
                return Err(evidence_error(
                    "INTENT_REQUEST_EVIDENCE_FINALIZATION_REVISION_INVALID",
                    "intent.request_evidence.entries.prior_workspace_revision",
                    "Terminal finalization prior revision does not match the accepted resolution history",
                    "Bind finalization to the exact current working-draft workspace revision",
                ));
            }
            Ok(())
        }
        IntentRequestEvidenceEntryV1::InitialHuman { .. } => Err(evidence_error(
            "INTENT_REQUEST_EVIDENCE_ORDER_INVALID",
            "intent.request_evidence.entries",
            "Only the first request evidence entry may be initial_human",
            "Preserve the original ordered evidence chain",
        )),
    }
}

fn transcript_human_turn(
    transcript: &[Message],
    transcript_message_index: u64,
    prefix: &str,
) -> Result<String, StructuredError> {
    if prefix.is_empty() {
        return Err(evidence_error(
            "INTENT_REQUEST_EVIDENCE_PREFIX_INVALID",
            "intent.request_evidence.prefix",
            "Human transcript envelope prefix must be nonempty",
            "Use the exact harness-owned INTENT_HUMAN prefix",
        ));
    }
    let index = usize::try_from(transcript_message_index).map_err(|_| {
        evidence_error(
            "INTENT_REQUEST_EVIDENCE_TRANSCRIPT_INDEX_INVALID",
            "intent.request_evidence.transcript_message_index",
            "Transcript message index cannot be represented on this platform",
            "Restore a valid architecture-independent transcript index",
        )
    })?;
    let message = transcript.get(index).ok_or_else(|| {
        evidence_error(
            "INTENT_REQUEST_EVIDENCE_TRANSCRIPT_INDEX_INVALID",
            "intent.request_evidence.transcript_message_index",
            "Request evidence references a missing transcript message",
            "Restore the original append-only intent transcript",
        )
    })?;
    if message.role != MessageRole::User
        || message.tool_call_id.is_some()
        || !message.tool_calls.is_empty()
    {
        return Err(evidence_error(
            "INTENT_REQUEST_EVIDENCE_TRANSCRIPT_ROLE_INVALID",
            "intent.request_evidence.transcript_message_index",
            "Request evidence must reference a plain user transcript message",
            "Reference the matching INTENT_HUMAN envelope",
        ));
    }
    let envelope = message.content.strip_prefix(prefix).ok_or_else(|| {
        evidence_error(
            "INTENT_REQUEST_EVIDENCE_ENVELOPE_INVALID",
            "intent.request_evidence.transcript_message_index",
            "Referenced transcript message is not an INTENT_HUMAN envelope",
            "Reference the exact harness-owned human message envelope",
        )
    })?;
    serde_json::from_str::<IntentHumanEnvelopeV1>(envelope)
        .map(|value| value.text)
        .map_err(|error| {
            evidence_error(
                "INTENT_REQUEST_EVIDENCE_ENVELOPE_INVALID",
                "intent.request_evidence.transcript_message_index",
                error.to_string(),
                "Restore a valid JSON envelope containing only text",
            )
        })
}

fn entry_digest(entry: &IntentRequestEvidenceEntryV1) -> Result<String, StructuredError> {
    let domain = match entry {
        IntentRequestEvidenceEntryV1::InitialHuman { .. } => INITIAL_EVIDENCE_DIGEST_DOMAIN_V1,
        IntentRequestEvidenceEntryV1::AcceptedResolution { .. } => {
            RESOLUTION_EVIDENCE_DIGEST_DOMAIN_V1
        }
        IntentRequestEvidenceEntryV1::TerminalOutcomeFinalization { .. } => {
            TERMINAL_FINALIZATION_EVIDENCE_DIGEST_DOMAIN_V1
        }
    };
    digest_serializable(domain, entry, "intent.request_evidence.entries")
}

fn digest_serializable<T: Serialize>(
    domain: &[u8],
    value: &T,
    location: &str,
) -> Result<String, StructuredError> {
    canonical_json_digest(
        domain,
        value,
        IdentityErrorSpec::new(
            "INTENT_REQUEST_EVIDENCE_SERIALIZATION_FAILED",
            location,
            "Canonical request evidence could not be serialized",
        ),
    )
}

fn validate_digest_shape(value: &str, field: &str) -> Result<(), StructuredError> {
    if is_lowercase_sha256_hex(value) {
        return Ok(());
    }
    Err(evidence_error(
        "INTENT_REQUEST_EVIDENCE_DIGEST_INVALID",
        format!("intent.request_evidence.entries.{field}"),
        "Request evidence digest must be 64 lowercase hexadecimal characters",
        "Restore a digest created by the V4 request evidence implementation",
    ))
}

fn evidence_error(
    code: impl Into<String>,
    location: impl Into<String>,
    message: impl Into<String>,
    hint: impl Into<String>,
) -> StructuredError {
    StructuredError::new(code, location, message, hint)
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;
    use crate::intent::{
        prepare_private_study_room, IntentLocaleV1, IntentRequestedOutcome,
        PrivateStudyRoomControlsProposalV1, PrivateStudyRoomCopyProposalV1,
        PrivateStudyRoomNamingProposalV1, PrivateStudyRoomProposalV2,
    };

    fn envelope(text: &str) -> Message {
        Message::user(format!("{INTENT_HUMAN_PREFIX}{}", json!({"text": text})))
    }

    fn transcript() -> Vec<Message> {
        vec![
            Message::system("system"),
            envelope("개인 스터디룸을 만들어줘\r\nstudy_hub는 아직 선택 안 했어"),
            Message::user("INTENT_STATE:{}"),
            Message::assistant_tool_calls(Vec::new()),
            envelope("study_hub로 해줘"),
        ]
    }

    fn finalization_transcript() -> Vec<Message> {
        vec![
            Message::system("system"),
            envelope("study_hub에 개인 스터디룸 작업 초안을 만들어줘"),
            Message::user("INTENT_STATE:{}"),
            Message::assistant_tool_calls(Vec::new()),
            envelope("이 작업 초안을 검증된 미리보기로 확정해줘"),
            Message::user("INTENT_STATE:{}"),
            envelope("다시 확정해줘"),
        ]
    }

    fn finalization_input(
        standalone_request_evidence_digest: &str,
    ) -> TerminalOutcomeFinalizationEvidenceInputV1<'_> {
        TerminalOutcomeFinalizationEvidenceInputV1 {
            transcript_message_index: 4,
            expected_draft_revision: 22,
            prior_workspace_revision: 1,
            next_workspace_revision: 2,
            standalone_request_evidence_digest,
            standalone_adjudication_digest:
                "2222222222222222222222222222222222222222222222222222222222222222",
            standalone_recipe_evidence_digest:
                "3333333333333333333333333333333333333333333333333333333333333333",
        }
    }

    fn finalization_request_digest(
        transcript: &[Message],
        transcript_message_index: u64,
        expected_draft_revision: u64,
    ) -> String {
        let human_turn =
            transcript_human_turn(transcript, transcript_message_index, INTENT_HUMAN_PREFIX)
                .unwrap();
        standalone_initial_evidence_digest(
            transcript_message_index,
            expected_draft_revision,
            &human_turn_digest(&human_turn).unwrap(),
        )
        .unwrap()
    }

    fn resolution_input<'a>(options: &'a [String]) -> AcceptedResolutionEvidenceInputV1<'a> {
        AcceptedResolutionEvidenceInputV1 {
            turn_index: 1,
            transcript_message_index: 4,
            expected_revision: 7,
            decision_id: "private_study_room.hub_channel",
            decision_path: "features.private_study_room.parameters.hub_channel",
            active_options: options,
            accepted_typed_value: AcceptedIntentResolutionV1::ExistingChannel(
                "study_hub".to_string(),
            ),
        }
    }

    fn resolved_workspace() -> (IntentResolutionContext, IntentWorkspaceV2) {
        let context = IntentResolutionContext::from_channel_bindings([
            ExistingChannelKey("community".to_string()),
            ExistingChannelKey("study_hub".to_string()),
        ]);
        let proposal = PrivateStudyRoomProposalV2 {
            requested_outcome: IntentRequestedOutcome::ValidatedPreview,
            hub_channel: None,
            locale: Some(IntentLocaleV1::Ko),
            copy: PrivateStudyRoomCopyProposalV1::default(),
            naming: PrivateStudyRoomNamingProposalV1::default(),
            controls: PrivateStudyRoomControlsProposalV1::default(),
        };
        let PreparedIntentWorkspaceV2::NeedsInput { workspace, .. } =
            prepare_private_study_room(proposal, &context).unwrap()
        else {
            panic!("expected missing channel decision")
        };
        let PreparedIntentWorkspaceV2::Resolved { workspace, .. } =
            apply_existing_channel_decision(
                &workspace,
                1,
                ExistingChannelKey("study_hub".to_string()),
                &context,
            )
            .unwrap()
        else {
            panic!("expected resolved workspace")
        };
        (context, workspace)
    }

    fn one_shot_workspace() -> (IntentResolutionContext, IntentWorkspaceV2) {
        let context = IntentResolutionContext::from_channel_bindings([
            ExistingChannelKey("community".to_string()),
            ExistingChannelKey("study_hub".to_string()),
        ]);
        let proposal = PrivateStudyRoomProposalV2 {
            requested_outcome: IntentRequestedOutcome::ValidatedPreview,
            hub_channel: Some(ExistingChannelKey("study_hub".to_string())),
            locale: Some(IntentLocaleV1::Ko),
            copy: PrivateStudyRoomCopyProposalV1::default(),
            naming: PrivateStudyRoomNamingProposalV1::default(),
            controls: PrivateStudyRoomControlsProposalV1::default(),
        };
        let PreparedIntentWorkspaceV2::Resolved { workspace, .. } =
            prepare_private_study_room(proposal, &context).unwrap()
        else {
            panic!("expected one-shot workspace")
        };
        (context, workspace)
    }

    fn rehash_resolution_chain(chain: &mut IntentRequestEvidenceChainV1, transcript: &[Message]) {
        let initial_head = entry_digest(&chain.entries[0]).unwrap();
        let IntentRequestEvidenceEntryV1::AcceptedResolution {
            previous_chain_head,
            transcript_message_index,
            human_turn_digest: persisted_human_digest,
            ..
        } = &mut chain.entries[1]
        else {
            panic!("expected accepted resolution")
        };
        *previous_chain_head = initial_head;
        let human =
            transcript_human_turn(transcript, *transcript_message_index, INTENT_HUMAN_PREFIX)
                .unwrap();
        *persisted_human_digest = human_turn_digest(&human).unwrap();
        chain.head = entry_digest(&chain.entries[1]).unwrap();
    }

    fn rehash_terminal_chain(chain: &mut IntentRequestEvidenceChainV1) {
        let final_index = chain.entries.len() - 1;
        let previous_head = entry_digest(&chain.entries[final_index - 1]).unwrap();
        let IntentRequestEvidenceEntryV1::TerminalOutcomeFinalization {
            previous_chain_head,
            ..
        } = &mut chain.entries[final_index]
        else {
            panic!("expected terminal finalization")
        };
        *previous_chain_head = previous_head;
        chain.head = entry_digest(&chain.entries[final_index]).unwrap();
    }

    #[test]
    fn human_turn_digest_normalizes_only_crlf() {
        assert_eq!(
            human_turn_digest("Alpha\r\n한글").unwrap(),
            human_turn_digest("Alpha\n한글").unwrap()
        );
        assert_ne!(
            human_turn_digest("Alpha\n한글").unwrap(),
            human_turn_digest("alpha\n한글").unwrap()
        );
        assert_ne!(
            human_turn_digest("Alpha\n한글").unwrap(),
            human_turn_digest("Alpha \n한글").unwrap()
        );
        assert_ne!(
            human_turn_digest("Alpha\r한글").unwrap(),
            human_turn_digest("Alpha\n한글").unwrap()
        );
    }

    #[test]
    fn active_options_are_order_independent_and_duplicates_reject() {
        let left = vec!["study_hub".to_string(), "general".to_string()];
        let right = vec!["general".to_string(), "study_hub".to_string()];
        assert_eq!(
            active_options_digest(&left).unwrap(),
            active_options_digest(&right).unwrap()
        );
        let duplicates = vec!["study_hub".to_string(), "study_hub".to_string()];
        assert_eq!(
            active_options_digest(&duplicates).unwrap_err().code,
            "INTENT_REQUEST_EVIDENCE_OPTION_DUPLICATE"
        );
    }

    #[test]
    fn accepted_resolution_builds_and_validates_ordered_chain() {
        let transcript = transcript();
        let options = vec!["general".to_string(), "study_hub".to_string()];
        let mut chain =
            IntentRequestEvidenceChainV1::from_initial_human(&transcript, 1, 7).unwrap();
        let initial_head = chain.head().to_string();
        chain
            .append_accepted_resolution(&transcript, resolution_input(&options))
            .unwrap();
        assert_eq!(chain.entries().len(), 2);
        assert_eq!(chain.accepted_resolution_count(), 1);
        assert_eq!(chain.initial_head().unwrap(), initial_head);
        assert_ne!(chain.head(), initial_head);
        assert_eq!(chain.head().len(), 64);
        assert!(chain.validate().is_ok());
        assert!(chain.validate_against_transcript(&transcript).is_ok());
    }

    #[test]
    fn terminal_finalization_binds_transition_and_standalone_evidence() {
        let transcript = finalization_transcript();
        let mut chain =
            IntentRequestEvidenceChainV1::from_initial_human(&transcript, 1, 21).unwrap();
        let previous_head = chain.head().to_string();
        let standalone_request_digest = finalization_request_digest(&transcript, 4, 22);
        chain
            .append_terminal_outcome_finalization(
                &transcript,
                finalization_input(&standalone_request_digest),
            )
            .unwrap();
        assert_eq!(chain.entries().len(), 2);
        assert_eq!(chain.accepted_resolution_count(), 0);
        assert_eq!(chain.terminal_outcome_finalization_count(), 1);
        assert_ne!(chain.head(), previous_head);
        let finalization = chain.terminal_outcome_finalization().unwrap();
        assert_eq!(finalization.previous_chain_head(), previous_head);
        assert_eq!(finalization.transcript_message_index(), 4);
        assert_eq!(finalization.expected_draft_revision(), 22);
        assert_eq!(finalization.prior_workspace_revision(), 1);
        assert_eq!(finalization.next_workspace_revision(), 2);
        assert_eq!(
            finalization.human_turn_digest(),
            human_turn_digest("이 작업 초안을 검증된 미리보기로 확정해줘").unwrap()
        );
        assert_eq!(
            finalization.standalone_request_evidence_digest(),
            standalone_request_digest
        );
        assert_eq!(
            finalization.standalone_adjudication_digest(),
            finalization_input(&standalone_request_digest).standalone_adjudication_digest
        );
        assert_eq!(
            finalization.standalone_recipe_evidence_digest(),
            finalization_input(&standalone_request_digest).standalone_recipe_evidence_digest
        );
        assert!(chain.validate().is_ok());
        assert!(chain.validate_against_transcript(&transcript).is_ok());
        let serialized = serde_json::to_value(&chain).unwrap();
        assert_eq!(
            serialized["entries"][1]["previous_chain_head"],
            previous_head
        );
        assert_eq!(
            serialized["entries"][1]["prior_requested_outcome"],
            "working_draft"
        );
        assert_eq!(
            serialized["entries"][1]["next_requested_outcome"],
            "validated_preview"
        );
    }

    #[test]
    fn terminal_finalization_reverses_and_replays_workspace_transition() {
        let transcript = finalization_transcript();
        let mut chain =
            IntentRequestEvidenceChainV1::from_initial_human(&transcript, 1, 21).unwrap();
        let standalone_request_digest = finalization_request_digest(&transcript, 4, 22);
        chain
            .append_terminal_outcome_finalization(
                &transcript,
                finalization_input(&standalone_request_digest),
            )
            .unwrap();
        let (context, mut final_workspace) = one_shot_workspace();
        final_workspace.revision = 2;
        let initial_workspace = chain.initial_workspace(&final_workspace).unwrap();
        assert_eq!(initial_workspace.revision, 1);
        assert_eq!(
            initial_workspace.requested_outcome,
            IntentRequestedOutcome::WorkingDraft
        );
        chain
            .validate_resolutions_against_workspace(&transcript, &final_workspace, &context)
            .unwrap();
    }

    #[test]
    fn terminal_finalization_reverses_before_accepted_resolution_history() {
        let mut transcript = transcript();
        transcript.push(Message::user("INTENT_STATE:{}"));
        transcript.push(envelope("이 작업 초안을 검증된 미리보기로 확정해줘"));
        let options = vec!["community".to_string(), "study_hub".to_string()];
        let mut chain =
            IntentRequestEvidenceChainV1::from_initial_human(&transcript, 1, 0).unwrap();
        let mut resolution = resolution_input(&options);
        resolution.expected_revision = 1;
        resolution.decision_path = "intent.features.0.configuration.parameters.hub_channel";
        chain
            .append_accepted_resolution(&transcript, resolution)
            .unwrap();
        let standalone_request_digest = finalization_request_digest(&transcript, 6, 22);
        let mut finalization = finalization_input(&standalone_request_digest);
        finalization.transcript_message_index = 6;
        finalization.prior_workspace_revision = 2;
        finalization.next_workspace_revision = 3;
        chain
            .append_terminal_outcome_finalization(&transcript, finalization)
            .unwrap();
        let (context, mut final_workspace) = resolved_workspace();
        final_workspace.revision = 3;
        let initial_workspace = chain.initial_workspace(&final_workspace).unwrap();
        assert_eq!(initial_workspace.revision, 1);
        assert_eq!(
            initial_workspace.requested_outcome,
            IntentRequestedOutcome::WorkingDraft
        );
        chain
            .validate_resolutions_against_workspace(&transcript, &final_workspace, &context)
            .unwrap();
    }

    #[test]
    fn terminal_finalization_is_unique_and_terminal() {
        let transcript = finalization_transcript();
        let mut chain =
            IntentRequestEvidenceChainV1::from_initial_human(&transcript, 1, 21).unwrap();
        let standalone_request_digest = finalization_request_digest(&transcript, 4, 22);
        chain
            .append_terminal_outcome_finalization(
                &transcript,
                finalization_input(&standalone_request_digest),
            )
            .unwrap();
        let second_request_digest = finalization_request_digest(&transcript, 6, 22);
        let mut second = finalization_input(&second_request_digest);
        second.transcript_message_index = 6;
        second.prior_workspace_revision = 2;
        second.next_workspace_revision = 3;
        assert_eq!(
            chain
                .append_terminal_outcome_finalization(&transcript, second)
                .unwrap_err()
                .code,
            "INTENT_REQUEST_EVIDENCE_FINALIZATION_NOT_TERMINAL"
        );

        let options = vec!["study_hub".to_string()];
        let mut resolution = resolution_input(&options);
        resolution.transcript_message_index = 6;
        resolution.expected_revision = 22;
        assert_eq!(
            chain
                .append_accepted_resolution(&transcript, resolution)
                .unwrap_err()
                .code,
            "INTENT_REQUEST_EVIDENCE_FINALIZATION_NOT_TERMINAL"
        );
    }

    #[test]
    fn terminal_finalization_semantic_and_digest_tampering_rejects() {
        let transcript = finalization_transcript();
        let mut baseline =
            IntentRequestEvidenceChainV1::from_initial_human(&transcript, 1, 21).unwrap();
        let standalone_request_digest = finalization_request_digest(&transcript, 4, 22);
        baseline
            .append_terminal_outcome_finalization(
                &transcript,
                finalization_input(&standalone_request_digest),
            )
            .unwrap();
        for pointer in [
            "/entries/1/previous_chain_head",
            "/entries/1/transcript_message_index",
            "/entries/1/expected_draft_revision",
            "/entries/1/prior_workspace_revision",
            "/entries/1/next_workspace_revision",
            "/entries/1/prior_requested_outcome",
            "/entries/1/next_requested_outcome",
            "/entries/1/human_turn_digest",
            "/entries/1/standalone_request_evidence_digest",
            "/entries/1/standalone_adjudication_digest",
            "/entries/1/standalone_recipe_evidence_digest",
            "/head",
        ] {
            let mut value = serde_json::to_value(&baseline).unwrap();
            let target = value.pointer_mut(pointer).unwrap();
            *target = match target {
                serde_json::Value::Number(_) => json!(99),
                _ => json!("tampered"),
            };
            if let Ok(tampered) = serde_json::from_value::<IntentRequestEvidenceChainV1>(value) {
                assert!(tampered.validate().is_err(), "tamper accepted at {pointer}");
            }
        }

        let mut changed_outcome = baseline.clone();
        let IntentRequestEvidenceEntryV1::TerminalOutcomeFinalization {
            next_requested_outcome,
            ..
        } = &mut changed_outcome.entries[1]
        else {
            panic!("expected terminal finalization")
        };
        *next_requested_outcome = IntentRequestedOutcome::WorkingDraft;
        rehash_terminal_chain(&mut changed_outcome);
        assert_eq!(
            changed_outcome.validate().unwrap_err().code,
            "INTENT_REQUEST_EVIDENCE_FINALIZATION_OUTCOME_INVALID"
        );

        let mut changed_revision = baseline;
        let IntentRequestEvidenceEntryV1::TerminalOutcomeFinalization {
            next_workspace_revision,
            ..
        } = &mut changed_revision.entries[1]
        else {
            panic!("expected terminal finalization")
        };
        *next_workspace_revision = 9;
        rehash_terminal_chain(&mut changed_revision);
        assert_eq!(
            changed_revision.validate().unwrap_err().code,
            "INTENT_REQUEST_EVIDENCE_FINALIZATION_REVISION_INVALID"
        );
    }

    #[test]
    fn chain_stores_no_raw_human_request_or_model_question() {
        let transcript = transcript();
        let options = vec!["study_hub".to_string()];
        let mut chain =
            IntentRequestEvidenceChainV1::from_initial_human(&transcript, 1, 7).unwrap();
        chain
            .append_accepted_resolution(&transcript, resolution_input(&options))
            .unwrap();
        let serialized = serde_json::to_string(&chain).unwrap();
        assert!(!serialized.contains("개인 스터디룸을 만들어줘"));
        assert!(!serialized.contains("어느 채널을 허브로 사용할까요"));
        assert!(!serialized.contains("study_hub로 해줘"));
    }

    #[test]
    fn rejected_resolution_does_not_advance_chain() {
        let transcript = transcript();
        let options = vec!["general".to_string()];
        let mut chain =
            IntentRequestEvidenceChainV1::from_initial_human(&transcript, 1, 7).unwrap();
        let before = chain.clone();
        let error = chain
            .append_accepted_resolution(&transcript, resolution_input(&options))
            .unwrap_err();
        assert_eq!(error.code, "INTENT_REQUEST_EVIDENCE_ACCEPTED_VALUE_INVALID");
        assert_eq!(chain, before);
    }

    #[test]
    fn transcript_content_role_index_and_envelope_are_verified() {
        let original = transcript();
        let chain = IntentRequestEvidenceChainV1::from_initial_human(&original, 1, 7).unwrap();
        let mut changed = original.clone();
        changed[1] = envelope("다른 요청");
        assert_eq!(
            chain
                .validate_against_transcript(&changed)
                .unwrap_err()
                .code,
            "INTENT_REQUEST_EVIDENCE_TRANSCRIPT_MISMATCH"
        );
        let mut wrong_role = original.clone();
        wrong_role[1] = Message::assistant(original[1].content.clone());
        assert_eq!(
            chain
                .validate_against_transcript(&wrong_role)
                .unwrap_err()
                .code,
            "INTENT_REQUEST_EVIDENCE_TRANSCRIPT_ROLE_INVALID"
        );
        let mut wrong_envelope = original.clone();
        wrong_envelope[1] = Message::user("INTENT_STATE:{}");
        assert_eq!(
            chain
                .validate_against_transcript(&wrong_envelope)
                .unwrap_err()
                .code,
            "INTENT_REQUEST_EVIDENCE_ENVELOPE_INVALID"
        );
    }

    #[test]
    fn unknown_envelope_fields_reject() {
        let transcript = vec![Message::user(format!(
            "{INTENT_HUMAN_PREFIX}{}",
            json!({"text": "hello", "question": "model prose"})
        ))];
        assert_eq!(
            IntentRequestEvidenceChainV1::from_initial_human(&transcript, 0, 0)
                .unwrap_err()
                .code,
            "INTENT_REQUEST_EVIDENCE_ENVELOPE_INVALID"
        );
    }

    #[test]
    fn entry_and_head_tampering_rejects() {
        let transcript = transcript();
        let options = vec!["general".to_string(), "study_hub".to_string()];
        let mut chain =
            IntentRequestEvidenceChainV1::from_initial_human(&transcript, 1, 7).unwrap();
        chain
            .append_accepted_resolution(&transcript, resolution_input(&options))
            .unwrap();
        for pointer in [
            "/entries/1/turn_index",
            "/entries/1/transcript_message_index",
            "/entries/1/expected_revision",
            "/entries/1/decision_id",
            "/entries/1/decision_path",
            "/entries/1/active_options_digest",
            "/entries/1/human_turn_digest",
            "/entries/1/accepted_typed_value/value",
            "/head",
        ] {
            let mut value = serde_json::to_value(&chain).unwrap();
            let target = value.pointer_mut(pointer).unwrap();
            *target = match target {
                serde_json::Value::Number(_) => json!(99),
                _ => json!("tampered"),
            };
            let tampered: IntentRequestEvidenceChainV1 = serde_json::from_value(value).unwrap();
            assert!(tampered.validate().is_err(), "tamper accepted at {pointer}");
        }
    }

    #[test]
    fn duplicate_or_reordered_entries_reject() {
        let transcript = transcript();
        let options = vec!["study_hub".to_string()];
        let mut chain =
            IntentRequestEvidenceChainV1::from_initial_human(&transcript, 1, 7).unwrap();
        chain
            .append_accepted_resolution(&transcript, resolution_input(&options))
            .unwrap();
        let mut duplicated = chain.clone();
        duplicated.entries.push(duplicated.entries[1].clone());
        assert!(duplicated.validate().is_err());
        let mut reordered = chain;
        reordered.entries.swap(0, 1);
        assert!(reordered.validate().is_err());
    }

    #[test]
    fn rehashed_false_resolution_provenance_rejects_against_human_and_workspace() {
        let mut transcript = transcript();
        transcript[4] = envelope("Use study_hub");
        let options = vec!["community".to_string(), "study_hub".to_string()];
        let mut chain =
            IntentRequestEvidenceChainV1::from_initial_human(&transcript, 1, 0).unwrap();
        let mut input = resolution_input(&options);
        input.expected_revision = 1;
        input.decision_path = "intent.features.0.configuration.parameters.hub_channel";
        chain
            .append_accepted_resolution(&transcript, input)
            .unwrap();
        let (context, workspace) = resolved_workspace();
        chain
            .validate_resolutions_against_workspace(&transcript, &workspace, &context)
            .unwrap();

        transcript[4] = envelope("Use community");
        rehash_resolution_chain(&mut chain, &transcript);
        assert!(chain.validate_against_transcript(&transcript).is_ok());
        assert_eq!(
            chain
                .validate_resolutions_against_workspace(&transcript, &workspace, &context)
                .unwrap_err()
                .code,
            "INTENT_REQUEST_EVIDENCE_HUMAN_SELECTION_MISMATCH"
        );

        let IntentRequestEvidenceEntryV1::AcceptedResolution {
            accepted_typed_value,
            ..
        } = &mut chain.entries[1]
        else {
            panic!("expected accepted resolution")
        };
        *accepted_typed_value =
            AcceptedIntentResolutionV1::ExistingChannel("community".to_string());
        rehash_resolution_chain(&mut chain, &transcript);
        assert!(chain.validate_against_transcript(&transcript).is_ok());
        assert_eq!(
            chain
                .validate_resolutions_against_workspace(&transcript, &workspace, &context)
                .unwrap_err()
                .code,
            "INTENT_REQUEST_EVIDENCE_WORKSPACE_MISMATCH"
        );
    }

    #[test]
    fn rehashed_initial_human_cannot_rebind_a_model_extracted_channel() {
        let mut transcript = transcript();
        transcript[1] = envelope("Use study_hub");
        let mut chain =
            IntentRequestEvidenceChainV1::from_initial_human(&transcript, 1, 0).unwrap();
        let (context, workspace) = one_shot_workspace();
        chain
            .validate_resolutions_against_workspace(&transcript, &workspace, &context)
            .unwrap();

        transcript[1] = envelope("Use community");
        let IntentRequestEvidenceEntryV1::InitialHuman {
            human_turn_digest: persisted_human_digest,
            ..
        } = &mut chain.entries[0]
        else {
            panic!("expected initial human evidence")
        };
        *persisted_human_digest = human_turn_digest("Use community").unwrap();
        chain.head = entry_digest(&chain.entries[0]).unwrap();
        assert!(chain.validate_against_transcript(&transcript).is_ok());
        assert_eq!(
            chain
                .validate_resolutions_against_workspace(&transcript, &workspace, &context)
                .unwrap_err()
                .code,
            "INTENT_REQUEST_EVIDENCE_INITIAL_SELECTION_MISMATCH"
        );
    }

    #[test]
    fn explicit_initial_human_channel_cannot_be_erased_from_workspace() {
        let mut transcript = transcript();
        transcript[1] = envelope("Use study_hub");
        let chain = IntentRequestEvidenceChainV1::from_initial_human(&transcript, 1, 0).unwrap();
        let (context, workspace) = one_shot_workspace();
        let mut projection = serde_json::to_value(workspace).unwrap();
        *projection
            .pointer_mut("/features/0/configuration/parameters/hub_channel")
            .unwrap() = serde_json::Value::Null;
        let erased = serde_json::from_value(projection).unwrap();
        assert_eq!(
            chain
                .validate_resolutions_against_workspace(&transcript, &erased, &context)
                .unwrap_err()
                .code,
            "INTENT_REQUEST_EVIDENCE_INITIAL_SELECTION_MISMATCH"
        );
    }
}
