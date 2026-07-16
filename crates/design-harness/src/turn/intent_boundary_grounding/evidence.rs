use std::collections::BTreeSet;

#[cfg(test)]
use std::cell::Cell;

use super::super::intent_interpretation::IntentBoundaryRequestV2;
use super::classification::{
    boundary_action_is_effectively_preserved, closed_boundary_action_adverb,
    closed_gate_control_weakening, inherited_action_negation, live_weak_context,
    prefix_negates_action, secret_target_is_locally_safe, starts_with_secret_target_object,
    suffix_negates_action, BoundaryKind, UnitFacts, ACTION_NEGATION_MODIFIERS,
    ACTION_POLARITY_TOKEN_WINDOW, CLOSED_BOUNDARY_ACTION_ADVERBS,
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
use super::syntax::{
    ascii_case_insensitive_chars_equal, normalized_text, word_continuation, BoundaryUnit, TextSpan,
    UnitLink,
};

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct CanonicalWhitespaceMap {
    folded: String,
    characters: Vec<char>,
    canonical_to_original: Vec<TextSpan>,
    original_to_canonical: Vec<TextSpan>,
    byte_to_character: Vec<usize>,
    character_to_byte: Vec<usize>,
}

impl CanonicalWhitespaceMap {
    pub(super) fn from_source(source: &str) -> Self {
        let original = source.chars().collect::<Vec<_>>();
        let mut characters = Vec::with_capacity(original.len());
        let mut canonical_to_original = Vec::with_capacity(original.len());
        let mut original_to_canonical = vec![TextSpan { start: 0, end: 0 }; original.len()];
        let mut index = 0usize;
        while index < original.len() {
            if original[index].is_whitespace() {
                let start = index;
                while index < original.len() && original[index].is_whitespace() {
                    index = index.saturating_add(1);
                }
                if !characters.is_empty() && index < original.len() {
                    let canonical = characters.len();
                    characters.push(' ');
                    canonical_to_original.push(TextSpan { start, end: index });
                    for span in &mut original_to_canonical[start..index] {
                        *span = TextSpan {
                            start: canonical,
                            end: canonical.saturating_add(1),
                        };
                    }
                } else {
                    let canonical = characters.len();
                    for span in &mut original_to_canonical[start..index] {
                        *span = TextSpan {
                            start: canonical,
                            end: canonical,
                        };
                    }
                }
                continue;
            }
            let canonical = characters.len();
            characters.push(original[index]);
            canonical_to_original.push(TextSpan {
                start: index,
                end: index.saturating_add(1),
            });
            original_to_canonical[index] = TextSpan {
                start: canonical,
                end: canonical.saturating_add(1),
            };
            index = index.saturating_add(1);
        }
        let text = characters.iter().collect::<String>();
        let folded = text.to_ascii_lowercase();
        let mut character_to_byte = text
            .char_indices()
            .map(|(byte, _)| byte)
            .collect::<Vec<_>>();
        character_to_byte.push(text.len());
        let mut byte_to_character = vec![usize::MAX; text.len().saturating_add(1)];
        for (character, byte) in character_to_byte.iter().copied().enumerate() {
            byte_to_character[byte] = character;
        }
        Self {
            folded,
            characters,
            canonical_to_original,
            original_to_canonical,
            byte_to_character,
            character_to_byte,
        }
    }

    fn canonical_span(&self, byte_start: usize, byte_end: usize) -> Option<TextSpan> {
        Some(TextSpan {
            start: *self.byte_to_character.get(byte_start)?,
            end: *self.byte_to_character.get(byte_end)?,
        })
        .filter(|span| span.start != usize::MAX && span.end != usize::MAX)
    }

    fn original_span(&self, canonical: TextSpan) -> Option<TextSpan> {
        if canonical.start >= canonical.end {
            return None;
        }
        let start = self.canonical_to_original.get(canonical.start)?.start;
        let end = self
            .canonical_to_original
            .get(canonical.end.saturating_sub(1))?
            .end;
        let original = TextSpan { start, end };
        let round_trip_start = self.original_to_canonical.get(original.start)?.start;
        let round_trip_end = self
            .original_to_canonical
            .get(original.end.saturating_sub(1))?
            .end;
        (round_trip_start == canonical.start && round_trip_end == canonical.end).then_some(original)
    }

    fn is_bounded(&self, candidate: &[char], span: TextSpan) -> bool {
        let left_valid = !candidate
            .first()
            .is_some_and(|value| word_continuation(*value))
            || !span
                .start
                .checked_sub(1)
                .and_then(|index| self.characters.get(index))
                .is_some_and(|value| word_continuation(*value));
        let right_valid = !candidate
            .last()
            .is_some_and(|value| word_continuation(*value))
            || !self
                .characters
                .get(span.end)
                .is_some_and(|value| word_continuation(*value))
            || self
                .character_to_byte
                .get(span.end)
                .is_some_and(|byte| known_korean_suffix_boundary(&self.folded[*byte..]));
        left_valid && right_valid
    }

    fn is_visible(&self, span: TextSpan, visible: &[char], candidate: &[char]) -> bool {
        self.canonical_to_original[span.start..span.end]
            .iter()
            .zip(candidate)
            .all(|(original, candidate)| {
                visible
                    .get(original.start..original.end)
                    .is_some_and(|value| {
                        if candidate.is_whitespace() {
                            value.iter().all(|character| character.is_whitespace())
                        } else {
                            ascii_case_insensitive_chars_equal(value, &[*candidate])
                        }
                    })
            })
    }
}

#[cfg(test)]
thread_local! {
    static COMPONENT_ONLY_EVALUATIONS: Cell<usize> = const { Cell::new(0) };
    static POLARITY_PREFIX_CHAR_VISITS: Cell<usize> = const { Cell::new(0) };
    static EVIDENCE_SPAN_LINEAR_WORK: Cell<usize> = const { Cell::new(0) };
    static MERGED_SPAN_LOCAL_WORK: Cell<usize> = const { Cell::new(0) };
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct BoundaryEvidenceGroup {
    kind: BoundaryKind,
    coverage_spans: Vec<TextSpan>,
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
    let raw_facts = units.iter().map(UnitFacts::for_unit).collect::<Vec<_>>();
    let facts = units
        .iter()
        .zip(&raw_facts)
        .map(|(unit, facts)| {
            if unit.hypothetical {
                UnitFacts::default()
            } else {
                facts.clone()
            }
        })
        .collect::<Vec<_>>();
    let mut groups = Vec::new();
    for kind in [BoundaryKind::Gate, BoundaryKind::Live, BoundaryKind::Secret] {
        let direct_seed = facts.iter().any(|unit| unit.is_seed(kind));
        let distributed_live_seed = kind == BoundaryKind::Live
            && (0..units.len().saturating_sub(1)).any(|index| {
                !units[index].hypothetical
                    && !units[index + 1].hypothetical
                    && !matches!(
                        units[index + 1].link,
                        UnitLink::Alternative | UnitLink::NegativeAlternative
                    )
                    && combine_facts(&facts[index], &facts[index + 1]).is_seed(kind)
            });
        let has_distributed_secret_seed = kind == BoundaryKind::Secret
            && (0..units.len().saturating_sub(1))
                .any(|index| distributed_secret_seed(units, &facts, &raw_facts, index));
        if !direct_seed && !distributed_live_seed && !has_distributed_secret_seed {
            continue;
        }
        let component_only = units
            .iter()
            .enumerate()
            .map(|(index, unit)| {
                !unit.hypothetical
                    && unit_is_boundary_component_only(
                        &unit.text,
                        kind,
                        facts[index].has_evidence(kind),
                    )
                    && !inherited_action_negation(unit, kind)
                    && !boundary_action_is_effectively_preserved(&unit.text, kind)
            })
            .collect::<Vec<_>>();
        let mut expanded_starts = (0..units.len()).collect::<Vec<_>>();
        for index in 1..units.len() {
            let previous = index - 1;
            if matches!(units[index].link, UnitLink::Additive | UnitLink::Sequential)
                && component_only[previous]
                && facts[previous].has_evidence(kind)
            {
                expanded_starts[index] = expanded_starts[previous];
            }
        }
        let mut expanded_ends = (0..units.len()).collect::<Vec<_>>();
        for index in (0..units.len().saturating_sub(1)).rev() {
            let next = index + 1;
            if (matches!(units[next].link, UnitLink::Additive | UnitLink::Sequential)
                || (kind == BoundaryKind::Gate && units[next].link == UnitLink::Scope))
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
                let combined = combine_facts(&facts[index], &facts[index + 1]);
                let sequential_role_bridge = units[index + 1].link == UnitLink::Sequential
                    && !facts[index].is_seed(kind)
                    && !facts[index + 1].is_seed(kind)
                    && combined.is_seed(kind);
                if units[index].hypothetical
                    || units[index + 1].hypothetical
                    || matches!(
                        units[index + 1].link,
                        UnitLink::Alternative | UnitLink::NegativeAlternative
                    )
                    || (!sequential_role_bridge
                        && (!component_only[index] || !component_only[index + 1]))
                {
                    continue;
                }
                if combined.is_seed(kind) {
                    intervals.insert((expanded_starts[index], expanded_ends[index + 1]));
                }
            }
        }
        if kind == BoundaryKind::Secret {
            for index in 0..units.len().saturating_sub(1) {
                if distributed_secret_seed(units, &facts, &raw_facts, index) {
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

pub(super) fn cross_sentence_role_bridge(left: &BoundaryUnit, right: &BoundaryUnit) -> bool {
    if left.hypothetical || right.hypothetical {
        return false;
    }
    let left_facts = UnitFacts::for_unit(left);
    let right_facts = UnitFacts::for_unit(right);
    let live = !left_facts.is_seed(BoundaryKind::Live)
        && !right_facts.is_seed(BoundaryKind::Live)
        && combine_facts(&left_facts, &right_facts).is_seed(BoundaryKind::Live)
        && (right_facts.live_action || closed_live_coreferential_continuation(&right.text));
    let secret = left_facts.secret_unsafe_target
        && right_facts.secret_action
        && right_facts.secret_delivery
        && starts_with_secret_coreferential_disclosure(&right.text);
    live || secret
}

fn closed_live_coreferential_continuation(value: &str) -> bool {
    ["do it", "do that", "carry it out", "carry that out"]
        .iter()
        .any(|prefix| {
            value.strip_prefix(prefix).is_some_and(|remaining| {
                remaining
                    .chars()
                    .next()
                    .is_none_or(|character| !word_continuation(character))
            })
        })
}

fn distributed_secret_seed(
    units: &[BoundaryUnit],
    facts: &[UnitFacts],
    raw_facts: &[UnitFacts],
    index: usize,
) -> bool {
    let Some(left_unit) = units.get(index) else {
        return false;
    };
    let Some(right_unit) = units.get(index.saturating_add(1)) else {
        return false;
    };
    let action_to_target = !left_unit.hypothetical
        && !right_unit.hypothetical
        && right_unit.link == UnitLink::Additive
        && facts[index].secret_action
        && facts[index.saturating_add(1)].secret_unsafe_target
        && starts_with_secret_target_object(&right_unit.text);
    let antecedent_is_authoritative = !left_unit.hypothetical || left_unit.non_authoritative_event;
    let target_to_action = antecedent_is_authoritative
        && !right_unit.hypothetical
        && matches!(
            right_unit.link,
            UnitLink::Additive | UnitLink::Sequential | UnitLink::Scope
        )
        && raw_facts[index].secret_unsafe_target
        && facts[index.saturating_add(1)].secret_action
        && facts[index.saturating_add(1)].secret_delivery
        && starts_with_secret_coreferential_disclosure(&right_unit.text);
    let metadata_to_value = !left_unit.hypothetical
        && !right_unit.hypothetical
        && right_unit.link == UnitLink::Additive
        && facts[index].secret_action
        && facts[index].secret_target
        && facts[index.saturating_add(1)].secret_delivery
        && starts_with_secret_value_content(&right_unit.text);
    action_to_target || target_to_action || metadata_to_value
}

fn starts_with_secret_value_content(value: &str) -> bool {
    let value_references = [
        "actual value",
        "content",
        "its actual value",
        "its raw value",
        "its value",
        "raw value",
        "secret content",
        "their values",
        "value",
        "values",
    ];
    let starts_with_reference = |value: &str| {
        value_references.iter().any(|role| {
            value.strip_prefix(role).is_some_and(|remaining| {
                remaining
                    .chars()
                    .next()
                    .is_none_or(|character| !word_continuation(character))
            })
        })
    };
    if starts_with_reference(value) {
        return true;
    }
    SECRET_ACTIONS.iter().any(|action| {
        value.strip_prefix(action).is_some_and(|remaining| {
            remaining
                .chars()
                .next()
                .is_some_and(|character| !word_continuation(character))
                && starts_with_reference(remaining.trim_start())
        })
    })
}

fn starts_with_secret_coreferential_disclosure(value: &str) -> bool {
    let action = SECRET_ACTIONS.iter().find_map(|marker| {
        value.match_indices(marker).find_map(|(start, matched)| {
            let end = start.saturating_add(matched.len());
            (start <= 24).then_some(end)
        })
    });
    let Some(action_end) = action else {
        return false;
    };
    let tail = value[action_end..].trim_start();
    [
        "it",
        "them",
        "its value",
        "their value",
        "the value",
        "that value",
    ]
    .iter()
    .any(|object| {
        tail.strip_prefix(object).is_some_and(|remaining| {
            remaining
                .chars()
                .next()
                .is_none_or(|character| !word_continuation(character))
        })
    })
}

pub(super) fn coordinated_groups_cover_candidate(
    groups: &[BoundaryEvidenceGroup],
    candidate: TextSpan,
    visible: &[char],
) -> bool {
    [BoundaryKind::Gate, BoundaryKind::Live, BoundaryKind::Secret]
        .into_iter()
        .any(|kind| {
            let matching = groups
                .iter()
                .filter(|group| {
                    group.kind == kind
                        && group
                            .positive_role_spans
                            .iter()
                            .any(|span| candidate.start < span.end && candidate.end > span.start)
                })
                .collect::<Vec<_>>();
            if matching.len() < 2 {
                return false;
            }
            let mut coverage_spans = matching
                .iter()
                .flat_map(|group| group.coverage_spans.iter().copied())
                .collect::<Vec<_>>();
            coverage_spans.extend(marker_occurrence_spans(visible, candidate, &["but"], false));
            let positive_role_spans = matching
                .iter()
                .flat_map(|group| group.positive_role_spans.iter().copied())
                .collect();
            group_covers_candidate(
                &BoundaryEvidenceGroup {
                    kind,
                    coverage_spans: merged_spans(coverage_spans),
                    positive_role_spans,
                },
                candidate,
                visible,
            )
        })
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
        if index > start
            && (matches!(units[index].link, UnitLink::Additive | UnitLink::Sequential)
                || (matches!(kind, BoundaryKind::Gate | BoundaryKind::Secret)
                    && units[index].link == UnitLink::Scope))
        {
            joiner_spans.push(TextSpan {
                start: units[index - 1].span.end,
                end: units[index].span.start,
            });
        }
        member_coverage_spans.extend(boundary_coverage_spans(visible, units[index].span, kind));
        if kind == BoundaryKind::Gate && closed_gate_control_weakening(&units[index].text) {
            member_coverage_spans.push(units[index].span);
        }
        positive_role_spans.extend(positive_role_spans_for_unit(visible, &units[index], kind));
    }
    member_coverage_spans.extend(joiner_spans);
    BoundaryEvidenceGroup {
        kind,
        coverage_spans: merged_spans(member_coverage_spans),
        positive_role_spans,
    }
}

fn merged_spans(mut spans: Vec<TextSpan>) -> Vec<TextSpan> {
    if spans.is_empty() {
        return spans;
    }
    let first = spans.iter().map(|span| span.start).min().unwrap_or(0);
    let furthest = spans.iter().map(|span| span.end).max().unwrap_or(0);
    let mut furthest_end_at_start = vec![None; furthest.saturating_sub(first).saturating_add(1)];
    for span in spans.drain(..) {
        #[cfg(test)]
        {
            EVIDENCE_SPAN_LINEAR_WORK.with(|work| work.set(work.get().saturating_add(1)));
            MERGED_SPAN_LOCAL_WORK.with(|work| work.set(work.get().saturating_add(1)));
        }
        let slot = &mut furthest_end_at_start[span.start.saturating_sub(first)];
        *slot = Some(slot.map_or(span.end, |current: usize| current.max(span.end)));
    }
    let mut merged: Vec<TextSpan> = Vec::new();
    for (relative_start, end) in furthest_end_at_start.into_iter().enumerate() {
        #[cfg(test)]
        {
            EVIDENCE_SPAN_LINEAR_WORK.with(|work| work.set(work.get().saturating_add(1)));
            MERGED_SPAN_LOCAL_WORK.with(|work| work.set(work.get().saturating_add(1)));
        }
        let Some(end) = end else {
            continue;
        };
        let start = first.saturating_add(relative_start);
        let span = TextSpan { start, end };
        if let Some(previous) = merged.last_mut() {
            if span.start <= previous.end {
                previous.end = previous.end.max(span.end);
                continue;
            }
        }
        merged.push(span);
    }
    merged
}

fn group_covers_candidate(
    group: &BoundaryEvidenceGroup,
    candidate: TextSpan,
    visible: &[char],
) -> bool {
    let mut has_content = false;
    let mut coverage = 0usize;
    for (offset, character) in visible[candidate.start..candidate.end].iter().enumerate() {
        if !word_continuation(*character) {
            continue;
        }
        has_content = true;
        let index = candidate.start.saturating_add(offset);
        while group
            .coverage_spans
            .get(coverage)
            .is_some_and(|span| index >= span.end)
        {
            coverage = coverage.saturating_add(1);
        }
        if !group
            .coverage_spans
            .get(coverage)
            .is_some_and(|span| index >= span.start && index < span.end)
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
        secret_unsafe_target: left.secret_unsafe_target || right.secret_unsafe_target,
        secret_delivery: left.secret_delivery || right.secret_delivery,
        secret_unprotected: left.secret_unprotected || right.secret_unprotected,
    }
}

pub(super) fn unique_visible_bounded_span(
    canonical_source: &CanonicalWhitespaceMap,
    visible: &[char],
    candidate: &str,
) -> Option<TextSpan> {
    let normalized_candidate = normalized_text(candidate);
    if normalized_candidate.is_empty() {
        return None;
    }
    let folded_candidate = normalized_candidate.to_ascii_lowercase();
    let candidate_characters = normalized_candidate.chars().collect::<Vec<_>>();
    let mut occurrence = None;
    for (byte_start, _) in canonical_source.folded.match_indices(&folded_candidate) {
        let byte_end = byte_start.saturating_add(folded_candidate.len());
        let Some(canonical_span) = canonical_source.canonical_span(byte_start, byte_end) else {
            continue;
        };
        if !canonical_source.is_bounded(&candidate_characters, canonical_span)
            || !canonical_source.is_visible(canonical_span, visible, &candidate_characters)
        {
            continue;
        }
        let Some(original_span) = canonical_source.original_span(canonical_span) else {
            continue;
        };
        if occurrence.is_some() {
            return None;
        }
        occurrence = Some(original_span);
    }
    occurrence
}

fn known_korean_suffix_boundary(value: &str) -> bool {
    ["해주세요", "해줘", "하도록", "하게", "하고", "하며"]
        .iter()
        .any(|suffix| value.starts_with(suffix))
}

fn unit_is_boundary_component_only(value: &str, kind: BoundaryKind, has_evidence: bool) -> bool {
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

fn boundary_coverage_spans(visible: &[char], span: TextSpan, kind: BoundaryKind) -> Vec<TextSpan> {
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

fn unsafe_secret_target_spans(visible: &[char], span: TextSpan) -> Vec<TextSpan> {
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

fn marker_occurrence_spans(
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

fn cover_markers(characters: &[char], covered: &mut [bool], markers: &[&str]) {
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

    fn polarity_prefix_visits(repetitions: usize) -> usize {
        let human = vec!["skip validation"; repetitions].join(" ");
        let visible = human.chars().collect::<Vec<_>>();
        POLARITY_PREFIX_CHAR_VISITS.with(|visits| visits.set(0));
        let spans = marker_occurrence_spans(
            &visible,
            TextSpan {
                start: 0,
                end: visible.len(),
            },
            &["skip"],
            true,
        );
        assert_eq!(spans.len(), repetitions);
        POLARITY_PREFIX_CHAR_VISITS.with(Cell::get)
    }

    #[test]
    fn component_classification_work_scales_linearly() {
        let small = component_evaluations(128);
        let large = component_evaluations(256);
        assert_eq!(small, 128);
        assert_eq!(large, 256);
        assert_eq!(large, small * 2);
    }

    #[test]
    fn action_polarity_prefix_work_uses_a_fixed_token_window() {
        let small = polarity_prefix_visits(128);
        let large = polarity_prefix_visits(256);
        assert!(
            large <= small.saturating_mul(2).saturating_add(2048),
            "small {small} large {large}"
        );

        for (controls, owned) in [(15, false), (16, true), (17, true)] {
            let human = format!("{}skip validation", "never ".repeat(controls));
            let visible = human.chars().collect::<Vec<_>>();
            let spans = marker_occurrence_spans(
                &visible,
                TextSpan {
                    start: 0,
                    end: visible.len(),
                },
                &["skip"],
                true,
            );
            assert_eq!(!spans.is_empty(), owned, "control count {controls}");
        }
    }

    #[test]
    fn marker_scans_preserve_ascii_folding_unicode_and_boundaries() {
        let human = "SKIP 검증 skipx Ä ä";
        let visible = human.chars().collect::<Vec<_>>();
        let span = TextSpan {
            start: 0,
            end: visible.len(),
        };
        let expected = vec!["SKIP", "검증", "Ä"];

        let spans = marker_occurrence_spans(&visible, span, &["skip", "검증", "Ä"], false);
        let matched = spans
            .iter()
            .map(|span| visible[span.start..span.end].iter().collect::<String>())
            .collect::<Vec<_>>();
        assert_eq!(matched, expected);

        let mut covered = vec![false; visible.len()];
        cover_markers(&visible, &mut covered, &["skip", "검증", "Ä"]);
        let covered = covered_runs(&covered, 0)
            .iter()
            .map(|span| visible[span.start..span.end].iter().collect::<String>())
            .collect::<Vec<_>>();
        assert_eq!(covered, expected);
    }

    #[test]
    fn secret_evidence_spans_measure_the_full_linear_path() {
        fn work(repeated: usize) -> usize {
            let human = "Publish the API token publicly. ".repeat(repeated);
            let visible = human.chars().collect::<Vec<_>>();
            EVIDENCE_SPAN_LINEAR_WORK.with(|steps| steps.set(0));
            let spans = unsafe_secret_target_spans(
                &visible,
                TextSpan {
                    start: 0,
                    end: visible.len(),
                },
            );
            assert_eq!(spans.len(), repeated);
            EVIDENCE_SPAN_LINEAR_WORK.with(Cell::get)
        }

        let small = work(256);
        let large = work(512);
        let fixed_marker_cost = SECRET_TARGETS
            .iter()
            .map(|marker| marker.chars().count())
            .sum::<usize>();
        assert!(
            large <= small.saturating_mul(2).saturating_add(fixed_marker_cost),
            "small {small} large {large}"
        );
    }

    #[test]
    fn merged_evidence_spans_measure_the_full_linear_path() {
        fn work(repeated: usize) -> usize {
            let spans = (0..repeated)
                .map(|index| TextSpan {
                    start: index.saturating_mul(2),
                    end: index.saturating_mul(2).saturating_add(1),
                })
                .collect::<Vec<_>>();
            EVIDENCE_SPAN_LINEAR_WORK.with(|steps| steps.set(0));
            assert_eq!(merged_spans(spans).len(), repeated);
            EVIDENCE_SPAN_LINEAR_WORK.with(Cell::get)
        }

        let small = work(2_048);
        let large = work(4_096);
        assert!(large <= small.saturating_mul(2).saturating_add(2));
    }

    #[test]
    fn separated_boundary_groups_use_group_local_merge_ranges() {
        fn work(repeated: usize) -> usize {
            let human = "Skip validation. ".repeat(repeated);
            MERGED_SPAN_LOCAL_WORK.with(|steps| steps.set(0));
            let analysis = super::super::SafetyBoundaryAnalysis::analyze(&human);
            assert_eq!(
                analysis.requests(),
                &[IntentBoundaryRequestV2::BypassValidationPreviewApproval]
            );
            MERGED_SPAN_LOCAL_WORK.with(Cell::get)
        }

        let small = work(1_024);
        let large = work(2_048);
        assert!(small > 0);
        assert!(large <= small.saturating_mul(2).saturating_add(32));
    }
}
