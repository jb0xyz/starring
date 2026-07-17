mod classification;
mod evidence;
mod syntax;

use std::collections::BTreeSet;

use self::classification::classify_sentence_units;
use self::evidence::{
    coordinated_groups_cover_candidate, cross_sentence_role_bridge, evidence_groups,
    unique_visible_bounded_span, BoundaryEvidenceGroup, CanonicalWhitespaceMap,
};
use self::syntax::{
    mask_quoted_text, normalized_text, sentence_spans, sentence_units, TextSpan, UnitLink,
};
use super::intent_interpretation::IntentBoundaryRequestV2;
use super::intent_metalinguistic_scope::{
    ends_metalinguistic_copy, metalinguistic_carrier, QuotedLiteralScope,
};
use super::intent_quote_scanner::{QuotedSpan, QuotedText};

#[cfg(test)]
use std::cell::Cell;

#[cfg(test)]
thread_local! {
    static QUOTE_ROLE_CONTEXT_WORK: Cell<usize> = const { Cell::new(0) };
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct SafetyBoundaryAnalysis {
    canonical_source: CanonicalWhitespaceMap,
    visible: Vec<char>,
    groups: Vec<BoundaryEvidenceGroup>,
    requests: Vec<IntentBoundaryRequestV2>,
    ownership_ambiguous: bool,
}

impl SafetyBoundaryAnalysis {
    pub(crate) fn analyze(human_message: &str) -> Self {
        let quoted = QuotedText::scan(human_message);
        let visible = semantically_visible_boundary_text(human_message, &quoted);
        let mut groups = Vec::new();
        let mut copied_block = false;
        let mut previous_tail = None;
        for (span, question) in sentence_spans(&visible) {
            let mut units = classify_sentence_units(&visible, span, question);
            for unit in &mut units {
                if copied_block {
                    unit.hypothetical = true;
                    if ends_metalinguistic_copy(&unit.text) {
                        copied_block = false;
                    }
                    continue;
                }
                if metalinguistic_carrier(&unit.text) {
                    unit.hypothetical = true;
                    copied_block = true;
                }
            }
            if let (Some(previous), Some(first)) = (previous_tail.as_ref(), units.first()) {
                if cross_sentence_role_bridge(previous, first) {
                    let mut current = first.clone();
                    current.link = UnitLink::Sequential;
                    groups.extend(evidence_groups(&[previous.clone(), current], &visible));
                }
            }
            groups.extend(evidence_groups(&units, &visible));
            previous_tail = units.last().cloned();
        }
        let requests = groups
            .iter()
            .map(BoundaryEvidenceGroup::request)
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect();
        Self {
            canonical_source: CanonicalWhitespaceMap::from_source(human_message),
            visible,
            groups,
            requests,
            ownership_ambiguous: quoted.unmatched(),
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
            unique_visible_bounded_span(&self.canonical_source, &self.visible, candidate)
        else {
            return false;
        };
        self.groups
            .iter()
            .any(|group| group.covers_candidate(candidate_span, &self.visible))
            || coordinated_groups_cover_candidate(&self.groups, candidate_span, &self.visible)
    }
}

fn semantically_visible_boundary_text(value: &str, quoted: &QuotedText) -> Vec<char> {
    let mut scope = QuotedLiteralScope::default();
    let mut cursor = 0usize;
    let roles = quoted
        .spans()
        .iter()
        .map(|span| {
            let between = &value[cursor..span.start];
            record_quote_role_context_work(between, &value[span.end..]);
            let literal = scope.classify(between, &value[span.end..]);
            cursor = span.end;
            (literal, quote_content_range(value, *span))
        })
        .collect::<Vec<_>>();
    let mut span_index = 0usize;
    value
        .char_indices()
        .map(|(start, character)| {
            while quoted
                .spans()
                .get(span_index)
                .is_some_and(|span| span.end <= start)
            {
                span_index = span_index.saturating_add(1);
            }
            let Some(span) = quoted.spans().get(span_index) else {
                return character;
            };
            if start < span.start || start >= span.end {
                return character;
            }
            let (literal, (content_start, content_end)) = roles[span_index];
            if literal || start < content_start || start >= content_end {
                ' '
            } else {
                character
            }
        })
        .collect()
}

#[cfg(test)]
fn record_quote_role_context_work(between: &str, suffix: &str) {
    let count = between
        .chars()
        .count()
        .saturating_add(suffix.chars().take(64).count());
    QUOTE_ROLE_CONTEXT_WORK.with(|work| work.set(work.get().saturating_add(count)));
}

#[cfg(not(test))]
fn record_quote_role_context_work(_between: &str, _suffix: &str) {}

fn quote_content_range(value: &str, span: QuotedSpan) -> (usize, usize) {
    let opener = value[span.start..span.end].chars().next();
    let content_start = if opener == Some('`') {
        span.start.saturating_add(
            value[span.start..span.end]
                .bytes()
                .take_while(|byte| *byte == b'`')
                .count(),
        )
    } else {
        span.start.saturating_add(opener.map_or(0, char::len_utf8))
    };
    let closer = value[span.start..span.end].chars().next_back();
    let content_end = if closer == Some('`') {
        span.end.saturating_sub(
            value[span.start..span.end]
                .bytes()
                .rev()
                .take_while(|byte| *byte == b'`')
                .count(),
        )
    } else {
        span.end.saturating_sub(closer.map_or(0, char::len_utf8))
    };
    (content_start.min(content_end), content_end)
}

pub(crate) fn analyze_safety_boundaries(human_message: &str) -> SafetyBoundaryAnalysis {
    SafetyBoundaryAnalysis::analyze(human_message)
}

pub(super) struct UnquotedGroundingText {
    pub(super) sentences: Vec<Vec<UnquotedGroundingUnit>>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum UnquotedGroundingLink {
    Detached,
    Additive,
    Alternative,
}

pub(super) struct UnquotedGroundingUnit {
    pub(super) text: String,
    pub(super) source_text: String,
    pub(super) question: bool,
    pub(super) link: UnquotedGroundingLink,
    pub(super) operative_authority: Option<bool>,
}

pub(super) fn unquoted_grounding_text(human_message: &str) -> Option<UnquotedGroundingText> {
    let mask = mask_quoted_text(human_message);
    if mask.unmatched {
        return None;
    }
    let source = human_message.chars().collect::<Vec<_>>();
    let sentences = sentence_spans(&mask.visible)
        .into_iter()
        .map(|(span, question)| {
            let (units, operative_split) = sentence_units(&mask.visible, span, question);
            let mut grounding_units: Vec<UnquotedGroundingUnit> = Vec::new();
            for unit in units {
                if unit.link == UnitLink::NegativeAlternative {
                    if let Some(previous) = grounding_units.last_mut() {
                        previous.text.push_str(" nor ");
                        previous.text.push_str(&unit.text);
                        previous.source_text.push_str(" nor ");
                        previous.source_text.push_str(&grounding_source_text(
                            &source,
                            &mask.visible,
                            unit.source_span,
                        ));
                        continue;
                    }
                }
                let operative_authority = operative_split.map(|split| unit.span.start >= split);
                grounding_units.push(UnquotedGroundingUnit {
                    text: unit.text,
                    source_text: grounding_source_text(&source, &mask.visible, unit.source_span),
                    question,
                    link: match unit.link {
                        UnitLink::Additive | UnitLink::Sequential => {
                            UnquotedGroundingLink::Additive
                        }
                        UnitLink::Alternative | UnitLink::ConditionalAlternative => {
                            UnquotedGroundingLink::Alternative
                        }
                        UnitLink::Start
                        | UnitLink::Scope
                        | UnitLink::Barrier
                        | UnitLink::NegativeAlternative => UnquotedGroundingLink::Detached,
                    },
                    operative_authority,
                });
            }
            grounding_units
        })
        .collect();
    Some(UnquotedGroundingText { sentences })
}

fn grounding_source_text(source: &[char], visible: &[char], span: TextSpan) -> String {
    let mut end = span.end;
    while end < source.len()
        && visible
            .get(end)
            .is_some_and(|character| character.is_whitespace())
    {
        end = end.saturating_add(1);
    }
    normalized_text(
        &source[span.start..end]
            .iter()
            .collect::<String>()
            .to_lowercase(),
    )
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

#[cfg(test)]
pub(crate) fn passive_gate_preservation_prefix_steps(human_message: &str) -> usize {
    super::intent_safety_control_grammar::reset_tail_prefix_steps();
    let _ = analyze_safety_boundaries(human_message);
    super::intent_safety_control_grammar::tail_prefix_steps()
}

#[cfg(test)]
pub(crate) fn boundary_quote_role_context_work(human_message: &str) -> usize {
    QUOTE_ROLE_CONTEXT_WORK.with(|work| work.set(0));
    let _ = analyze_safety_boundaries(human_message);
    QUOTE_ROLE_CONTEXT_WORK.with(Cell::get)
}
