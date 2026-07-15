use std::collections::BTreeSet;

#[cfg(test)]
use std::cell::Cell;

use super::super::intent_interpretation::IntentBoundaryRequestV2;
use super::classification::{
    contains_any, live_weak_context, BoundaryKind, UnitFacts, GATE_ACTIONS, GATE_TARGETS,
    IMMEDIATE_CONTEXT, LIVE_ACTIONS, LIVE_CONTEXT, PREFIX_NEGATIONS, SECRET_ACTIONS,
    SECRET_DELIVERY_CONTEXT, SECRET_TARGETS, SUFFIX_NEGATIONS, UNPROTECTED_SECRET,
};
use super::syntax::{
    ascii_case_insensitive_chars_equal, normalized_text, word_continuation, BoundaryUnit, TextSpan,
    UnitLink,
};

#[cfg(test)]
thread_local! {
    static COMPONENT_ONLY_EVALUATIONS: Cell<usize> = const { Cell::new(0) };
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct BoundaryEvidenceGroup {
    kind: BoundaryKind,
    member_coverage_spans: Vec<TextSpan>,
    joiner_spans: Vec<TextSpan>,
    positive_role_spans: Vec<TextSpan>,
}

impl BoundaryEvidenceGroup {
    pub(super) fn request(&self) -> IntentBoundaryRequestV2 {
        self.kind.request()
    }

    pub(super) fn covers_candidate(&self, candidate: TextSpan, visible: &[char]) -> bool {
        group_covers_candidate(self, candidate, visible)
    }
}

pub(super) fn evidence_groups(
    units: &[BoundaryUnit],
    visible: &[char],
) -> Vec<BoundaryEvidenceGroup> {
    let facts = units
        .iter()
        .map(|unit| {
            if unit.hypothetical {
                UnitFacts::default()
            } else {
                UnitFacts::for_unit(unit)
            }
        })
        .collect::<Vec<_>>();
    let mut groups = Vec::new();
    for kind in [BoundaryKind::Gate, BoundaryKind::Live, BoundaryKind::Secret] {
        let component_only = units
            .iter()
            .map(|unit| !unit.hypothetical && unit_is_boundary_component_only(&unit.text, kind))
            .collect::<Vec<_>>();
        let mut expanded_starts = (0..units.len()).collect::<Vec<_>>();
        for index in 1..units.len() {
            let previous = index - 1;
            if units[index].link == UnitLink::Additive
                && component_only[previous]
                && facts[previous].has_evidence(kind)
            {
                expanded_starts[index] = expanded_starts[previous];
            }
        }
        let mut expanded_ends = (0..units.len()).collect::<Vec<_>>();
        for index in (0..units.len().saturating_sub(1)).rev() {
            let next = index + 1;
            if units[next].link == UnitLink::Additive
                && component_only[next]
                && facts[next].has_evidence(kind)
            {
                expanded_ends[index] = expanded_ends[next];
            }
        }
        let mut intervals = BTreeSet::new();
        for (index, unit_facts) in facts.iter().enumerate() {
            if unit_facts.is_seed(kind) {
                intervals.insert((expanded_starts[index], expanded_ends[index]));
            }
        }
        if kind == BoundaryKind::Live {
            for index in 0..units.len().saturating_sub(1) {
                if units[index].hypothetical
                    || units[index + 1].hypothetical
                    || units[index + 1].link == UnitLink::Alternative
                    || !component_only[index]
                    || !component_only[index + 1]
                {
                    continue;
                }
                let combined = combine_facts(&facts[index], &facts[index + 1]);
                if combined.is_seed(kind) {
                    intervals.insert((expanded_starts[index], expanded_ends[index + 1]));
                }
            }
        }
        for (start, end) in intervals {
            groups.push(evidence_group_for_interval(
                units, visible, kind, start, end,
            ));
        }
    }
    groups
}

fn evidence_group_for_interval(
    units: &[BoundaryUnit],
    visible: &[char],
    kind: BoundaryKind,
    start: usize,
    end: usize,
) -> BoundaryEvidenceGroup {
    let mut member_coverage_spans = Vec::new();
    let mut joiner_spans = Vec::new();
    let mut positive_role_spans = Vec::new();
    for index in start..=end {
        if index > start && units[index].link == UnitLink::Additive {
            joiner_spans.push(TextSpan {
                start: units[index - 1].span.end,
                end: units[index].span.start,
            });
        }
        member_coverage_spans.extend(boundary_coverage_spans(visible, units[index].span, kind));
        positive_role_spans.extend(positive_role_spans_for_unit(visible, &units[index], kind));
    }
    BoundaryEvidenceGroup {
        kind,
        member_coverage_spans,
        joiner_spans,
        positive_role_spans,
    }
}

fn group_covers_candidate(
    group: &BoundaryEvidenceGroup,
    candidate: TextSpan,
    visible: &[char],
) -> bool {
    let mut has_content = false;
    for (offset, character) in visible[candidate.start..candidate.end].iter().enumerate() {
        if !word_continuation(*character) {
            continue;
        }
        has_content = true;
        let index = candidate.start.saturating_add(offset);
        if !group
            .member_coverage_spans
            .iter()
            .chain(&group.joiner_spans)
            .any(|span| index >= span.start && index < span.end)
        {
            return false;
        }
    }
    has_content
        && group
            .positive_role_spans
            .iter()
            .any(|span| candidate.start < span.end && candidate.end > span.start)
}

fn combine_facts(left: &UnitFacts, right: &UnitFacts) -> UnitFacts {
    UnitFacts {
        gate_action: left.gate_action || right.gate_action,
        gate_target: left.gate_target || right.gate_target,
        live_action: left.live_action || right.live_action,
        live_strong_context: left.live_strong_context || right.live_strong_context,
        live_weak_context: left.live_weak_context || right.live_weak_context,
        immediate: left.immediate || right.immediate,
        secret_action: left.secret_action || right.secret_action,
        secret_target: left.secret_target || right.secret_target,
        secret_delivery: left.secret_delivery || right.secret_delivery,
        secret_unprotected: left.secret_unprotected || right.secret_unprotected,
        secret_safe: left.secret_safe || right.secret_safe,
    }
}

pub(super) fn unique_visible_bounded_span(
    source: &str,
    visible: &[char],
    candidate: &str,
) -> Option<TextSpan> {
    if candidate.is_empty() {
        return None;
    }
    let candidate_length = candidate.chars().count();
    let mut occurrence = None;
    for (byte_start, _) in source.match_indices(candidate) {
        let byte_end = byte_start.saturating_add(candidate.len());
        if !bounded_string_occurrence(source, candidate, byte_start, byte_end) {
            continue;
        }
        let start = source[..byte_start].chars().count();
        let end = start.saturating_add(candidate_length);
        if !visible
            .get(start..end)
            .is_some_and(|value| value.iter().copied().eq(candidate.chars()))
        {
            continue;
        }
        if occurrence.is_some() {
            return None;
        }
        occurrence = Some(TextSpan { start, end });
    }
    occurrence
}

fn bounded_string_occurrence(source: &str, candidate: &str, start: usize, end: usize) -> bool {
    let left_valid = !candidate.chars().next().is_some_and(word_continuation)
        || !source[..start]
            .chars()
            .next_back()
            .is_some_and(word_continuation);
    let right_valid = !candidate.chars().next_back().is_some_and(word_continuation)
        || !source[end..].chars().next().is_some_and(word_continuation)
        || known_korean_suffix_boundary(&source[end..]);
    left_valid && right_valid
}

fn known_korean_suffix_boundary(value: &str) -> bool {
    ["해주세요", "해줘", "하도록", "하게", "하고", "하며"]
        .iter()
        .any(|suffix| value.starts_with(suffix))
}

fn unit_is_boundary_component_only(value: &str, kind: BoundaryKind) -> bool {
    #[cfg(test)]
    COMPONENT_ONLY_EVALUATIONS.with(|evaluations| {
        evaluations.set(evaluations.get().saturating_add(1));
    });
    let value = normalized_text(&value.to_lowercase());
    !value.is_empty()
        && UnitFacts::for_text(&value).has_evidence(kind)
        && boundary_text_is_covered(&value, kind)
}

fn boundary_text_is_covered(value: &str, kind: BoundaryKind) -> bool {
    let characters = value.chars().collect::<Vec<_>>();
    let mut covered = vec![false; characters.len()];
    for markers in boundary_candidate_markers(kind) {
        cover_markers(&characters, &mut covered, markers);
    }
    cover_markers(&characters, &mut covered, english_neutral_words());
    cover_markers(&characters, &mut covered, korean_neutral_fragments());
    characters
        .iter()
        .enumerate()
        .all(|(index, character)| covered[index] || !word_continuation(*character))
}

fn boundary_coverage_spans(visible: &[char], span: TextSpan, kind: BoundaryKind) -> Vec<TextSpan> {
    let characters = &visible[span.start..span.end];
    let mut covered = vec![false; characters.len()];
    for markers in boundary_candidate_markers(kind) {
        cover_markers(characters, &mut covered, markers);
    }
    cover_markers(characters, &mut covered, english_neutral_words());
    cover_markers(characters, &mut covered, korean_neutral_fragments());
    covered_runs(&covered, span.start)
}

fn covered_runs(covered: &[bool], offset: usize) -> Vec<TextSpan> {
    let mut spans = Vec::new();
    let mut start = None;
    for index in 0..=covered.len() {
        if covered.get(index).copied().unwrap_or(false) {
            start.get_or_insert(index);
        } else if let Some(run_start) = start.take() {
            spans.push(TextSpan {
                start: offset.saturating_add(run_start),
                end: offset.saturating_add(index),
            });
        }
    }
    spans
}

fn positive_role_spans_for_unit(
    visible: &[char],
    unit: &BoundaryUnit,
    kind: BoundaryKind,
) -> Vec<TextSpan> {
    let mut spans = Vec::new();
    if !unit.inherited_action_negation {
        let actions = match kind {
            BoundaryKind::Gate => GATE_ACTIONS,
            BoundaryKind::Live => LIVE_ACTIONS,
            BoundaryKind::Secret => SECRET_ACTIONS,
        };
        spans.extend(marker_occurrence_spans(visible, unit.span, actions, true));
    }
    match kind {
        BoundaryKind::Gate => {
            spans.extend(marker_occurrence_spans(
                visible,
                unit.span,
                GATE_TARGETS,
                false,
            ));
        }
        BoundaryKind::Live => {
            for markers in [
                LIVE_CONTEXT,
                live_weak_context(),
                IMMEDIATE_CONTEXT,
                &["live changes", "라이브 변경"],
            ] {
                spans.extend(marker_occurrence_spans(visible, unit.span, markers, false));
            }
        }
        BoundaryKind::Secret => {
            for markers in [SECRET_TARGETS, UNPROTECTED_SECRET] {
                spans.extend(marker_occurrence_spans(visible, unit.span, markers, false));
            }
        }
    }
    spans
}

fn marker_occurrence_spans(
    visible: &[char],
    span: TextSpan,
    markers: &[&str],
    require_unnegated: bool,
) -> Vec<TextSpan> {
    let characters = &visible[span.start..span.end];
    let mut spans = Vec::new();
    for marker in markers {
        let marker = marker.chars().collect::<Vec<_>>();
        if marker.is_empty() || marker.len() > characters.len() {
            continue;
        }
        for start in 0..=characters.len().saturating_sub(marker.len()) {
            let end = start.saturating_add(marker.len());
            if ascii_case_insensitive_chars_equal(&characters[start..end], &marker)
                && char_marker_has_boundaries(characters, &marker, start, end)
                && (!require_unnegated || !char_marker_is_negated(characters, start, end))
            {
                spans.push(TextSpan {
                    start: span.start.saturating_add(start),
                    end: span.start.saturating_add(end),
                });
            }
        }
    }
    spans
}

fn char_marker_is_negated(value: &[char], start: usize, end: usize) -> bool {
    let prefix = value[start.saturating_sub(48)..start]
        .iter()
        .collect::<String>()
        .to_lowercase();
    let suffix = value[end..value.len().min(end.saturating_add(32))]
        .iter()
        .collect::<String>()
        .to_lowercase();
    contains_any(&prefix, PREFIX_NEGATIONS)
        || SUFFIX_NEGATIONS
            .iter()
            .any(|negation| suffix.trim_start().starts_with(negation))
}

fn boundary_candidate_markers(kind: BoundaryKind) -> Vec<&'static [&'static str]> {
    match kind {
        BoundaryKind::Gate => vec![GATE_ACTIONS, GATE_TARGETS],
        BoundaryKind::Live => vec![
            LIVE_ACTIONS,
            LIVE_CONTEXT,
            live_weak_context(),
            IMMEDIATE_CONTEXT,
            &["live changes", "라이브 변경"],
        ],
        BoundaryKind::Secret => vec![
            SECRET_ACTIONS,
            SECRET_TARGETS,
            UNPROTECTED_SECRET,
            SECRET_DELIVERY_CONTEXT,
        ],
    }
}

fn cover_markers(characters: &[char], covered: &mut [bool], markers: &[&str]) {
    for marker in markers {
        let marker = marker.chars().collect::<Vec<_>>();
        if marker.is_empty() || marker.len() > characters.len() {
            continue;
        }
        for start in 0..=characters.len().saturating_sub(marker.len()) {
            let end = start.saturating_add(marker.len());
            if ascii_case_insensitive_chars_equal(&characters[start..end], &marker)
                && char_marker_has_boundaries(characters, &marker, start, end)
            {
                covered[start..end].fill(true);
            }
        }
    }
}

fn char_marker_has_boundaries(value: &[char], marker: &[char], start: usize, end: usize) -> bool {
    let left_valid = !marker
        .first()
        .is_some_and(|character| character.is_ascii_alphanumeric())
        || start == 0
        || !value[start - 1..start]
            .first()
            .is_some_and(|character| character.is_ascii_alphanumeric());
    let right_valid = !marker
        .last()
        .is_some_and(|character| character.is_ascii_alphanumeric())
        || !value
            .get(end)
            .is_some_and(|character| character.is_ascii_alphanumeric());
    left_valid && right_valid
}

fn english_neutral_words() -> &'static [&'static str] {
    &[
        "a",
        "all",
        "an",
        "and",
        "any",
        "at",
        "but",
        "design",
        "directly",
        "every",
        "for",
        "from",
        "in",
        "immediately",
        "into",
        "it",
        "now",
        "of",
        "on",
        "only",
        "please",
        "the",
        "them",
        "then",
        "these",
        "this",
        "those",
        "to",
        "user",
        "users",
        "value",
        "values",
    ]
}

fn korean_neutral_fragments() -> &'static [&'static str] {
    &[
        "해주세요",
        "하도록",
        "하게",
        "해줘",
        "에서",
        "에게",
        "으로",
        "은",
        "는",
        "이",
        "가",
        "을",
        "를",
        "의",
        "에",
        "로",
        "와",
        "과",
        "만",
        "도",
        "고",
        "해",
    ]
}

#[cfg(test)]
mod tests {
    use super::super::classification::classify_sentence_units;
    use super::super::syntax::sentence_spans;
    use super::*;

    fn component_evaluations(unit_count: usize) -> usize {
        let human = vec!["skip validation"; unit_count].join(" and ");
        let visible = human.chars().collect::<Vec<_>>();
        let (span, question) = sentence_spans(&visible).into_iter().next().unwrap();
        let units = classify_sentence_units(&visible, span, question);
        assert_eq!(units.len(), unit_count);
        COMPONENT_ONLY_EVALUATIONS.with(|evaluations| evaluations.set(0));
        let groups = evidence_groups(&units, &visible);
        assert_eq!(groups.len(), 1);
        assert_eq!(
            groups[0].request(),
            IntentBoundaryRequestV2::BypassValidationPreviewApproval
        );
        COMPONENT_ONLY_EVALUATIONS.with(Cell::get)
    }

    #[test]
    fn component_classification_work_scales_linearly() {
        let small = component_evaluations(128);
        let large = component_evaluations(256);
        assert_eq!(small, 3 * 128);
        assert_eq!(large, 3 * 256);
        assert_eq!(large, small * 2);
    }
}
