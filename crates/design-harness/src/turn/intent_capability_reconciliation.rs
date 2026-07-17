mod classification;
mod control_restatement;
mod managed_recipe;
mod syntax;

use std::collections::BTreeSet;

use self::classification::{
    closed_fields_or_preservation_own, closed_route_selection_restatement_owns,
    custom_automation_owns, external_requirement_spans, has_external_marker,
    runtime_business_spans,
};
use self::managed_recipe::managed_recipe_restatement_owns;
pub(super) use self::managed_recipe::ManagedRecipeCoreContext;
use self::syntax::{SourceSyntaxError, SourceText};
use super::intent_capability_grounding::{
    ground_unmapped_capability_evidence, CapabilityEvidenceGroundingError,
};
use super::intent_interpretation::{IntentAutomationKindV2, RuntimeRequirementsV2};

const MAX_CAPABILITIES: usize = 8;
const MAX_CAPABILITY_UTF16: usize = 160;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum CapabilityReconciliationError {
    Grounding {
        candidate_index: usize,
        reason: CapabilityEvidenceGroundingError,
    },
    IncompleteExternalEvidence {
        candidate_index: Option<usize>,
    },
    AmbiguousExternalEvidence {
        count: usize,
    },
    EvidenceTooLong {
        utf16_len: usize,
    },
    TooManyCapabilities {
        count: usize,
    },
    UnbalancedQuote,
}

#[cfg(test)]
pub(super) fn reconcile_unmapped_capabilities(
    canonical_human: &str,
    automation_kind: IntentAutomationKindV2,
    runtime: &RuntimeRequirementsV2,
    candidates: Vec<String>,
) -> Result<Vec<String>, CapabilityReconciliationError> {
    reconcile_unmapped_capabilities_with_context(
        canonical_human,
        automation_kind,
        runtime,
        None,
        candidates,
    )
}

pub(super) fn reconcile_unmapped_capabilities_with_context(
    canonical_human: &str,
    automation_kind: IntentAutomationKindV2,
    runtime: &RuntimeRequirementsV2,
    managed_context: Option<&ManagedRecipeCoreContext<'_>>,
    candidates: Vec<String>,
) -> Result<Vec<String>, CapabilityReconciliationError> {
    let source = SourceText::analyze(canonical_human).map_err(|error| match error {
        SourceSyntaxError::UnbalancedQuote => CapabilityReconciliationError::UnbalancedQuote,
    })?;
    let mut reconciled = BTreeSet::new();
    let mut external_candidate = None;
    let mut external_grounding_failure = None;
    for (candidate_index, candidate) in candidates.into_iter().enumerate() {
        let candidate_mentions_external = has_external_marker(&candidate);
        match ground_unmapped_capability_evidence(
            canonical_human,
            vec![candidate],
            MAX_CAPABILITY_UTF16,
        ) {
            Ok(values) => {
                for value in values {
                    if has_external_marker(&value) {
                        external_candidate.get_or_insert(candidate_index);
                        continue;
                    }
                    if !source.has_asserted_occurrence(&value) {
                        if source.has_only_proven_irrelevant_occurrences(&value) {
                            continue;
                        }
                        return Err(CapabilityReconciliationError::Grounding {
                            candidate_index,
                            reason: CapabilityEvidenceGroundingError::Ungrounded,
                        });
                    }
                    if custom_automation_owns(&source, automation_kind, &value)
                        || closed_route_selection_restatement_owns(&source, automation_kind, &value)
                        || closed_fields_or_preservation_own(&source, &value, runtime)
                        || managed_context.is_some_and(|context| {
                            managed_recipe_restatement_owns(&source, context, &value)
                        })
                    {
                        continue;
                    }
                    insert_checked(&mut reconciled, value)?;
                }
            }
            Err(reason)
                if candidate_mentions_external
                    && reason == CapabilityEvidenceGroundingError::Ungrounded =>
            {
                external_candidate.get_or_insert(candidate_index);
                external_grounding_failure.get_or_insert((candidate_index, reason));
            }
            Err(reason) => {
                return Err(CapabilityReconciliationError::Grounding {
                    candidate_index,
                    reason,
                });
            }
        }
    }
    if matches!(
        automation_kind,
        IntentAutomationKindV2::CustomAutomation | IntentAutomationKindV2::None
    ) {
        for span in runtime_business_spans(&source, runtime) {
            let Some(value) = source.value(span) else {
                continue;
            };
            insert_checked(&mut reconciled, value.to_string())?;
        }
    }
    let external_spans = external_requirement_spans(&source);
    if external_spans.len() > 1 {
        return Err(CapabilityReconciliationError::AmbiguousExternalEvidence {
            count: external_spans.len(),
        });
    }
    if let Some(span) = external_spans.first().copied() {
        if let Some(value) = source.value(span) {
            insert_checked(&mut reconciled, value.to_string())?;
        }
    } else if let Some((candidate_index, reason)) = external_grounding_failure {
        return Err(CapabilityReconciliationError::Grounding {
            candidate_index,
            reason,
        });
    } else if external_candidate.is_some() {
        return Err(CapabilityReconciliationError::IncompleteExternalEvidence {
            candidate_index: external_candidate,
        });
    }
    if reconciled.len() > MAX_CAPABILITIES {
        return Err(CapabilityReconciliationError::TooManyCapabilities {
            count: reconciled.len(),
        });
    }
    Ok(reconciled.into_iter().collect())
}

fn insert_checked(
    values: &mut BTreeSet<String>,
    value: String,
) -> Result<(), CapabilityReconciliationError> {
    let utf16_len = value.encode_utf16().count();
    if utf16_len > MAX_CAPABILITY_UTF16 {
        return Err(CapabilityReconciliationError::EvidenceTooLong { utf16_len });
    }
    values.insert(value);
    Ok(())
}
