mod classification;
mod evidence;
mod syntax;

use std::collections::BTreeSet;

use self::classification::classify_sentence_units;
use self::evidence::{evidence_groups, unique_visible_bounded_span, BoundaryEvidenceGroup};
use self::syntax::{mask_quoted_text, sentence_spans, sentence_units};
use super::intent_interpretation::IntentBoundaryRequestV2;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct SafetyBoundaryAnalysis<'a> {
    source: &'a str,
    visible: Vec<char>,
    groups: Vec<BoundaryEvidenceGroup>,
    requests: Vec<IntentBoundaryRequestV2>,
    ownership_ambiguous: bool,
}

impl<'a> SafetyBoundaryAnalysis<'a> {
    pub(crate) fn analyze(human_message: &'a str) -> Self {
        let quote_mask = mask_quoted_text(human_message);
        let visible = quote_mask.visible;
        let mut groups = Vec::new();
        for (span, question) in sentence_spans(&visible) {
            let units = classify_sentence_units(&visible, span, question);
            groups.extend(evidence_groups(&units, &visible));
        }
        let requests = groups
            .iter()
            .map(BoundaryEvidenceGroup::request)
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect();
        Self {
            source: human_message,
            visible,
            groups,
            requests,
            ownership_ambiguous: quote_mask.unmatched,
        }
    }

    pub(crate) fn requests(&self) -> &[IntentBoundaryRequestV2] {
        &self.requests
    }

    pub(crate) fn owns_capability_evidence(&self, candidate: &str) -> bool {
        if self.ownership_ambiguous {
            return false;
        }
        let Some(candidate_span) =
            unique_visible_bounded_span(self.source, &self.visible, candidate)
        else {
            return false;
        };
        self.groups
            .iter()
            .any(|group| group.covers_candidate(candidate_span, &self.visible))
    }
}

pub(crate) fn analyze_safety_boundaries(human_message: &str) -> SafetyBoundaryAnalysis<'_> {
    SafetyBoundaryAnalysis::analyze(human_message)
}

pub(super) struct UnquotedGroundingText {
    pub(super) sentences: Vec<Vec<UnquotedGroundingUnit>>,
}

pub(super) struct UnquotedGroundingUnit {
    pub(super) text: String,
}

pub(super) fn unquoted_grounding_text(human_message: &str) -> Option<UnquotedGroundingText> {
    let mask = mask_quoted_text(human_message);
    if mask.unmatched {
        return None;
    }
    let sentences = sentence_spans(&mask.visible)
        .into_iter()
        .map(|(span, _)| {
            sentence_units(&mask.visible, span)
                .into_iter()
                .map(|unit| UnquotedGroundingUnit { text: unit.text })
                .collect()
        })
        .collect();
    Some(UnquotedGroundingText { sentences })
}

#[cfg(test)]
pub(crate) fn ground_safety_boundary_requests(human_message: &str) -> Vec<IntentBoundaryRequestV2> {
    analyze_safety_boundaries(human_message).requests().to_vec()
}

#[cfg(test)]
pub(crate) fn safety_boundary_owns_capability_evidence(
    human_message: &str,
    candidate: &str,
) -> bool {
    analyze_safety_boundaries(human_message).owns_capability_evidence(candidate)
}
