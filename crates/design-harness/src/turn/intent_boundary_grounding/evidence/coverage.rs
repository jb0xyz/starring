use super::super::classification::{
    closed_boundary_action_adverb, inherited_action_negation, live_weak_context,
    prefix_negates_action, secret_target_is_locally_safe, suffix_negates_action, BoundaryKind,
    ACTION_NEGATION_MODIFIERS, ACTION_POLARITY_TOKEN_WINDOW, CLOSED_BOUNDARY_ACTION_ADVERBS,
    CLOSED_SAFETY_CONTROL_SCOPE_PREPOSITIONS, CLOSED_SAFETY_CONTROL_SCOPE_TERMS,
    CLOSED_SAFETY_CONTROL_TARGETS, CLOSED_SAFETY_CONTROL_TARGET_TERMS,
    CLOSED_SECRET_DISCLOSURE_ACTORS, CLOSED_THIRD_PERSON_BOUNDARY_ACTORS, GATE_ACTIONS,
    GATE_ACTION_PERMISSION_PREDICATES, GATE_CONTROL_TERMS, GATE_DESTRUCTIVE_ACTIONS,
    GATE_EXACT_ACTIONS, GATE_EXACT_ACTION_TERMS, GATE_EXACT_PREFIX_WRAPPERS,
    GATE_EXACT_SUFFIX_WRAPPERS, GATE_EXACT_WRAPPER_TERMS, GATE_REQUIREMENT_REVERSAL_ACTIONS,
    GATE_REQUIREMENT_REVERSAL_PREDICATES, GATE_RESULT_ACTIONS, GATE_TARGETS, IMMEDIATE_CONTEXT,
    LIVE_ACTIONS, LIVE_CONTEXT, LIVE_CONTEXT_ALIASES, ORDINARY_PREFIX_NEGATIONS,
    PRESERVATION_ACTOR_TERMS, PRESERVATION_DETERMINERS, PRESERVATION_PREFIX_NEGATIONS,
    SAFETY_CONTROL_TARGET_MODIFIERS, SAFE_REDACTION, SECRET_ACTIONS, SECRET_DELIVERY_CONTEXT,
    SECRET_TARGETS, UNPROTECTED_SECRET,
};
use super::super::syntax::{normalized_text, word_continuation, BoundaryUnit, TextSpan};

#[cfg(test)]
use super::{COMPONENT_ONLY_EVALUATIONS, EVIDENCE_SPAN_LINEAR_WORK, POLARITY_PREFIX_CHAR_VISITS};

pub(super) fn unit_is_boundary_component_only(
    value: &str,
    kind: BoundaryKind,
    has_evidence: bool,
) -> bool {
    #[cfg(test)]
    COMPONENT_ONLY_EVALUATIONS.with(|evaluations| {
        evaluations.set(evaluations.get().saturating_add(1));
    });
    !value.is_empty() && has_evidence && boundary_text_is_covered(value, kind)
}

fn boundary_text_is_covered(value: &str, kind: BoundaryKind) -> bool {
    let characters = value.chars().collect::<Vec<_>>();
    let mut covered = vec![false; characters.len()];
    for_each_boundary_candidate_marker_set(kind, |markers| {
        cover_markers(&characters, &mut covered, markers);
    });
    cover_markers(&characters, &mut covered, english_neutral_words());
    cover_fragments(&characters, &mut covered, korean_neutral_fragments());
    cover_closed_action_adverbs(&characters, &mut covered);
    characters
        .iter()
        .enumerate()
        .all(|(index, character)| covered[index] || !word_continuation(*character))
}

pub(super) fn boundary_coverage_spans(
    visible: &[char],
    span: TextSpan,
    kind: BoundaryKind,
) -> Vec<TextSpan> {
    let characters = &visible[span.start..span.end];
    let mut covered = vec![false; characters.len()];
    for_each_boundary_candidate_marker_set(kind, |markers| {
        cover_markers(characters, &mut covered, markers);
    });
    cover_markers(characters, &mut covered, english_neutral_words());
    cover_fragments(characters, &mut covered, korean_neutral_fragments());
    cover_closed_action_adverbs(characters, &mut covered);
    covered_runs(&covered, span.start)
}

pub(super) fn covered_runs(covered: &[bool], offset: usize) -> Vec<TextSpan> {
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

pub(super) fn positive_role_spans_for_unit(
    visible: &[char],
    unit: &BoundaryUnit,
    kind: BoundaryKind,
) -> Vec<TextSpan> {
    let mut spans = Vec::new();
    if !inherited_action_negation(unit, kind) {
        let actions = match kind {
            BoundaryKind::Gate => GATE_ACTIONS,
            BoundaryKind::Live => LIVE_ACTIONS,
            BoundaryKind::Secret => SECRET_ACTIONS,
        };
        spans.extend(marker_occurrence_spans(visible, unit.span, actions, true));
        if kind == BoundaryKind::Gate {
            spans.extend(marker_occurrence_spans(
                visible,
                unit.span,
                GATE_DESTRUCTIVE_ACTIONS,
                true,
            ));
            spans.extend(marker_occurrence_spans(
                visible,
                unit.span,
                GATE_REQUIREMENT_REVERSAL_ACTIONS,
                true,
            ));
            spans.extend(marker_occurrence_spans(
                visible,
                unit.span,
                GATE_EXACT_ACTIONS,
                true,
            ));
            spans.extend(marker_occurrence_spans(
                visible,
                unit.span,
                GATE_RESULT_ACTIONS,
                true,
            ));
        }
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
                LIVE_CONTEXT_ALIASES,
                live_weak_context(),
                IMMEDIATE_CONTEXT,
                &["live changes", "라이브 변경"],
            ] {
                spans.extend(marker_occurrence_spans(visible, unit.span, markers, false));
            }
        }
        BoundaryKind::Secret => {
            spans.extend(unsafe_secret_target_spans(visible, unit.span));
            spans.extend(marker_occurrence_spans(
                visible,
                unit.span,
                UNPROTECTED_SECRET,
                false,
            ));
        }
    }
    spans
}

pub(super) fn unsafe_secret_target_spans(visible: &[char], span: TextSpan) -> Vec<TextSpan> {
    let value = visible[span.start..span.end]
        .iter()
        .collect::<String>()
        .to_ascii_lowercase();
    let targets = marker_occurrence_spans(visible, span, SECRET_TARGETS, false);
    let mut maximal: Vec<TextSpan> = Vec::new();
    let mut covered_end = 0usize;
    for target in targets {
        #[cfg(test)]
        EVIDENCE_SPAN_LINEAR_WORK.with(|work| work.set(work.get().saturating_add(1)));
        if target.end <= covered_end {
            continue;
        }
        covered_end = target.end;
        maximal.push(target);
    }
    let mut character_to_byte = value
        .char_indices()
        .map(|(byte, _)| byte)
        .collect::<Vec<_>>();
    character_to_byte.push(value.len());
    #[cfg(test)]
    EVIDENCE_SPAN_LINEAR_WORK.with(|work| {
        work.set(work.get().saturating_add(character_to_byte.len()));
    });
    maximal
        .into_iter()
        .filter(|target| {
            let local_start = target.start.saturating_sub(span.start);
            let local_end = target.end.saturating_sub(span.start);
            let start = character_to_byte
                .get(local_start)
                .copied()
                .unwrap_or(value.len());
            let end = character_to_byte
                .get(local_end)
                .copied()
                .unwrap_or(value.len());
            !secret_target_is_locally_safe(&value, start, end)
        })
        .collect()
}

pub(super) fn marker_occurrence_spans(
    visible: &[char],
    span: TextSpan,
    markers: &[&str],
    require_unnegated: bool,
) -> Vec<TextSpan> {
    let characters = &visible[span.start..span.end];
    let mut furthest_end_at_start = vec![None; characters.len().saturating_add(1)];
    for marker in markers {
        let marker_length = marker.chars().count();
        if marker_length == 0 || marker_length > characters.len() {
            continue;
        }
        for start in 0..=characters.len().saturating_sub(marker_length) {
            #[cfg(test)]
            EVIDENCE_SPAN_LINEAR_WORK.with(|work| work.set(work.get().saturating_add(1)));
            let end = start.saturating_add(marker_length);
            if ascii_case_insensitive_marker_equal(&characters[start..end], marker)
                && char_marker_has_boundaries(characters, marker, start, end)
                && (!require_unnegated || !char_marker_is_negated(characters, start, end))
            {
                let slot = &mut furthest_end_at_start[start];
                *slot = Some(slot.map_or(end, |current: usize| current.max(end)));
            }
        }
    }
    furthest_end_at_start
        .into_iter()
        .enumerate()
        .filter_map(|(start, end)| {
            #[cfg(test)]
            EVIDENCE_SPAN_LINEAR_WORK.with(|work| work.set(work.get().saturating_add(1)));
            end.map(|end| TextSpan {
                start: span.start.saturating_add(start),
                end: span.start.saturating_add(end),
            })
        })
        .collect()
}

fn char_marker_is_negated(value: &[char], start: usize, end: usize) -> bool {
    let prefix_start = preceding_token_window_start(value, start);
    #[cfg(test)]
    POLARITY_PREFIX_CHAR_VISITS.with(|visits| {
        visits.set(
            visits
                .get()
                .saturating_add(start.saturating_sub(prefix_start)),
        );
    });
    let prefix = value[prefix_start..start]
        .iter()
        .collect::<String>()
        .to_lowercase();
    let suffix = value[end..value.len().min(end.saturating_add(32))]
        .iter()
        .collect::<String>()
        .to_lowercase();
    prefix_negates_action(&normalized_text(&prefix))
        || suffix_negates_action(&normalized_text(&suffix))
}

fn preceding_token_window_start(value: &[char], end: usize) -> usize {
    let mut start = end;
    for _ in 0..ACTION_POLARITY_TOKEN_WINDOW {
        while start > 0 && value[start.saturating_sub(1)].is_whitespace() {
            start = start.saturating_sub(1);
        }
        if start == 0 {
            break;
        }
        while start > 0 && !value[start.saturating_sub(1)].is_whitespace() {
            start = start.saturating_sub(1);
        }
    }
    start
}

fn for_each_boundary_candidate_marker_set(
    kind: BoundaryKind,
    mut visit: impl FnMut(&'static [&'static str]),
) {
    match kind {
        BoundaryKind::Gate => {
            for markers in [
                GATE_ACTIONS,
                GATE_ACTION_PERMISSION_PREDICATES,
                GATE_DESTRUCTIVE_ACTIONS,
                CLOSED_SAFETY_CONTROL_TARGETS,
                GATE_REQUIREMENT_REVERSAL_ACTIONS,
                GATE_REQUIREMENT_REVERSAL_PREDICATES,
                GATE_RESULT_ACTIONS,
                GATE_EXACT_ACTIONS,
                GATE_EXACT_ACTION_TERMS,
                GATE_EXACT_PREFIX_WRAPPERS,
                GATE_EXACT_SUFFIX_WRAPPERS,
                GATE_EXACT_WRAPPER_TERMS,
                GATE_CONTROL_TERMS,
                ACTION_NEGATION_MODIFIERS,
                ORDINARY_PREFIX_NEGATIONS,
                PRESERVATION_ACTOR_TERMS,
                PRESERVATION_DETERMINERS,
                PRESERVATION_PREFIX_NEGATIONS,
                CLOSED_SAFETY_CONTROL_SCOPE_PREPOSITIONS,
                CLOSED_SAFETY_CONTROL_SCOPE_TERMS,
                SAFETY_CONTROL_TARGET_MODIFIERS,
                GATE_TARGETS,
                CLOSED_SAFETY_CONTROL_TARGET_TERMS,
            ] {
                visit(markers);
            }
        }
        BoundaryKind::Live => {
            for markers in [
                LIVE_ACTIONS,
                LIVE_CONTEXT,
                LIVE_CONTEXT_ALIASES,
                live_weak_context(),
                IMMEDIATE_CONTEXT,
                &["live changes", "라이브 변경"],
                &[
                    "channel",
                    "channels",
                    "permission",
                    "permissions",
                    "role",
                    "roles",
                ],
                CLOSED_BOUNDARY_ACTION_ADVERBS,
                CLOSED_THIRD_PERSON_BOUNDARY_ACTORS,
                GATE_CONTROL_TERMS,
                ACTION_NEGATION_MODIFIERS,
                ORDINARY_PREFIX_NEGATIONS,
                PRESERVATION_ACTOR_TERMS,
                PRESERVATION_DETERMINERS,
                PRESERVATION_PREFIX_NEGATIONS,
            ] {
                visit(markers);
            }
        }
        BoundaryKind::Secret => {
            for markers in [
                SECRET_ACTIONS,
                SECRET_TARGETS,
                UNPROTECTED_SECRET,
                SAFE_REDACTION,
                SECRET_DELIVERY_CONTEXT,
                CLOSED_BOUNDARY_ACTION_ADVERBS,
                CLOSED_SECRET_DISCLOSURE_ACTORS,
                CLOSED_THIRD_PERSON_BOUNDARY_ACTORS,
                GATE_CONTROL_TERMS,
                ACTION_NEGATION_MODIFIERS,
                ORDINARY_PREFIX_NEGATIONS,
                PRESERVATION_ACTOR_TERMS,
                PRESERVATION_DETERMINERS,
                PRESERVATION_PREFIX_NEGATIONS,
            ] {
                visit(markers);
            }
        }
    }
}

pub(super) fn cover_markers(characters: &[char], covered: &mut [bool], markers: &[&str]) {
    for marker in markers {
        let marker_length = marker.chars().count();
        if marker_length == 0 || marker_length > characters.len() {
            continue;
        }
        for start in 0..=characters.len().saturating_sub(marker_length) {
            let end = start.saturating_add(marker_length);
            if ascii_case_insensitive_marker_equal(&characters[start..end], marker)
                && char_marker_has_boundaries(characters, marker, start, end)
            {
                covered[start..end].fill(true);
            }
        }
    }
}

fn cover_fragments(characters: &[char], covered: &mut [bool], markers: &[&str]) {
    for marker in markers {
        let marker_length = marker.chars().count();
        if marker_length == 0 || marker_length > characters.len() {
            continue;
        }
        for start in 0..=characters.len().saturating_sub(marker_length) {
            let end = start.saturating_add(marker_length);
            if ascii_case_insensitive_marker_equal(&characters[start..end], marker) {
                covered[start..end].fill(true);
            }
        }
    }
}

fn cover_closed_action_adverbs(characters: &[char], covered: &mut [bool]) {
    let mut start = 0usize;
    while start < characters.len() {
        while start < characters.len() && !word_continuation(characters[start]) {
            start = start.saturating_add(1);
        }
        let mut end = start;
        while end < characters.len() && word_continuation(characters[end]) {
            end = end.saturating_add(1);
        }
        if start < end {
            let word = characters[start..end]
                .iter()
                .collect::<String>()
                .to_ascii_lowercase();
            if closed_boundary_action_adverb(&word) {
                covered[start..end].fill(true);
            }
        }
        start = end.saturating_add(1);
    }
}

fn ascii_case_insensitive_marker_equal(value: &[char], marker: &str) -> bool {
    let mut marker_characters = marker.chars();
    value.iter().all(|left| {
        marker_characters.next().is_some_and(|right| {
            *left == right
                || (left.is_ascii() && right.is_ascii() && left.eq_ignore_ascii_case(&right))
        })
    }) && marker_characters.next().is_none()
}

fn char_marker_has_boundaries(value: &[char], _marker: &str, start: usize, end: usize) -> bool {
    let left_valid = start == 0
        || !value[start - 1..start]
            .first()
            .is_some_and(|character| word_continuation(*character));
    let right_valid = !value
        .get(end)
        .is_some_and(|character| word_continuation(*character))
        || known_korean_char_suffix(&value[end..]);
    left_valid && right_valid
}

fn known_korean_char_suffix(value: &[char]) -> bool {
    [
        "가",
        "게",
        "고",
        "과",
        "기",
        "도",
        "된",
        "돼",
        "들",
        "를",
        "만",
        "면",
        "로",
        "에",
        "에서",
        "에게",
        "와",
        "으",
        "은",
        "을",
        "의",
        "이",
        "인",
        "지",
        "주",
        "줘",
        "주세요",
        "하",
        "해",
    ]
    .iter()
    .any(|suffix| {
        let length = suffix.chars().count();
        value.len() >= length && value[..length].iter().copied().eq(suffix.chars())
    })
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
